//! The `<task-notification>` a background task sends back when it ends: an
//! `Agent` launched with `async_launched`, a background Bash command, a
//! `Workflow` run. Claude Code delivers it three ways, all carrying the same
//! text — as a `user` line with `origin.kind = "task-notification"` when the
//! model is idle, or, when it is busy, as a `queue-operation` `enqueue` line
//! and later a `queued_command` attachment — so a collector keys what it
//! keeps by `task_id` and lets a repeat overwrite the same facts.
//!
//! Only element names, enum values, counts and lengths are taken; the text
//! of `<summary>`, `<result>`, `<note>` and `<output-file>` is never kept
//! (`crate::harness_facts::task_notification` has what was measured).

/// `<status>` of a finished task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    Failed,
    /// A `TaskStop`, or the session ended under it.
    Killed,
    /// A value not seen so far, kept as written.
    Other(String),
}

impl TaskStatus {
    fn parse(s: &str) -> TaskStatus {
        match s.trim() {
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            "killed" => TaskStatus::Killed,
            other => TaskStatus::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Killed => "killed",
            TaskStatus::Other(s) => s,
        }
    }
}

/// The `<usage>` children a `Workflow` run's notification carries instead
/// of a single agent's figures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct WorkflowNotification {
    pub agent_count: usize,
    pub done: usize,
    pub error: usize,
    pub skipped: usize,
    pub empty_result: usize,
}

/// One notification, without its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotification {
    /// The agent id for an `Agent` task, Claude Code's short task id for a
    /// background shell command or a workflow run.
    pub task_id: String,
    /// The launching `tool_use` id (absent on some agent notifications).
    pub tool_use_id: Option<String>,
    pub status: TaskStatus,
    /// Length of `<result>`: `None` when the element is absent, `Some(0)`
    /// when it is empty.
    pub result_chars: Option<usize>,
    pub summary_chars: usize,
    /// Claude Code's own figures (`<usage>`), optional on every version seen.
    pub subagent_tokens: Option<u64>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Present on a `Workflow` run's notification.
    pub workflow: Option<WorkflowNotification>,
}

impl TaskNotification {
    /// Parse the text of one notification; `None` unless it is one.
    pub fn parse(text: &str) -> Option<TaskNotification> {
        let t = text.trim_start();
        if !t.starts_with("<task-notification") {
            return None;
        }
        let body = element(t, "task-notification").unwrap_or(t);
        let task_id = element(body, "task-id")?.trim().to_string();
        let num = |name: &str| element(body, name).and_then(|v| v.trim().parse::<u64>().ok());
        let workflow = element(body, "agent_count").map(|_| WorkflowNotification {
            agent_count: num("agent_count").unwrap_or(0) as usize,
            done: num("agents_done").unwrap_or(0) as usize,
            error: num("agents_error").unwrap_or(0) as usize,
            skipped: num("agents_skipped").unwrap_or(0) as usize,
            empty_result: num("agents_empty_result").unwrap_or(0) as usize,
        });
        Some(TaskNotification {
            task_id,
            tool_use_id: element(body, "tool-use-id").map(|s| s.trim().to_string()),
            status: TaskStatus::parse(element(body, "status").unwrap_or("")),
            result_chars: result(body).map(|r| r.chars().count()),
            summary_chars: element(body, "summary").map_or(0, |s| s.chars().count()),
            subagent_tokens: num("subagent_tokens"),
            tool_uses: num("tool_uses"),
            duration_ms: num("duration_ms"),
            workflow,
        })
    }

    /// True for an `Agent`'s notification rather than a shell task's or a
    /// workflow run's, judged by the id Claude Code gives agents: 17 hex
    /// characters (`agent-<id>.jsonl`). A shell task's is 9 base-36
    /// characters; a workflow's carries `<agent_count>`.
    pub fn is_agent(&self) -> bool {
        self.workflow.is_none()
            && self.task_id.len() == crate::harness_facts::task_notification::AGENT_ID_HEX_LEN
            && self.task_id.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// The content of the first `<name>…</name>` in `s`.
fn element<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&close)? + start;
    Some(&s[start..end])
}

/// `<result>` may itself contain markup, so it ends at the *last*
/// `</result>`.
fn result(s: &str) -> Option<&str> {
    let start = s.find("<result>")? + "<result>".len();
    let end = s.rfind("</result>")?;
    (end >= start).then(|| &s[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &str = "<task-notification>\n<task-id>a2de915228d5f40fe</task-id>\n<tool-use-id>toolu_01ABC</tool-use-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n<summary>Agent \"Explore\" completed</summary>\n<note>the note</note>\n<result>Found <b>three</b> files.</result>\n<usage><subagent_tokens>75218</subagent_tokens><tool_uses>6</tool_uses><duration_ms>42499</duration_ms></usage>\n</task-notification>";

    #[test]
    fn agent_notification_fields_and_lengths() {
        let n = TaskNotification::parse(AGENT).unwrap();
        assert_eq!(n.task_id, "a2de915228d5f40fe");
        assert_eq!(n.tool_use_id.as_deref(), Some("toolu_01ABC"));
        assert_eq!(n.status, TaskStatus::Completed);
        assert_eq!(n.result_chars, Some("Found <b>three</b> files.".len()));
        assert_eq!(n.summary_chars, "Agent \"Explore\" completed".len());
        assert_eq!(n.subagent_tokens, Some(75218));
        assert_eq!(n.tool_uses, Some(6));
        assert_eq!(n.duration_ms, Some(42499));
        assert_eq!(n.workflow, None);
        assert!(n.is_agent());
    }

    #[test]
    fn statuses_and_the_optional_parts() {
        let mk = |status: &str, tail: &str| {
            TaskNotification::parse(&format!(
                "<task-notification><task-id>0123456789abcdef0</task-id><status>{status}</status><summary>s</summary>{tail}</task-notification>"
            ))
            .unwrap()
        };
        let failed = mk("failed", "");
        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(failed.result_chars, None, "no <result> element");
        assert_eq!(failed.subagent_tokens, None, "<usage> is optional");
        assert_eq!(failed.tool_use_id, None);
        assert!(failed.is_agent());
        let killed = mk("killed", "<result></result>");
        assert_eq!(killed.status, TaskStatus::Killed);
        assert_eq!(
            killed.result_chars,
            Some(0),
            "an empty result is not an absent one"
        );
        assert_eq!(mk("paused", "").status, TaskStatus::Other("paused".into()));
    }

    #[test]
    fn shell_task_and_workflow_notifications() {
        let shell = TaskNotification::parse(
            "<task-notification><task-id>baqaldmpb</task-id><tool-use-id>toolu_0171</tool-use-id><output-file>/tmp/t.output</output-file><status>failed</status><summary>Background command failed</summary></task-notification>",
        )
        .unwrap();
        assert!(!shell.is_agent(), "a 9-character task id is a shell task's");
        assert_eq!(shell.status, TaskStatus::Failed);
        let wf = TaskNotification::parse(
            "<task-notification><task-id>wquxwsh3</task-id><tool-use-id>toolu_0199</tool-use-id><status>completed</status><summary>Workflow done</summary><result>…</result><diagnostics>d</diagnostics><failures>f</failures><usage><agent_count>9</agent_count><agents_done>7</agents_done><agents_error>2</agents_error><agents_skipped>0</agents_skipped><agents_empty_result>1</agents_empty_result><subagent_tokens>1234567</subagent_tokens><tool_uses>310</tool_uses><duration_ms>900000</duration_ms></usage></task-notification>",
        )
        .unwrap();
        assert!(!wf.is_agent());
        assert_eq!(
            wf.workflow,
            Some(WorkflowNotification {
                agent_count: 9,
                done: 7,
                error: 2,
                skipped: 0,
                empty_result: 1
            })
        );
        assert_eq!(wf.subagent_tokens, Some(1_234_567));
    }

    #[test]
    fn not_a_notification() {
        assert!(TaskNotification::parse("hello").is_none());
        assert!(
            TaskNotification::parse("<task-notification><status>x</status></task-notification>")
                .is_none(),
            "no task id"
        );
        assert!(TaskNotification::parse(
            "  <task-notification><task-id>abc</task-id></task-notification>"
        )
        .is_some());
    }
}
