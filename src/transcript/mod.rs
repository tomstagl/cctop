//! Typed, tolerant model of one Claude Code transcript line
//! (`~/.claude/projects/<cwd>/<session>.jsonl`).
//!
//! Parsing never fails on an unknown `type`: it becomes [`Line::Unknown`] so a
//! newer Claude Code cannot break the tailer. Only fields cctop uses are
//! modelled; everything else is ignored by serde. Free text the user or the
//! model wrote is kept only for the life of the line — collectors take
//! lengths and booleans from it, never the text.
//!
//! Shapes were checked against 132 local transcripts written by Claude Code
//! 2.1.231 … 2.1.270; the version each field first appeared in is in
//! [`crate::harness_facts::first_seen`].

pub mod attachment;
pub mod local_command;
pub mod tool_result;

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use serde_json::Value;

pub use crate::tail::{parse_file, Tailer};
pub use attachment::{Attachment, AttachmentKind};
pub use local_command::{ContextCapture, LocalCommand};
pub use tool_result::{
    AgentResult, AskResult, BashResult, EditResult, GitOperation, ReadKind, ReadResult,
    TaskUpdateResult, TestMarker, ToolUseDetail,
};

/// One line of the transcript.
// Lines are transient (parsed, applied, dropped), so the size skew between
// variants costs nothing worth boxing for.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Line {
    User(UserLine),
    Assistant(AssistantLine),
    System(SystemLine),
    CostState(CostState),
    PermissionMode(PermissionMode),
    QueueOperation(QueueOperation),
    FileHistorySnapshot(FileHistorySnapshot),
    FileHistoryDelta(FileHistoryDelta),
    Attachment(Attachment),
    PrLink(PrLink),
    AiTitle(AiTitle),
    CustomTitle(CustomTitle),
    /// `/clear` on 2.1.270 leaves this in the old file: the session went on
    /// under another id.
    ContinuedIn(ContinuedIn),
    AgentSetting(AgentSetting),
    BridgeSession(BridgeSession),
    /// A `/loop` or cron wake-up (kept raw: the shape is not yet observed).
    ScheduledTaskFire(Value),
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
            "file-history-delta" => as_(&raw).map(Line::FileHistoryDelta),
            "attachment" => as_(&raw).map(Line::Attachment),
            "pr-link" => as_(&raw).map(Line::PrLink),
            "ai-title" => as_(&raw).map(Line::AiTitle),
            "custom-title" => as_(&raw).map(Line::CustomTitle),
            "continued-in" => as_(&raw).map(Line::ContinuedIn),
            "agent-setting" => as_(&raw).map(Line::AgentSetting),
            "bridge-session" => as_(&raw).map(Line::BridgeSession),
            "scheduled_task_fire" | "scheduled-task-fire" => {
                Some(Line::ScheduledTaskFire(raw.clone()))
            }
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
            Line::FileHistoryDelta(_) => "file-history-delta",
            Line::Attachment(_) => "attachment",
            Line::PrLink(_) => "pr-link",
            Line::AiTitle(_) => "ai-title",
            Line::CustomTitle(_) => "custom-title",
            Line::ContinuedIn(_) => "continued-in",
            Line::AgentSetting(_) => "agent-setting",
            Line::BridgeSession(_) => "bridge-session",
            Line::ScheduledTaskFire(_) => "scheduled_task_fire",
            Line::Unknown(v) => v.get("type").and_then(Value::as_str).unwrap_or("?"),
        }
    }

    /// The Claude Code version that wrote the line, when it says.
    pub fn version(&self) -> Option<&str> {
        match self {
            Line::User(u) => u.version.as_deref(),
            Line::Assistant(a) => a.version.as_deref(),
            Line::System(s) => s.version.as_deref(),
            Line::Attachment(a) => a.version.as_deref(),
            _ => None,
        }
    }

    /// ISO-8601 timestamp, for the line kinds that carry one.
    pub fn timestamp(&self) -> Option<&str> {
        match self {
            Line::User(u) => u.timestamp.as_deref(),
            Line::Assistant(a) => a.timestamp.as_deref(),
            Line::System(s) => s.timestamp.as_deref(),
            Line::Attachment(a) => a.timestamp.as_deref(),
            Line::QueueOperation(q) => q.timestamp.as_deref(),
            Line::PrLink(p) => p.timestamp.as_deref(),
            Line::FileHistoryDelta(d) => d.timestamp.as_deref(),
            Line::ContinuedIn(c) => c.timestamp.as_deref(),
            _ => None,
        }
    }
}

// ------------------------------------------------------------ serde helpers

/// `null` where a struct is expected → the default. Claude Code writes
/// `"output_tokens_details": null` on API-error lines.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// A string's length, never the string (`userFeedback`).
fn string_len<'de, D>(d: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.map(|s| s.chars().count()))
}

// ---------------------------------------------------------------- user

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLine {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub version: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub is_meta: bool,
    #[serde(default)]
    pub is_sidechain: bool,
    pub message: UserMessage,
    /// Structured result Claude Code attached to a tool_result line
    /// (`stdout`/`stderr`/`file`/`structuredPatch`…). Shape varies per tool
    /// (see [`ToolUseDetail`]), so it is kept raw.
    #[serde(default)]
    pub tool_use_result: Option<Value>,
    /// Shared by a prompt and every tool_result line of its turn (2.1.220+):
    /// the exact turn identity.
    pub prompt_id: Option<String>,
    /// `typed` / `suggestion_accepted` / `queued` / `system` / `sdk`.
    pub prompt_source: Option<String>,
    pub origin: Option<Origin>,
    /// Why a tool was refused (`automode-blocked`, `permission-rule`,
    /// `user-rejected`, `automode-unavailable`).
    pub tool_denial_kind: Option<String>,
    /// Length of the reason the user typed on a rejection; the text is
    /// never kept.
    #[serde(default, rename = "userFeedback", deserialize_with = "string_len")]
    pub user_feedback_len: Option<usize>,
    /// The API message an interrupt cut short.
    pub interrupted_message_id: Option<String>,
    /// The summary written after a compaction (not a turn).
    #[serde(default)]
    pub is_compact_summary: bool,
    #[serde(default)]
    pub turn_companion: bool,
    pub source_tool_assistant_uuid: Option<String>,
    pub git_branch: Option<String>,
    /// Permission mode in force when a prompt was submitted.
    pub permission_mode: Option<String>,
    /// `bg` on 2.1.270 background sessions.
    pub session_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Origin {
    #[serde(default)]
    pub kind: String,
}

/// What a `user` line is. Only [`PromptKind::Human`] starts a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptKind {
    /// Typed, accepted from a suggestion, or queued by the person.
    Human,
    /// `promptSource` `system` / `sdk`, or a coordinator's message.
    Machine,
    /// A tool_result line.
    ToolResult,
    /// `<command-name>/x</command-name>` (a slash command).
    SlashCommand,
    /// `<local-command-stdout>` / `<local-command-caveat>`.
    LocalCommandOutput,
    /// `[Request interrupted by user…]`.
    Interrupt,
    /// `<task-notification>` (a background task finished).
    TaskNotification,
    /// `<teammate-message>`.
    TeammateMessage,
    /// `<bash-input>` and its output: the person ran `! cmd`.
    BashShell,
    /// `<ide_opened_file>` / `<ide_selection>` (`promptSource` `sdk`).
    IdeContext,
    /// The post-compaction summary.
    CompactSummary,
    /// `isMeta` (caveats, companions, injected context).
    Meta,
}

impl UserLine {
    /// Classify the line. The tag prefixes are Claude Code's own; the
    /// `promptSource` / `origin` keys exist since 2.1.220 and an untagged,
    /// non-meta text line without them is taken as human (the pre-2.1.220
    /// heuristic).
    pub fn prompt_kind(&self) -> PromptKind {
        if self.message.content.tool_results().next().is_some() {
            return PromptKind::ToolResult;
        }
        if self.is_compact_summary {
            return PromptKind::CompactSummary;
        }
        let text = self.message.content.text();
        let t = text.trim_start();
        if self.interrupted_message_id.is_some() || t.starts_with("[Request interrupted by user") {
            return PromptKind::Interrupt;
        }
        if t.starts_with("<command-name>") {
            return PromptKind::SlashCommand;
        }
        if t.starts_with("<local-command-stdout>") || t.starts_with("<local-command-caveat>") {
            return PromptKind::LocalCommandOutput;
        }
        if t.starts_with("<task-notification") {
            return PromptKind::TaskNotification;
        }
        if t.starts_with("<teammate-message") {
            return PromptKind::TeammateMessage;
        }
        if t.starts_with("<bash-input>")
            || t.starts_with("<bash-stdout>")
            || t.starts_with("<bash-stderr>")
        {
            return PromptKind::BashShell;
        }
        if t.starts_with("<ide_opened_file>") || t.starts_with("<ide_selection>") {
            return PromptKind::IdeContext;
        }
        if self.is_meta {
            return PromptKind::Meta;
        }
        match self.origin.as_ref().map(|o| o.kind.as_str()) {
            Some("task-notification") => return PromptKind::TaskNotification,
            Some("human") => return PromptKind::Human,
            Some("coordinator") => return PromptKind::Machine,
            _ => {}
        }
        match self.prompt_source.as_deref() {
            Some("typed" | "suggestion_accepted" | "queued") => PromptKind::Human,
            Some("system" | "sdk") => PromptKind::Machine,
            _ => PromptKind::Human,
        }
    }

    /// A prompt the person wrote: the only kind that starts a turn.
    pub fn is_human_prompt(&self) -> bool {
        self.prompt_kind() == PromptKind::Human
    }

    /// The slash command of a `<command-name>` line (`/clear`, `/model`…).
    pub fn slash_command(&self) -> Option<String> {
        let text = self.message.content.text();
        let t = text.trim_start();
        let rest = t.strip_prefix("<command-name>")?;
        let end = rest.find("</command-name>")?;
        Some(rest[..end].trim().to_string())
    }

    pub fn is_interrupt(&self) -> bool {
        self.prompt_kind() == PromptKind::Interrupt
    }

    /// The typed denial kind, when this is a refused tool's result.
    pub fn denial(&self) -> Option<DenialKind> {
        self.tool_denial_kind.as_deref().map(DenialKind::parse)
    }

    /// The typed `toolUseResult`, when the line carries one.
    pub fn tool_use_detail(&self) -> Option<ToolUseDetail> {
        self.tool_use_result.as_ref().map(ToolUseDetail::parse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DenialKind {
    /// The auto-mode classifier refused the call.
    AutoModeBlocked,
    /// The classifier was unavailable, so the call was refused.
    AutoModeUnavailable,
    /// A `permissions.deny` rule in settings.
    PermissionRule,
    /// The person said no at the prompt.
    UserRejected,
    Other,
}

impl DenialKind {
    pub fn parse(s: &str) -> DenialKind {
        match s {
            "automode-blocked" => DenialKind::AutoModeBlocked,
            "automode-unavailable" => DenialKind::AutoModeUnavailable,
            "permission-rule" => DenialKind::PermissionRule,
            "user-rejected" => DenialKind::UserRejected,
            _ => DenialKind::Other,
        }
    }

    /// Claude Code's own word for it.
    pub fn label(self) -> &'static str {
        match self {
            DenialKind::AutoModeBlocked => "automode-blocked",
            DenialKind::AutoModeUnavailable => "automode-unavailable",
            DenialKind::PermissionRule => "permission-rule",
            DenialKind::UserRejected => "user-rejected",
            DenialKind::Other => "denied",
        }
    }
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

    /// Image blocks the person pasted.
    pub fn images(&self) -> usize {
        match self {
            Content::Blocks(b) => b.iter().filter(|b| matches!(b, UserBlock::Image)).count(),
            Content::Text(_) => 0,
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
    #[serde(rename = "image")]
    Image,
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

    /// Image blocks in the result (screenshots, `Read` of a PNG).
    pub fn images(&self) -> usize {
        match &self.content {
            Value::Array(items) => items
                .iter()
                .filter(|i| i.get("type").and_then(Value::as_str) == Some("image"))
                .count(),
            _ => 0,
        }
    }

    /// A `keepRecent` clearing left this in place of the real output.
    pub fn is_cleared(&self) -> bool {
        self.text()
            .trim_start()
            .starts_with("[Old tool result content cleared")
    }
}

// ----------------------------------------------------------- assistant

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantLine {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub version: Option<String>,
    pub cwd: Option<String>,
    pub request_id: Option<String>,
    /// Effort level in force for this response (`low`/`medium`/`high`…).
    pub effort: Option<String>,
    /// Effort set for this turn alone with `/effort` (2.1.269+); `effort`
    /// already reflects it.
    pub per_turn_effort: Option<String>,
    #[serde(default)]
    pub is_sidechain: bool,
    pub message: AssistantMessage,
    /// The skill / plugin / agent / MCP server that "owns" this line.
    pub attribution_skill: Option<String>,
    pub attribution_plugin: Option<String>,
    pub attribution_agent: Option<String>,
    pub attribution_mcp_server: Option<String>,
    pub attribution_mcp_tool: Option<String>,
    /// The request failed; the line is Claude Code's own error message, not
    /// a model response (`message.model` is `<synthetic>`, usage all zero).
    #[serde(default)]
    pub is_api_error_message: bool,
    /// `rate_limit`, `authentication_failed`, `prompt_too_long`…
    pub error: Option<String>,
    pub api_error_status: Option<u16>,
    pub quota_limits: Option<QuotaLimits>,
    pub git_branch: Option<String>,
    /// Index of this line's block within the API response (2.1.258+).
    pub api_block_index: Option<u64>,
}

/// The rate-limit state a 429 line carries.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaLimits {
    pub status: Option<String>,
    /// Unix seconds.
    pub resets_at: Option<f64>,
    /// `five_hour` / `seven_day` / …
    pub rate_limit_type: Option<String>,
    pub low_priority_retry_after_seconds: Option<u64>,
    pub is_using_overage: Option<bool>,
}

impl AssistantLine {
    /// An API error rendered as an assistant line: never a model response.
    pub fn is_api_error(&self) -> bool {
        self.is_api_error_message || self.message.model == "<synthetic>"
    }

    /// The effort that priced this response.
    pub fn effective_effort(&self) -> Option<&str> {
        self.per_turn_effort.as_deref().or(self.effort.as_deref())
    }

    /// The response's last text block ends with a question mark.
    pub fn ends_with_question(&self) -> bool {
        self.message
            .content
            .iter()
            .rev()
            .find_map(|b| match b {
                AssistantBlock::Text { text } => Some(text.trim_end().ends_with('?')),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Characters of prose in this line (not thinking, not tool inputs).
    pub fn text_chars(&self) -> usize {
        self.message
            .content
            .iter()
            .map(|b| match b {
                AssistantBlock::Text { text } => text.chars().count(),
                _ => 0,
            })
            .sum()
    }

    /// The API's own reason this call missed the cache, when it said.
    pub fn cache_miss_reason(&self) -> Option<&CacheMissReason> {
        self.message
            .diagnostics
            .as_ref()
            .and_then(|d| d.cache_miss_reason.as_ref())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    /// API message id. One response is written as several lines (one per
    /// content block) that share this id — usage must be counted once per id.
    pub id: String,
    pub model: String,
    #[serde(default, deserialize_with = "null_default")]
    pub content: Vec<AssistantBlock>,
    pub stop_reason: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub usage: Usage,
    /// `null` when the call hit the cache.
    #[serde(default)]
    pub diagnostics: Option<Diagnostics>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Diagnostics {
    #[serde(default)]
    pub cache_miss_reason: Option<CacheMissReason>,
}

/// `model_changed`, `tools_changed`, `messages_changed`,
/// `previous_message_not_found`, `unavailable`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CacheMissReason {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub cache_missed_input_tokens: Option<u64>,
}

impl CacheMissReason {
    /// A cause the person can act on (a switch or a rewrite), as opposed to
    /// the API losing the entry.
    pub fn is_named(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "model_changed" | "tools_changed" | "messages_changed"
        )
    }
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
    #[serde(default, deserialize_with = "null_default")]
    pub output_tokens_details: OutputDetails,
    #[serde(default, deserialize_with = "null_default")]
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
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub subtype: String,
    /// `turn_duration`: milliseconds of the turn that just ended.
    pub duration_ms: Option<u64>,
    /// `turn_duration`: messages in the conversation at turn end.
    pub message_count: Option<u64>,
    pub pending_background_agent_count: Option<u64>,
    pub pending_workflow_count: Option<u64>,
    /// `stop_hook_summary`: one entry per hook command that ran.
    #[serde(default)]
    pub hook_infos: Vec<HookInfo>,
    #[serde(default)]
    pub hook_errors: Vec<Value>,
    /// `away_summary`, `local_command`, `informational` and free-form text.
    pub content: Option<String>,
    pub level: Option<String>,
    /// `compact_boundary`: what the compaction did.
    pub compact_metadata: Option<CompactMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    TurnDuration,
    StopHookSummary,
    AwaySummary,
    /// An exact compaction record (2.1.263+).
    CompactBoundary,
    /// A future `keepRecent` clearing marker; never observed, parsed
    /// defensively.
    MicrocompactBoundary,
    /// A slash command and its captured output.
    LocalCommand,
    Informational,
    ScheduledTaskFire,
    Other,
}

impl SystemLine {
    pub fn kind(&self) -> SystemKind {
        match self.subtype.as_str() {
            "turn_duration" => SystemKind::TurnDuration,
            "stop_hook_summary" => SystemKind::StopHookSummary,
            "away_summary" => SystemKind::AwaySummary,
            "compact_boundary" => SystemKind::CompactBoundary,
            "microcompact_boundary" => SystemKind::MicrocompactBoundary,
            "local_command" => SystemKind::LocalCommand,
            "informational" => SystemKind::Informational,
            "scheduled_task_fire" => SystemKind::ScheduledTaskFire,
            _ => SystemKind::Other,
        }
    }

    /// The slash command or captured output of a `local_command` line.
    pub fn local_command(&self) -> Option<LocalCommand> {
        if self.kind() != SystemKind::LocalCommand {
            return None;
        }
        LocalCommand::parse(self.content.as_deref()?)
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompactMetadata {
    /// `auto` or `manual`.
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub pre_tokens: u64,
    #[serde(default)]
    pub post_tokens: u64,
    #[serde(default)]
    pub cumulative_dropped_tokens: u64,
    #[serde(default)]
    pub duration_ms: u64,
    /// How many messages survived, counted from the uuid list.
    #[serde(default, deserialize_with = "preserved_count")]
    pub preserved_messages: usize,
}

fn preserved_count<'de, D>(d: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<Value>::deserialize(d)?;
    Ok(v.as_ref()
        .and_then(|v| v.get("uuids"))
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0))
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
    /// `absorbed_mid_turn` when a queued steer was folded into the running
    /// turn.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistorySnapshot {
    pub message_id: Option<String>,
}

/// One tracked file got a new checkpoint version.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistoryDelta {
    pub message_id: Option<String>,
    pub tracking_path: Option<String>,
    #[serde(default)]
    pub backup: Option<FileBackup>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBackup {
    #[serde(default)]
    pub version: u64,
    pub backup_file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrLink {
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
    pub pr_repository: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTitle {
    #[serde(default)]
    pub ai_title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTitle {
    #[serde(default)]
    pub custom_title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuedIn {
    pub continued_in_session_id: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSetting {
    #[serde(default)]
    pub agent_setting: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSession {
    pub bridge_session_id: Option<String>,
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::Line;
    use std::path::Path;

    pub fn fixture(name: &str) -> Vec<Line> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl"));
        std::fs::read_to_string(p)
            .unwrap()
            .lines()
            .map(|l| Line::parse(l).expect("valid json"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::fixture;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn every_line_parses_to_a_known_variant_except_the_expected_set() {
        let expected_unknown = ["mode", "atis-latch", "last-prompt"];
        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in fixture("session-a") {
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
            "file-history-delta",
            "attachment",
            "ai-title",
            "bridge-session",
        ] {
            assert!(counts[ty] > 0, "{ty} missing");
        }
        assert_eq!(counts["assistant"], 386);
        assert_eq!(counts["user"], 276);
        assert_eq!(counts["cost-state"], 1);
    }

    #[test]
    fn fixture_b_parses_every_line_it_models() {
        let expected_unknown = ["mode", "atis-latch", "last-prompt", "agent-name"];
        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in fixture("session-b") {
            if let Line::Unknown(v) = &line {
                let ty = v["type"].as_str().unwrap();
                assert!(expected_unknown.contains(&ty), "unexpected Unknown {ty}");
            }
            *counts.entry(line.kind().to_string()).or_default() += 1;
        }
        assert_eq!(counts["assistant"], 266);
        assert_eq!(counts["user"], 150);
        assert_eq!(counts["continued-in"], 1);
        assert_eq!(counts["pr-link"], 1);
        assert_eq!(counts["file-history-delta"], 16);
    }

    #[test]
    fn assistant_fields_and_usage() {
        let lines = fixture("session-a");
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
        assert_eq!(first.effective_effort(), Some("medium"));
        assert!(!first.is_api_error());
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
        for l in fixture("session-a") {
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
        for l in fixture("session-a") {
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
        let lines = fixture("session-a");
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
                    _ => {}
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
            Line::User(u) => {
                assert_eq!(u.message.content.text(), "hello");
                assert!(u.is_human_prompt(), "untagged text is human (pre-2.1.220)");
            }
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

    // ----- the coach fields, on fixture B (real shapes, anonymised)

    #[test]
    fn user_line_kinds_on_fixture_b() {
        let lines = fixture("session-b");
        let mut kinds: HashMap<PromptKind, usize> = HashMap::new();
        let mut denials: HashMap<DenialKind, usize> = HashMap::new();
        let mut prompt_ids = std::collections::HashSet::new();
        let mut slash = Vec::new();
        for l in &lines {
            if let Line::User(u) = l {
                *kinds.entry(u.prompt_kind()).or_default() += 1;
                if let Some(d) = u.denial() {
                    *denials.entry(d).or_default() += 1;
                    assert!(u.tool_use_result.as_ref().is_some_and(|v| v.is_string()));
                }
                if u.is_human_prompt() {
                    prompt_ids.insert(u.prompt_id.clone().expect("promptId on 2.1.270"));
                }
                if let Some(c) = u.slash_command() {
                    slash.push(c);
                }
            }
        }
        assert_eq!(kinds[&PromptKind::Human], 6, "{kinds:?}");
        assert_eq!(prompt_ids.len(), 6, "one promptId per human turn");
        assert_eq!(kinds[&PromptKind::Interrupt], 1);
        assert_eq!(kinds[&PromptKind::SlashCommand], 1);
        assert_eq!(kinds[&PromptKind::CompactSummary], 1);
        assert_eq!(
            kinds[&PromptKind::LocalCommandOutput],
            1,
            "the /clear caveat"
        );
        assert!(kinds[&PromptKind::ToolResult] > 100);
        assert_eq!(slash, ["/clear"]);
        assert_eq!(denials[&DenialKind::AutoModeBlocked], 1);
        assert_eq!(denials[&DenialKind::PermissionRule], 1);
        assert_eq!(denials[&DenialKind::UserRejected], 1);
        assert_eq!(denials[&DenialKind::AutoModeUnavailable], 1);
        let rejected = Line::parse(r#"{"type":"user","timestamp":"t","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"The user doesn't want to proceed","is_error":true}]},"toolUseResult":"The user doesn't want to proceed","toolDenialKind":"user-rejected","userFeedback":"not that one"}"#).unwrap();
        let Line::User(rejected) = rejected else {
            panic!()
        };
        assert_eq!(rejected.denial(), Some(DenialKind::UserRejected));
        assert_eq!(
            rejected.user_feedback_len,
            Some(12),
            "length only, text never kept"
        );
        assert_eq!(
            rejected.tool_use_detail(),
            Some(ToolUseDetail::Text { chars: 32 })
        );
        let interrupt = lines
            .iter()
            .find_map(|l| match l {
                Line::User(u) if u.is_interrupt() => Some(u),
                _ => None,
            })
            .unwrap();
        assert!(interrupt.interrupted_message_id.is_some());
        assert!(!interrupt.is_human_prompt());
    }

    #[test]
    fn api_error_line_is_typed_and_excluded() {
        let lines = fixture("session-b");
        let errs: Vec<&AssistantLine> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Assistant(a) if a.is_api_error() => Some(a),
                _ => None,
            })
            .collect();
        assert!(!errs.is_empty());
        for e in &errs {
            assert_eq!(e.message.model, "<synthetic>");
            assert_eq!(e.message.usage.total_input(), 0);
        }
        // One of them is a real API error, the other a synthetic notice
        // without the flag: `<synthetic>` alone must be enough to exclude it.
        assert!(errs
            .iter()
            .any(|e| e.is_api_error_message && e.error.is_some()));
        let synthetic = r#"{"type":"assistant","timestamp":"2026-09-12T15:36:15.062Z","message":{"diagnostics":null,"id":"9ca937c8","container":null,"model":"<synthetic>","role":"assistant","stop_details":null,"stop_reason":"stop_sequence","stop_sequence":"","type":"message","usage":{"output_tokens_details":null,"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":null,"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0},"inference_geo":null,"iterations":null,"speed":null},"content":[{"type":"text","text":"Rate limit hit"}],"context_management":null},"requestId":"req_1","quotaLimits":{"status":"rejected","resetsAt":1789244400,"unifiedRateLimitFallbackAvailable":false,"rateLimitType":"five_hour","overageStatus":"disabled","isUsingOverage":false,"lowPriorityRetryAfterSeconds":20,"lowPriorityMaxWaitSeconds":1200},"error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429,"version":"2.1.269"}"#;
        let Line::Assistant(a) = Line::parse(synthetic).unwrap() else {
            panic!("null output_tokens_details must not make the line Unknown")
        };
        assert!(a.is_api_error());
        assert_eq!(a.error.as_deref(), Some("rate_limit"));
        assert_eq!(a.api_error_status, Some(429));
        let q = a.quota_limits.unwrap();
        assert_eq!(q.rate_limit_type.as_deref(), Some("five_hour"));
        assert_eq!(q.resets_at, Some(1_789_244_400.0));
        assert_eq!(q.low_priority_retry_after_seconds, Some(20));
        assert_eq!(a.message.usage.output_tokens_details.thinking_tokens, 0);
    }

    #[test]
    fn diagnostics_attribution_and_questions() {
        let line = r#"{"type":"assistant","timestamp":"2026-09-12T05:54:37.344Z","message":{"model":"claude-opus-5","id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"Shall I go on?"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":836644,"cache_read_input_tokens":29377,"output_tokens":1129,"output_tokens_details":{"thinking_tokens":255},"cache_creation":{"ephemeral_1h_input_tokens":836644,"ephemeral_5m_input_tokens":0}},"diagnostics":{"cache_miss_reason":{"type":"model_changed"}}},"attributionSkill":"claude-api","effort":"medium","perTurnEffort":"high","gitBranch":"main","apiBlockIndex":0,"version":"2.1.269"}"#;
        let Line::Assistant(a) = Line::parse(line).unwrap() else {
            panic!()
        };
        let miss = a.cache_miss_reason().unwrap();
        assert_eq!(miss.kind, "model_changed");
        assert!(miss.is_named());
        assert_eq!(miss.cache_missed_input_tokens, None);
        assert_eq!(a.attribution_skill.as_deref(), Some("claude-api"));
        assert_eq!(a.effective_effort(), Some("high"), "perTurnEffort wins");
        assert!(a.ends_with_question());
        assert_eq!(a.text_chars(), 14);
        assert_eq!(a.git_branch.as_deref(), Some("main"));
        assert_eq!(a.api_block_index, Some(0));
        // `previous_message_not_found` is the API losing the entry, not a cause.
        let lost = CacheMissReason {
            kind: "previous_message_not_found".into(),
            cache_missed_input_tokens: None,
        };
        assert!(!lost.is_named());
    }

    #[test]
    fn compaction_boundary_and_summary_on_fixture_b() {
        let lines = fixture("session-b");
        let cb = lines
            .iter()
            .find_map(|l| match l {
                Line::System(s) if s.kind() == SystemKind::CompactBoundary => Some(s),
                _ => None,
            })
            .expect("compact_boundary");
        let m = cb.compact_metadata.as_ref().unwrap();
        assert_eq!(m.trigger, "auto");
        assert_eq!(m.pre_tokens, 567_672);
        assert_eq!(m.post_tokens, 230_014);
        assert_eq!(m.cumulative_dropped_tokens, 337_658);
        assert_eq!(m.duration_ms, 80_690);
        assert!(m.preserved_messages > 1000);
        assert_eq!(cb.version.as_deref(), Some("2.1.263"));
        let summary = lines
            .iter()
            .any(|l| matches!(l, Line::User(u) if u.is_compact_summary));
        assert!(summary);
        let td = lines
            .iter()
            .filter_map(|l| match l {
                Line::System(s) if s.kind() == SystemKind::TurnDuration => s.message_count,
                _ => None,
            })
            .count();
        assert_eq!(td, 5, "messageCount on every turn_duration");
    }

    #[test]
    fn boundary_and_title_lines() {
        let lines = fixture("session-b");
        let cont = lines
            .iter()
            .find_map(|l| match l {
                Line::ContinuedIn(c) => Some(c),
                _ => None,
            })
            .unwrap();
        assert!(cont.continued_in_session_id.is_some());
        assert!(cont.timestamp.is_some());
        let pr = lines
            .iter()
            .find_map(|l| match l {
                Line::PrLink(p) => Some(p),
                _ => None,
            })
            .unwrap();
        assert_eq!(pr.pr_number, Some(121));
        let deltas: Vec<&FileHistoryDelta> = lines
            .iter()
            .filter_map(|l| match l {
                Line::FileHistoryDelta(d) => Some(d),
                _ => None,
            })
            .collect();
        assert_eq!(deltas.len(), 16);
        assert!(deltas.iter().all(|d| d.tracking_path.is_some()));
        assert!(deltas
            .iter()
            .all(|d| d.backup.as_ref().unwrap().version >= 1));
        assert!(lines
            .iter()
            .any(|l| matches!(l, Line::AiTitle(t) if !t.ai_title.is_empty())));
        let l = Line::parse(r#"{"type":"custom-title","customTitle":"my title","sessionId":"s"}"#)
            .unwrap();
        assert!(matches!(l, Line::CustomTitle(t) if t.custom_title == "my title"));
        let l = Line::parse(r#"{"type":"scheduled_task_fire","x":1}"#).unwrap();
        assert!(matches!(l, Line::ScheduledTaskFire(_)));
        assert_eq!(
            Line::parse(r#"{"type":"user","message":{"content":"x"},"version":"2.1.270"}"#)
                .unwrap()
                .version(),
            Some("2.1.270")
        );
    }

    #[test]
    fn prompt_kinds_from_tags_and_sources() {
        let mk = |extra: &str, content: &str| -> UserLine {
            let text = format!(
                r#"{{"type":"user","timestamp":"t","message":{{"role":"user","content":{content}}}{extra}}}"#
            );
            match Line::parse(&text).unwrap() {
                Line::User(u) => u,
                other => panic!("{other:?}"),
            }
        };
        let q = |s: &str| serde_json::to_string(s).unwrap();
        assert_eq!(
            mk(
                r#","promptSource":"typed","origin":{"kind":"human"}"#,
                &q("do it")
            )
            .prompt_kind(),
            PromptKind::Human
        );
        assert_eq!(
            mk(
                r#","promptSource":"queued","origin":{"kind":"human"}"#,
                &q("and this")
            )
            .prompt_kind(),
            PromptKind::Human
        );
        assert_eq!(
            mk(r#","promptSource":"suggestion_accepted""#, &q("ok")).prompt_kind(),
            PromptKind::Human
        );
        assert_eq!(
            mk(
                r#","promptSource":"system","origin":{"kind":"task-notification"}"#,
                &q("<task-notification>done")
            )
            .prompt_kind(),
            PromptKind::TaskNotification
        );
        assert_eq!(
            mk(r#","promptSource":"system""#, &q("machine")).prompt_kind(),
            PromptKind::Machine
        );
        assert_eq!(
            mk(r#","promptSource":"sdk""#, &q("<ide_opened_file>x")).prompt_kind(),
            PromptKind::IdeContext
        );
        assert_eq!(
            mk(r#","promptSource":"sdk""#, &q("from the sdk")).prompt_kind(),
            PromptKind::Machine
        );
        assert_eq!(
            mk(
                "",
                &q("<command-name>/model</command-name>\n<command-message>model</command-message>")
            )
            .prompt_kind(),
            PromptKind::SlashCommand
        );
        assert_eq!(
            mk("", &q("<command-name>/model</command-name>"))
                .slash_command()
                .as_deref(),
            Some("/model")
        );
        assert_eq!(
            mk("", &q("<local-command-stdout>out</local-command-stdout>")).prompt_kind(),
            PromptKind::LocalCommandOutput
        );
        assert_eq!(
            mk(r#","isMeta":true"#, &q("<local-command-caveat>c")).prompt_kind(),
            PromptKind::LocalCommandOutput
        );
        assert_eq!(
            mk("", &q("<teammate-message from=\"a\">hi")).prompt_kind(),
            PromptKind::TeammateMessage
        );
        assert_eq!(
            mk("", &q("<bash-input>ls</bash-input>")).prompt_kind(),
            PromptKind::BashShell
        );
        assert_eq!(
            mk(
                "",
                r#"[{"type":"text","text":"[Request interrupted by user for tool use]"}]"#
            )
            .prompt_kind(),
            PromptKind::Interrupt
        );
        assert_eq!(
            mk(r#","isMeta":true,"turnCompanion":true"#, &q("companion")).prompt_kind(),
            PromptKind::Meta
        );
        assert_eq!(
            mk(r#","isCompactSummary":true"#, &q("summary")).prompt_kind(),
            PromptKind::CompactSummary
        );
        assert_eq!(
            mk(
                "",
                r#"[{"type":"tool_result","tool_use_id":"t","content":"x"}]"#
            )
            .prompt_kind(),
            PromptKind::ToolResult
        );
        assert_eq!(
            mk(
                "",
                r#"[{"type":"text","text":"see"},{"type":"image","source":{}}]"#
            )
            .message
            .content
            .images(),
            1
        );
        let cleared = ToolResult {
            tool_use_id: "t".into(),
            content: Value::String("[Old tool result content cleared]".into()),
            is_error: false,
        };
        assert!(cleared.is_cleared());
    }

    #[test]
    fn queue_operation_reason_and_denial_labels() {
        let l = Line::parse(r#"{"type":"queue-operation","operation":"dequeue","timestamp":"t","reason":"absorbed_mid_turn"}"#).unwrap();
        assert!(
            matches!(l, Line::QueueOperation(q) if q.reason.as_deref() == Some("absorbed_mid_turn"))
        );
        assert_eq!(
            DenialKind::parse("automode-blocked").label(),
            "automode-blocked"
        );
        assert_eq!(DenialKind::parse("weird"), DenialKind::Other);
    }
}
