//! Which session should cctop attach to? Resolution order:
//!
//! 1. `--session <id|name|pid>`
//! 2. `--cwd <path>`
//! 3. `$CLAUDE_CODE_SESSION_ID` (what Claude Code exports to its child
//!    processes; `$CLAUDE_SESSION_ID` is accepted too)
//! 4. a tmux pane in the current window whose TTY belongs to a session pid
//! 5. inside zellij: the newest busy session whose cwd is `$PWD`
//! 6. the newest busy session whose cwd is `$PWD`
//!
//! Dead registry entries (pid gone) are ignored unless named explicitly.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::registry::{self, Session, Status};

/// What the user and environment told us.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub session: Option<String>,
    pub cwd: Option<PathBuf>,
    pub claude_session_id: Option<String>,
    pub in_tmux: bool,
    pub in_zellij: bool,
    pub pwd: PathBuf,
}

impl Query {
    /// Build from the process environment plus explicit flags.
    pub fn from_env(session: Option<String>, cwd: Option<PathBuf>) -> Self {
        Self {
            session,
            cwd,
            claude_session_id: ["CLAUDE_CODE_SESSION_ID", "CLAUDE_SESSION_ID"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty())),
            in_tmux: std::env::var_os("TMUX").is_some(),
            in_zellij: std::env::var_os("ZELLIJ").is_some(),
            pwd: std::env::current_dir().unwrap_or_default(),
        }
    }
}

/// Terminal-multiplexer and process lookups, behind a trait so tests can
/// inject answers.
pub trait Env {
    /// `(pane_tty, pane_pid)` for every pane in the current tmux window.
    fn tmux_panes(&self) -> Vec<(String, u32)>;
    /// Controlling TTY of `pid`, e.g. `ttys003`.
    fn tty_of(&self, pid: u32) -> Option<String>;
    fn is_alive(&self, s: &Session) -> bool;
}

/// Real implementation shelling out to `tmux` and `ps`.
pub struct SystemEnv;

impl Env for SystemEnv {
    fn tmux_panes(&self) -> Vec<(String, u32)> {
        let Ok(out) = Command::new("tmux")
            .args(["list-panes", "-F", "#{pane_tty} #{pane_pid}"])
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                Some((it.next()?.to_string(), it.next()?.parse().ok()?))
            })
            .collect()
    }

    fn tty_of(&self, pid: u32) -> Option<String> {
        let out = Command::new("ps")
            .args(["-o", "tty=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let tty = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!tty.is_empty() && tty != "??").then_some(tty)
    }

    fn is_alive(&self, s: &Session) -> bool {
        s.is_alive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("no Claude Code session found (try --session or --wait)")]
    NotFound,
    #[error("no session matches {0:?}")]
    NoMatch(String),
}

impl DiscoverError {
    /// Process exit code for the CLI.
    pub const EXIT_CODE: i32 = 2;
}

/// Pick the session for `q` from `sessions`.
pub fn resolve(sessions: &[Session], q: &Query, env: &dyn Env) -> Result<Session, DiscoverError> {
    if let Some(key) = &q.session {
        return sessions
            .iter()
            .find(|s| matches_key(s, key))
            .cloned()
            .ok_or_else(|| DiscoverError::NoMatch(key.clone()));
    }
    let live: Vec<&Session> = sessions.iter().filter(|s| env.is_alive(s)).collect();

    if let Some(cwd) = &q.cwd {
        return best(live.iter().copied().filter(|s| same_dir(&s.cwd, cwd)))
            .ok_or(DiscoverError::NotFound);
    }
    if let Some(id) = &q.claude_session_id {
        if let Some(s) = live.iter().find(|s| s.session_id == *id) {
            return Ok((*s).clone());
        }
    }
    if q.in_tmux {
        let ttys: Vec<String> = env
            .tmux_panes()
            .into_iter()
            .map(|(tty, _)| norm_tty(&tty))
            .collect();
        if !ttys.is_empty() {
            let on_pane = live.iter().copied().filter(|s| {
                env.tty_of(s.pid)
                    .map(|t| ttys.contains(&norm_tty(&t)))
                    .unwrap_or(false)
            });
            if let Some(s) = best(on_pane) {
                return Ok(s);
            }
        }
    }
    // zellij and the plain fallback share the same rule: newest busy in $PWD,
    // then newest anything in $PWD.
    best(live.iter().copied().filter(|s| same_dir(&s.cwd, &q.pwd))).ok_or(DiscoverError::NotFound)
}

/// Resolve with the real registry and environment, optionally polling until
/// a session appears.
pub fn resolve_system(q: &Query, wait: bool) -> Result<Session, DiscoverError> {
    let dir = registry::default_dir().unwrap_or_default();
    loop {
        match resolve(&registry::list(&dir), q, &SystemEnv) {
            Ok(s) => return Ok(s),
            Err(DiscoverError::NotFound) if wait => std::thread::sleep(Duration::from_secs(1)),
            Err(e) => return Err(e),
        }
    }
}

/// Busy beats idle; newer beats older.
fn best<'a>(it: impl Iterator<Item = &'a Session>) -> Option<Session> {
    it.max_by_key(|s| (s.status() == Status::Busy, s.updated_at))
        .cloned()
}

fn matches_key(s: &Session, key: &str) -> bool {
    s.session_id == key
        || s.name == key
        || key.parse::<u32>().is_ok_and(|p| p == s.pid)
        || (key.len() >= 8 && s.session_id.starts_with(key))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// `/dev/ttys003` and `ttys003` name the same terminal.
fn norm_tty(t: &str) -> String {
    t.trim().trim_start_matches("/dev/").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake {
        panes: Vec<(String, u32)>,
        ttys: HashMap<u32, String>,
        dead: Vec<u32>,
    }

    impl Env for Fake {
        fn tmux_panes(&self) -> Vec<(String, u32)> {
            self.panes.clone()
        }
        fn tty_of(&self, pid: u32) -> Option<String> {
            self.ttys.get(&pid).cloned()
        }
        fn is_alive(&self, s: &Session) -> bool {
            !self.dead.contains(&s.pid)
        }
    }

    fn fake() -> Fake {
        Fake {
            panes: vec![("/dev/ttys003".into(), 500), ("/dev/ttys004".into(), 501)],
            ttys: HashMap::from([(86143, "ttys004".into()), (40758, "ttys010".into())]),
            dead: vec![4194000],
        }
    }

    fn sessions() -> Vec<Session> {
        registry::list(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions"))
    }

    fn q() -> Query {
        Query {
            pwd: PathBuf::from("/Users/tom/code/finsight"),
            ..Default::default()
        }
    }

    #[test]
    fn explicit_session_by_id_name_pid_and_prefix() {
        let s = sessions();
        for key in [
            "12f423aa-fbba-43f3-9f4b-d27cd5a78070",
            "cctop-46",
            "86143",
            "12f423aa",
        ] {
            let q = Query {
                session: Some(key.into()),
                ..q()
            };
            assert_eq!(
                resolve(&s, &q, &fake()).unwrap().name,
                "cctop-46",
                "key {key}"
            );
        }
    }

    #[test]
    fn explicit_session_may_be_dead() {
        let q = Query {
            session: Some("old-1".into()),
            ..q()
        };
        assert_eq!(resolve(&sessions(), &q, &fake()).unwrap().pid, 4194000);
    }

    #[test]
    fn explicit_session_no_match() {
        let q = Query {
            session: Some("nope".into()),
            ..q()
        };
        assert!(matches!(
            resolve(&sessions(), &q, &fake()),
            Err(DiscoverError::NoMatch(_))
        ));
    }

    #[test]
    fn cwd_flag_beats_env_and_tmux() {
        let q = Query {
            cwd: Some("/Users/tom/code/finsight".into()),
            claude_session_id: Some("12f423aa-fbba-43f3-9f4b-d27cd5a78070".into()),
            in_tmux: true,
            ..q()
        };
        assert_eq!(
            resolve(&sessions(), &q, &fake()).unwrap().name,
            "finsight-3"
        );
    }

    #[test]
    fn claude_session_id_env_beats_tmux() {
        let q = Query {
            claude_session_id: Some("0f118e31-84b7-4a96-ad68-3508a705e468".into()),
            in_tmux: true,
            ..q()
        };
        assert_eq!(
            resolve(&sessions(), &q, &fake()).unwrap().name,
            "finsight-3"
        );
    }

    #[test]
    fn tmux_pane_tty_match() {
        let q = Query {
            in_tmux: true,
            pwd: "/nowhere".into(),
            ..q()
        };
        // 86143 sits on ttys004, which is a pane; 40758 is on ttys010, not a pane.
        assert_eq!(resolve(&sessions(), &q, &fake()).unwrap().name, "cctop-46");
    }

    #[test]
    fn tmux_without_match_falls_back_to_pwd() {
        let mut f = fake();
        f.ttys.clear();
        let q = Query {
            in_tmux: true,
            ..q()
        };
        assert_eq!(resolve(&sessions(), &q, &f).unwrap().name, "finsight-3");
    }

    #[test]
    fn zellij_uses_pwd() {
        let q = Query {
            in_zellij: true,
            pwd: "/Users/tom/code/cctop".into(),
            ..q()
        };
        assert_eq!(resolve(&sessions(), &q, &fake()).unwrap().name, "cctop-46");
    }

    #[test]
    fn dead_sessions_are_ignored_in_fallbacks() {
        let q = Query {
            pwd: "/Users/tom/code/old".into(),
            ..q()
        };
        assert!(matches!(
            resolve(&sessions(), &q, &fake()),
            Err(DiscoverError::NotFound)
        ));
    }

    #[test]
    fn busy_beats_newer_idle() {
        let mut s = sessions();
        // Make the idle finsight session newer than the busy cctop one, same cwd.
        for x in &mut s {
            if x.name == "finsight-3" {
                x.cwd = "/Users/tom/code/cctop".into();
                x.updated_at = u64::MAX;
            }
        }
        let q = Query {
            pwd: "/Users/tom/code/cctop".into(),
            ..q()
        };
        assert_eq!(resolve(&s, &q, &fake()).unwrap().name, "cctop-46");
    }

    #[test]
    fn not_found_message_and_exit_code() {
        assert_eq!(
            DiscoverError::NotFound.to_string(),
            "no Claude Code session found (try --session or --wait)"
        );
        assert_eq!(DiscoverError::EXIT_CODE, 2);
    }
}
