//! Subagents. Their transcripts live beside the main one:
//! `<projects>/<slug>/<sessionId>/subagents/agent-<id>.jsonl` with an
//! `agent-<id>.meta.json` describing type, model and description.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;

use notify::{RecursiveMode, Watcher};
use serde::Deserialize;

use crate::metrics::cost::parse_ts_ms;
use crate::metrics::{Aggregate, Usage};
use crate::tail::{parse_file, Tailer};
use crate::transcript::{AssistantBlock, Line};

/// After this long with no new lines, an agent whose last event was a
/// failed tool result is reported as failed.
pub const FAILED_AFTER_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Done,
    Failed,
}

/// `agent-<id>.meta.json`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub is_fork: bool,
    #[serde(default)]
    pub description: String,
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub spawn_depth: u32,
    /// `inherit` means the parent's model; the transcript tells the truth.
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    /// Model actually used (from the transcript), falling back to meta.
    pub model: String,
    pub spawn_depth: u32,
    pub is_fork: bool,
    /// Context inherited from the parent (`fork-context-ref.contextLength`).
    pub inherited_context_len: Option<u64>,
    /// The workflow run this agent belongs to (`subagents/workflows/<run>/`).
    pub workflow: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub last_line_at: Option<i64>,
    pub usage: Usage,
    pub api_calls: usize,
    pub tool_calls: usize,
    /// Tool calls the hook spool attributed to this agent (`agent_id`), and
    /// their exact run time — the agent's transcript may not exist yet.
    pub hook_tool_calls: usize,
    pub hook_tool_ms: u64,
    pub hook_tool_errors: usize,
    // -- derived-state inputs
    pending_tool_uses: usize,
    last_was_error_result: bool,
    last_assistant_ended_with_text: bool,
    last_stop_reason: Option<String>,
    agg: Aggregate,
}

impl Agent {
    pub fn new(id: &str, meta: Meta) -> Agent {
        Agent {
            id: id.to_string(),
            agent_type: meta.agent_type,
            description: meta.description,
            model: meta.model,
            spawn_depth: meta.spawn_depth,
            is_fork: meta.is_fork,
            inherited_context_len: None,
            workflow: None,
            started_at: None,
            finished_at: None,
            last_line_at: None,
            usage: Usage::default(),
            api_calls: 0,
            tool_calls: 0,
            hook_tool_calls: 0,
            hook_tool_ms: 0,
            hook_tool_errors: 0,
            pending_tool_uses: 0,
            last_was_error_result: false,
            last_assistant_ended_with_text: false,
            last_stop_reason: None,
            agg: Aggregate::default(),
        }
    }

    /// A hook event carrying this agent's `agent_id`: exact tool timings
    /// before (or without) the agent's own transcript.
    pub fn note_hook(&mut self, event: &str, duration_ms: Option<u64>, at_ms: i64) {
        self.started_at = self.started_at.or(Some(at_ms));
        self.last_line_at = Some(self.last_line_at.unwrap_or(at_ms).max(at_ms));
        match event {
            "PostToolUse" => {
                self.hook_tool_calls += 1;
                self.hook_tool_ms += duration_ms.unwrap_or(0);
            }
            "PostToolUseFailure" => {
                self.hook_tool_calls += 1;
                self.hook_tool_errors += 1;
                self.hook_tool_ms += duration_ms.unwrap_or(0);
            }
            _ => {}
        }
    }

    pub fn from_lines<'a>(
        id: &str,
        meta: Meta,
        lines: impl IntoIterator<Item = &'a Line>,
    ) -> Agent {
        let mut a = Agent::new(id, meta);
        for l in lines {
            a.push(l);
        }
        a
    }

    pub fn push(&mut self, line: &Line) {
        let at = match line {
            Line::User(u) => u.timestamp.as_deref(),
            Line::Assistant(a) => a.timestamp.as_deref(),
            Line::System(s) => s.timestamp.as_deref(),
            _ => None,
        }
        .and_then(parse_ts_ms);
        if at.is_some() {
            self.started_at = self.started_at.or(at);
            self.last_line_at = at;
        }
        match line {
            Line::Unknown(v)
                if v.get("type").and_then(|t| t.as_str()) == Some("fork-context-ref") =>
            {
                self.inherited_context_len = v.get("contextLength").and_then(|c| c.as_u64());
            }
            Line::Assistant(a) => {
                let before = self.agg.api_calls();
                self.agg.push(line);
                if self.agg.api_calls() > before {
                    self.api_calls += 1;
                    self.usage.add(&Usage::from_api(&a.message.usage));
                    if !a.message.model.is_empty() {
                        self.model = a.message.model.clone();
                    }
                }
                let uses = a
                    .message
                    .content
                    .iter()
                    .filter(|b| matches!(b, AssistantBlock::ToolUse { .. }))
                    .count();
                self.tool_calls += uses;
                self.pending_tool_uses += uses;
                self.last_assistant_ended_with_text =
                    matches!(a.message.content.last(), Some(AssistantBlock::Text { .. }))
                        && uses == 0;
                self.last_stop_reason = a
                    .message
                    .stop_reason
                    .clone()
                    .or(self.last_stop_reason.take());
                self.last_was_error_result = false;
            }
            Line::User(u) => {
                self.agg.push(line);
                let results: Vec<_> = u.message.content.tool_results().collect();
                if !results.is_empty() {
                    self.pending_tool_uses = self.pending_tool_uses.saturating_sub(results.len());
                    self.last_was_error_result = results.last().is_some_and(|r| r.is_error);
                    self.last_assistant_ended_with_text = false;
                }
            }
            _ => {}
        }
        if self.state(i64::MAX) == State::Done {
            self.finished_at = self.last_line_at;
        } else {
            self.finished_at = None;
        }
    }

    /// Current state given the wall clock (epoch ms).
    pub fn state(&self, now_ms: i64) -> State {
        let ended = self.last_stop_reason.as_deref() == Some("end_turn")
            || self.last_assistant_ended_with_text;
        if self.pending_tool_uses == 0 && ended && self.api_calls > 0 {
            return State::Done;
        }
        if self.last_was_error_result
            && self
                .last_line_at
                .is_some_and(|t| now_ms.saturating_sub(t) >= FAILED_AFTER_MS)
        {
            return State::Failed;
        }
        State::Running
    }

    pub fn elapsed_ms(&self, now_ms: i64) -> Option<i64> {
        let end = self.finished_at.unwrap_or(now_ms);
        self.started_at.map(|s| (end - s).max(0))
    }
}

/// Every `agent-<id>.jsonl` under `dir`, one level and deeper
/// (`workflows/wf_*/agent-*.jsonl`): `(path, id, workflow run)`.
pub fn agent_files(dir: &Path) -> Vec<(PathBuf, String, Option<String>)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), None::<String>)];
    while let Some((d, run)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for p in entries.flatten().map(|e| e.path()) {
            if p.is_dir() {
                // `workflows/<run>/…`: the run directory names the workflow.
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                let run = if d.file_name().is_some_and(|n| n == "workflows") {
                    name
                } else {
                    run.clone()
                };
                stack.push((p, run));
            } else if let Some(id) = id_from_path(&p) {
                out.push((p, id, run.clone()));
            }
        }
    }
    out.sort();
    out
}

/// A workflow run's journal (`journal.jsonl`): how many agents it launched,
/// finished and failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowJournal {
    pub run: String,
    pub launched: usize,
    pub started: usize,
    pub results: usize,
    pub failed: usize,
}

/// Read every `workflows/<run>/journal.jsonl` under `subagents`.
pub fn workflow_journals(subagents: &Path) -> Vec<WorkflowJournal> {
    let mut out = Vec::new();
    let Ok(runs) = std::fs::read_dir(subagents.join("workflows")) else {
        return out;
    };
    for run in runs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(text) = std::fs::read_to_string(run.join("journal.jsonl")) else {
            continue;
        };
        let mut j = WorkflowJournal {
            run: run
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ..Default::default()
        };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            match v.get("type").and_then(|t| t.as_str()) {
                Some("launched") => j.launched += 1,
                Some("started") => j.started += 1,
                Some("result") => j.results += 1,
                Some("failed") => j.failed += 1,
                _ => {}
            }
        }
        out.push(j);
    }
    out.sort_by(|a, b| a.run.cmp(&b.run));
    out
}

/// `~/.claude/teams`.
pub fn teams_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/teams"))
}

/// A teammate from `~/.claude/teams/<team>/config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Teammate {
    pub name: String,
    pub agent_type: String,
}

/// Members of the team this session leads (or belongs to), if any.
pub fn teammates(teams_dir: &Path, session_id: &str) -> Vec<Teammate> {
    let Ok(entries) = std::fs::read_dir(teams_dir) else {
        return Vec::new();
    };
    for team in entries.flatten().map(|e| e.path()) {
        let Ok(text) = std::fs::read_to_string(team.join("config.json")) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let lead = v
            .get("leadSessionId")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let named = team.file_name().is_some_and(|n| {
            n.to_string_lossy()
                .ends_with(&session_id[..session_id.len().min(8)])
        });
        if lead != session_id && !named {
            continue;
        }
        return v
            .get("members")
            .and_then(|m| m.as_array())
            .map(|m| {
                m.iter()
                    .map(|x| Teammate {
                        name: x
                            .get("name")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        agent_type: x
                            .get("agentType")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    Vec::new()
}

/// Agent id from `agent-<id>.jsonl`.
pub fn id_from_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    let id = stem.strip_prefix("agent-")?;
    (p.extension().is_some_and(|e| e == "jsonl")).then(|| id.to_string())
}

fn read_meta(dir: &Path, id: &str) -> Meta {
    std::fs::read_to_string(dir.join(format!("agent-{id}.meta.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Read all agents under `session_dir/subagents` once (no following),
/// workflow agents included.
pub fn load(session_dir: &Path) -> BTreeMap<String, Agent> {
    let dir = session_dir.join("subagents");
    let mut out = BTreeMap::new();
    for (p, id, run) in agent_files(&dir) {
        let lines = parse_file(&p).unwrap_or_default();
        let meta_dir = p.parent().unwrap_or(&dir);
        let mut a = Agent::from_lines(&id, read_meta(meta_dir, &id), &lines);
        a.workflow = run;
        out.insert(id, a);
    }
    out
}

/// Live view of `session_dir/subagents`: discovers new agent transcripts and
/// tails each. Call [`AgentWatcher::poll`] from the render loop.
pub struct AgentWatcher {
    dir: PathBuf,
    pub agents: BTreeMap<String, Agent>,
    tailers: HashMap<String, Tailer>,
    _watcher: Option<notify::RecommendedWatcher>,
    events: std_mpsc::Receiver<()>,
}

impl AgentWatcher {
    pub fn watch(session_dir: &Path) -> AgentWatcher {
        let dir = session_dir.join("subagents");
        let (tx, rx) = std_mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })
        .ok();
        // The directory may not exist yet; watch the session dir so we see it appear.
        if let Some(w) = watcher.as_mut() {
            let target = if dir.exists() {
                dir.clone()
            } else {
                session_dir.to_path_buf()
            };
            let _ = w.watch(&target, RecursiveMode::Recursive);
        }
        let mut aw = AgentWatcher {
            dir,
            agents: BTreeMap::new(),
            tailers: HashMap::new(),
            _watcher: watcher,
            events: rx,
        };
        aw.scan();
        aw
    }

    /// Pick up new agent files, workflow agents included.
    fn scan(&mut self) {
        for (p, id, run) in agent_files(&self.dir) {
            if self.tailers.contains_key(&id) {
                continue;
            }
            if let Ok(t) = Tailer::open(&p) {
                self.tailers.insert(id.clone(), t);
                let meta_dir = p.parent().unwrap_or(&self.dir).to_path_buf();
                let mut a = Agent::new(&id, read_meta(&meta_dir, &id));
                a.workflow = run;
                self.agents.insert(id.clone(), a);
            }
        }
    }

    /// Drain filesystem events and tailers; returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if self.events.try_recv().is_ok() {
            while self.events.try_recv().is_ok() {}
            self.scan();
            changed = true;
        }
        for (id, t) in self.tailers.iter_mut() {
            while let Some(line) = t.try_recv() {
                if let Some(a) = self.agents.get_mut(id) {
                    a.push(&line);
                    changed = true;
                }
            }
        }
        // Meta files can land after the transcript; refresh empty ones.
        for (id, a) in self.agents.iter_mut() {
            if a.agent_type.is_empty() {
                let meta_dir = match &a.workflow {
                    Some(run) => self.dir.join("workflows").join(run),
                    None => self.dir.clone(),
                };
                let m = read_meta(&meta_dir, id);
                if !m.agent_type.is_empty() {
                    a.agent_type = m.agent_type;
                    a.description = m.description;
                    a.is_fork = m.is_fork;
                    a.spawn_depth = m.spawn_depth;
                    if a.model.is_empty() {
                        a.model = m.model;
                    }
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn running(&self, now_ms: i64) -> usize {
        self.agents
            .values()
            .filter(|a| a.state(now_ms) == State::Running)
            .count()
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::*;

    #[test]
    fn workflow_agents_journals_and_teammates_are_found() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("subagents");
        let run = sub.join("workflows").join("wf_abc-123");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(sub.join("agent-top1.jsonl"), "").unwrap();
        std::fs::write(run.join("agent-deep1.jsonl"), "").unwrap();
        std::fs::write(
            run.join("agent-deep1.meta.json"),
            r#"{"agentType":"workflow-subagent","model":"claude-haiku-4-5-20251001"}"#,
        )
        .unwrap();
        std::fs::write(run.join("journal.jsonl"), "{\"type\":\"launched\"}\n{\"type\":\"started\"}\n{\"type\":\"failed\",\"agentId\":\"deep1\"}\n{\"type\":\"result\"}\nnot json\n").unwrap();
        let files = agent_files(&sub);
        assert_eq!(files.len(), 2);
        let deep = files.iter().find(|f| f.1 == "deep1").unwrap();
        assert_eq!(deep.2.as_deref(), Some("wf_abc-123"));
        let top = files.iter().find(|f| f.1 == "top1").unwrap();
        assert_eq!(top.2, None);
        let agents = load(&dir);
        assert_eq!(agents["deep1"].agent_type, "workflow-subagent");
        assert_eq!(agents["deep1"].workflow.as_deref(), Some("wf_abc-123"));
        assert!(agents["top1"].workflow.is_none());
        let j = workflow_journals(&sub);
        assert_eq!(
            j,
            vec![WorkflowJournal {
                run: "wf_abc-123".into(),
                launched: 1,
                started: 1,
                results: 1,
                failed: 1
            }]
        );
        let teams = dir.join("teams");
        std::fs::create_dir_all(teams.join("session-deadbeef")).unwrap();
        std::fs::write(
            teams.join("session-deadbeef/config.json"),
            r#"{"name":"t","leadSessionId":"deadbeef-1111","members":[{"agentId":"a","name":"lead","agentType":"team-lead"},{"agentId":"b","name":"researcher","agentType":"claude-code-guide"}]}"#,
        )
        .unwrap();
        let t = teammates(&teams, "deadbeef-1111");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].name, "researcher");
        assert!(teammates(&teams, "other-session").is_empty());
        assert!(teammates(Path::new("/nonexistent"), "x").is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn session_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a")
    }

    #[test]
    fn fixture_fork_agent_parses() {
        let agents = load(&session_dir());
        assert_eq!(agents.len(), 1);
        let a = &agents["a9a92645226d3a561"];
        assert_eq!(a.agent_type, "fork");
        assert!(a.is_fork);
        assert_eq!(a.spawn_depth, 1);
        assert_eq!(
            a.description,
            "Check whether a setup step needs the user personally"
        );
        assert_eq!(
            a.model, "claude-sonnet-5",
            "meta says inherit; transcript wins"
        );
        assert_eq!(a.inherited_context_len, Some(32));
        // Hand-counted: 14 assistant lines, 8 distinct responses, 1415 output tokens.
        assert_eq!(a.api_calls, 8);
        assert_eq!(a.usage.output, 1415);
        assert!(a.usage.cache_read > 0);
        assert_eq!(a.state(i64::MAX), State::Done);
        assert!(a.finished_at.is_some());
        assert!(a.elapsed_ms(i64::MAX).unwrap() > 0);
    }

    #[test]
    fn state_transitions() {
        let asst = |id: &str, block: &str, stop: &str| -> Line {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{{"id":"{id}","model":"m","content":[{block}],"stop_reason":{stop},"usage":{{"output_tokens":1}}}}}}"#
            ))
            .unwrap()
        };
        let tool_use = r#"{"type":"tool_use","id":"t1","name":"Bash","input":{}}"#;
        let text = r#"{"type":"text","text":"done"}"#;
        let result = |err: bool| -> Line {
            Line::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T00:00:10Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"x","is_error":{err}}}]}}}}"#
            ))
            .unwrap()
        };
        let t0 = parse_ts_ms("2026-01-01T00:00:10Z").unwrap();

        let mut a = Agent::new("x", Meta::default());
        assert_eq!(a.state(t0), State::Running);
        a.push(&asst("m1", tool_use, "\"tool_use\""));
        assert_eq!(a.state(t0), State::Running);
        a.push(&result(true));
        assert_eq!(
            a.state(t0 + 1_000),
            State::Running,
            "error result, but recent"
        );
        assert_eq!(a.state(t0 + FAILED_AFTER_MS), State::Failed);
        a.push(&asst("m2", text, "\"end_turn\""));
        assert_eq!(a.state(t0 + FAILED_AFTER_MS), State::Done);
        assert_eq!(a.api_calls, 2);

        // Final text without an explicit stop_reason (as Claude Code writes it) also counts as done.
        let mut b = Agent::new("y", Meta::default());
        b.push(&asst("m1", text, "null"));
        assert_eq!(b.state(t0), State::Done);
    }

    #[test]
    fn id_from_path_rules() {
        assert_eq!(
            id_from_path(Path::new("/x/agent-abc.jsonl")).as_deref(),
            Some("abc")
        );
        assert_eq!(id_from_path(Path::new("/x/agent-abc.meta.json")), None);
        assert_eq!(id_from_path(Path::new("/x/other.jsonl")), None);
    }

    #[tokio::test]
    async fn watcher_discovers_new_agent_files() {
        let dir = std::env::temp_dir().join(format!("cctop-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("subagents")).unwrap();
        let mut w = AgentWatcher::watch(&dir);
        assert!(w.agents.is_empty());
        std::fs::write(
            dir.join("subagents/agent-new1.meta.json"),
            r#"{"agentType":"Explore","description":"find things","spawnDepth":1,"model":"claude-haiku-4-5"}"#,
        )
        .unwrap();
        let mut f = std::fs::File::create(dir.join("subagents/agent-new1.jsonl")).unwrap();
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{{"id":"m1","model":"claude-haiku-4-5","content":[{{"type":"tool_use","id":"t1","name":"Grep","input":{{}}}}],"usage":{{"output_tokens":7}}}}}}"#).unwrap();
        drop(f);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            w.poll();
            if w.agents.get("new1").is_some_and(|a| a.api_calls == 1) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let a = &w.agents["new1"];
        assert_eq!(a.agent_type, "Explore");
        assert_eq!(a.api_calls, 1);
        assert_eq!(a.usage.output, 7);
        assert_eq!(w.running(0), 1);
    }
}
