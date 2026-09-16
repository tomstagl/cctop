//! `attachment` lines: what the harness injected around the person's
//! prompts and the tool results. Since 2.1.266 the line carries
//! `rendered[].content`, the exact text the model saw; before that the cost
//! is estimated from the attachment's own JSON with a per-subtype ratio
//! measured on this machine's 2.1.266+ transcripts.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub attachment: Value,
    /// The text as sent, one entry per rendered block; only lengths are kept.
    #[serde(default, deserialize_with = "rendered_chars")]
    pub rendered: Option<usize>,
}

fn rendered_chars<'de, D>(d: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Block {
        #[serde(default)]
        content: String,
    }
    Ok(Option::<Vec<Block>>::deserialize(d)?
        .map(|v| v.iter().map(|b| b.content.chars().count()).sum()))
}

/// Per-subtype `rendered chars / attachment JSON bytes`, p50 over the
/// 2.1.266+ corpus, for lines without `rendered`. Subtypes that are records
/// rather than injections (a snapshot of the prefix, a permissions list)
/// cost nothing.
const FALLBACK_RATIO: &[(&str, f64)] = &[
    ("environment", 1.09),
    ("model", 0.62),
    ("deferred_tools_delta", 0.52),
    ("agent_listing_delta", 0.99),
    ("mcp_instructions_delta", 0.99),
    ("skill_listing", 0.92),
    ("auto_mode", 2.94),
    ("auto_mode_exit", 4.65),
    ("total_tokens_reminder", 0.91),
    ("session_context", 1.19),
    ("date", 1.68),
    ("date_change", 1.68),
    ("remote_session_change", 2.15),
    ("silent_turn_reminder", 0.92),
    ("edited_text_file", 0.99),
    ("instructions", 1.72),
    ("nested_memory", 1.0),
    ("queued_command", 1.55),
    ("hook_blocking_error", 0.89),
    ("hook_success", 0.32),
    ("hook_system_message", 1.0),
    ("bash_output_audience_note", 2.2),
    ("read_truncation_notice", 0.87),
    ("file", 0.98),
    ("directory", 1.8),
    ("plan_mode", 1.2),
    ("plan_mode_exit", 1.16),
    ("plan_file_reference", 1.0),
    ("ultra_effort_enter", 6.39),
    ("ultra_effort_exit", 3.83),
    ("batching_reminder_sent", 0.9),
    ("goal_status", 0.5),
    ("todo_reminder", 0.3),
    ("invoked_skills", 1.0),
    ("prompt_snapshot", 0.0),
    ("command_permissions", 0.0),
    ("deferred_tools_record", 0.0),
];

impl Attachment {
    /// A task notification delivered as a `queued_command` (the model was
    /// busy when the task ended); the queued prompt's text is not kept.
    pub fn task_notification(&self) -> Option<super::TaskNotification> {
        if self.subtype() != "queued_command" {
            return None;
        }
        self.attachment
            .get("prompt")
            .and_then(Value::as_str)
            .and_then(super::TaskNotification::parse)
    }

    /// The `attachment.type` string.
    pub fn subtype(&self) -> &str {
        self.attachment
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("?")
    }

    /// Tokens this injection occupies in context: exact-ish from `rendered`
    /// (chars / 4), else the per-subtype estimate. The flag says which.
    pub fn tokens_est(&self) -> (u64, bool) {
        if let Some(chars) = self.rendered {
            return ((chars / 4) as u64, false);
        }
        let json_len = self.attachment.to_string().len() as f64;
        let subtype = self.subtype();
        let ratio = FALLBACK_RATIO
            .iter()
            .find(|(s, _)| *s == subtype)
            .map(|(_, r)| *r)
            .unwrap_or(1.0);
        let chars = match subtype {
            // The list is boilerplate plus ~0.3 of the items' JSON.
            "task_reminder" => 450.0 + 0.3 * json_len,
            _ => json_len * ratio,
        };
        ((chars / 4.0) as u64, true)
    }

    /// The typed view of the subtypes cctop reads.
    pub fn kind(&self) -> AttachmentKind {
        let a = &self.attachment;
        let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| a.get(k).and_then(Value::as_u64);
        let b = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
        let chars = |k: &str| {
            a.get(k)
                .and_then(Value::as_str)
                .map(|t| t.chars().count())
                .unwrap_or(0)
        };
        let names = |k: &str| -> Vec<String> {
            a.get(k)
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        match self.subtype() {
            "task_reminder" => AttachmentKind::TaskReminder {
                item_count: n("itemCount").unwrap_or(0),
            },
            "todo_reminder" => AttachmentKind::TaskReminder {
                item_count: n("itemCount").unwrap_or(0),
            },
            "edited_text_file" => AttachmentKind::EditedTextFile {
                filename: s("filename").unwrap_or_default(),
                snippet_chars: chars("snippet"),
            },
            "hook_success" => AttachmentKind::HookSuccess {
                hook_event: s("hookEvent").unwrap_or_default(),
                command: s("command").unwrap_or_default(),
                duration_ms: n("durationMs").unwrap_or(0),
                exit_code: a.get("exitCode").and_then(Value::as_i64).unwrap_or(0),
            },
            "hook_blocking_error" => AttachmentKind::HookBlockingError {
                hook_event: s("hookEvent").unwrap_or_default(),
                hook_name: s("hookName").unwrap_or_default(),
            },
            "hook_system_message" => AttachmentKind::HookSystemMessage {
                hook_event: s("hookEvent").unwrap_or_default(),
            },
            "plan_mode" => AttachmentKind::PlanMode {
                plan_file_path: s("planFilePath"),
                plan_exists: b("planExists"),
            },
            "plan_mode_exit" => AttachmentKind::PlanModeExit {
                plan_file_path: s("planFilePath"),
                plan_exists: b("planExists"),
            },
            "plan_file_reference" => AttachmentKind::PlanFileReference {
                plan_file_path: s("planFilePath"),
                chars: chars("planContent"),
            },
            "goal_status" => AttachmentKind::GoalStatus {
                met: b("met"),
                tokens: n("tokens"),
                iterations: n("iterations"),
                duration_ms: n("durationMs"),
            },
            "queued_command" => AttachmentKind::QueuedCommand {
                command_mode: s("commandMode").unwrap_or_default(),
                origin_kind: a
                    .get("origin")
                    .and_then(|o| o.get("kind"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                prompt_chars: chars("prompt"),
            },
            "batching_reminder_sent" => AttachmentKind::BatchingReminderSent,
            "silent_turn_reminder" => AttachmentKind::SilentTurnReminder,
            "total_tokens_reminder" => AttachmentKind::TotalTokensReminder,
            "bash_output_audience_note" => AttachmentKind::BashOutputAudienceNote,
            "read_truncation_notice" => AttachmentKind::ReadTruncationNotice {
                tool_use_id: s("toolUseID").unwrap_or_default(),
            },
            "auto_mode" => AttachmentKind::AutoMode {
                bash_first: b("bashFirst"),
                steer_only: b("steerOnly"),
                bypass: b("bypass"),
            },
            "auto_mode_exit" => AttachmentKind::AutoModeExit,
            "prompt_snapshot" => AttachmentKind::PromptSnapshot {
                system_prompt_chars: a
                    .get("systemPrompt")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter_map(Value::as_str)
                            .map(|s| s.chars().count())
                            .collect()
                    })
                    .unwrap_or_default(),
                tools: a
                    .get("tools")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter_map(|t| {
                                Some((
                                    t.get("name")?.as_str()?.to_string(),
                                    t.get("schema").map(|s| s.to_string().len()).unwrap_or(0),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                cli_prefix_chars: chars("cliPrefix"),
            },
            "instructions" => AttachmentKind::Instructions {
                files: a
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .map(|f| InstructionFile {
                                path: f
                                    .get("path")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                kind: f
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                chars: f
                                    .get("content")
                                    .and_then(Value::as_str)
                                    .map(|c| c.chars().count())
                                    .unwrap_or(0),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "nested_memory" => AttachmentKind::NestedMemory {
                path: s("path").or_else(|| s("filename")),
                chars: chars("content"),
            },
            "invoked_skills" => AttachmentKind::InvokedSkills {
                skills: a
                    .get("skills")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter_map(|s| {
                                s.as_str().map(str::to_string).or_else(|| {
                                    s.get("name").and_then(Value::as_str).map(str::to_string)
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "skill_listing" => AttachmentKind::SkillListing {
                skill_count: n("skillCount").unwrap_or(0),
                chars: chars("content"),
            },
            "deferred_tools_delta" => AttachmentKind::DeferredToolsDelta {
                added: names("addedNames").len(),
                removed: names("removedNames").len(),
                pending_mcp_servers: names("pendingMcpServers"),
                failed_mcp_servers: names("failedMcpServers"),
                needs_auth_mcp_servers: names("needsAuthMcpServers"),
            },
            "mcp_instructions_delta" => AttachmentKind::McpInstructionsDelta {
                added: names("addedNames").len(),
                removed: names("removedNames").len(),
            },
            "command_permissions" => AttachmentKind::CommandPermissions {
                allowed_tools: names("allowedTools").len(),
            },
            "file" => AttachmentKind::File {
                filename: s("filename").unwrap_or_default(),
            },
            "environment" => AttachmentKind::Environment,
            "model" => AttachmentKind::Model,
            "date" | "date_change" => AttachmentKind::Date,
            "session_context" => AttachmentKind::SessionContext,
            "remote_session_change" => AttachmentKind::RemoteSessionChange,
            "agent_listing_delta" => AttachmentKind::AgentListingDelta,
            "deferred_tools_record" => AttachmentKind::DeferredToolsRecord,
            "ultra_effort_enter" => AttachmentKind::UltraEffortEnter,
            "ultra_effort_exit" => AttachmentKind::UltraEffortExit,
            "directory" => AttachmentKind::Directory,
            other => AttachmentKind::Other(other.to_string()),
        }
    }
}

/// Attachment subtypes as cctop reads them; text is measured, never kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentKind {
    /// The task list re-sent every few turns (up to ~5.6 k tokens).
    TaskReminder {
        item_count: u64,
    },
    /// A file the person edited in the IDE mid-session.
    EditedTextFile {
        filename: String,
        snippet_chars: usize,
    },
    HookSuccess {
        hook_event: String,
        command: String,
        duration_ms: u64,
        exit_code: i64,
    },
    HookBlockingError {
        hook_event: String,
        hook_name: String,
    },
    HookSystemMessage {
        hook_event: String,
    },
    PlanMode {
        plan_file_path: Option<String>,
        plan_exists: bool,
    },
    PlanModeExit {
        plan_file_path: Option<String>,
        plan_exists: bool,
    },
    PlanFileReference {
        plan_file_path: Option<String>,
        chars: usize,
    },
    GoalStatus {
        met: bool,
        tokens: Option<u64>,
        iterations: Option<u64>,
        duration_ms: Option<u64>,
    },
    /// A queued prompt folded into the running turn.
    QueuedCommand {
        command_mode: String,
        origin_kind: Option<String>,
        prompt_chars: usize,
    },
    BatchingReminderSent,
    SilentTurnReminder,
    TotalTokensReminder,
    BashOutputAudienceNote,
    ReadTruncationNotice {
        tool_use_id: String,
    },
    AutoMode {
        bash_first: bool,
        steer_only: bool,
        bypass: bool,
    },
    AutoModeExit,
    /// The fixed prefix as sent: system prompt sections and tool schemas
    /// (a record, not an injection).
    PromptSnapshot {
        system_prompt_chars: Vec<usize>,
        tools: Vec<(String, usize)>,
        cli_prefix_chars: usize,
    },
    Instructions {
        files: Vec<InstructionFile>,
    },
    NestedMemory {
        path: Option<String>,
        chars: usize,
    },
    InvokedSkills {
        skills: Vec<String>,
    },
    SkillListing {
        skill_count: u64,
        chars: usize,
    },
    DeferredToolsDelta {
        added: usize,
        removed: usize,
        pending_mcp_servers: Vec<String>,
        failed_mcp_servers: Vec<String>,
        needs_auth_mcp_servers: Vec<String>,
    },
    McpInstructionsDelta {
        added: usize,
        removed: usize,
    },
    CommandPermissions {
        allowed_tools: usize,
    },
    File {
        filename: String,
    },
    Environment,
    Model,
    Date,
    SessionContext,
    RemoteSessionChange,
    AgentListingDelta,
    DeferredToolsRecord,
    UltraEffortEnter,
    UltraEffortExit,
    Directory,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    pub path: String,
    /// `CLAUDE.md`, `AutoMem`, …
    pub kind: String,
    pub chars: usize,
}

impl AttachmentKind {
    /// Injected on every call as a reminder (as opposed to once at start).
    pub fn is_per_call(&self) -> bool {
        matches!(
            self,
            AttachmentKind::TotalTokensReminder
                | AttachmentKind::BashOutputAudienceNote
                | AttachmentKind::BatchingReminderSent
                | AttachmentKind::SilentTurnReminder
                | AttachmentKind::TaskReminder { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::tests_support::fixture;
    use crate::transcript::Line;
    use std::collections::HashMap;

    fn att(json: &str) -> Attachment {
        match Line::parse(json).unwrap() {
            Line::Attachment(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rendered_is_measured_and_fallbacks_apply() {
        let a = att(
            r#"{"type":"attachment","attachment":{"type":"task_reminder","content":[],"itemCount":0},"rendered":[{"content":"<system-reminder>The task list is empty.</system-reminder>"}],"timestamp":"t","version":"2.1.270"}"#,
        );
        assert_eq!(a.rendered, Some(58));
        assert_eq!(a.tokens_est(), (14, false));
        assert_eq!(a.kind(), AttachmentKind::TaskReminder { item_count: 0 });
        assert!(a.kind().is_per_call());
        let old = att(
            r#"{"type":"attachment","attachment":{"type":"total_tokens_reminder","text":"Total tokens used so far: 12,345 of 15,000,000"},"timestamp":"t","version":"2.1.263"}"#,
        );
        assert_eq!(old.rendered, None);
        let (t, approx) = old.tokens_est();
        assert!(approx && t > 10 && t < 40, "{t}");
        let snap = att(
            r#"{"type":"attachment","attachment":{"type":"prompt_snapshot","systemPrompt":["abc","defgh"],"tools":[{"name":"Bash","schema":{"name":"Bash","input_schema":{"type":"object"}}}],"cliPrefix":"xy"},"timestamp":"t","version":"2.1.269"}"#,
        );
        assert_eq!(snap.tokens_est(), (0, true), "a record, not an injection");
        assert_eq!(
            snap.kind(),
            AttachmentKind::PromptSnapshot {
                system_prompt_chars: vec![3, 5],
                tools: vec![(
                    "Bash".into(),
                    r#"{"input_schema":{"type":"object"},"name":"Bash"}"#.len()
                )],
                cli_prefix_chars: 2
            }
        );
    }

    #[test]
    fn typed_subtypes() {
        let k = |json: &str| {
            att(&format!(
                r#"{{"type":"attachment","attachment":{json},"timestamp":"t"}}"#
            ))
            .kind()
        };
        assert_eq!(
            k(r#"{"type":"edited_text_file","filename":"/p/x.md","snippet":"abcd"}"#),
            AttachmentKind::EditedTextFile {
                filename: "/p/x.md".into(),
                snippet_chars: 4
            }
        );
        assert_eq!(
            k(
                r#"{"type":"hook_success","hookName":"Stop","toolUseID":"u","hookEvent":"Stop","content":"c","stdout":"s","stderr":"","exitCode":0,"command":"cctop hook","durationMs":463}"#
            ),
            AttachmentKind::HookSuccess {
                hook_event: "Stop".into(),
                command: "cctop hook".into(),
                duration_ms: 463,
                exit_code: 0
            }
        );
        assert_eq!(
            k(
                r#"{"type":"hook_blocking_error","hookName":"Stop","toolUseID":"u","hookEvent":"Stop","blockingError":"x"}"#
            ),
            AttachmentKind::HookBlockingError {
                hook_event: "Stop".into(),
                hook_name: "Stop".into()
            }
        );
        assert_eq!(
            k(
                r#"{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"/p/plan.md","planExists":true}"#
            ),
            AttachmentKind::PlanMode {
                plan_file_path: Some("/p/plan.md".into()),
                plan_exists: true
            }
        );
        assert_eq!(
            k(
                r#"{"type":"goal_status","met":true,"condition":"c","reason":"r","iterations":1,"durationMs":781071,"tokens":60858}"#
            ),
            AttachmentKind::GoalStatus {
                met: true,
                tokens: Some(60858),
                iterations: Some(1),
                duration_ms: Some(781071)
            }
        );
        assert_eq!(
            k(
                r#"{"type":"queued_command","prompt":"steer me","source_uuid":"u","commandMode":"prompt","origin":{"kind":"human"},"timestamp":"t"}"#
            ),
            AttachmentKind::QueuedCommand {
                command_mode: "prompt".into(),
                origin_kind: Some("human".into()),
                prompt_chars: 8
            }
        );
        assert_eq!(
            k(
                r#"{"type":"auto_mode","autoModeConsentFlow":"x","bashFirst":true,"steerOnly":false,"bypass":false}"#
            ),
            AttachmentKind::AutoMode {
                bash_first: true,
                steer_only: false,
                bypass: false
            }
        );
        assert_eq!(
            k(
                r#"{"type":"instructions","files":[{"path":"/p/CLAUDE.md","type":"Project","content":"abc"}]}"#
            ),
            AttachmentKind::Instructions {
                files: vec![InstructionFile {
                    path: "/p/CLAUDE.md".into(),
                    kind: "Project".into(),
                    chars: 3
                }]
            }
        );
        assert_eq!(
            k(r#"{"type":"invoked_skills","skills":["design",{"name":"pr"}]}"#),
            AttachmentKind::InvokedSkills {
                skills: vec!["design".into(), "pr".into()]
            }
        );
        assert_eq!(
            k(
                r#"{"type":"deferred_tools_delta","addedNames":["a","b"],"removedNames":[],"pendingMcpServers":["p"],"failedMcpServers":[],"needsAuthMcpServers":["gh"]}"#
            ),
            AttachmentKind::DeferredToolsDelta {
                added: 2,
                removed: 0,
                pending_mcp_servers: vec!["p".into()],
                failed_mcp_servers: vec![],
                needs_auth_mcp_servers: vec!["gh".into()]
            }
        );
        assert_eq!(
            k(
                r#"{"type":"skill_listing","content":"xyz","skillCount":45,"isInitial":true,"names":[]}"#
            ),
            AttachmentKind::SkillListing {
                skill_count: 45,
                chars: 3
            }
        );
        assert_eq!(
            k(r#"{"type":"read_truncation_notice","banner":"b","toolUseID":"toolu_1"}"#),
            AttachmentKind::ReadTruncationNotice {
                tool_use_id: "toolu_1".into()
            }
        );
        assert_eq!(
            k(r#"{"type":"silent_turn_reminder","text":"t"}"#),
            AttachmentKind::SilentTurnReminder
        );
        assert_eq!(
            k(r#"{"type":"batching_reminder_sent","text":"t","model":"m"}"#),
            AttachmentKind::BatchingReminderSent
        );
        assert_eq!(
            k(r#"{"type":"something_new","x":1}"#),
            AttachmentKind::Other("something_new".into())
        );
    }

    #[test]
    fn fixture_b_attachments_are_all_typed_and_mostly_rendered() {
        let lines = fixture("session-b");
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut rendered = 0;
        let mut total = 0;
        let mut tokens = 0;
        for l in &lines {
            if let Line::Attachment(a) = l {
                total += 1;
                *counts.entry(a.subtype().to_string()).or_default() += 1;
                if a.rendered.is_some() {
                    rendered += 1;
                }
                tokens += a.tokens_est().0;
                if let AttachmentKind::Other(s) = a.kind() {
                    panic!("untyped attachment {s}");
                }
            }
        }
        assert_eq!(total, 132);
        assert_eq!(counts["total_tokens_reminder"], 104);
        assert_eq!(counts["edited_text_file"], 3);
        assert_eq!(counts["file"], 5, "the compaction's re-injections");
        assert!(rendered > 100, "{rendered}");
        assert!(tokens > 10_000 && tokens < 200_000, "{tokens}");
    }
}
