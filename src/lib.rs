//! cctop library: collectors, metrics and UI. The `cctop` binary is a thin
//! CLI over this crate so that every number is reachable from tests and from
//! `cctop query`.

pub mod advisor;
pub mod agents;
pub mod alerts;
pub mod app;
pub mod ask;
pub mod attach;
pub mod baseline;
pub mod config;
pub mod discover;
pub mod events;
pub mod files;
pub mod git;
pub mod harness_facts;
pub mod hooks;
pub mod install;
pub mod ledger;
pub mod load;
pub mod mcp;
pub mod metrics;
pub mod otel;
pub mod pane;
pub mod phase;
pub mod prefix;
pub mod procs;
pub mod query;
pub mod registry;
pub mod report;
pub mod split;
pub mod status;
pub mod tail;
pub mod tasks;
pub mod theme;
pub mod tools;
pub mod transcript;
pub mod ui;

/// `~/.claude/projects/<cwd-slug>/<sessionId>.jsonl` for a registry session.
/// Claude Code derives the slug by replacing every non-alphanumeric byte of
/// the absolute cwd with `-`.
pub fn transcript_path(s: &registry::Session) -> std::path::PathBuf {
    let slug = slug(&s.cwd);
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    home.join(".claude")
        .join("projects")
        .join(slug)
        .join(format!("{}.jsonl", s.session_id))
}

/// Claude Code's project slug for a working directory: every non-alphanumeric
/// character of the absolute path becomes `-`.
pub fn slug(cwd: &std::path::Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
