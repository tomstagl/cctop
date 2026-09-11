//! Typed, tolerant model of one Claude Code transcript line
//! (`~/.claude/projects/<cwd>/<session>.jsonl`).
//!
//! Parsing never fails on an unknown `type`: it becomes [`Line::Unknown`] so a
//! newer Claude Code cannot break the tailer. Only fields cctop uses are
//! modelled; everything else is ignored by serde.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

pub use crate::tail::{parse_file, Tailer};

/// One line of the transcript.
#[derive(Debug, Clone)]
pub enum Line {
    User(UserLine),
    Assistant(AssistantLine),
    System(SystemLine),
    CostState(CostState),
    PermissionMode(PermissionMode),
    QueueOperation(QueueOperation),
    FileHistorySnapshot(FileHistorySnapshot),
    Attachment(Attachment),
    /// Any `type` cctop does not model, kept verbatim.
    Unknown(Value),
}

impl Line {
    /// Parse one JSON line. Malformed JSON is an error; an unknown or
    /// unexpectedly shaped line is `Unknown`, never an error.
    pub fn parse(text: &str) -> Result<Line, serde_json::Error> {
        let raw: Value = serde_json::from_str(text)?;
        Ok(Self::from_value(raw))
    }

    pub fn from_value(raw: Value) -> Line {
        let ty = raw.get("type").and_then(Value::as_str).unwrap_or("");
        fn as_<T: for<'de> Deserialize<'de>>(raw: &Value) -> Option<T> {
            serde_json::from_value(raw.clone()).ok()
        }
        let parsed = match ty {
            "user" => as_(&raw).map(Line::User),
            "assistant" => as_(&raw).map(Line::Assistant),
            "system" => as_(&raw).map(Line::System),
            "cost-state" => as_(&raw).map(Line::CostState),
            "permission-mode" => as_(&raw).map(Line::PermissionMode),
            "queue-operation" => as_(&raw).map(Line::QueueOperation),
            "file-history-snapshot" => as_(&raw).map(Line::FileHistorySnapshot),
            "attachment" => as_(&raw).map(Line::Attachment),
            _ => None,
        };
        parsed.unwrap_or(Line::Unknown(raw))
    }

    /// The `type` string of this line.
    pub fn kind(&self) -> &str {
        match self {
            Line::User(_) => "user",
            Line::Assistant(_) => "assistant",
            Line::System(_) => "system",
            Line::CostState(_) => "cost-state",
            Line::PermissionMode(_) => "permission-mode",
            Line::QueueOperation(_) => "queue-operation",
            Line::FileHistorySnapshot(_) => "file-history-snapshot",
            Line::Attachment(_) => "attachment",
            Line::Unknown(v) => v.get("type").and_then(Value::as_str).unwrap_or("?"),
        }
    }
}

// ---------------------------------------------------------------- user

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLine {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    #[serde(default)]
    pub is_meta: bool,
    #[serde(default)]
    pub is_sidechain: bool,
    pub message: UserMessage,
    /// Structured result Claude Code attached to a tool_result line
    /// (`stdout`/`stderr`/`file`/`structuredPatch`…). Shape varies per tool,
    /// so it is kept raw.
    #[serde(default)]
    pub tool_use_result: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserMessage {
    #[serde(default)]
    pub content: Content,
}

/// User content is either a plain string (a typed prompt) or content blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<UserBlock>),
}

impl Default for Content {
    fn default() -> Self {
        Content::Text(String::new())
    }
}

impl Content {
    pub fn tool_results(&self) -> impl Iterator<Item = &ToolResult> {
        let blocks = match self {
            Content::Blocks(b) => b.as_slice(),
            Content::Text(_) => &[],
        };
        blocks.iter().filter_map(|b| match b {
            UserBlock::ToolResult(r) => Some(r),
            _ => None,
        })
    }

    /// Human-typed text, if any.
    pub fn text(&self) -> String {
        match self {
            Content::Text(t) => t.clone(),
            Content::Blocks(b) => b
                .iter()
                .filter_map(|b| match b {
                    UserBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum UserBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_result")]
    ToolResult(ToolResult),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub is_error: bool,
}

impl ToolResult {
    /// Text of the result as the model sees it (string, or joined text blocks).
    pub fn text(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(items) => items
                .iter()
                .filter_map(|i| i.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

// ----------------------------------------------------------- assistant

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantLine {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub request_id: Option<String>,
    /// Effort level in force for this response (`low`/`medium`/`high`…).
    pub effort: Option<String>,
    #[serde(default)]
    pub is_sidechain: bool,
    pub message: AssistantMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    /// API message id. One response is written as several lines (one per
    /// content block) that share this id — usage must be counted once per id.
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub content: Vec<AssistantBlock>,
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub output_tokens_details: OutputDetails,
    #[serde(default)]
    pub cache_creation: CacheCreation,
    pub service_tier: Option<String>,
    /// `standard` or `fast` (fast mode).
    pub speed: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OutputDetails {
    #[serde(default)]
    pub thinking_tokens: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CacheCreation {
    #[serde(default)]
    pub ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    pub ephemeral_1h_input_tokens: u64,
}

impl Usage {
    /// Everything the model read this call = current context size.
    pub fn total_input(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }

    /// Prompt-cache TTL observed on this call, if any cache was written.
    pub fn cache_ttl(&self) -> Option<CacheTtl> {
        if self.cache_creation.ephemeral_1h_input_tokens > 0 {
            Some(CacheTtl::OneHour)
        } else if self.cache_creation.ephemeral_5m_input_tokens > 0
            || self.cache_creation_input_tokens > 0
        {
            Some(CacheTtl::FiveMinutes)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTtl {
    FiveMinutes,
    OneHour,
}

// -------------------------------------------------------------- system

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLine {
    pub timestamp: Option<String>,
    #[serde(default)]
    pub subtype: String,
    /// `turn_duration`: milliseconds of the turn that just ended.
    pub duration_ms: Option<u64>,
    /// `stop_hook_summary`: one entry per hook command that ran.
    #[serde(default)]
    pub hook_infos: Vec<HookInfo>,
    #[serde(default)]
    pub hook_errors: Vec<Value>,
    /// `away_summary` and free-form system messages.
    pub content: Option<String>,
    pub level: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    TurnDuration,
    StopHookSummary,
    AwaySummary,
    Other,
}

impl SystemLine {
    pub fn kind(&self) -> SystemKind {
        match self.subtype.as_str() {
            "turn_duration" => SystemKind::TurnDuration,
            "stop_hook_summary" => SystemKind::StopHookSummary,
            "away_summary" => SystemKind::AwaySummary,
            _ => SystemKind::Other,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInfo {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub duration_ms: u64,
}

// ---------------------------------------------------------- cost-state

/// Claude Code's own cumulative accounting for the session.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostState {
    #[serde(rename = "totalCostUSD", default)]
    pub total_cost_usd: f64,
    /// Milliseconds.
    #[serde(rename = "totalAPIDuration", default)]
    pub total_api_duration: u64,
    #[serde(rename = "totalAPIDurationWithoutRetries", default)]
    pub total_api_duration_without_retries: u64,
    #[serde(default)]
    pub total_tool_duration: u64,
    #[serde(default)]
    pub total_lines_added: u64,
    #[serde(default)]
    pub total_lines_removed: u64,
    #[serde(default)]
    pub total_duration: u64,
    #[serde(default)]
    pub model_usage: BTreeMap<String, ModelUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(rename = "costUSD", default)]
    pub cost_usd: f64,
}

// ---------------------------------------------------------------- misc

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionMode {
    pub permission_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueueOperation {
    pub operation: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistorySnapshot {
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attachment {
    pub timestamp: Option<String>,
    #[serde(default)]
    pub attachment: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    fn fixture() -> Vec<Line> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        std::fs::read_to_string(p)
            .unwrap()
            .lines()
            .map(|l| Line::parse(l).expect("valid json"))
            .collect()
    }

    #[test]
    fn every_line_parses_to_a_known_variant_except_the_expected_set() {
        let expected_unknown = [
            "mode",
            "atis-latch",
            "bridge-session",
            "last-prompt",
            "ai-title",
            "file-history-delta",
        ];
        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in fixture() {
            if let Line::Unknown(v) = &line {
                let ty = v["type"].as_str().unwrap();
                assert!(
                    expected_unknown.contains(&ty),
                    "unexpected Unknown line type {ty}"
                );
            }
            *counts.entry(line.kind().to_string()).or_default() += 1;
        }
        // Nothing modelled fell through to Unknown.
        for ty in [
            "user",
            "assistant",
            "system",
            "cost-state",
            "permission-mode",
            "queue-operation",
            "file-history-snapshot",
            "attachment",
        ] {
            assert!(counts[ty] > 0, "{ty} missing");
        }
        assert_eq!(counts["assistant"], 386);
        assert_eq!(counts["user"], 276);
        assert_eq!(counts["cost-state"], 1);
    }

    #[test]
    fn assistant_fields_and_usage() {
        let lines = fixture();
        let first = lines
            .iter()
            .find_map(|l| match l {
                Line::Assistant(a) => Some(a),
                _ => None,
            })
            .unwrap();
        assert!(first.message.id.starts_with("msg_"));
        assert!(!first.message.model.is_empty());
        assert!(first.timestamp.is_some());
        assert!(first.request_id.is_some());
        assert_eq!(first.effort.as_deref(), Some("medium"));
        let u = &first.message.usage;
        assert_eq!(u.input_tokens, 2);
        assert_eq!(u.cache_creation_input_tokens, 60582);
        assert_eq!(u.cache_creation.ephemeral_1h_input_tokens, 60582);
        assert_eq!(u.output_tokens_details.thinking_tokens, 31);
        assert_eq!(u.speed.as_deref(), Some("standard"));
        assert_eq!(u.service_tier.as_deref(), Some("standard"));
        assert_eq!(u.cache_ttl(), Some(CacheTtl::OneHour));
        assert_eq!(u.total_input(), 60584);
    }

    #[test]
    fn one_response_spans_thirteen_lines_with_the_same_id() {
        let mut per_id: HashMap<String, usize> = HashMap::new();
        for l in fixture() {
            if let Line::Assistant(a) = l {
                *per_id.entry(a.message.id).or_default() += 1;
            }
        }
        assert_eq!(per_id.len(), 143);
        assert_eq!(*per_id.values().max().unwrap(), 13);
    }

    #[test]
    fn content_blocks_are_typed() {
        let mut tool_uses = 0;
        let mut thinking = 0;
        let mut results = 0;
        let mut with_tool_use_result = 0;
        for l in fixture() {
            match l {
                Line::Assistant(a) => {
                    for b in &a.message.content {
                        match b {
                            AssistantBlock::ToolUse { id, name, input } => {
                                assert!(id.starts_with("toolu_"));
                                assert!(!name.is_empty());
                                assert!(input.is_object());
                                tool_uses += 1;
                            }
                            AssistantBlock::Thinking { .. } => thinking += 1,
                            _ => {}
                        }
                    }
                }
                Line::User(u) => {
                    results += u.message.content.tool_results().count();
                    if u.tool_use_result.is_some() {
                        with_tool_use_result += 1;
                    }
                }
                _ => {}
            }
        }
        assert_eq!(tool_uses, 257);
        assert_eq!(results, 257);
        assert_eq!(thinking, 73);
        assert_eq!(with_tool_use_result, 257);
    }

    #[test]
    fn system_subtypes_and_cost_state() {
        let lines = fixture();
        let mut td = 0;
        let mut hooks = 0;
        let mut away = 0;
        for l in &lines {
            if let Line::System(s) = l {
                match s.kind() {
                    SystemKind::TurnDuration => {
                        assert!(s.duration_ms.unwrap() > 0);
                        td += 1;
                    }
                    SystemKind::StopHookSummary => {
                        assert!(!s.hook_infos.is_empty());
                        hooks += 1;
                    }
                    SystemKind::AwaySummary => {
                        assert!(s.content.is_some());
                        away += 1;
                    }
                    SystemKind::Other => {}
                }
            }
        }
        assert_eq!((td, hooks, away), (9, 9, 1));
        let cs = lines
            .iter()
            .find_map(|l| match l {
                Line::CostState(c) => Some(c),
                _ => None,
            })
            .unwrap();
        assert!(cs.total_cost_usd > 9.0 && cs.total_cost_usd < 10.0);
        assert!(cs.total_api_duration >= cs.total_api_duration_without_retries);
        assert!(cs.model_usage.contains_key("claude-sonnet-5"));
        assert_eq!(cs.total_lines_added, 13);
    }

    #[test]
    fn plain_string_user_content_and_unknown_type() {
        let l = Line::parse(
            r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"t"}"#,
        )
        .unwrap();
        match l {
            Line::User(u) => assert_eq!(u.message.content.text(), "hello"),
            _ => panic!(),
        }
        let l = Line::parse(r#"{"type":"brand-new-thing","x":1}"#).unwrap();
        assert!(matches!(l, Line::Unknown(_)));
        assert_eq!(l.kind(), "brand-new-thing");
        // Known type with an unexpected shape degrades to Unknown, not an error.
        let l = Line::parse(r#"{"type":"assistant","message":"not an object"}"#).unwrap();
        assert!(matches!(l, Line::Unknown(_)));
        assert!(Line::parse("{not json").is_err());
    }
}
