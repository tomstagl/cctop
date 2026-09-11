//! Build a fully loaded [`State`] for a session without a UI — shared by the
//! TUI start-up, `cctop query` and `cctop advise`, so every number comes from
//! the same collectors.

use std::path::{Path, PathBuf};

use crate::discover::{self, DiscoverError, Query};
use crate::ui::state::{SessionInfo, State};

/// Which session to load.
#[derive(Debug, Clone, Default)]
pub struct Target {
    /// Session id / name / pid, or a path to a fixture `.jsonl`.
    pub session: Option<String>,
    pub cwd: Option<PathBuf>,
    pub wait: bool,
}

/// A `--session` value naming an existing `.jsonl` (or a stem with one next
/// to it) is a fixture: read it instead of the live registry.
pub fn fixture_path(session: &str) -> Option<PathBuf> {
    let p = PathBuf::from(session);
    if p.is_file() {
        return Some(p);
    }
    let with_ext = p.with_extension("jsonl");
    with_ext.is_file().then_some(with_ext)
}

/// Resolve the target to a transcript path and session info.
pub fn resolve(t: &Target) -> Result<(PathBuf, SessionInfo), DiscoverError> {
    if let Some(p) = t.session.as_deref().and_then(fixture_path) {
        let info = SessionInfo::from_fixture(&p);
        return Ok((p, info));
    }
    let q = Query::from_env(t.session.clone(), t.cwd.clone());
    let s = discover::resolve_system(&q, t.wait)?;
    Ok((crate::transcript_path(&s), SessionInfo::from_registry(&s)))
}

/// Load everything once: transcript, subagents, prefix files, status-line
/// sample and hook spool.
pub fn state(t: &Target) -> Result<State, DiscoverError> {
    let (transcript, info) = resolve(t)?;
    Ok(state_from(&transcript, info))
}

pub fn state_from(transcript: &Path, info: SessionInfo) -> State {
    let mut state = State::new(crate::metrics::Pricing::load());
    state.session = info;
    state.now_ms = crate::app::now_ms();
    for line in crate::transcript::parse_file(transcript).unwrap_or_default() {
        state.apply(&line);
    }
    state.agents = crate::agents::load(&transcript.with_extension(""));
    if !state.session.alive {
        state.session.ended_at_ms = state.last_line_at_ms;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = state.session.cwd.clone();
    state.prefix.scan(&cwd, home.as_deref());
    let status = crate::status::Watcher::new(&state.session.session_id);
    if let Some(s) = status.latest.as_ref() {
        state.apply_status(s, &status.series_5h);
    }
    let mut hooks =
        crate::hooks::Watcher::new(&crate::status::cctop_dir(), &state.session.session_id);
    for ev in hooks.poll() {
        state.apply_hook(&ev);
    }
    if let Some(projects) = crate::baseline::default_projects_dir() {
        state.baseline = Some(crate::baseline::load_or_compute(
            &crate::status::cctop_dir(),
            &projects,
            state.now_ms,
        ));
    }
    state
}
