//! cctop library: collectors, metrics and UI. The `cctop` binary is a thin
//! CLI over this crate so that every number is reachable from tests and from
//! `cctop query`.

pub mod agents;
pub mod alerts;
pub mod app;
pub mod discover;
pub mod events;
pub mod git;
pub mod metrics;
pub mod procs;
pub mod registry;
pub mod tail;
pub mod tools;
pub mod transcript;
pub mod ui;

/// `~/.claude/projects/<cwd-slug>/<sessionId>.jsonl` for a registry session.
/// Claude Code derives the slug by replacing every non-alphanumeric byte of
/// the absolute cwd with `-`.
pub fn transcript_path(s: &registry::Session) -> std::path::PathBuf {
    let slug: String = s
        .cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    home.join(".claude")
        .join("projects")
        .join(slug)
        .join(format!("{}.jsonl", s.session_id))
}
