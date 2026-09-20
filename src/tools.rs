//! Tool calls: `tool_use` blocks paired with their `tool_result`, plus
//! per-tool statistics for the Tools panel and the Advisor.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::metrics::cost::parse_ts_ms;
use crate::phase::{self, BashClass, CallShape, Phase, ToolClass};
use crate::transcript::{AssistantBlock, Line, PromptKind, ReadKind, TestMarker, ToolUseDetail};

/// One tool invocation.
#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    /// Display name: the tool name, or `mcp:<server>` for MCP tools.
    pub name: String,
    /// For MCP tools, the tool name within the server.
    pub mcp_tool: Option<String>,
    /// The most telling input field: a bounded 200-char command for Bash,
    /// ≤ 30 chars otherwise.
    pub input_summary: String,
    /// Characters the model wrote as the tool's input (`IN→CTX`: they
    /// stay in context like a result does).
    pub input_chars: usize,
    /// What the call is for, from its name and input.
    pub class: ToolClass,
    /// The Bash class, for Bash calls.
    pub bash_class: Option<BashClass>,
    /// `subagent_type` of an `Agent` call (the launch result names none).
    pub agent_type: Option<String>,
    /// A Bash command on the read-only allowlist (exploration runs).
    pub read_only: bool,
    /// File basenames the call touches.
    pub paths: Vec<String>,
    /// The one file the call reads or writes, as written in its input: the
    /// `file_path` of a Read / Edit / Write / NotebookEdit, or the single path
    /// a Bash `cat` / `sed -n` / `head` / `tail` reads (`None` when the
    /// command reads two or more — their output stays unattributed). What
    /// the residency view keys its per-file rows on; `paths` keeps basenames
    /// for the phase assignment.
    pub path: Option<String>,
    /// What the output said about a test run.
    pub test_marker: TestMarker,
    /// Epoch ms of the assistant line that issued the call.
    pub started_at: Option<i64>,
    /// Epoch ms of the user line that carried the result.
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    /// Transcript timestamps include any permission wait; hook events (later)
    /// replace them and clear this flag.
    pub approx_duration: bool,
    pub is_error: bool,
    /// `len(result text) / 4`, plus `w·h/750` per image (1 500 when the
    /// size is unknown) — the context this result occupies.
    pub result_tokens_est: u64,
    /// Bytes of output Claude Code spilled to `tool-results/` instead of the
    /// context (the head stays; `result_tokens_est` is what stayed).
    pub persisted_output_size: Option<u64>,
    /// The result was replaced by `[Old tool result content cleared]`: it
    /// no longer occupies context.
    pub cleared: bool,
    /// Claude Code's error class for a failed call (its `/insights`
    /// taxonomy plus content-not-found / timeout / tool-not-found).
    pub error_class: Option<ErrorClass>,
    /// The result was cut at a cap (`truncatedByTokenCap`, a persisted
    /// spill, an MCP cap).
    pub truncated: bool,
    /// Turn number the call belongs to (1-based), if known.
    pub turn: usize,
    /// The call did not block the turn: `run_in_background`, or Claude Code
    /// moved it to the background itself (`backgroundTaskId`,
    /// `timedOutAfterMs`).
    pub background: bool,
    /// The typed refusal, when the call was denied.
    pub denial: Option<crate::transcript::DenialKind>,
}

/// What an `Agent` call reported back (`toolUseResult`), without its text.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSpawn {
    pub at: Option<i64>,
    pub turn: usize,
    /// The `Agent` call's own id: the second key a task notification
    /// carries (`<tool-use-id>`).
    pub tool_use_id: String,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    /// The model Claude Code resolved for it (`resolvedModel`).
    pub resolved_model: Option<String>,
    /// A `teammate_spawned` result's member name and team: the lead-side
    /// record of a teammate (`agent_id` is `<name>@<team>`).
    pub name: Option<String>,
    pub team_name: Option<String>,
    pub tool_uses: Option<u64>,
    pub is_async: bool,
    /// `usage.total()` on synchronous completions.
    pub tokens: u64,
}

/// Claude Code's own tool-error taxonomy (`tool_error_categories` in
/// `usage-data/session-meta`, ordered substrings), plus three classes its
/// `Other` bucket hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    CommandFailed,
    UserRejected,
    EditFailed,
    FileChanged,
    FileTooLarge,
    FileNotFound,
    ContentNotFound,
    Timeout,
    ToolNotFound,
    Denied,
    Other,
}

impl ErrorClass {
    /// Classify a failed result's text, in Claude Code's order.
    pub fn classify(text: &str, denied: bool) -> ErrorClass {
        let t = text.to_ascii_lowercase();
        if denied {
            return ErrorClass::Denied;
        }
        if t.contains("user rejected")
            || t.contains("user doesn't want")
            || t.contains("user declined")
        {
            ErrorClass::UserRejected
        } else if t.contains("exit code")
            || t.contains("command failed")
            || t.contains("exit status")
        {
            ErrorClass::CommandFailed
        } else if t.contains("string to replace not found")
            || t.contains("edit failed")
            || t.contains("old_string")
        {
            ErrorClass::EditFailed
        } else if t.contains("has been modified since")
            || t.contains("file has changed")
            || t.contains("modified since read")
        {
            ErrorClass::FileChanged
        } else if t.contains("too large") || t.contains("exceeds maximum") {
            ErrorClass::FileTooLarge
        } else if t.contains("no such file")
            || t.contains("does not exist")
            || t.contains("file not found")
            || t.contains("enoent")
        {
            ErrorClass::FileNotFound
        } else if t.contains("no matches found")
            || t.contains("no files found")
            || t.contains("not found in")
        {
            ErrorClass::ContentNotFound
        } else if t.contains("timed out") || t.contains("timeout") {
            ErrorClass::Timeout
        } else if t.contains("unknown tool")
            || t.contains("no such tool")
            || t.contains("tool not found")
        {
            ErrorClass::ToolNotFound
        } else {
            ErrorClass::Other
        }
    }

    /// Claude Code's own label where it has one.
    pub fn label(self) -> &'static str {
        match self {
            ErrorClass::CommandFailed => "Command Failed",
            ErrorClass::UserRejected => "User Rejected",
            ErrorClass::EditFailed => "Edit Failed",
            ErrorClass::FileChanged => "File Changed",
            ErrorClass::FileTooLarge => "File Too Large",
            ErrorClass::FileNotFound => "File Not Found",
            ErrorClass::ContentNotFound => "Content Not Found",
            ErrorClass::Timeout => "Timeout",
            ErrorClass::ToolNotFound => "Tool Not Found",
            ErrorClass::Denied => "Denied",
            ErrorClass::Other => "Other",
        }
    }
}

impl Call {
    pub fn is_running(&self) -> bool {
        self.finished_at.is_none()
    }

    /// The shape the phase assignment reads.
    pub fn shape(&self) -> CallShape {
        CallShape {
            name: self.name.clone(),
            class: self.class,
            paths: self.paths.clone(),
            turn: self.turn,
            result_bytes: self.result_tokens_est * 4,
            test_confirmed: self.test_marker != TestMarker::None,
        }
    }

    /// A test-class command whose output confirmed a run.
    pub fn is_confirmed_test(&self) -> bool {
        self.class == ToolClass::Test && self.test_marker != TestMarker::None
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

/// Pick the most telling input field: a Bash command bounded at 200 chars,
/// anything else clipped to 30.
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
    clip(&s, if name == "Bash" { 200 } else { 30 })
}

/// Characters of every string in the input, recursively: what the model
/// wrote to call the tool.
pub fn input_chars(input: &Value) -> usize {
    match input {
        Value::String(s) => s.chars().count(),
        Value::Array(a) => a.iter().map(input_chars).sum(),
        Value::Object(m) => m.values().map(input_chars).sum(),
        _ => 0,
    }
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
    /// Characters the model wrote as inputs, as tokens (`IN→CTX`).
    pub input_tokens: u64,
    /// Results cut at a cap (token cap, persisted spill).
    pub truncated: usize,
    /// True when any duration in the sample is approximate.
    pub approx: bool,
}

/// All calls of a session, in issue order.
#[derive(Debug, Default)]
pub struct Stats {
    pub calls: Vec<Call>,
    index: HashMap<String, usize>,
    turn: usize,
    /// Exact per-tool durations from OpenTelemetry, keyed by display name.
    pub otel_durations: HashMap<String, Vec<u64>>,
    /// Deferred tools loaded through `ToolSearch`, per MCP server (or
    /// `builtin`): each load rewrites the cached prefix.
    pub tool_search_loads: BTreeMap<String, usize>,
    /// Commits Claude Code summarised (`gitOperation.commit`): `(epoch ms,
    /// sha, turn)`.
    pub commits: Vec<(i64, String, usize)>,
    /// Pushes and PR actions: `(epoch ms, what)`.
    pub git_events: Vec<(i64, String)>,
    /// Every `Agent` result, in order.
    pub agent_spawns: Vec<AgentSpawn>,
    /// Every `Workflow` launch: `(tool_use_id, task id, run id)`.
    pub workflow_launches: Vec<(String, Option<String>, Option<String>)>,
    /// `TaskUpdate` results that completed a task: `(epoch ms, turn)`.
    pub task_completions: Vec<(i64, usize)>,
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
                if matches!(
                    u.prompt_kind(),
                    PromptKind::Human
                        | PromptKind::Machine
                        | PromptKind::TaskNotification
                        | PromptKind::TeammateMessage
                ) {
                    self.turn += 1;
                }
                let at = u.timestamp.as_deref().and_then(parse_ts_ms);
                let detail = u.tool_use_detail();
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
                    c.cleared = r.is_cleared();
                    c.denial = u.denial();
                    let text = r.text();
                    if r.is_error {
                        c.error_class =
                            Some(ErrorClass::classify(&text, u.tool_denial_kind.is_some()));
                    }
                    if text.contains("<persisted-output>") || text.contains("Output too large") {
                        c.truncated = true;
                    }
                    let mut tokens = (text.len() / 4) as u64;
                    let mut images = r.images() as u64;
                    match &detail {
                        Some(ToolUseDetail::Bash(b)) => {
                            c.test_marker = b.test_marker;
                            c.persisted_output_size = b.persisted_output_size;
                            c.truncated |= b.persisted_output_size.is_some();
                            c.background |=
                                b.background_task_id.is_some() || b.timed_out_after_ms.is_some();
                            if let Some(g) = &b.git_operation {
                                let when = at.unwrap_or(0);
                                if let Some((sha, _)) = &g.commit {
                                    self.commits.push((when, sha.clone(), c.turn));
                                }
                                if let Some(branch) = &g.push {
                                    self.git_events.push((when, format!("push {branch}")));
                                }
                                if let Some((n, action)) = &g.pr {
                                    self.git_events.push((when, format!("PR #{n} {action}")));
                                }
                            }
                        }
                        Some(ToolUseDetail::Read(rd)) => {
                            c.truncated |= rd.truncated_by_token_cap;
                            if rd.kind == ReadKind::Image {
                                tokens += rd.image_tokens().unwrap_or(1_500);
                                images = images.saturating_sub(1);
                            }
                        }
                        Some(ToolUseDetail::TaskUpdate(tu)) if tu.completed() => {
                            self.task_completions.push((at.unwrap_or(0), c.turn));
                        }
                        Some(ToolUseDetail::Workflow { task_id, run_id }) => {
                            self.workflow_launches.push((
                                r.tool_use_id.clone(),
                                task_id.clone(),
                                run_id.clone(),
                            ));
                        }
                        Some(ToolUseDetail::Agent(ag)) => {
                            self.agent_spawns.push(AgentSpawn {
                                at,
                                turn: c.turn,
                                tool_use_id: r.tool_use_id.clone(),
                                agent_id: ag.agent_id.clone(),
                                agent_type: ag.agent_type.clone().or(c.agent_type.clone()),
                                resolved_model: ag.resolved_model.clone(),
                                name: ag.name.clone(),
                                team_name: ag.team_name.clone(),
                                tool_uses: ag.total_tool_use_count,
                                is_async: ag.is_async,
                                tokens: ag.usage.as_ref().map(|u| u.total()).unwrap_or(0),
                            });
                        }
                        _ => {}
                    }
                    if c.name == "ToolSearch" {
                        if let Some(matches) = u
                            .tool_use_result
                            .as_ref()
                            .and_then(|v| v.get("matches"))
                            .and_then(Value::as_array)
                        {
                            for m in matches {
                                let name = m
                                    .as_str()
                                    .or_else(|| m.get("name").and_then(Value::as_str))
                                    .unwrap_or("");
                                let server = name
                                    .strip_prefix("mcp__")
                                    .and_then(|r| r.split("__").next())
                                    .unwrap_or("builtin")
                                    .to_string();
                                *self.tool_search_loads.entry(server).or_insert(0) += 1;
                            }
                        }
                    }
                    tokens += images * 1_500;
                    c.result_tokens_est = if c.cleared { 0 } else { tokens };
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
                        let class = phase::classify_tool(name, input);
                        let command = input.get("command").and_then(Value::as_str);
                        self.index.insert(id.clone(), self.calls.len());
                        self.calls.push(Call {
                            id: id.clone(),
                            name: display,
                            mcp_tool,
                            input_summary: summarize_input(name, input),
                            input_chars: input_chars(input),
                            class,
                            bash_class: (name == "Bash")
                                .then(|| phase::classify_bash(command.unwrap_or(""))),
                            agent_type: (name == "Agent")
                                .then(|| input.get("subagent_type").and_then(Value::as_str))
                                .flatten()
                                .map(str::to_string),
                            read_only: match name.as_str() {
                                "Bash" => phase::read_only_bash(command.unwrap_or("")),
                                "Read" | "Grep" | "Glob" | "WebFetch" | "LS" => true,
                                _ => false,
                            },
                            paths: phase::paths_of(name, input),
                            path: match name.as_str() {
                                "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
                                | "NotebookRead" => input
                                    .get("file_path")
                                    .or_else(|| input.get("notebook_path"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                "Bash" => {
                                    let ps = crate::files::bash_read_paths(command.unwrap_or(""));
                                    (ps.len() == 1).then(|| ps[0].clone())
                                }
                                _ => None,
                            },
                            test_marker: TestMarker::None,
                            started_at: at,
                            finished_at: None,
                            duration_ms: None,
                            approx_duration: true,
                            is_error: false,
                            result_tokens_est: 0,
                            persisted_output_size: None,
                            cleared: false,
                            error_class: None,
                            truncated: false,
                            turn: self.turn.max(1),
                            background: input
                                .get("run_in_background")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            denial: None,
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

    /// The state-line phase word and its run length, over the last calls.
    pub fn phase_now(&self) -> Option<(Phase, usize)> {
        let start = self.calls.len().saturating_sub(7);
        let shapes: Vec<CallShape> = self.calls[start..].iter().map(Call::shape).collect();
        phase::current(&shapes)
    }

    /// Consecutive read-only calls at the end of the list (the current
    /// exploration run): `(calls, context tokens they added)`. An `Agent`
    /// spawn or any other call ends the run.
    pub fn explore_run(&self) -> (usize, u64) {
        let mut n = 0;
        let mut tokens = 0;
        for c in self.calls.iter().rev() {
            if !c.read_only {
                break;
            }
            n += 1;
            tokens += c.result_tokens_est;
        }
        (n, tokens)
    }

    /// Failed calls by error class, most frequent first.
    pub fn errors_by_class(&self) -> Vec<(ErrorClass, usize)> {
        let mut m: BTreeMap<&'static str, (ErrorClass, usize)> = BTreeMap::new();
        for c in self.calls.iter().filter(|c| c.is_error) {
            let k = c.error_class.unwrap_or(ErrorClass::Other);
            m.entry(k.label()).or_insert((k, 0)).1 += 1;
        }
        let mut v: Vec<(ErrorClass, usize)> = m.into_values().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        v
    }

    /// Bash calls by class, most frequent first: `(class, calls, errors)`.
    pub fn bash_by_class(&self) -> Vec<(BashClass, usize, usize)> {
        let mut m: BTreeMap<&'static str, (BashClass, usize, usize)> = BTreeMap::new();
        for c in &self.calls {
            let Some(k) = c.bash_class else { continue };
            let e = m.entry(k.label()).or_insert((k, 0, 0));
            e.1 += 1;
            if c.is_error {
                e.2 += 1;
            }
        }
        let mut v: Vec<_> = m.into_values().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        v
    }

    /// Model-written input characters per tool, the `IN→CTX` column.
    pub fn input_chars_by_name(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for c in &self.calls {
            *m.entry(c.name.clone()).or_insert(0) += c.input_chars;
        }
        m
    }

    /// Per-tool statistics, keyed by display name.
    pub fn by_name(&self) -> BTreeMap<String, ToolStats> {
        self.grouped(|c| c.name.clone())
    }

    /// Per-tool statistics with Bash split by class (`Bash·test`,
    /// `Bash·explore`…), for the table.
    pub fn by_name_and_class(&self) -> BTreeMap<String, ToolStats> {
        self.grouped(|c| match c.bash_class {
            Some(k) => format!("Bash·{}", k.label()),
            None => c.name.clone(),
        })
    }

    fn grouped(&self, key: impl Fn(&Call) -> String) -> BTreeMap<String, ToolStats> {
        let mut groups: BTreeMap<String, Vec<&Call>> = BTreeMap::new();
        for c in &self.calls {
            groups.entry(key(c)).or_default().push(c);
        }
        groups
            .into_iter()
            .map(|(name, calls)| {
                let otel_key = name.split('·').next().unwrap_or(&name).to_string();
                let otel = self.otel_durations.get(&otel_key).filter(|v| !v.is_empty());
                let mut durs: Vec<u64> = match otel {
                    Some(v) => v.clone(),
                    None => calls.iter().filter_map(|c| c.duration_ms).collect(),
                };
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
                        input_tokens: calls.iter().map(|c| c.input_chars as u64 / 4).sum(),
                        truncated: calls.iter().filter(|c| c.truncated).count(),
                        approx: otel.is_none()
                            && calls
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
        v.sort_by_key(|t| std::cmp::Reverse(t.result_tokens_est));
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
    fn teammate_spawns_carry_the_name_and_team() {
        let lines =
            parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-d.jsonl"))
                .unwrap();
        let s = Stats::from_lines(&lines);
        let spawns: Vec<&AgentSpawn> = s
            .agent_spawns
            .iter()
            .filter(|a| a.agent_id.as_deref().is_some_and(|id| id.contains('@')))
            .collect();
        let names: Vec<&str> = spawns.iter().filter_map(|a| a.name.as_deref()).collect();
        assert_eq!(
            names,
            [
                "diff-pane-research",
                "diff-pane-research-2",
                "diff-pane-research-3"
            ]
        );
        for a in &spawns {
            assert_eq!(a.team_name.as_deref(), Some("session-afd065d3"));
            assert_eq!(a.agent_type.as_deref(), Some("claude-code-guide"));
            assert_eq!(a.resolved_model.as_deref(), Some("haiku"));
            assert_eq!(
                a.agent_id.as_deref(),
                Some(format!("{}@session-afd065d3", a.name.as_deref().unwrap()).as_str())
            );
        }
        // Subagent spawns carry neither.
        let a = Stats::from_lines(&fixture());
        assert!(a
            .agent_spawns
            .iter()
            .all(|s| s.name.is_none() && s.team_name.is_none()));
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
        assert_eq!(c.class, ToolClass::Test);
        assert_eq!(c.bash_class, Some(BashClass::Test));
        assert!(!c.read_only);
        assert!(c.input_chars >= "make check".len());
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
        // Fixture strings are capped at 4000 bytes → at most 1000 tokens of
        // text each; the largest results are screenshots, ~1 500 tokens per
        // image on top of their text.
        assert_eq!(top[0].name, "mcp:claude-in-chrome");
        assert!(top[0].result_tokens_est > 1_500 && top[0].result_tokens_est <= 2_500);
        assert!(s.calls.iter().all(|c| !c.cleared));
        let (run, _) = s.explore_run();
        assert_eq!(
            run, 0,
            "the session ended on MCP calls, not a read-only run"
        );
        assert!(s.phase_now().is_some());
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
