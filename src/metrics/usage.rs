//! Token usage, counted once per API response, grouped into turns.
//!
//! Claude Code writes one `assistant` line per content block, all carrying the
//! same `message.id` and the same `usage`. Summing lines naïvely overstates
//! everything ~2–3×; [`Aggregate`] keys on the id.
//!
//! A turn starts at a prompt the person wrote (or a machine-originated one: a
//! task notification, a teammate message, an SDK prompt). Interrupts, slash
//! commands, the compaction summary and injected context are not turns, and
//! an API-error line (`<synthetic>`, zero usage) is not a response: it
//! neither sets the model nor moves the context.

use std::collections::HashSet;

use crate::transcript::{AssistantLine, CacheTtl, Content, Line, PromptKind};

/// Token counts by class. `cache_write_5m` + `cache_write_1h` is the raw
/// `cache_creation_input_tokens`; when the API does not break it down the
/// whole amount is attributed to the 5-minute bucket.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    pub thinking: u64,
}

impl Usage {
    pub fn from_api(u: &crate::transcript::Usage) -> Usage {
        let (w5, w1) = if u.cache_creation.ephemeral_1h_input_tokens > 0
            || u.cache_creation.ephemeral_5m_input_tokens > 0
        {
            (
                u.cache_creation.ephemeral_5m_input_tokens,
                u.cache_creation.ephemeral_1h_input_tokens,
            )
        } else {
            (u.cache_creation_input_tokens, 0)
        };
        Usage {
            input: u.input_tokens,
            cache_write_5m: w5,
            cache_write_1h: w1,
            cache_read: u.cache_read_input_tokens,
            output: u.output_tokens,
            thinking: u.output_tokens_details.thinking_tokens,
        }
    }

    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }

    /// Everything sent to the model.
    pub fn total_input(&self) -> u64 {
        self.input + self.cache_write() + self.cache_read
    }

    pub fn total(&self) -> u64 {
        self.total_input() + self.output
    }

    /// Share of input served from cache; `None` when nothing was sent.
    pub fn cache_hit_ratio(&self) -> Option<f64> {
        let denom = self.total_input();
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }

    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
        self.output += o.output;
        self.thinking += o.thinking;
    }

    /// Takes back what `add` counted (saturating).
    pub fn sub(&mut self, o: &Usage) {
        self.input = self.input.saturating_sub(o.input);
        self.cache_write_5m = self.cache_write_5m.saturating_sub(o.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_sub(o.cache_write_1h);
        self.cache_read = self.cache_read.saturating_sub(o.cache_read);
        self.output = self.output.saturating_sub(o.output);
        self.thinking = self.thinking.saturating_sub(o.thinking);
    }
}

/// One prompt and everything the model did in response.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    /// 1-based, over all turns (human and machine).
    pub number: usize,
    /// Claude Code's own turn identity (2.1.220+).
    pub prompt_id: Option<String>,
    /// The person wrote the prompt (typed, accepted a suggestion, queued).
    /// False for machine-originated turns: task notifications, teammate
    /// messages, SDK prompts.
    pub human: bool,
    /// `promptSource` as written (`typed`, `suggestion_accepted`, `queued`,
    /// `system`, `sdk`…).
    pub prompt_source: Option<String>,
    /// Timestamp of the user line that started the turn (ISO-8601 as written).
    pub started_at: Option<String>,
    /// Timestamp of the last line seen in this turn.
    pub last_at: Option<String>,
    /// Exact duration from the `turn_duration` system line, when it arrived.
    pub duration_ms: Option<u64>,
    /// Distinct API responses.
    pub api_calls: usize,
    /// API-error lines seen in this turn (never counted as calls).
    pub api_errors: usize,
    pub usage: Usage,
    /// Models seen, in first-use order.
    pub models: Vec<String>,
    /// Effort level of the last response (`perTurnEffort` when set).
    pub effort: Option<String>,
    /// `cache_read + cache_write + input` of the last API call: the context
    /// size the model saw.
    pub context_size: u64,
    /// Cache TTL on the last call that wrote cache.
    pub cache_ttl: Option<CacheTtl>,
    /// `cache_read + cache_write` of the turn's first API call (the fixed
    /// prefix, when this is the session's first turn).
    pub first_call_prefix: u64,
    /// Uncached `input_tokens` of the turn's first API call: what the prompt
    /// itself (and a paste) cost fresh.
    pub first_call_input: u64,
    /// Tool result text length pushed into context this turn (bytes).
    pub tool_result_bytes: u64,
    /// Number of tool calls issued this turn.
    pub tool_calls: usize,
    /// Number of tool results flagged `is_error`.
    pub tool_errors: usize,
    /// Tool results refused by kind (`automode-blocked`, `permission-rule`…).
    pub denials: usize,
    /// Length of the user's prompt text, in characters.
    pub prompt_chars: usize,
    /// Images the person pasted with the prompt.
    pub prompt_images: usize,
    /// Time between a user/tool_result line and the next response (≈ API).
    pub api_ms: i64,
    /// Time between a tool_use response and its tool_result (≈ tools).
    pub tool_ms: i64,
    /// Hook commands that ran at the end of this turn (`stop_hook_summary`).
    pub hook_runs: usize,
    /// Total hook wall time for this turn, milliseconds.
    pub hook_ms: u64,
    pub hook_errors: usize,
    /// The person interrupted the turn after this many tool calls.
    pub interrupted_after_calls: Option<usize>,
    /// When the `turn_duration` line landed (the turn's end).
    pub ended_at: Option<String>,
    /// The last response's text ended with a question mark.
    pub ended_with_question: bool,
    /// `stop_reason` of the last response.
    pub last_stop_reason: Option<String>,
    /// Tokens the harness injected (attachments) during this turn, and
    /// whether any of them was estimated rather than measured.
    pub harness_tokens: u64,
    pub harness_approx: bool,
    /// Characters of prose the model wrote this turn (text blocks).
    pub prose_chars: usize,
    /// Human steers Claude Code folded into this turn (`queued_command`
    /// attachments with a human origin).
    pub steers: usize,
    /// `turn_duration.pendingBackgroundAgentCount` at turn end.
    pub pending_background_agents: Option<u64>,
    /// A Stop hook prevented the turn from ending (`preventedContinuation`).
    pub hook_blocked: bool,
    /// `file-history-snapshot` lines in this turn: rewind points.
    pub checkpoints: usize,
    /// Timestamp of the model's last text block (prose) in this turn.
    pub last_text_at: Option<String>,
    /// Tool calls issued since the model last wrote prose (the silent run).
    pub calls_since_text: usize,
    /// `silent_turn_reminder` attachments the harness injected this turn.
    pub silent_reminders: usize,
    /// The person's last input in this turn: the prompt, or an answer to
    /// `AskUserQuestion` / `ExitPlanMode`.
    pub last_human_input_at: Option<String>,
    /// The prompt's shape (booleans computed at parse time; never the text).
    pub prompt_shape: crate::transcript::PromptShape,
    /// Calls the person refused at the prompt (`user-rejected`) or answered
    /// with feedback (`userFeedback`).
    pub rejections: usize,
    /// An `instructions` / `nested_memory` attachment landed this turn
    /// (CLAUDE.md or a memory file re-injected).
    pub instructions_seen: bool,
    /// Distinct source files edited this turn (basenames).
    pub files_edited: Vec<String>,
}

impl Turn {
    /// Wall time so far: exact once `turn_duration` arrived, else `now − start`.
    pub fn elapsed_ms(&self, now_ms: i64) -> Option<i64> {
        if let Some(d) = self.duration_ms {
            return Some(d as i64);
        }
        self.started_at
            .as_deref()
            .and_then(crate::metrics::cost::parse_ts_ms)
            .map(|s| (now_ms - s).max(0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastKind {
    User,
    Assistant,
}

/// An `away_summary` Claude Code wrote while the user was gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Away {
    pub at: Option<String>,
    pub content: String,
}

/// One API error line, as Claude Code wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub at: Option<String>,
    pub turn: usize,
    /// `isApiErrorMessage` was set: a real API failure. A `<synthetic>`
    /// line without it is Claude Code's own text (an interrupt notice).
    pub confirmed: bool,
    /// `rate_limit`, `invalid_request`, `authentication_failed`…
    pub error: Option<String>,
    pub status: Option<u16>,
    /// `five_hour` / `seven_day` on a 429.
    pub rate_limit_type: Option<String>,
    /// Unix seconds.
    pub resets_at: Option<f64>,
    pub low_priority_retry_after_s: Option<u64>,
}

/// An exact compaction from a `compact_boundary` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionRecord {
    pub at: Option<String>,
    /// Turn in which the boundary appeared.
    pub turn: usize,
    /// `auto` / `manual`.
    pub trigger: String,
    pub pre_tokens: u64,
    pub post_tokens: u64,
    pub duration_ms: u64,
}

/// One named cache miss: the cause and the tokens re-written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheMissRecord {
    pub at: Option<String>,
    pub turn: usize,
    /// `model_changed` / `tools_changed` / `messages_changed`.
    pub kind: String,
    /// `cache_missed_input_tokens`, else the call's cache write.
    pub tokens: u64,
    pub message_id: String,
}

/// A point after which the model's context is not what it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind {
    /// The person ran `/clear` (this file ended; or `continued-in`).
    Clear,
    Compact,
    /// A future `keepRecent` clearing.
    Microcompact,
    /// The session was resumed or forked (from the hook spool).
    Resume,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub at: Option<String>,
    pub turn: usize,
    pub kind: BoundaryKind,
}

/// One API response, for the per-request arithmetic (`/usage` weight,
/// the behaviour flags, the cost gradient).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub turn: usize,
    /// Epoch ms of the response line.
    pub at_ms: Option<i64>,
    pub model: String,
    pub usage: Usage,
    /// The turn is machine-originated (a task notification, an SDK prompt).
    pub machine: bool,
}

impl CallRecord {
    /// Context the call saw.
    pub fn context(&self) -> u64 {
        self.usage.total_input()
    }
}

/// Running aggregate over a transcript. Feed lines in order.
#[derive(Debug, Clone, Default)]
pub struct Aggregate {
    pub turns: Vec<Turn>,
    pub total: Usage,
    /// One record per API response, in order.
    pub calls: Vec<CallRecord>,
    /// Usage by the skill / plugin / agent / MCP server that owned the
    /// response (`attribution*` keys), keyed `skill:code-review`,
    /// `plugin:x`, `agent:y`, `mcp:server`; machine turns under `idle`.
    pub attribution: std::collections::BTreeMap<String, Usage>,
    /// Distinct API responses seen (for dedupe).
    seen_ids: HashSet<String>,
    /// Most recent cache TTL observed on any call.
    pub observed_ttl: Option<CacheTtl>,
    /// Last model used (never `<synthetic>`).
    pub model: Option<String>,
    /// Timestamp of the last API response (never an error line).
    pub last_api_at: Option<String>,
    /// Prompts waiting in Claude Code's input queue (`queue-operation`).
    pub queued_prompts: usize,
    /// Queued prompts Claude Code folded into the running turn.
    pub steers_absorbed: usize,
    /// Timestamp and kind of the last timed line, for api/tool split.
    last_ts: Option<(i64, LastKind)>,
    /// `away_summary` lines, in order.
    pub away: Vec<Away>,
    /// API-error lines, in order.
    pub api_errors: Vec<ApiError>,
    /// Exact compactions, in order.
    pub compactions: Vec<CompactionRecord>,
    /// Context boundaries, in order.
    pub boundaries: Vec<Boundary>,
    /// The Claude Code version that wrote the transcript (first seen).
    pub version: Option<String>,
    /// The transcript's own `sessionId` (first seen): a fixture has no
    /// registry entry, so this is where its id comes from.
    pub session_id: Option<String>,
    /// `(teamName, agentName)` of the first line carrying them: set only
    /// when the transcript is a teammate's, so a teammate attached as the
    /// main session shows its team.
    pub team: Option<(String, String)>,
    /// Interrupts, in order: `(turn, tool calls so far)`.
    pub interrupts: Vec<(usize, usize)>,
    /// The `/model`, `/effort`, `/clear` … lines seen, with the turn.
    pub slash_commands: Vec<(usize, String)>,
    /// The session's title (`custom-title` wins over `ai-title`).
    pub title: Option<String>,
    custom_title: bool,
    /// Open pull request number, from `pr-link`.
    pub pr_number: Option<u64>,
    /// `agent-setting` seen: the session runs a named agent persona (team).
    pub agent_setting: Option<String>,
    /// A `bridge-session` line: the session is reachable remotely.
    pub bridged: bool,
    /// Hook wall time by command, over the session (`stop_hook_summary`).
    pub hook_ms_by_command: std::collections::BTreeMap<String, u64>,
    /// Named cache misses (`diagnostics.cache_miss_reason`, actionable
    /// kinds only): `(turn, cause)`, once per response.
    pub miss_causes_by_turn: Vec<(usize, String)>,
    /// Every named cache miss with what it re-wrote (`diagnostics.
    /// cache_miss_reason`), in order.
    pub cache_misses: Vec<CacheMissRecord>,
    /// The last `goal_status` attachment (`/goal`): met, tokens, iterations.
    pub goal: Option<GoalStatus>,
}

/// `/goal` progress as Claude Code reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoalStatus {
    pub met: bool,
    pub tokens: Option<u64>,
    pub iterations: Option<u64>,
    pub duration_ms: Option<u64>,
}

impl Aggregate {
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a Line>) -> Aggregate {
        let mut a = Aggregate::default();
        for l in lines {
            a.push(l);
        }
        a
    }

    /// Distinct API responses so far.
    pub fn api_calls(&self) -> usize {
        self.seen_ids.len()
    }

    /// Turns the person started: the number the header shows.
    pub fn human_turns(&self) -> usize {
        self.turns.iter().filter(|t| t.human).count()
    }

    pub fn current_turn(&self) -> Option<&Turn> {
        self.turns.last()
    }

    /// Context size the model last saw (0 before any call).
    pub fn context_size(&self) -> u64 {
        self.turns
            .iter()
            .rev()
            .find(|t| t.api_calls > 0)
            .map(|t| t.context_size)
            .unwrap_or(0)
    }

    fn turn_number(&self) -> usize {
        self.turns.len()
    }

    pub fn push(&mut self, line: &Line) {
        if self.version.is_none() {
            self.version = line.version().map(str::to_string);
        }
        if self.session_id.is_none() {
            self.session_id = line.session_id().map(str::to_string);
        }
        if self.team.is_none() {
            self.team = line.team().map(|(t, a)| (t.to_string(), a.to_string()));
        }
        match line {
            Line::User(u) => {
                let kind = u.prompt_kind();
                let starts_turn = matches!(
                    kind,
                    PromptKind::Human
                        | PromptKind::Machine
                        | PromptKind::TaskNotification
                        | PromptKind::TeammateMessage
                );
                let ts = u
                    .timestamp
                    .as_deref()
                    .and_then(crate::metrics::cost::parse_ts_ms);
                if let (Some(ts), Some((prev, LastKind::Assistant)), PromptKind::ToolResult) =
                    (ts, self.last_ts, kind)
                {
                    if let Some(t) = self.turns.last_mut() {
                        t.tool_ms += (ts - prev).max(0);
                    }
                }
                if let Some(ts) = ts {
                    self.last_ts = Some((ts, LastKind::User));
                }
                if starts_turn {
                    self.turns.push(Turn {
                        number: self.turns.len() + 1,
                        prompt_id: u.prompt_id.clone(),
                        human: kind == PromptKind::Human,
                        prompt_source: u.prompt_source.clone(),
                        started_at: u.timestamp.clone(),
                        last_at: u.timestamp.clone(),
                        last_human_input_at: u.timestamp.clone(),
                        prompt_shape: u.prompt_shape(),
                        prompt_chars: u.message.content.text().chars().count(),
                        prompt_images: u.message.content.images(),
                        ..Default::default()
                    });
                    return;
                }
                let turn = self.turn_number();
                match kind {
                    PromptKind::ToolResult => {
                        if let Some(t) = self.turns.last_mut() {
                            if let Content::Blocks(_) = &u.message.content {
                                for r in u.message.content.tool_results() {
                                    t.tool_result_bytes += r.text().len() as u64;
                                    if r.is_error {
                                        t.tool_errors += 1;
                                    }
                                }
                            }
                            if u.tool_denial_kind.is_some() {
                                t.denials += 1;
                            }
                            if u.tool_denial_kind.as_deref() == Some("user-rejected")
                                || u.user_feedback_len.is_some_and(|n| n > 0)
                            {
                                t.rejections += 1;
                            }
                            if matches!(
                                u.tool_use_detail(),
                                Some(crate::transcript::ToolUseDetail::AskUserQuestion(_))
                            ) {
                                t.last_human_input_at = u.timestamp.clone();
                            }
                            t.last_at = u.timestamp.clone().or(t.last_at.take());
                        }
                    }
                    PromptKind::Interrupt => {
                        if let Some(t) = self.turns.last_mut() {
                            t.interrupted_after_calls = Some(t.tool_calls);
                            t.last_at = u.timestamp.clone().or(t.last_at.take());
                            let calls = t.tool_calls;
                            self.interrupts.push((turn, calls));
                        }
                    }
                    PromptKind::SlashCommand => {
                        if let Some(cmd) = u.slash_command() {
                            if cmd == "/clear" {
                                self.boundaries.push(Boundary {
                                    at: u.timestamp.clone(),
                                    turn,
                                    kind: BoundaryKind::Clear,
                                });
                            }
                            self.slash_commands.push((turn, cmd));
                        }
                    }
                    _ => {}
                }
            }
            Line::Assistant(a) => self.push_assistant(a),
            Line::Attachment(att) => {
                use crate::transcript::AttachmentKind;
                match att.kind() {
                    AttachmentKind::QueuedCommand { origin_kind, .. }
                        if origin_kind.as_deref() != Some("task-notification") =>
                    {
                        if let Some(t) = self.turns.last_mut() {
                            t.steers += 1;
                        }
                    }
                    AttachmentKind::GoalStatus {
                        met,
                        tokens,
                        iterations,
                        duration_ms,
                    } => {
                        self.goal = Some(GoalStatus {
                            met,
                            tokens,
                            iterations,
                            duration_ms,
                        });
                    }
                    AttachmentKind::HookSuccess {
                        command,
                        duration_ms,
                        ..
                    } => {
                        *self.hook_ms_by_command.entry(command).or_default() += duration_ms;
                    }
                    AttachmentKind::SilentTurnReminder => {
                        if let Some(t) = self.turns.last_mut() {
                            t.silent_reminders += 1;
                        }
                    }
                    AttachmentKind::Instructions { .. } | AttachmentKind::NestedMemory { .. } => {
                        if let Some(t) = self.turns.last_mut() {
                            t.instructions_seen = true;
                        }
                    }
                    _ => {}
                }
                if let Some(t) = self.turns.last_mut() {
                    let (tokens, approx) = att.tokens_est();
                    t.harness_tokens += tokens;
                    t.harness_approx |= approx && tokens > 0;
                }
            }
            Line::System(s) => {
                use crate::transcript::SystemKind;
                match s.kind() {
                    SystemKind::TurnDuration => {
                        if let (Some(ms), Some(t)) = (s.duration_ms, self.turns.last_mut()) {
                            t.duration_ms = Some(ms);
                            t.ended_at = s.timestamp.clone();
                            t.last_at = s.timestamp.clone().or(t.last_at.take());
                            t.pending_background_agents = s.pending_background_agent_count;
                        }
                    }
                    SystemKind::StopHookSummary => {
                        for h in &s.hook_infos {
                            let cmd = if h.command.is_empty() {
                                "hook".to_string()
                            } else {
                                h.command.clone()
                            };
                            *self.hook_ms_by_command.entry(cmd).or_default() += h.duration_ms;
                        }
                        if let Some(t) = self.turns.last_mut() {
                            t.hook_runs += s.hook_infos.len();
                            t.hook_ms += s.hook_infos.iter().map(|h| h.duration_ms).sum::<u64>();
                            t.hook_errors += s.hook_errors.len();
                            t.hook_blocked |= s.prevented_continuation;
                        }
                    }
                    SystemKind::AwaySummary => self.away.push(Away {
                        at: s.timestamp.clone(),
                        content: s.content.clone().unwrap_or_default(),
                    }),
                    SystemKind::CompactBoundary => {
                        let turn = self.turn_number();
                        let m = s.compact_metadata.clone().unwrap_or_default();
                        self.compactions.push(CompactionRecord {
                            at: s.timestamp.clone(),
                            turn,
                            trigger: m.trigger,
                            pre_tokens: m.pre_tokens,
                            post_tokens: m.post_tokens,
                            duration_ms: m.duration_ms,
                        });
                        self.boundaries.push(Boundary {
                            at: s.timestamp.clone(),
                            turn,
                            kind: BoundaryKind::Compact,
                        });
                    }
                    SystemKind::MicrocompactBoundary => {
                        let turn = self.turn_number();
                        self.boundaries.push(Boundary {
                            at: s.timestamp.clone(),
                            turn,
                            kind: BoundaryKind::Microcompact,
                        });
                    }
                    _ => {}
                }
            }
            Line::QueueOperation(q) => match q.operation.as_str() {
                "enqueue" => self.queued_prompts += 1,
                "popAll" | "clear" => self.queued_prompts = 0,
                _ => {
                    if q.reason.as_deref() == Some("absorbed_mid_turn") {
                        self.steers_absorbed += 1;
                    }
                    self.queued_prompts = self.queued_prompts.saturating_sub(1)
                }
            },
            Line::FileHistorySnapshot(_) => {
                if let Some(t) = self.turns.last_mut() {
                    t.checkpoints += 1;
                }
            }
            Line::ContinuedIn(c) => {
                let turn = self.turn_number();
                self.boundaries.push(Boundary {
                    at: c.timestamp.clone(),
                    turn,
                    kind: BoundaryKind::Clear,
                });
            }
            Line::AiTitle(t) => {
                if !self.custom_title && !t.ai_title.is_empty() {
                    self.title = Some(t.ai_title.clone());
                }
            }
            Line::CustomTitle(t) => {
                if !t.custom_title.is_empty() {
                    self.title = Some(t.custom_title.clone());
                    self.custom_title = true;
                }
            }
            Line::PrLink(p) => {
                if p.pr_number.is_some() {
                    self.pr_number = p.pr_number;
                }
            }
            Line::AgentSetting(a) => {
                if !a.agent_setting.is_empty() {
                    self.agent_setting = Some(a.agent_setting.clone());
                }
            }
            Line::BridgeSession(_) => self.bridged = true,
            _ => {}
        }
    }

    /// Record a boundary the transcript cannot show (a resume or fork seen
    /// by the hook spool).
    pub fn push_boundary(&mut self, kind: BoundaryKind, at_ms: i64) {
        let turn = self.turn_number();
        self.boundaries.push(Boundary {
            at: Some(crate::metrics::cost::format_ts_ms(at_ms)),
            turn,
            kind,
        });
    }

    fn push_assistant(&mut self, a: &AssistantLine) {
        // A response before any user prompt (e.g. resumed session) still
        // needs a turn to live in.
        if self.turns.is_empty() {
            self.turns.push(Turn {
                number: 1,
                human: true,
                started_at: a.timestamp.clone(),
                ..Default::default()
            });
        }
        if a.is_api_error() {
            let turn = self.turn_number();
            let t = self.turns.last_mut().expect("turn exists");
            t.api_errors += 1;
            t.last_at = a.timestamp.clone().or(t.last_at.take());
            let q = a.quota_limits.as_ref();
            self.api_errors.push(ApiError {
                at: a.timestamp.clone(),
                turn,
                confirmed: a.is_api_error_message,
                error: a.error.clone(),
                status: a.api_error_status,
                rate_limit_type: q.and_then(|q| q.rate_limit_type.clone()),
                resets_at: q.and_then(|q| q.resets_at),
                low_priority_retry_after_s: q.and_then(|q| q.low_priority_retry_after_seconds),
            });
            return;
        }
        let ts = a
            .timestamp
            .as_deref()
            .and_then(crate::metrics::cost::parse_ts_ms);
        let is_new_response = !self.seen_ids.contains(&a.message.id);
        if let (Some(ts), Some((prev, LastKind::User)), true) = (ts, self.last_ts, is_new_response)
        {
            if let Some(t) = self.turns.last_mut() {
                t.api_ms += (ts - prev).max(0);
            }
        }
        if let Some(ts) = ts {
            self.last_ts = Some((ts, LastKind::Assistant));
        }
        let t = self.turns.last_mut().expect("turn exists");
        let mut uses = 0;
        for b in &a.message.content {
            if let crate::transcript::AssistantBlock::ToolUse { name, input, .. } = b {
                uses += 1;
                if matches!(
                    name.as_str(),
                    "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
                ) {
                    for p in crate::phase::paths_of(name, input) {
                        if !t.files_edited.contains(&p) {
                            t.files_edited.push(p);
                        }
                    }
                }
            }
        }
        t.tool_calls += uses;
        t.calls_since_text += uses;
        t.last_at = a.timestamp.clone().or(t.last_at.take());
        let prose = a.text_chars();
        if prose > 0 {
            t.ended_with_question = a.ends_with_question();
            t.prose_chars += prose;
            t.last_text_at = a.timestamp.clone();
            t.calls_since_text = 0;
        }
        if !self.seen_ids.insert(a.message.id.clone()) {
            return; // another block of a response already counted
        }
        let u = Usage::from_api(&a.message.usage);
        if t.api_calls == 0 {
            t.first_call_prefix = u.cache_read + u.cache_write();
            t.first_call_input = u.input;
        }
        if let Some(miss) = a.cache_miss_reason().filter(|m| m.is_named()) {
            self.miss_causes_by_turn.push((t.number, miss.kind.clone()));
            self.cache_misses.push(CacheMissRecord {
                at: a.timestamp.clone(),
                turn: t.number,
                kind: miss.kind.clone(),
                tokens: miss
                    .cache_missed_input_tokens
                    .unwrap_or(a.message.usage.cache_creation_input_tokens),
                message_id: a.message.id.clone(),
            });
        }
        t.api_calls += 1;
        t.usage.add(&u);
        t.context_size = u.total_input();
        t.effort = a.effective_effort().map(str::to_string).or(t.effort.take());
        t.last_stop_reason = a.message.stop_reason.clone();
        if !t.models.contains(&a.message.model) {
            t.models.push(a.message.model.clone());
        }
        if let Some(ttl) = a.message.usage.cache_ttl() {
            t.cache_ttl = Some(ttl);
            self.observed_ttl = Some(ttl);
        }
        self.total.add(&u);
        self.model = Some(a.message.model.clone());
        self.last_api_at = a.timestamp.clone().or(self.last_api_at.take());
        let machine = !t.human;
        self.calls.push(CallRecord {
            turn: t.number,
            at_ms: ts,
            model: a.message.model.clone(),
            usage: u,
            machine,
        });
        let owner = if machine {
            Some("idle".to_string())
        } else if let Some(s) = &a.attribution_skill {
            Some(format!("skill:{s}"))
        } else if let Some(p) = &a.attribution_plugin {
            Some(format!("plugin:{p}"))
        } else if let Some(g) = &a.attribution_agent {
            Some(format!("agent:{g}"))
        } else {
            a.attribution_mcp_server
                .as_ref()
                .map(|m| format!("mcp:{m}"))
        };
        if let Some(k) = owner {
            self.attribution.entry(k).or_default().add(&u);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn fixture(name: &str) -> Vec<Line> {
        parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl")))
            .unwrap()
    }

    #[test]
    fn aggregate_records_the_session_id_and_a_teammates_team() {
        let lead = Aggregate::from_lines(&fixture("session-d"));
        assert_eq!(
            lead.session_id.as_deref(),
            Some("afd065d3-054a-5984-b05e-c46a3fa0a9ca")
        );
        assert_eq!(lead.team, None, "the lead's lines carry no team key");
        let teammate = parse_file(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/session-d/teammates/85424f6c-500e-5993-641b-0a66d897a888.jsonl"),
        )
        .unwrap();
        let agg = Aggregate::from_lines(&teammate);
        assert_eq!(
            agg.team,
            Some(("session-afd065d3".into(), "diff-pane-research".into()))
        );
        assert_eq!(
            agg.session_id.as_deref(),
            Some("85424f6c-500e-5993-641b-0a66d897a888")
        );
        // A teammate is a session like any other to the aggregate.
        assert_eq!(agg.api_calls(), 8);
        assert_eq!(agg.human_turns(), 0, "its prompts are teammate messages");
        assert!(!agg.turns.is_empty());
        assert!(fixture("session-a").iter().all(|l| l.team().is_none()));
        assert_eq!(Aggregate::from_lines(&fixture("session-b")).team, None);
        assert_eq!(Aggregate::from_lines(&fixture("session-c")).team, None);
    }

    #[test]
    fn output_tokens_are_counted_once_per_response() {
        let lines = fixture("session-a");
        let agg = Aggregate::from_lines(&lines);
        // Hand-computed from the fixture: 143 distinct ids; naïve per-line sum is 214,682 (3.2×).
        assert_eq!(agg.api_calls(), 143);
        assert_eq!(agg.total.output, 67_061);
        let naive: u64 = lines
            .iter()
            .filter_map(|l| match l {
                Line::Assistant(a) => Some(a.message.usage.output_tokens),
                _ => None,
            })
            .sum();
        assert!(naive > 2 * agg.total.output, "naive {naive} must overstate");
    }

    #[test]
    fn thirteen_block_response_counts_once() {
        let lines = fixture("session-a");
        // Find the id that appears 13 times and aggregate only those lines.
        let mut counts = std::collections::HashMap::new();
        for l in &lines {
            if let Line::Assistant(a) = l {
                *counts.entry(a.message.id.clone()).or_insert(0) += 1;
            }
        }
        let (id, n) = counts.iter().max_by_key(|(_, n)| **n).unwrap();
        assert_eq!(*n, 13);
        let only: Vec<&Line> = lines
            .iter()
            .filter(|l| matches!(l, Line::Assistant(a) if &a.message.id == id))
            .collect();
        let agg = Aggregate::from_lines(only.iter().copied());
        let expected = match only[0] {
            Line::Assistant(a) => a.message.usage.output_tokens,
            _ => unreachable!(),
        };
        assert_eq!(agg.api_calls(), 1);
        assert_eq!(agg.total.output, expected);
        assert_eq!(agg.turns[0].tool_calls, 12, "12 tool_use blocks + 1 text");
    }

    #[test]
    fn turn_count_matches_hand_count() {
        let agg = Aggregate::from_lines(&fixture("session-a"));
        // 15 non-meta text lines in the fixture; the last one is the
        // interrupt that ended the session (`interruptedMessageId`), which
        // is not a turn. (The fixture predates the anonymiser that keeps
        // `promptSource`, so every remaining prompt reads as human.)
        assert_eq!(agg.turns.len(), 14);
        assert_eq!(agg.human_turns(), 14);
        assert_eq!(agg.interrupts, vec![(14, 125)], "after 125 tool calls");
        assert!(agg.slash_commands.is_empty());
        assert_eq!(agg.turns.iter().map(|t| t.api_calls).sum::<usize>(), 143);
        assert!(agg.turns.iter().all(|t| t.number >= 1));
        assert!(agg.turns.iter().all(|t| t.prompt_id.is_some()));
        assert_eq!(
            agg.turns.iter().filter(|t| t.duration_ms.is_some()).count(),
            9
        );
        assert_eq!(agg.turns.iter().map(|t| t.tool_calls).sum::<usize>(), 257);
        assert_eq!(agg.version.as_deref(), Some("2.1.247"));
    }

    #[test]
    fn ttl_and_context_size() {
        let agg = Aggregate::from_lines(&fixture("session-a"));
        assert_eq!(agg.observed_ttl, Some(CacheTtl::OneHour));
        let last = agg.turns.iter().rev().find(|t| t.api_calls > 0).unwrap();
        assert!(last.context_size > 10_000, "{}", last.context_size);
        assert_eq!(last.usage.cache_write_5m, 0);
        assert!(agg.total.cache_write_1h > 0);
        assert!(agg.total.cache_hit_ratio().unwrap() > 0.9);
        assert_eq!(agg.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(last.effort.as_deref(), Some("medium"));
        assert_eq!(agg.context_size(), last.context_size);
    }

    #[test]
    fn turn_duration_hooks_queue_and_away_from_system_lines() {
        let lines = fixture("session-a");
        let agg = Aggregate::from_lines(&lines);
        // Hand-computed from the fixture's system lines.
        assert_eq!(agg.turns[2].duration_ms, Some(95_830), "turn 3");
        assert_eq!(agg.turns[0].duration_ms, Some(20_814));
        assert_eq!(agg.turns[11].duration_ms, Some(398_534));
        assert_eq!(
            agg.turns.iter().map(|t| t.hook_ms).sum::<u64>(),
            37 + 65 + 60 + 33 + 39 + 38 + 59 + 67 + 58
        );
        assert_eq!(agg.turns.iter().map(|t| t.hook_runs).sum::<usize>(), 9);
        assert_eq!(agg.turns.iter().map(|t| t.hook_errors).sum::<usize>(), 0);
        assert_eq!(agg.turns[2].hook_ms, 60);
        // enqueue, remove, enqueue, popAll → 0 queued at the end.
        assert_eq!(agg.queued_prompts, 0);
        let mut partial = Aggregate::default();
        for l in &lines {
            partial.push(l);
            if partial.turns.len() == 14 && partial.queued_prompts == 1 {
                break;
            }
        }
        assert_eq!(
            partial.queued_prompts, 1,
            "queued after the enqueue in turn 14"
        );
        assert_eq!(agg.away.len(), 1);
        assert!(agg.away[0].at.is_some());
        assert!(!agg.away[0].content.is_empty());
        // Live elapsed for a turn without turn_duration.
        let t = &agg.turns[3];
        assert!(t.duration_ms.is_none());
        let start = crate::metrics::cost::parse_ts_ms(t.started_at.as_deref().unwrap()).unwrap();
        assert_eq!(t.elapsed_ms(start + 5_000), Some(5_000));
        assert_eq!(agg.turns[2].elapsed_ms(0), Some(95_830));
        // API + tool time never exceeds the turn's wall time (first to last
        // line), and both are non-trivial on a real turn. Note `turn_duration`
        // (95.8 s) is shorter than that wall time (150 s): Claude Code does
        // not count the whole span.
        let t3 = &agg.turns[2];
        let wall = crate::metrics::cost::parse_ts_ms(t3.last_at.as_deref().unwrap()).unwrap()
            - crate::metrics::cost::parse_ts_ms(t3.started_at.as_deref().unwrap()).unwrap();
        assert!(t3.api_ms > 0 && t3.tool_ms > 0, "{t3:?}");
        assert!(t3.api_ms + t3.tool_ms <= wall, "{t3:?} wall {wall}");
    }

    #[test]
    fn usage_from_api_without_ttl_breakdown_goes_to_5m() {
        let api = crate::transcript::Usage {
            input_tokens: 10,
            cache_creation_input_tokens: 100,
            cache_read_input_tokens: 1000,
            output_tokens: 5,
            ..Default::default()
        };
        let u = Usage::from_api(&api);
        assert_eq!((u.cache_write_5m, u.cache_write_1h), (100, 0));
        assert_eq!(u.total_input(), 1110);
        assert!((u.cache_hit_ratio().unwrap() - 0.9009).abs() < 1e-3);
    }

    // ----- fixture B: the coach's turn identity

    #[test]
    fn fixture_b_turns_errors_compactions_and_boundaries() {
        let lines = fixture("session-b");
        let agg = Aggregate::from_lines(&lines);
        // Six prompts the person wrote; the interrupt, the /clear, the
        // caveat, the compaction summary and the /context lines are not turns.
        assert_eq!(agg.human_turns(), 6);
        assert_eq!(agg.turns.len(), 6);
        let ids: std::collections::HashSet<_> =
            agg.turns.iter().map(|t| t.prompt_id.clone()).collect();
        assert_eq!(ids.len(), 6, "distinct promptIds");
        assert_eq!(agg.version.as_deref(), Some("2.1.270"));
        // API errors are events, not calls, and never the model.
        assert_eq!(agg.api_errors.len(), 2);
        assert!(agg
            .api_errors
            .iter()
            .any(|e| e.error.as_deref() == Some("invalid_request")));
        assert_eq!(agg.turns.iter().map(|t| t.api_errors).sum::<usize>(), 2);
        assert_ne!(agg.model.as_deref(), Some("<synthetic>"));
        let calls: usize = agg.turns.iter().map(|t| t.api_calls).sum();
        assert_eq!(calls, agg.api_calls());
        assert!(agg
            .turns
            .iter()
            .all(|t| t.context_size > 0 || t.api_calls == 0));
        // The exact compaction and the boundaries.
        assert_eq!(agg.compactions.len(), 1);
        let c = &agg.compactions[0];
        assert_eq!(
            (c.pre_tokens, c.post_tokens, c.duration_ms),
            (567_672, 230_014, 80_690)
        );
        assert_eq!(c.trigger, "auto");
        let kinds: Vec<BoundaryKind> = agg.boundaries.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                BoundaryKind::Clear,
                BoundaryKind::Compact,
                BoundaryKind::Clear
            ]
        );
        // The interrupt cut the last turn after its tool calls.
        assert_eq!(agg.interrupts.len(), 1);
        assert!(agg
            .turns
            .iter()
            .any(|t| t.interrupted_after_calls.is_some()));
        // Slash commands, denials, title, PR, harness tokens.
        assert!(agg.slash_commands.iter().any(|(_, c)| c == "/clear"));
        assert_eq!(agg.turns.iter().map(|t| t.denials).sum::<usize>(), 4);
        assert!(agg.title.is_some());
        assert_eq!(agg.pr_number, Some(121));
        assert!(agg.turns.iter().map(|t| t.harness_tokens).sum::<u64>() > 5_000);
        assert!(
            agg.turns.iter().any(|t| t.ended_with_question),
            "the AskUserQuestion turn"
        );
        assert!(agg
            .turns
            .iter()
            .all(|t| t.first_call_input > 0 || t.api_calls == 0));
    }

    #[test]
    fn machine_turns_and_non_turns() {
        let mut a = Aggregate::default();
        let user = |extra: &str, text: &str| {
            Line::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T00:00:00Z","promptId":"p{}","message":{{"role":"user","content":{}}}{extra}}}"#,
                text.len(),
                serde_json::to_string(text).unwrap()
            ))
            .unwrap()
        };
        a.push(&user(
            r#","promptSource":"typed","origin":{"kind":"human"}"#,
            "do it",
        ));
        a.push(&user("", "<command-name>/model</command-name>"));
        a.push(&user(
            r#","promptSource":"system","origin":{"kind":"task-notification"}"#,
            "<task-notification>done</task-notification>",
        ));
        a.push(&user("", "[Request interrupted by user]"));
        a.push(&user(r#","isMeta":true"#, "<local-command-caveat>x"));
        a.push(&user(r#","isCompactSummary":true"#, "summary text"));
        a.push(&user(
            r#","promptSource":"queued","origin":{"kind":"human"}"#,
            "and then this",
        ));
        assert_eq!(a.turns.len(), 3);
        assert_eq!(a.human_turns(), 2);
        assert!(!a.turns[1].human);
        assert_eq!(a.slash_commands, vec![(1, "/model".to_string())]);
        assert_eq!(a.interrupts, vec![(2, 0)]);
        // An API error line in the last turn: an error, not a call.
        a.push(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"oops"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1789244400}}"#).unwrap());
        assert_eq!(a.api_calls(), 0);
        assert_eq!(a.model, None);
        assert_eq!(a.api_errors.len(), 1);
        assert_eq!(
            a.api_errors[0].rate_limit_type.as_deref(),
            Some("five_hour")
        );
        assert_eq!(a.turns[2].api_errors, 1);
        a.push_boundary(BoundaryKind::Resume, 1_700_000_000_000);
        assert_eq!(a.boundaries.last().unwrap().kind, BoundaryKind::Resume);
        assert!(a
            .boundaries
            .last()
            .unwrap()
            .at
            .as_deref()
            .unwrap()
            .starts_with("2023-11-14T22:13:20"));
    }
}
