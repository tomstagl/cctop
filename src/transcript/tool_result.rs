//! `toolUseResult`: the structured result Claude Code attaches to a
//! tool_result line, typed per tool. The line does not name the tool, so the
//! shape is recognised by its keys (every shape below was checked on this
//! machine's transcripts; unknown shapes are [`ToolUseDetail::Other`]).
//! Output text is measured, never kept.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ToolUseDetail {
    Bash(BashResult),
    Edit(EditResult),
    Write(EditResult),
    Read(ReadResult),
    Agent(AgentResult),
    AskUserQuestion(AskResult),
    TaskCreate {
        task_id: Option<String>,
    },
    TaskUpdate(TaskUpdateResult),
    /// A `Workflow` run launched (`runId` names `subagents/workflows/<run>/`;
    /// `taskId` is what its task notification will carry).
    Workflow {
        task_id: Option<String>,
        run_id: Option<String>,
    },
    /// A denial or a plain error: Claude Code wrote the message as a string.
    Text {
        chars: usize,
    },
    Other,
}

impl ToolUseDetail {
    pub fn parse(v: &Value) -> ToolUseDetail {
        let Some(o) = v.as_object() else {
            return match v {
                Value::String(s) => ToolUseDetail::Text {
                    chars: s.chars().count(),
                },
                _ => ToolUseDetail::Other,
            };
        };
        let has = |k: &str| o.contains_key(k);
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| o.get(k).and_then(Value::as_u64);
        let b = |k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
        if has("stdout") && has("stderr") {
            let stdout = o.get("stdout").and_then(Value::as_str).unwrap_or("");
            let stderr = o.get("stderr").and_then(Value::as_str).unwrap_or("");
            return ToolUseDetail::Bash(BashResult {
                interrupted: b("interrupted"),
                timed_out_after_ms: n("timedOutAfterMs"),
                background_task_id: s("backgroundTaskId"),
                persisted_output_path: s("persistedOutputPath"),
                persisted_output_size: n("persistedOutputSize"),
                return_code_interpretation: s("returnCodeInterpretation"),
                stale_read_paths: o
                    .get("staleReadFileStateHint")
                    .and_then(Value::as_str)
                    .map(stale_paths)
                    .unwrap_or_default(),
                git_operation: o.get("gitOperation").map(GitOperation::parse),
                bash_edit_files: o
                    .get("bashEditDiff")
                    .and_then(|d| d.get("changedFiles"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                stdout_chars: stdout.chars().count(),
                stderr_chars: stderr.chars().count(),
                test_marker: TestMarker::from_output(stdout, stderr),
            });
        }
        if has("oldString") || (has("filePath") && has("structuredPatch") && has("content")) {
            let (added, removed) = patch_lines(o.get("structuredPatch"));
            let r = EditResult {
                file_path: s("filePath").unwrap_or_default(),
                new_file: o.get("originalFile").is_none_or(Value::is_null),
                stale_recovered: b("staleRecovered"),
                user_modified: b("userModified"),
                lines_added: added,
                lines_removed: removed,
            };
            return if has("oldString") {
                ToolUseDetail::Edit(r)
            } else {
                ToolUseDetail::Write(r)
            };
        }
        if has("file") && has("type") {
            let f = o.get("file").and_then(Value::as_object);
            let fs = |k: &str| {
                f.and_then(|f| f.get(k))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let fnum = |k: &str| f.and_then(|f| f.get(k)).and_then(Value::as_u64);
            let dims = f.and_then(|f| f.get("dimensions")).and_then(|d| {
                Some((
                    d.get("originalWidth")?.as_u64()?,
                    d.get("originalHeight")?.as_u64()?,
                ))
            });
            let kind = match o.get("type").and_then(Value::as_str) {
                Some("text") => ReadKind::Text,
                Some("image") => ReadKind::Image,
                Some("file_unchanged") => ReadKind::FileUnchanged,
                Some("pdf") => ReadKind::Pdf,
                _ => ReadKind::Other,
            };
            return ToolUseDetail::Read(ReadResult {
                kind,
                file_path: fs("filePath").unwrap_or_default(),
                num_lines: fnum("numLines"),
                start_line: fnum("startLine"),
                total_lines: fnum("totalLines"),
                truncated_by_token_cap: f
                    .and_then(|f| f.get("truncatedByTokenCap"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                content_chars: fs("content").map(|c| c.chars().count()).unwrap_or(0),
                image_dimensions: dims,
            });
        }
        if has("questions") && has("answers") {
            let count = |k: &str| match o.get(k) {
                Some(Value::Array(a)) => a.len(),
                Some(Value::Object(m)) => m.len(),
                _ => 0,
            };
            return ToolUseDetail::AskUserQuestion(AskResult {
                questions: count("questions"),
                answers: count("answers"),
            });
        }
        if has("task") {
            return ToolUseDetail::TaskCreate {
                task_id: o
                    .get("task")
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
        }
        if has("taskId") && has("updatedFields") {
            let change = o
                .get("statusChange")
                .and_then(Value::as_object)
                .and_then(|c| {
                    Some((
                        c.get("from")?.as_str()?.to_string(),
                        c.get("to")?.as_str()?.to_string(),
                    ))
                });
            return ToolUseDetail::TaskUpdate(TaskUpdateResult {
                task_id: s("taskId").unwrap_or_default(),
                status_change: change,
                success: b("success"),
            });
        }
        if has("status") && has("runId") {
            return ToolUseDetail::Workflow {
                task_id: s("taskId"),
                run_id: s("runId"),
            };
        }
        if has("status") && (has("agentId") || has("agent_id") || has("resolvedModel")) {
            let usage = o
                .get("usage")
                .and_then(|u| serde_json::from_value::<super::Usage>(u.clone()).ok());
            return ToolUseDetail::Agent(AgentResult {
                status: s("status").unwrap_or_default(),
                agent_id: s("agentId").or_else(|| s("agent_id")),
                agent_type: s("agent_type"),
                resolved_model: s("resolvedModel").or_else(|| s("model")),
                name: s("name"),
                team_name: s("team_name"),
                is_async: b("isAsync"),
                total_tool_use_count: n("totalToolUseCount"),
                usage: usage.map(|u| crate::metrics::Usage::from_api(&u)),
                // A string, or blocks (`[{type: text, text}]`) on newer shapes.
                result_chars: match o.get("content") {
                    Some(Value::String(c)) => c.chars().count(),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .map(|t| t.chars().count())
                        .sum(),
                    _ => 0,
                },
            });
        }
        ToolUseDetail::Other
    }
}

/// `[This command modified 2 files you've previously read: a.rs, b.rs]` →
/// the paths.
fn stale_paths(hint: &str) -> Vec<String> {
    let Some(idx) = hint.find("previously read:") else {
        return Vec::new();
    };
    hint[idx + "previously read:".len()..]
        .trim_end_matches(']')
        .split(',')
        .map(|p| p.trim().trim_end_matches('.').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// `+`/`-` line counts over a `structuredPatch`.
fn patch_lines(patch: Option<&Value>) -> (u64, u64) {
    let mut added = 0;
    let mut removed = 0;
    if let Some(Value::Array(hunks)) = patch {
        for h in hunks {
            if let Some(Value::Array(lines)) = h.get("lines") {
                for l in lines.iter().filter_map(Value::as_str) {
                    if l.starts_with('+') {
                        added += 1;
                    } else if l.starts_with('-') {
                        removed += 1;
                    }
                }
            }
        }
    }
    (added, removed)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BashResult {
    pub interrupted: bool,
    pub timed_out_after_ms: Option<u64>,
    /// Set when the command was (auto-)backgrounded.
    pub background_task_id: Option<String>,
    /// Output spilled to `tool-results/`: only a head stays in context.
    pub persisted_output_path: Option<String>,
    pub persisted_output_size: Option<u64>,
    /// Claude Code's reading of a non-zero exit (`No matches found`…).
    pub return_code_interpretation: Option<String>,
    /// Files this command changed that the model had read before.
    pub stale_read_paths: Vec<String>,
    pub git_operation: Option<GitOperation>,
    /// Files a Bash command wrote, per `bashEditDiff`.
    pub bash_edit_files: Vec<String>,
    pub stdout_chars: usize,
    pub stderr_chars: usize,
    /// What the output says about a test run, if anything.
    pub test_marker: TestMarker,
}

/// A test runner's verdict read from stdout/stderr (`test result: ok.`,
/// `12 passed`, `FAILED`); the phase classifier confirms a test-class
/// command with it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TestMarker {
    #[default]
    None,
    Passed,
    Failed,
}

impl TestMarker {
    pub fn from_output(stdout: &str, stderr: &str) -> TestMarker {
        let mut marker = TestMarker::None;
        for text in [stdout, stderr] {
            for line in text.lines() {
                let l = line.trim();
                let failed = l.starts_with("test result: FAILED")
                    || l.starts_with("FAILED")
                    || l.starts_with("# fail ") && !l.starts_with("# fail 0")
                    || l.contains(" failed")
                        && !l.contains(" 0 failed")
                        && (l.contains("passed") || l.starts_with("Tests:"))
                    || l.starts_with("FAIL ");
                let passed = l.starts_with("test result: ok")
                    || l.starts_with("# pass ")
                    || l.starts_with("PASS ")
                    || (l.contains(" passed") && (l.contains("0 failed") || !l.contains("failed")))
                    || l.starts_with("ok ");
                if failed {
                    return TestMarker::Failed;
                }
                if passed {
                    marker = TestMarker::Passed;
                }
            }
        }
        marker
    }
}

/// `gitOperation`: what a Bash command did to the repository, as Claude Code
/// summarised it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitOperation {
    /// `(sha, branch)` of a commit.
    pub commit: Option<(String, String)>,
    /// Branch pushed.
    pub push: Option<String>,
    /// `(number, action)` of a pull request (`created`, `ready`, `merged`…).
    pub pr: Option<(u64, String)>,
    /// `(ref, action)` of a branch operation (`merged`, `created`, `deleted`).
    pub branch: Option<(String, String)>,
}

impl GitOperation {
    pub fn parse(v: &Value) -> GitOperation {
        let s = |v: Option<&Value>, k: &str| {
            v.and_then(|v| v.get(k))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        GitOperation {
            commit: v
                .get("commit")
                .and_then(|c| Some((s(Some(c), "sha")?, s(Some(c), "branch").unwrap_or_default()))),
            push: s(v.get("push"), "branch"),
            pr: v.get("pr").and_then(|p| {
                Some((
                    p.get("number")?.as_u64()?,
                    s(Some(p), "action").unwrap_or_default(),
                ))
            }),
            branch: v
                .get("branch")
                .and_then(|b| Some((s(Some(b), "ref")?, s(Some(b), "action").unwrap_or_default()))),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditResult {
    pub file_path: String,
    /// `originalFile` was null: the tool created the file.
    pub new_file: bool,
    /// The file had changed under the model; Claude Code re-read and applied.
    pub stale_recovered: bool,
    pub user_modified: bool,
    pub lines_added: u64,
    pub lines_removed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKind {
    Text,
    Image,
    Pdf,
    /// The file was unchanged since the last read: ~30 tokens went into
    /// context, not the file.
    FileUnchanged,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    pub kind: ReadKind,
    pub file_path: String,
    pub num_lines: Option<u64>,
    pub start_line: Option<u64>,
    pub total_lines: Option<u64>,
    pub truncated_by_token_cap: bool,
    pub content_chars: usize,
    /// `(width, height)` of an image.
    pub image_dimensions: Option<(u64, u64)>,
}

impl ReadResult {
    /// A read with offset/limit that did not cover the whole file.
    pub fn is_ranged(&self) -> bool {
        match (self.num_lines, self.start_line, self.total_lines) {
            (Some(n), Some(s), Some(t)) => s > 1 || n < t,
            _ => false,
        }
    }

    /// Tokens an image costs the context: `w·h/750`, 1 500 when unknown.
    pub fn image_tokens(&self) -> Option<u64> {
        match self.kind {
            ReadKind::Image => Some(
                self.image_dimensions
                    .map(|(w, h)| w * h / 750)
                    .unwrap_or(1_500),
            ),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentResult {
    /// `async_launched`, `teammate_spawned`, `completed`…
    pub status: String,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub resolved_model: Option<String>,
    /// `teammate_spawned` only: the member's name (a role) and its team
    /// (`session-<lead id8>`), the keys its own transcript carries.
    pub name: Option<String>,
    pub team_name: Option<String>,
    pub is_async: bool,
    pub total_tool_use_count: Option<u64>,
    /// Present on synchronous completions.
    pub usage: Option<crate::metrics::Usage>,
    pub result_chars: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskResult {
    pub questions: usize,
    pub answers: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskUpdateResult {
    pub task_id: String,
    /// `(from, to)` when the status changed.
    pub status_change: Option<(String, String)>,
    pub success: bool,
}

impl TaskUpdateResult {
    pub fn completed(&self) -> bool {
        self.status_change
            .as_ref()
            .is_some_and(|(_, to)| to == "completed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::tests_support::fixture;
    use crate::transcript::{AssistantBlock, Line};
    use std::collections::HashMap;

    fn parse(json: &str) -> ToolUseDetail {
        ToolUseDetail::parse(&serde_json::from_str(json).unwrap())
    }

    #[test]
    fn bash_shape() {
        let d = parse(
            r#"{"stdout":"running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false,"gitOperation":{"commit":{"sha":"c95bdc9","kind":"committed","branch":"main"},"push":{"branch":"main"}},"persistedOutputPath":"/x/tool-results/abc.txt","persistedOutputSize":741234,"staleReadFileStateHint":"[This command modified 2 files you've previously read: src/a.rs, src/b.rs]","bashEditDiff":{"files":[],"moreFiles":0,"changedFiles":["/p/src/a.rs"]},"returnCodeInterpretation":"No matches found","backgroundTaskId":"b7f3","timedOutAfterMs":120000}"#,
        );
        let ToolUseDetail::Bash(b) = d else {
            panic!("{d:?}")
        };
        assert_eq!(b.test_marker, TestMarker::Passed);
        assert_eq!(
            b.git_operation.as_ref().unwrap().commit,
            Some(("c95bdc9".into(), "main".into()))
        );
        assert_eq!(
            b.git_operation.as_ref().unwrap().push.as_deref(),
            Some("main")
        );
        assert_eq!(b.persisted_output_size, Some(741_234));
        assert_eq!(b.stale_read_paths, ["src/a.rs", "src/b.rs"]);
        assert_eq!(b.bash_edit_files, ["/p/src/a.rs"]);
        assert_eq!(
            b.return_code_interpretation.as_deref(),
            Some("No matches found")
        );
        assert_eq!(b.background_task_id.as_deref(), Some("b7f3"));
        assert_eq!(b.timed_out_after_ms, Some(120_000));
        assert!(b.stdout_chars > 50);
        assert_eq!(
            TestMarker::from_output(
                "test result: FAILED. 196 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out",
                ""
            ),
            TestMarker::Failed
        );
        assert_eq!(
            TestMarker::from_output("# tests 121\n# pass 121\n# fail 0", ""),
            TestMarker::Passed
        );
        assert_eq!(
            TestMarker::from_output("# tests 121\n# pass 117\n# fail 4", ""),
            TestMarker::Failed
        );
        assert_eq!(
            TestMarker::from_output("12 passed in 0.4s", ""),
            TestMarker::Passed
        );
        assert_eq!(
            TestMarker::from_output("1 failed, 11 passed in 0.4s", ""),
            TestMarker::Failed
        );
        assert_eq!(
            TestMarker::from_output("Tests:       3 failed, 40 passed, 43 total", ""),
            TestMarker::Failed
        );
        assert_eq!(
            TestMarker::from_output("just some output", "warning: unused"),
            TestMarker::None
        );
        let g = GitOperation::parse(
            &serde_json::json!({"pr":{"number":70,"url":"u","action":"created"},"branch":{"ref":"feat","action":"merged"}}),
        );
        assert_eq!(g.pr, Some((70, "created".into())));
        assert_eq!(g.branch, Some(("feat".into(), "merged".into())));
    }

    #[test]
    fn edit_write_read_shapes() {
        let e = parse(
            r#"{"filePath":"/p/src/a.rs","oldString":"a","newString":"b","originalFile":"a","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":2,"lines":["-a","+b","+c"]}],"userModified":false,"replaceAll":false,"staleRecovered":true}"#,
        );
        let ToolUseDetail::Edit(e) = e else { panic!() };
        assert_eq!((e.lines_added, e.lines_removed), (2, 1));
        assert!(e.stale_recovered && !e.new_file);
        let w = parse(
            r#"{"type":"create","filePath":"/p/src/n.rs","content":"x","structuredPatch":[],"originalFile":null,"userModified":false}"#,
        );
        let ToolUseDetail::Write(w) = w else {
            panic!("{w:?}")
        };
        assert!(w.new_file);
        let r = parse(
            r#"{"type":"text","file":{"filePath":"/p/src/a.rs","content":"…","numLines":120,"startLine":1,"totalLines":615}}"#,
        );
        let ToolUseDetail::Read(r) = r else { panic!() };
        assert_eq!(r.kind, ReadKind::Text);
        assert!(r.is_ranged());
        assert_eq!(r.image_tokens(), None);
        let r = parse(
            r#"{"type":"text","file":{"filePath":"/p/a.rs","content":"…","numLines":10,"startLine":1,"totalLines":10,"truncatedByTokenCap":true}}"#,
        );
        let ToolUseDetail::Read(r) = r else { panic!() };
        assert!(!r.is_ranged() && r.truncated_by_token_cap);
        let u = parse(r#"{"type":"file_unchanged","file":{"filePath":"/p/a.rs"}}"#);
        assert!(matches!(
            u,
            ToolUseDetail::Read(ReadResult {
                kind: ReadKind::FileUnchanged,
                ..
            })
        ));
        let i = parse(
            r#"{"type":"image","file":{"base64":"…","type":"image/png","originalSize":334389,"dimensions":{"originalWidth":992,"originalHeight":992,"displayWidth":992,"displayHeight":992}}}"#,
        );
        let ToolUseDetail::Read(i) = i else { panic!() };
        assert_eq!(i.image_tokens(), Some(992 * 992 / 750));
        let i = parse(r#"{"type":"image","file":{"base64":"…","type":"image/png"}}"#);
        let ToolUseDetail::Read(i) = i else { panic!() };
        assert_eq!(i.image_tokens(), Some(1_500));
    }

    #[test]
    fn agent_task_ask_and_text_shapes() {
        let a = parse(
            r#"{"isAsync":true,"status":"async_launched","agentId":"a3f5","description":"d","resolvedModel":"claude-haiku-4-5-20251001","prompt":"p","outputFile":"o","canReadOutputFile":true}"#,
        );
        let ToolUseDetail::Agent(a) = a else { panic!() };
        assert_eq!(
            a.resolved_model.as_deref(),
            Some("claude-haiku-4-5-20251001")
        );
        assert!(a.is_async && a.usage.is_none());
        let t = parse(
            r#"{"status":"teammate_spawned","prompt":"p","teammate_id":"n@session-83f0e9b9","agent_id":"n@session-83f0e9b9","agent_type":"claude-code-guide","model":"haiku","name":"n","team_name":"session-83f0e9b9"}"#,
        );
        let ToolUseDetail::Agent(t) = t else { panic!() };
        assert_eq!(t.agent_type.as_deref(), Some("claude-code-guide"));
        assert_eq!(t.resolved_model.as_deref(), Some("haiku"));
        assert_eq!(t.name.as_deref(), Some("n"));
        assert_eq!(t.team_name.as_deref(), Some("session-83f0e9b9"));
        assert!(a.name.is_none() && a.team_name.is_none());
        let done = parse(
            r#"{"status":"completed","agentId":"a","totalToolUseCount":12,"usage":{"input_tokens":5,"cache_read_input_tokens":1000,"output_tokens":71,"output_tokens_details":{"thinking_tokens":10}},"content":"summary"}"#,
        );
        let ToolUseDetail::Agent(done) = done else {
            panic!()
        };
        assert_eq!(done.total_tool_use_count, Some(12));
        assert_eq!(done.usage.unwrap().output, 71);
        assert_eq!(done.result_chars, 7);
        let q = parse(r#"{"questions":[{"q":1},{"q":2}],"answers":{"a":"x"}}"#);
        assert_eq!(
            q,
            ToolUseDetail::AskUserQuestion(AskResult {
                questions: 2,
                answers: 1
            })
        );
        let c = parse(r#"{"task":{"id":"7","subject":"s"}}"#);
        assert_eq!(
            c,
            ToolUseDetail::TaskCreate {
                task_id: Some("7".into())
            }
        );
        let u = parse(
            r#"{"success":true,"taskId":"7","updatedFields":["status"],"statusChange":{"from":"in_progress","to":"completed"}}"#,
        );
        let ToolUseDetail::TaskUpdate(u) = u else {
            panic!()
        };
        assert!(u.completed());
        assert_eq!(
            parse(r#""denied by rule""#),
            ToolUseDetail::Text { chars: 14 }
        );
        assert_eq!(parse(r#"[1,2]"#), ToolUseDetail::Other);
        assert_eq!(parse(r#"{"weird":1}"#), ToolUseDetail::Other);
    }

    #[test]
    fn fixture_b_results_are_recognised_per_tool() {
        let lines = fixture("session-b");
        let mut names: HashMap<String, String> = HashMap::new();
        let mut per_tool: HashMap<String, HashMap<&'static str, usize>> = HashMap::new();
        let mut tests_seen = (0, 0);
        let mut commits = 0;
        let mut stale = 0;
        for l in &lines {
            match l {
                Line::Assistant(a) => {
                    for b in &a.message.content {
                        if let AssistantBlock::ToolUse { id, name, .. } = b {
                            names.insert(id.clone(), name.clone());
                        }
                    }
                }
                Line::User(u) => {
                    let Some(detail) = u.tool_use_detail() else {
                        continue;
                    };
                    let Some(r) = u.message.content.tool_results().next() else {
                        continue;
                    };
                    let name = names.get(&r.tool_use_id).cloned().unwrap_or_default();
                    let label = match &detail {
                        ToolUseDetail::Bash(b) => {
                            match b.test_marker {
                                TestMarker::Passed => tests_seen.0 += 1,
                                TestMarker::Failed => tests_seen.1 += 1,
                                TestMarker::None => {}
                            }
                            if b.git_operation.as_ref().is_some_and(|g| g.commit.is_some()) {
                                commits += 1;
                            }
                            stale += b.stale_read_paths.len();
                            "bash"
                        }
                        ToolUseDetail::Edit(_) => "edit",
                        ToolUseDetail::Write(_) => "write",
                        ToolUseDetail::Read(_) => "read",
                        ToolUseDetail::Agent(_) => "agent",
                        ToolUseDetail::AskUserQuestion(_) => "ask",
                        ToolUseDetail::TaskCreate { .. } => "task_create",
                        ToolUseDetail::TaskUpdate(_) => "task_update",
                        ToolUseDetail::Workflow { .. } => "workflow",
                        ToolUseDetail::Text { .. } => "text",
                        ToolUseDetail::Other => "other",
                    };
                    *per_tool.entry(name).or_default().entry(label).or_default() += 1;
                }
                _ => {}
            }
        }
        // Every Bash/Edit/Write/Read result is recognised as its own tool.
        assert_eq!(
            per_tool["Edit"].keys().copied().collect::<Vec<_>>(),
            ["edit"]
        );
        assert!(
            per_tool["Write"].contains_key("write"),
            "{:?}",
            per_tool["Write"]
        );
        assert!(per_tool["Read"].contains_key("read"));
        let bash = &per_tool["Bash"];
        assert!(bash["bash"] > 50, "{bash:?}");
        assert!(
            bash.get("text").copied().unwrap_or(0) >= 3,
            "denials are strings: {bash:?}"
        );
        assert_eq!(per_tool["AskUserQuestion"]["ask"], 1);
        assert_eq!(per_tool["TaskUpdate"]["task_update"], 1);
        assert!(tests_seen.0 >= 5 && tests_seen.1 >= 1, "{tests_seen:?}");
        assert_eq!(
            commits, 2,
            "the spliced gitOperation results; the spine's compound commits carry none"
        );
        assert!(
            stale >= 1,
            "a staleReadFileStateHint survives anonymisation"
        );
    }
}
