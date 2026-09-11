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
    // Other live sessions (shared rate limit), every 5 s.
    let my_pid = app.state.session.pid;
    let mut last_reg = Instant::now() - Duration::from_secs(10);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if last_reg.elapsed() >= Duration::from_secs(5) {
            last_reg = Instant::now();
            if let Some(dir) = crate::registry::default_dir() {
                state.other_live_sessions = crate::registry::list(&dir)
                    .iter()
                    .filter(|s| Some(s.pid) != my_pid && s.is_alive())
                    .filter(|s| s.status() == crate::registry::Status::Busy)
                    .count();
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
                state.tasks = crate::tasks::load(&dir);
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

/// Load a fixture fully into `app` (headless): attach without followers,
/// then feed every line.
pub fn attach_headless(app: &mut App, transcript: &Path, info: SessionInfo) {
    attach(app, transcript, info, false);
    for line in crate::transcript::parse_file(transcript).unwrap_or_default() {
        app.feed(line);
    }
    app.state.agents = crate::agents::load(&transcript.with_extension(""));
    if app.state.session.ended_at_ms.is_none() && !app.state.session.alive {
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
    }
    app.tick();
}
