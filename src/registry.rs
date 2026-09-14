//! Session registry: `~/.claude/sessions/<pid>.json`, one file per running
//! Claude Code process. Read-only.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Whether the session's main loop is currently busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Busy,
    Idle,
    /// Any value cctop does not know; kept verbatim in [`Session::status_raw`].
    Other,
}

/// One registry entry. Unknown fields are ignored so newer Claude Code
/// versions never break parsing.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub pid: u32,
    pub session_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "status", default)]
    pub status_raw: String,
    /// Unix epoch milliseconds.
    #[serde(default)]
    pub started_at: u64,
    /// Unix epoch milliseconds.
    #[serde(default)]
    pub updated_at: u64,
    /// Unix epoch milliseconds when `status` last changed: how long the
    /// session has been idle or busy.
    #[serde(default)]
    pub status_updated_at: u64,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub messaging_socket_path: Option<PathBuf>,
    /// Registry file this entry was read from.
    #[serde(skip)]
    pub path: PathBuf,
}

impl Session {
    pub fn status(&self) -> Status {
        match self.status_raw.as_str() {
            "busy" => Status::Busy,
            "idle" => Status::Idle,
            _ => Status::Other,
        }
    }

    /// True when a process with this pid exists (`kill(pid, 0)` semantics).
    /// `EPERM` counts as alive: the process is there, we just may not signal it.
    pub fn is_alive(&self) -> bool {
        pid_alive(self.pid)
    }
}

/// True when a process with `pid` exists (see [`Session::is_alive`]).
pub fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: kill with signal 0 performs no action; it only checks existence
    // and permissions, and has no memory-safety preconditions.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Default registry directory: `~/.claude/sessions`.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude").join("sessions"))
}

/// Read every `*.json` file in `dir`. Malformed files are skipped with a
/// warning on stderr; a missing directory yields an empty list.
pub fn list(dir: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| match read_one(&p) {
            Ok(s) => Some(s),
            Err(err) => {
                eprintln!("cctop: skipping {}: {err}", p.display());
                None
            }
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    sessions
}

fn read_one(path: &Path) -> anyhow::Result<Session> {
    let text = std::fs::read_to_string(path)?;
    let mut s: Session = serde_json::from_str(&text)?;
    s.path = path.to_path_buf();
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions")
    }

    #[test]
    fn lists_valid_files_and_skips_malformed() {
        let sessions = list(&fixtures());
        let mut names: Vec<_> = sessions.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["cctop-46", "finsight-3", "old-1"]);
    }

    #[test]
    fn newest_first() {
        let sessions = list(&fixtures());
        assert_eq!(sessions[0].name, "cctop-46");
        assert_eq!(sessions.last().unwrap().name, "old-1");
    }

    #[test]
    fn parses_fields_and_status() {
        let sessions = list(&fixtures());
        let busy = sessions.iter().find(|s| s.name == "cctop-46").unwrap();
        assert_eq!(busy.pid, 86143);
        assert_eq!(busy.session_id, "12f423aa-fbba-43f3-9f4b-d27cd5a78070");
        assert_eq!(busy.cwd, PathBuf::from("/Users/tom/code/cctop"));
        assert_eq!(busy.status(), Status::Busy);
        assert_eq!(busy.version, "2.1.269");
        assert_eq!(busy.kind, "interactive");
        assert_eq!(
            busy.messaging_socket_path.as_deref(),
            Some(Path::new("/tmp/cc-socks/86143.sock"))
        );
        assert_eq!(busy.started_at, 1789151862854);
        let idle = sessions.iter().find(|s| s.name == "finsight-3").unwrap();
        assert_eq!(idle.status(), Status::Idle);
        let old = sessions.iter().find(|s| s.name == "old-1").unwrap();
        assert!(old.messaging_socket_path.is_none());
    }

    #[test]
    fn dead_pid_is_not_alive() {
        let sessions = list(&fixtures());
        let old = sessions.iter().find(|s| s.name == "old-1").unwrap();
        assert!(!old.is_alive(), "pid 4194000 must not exist");
    }

    #[test]
    fn own_pid_is_alive() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn missing_dir_is_empty() {
        assert!(list(Path::new("/nonexistent/cctop-sessions")).is_empty());
    }
}
