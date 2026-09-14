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
    match discover::resolve_system(&q, t.wait) {
        Ok(s) => Ok((crate::transcript_path(&s), SessionInfo::from_registry(&s))),
        // The key names no live session, but its transcript may still be on
        // disk: `/clear` gave the process a new id (issue #2), or the
        // process exited. Read that, as an ended session, rather than answer
        // nothing.
        Err(DiscoverError::NoMatch(key)) => {
            match crate::baseline::default_projects_dir()
                .and_then(|projects| archived_transcript(&projects, &key))
            {
                Some(p) => {
                    let info = SessionInfo::from_transcript(&p);
                    Ok((p, info))
                }
                None => Err(DiscoverError::NoMatch(key)),
            }
        }
        Err(e) => Err(e),
    }
}

/// The transcript under `projects` (`~/.claude/projects`) for a session id
/// the registry no longer lists: `<slug>/<key>.jsonl` for the full id, else,
/// for a key of at least 8 characters (what `--session` accepts as a
/// prefix), the newest transcript whose stem starts with it.
pub fn archived_transcript(projects: &Path, key: &str) -> Option<PathBuf> {
    if key.is_empty() || key.contains(['/', '\\']) {
        return None;
    }
    let slugs: Vec<PathBuf> = std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    let exact = slugs
        .iter()
        .map(|dir| dir.join(format!("{key}.jsonl")))
        .find(|p| p.is_file());
    if exact.is_some() || key.len() < 8 {
        return exact;
    }
    slugs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "jsonl")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with(key))
                && p.is_file()
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
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
    // Account facts only for a session of this machine, never for a fixture
    // file (the pane fixtures are generated from one).
    if state.session.pid.is_some() {
        if let Some(v) = crate::claude_home::read() {
            state.apply_claude_home(&v);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    const A: &str = "b34fc081-f0e9-48ae-9c1a-552587db6403";
    const B: &str = "b34fc081-0000-4000-8000-000000000000";

    // `~/.claude/projects` with two project slugs: A in the first, an older
    // B sharing A's first eight characters in the second, plus decoys.
    fn projects(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cctop-load-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let one = root.join("-Users-me-code-one");
        let two = root.join("-Users-me-code-two");
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        fs::write(one.join(format!("{A}.jsonl")), "{}\n").unwrap();
        fs::write(two.join(format!("{B}.jsonl")), "{}\n").unwrap();
        // The subagent directory named like a session is not a transcript.
        fs::create_dir_all(two.join(A)).unwrap();
        // A stray file at the top level is not a project.
        fs::write(root.join(format!("{A}.jsonl")), "{}\n").unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        fs::File::open(two.join(format!("{B}.jsonl")))
            .unwrap()
            .set_modified(old)
            .unwrap();
        root
    }

    #[test]
    fn archived_transcript_finds_the_full_id_in_any_project() {
        let root = projects("exact");
        let found = archived_transcript(&root, A).unwrap();
        assert_eq!(
            found,
            root.join("-Users-me-code-one").join(format!("{A}.jsonl"))
        );
        assert_eq!(
            archived_transcript(&root, B)
                .unwrap()
                .parent()
                .unwrap()
                .file_name()
                .unwrap(),
            "-Users-me-code-two"
        );
        assert_eq!(
            archived_transcript(&root, "c0ffee00-0000-4000-8000-000000000000"),
            None
        );
    }

    #[test]
    fn archived_transcript_takes_a_prefix_of_eight_and_prefers_the_newest() {
        let root = projects("prefix");
        // Both transcripts start with the eight characters; A is the newer.
        let found = archived_transcript(&root, "b34fc081").unwrap();
        assert!(found.ends_with(format!("{A}.jsonl")), "{}", found.display());
        assert!(archived_transcript(&root, "b34fc081-0000")
            .unwrap()
            .ends_with(format!("{B}.jsonl")));
        // Shorter than eight: no prefix search, as `--session` itself.
        assert_eq!(archived_transcript(&root, "b34fc08"), None);
        assert_eq!(archived_transcript(&root, ""), None);
        // A path is never a key.
        assert_eq!(archived_transcript(&root, "../-Users-me-code-one/x"), None);
        assert_eq!(archived_transcript(&root.join("missing"), A), None);
    }

    #[test]
    fn from_transcript_is_an_ended_session_named_by_its_id() {
        let info = SessionInfo::from_transcript(&PathBuf::from(format!("/x/-slug/{A}.jsonl")));
        assert_eq!(info.session_id, A);
        assert_eq!(info.name, "b34fc081");
        assert!(!info.alive);
        assert_eq!(info.pid, None);
        assert!(info.cwd.as_os_str().is_empty(), "the lines fill the cwd");
    }
}
