//! The pane's context-breakdown spool, `~/.cctop/context/<session>.json`:
//! what `$.session.usage({ breakdown: 'summary' })` said about the window,
//! written by `plugin/hooks/breakdown.ts`. Here it becomes a
//! [`ContextCapture`] — the same shape `/context` prints — so residency
//! calibrates from it. Read-only; unknown schema versions are ignored.

use std::path::PathBuf;

use serde::Deserialize;

use crate::transcript::ContextCapture;

/// The schema version this reader understands.
const VERSION: u64 = 1;

/// A parsed spool: the capture and the epoch ms the pane read it at.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolCapture {
    pub at_ms: i64,
    pub capture: ContextCapture,
}

#[derive(Deserialize)]
struct Spool {
    v: u64,
    at: i64,
    tokens: u64,
    window: u64,
    model: Option<String>,
    #[serde(default)]
    categories: Vec<Category>,
    autocompact_threshold: Option<u64>,
    #[serde(default)]
    mcp_tools: Vec<Tool>,
    skills: Option<Skills>,
}

#[derive(Deserialize)]
struct Category {
    name: String,
    tokens: u64,
    kind: String,
}

#[derive(Deserialize)]
struct Tool {
    tokens: u64,
}

#[derive(Deserialize)]
struct Skills {
    count: u64,
    tokens: u64,
}

/// Parse a spool; `None` for malformed JSON or a version other than 1.
pub fn parse(text: &str) -> Option<SpoolCapture> {
    let s: Spool = serde_json::from_str(text).ok()?;
    if s.v != VERSION {
        return None;
    }
    let sum = |kind: &str| -> Option<u64> {
        let mut rows = s.categories.iter().filter(|c| c.kind == kind).peekable();
        rows.peek()?;
        Some(rows.map(|c| c.tokens).sum())
    };
    let capture = ContextCapture {
        model: s.model.clone(),
        used_tokens: s.tokens,
        window_tokens: s.window,
        used_pct: (s.window > 0).then(|| s.tokens as f64 / s.window as f64 * 100.0),
        categories: s
            .categories
            .iter()
            .filter(|c| c.kind == "used")
            .map(|c| (c.name.clone(), c.tokens))
            .collect(),
        free_tokens: sum("free"),
        autocompact_buffer: sum("buffer"),
        autocompact_window: s.autocompact_threshold,
        mcp_tools: Some((
            s.mcp_tools.len() as u64,
            s.mcp_tools.iter().map(|t| t.tokens).sum(),
        )),
        skills: s.skills.as_ref().map(|k| (k.count, k.tokens)),
    };
    Some(SpoolCapture {
        at_ms: s.at,
        capture,
    })
}

pub fn path(session_id: &str) -> PathBuf {
    crate::status::cctop_dir()
        .join("context")
        .join(format!("{session_id}.json"))
}

/// Read the spool once, if present and understood.
pub fn read(session_id: &str) -> Option<SpoolCapture> {
    parse(&std::fs::read_to_string(path(session_id)).ok()?)
}

/// Polls the spool file for a session; yields a capture when it changed.
#[derive(Debug)]
pub struct Watcher {
    path: PathBuf,
    last_mtime: Option<std::time::SystemTime>,
}

impl Watcher {
    pub fn new(session_id: &str) -> Watcher {
        Watcher::at(path(session_id))
    }

    pub fn at(path: PathBuf) -> Watcher {
        Watcher {
            path,
            last_mtime: None,
        }
    }

    pub fn poll(&mut self) -> Option<SpoolCapture> {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            self.last_mtime = None;
            return None;
        };
        let mtime = meta.modified().ok();
        if mtime.is_some() && mtime == self.last_mtime {
            return None;
        }
        self.last_mtime = mtime;
        parse(&std::fs::read_to_string(&self.path).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPOOL: &str = r#"{"v":1,"at":1789000000000,"tokens":88700,"window":1000000,"model":"claude-opus-5","categories":[{"name":"System prompt","tokens":6000,"kind":"used"},{"name":"System tools","tokens":12000,"kind":"used"},{"name":"MCP tools (deferred)","tokens":900,"kind":"deferred"},{"name":"Messages","tokens":70000,"kind":"used"},{"name":"Free space","tokens":850000,"kind":"free"},{"name":"Autocompact buffer","tokens":33000,"kind":"buffer"}],"autocompact_threshold":967000,"is_autocompact_enabled":true,"memory_files":[{"path":"/x/CLAUDE.md","tokens":500}],"mcp_tools":[{"name":"a","tokens":300},{"name":"b","tokens":600}],"skills":{"count":4,"tokens":1200},"agents":[]}"#;

    #[test]
    fn parses_v1() {
        let s = parse(SPOOL).unwrap();
        assert_eq!(s.at_ms, 1_789_000_000_000);
        let c = s.capture;
        assert_eq!(c.model.as_deref(), Some("claude-opus-5"));
        assert_eq!((c.used_tokens, c.window_tokens), (88_700, 1_000_000));
        assert!((c.used_pct.unwrap() - 8.87).abs() < 1e-9);
        let names: Vec<_> = c.categories.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        assert_eq!(
            names,
            [
                ("System prompt", 6000),
                ("System tools", 12000),
                ("Messages", 70000)
            ]
        );
        assert!(
            !names.iter().any(|(n, _)| n.contains("deferred")),
            "deferred tools are not in the window"
        );
        assert_eq!(c.free_tokens, Some(850_000));
        assert_eq!(c.autocompact_buffer, Some(33_000));
        assert_eq!(c.autocompact_window, Some(967_000));
        assert_eq!(c.mcp_tools, Some((2, 900)));
        assert_eq!(c.skills, Some((4, 1200)));
    }

    #[test]
    fn absent_optionals_stay_none() {
        let s = parse(r#"{"v":1,"at":5,"tokens":10,"window":100,"model":null}"#).unwrap();
        assert_eq!(s.capture.skills, None);
        assert_eq!(s.capture.autocompact_window, None);
        assert_eq!(s.capture.free_tokens, None);
    }

    #[test]
    fn unknown_version_ignored() {
        assert!(parse(&SPOOL.replacen("\"v\":1", "\"v\":2", 1)).is_none());
    }

    #[test]
    fn truncated_json_ignored() {
        assert!(parse(&SPOOL[..SPOOL.len() / 2]).is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn watcher_yields_once_per_change() {
        let dir = std::env::temp_dir().join(format!("cctop-spool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.json");
        let mut w = Watcher::at(p.clone());
        assert!(w.poll().is_none());
        std::fs::write(&p, SPOOL).unwrap();
        assert!(w.poll().is_some());
        assert!(w.poll().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
