//! Tool calls: `tool_use` blocks paired with their `tool_result`, plus
//! per-tool statistics for the Tools panel and the Advisor.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::metrics::cost::parse_ts_ms;
use crate::transcript::{AssistantBlock, Line};

/// One tool invocation.
#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    /// Display name: the tool name, or `mcp:<server>` for MCP tools.
    pub name: String,
    /// For MCP tools, the tool name within the server.
    pub mcp_tool: Option<String>,
    /// ≤ 30 chars of the most telling input field.
    pub input_summary: String,
    /// Epoch ms of the assistant line that issued the call.
    pub started_at: Option<i64>,
    /// Epoch ms of the user line that carried the result.
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    /// Transcript timestamps include any permission wait; hook events (later)
    /// replace them and clear this flag.
    pub approx_duration: bool,
    pub is_error: bool,
    /// `len(result text) / 4` — the context this result occupies.
    pub result_tokens_est: u64,
    /// Turn number the call belongs to (1-based), if known.
    pub turn: usize,
}

impl Call {
    pub fn is_running(&self) -> bool {
        self.finished_at.is_none()
    }
}

/// Split `mcp__server__tool` into display name and tool.
pub fn display_name(raw: &str) -> (String, Option<String>) {
    if let Some(rest) = raw.strip_prefix("mcp__") {
        if let Some((server, tool)) = rest.split_once("__") {
            return (format!("mcp:{server}"), Some(tool.to_string()));
        }
    }
    (raw.to_string(), None)
}

/// Pick the most telling input field and clip it to 30 chars.
pub fn summarize_input(name: &str, input: &Value) -> String {
    const KEYS: &[&str] = &[
        "command",
        "file_path",
        "notebook_path",
        "pattern",
        "path",
        "query",
        "url",
        "description",
        "prompt",
        "skill",
    ];
    let s = KEYS
        .iter()
        .filter_map(|k| input.get(k).and_then(Value::as_str))
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| match input {
            Value::Object(m) if !m.is_empty() => m
                .values()
                .find_map(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            _ => String::new(),
        });
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = name;
    clip(&s, 30)
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

/// Aggregated figures for one tool name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolStats {
    pub name: String,
    pub calls: usize,
    pub errors: usize,
    pub running: usize,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub last_call_at: Option<i64>,
    pub tokens_to_ctx: u64,
    /// True when any duration in the sample is approximate.
    pub approx: bool,
}

/// All calls of a session, in issue order.
#[derive(Debug, Default)]
pub struct Stats {
    pub calls: Vec<Call>,
    index: HashMap<String, usize>,
    turn: usize,
}

impl Stats {
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a Line>) -> Stats {
        let mut s = Stats::default();
        for l in lines {
            s.push(l);
        }
        s
    }

    pub fn push(&mut self, line: &Line) {
        match line {
            Line::User(u) => {
                let is_prompt = !u.is_meta && u.message.content.tool_results().next().is_none();
                if is_prompt {
                    self.turn += 1;
                }
                let at = u.timestamp.as_deref().and_then(parse_ts_ms);
                for r in u.message.content.tool_results() {
                    let Some(&i) = self.index.get(&r.tool_use_id) else {
                        continue;
                    };
                    let c = &mut self.calls[i];
                    c.finished_at = at;
                    c.duration_ms = match (c.started_at, at) {
                        (Some(s), Some(f)) if f >= s => Some((f - s) as u64),
                        _ => None,
                    };
                    c.is_error = r.is_error;
                    c.result_tokens_est = (r.text().len() / 4) as u64;
                }
            }
            Line::Assistant(a) => {
                let at = a.timestamp.as_deref().and_then(parse_ts_ms);
                for b in &a.message.content {
                    if let AssistantBlock::ToolUse { id, name, input } = b {
                        if self.index.contains_key(id) {
                            continue; // duplicate line of the same response
                        }
                        let (display, mcp_tool) = display_name(name);
                        self.index.insert(id.clone(), self.calls.len());
                        self.calls.push(Call {
                            id: id.clone(),
                            name: display,
                            mcp_tool,
                            input_summary: summarize_input(name, input),
                            started_at: at,
                            finished_at: None,
                            duration_ms: None,
                            approx_duration: true,
                            is_error: false,
                            result_tokens_est: 0,
                            turn: self.turn.max(1),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    pub fn get(&self, id: &str) -> Option<&Call> {
        self.index.get(id).map(|&i| &self.calls[i])
    }

    /// Replace a call's timing with exact figures (from hook events).
    pub fn set_exact_duration(&mut self, id: &str, started_at: i64, finished_at: i64) {
        if let Some(&i) = self.index.get(id) {
            let c = &mut self.calls[i];
            c.started_at = Some(started_at);
            c.finished_at = Some(finished_at);
            c.duration_ms = Some((finished_at - started_at).max(0) as u64);
            c.approx_duration = false;
        }
    }

    /// The call currently running, if any (most recently issued first).
    pub fn running(&self) -> Option<&Call> {
        self.calls.iter().rev().find(|c| c.is_running())
    }

    /// Per-tool statistics, keyed by display name.
    pub fn by_name(&self) -> BTreeMap<String, ToolStats> {
        let mut groups: BTreeMap<String, Vec<&Call>> = BTreeMap::new();
        for c in &self.calls {
            groups.entry(c.name.clone()).or_default().push(c);
        }
        groups
            .into_iter()
            .map(|(name, calls)| {
                let mut durs: Vec<u64> = calls.iter().filter_map(|c| c.duration_ms).collect();
                durs.sort_unstable();
                (
                    name.clone(),
                    ToolStats {
                        name,
                        calls: calls.len(),
                        errors: calls.iter().filter(|c| c.is_error).count(),
                        running: calls.iter().filter(|c| c.is_running()).count(),
                        p50_ms: percentile(&durs, 0.50),
                        p95_ms: percentile(&durs, 0.95),
                        last_call_at: calls.iter().filter_map(|c| c.started_at).max(),
                        tokens_to_ctx: calls.iter().map(|c| c.result_tokens_est).sum(),
                        approx: calls
                            .iter()
                            .any(|c| c.duration_ms.is_some() && c.approx_duration),
                    },
                )
            })
            .collect()
    }

    /// The `n` calls whose results occupy the most context.
    pub fn top_ctx(&self, n: usize) -> Vec<&Call> {
        let mut v: Vec<&Call> = self
            .calls
            .iter()
            .filter(|c| c.result_tokens_est > 0)
            .collect();
        v.sort_by(|a, b| b.result_tokens_est.cmp(&a.result_tokens_est));
        v.truncate(n);
        v
    }

    /// Total context occupied by tool results.
    pub fn tokens_to_ctx(&self) -> u64 {
        self.calls.iter().map(|c| c.result_tokens_est).sum()
    }
}

/// Nearest-rank percentile of a sorted slice.
fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn fixture() -> Vec<Line> {
        parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl")).unwrap()
    }

    #[test]
    fn every_tool_use_is_paired() {
        let lines = fixture();
        let s = Stats::from_lines(&lines);
        assert_eq!(s.calls.len(), 257);
        assert_eq!(
            s.calls.iter().filter(|c| c.finished_at.is_some()).count(),
            257
        );
        assert!(s.running().is_none());
        assert!(s
            .calls
            .iter()
            .all(|c| c.duration_ms.is_some() && c.approx_duration));
        assert!(s.calls.iter().all(|c| c.turn >= 1));
    }

    #[test]
    fn known_bash_call_matches_hand_computed_values() {
        // toolu_01RFWxUzic1XtD2xrUwwVYhe: issued 09:24:53.729Z, result 09:24:54.592Z,
        // result text 2900 bytes, is_error true in the source session.
        let s = Stats::from_lines(&fixture());
        let c = s.get("toolu_01RFWxUzic1XtD2xrUwwVYhe").unwrap();
        assert_eq!(c.name, "Bash");
        assert_eq!(c.duration_ms, Some(863));
        assert_eq!(c.result_tokens_est, 725);
        assert!(c.is_error);
        assert_eq!(c.input_summary, "make check");
    }

    #[test]
    fn mcp_tools_group_by_server() {
        let s = Stats::from_lines(&fixture());
        let by = s.by_name();
        let chrome = &by["mcp:claude-in-chrome"];
        assert_eq!(chrome.calls, 192 + 14 + 4 + 3 + 1);
        assert_eq!(by["Bash"].calls, 15);
        assert_eq!(by["Read"].calls, 3);
        let computer = s
            .calls
            .iter()
            .find(|c| c.mcp_tool.as_deref() == Some("computer"))
            .unwrap();
        assert_eq!(computer.name, "mcp:claude-in-chrome");
        assert_eq!(display_name("Read"), ("Read".into(), None));
        assert_eq!(
            display_name("mcp__github__get_me"),
            ("mcp:github".into(), Some("get_me".into()))
        );
    }

    #[test]
    fn stats_percentiles_and_top_ctx() {
        let s = Stats::from_lines(&fixture());
        let by = s.by_name();
        let bash = &by["Bash"];
        assert!(bash.p50_ms.unwrap() <= bash.p95_ms.unwrap());
        assert!(bash.approx);
        assert!(bash.last_call_at.is_some());
        assert_eq!(
            by.values().map(|t| t.tokens_to_ctx).sum::<u64>(),
            s.tokens_to_ctx()
        );
        let top = s.top_ctx(3);
        assert_eq!(top.len(), 3);
        assert!(top[0].result_tokens_est >= top[1].result_tokens_est);
        assert!(top[1].result_tokens_est >= top[2].result_tokens_est);
        // Fixture strings are capped at 4000 bytes → at most 1000 tokens each.
        assert!(top[0].result_tokens_est <= 1000);
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[10, 20, 30, 40], 0.5), Some(20));
        assert_eq!(percentile(&[10, 20, 30, 40], 0.95), Some(40));
    }

    #[test]
    fn exact_duration_clears_approx() {
        let mut s = Stats::from_lines(&fixture());
        s.set_exact_duration("toolu_01RFWxUzic1XtD2xrUwwVYhe", 1000, 1500);
        let c = s.get("toolu_01RFWxUzic1XtD2xrUwwVYhe").unwrap();
        assert_eq!(c.duration_ms, Some(500));
        assert!(!c.approx_duration);
    }

    #[test]
    fn input_summary_clips_and_picks_key() {
        let v: Value = serde_json::from_str(
            r#"{"file_path":"/a/very/long/path/that/goes/on/and/on/forever.rs","limit":5}"#,
        )
        .unwrap();
        let s = summarize_input("Read", &v);
        assert_eq!(s.chars().count(), 30);
        assert!(s.ends_with('…'));
        let v: Value = serde_json::from_str(r#"{"command":"ls   -la\n  /tmp"}"#).unwrap();
        assert_eq!(summarize_input("Bash", &v), "ls -la /tmp");
        assert_eq!(summarize_input("X", &Value::Null), "");
    }
}
