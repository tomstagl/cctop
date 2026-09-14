//! Attach the app to a session: reset the per-session state, rebuild every
//! collector (tailer, agents, hooks, status, process tree, git, prefix scan)
//! and register the clock-driven hooks. Used at start-up and by the session
//! picker, so switching tears the previous collectors down by dropping them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::app::{App, LineSource};
use crate::ui::state::{SessionInfo, State};

/// Build a fresh state for `info`, keeping UI preferences from `prev`.
fn fresh_state(prev: &State, info: SessionInfo) -> State {
    let mut s = State::new(crate::metrics::Pricing::load());
    s.session = info;
    s.now_ms = crate::app::now_ms();
    s.hidden = prev.hidden.clone();
    s.tokens_include_agents = prev.tokens_include_agents;
    s.baseline = prev.baseline.clone();
    s.theme = prev.theme.clone();
    s.other_live_sessions = prev.other_live_sessions;
    if let Some(pid) = s.session.pid {
        if let Some(dir) = crate::registry::default_dir() {
            s.messaging_socket = crate::registry::list(&dir)
                .iter()
                .find(|x| x.pid == pid)
                .and_then(SessionInfo::socket_of);
        }
    }
    s
}

/// Attach `app` to the session behind `transcript`. `live` wires the
/// followers and process sampling; headless callers pass `false` and feed
/// the transcript themselves.
pub fn attach(app: &mut App, transcript: &Path, info: SessionInfo, live: bool) {
    // Dropping the old sources/hooks stops their tailers and watchers.
    app.sources.clear();
    app.tick_hooks.clear();
    app.state = fresh_state(&app.state, info);
    let session_dir = transcript.with_extension("");

    // Liveness + git every 5 s.
    let mut last_git = Instant::now() - Duration::from_secs(10);
    let base_commit = crate::files::head_commit(&app.state.session.cwd);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        state.session.refresh_alive(state.now_ms);
        if !state.session.alive && state.session.ended_at_ms.is_none() {
            state.session.ended_at_ms = state.last_line_at_ms;
        }
        if last_git.elapsed() >= Duration::from_secs(5) && state.session.cwd.is_dir() {
            last_git = Instant::now();
            if let Some(g) = crate::git::info(&state.session.cwd) {
                state.session.git_branch = g.branch;
                state.session.git_dirty = g.dirty;
            }
            if let Some(base) = &base_commit {
                let ns = crate::files::numstat(&state.session.cwd, base);
                let cwd = state.session.cwd.clone();
                state.files.apply_numstat(&cwd, &ns);
            }
        }
    }));

    // CLAUDE.md files and memory index for the prefix inspector, every 30 s.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut last_scan = Instant::now() - Duration::from_secs(60);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if last_scan.elapsed() >= Duration::from_secs(30) {
            last_scan = Instant::now();
            let cwd = state.session.cwd.clone();
            state.prefix.scan(&cwd, home.as_deref());
        }
    }));

    if !live {
        return;
    }

    // Process tree once a second.
    if let Some(pid) = app.state.session.pid {
        let configs = crate::procs::mcp_configs(&app.state.session.cwd);
        let mut sampler = crate::procs::Sampler::default();
        let mut last = Instant::now() - Duration::from_secs(5);
        app.tick_hooks.push(Box::new(move |state: &mut State| {
            if last.elapsed() < Duration::from_secs(1) {
                return;
            }
            last = Instant::now();
            let snap = sampler.snapshot(&crate::procs::Ps, pid, &configs);
            if let Some(m) = &snap.main {
                state.session.cpu_pct = Some(m.cpu_pct);
                state.session.rss_bytes = Some(m.rss_bytes);
            }
            state.mcp_exited = sampler.exited.clone();
            state.procs = snap;
        }));
    }
    // The registry every 2 s: other live sessions (they share the rate
    // limit), and this pid's own entry, which `/clear` rewrites with a new
    // session id while the process runs on (issue #2). The old transcript
    // never grows again, so the loop re-attaches to the new id.
    let my_pid = app.state.session.pid;
    let mut last_reg = Instant::now() - Duration::from_secs(10);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if last_reg.elapsed() >= Duration::from_secs(2) {
            last_reg = Instant::now();
            if let Some(dir) = crate::registry::default_dir() {
                let sessions = crate::registry::list(&dir);
                let others: Vec<&crate::registry::Session> = sessions
                    .iter()
                    .filter(|s| Some(s.pid) != my_pid && s.is_alive())
                    .collect();
                state.other_live_sessions = others
                    .iter()
                    .filter(|s| s.status() == crate::registry::Status::Busy)
                    .count();
                state.other_sessions = others
                    .iter()
                    .map(|s| {
                        (
                            s.name.clone(),
                            s.status() == crate::registry::Status::Busy,
                            s.status_updated_at as i64,
                        )
                    })
                    .collect();
                if state.rotated_to.is_none() {
                    state.rotated_to = rotated_entry(&sessions, my_pid, &state.session.session_id);
                }
            }
        }
    }));
    // Hook spool.
    let cctop_home = crate::status::cctop_dir();
    let mut hooks = crate::hooks::Watcher::new(&cctop_home, &app.state.session.session_id);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        for ev in hooks.poll() {
            state.apply_hook(&ev);
        }
    }));
    if let Some(v) = crate::claude_home::read() {
        app.state.apply_claude_home(&v);
    }
    if app.state.session.pid.is_some() {
        app.state.load_autocompact();
    }
    // Status-line samples.
    let mut status = crate::status::Watcher::new(&app.state.session.session_id);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if status.poll() {
            if let Some(s) = status.latest.as_ref() {
                state.apply_status(s, &status.series_5h);
            }
        }
    }));
    // Background tasks every 2 s.
    if let Some(dir) = crate::tasks::dir_for(&app.state.session.session_id) {
        let mut last = Instant::now() - Duration::from_secs(5);
        app.tick_hooks.push(Box::new(move |state: &mut State| {
            if last.elapsed() >= Duration::from_secs(2) {
                last = Instant::now();
                let dir_tasks = crate::tasks::load(&dir);
                state.refresh_tasks(dir_tasks);
            }
        }));
    }
    // Subagents.
    let mut agents = crate::agents::AgentWatcher::watch(&session_dir);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if agents.poll() || state.agents.len() != agents.agents.len() {
            state.agents = agents.agents.clone();
        }
    }));
    // The transcript itself.
    if let Ok(t) = crate::tail::Tailer::open(transcript) {
        app.sources.push(Box::new(t) as Box<dyn LineSource>);
    }
}

/// The live registry entry that carries `pid` under a session id other than
/// `session_id`: `/clear` starts a new transcript and rewrites
/// `sessions/<pid>.json` in place, and nothing else announces it. None for
/// a fixture (no pid), an unchanged id, a gone entry or a dead process.
pub fn rotated_entry(
    sessions: &[crate::registry::Session],
    pid: Option<u32>,
    session_id: &str,
) -> Option<crate::registry::Session> {
    let pid = pid?;
    sessions
        .iter()
        .find(|s| s.pid == pid && !s.session_id.is_empty() && s.session_id != session_id)
        .filter(|s| s.is_alive())
        .cloned()
}

/// Load a fixture fully into `app` (headless): attach without followers,
/// then feed every line.
pub fn attach_headless(app: &mut App, transcript: &Path, info: SessionInfo) {
    attach_headless_prefix(app, transcript, info, usize::MAX);
}

/// Like [`attach_headless`] but only the first `n` lines — a point in time.
pub fn attach_headless_prefix(app: &mut App, transcript: &Path, info: SessionInfo, n: usize) {
    attach(app, transcript, info, false);
    for line in crate::transcript::parse_file(transcript)
        .unwrap_or_default()
        .into_iter()
        .take(n)
    {
        app.feed(line);
    }
    app.state.agents = crate::agents::load(&transcript.with_extension(""));
    if app.state.session.ended_at_ms.is_none() && !app.state.session.alive {
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
    }
    app.tick();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Session;

    // The registry fixtures: `86143.json` is cctop-46; the pid is replaced by
    // this test process's own, so `is_alive` holds.
    fn registry() -> Vec<Session> {
        crate::registry::list(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions"))
    }

    fn mine(session_id: &str) -> Session {
        let mut s = registry()
            .into_iter()
            .find(|s| s.name == "cctop-46")
            .unwrap();
        s.pid = std::process::id();
        s.session_id = session_id.into();
        s
    }

    const OLD: &str = "b34fc081-f0e9-48ae-9c1a-552587db6403";
    const NEW: &str = "9d337130-79ad-4df0-8bd5-1365210208b1";

    #[test]
    fn rotated_entry_is_this_pid_under_another_live_id() {
        let pid = Some(std::process::id());
        let mut sessions = registry();
        sessions.push(mine(NEW));
        // /clear: the entry for our pid now names the new id.
        let next = rotated_entry(&sessions, pid, OLD).unwrap();
        assert_eq!(next.session_id, NEW);
        assert_eq!(next.name, "cctop-46");
        // Unchanged id, unknown pid (a fixture), another pid's entry: nothing.
        assert!(rotated_entry(&sessions, pid, NEW).is_none());
        assert!(rotated_entry(&sessions, None, OLD).is_none());
        assert!(rotated_entry(&sessions, Some(86143), OLD).is_none());
        // A dead process's entry is not followed, whatever id it carries.
        let mut dead = mine(NEW);
        dead.pid = 4_194_000;
        assert!(rotated_entry(&[dead], Some(4_194_000), OLD).is_none());
        // An entry with no id (a broken file) is not a rotation either.
        assert!(rotated_entry(&[mine("")], pid, OLD).is_none());
    }
}
