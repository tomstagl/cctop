//! Everything the panels read. Collectors fill it through [`State::apply`];
//! panels only read it, except for UI-local fields (focus, hidden, sort keys)
//! which key handlers mutate.

use std::path::{Path, PathBuf};

use crate::metrics::{Aggregate, CostTracker, Pricing};
use crate::tools;
use crate::transcript::Line;
use crate::ui::panel::PanelId;

/// How long a toast stays in the footer.
pub const TOAST_MS: i64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionStatus {
    Busy,
    Idle,
    #[default]
    Unknown,
}

/// Identity and liveness of the attached session, from the registry (live)
/// or the transcript (fixture).
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub name: String,
    pub session_id: String,
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    pub version: String,
    pub status: SessionStatus,
    pub started_at_ms: Option<i64>,
    /// False once the pid is gone; values freeze and the header shows ENDED.
    pub alive: bool,
    /// When the session was seen ended (epoch ms).
    pub ended_at_ms: Option<i64>,
    /// Rate-limit tier from `~/.claude.json` (`oauthAccount.userRateLimitTier`).
    pub tier: Option<String>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub cpu_pct: Option<f32>,
    pub rss_bytes: Option<u64>,
    pub permission_mode: Option<String>,
    /// A PermissionRequest hook fired and no tool has completed since.
    pub permission_pending: bool,
    /// When the pending permission request was raised (epoch ms).
    pub permission_waiting_since_ms: Option<i64>,
    /// Hook events are arriving (cctop install ran), so waits are measured.
    pub hooks_installed: bool,
    pub permission_waits: usize,
    pub permission_wait_ms: i64,
    /// Permission prompts per tool name.
    pub permission_by_tool: std::collections::BTreeMap<String, usize>,
    /// The last `Notification` hook: `(notification_type, epoch ms)` —
    /// `permission_prompt`, `idle_prompt`, `agent_needs_input`…; cleared by
    /// the next prompt or tool result.
    pub notification: Option<(String, i64)>,
    /// How the session started or resumed (`SessionStart.source`: startup,
    /// resume, clear, compact, fork).
    pub start_source: Option<String>,
    /// Claude Code's own cold-resume figures (`SessionStart` on a resume).
    pub resume: Option<ResumeInfo>,
    /// `Stop.background_tasks`: what is still running at turn end.
    pub background_tasks: Vec<BackgroundTask>,
    /// `Stop.session_crons` count.
    pub session_crons: usize,
    /// The last `Stop`'s assistant message ended with a question mark.
    pub last_stop_asked: Option<bool>,
    /// Hook `prompt_id` of the last event, to join with the transcript.
    pub hook_prompt_id: Option<String>,
    /// `SessionEnd.reason`, once it fired.
    pub end_reason: Option<String>,
}

/// `SessionStart` on a resume or fork: what Claude Code expects the first
/// call to cost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResumeInfo {
    pub source: String,
    pub seconds_since_last_response: Option<u64>,
    pub context_tokens: Option<u64>,
    pub prompt_cache_likely_expired: Option<bool>,
    pub estimated_cache_write_usd: Option<f64>,
    pub at_ms: i64,
}

/// A background task Claude Code listed at turn end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackgroundTask {
    pub id: String,
    /// `shell`, `agent`, `workflow`…
    pub kind: String,
    pub status: String,
    pub description: String,
}

/// Rate-limit figures from the status line (shim), plus cctop's projection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    pub five_hour_pct: f64,
    pub seven_day_pct: f64,
    pub five_hour_resets_at_ms: Option<i64>,
    pub seven_day_resets_at_ms: Option<i64>,
    /// Projected moment the 5 h limit hits 100 % at the current slope.
    pub exhaustion_ms: Option<i64>,
}

impl SessionInfo {
    /// A live session from the registry.
    pub fn from_registry(s: &crate::registry::Session) -> SessionInfo {
        use crate::registry::Status;
        SessionInfo {
            name: s.name.clone(),
            session_id: s.session_id.clone(),
            pid: Some(s.pid),
            cwd: s.cwd.clone(),
            version: s.version.clone(),
            status: match s.status() {
                Status::Busy => SessionStatus::Busy,
                Status::Idle => SessionStatus::Idle,
                Status::Other => SessionStatus::Unknown,
            },
            started_at_ms: (s.started_at > 0).then_some(s.started_at as i64),
            alive: s.is_alive(),
            ..Default::default()
        }
    }

    /// Messaging socket path if the registry has one and it exists.
    pub fn socket_of(s: &crate::registry::Session) -> Option<std::path::PathBuf> {
        s.messaging_socket_path.clone().filter(|p| p.exists())
    }

    /// A transcript file with no live process behind it.
    pub fn from_fixture(path: &std::path::Path) -> SessionInfo {
        SessionInfo {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            alive: false,
            ..Default::default()
        }
    }

    /// A real transcript whose session the registry no longer lists (the id
    /// was rotated by `/clear`, or the process exited): ended, but keeping
    /// its id so the status and hook spools written under it still join,
    /// and named by the id's first eight characters. `cwd` and `version`
    /// come from the lines as they are applied.
    pub fn from_transcript(path: &std::path::Path) -> SessionInfo {
        let session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        SessionInfo {
            name: session_id.chars().take(8).collect(),
            session_id,
            alive: false,
            ..Default::default()
        }
    }

    /// Re-check liveness; on the first miss, freeze the clock at `now_ms`.
    pub fn refresh_alive(&mut self, now_ms: i64) {
        let Some(pid) = self.pid else {
            return;
        };
        let alive = crate::registry::pid_alive(pid);
        if self.alive && !alive {
            self.ended_at_ms = Some(now_ms);
        }
        self.alive = alive;
    }
}

/// Claude Code's own prompt-cache diagnosis, from the status line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheState {
    /// A `prompt_cache` block has been seen (the shim is installed and the
    /// Claude Code version writes it).
    pub from_shim: bool,
    pub warm: bool,
    pub ttl_ms: Option<i64>,
    /// Epoch ms when the entry expires, while warm.
    pub expires_at_ms: Option<i64>,
    pub recache_tokens_if_cold: u64,
    pub misses: u64,
    pub expected_rebuilds: u64,
    pub last_miss_cause: Option<String>,
    pub miss_causes: std::collections::BTreeMap<String, u64>,
    pub hit_ratio: Option<f64>,
    /// When the status file that carried this was written (epoch ms).
    pub sample_at_ms: i64,
}

/// Facts the status line carries beyond the numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusFacts {
    pub effort_level: Option<String>,
    pub thinking_enabled: Option<bool>,
    pub fast_mode: bool,
    pub exceeds_200k_tokens: bool,
    pub session_name: Option<String>,
    pub prompt_id: Option<String>,
    pub version: Option<String>,
    pub pr_number: Option<u64>,
    pub pr_review_state: Option<String>,
    pub spend_limit_pct: Option<f64>,
}

/// A cache countdown, clock-driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheClock {
    /// Milliseconds until the entry expires (0 when it already has).
    pub remaining_ms: i64,
    /// From the last API call + observed TTL rather than the shim.
    pub approx: bool,
}

#[derive(Debug, Default)]
pub struct State {
    // -- collectors
    pub session: SessionInfo,
    /// Claude Code's prompt-cache diagnosis (shim).
    pub cache: CacheState,
    pub status_facts: StatusFacts,
    /// The previous session in this directory, from `~/.claude.json`.
    pub previous_session: Option<crate::claude_home::PreviousSession>,
    pub agg: Aggregate,
    pub cost: CostTracker,
    pub tools: tools::Stats,
    /// Subagents of this session, by id.
    pub agents: std::collections::BTreeMap<String, crate::agents::Agent>,
    /// Rate limits, when the status-line shim is installed.
    pub limits: Option<Limits>,
    /// `(epoch ms, 5 h used %)` samples for the exhaustion projection.
    pub limits_series_5h: Vec<(i64, f64)>,
    /// Other busy sessions in the registry (they share the rate limit).
    pub other_live_sessions: usize,
    /// The registry entry of this session's pid once it carries another
    /// session id (`/clear` rewrites it under the running process); the
    /// loop re-attaches to it.
    pub rotated_to: Option<crate::registry::Session>,
    /// PreToolUse / PermissionRequest timestamps awaiting their PostToolUse.
    hook_pre: std::collections::HashMap<String, i64>,
    hook_perm: std::collections::HashMap<String, i64>,
    /// MCP servers whose process disappeared since the last evaluation.
    pub mcp_exited: Vec<String>,
    /// Latest process-tree sample (live sessions only).
    pub procs: crate::procs::Snapshot,
    /// Background tasks from `~/.claude/tasks/session-<id8>/`.
    pub tasks: Vec<crate::tasks::Task>,
    pub files: crate::files::Files,
    pub files_sort: FileSort,
    /// Exact context figures from the status line (shim), when present.
    pub context_window_exact: Option<u64>,
    pub context_size_exact: Option<u64>,
    /// Autocompact threshold learned from an observed compaction, by model.
    pub learned_thresholds: std::collections::BTreeMap<String, u64>,
    /// Autocompact overrides from settings and the claude process environment.
    pub autocompact: crate::metrics::context::AutocompactConfig,
    /// The allowlisted variables of the claude process (`—` when absent).
    pub claude_env: std::collections::BTreeMap<String, String>,
    // -- ui
    /// Panel that receives keys; `None` = global.
    pub focused: Option<PanelId>,
    /// Panels the user toggled off with their hotkey digit.
    pub hidden: Vec<PanelId>,
    /// Wall clock for the frame being rendered (epoch ms).
    pub now_ms: i64,
    /// Footer replacement: message and the epoch ms it expires.
    pub toast: Option<(String, i64)>,
    /// Updates are buffered, not applied.
    pub paused: bool,
    /// Lines buffered while paused.
    pub paused_pending: usize,
    /// Transcript lines applied so far.
    pub lines_seen: usize,
    /// Newest transcript timestamp seen (epoch ms).
    pub last_line_at_ms: Option<i64>,
    /// Tokens panel: include subagent usage (toggled with `a`).
    pub tokens_include_agents: bool,
    /// Full-screen view owned by a panel (`Enter`), closed with Esc.
    pub overlay: Option<PanelId>,
    pub tools_ui: ToolsUi,
    pub events: crate::events::Log,
    pub events_ui: EventsUi,
    pub ledger_ui: crate::ui::ledger_view::LedgerUi,
    pub prefix: crate::prefix::Prefix,
    /// Which full-screen view the Context panel shows when it owns the overlay.
    pub context_view: ContextView,
    /// OpenTelemetry data for this session, when the receiver is running.
    pub otel: Option<crate::otel::SessionData>,
    /// Colours and glyphs for this terminal.
    pub theme: crate::theme::Theme,
    /// Session picker overlay (`L`).
    pub picker: Option<crate::ui::picker::PickerUi>,
    /// Drafted question from `a` (panel id, text) awaiting Enter/S/Esc.
    pub ask: Option<(PanelId, String)>,
    /// The session's messaging socket, from the registry.
    pub messaging_socket: Option<std::path::PathBuf>,
    /// Your last-7-days medians, when computed.
    pub baseline: Option<crate::baseline::Baseline>,
    /// Ranked advice from the Advisor engine (best first).
    pub advice: Vec<crate::advisor::Advice>,
    pub advice_index: usize,
    pub advice_dismissed: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextView {
    #[default]
    Ledger,
    Prefix,
}

#[derive(Debug, Clone, Default)]
pub struct EventsUi {
    /// Index of the bottom-most visible event in the overlay; `None` follows the tail.
    pub scroll: Option<usize>,
    pub search: Option<String>,
    pub editing: bool,
}

impl EventsUi {
    /// Move `scroll` to the next (or previous) event matching the search.
    pub fn jump_to_match(&mut self, log: &crate::events::Log, forward: bool) {
        let Some(q) = self.search.as_deref().map(str::to_lowercase) else {
            return;
        };
        if q.is_empty() {
            return;
        }
        let n = log.len();
        let cur = self.scroll.unwrap_or(n.saturating_sub(1));
        let matches = |i: usize| {
            log.iter()
                .nth(i)
                .is_some_and(|e| e.text.to_lowercase().contains(&q))
        };
        let found = if forward {
            (cur + 1..n)
                .chain(0..=cur.min(n.saturating_sub(1)))
                .find(|&i| matches(i))
        } else {
            (0..cur).rev().chain((cur..n).rev()).find(|&i| matches(i))
        };
        if let Some(i) = found {
            self.scroll = Some(i);
        }
    }
}

/// Sort order of the Files panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileSort {
    #[default]
    LastTouch,
    Touches,
    Lines,
    Name,
}

impl FileSort {
    pub fn next(self) -> FileSort {
        match self {
            FileSort::LastTouch => FileSort::Touches,
            FileSort::Touches => FileSort::Lines,
            FileSort::Lines => FileSort::Name,
            FileSort::Name => FileSort::LastTouch,
        }
    }
}

/// Sort column of the Tools table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolSort {
    #[default]
    Calls,
    Errors,
    P50,
    P95,
    Last,
    Tokens,
    Name,
}

impl ToolSort {
    pub fn next(self) -> ToolSort {
        use ToolSort::*;
        match self {
            Calls => Errors,
            Errors => P50,
            P50 => P95,
            P95 => Last,
            Last => Tokens,
            Tokens => Name,
            Name => Calls,
        }
    }
    pub fn label(self) -> &'static str {
        use ToolSort::*;
        match self {
            Calls => "N",
            Errors => "ERR",
            P50 => "p50",
            P95 => "p95",
            Last => "LAST",
            Tokens => "TOKENS→CTX",
            Name => "TOOL",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolsUi {
    pub sort: ToolSort,
    pub ascending: bool,
    /// Substring filter on the tool name; `Some` while active.
    pub filter: Option<String>,
    /// The filter box is taking keystrokes.
    pub editing: bool,
    /// Selected row in the (sorted, filtered) table.
    pub selected: usize,
}

impl State {
    pub fn new(pricing: Pricing) -> State {
        State {
            cost: CostTracker::new(pricing),
            tokens_include_agents: true,
            ..Default::default()
        }
    }

    /// Fold one hook event into timings, permission waits and events.
    pub fn apply_hook(&mut self, ev: &crate::hooks::HookEvent) {
        use crate::events::Kind;
        self.session.hooks_installed = true;
        let at = ev.at;
        let id = ev.tool_use_id.clone().unwrap_or_default();
        let p = &ev.payload;
        let pstr = |k: &str| p.get(k).and_then(|v| v.as_str()).map(str::to_string);
        if let Some(mode) = pstr("permission_mode") {
            self.session.permission_mode = Some(mode);
        }
        if let Some(level) = p
            .get("effort")
            .and_then(|e| e.get("level"))
            .and_then(|v| v.as_str())
        {
            self.status_facts.effort_level = Some(level.to_string());
        }
        if let Some(pid) = pstr("prompt_id") {
            self.session.hook_prompt_id = Some(pid);
        }
        // A subagent's event is the agent's, not the session's.
        if let Some(agent_id) = pstr("agent_id").filter(|a| !a.is_empty()) {
            if !matches!(ev.event.as_str(), "SubagentStart" | "SubagentStop") {
                let agent = self.agents.entry(agent_id.clone()).or_insert_with(|| {
                    crate::agents::Agent::new(
                        &agent_id,
                        crate::agents::Meta {
                            agent_type: pstr("agent_type").unwrap_or_default(),
                            ..Default::default()
                        },
                    )
                });
                agent.note_hook(&ev.event, p.get("duration_ms").and_then(|v| v.as_u64()), at);
                return;
            }
        }
        match ev.event.as_str() {
            "PreToolUse" => {
                if !id.is_empty() {
                    self.hook_pre.insert(id, at);
                }
            }
            "PermissionRequest" => {
                *self
                    .session
                    .permission_by_tool
                    .entry(ev.tool_name.clone().unwrap_or_else(|| "tool".into()))
                    .or_default() += 1;
                self.session.permission_pending = true;
                self.session.permission_waiting_since_ms = Some(at);
                if !id.is_empty() {
                    self.hook_perm.insert(id.clone(), at);
                }
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Perm,
                    text: format!(
                        "{} asks permission",
                        ev.tool_name.as_deref().unwrap_or("tool")
                    ),
                });
            }
            "PostToolUse" | "PostToolUseFailure" => {
                self.session.permission_pending = false;
                self.session.permission_waiting_since_ms = None;
                self.session.notification = None;
                let name = self.tools.get(&id).map(|c| c.name.clone());
                // Exact timing first, so the permission wait below subtracts
                // the tool's real median rather than the transcript's guess.
                // `duration_ms` (2.1.26x) is the tool's own run time, without
                // the permission wait or the hooks; the PreToolUse gap is
                // the fallback.
                let start = self.hook_pre.remove(&id);
                if let Some(d) = p.get("duration_ms").and_then(|v| v.as_i64()) {
                    self.tools.set_exact_duration(&id, at - d, at);
                } else if let Some(start) = start {
                    self.tools.set_exact_duration(&id, start, at);
                }
                if let Some(perm_at) = self.hook_perm.remove(&id) {
                    // ≈ wait: the tool's own median duration is subtracted.
                    let median = name
                        .as_deref()
                        .and_then(|n| self.tools.by_name().get(n).and_then(|t| t.p50_ms))
                        .unwrap_or(0) as i64;
                    let wait = (at - perm_at - median).max(0);
                    self.session.permission_waits += 1;
                    self.session.permission_wait_ms += wait;
                    self.events.push(crate::events::Event {
                        at,
                        kind: Kind::Perm,
                        text: format!(
                            "{} allowed after ≈{}",
                            name.clone().unwrap_or_else(|| "tool".into()),
                            crate::ui::fmt::duration_ms(wait)
                        ),
                    });
                }
                if ev.event == "PostToolUseFailure" {
                    let interrupted = p
                        .get("is_interrupt")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let error = pstr("error").map(|e| crate::ui::fmt::clip(&e, 60));
                    let text = match (interrupted, error) {
                        (true, _) => {
                            format!("{} interrupted", name.unwrap_or_else(|| "tool".into()))
                        }
                        (false, Some(e)) => {
                            format!("{} failed: {e}", name.unwrap_or_else(|| "tool".into()))
                        }
                        (false, None) => {
                            format!("{} failed", name.unwrap_or_else(|| "tool".into()))
                        }
                    };
                    self.events.push(crate::events::Event {
                        at,
                        kind: Kind::Api,
                        text,
                    });
                }
            }
            "PermissionDenied" => self.events.push(crate::events::Event {
                at,
                kind: Kind::Perm,
                text: format!(
                    "{} denied{}",
                    ev.tool_name.as_deref().unwrap_or("tool"),
                    pstr("reason")
                        .map(|r| format!(": {}", crate::ui::fmt::clip(&r, 60)))
                        .unwrap_or_default()
                ),
            }),
            "PreCompact" => {
                let size = self.context().size;
                if let Some(m) = self.model().map(str::to_string) {
                    let entry = self.learned_thresholds.entry(m).or_insert(size);
                    *entry = (*entry).max(size);
                }
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Compact,
                    text: format!("compacting at {}", crate::ui::fmt::tokens(size)),
                });
            }
            "SubagentStart" | "SubagentStop" => self.events.push(crate::events::Event {
                at,
                kind: Kind::Agent,
                text: ev.event.trim_start_matches("Subagent").to_lowercase(),
            }),
            "Notification" => {
                let kind = pstr("notification_type").unwrap_or_else(|| "notification".into());
                let msg = ev
                    .payload
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(kind.as_str())
                    .to_string();
                if kind == "permission_prompt" && !self.session.permission_pending {
                    // Auto mode never raises PermissionRequest; the notification
                    // is the only sign the model is waiting on a dialog.
                    self.session.permission_pending = true;
                    self.session.permission_waiting_since_ms = Some(at);
                }
                self.session.notification = Some((kind, at));
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Note,
                    text: crate::ui::fmt::clip(&msg, 80),
                });
            }
            "UserPromptSubmit" => {
                self.session.notification = None;
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Hook,
                    text: ev.event.clone(),
                })
            }
            "SessionStart" => {
                let source = pstr("source").unwrap_or_else(|| "startup".into());
                self.session.start_source = Some(source.clone());
                let n = |k: &str| p.get(k).and_then(|v| v.as_u64());
                if p.get("context_tokens").is_some()
                    || p.get("seconds_since_last_response").is_some()
                {
                    self.session.resume = Some(ResumeInfo {
                        source: source.clone(),
                        seconds_since_last_response: n("seconds_since_last_response"),
                        context_tokens: n("context_tokens"),
                        prompt_cache_likely_expired: p
                            .get("prompt_cache_likely_expired")
                            .and_then(|v| v.as_bool()),
                        estimated_cache_write_usd: p
                            .get("estimated_cache_write_usd")
                            .and_then(|v| v.as_f64()),
                        at_ms: at,
                    });
                }
                use crate::metrics::usage::BoundaryKind;
                match source.as_str() {
                    "resume" => self.agg.push_boundary(BoundaryKind::Resume, at),
                    "fork" => self.agg.push_boundary(BoundaryKind::Fork, at),
                    "compact" => self.agg.push_boundary(BoundaryKind::Compact, at),
                    _ => {}
                }
                if source != "startup" {
                    self.files.boundary();
                }
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Hook,
                    text: format!("SessionStart {source}"),
                });
            }
            "Stop" => {
                self.session.background_tasks = p
                    .get("background_tasks")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|t| BackgroundTask {
                                id: t
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                kind: t
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("task")
                                    .to_string(),
                                status: t
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                description: crate::ui::fmt::clip(
                                    t.get("description")
                                        .or_else(|| t.get("command"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(""),
                                    60,
                                ),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.session.session_crons = p
                    .get("session_crons")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                self.session.last_stop_asked = p
                    .get("last_assistant_message_ends_with_question")
                    .and_then(|v| v.as_bool());
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Hook,
                    text: if self.session.background_tasks.is_empty() {
                        "Stop".into()
                    } else {
                        format!("Stop · {} background", self.session.background_tasks.len())
                    },
                })
            }
            "StopFailure" => self.events.push(crate::events::Event {
                at,
                kind: Kind::Api,
                text: format!(
                    "turn died: {}",
                    pstr("error")
                        .map(|e| crate::ui::fmt::clip(&e, 60))
                        .unwrap_or_else(|| "error".into())
                ),
            }),
            "SessionEnd" => {
                self.session.end_reason = pstr("reason");
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Hook,
                    text: format!(
                        "SessionEnd{}",
                        self.session
                            .end_reason
                            .as_deref()
                            .map(|r| format!(" ({r})"))
                            .unwrap_or_default()
                    ),
                })
            }
            "PostCompact" => self.events.push(crate::events::Event {
                at,
                kind: Kind::Compact,
                text: "compaction done (hook)".into(),
            }),
            "PreModelSwitch" | "PostModelSwitch" => {
                let from = pstr("from_model").unwrap_or_default();
                let to = pstr("to_model").unwrap_or_default();
                let warm = p.get("prompt_cache_warm").and_then(|v| v.as_bool());
                let usd = p.get("estimated_cache_write_usd").and_then(|v| v.as_f64());
                let mut text = format!(
                    "{} {from} → {to}",
                    if ev.event == "PreModelSwitch" {
                        "model switch"
                    } else {
                        "model switched"
                    }
                );
                if warm == Some(true) {
                    text.push_str(" · cache warm");
                }
                if let Some(u) = usd {
                    text.push_str(&format!(" · re-write ≈{}", crate::ui::fmt::usd(u)));
                }
                self.events.push(crate::events::Event {
                    at,
                    kind: Kind::Api,
                    text,
                });
            }
            "TaskCreated"
            | "TaskCompleted"
            | "InstructionsLoaded"
            | "UserPromptExpansion"
            | "PostToolBatch" => self.events.push(crate::events::Event {
                at,
                kind: Kind::Hook,
                text: match ev.event.as_str() {
                    "InstructionsLoaded" => format!(
                        "instructions loaded{}",
                        pstr("memory_type")
                            .map(|m| format!(" ({m})"))
                            .unwrap_or_default()
                    ),
                    "UserPromptExpansion" => format!(
                        "skill expanded{}",
                        pstr("command_name")
                            .map(|c| format!(" {c}"))
                            .unwrap_or_default()
                    ),
                    other => other.to_string(),
                },
            }),
            _ => {}
        }
    }

    /// Take the receiver's data for this session: exact tool durations by
    /// tool_use_id, per-tool duration samples, TTFT.
    pub fn apply_otel(&mut self, data: &crate::otel::SessionData) {
        for t in &data.tool_results {
            if let (Some(id), Some(d)) = (&t.tool_use_id, t.duration_ms) {
                if let Some(c) = self.tools.get(id) {
                    let start = c.started_at.unwrap_or(t.at_ms - d as i64);
                    self.tools.set_exact_duration(id, start, start + d as i64);
                }
            }
        }
        self.tools.otel_durations = data.durations_by_tool();
        self.otel = Some(data.clone());
    }

    /// Take a status-line sample: exact context, rate limits, the cache
    /// diagnosis and the facts the payload carries.
    pub fn apply_status(&mut self, s: &crate::status::Sample, series_5h: &[(i64, f64)]) {
        if s.context_window.context_window_size > 0 {
            self.context_window_exact = Some(s.context_window.context_window_size);
            self.context_size_exact = Some(s.context_window.total_input_tokens);
        }
        if let Some(pc) = &s.prompt_cache {
            self.cache = CacheState {
                from_shim: true,
                warm: pc.warm,
                ttl_ms: pc.ttl_ms(),
                expires_at_ms: pc.expires_at.map(|e| (e * 1000.0) as i64),
                recache_tokens_if_cold: pc.recache_tokens_if_cold,
                misses: pc.misses,
                expected_rebuilds: pc.expected_rebuilds,
                last_miss_cause: pc.last_cause().map(str::to_string),
                miss_causes: pc.miss_causes.clone(),
                hit_ratio: pc.hit_ratio,
                sample_at_ms: s.at_ms,
            };
        }
        let facts = &mut self.status_facts;
        if let Some(e) = s.effort_level() {
            facts.effort_level = Some(e.to_string());
        }
        if let Some(t) = s.thinking_enabled() {
            facts.thinking_enabled = Some(t);
        }
        facts.fast_mode = s.fast_mode;
        facts.exceeds_200k_tokens = s.exceeds_200k_tokens;
        if s.session_name.is_some() {
            facts.session_name = s.session_name.clone();
        }
        if s.prompt_id.is_some() {
            facts.prompt_id = s.prompt_id.clone();
        }
        if s.version.is_some() {
            facts.version = s.version.clone();
        }
        if let Some(n) = s.pr_number() {
            facts.pr_number = Some(n);
        }
        if let Some(r) = s.pr_review_state() {
            facts.pr_review_state = Some(r.to_string());
        }
        facts.spend_limit_pct = s
            .rate_limits
            .as_ref()
            .and_then(|rl| rl.spend_limit.as_ref())
            .map(|l| l.used_percentage);
        if let Some(rl) = &s.rate_limits {
            let to_ms = |secs: Option<f64>| secs.map(|x| (x * 1000.0) as i64);
            self.limits = Some(Limits {
                five_hour_pct: rl.five_hour.used_percentage,
                seven_day_pct: rl.seven_day.used_percentage,
                five_hour_resets_at_ms: to_ms(rl.five_hour.resets_at),
                seven_day_resets_at_ms: to_ms(rl.seven_day.resets_at),
                exhaustion_ms: None,
            });
            self.limits_series_5h = series_5h.to_vec();
            let ex = crate::metrics::limits::exhaustion(
                &self.limits_series_5h,
                s.at_ms.max(self.now_ms),
            );
            if let Some(l) = self.limits.as_mut() {
                l.exhaustion_ms = ex;
            }
        }
    }

    /// The task list for the Agents panel: what the last `Stop` hook listed
    /// (exact, when hooks are installed), else what the task directory
    /// holds (older Claude Code versions).
    pub fn refresh_tasks(&mut self, dir_tasks: Vec<crate::tasks::Task>) {
        if self.session.background_tasks.is_empty() {
            self.tasks = dir_tasks;
            return;
        }
        let now = self.clock_ms();
        self.tasks = self
            .session
            .background_tasks
            .iter()
            .map(|t| crate::tasks::Task {
                id: t.id.clone(),
                kind: t.kind.clone(),
                description: t.description.clone(),
                started_at_ms: now,
                status: Some(t.status.clone()),
            })
            .chain(dir_tasks)
            .collect();
    }

    /// Take what `~/.claude.json` says about the account and this directory.
    pub fn apply_claude_home(&mut self, v: &serde_json::Value) {
        if let Some(t) = crate::claude_home::rate_limit_tier(v) {
            self.session.tier = Some(t);
        }
        self.previous_session = crate::claude_home::previous_session(v, &self.session.cwd);
    }

    /// The observed cache TTL in milliseconds: the shim's, else the last
    /// call's `ephemeral_*` split, else 5 minutes.
    pub fn cache_ttl_ms(&self) -> i64 {
        self.cache.ttl_ms.unwrap_or(match self.agg.observed_ttl {
            Some(crate::transcript::CacheTtl::OneHour) => 3_600_000,
            _ => 300_000,
        })
    }

    /// Epoch ms of the last API response, if any.
    pub fn last_api_call_ms(&self) -> Option<i64> {
        self.agg
            .last_api_at
            .as_deref()
            .and_then(crate::metrics::cost::parse_ts_ms)
    }

    /// The cache countdown at `clock_ms()`: from `prompt_cache.expires_at`
    /// when the status file is at least as new as the last assistant line;
    /// otherwise from the last API call plus the observed TTL, marked `≈`.
    /// `None` before the first call.
    pub fn cache_clock(&self) -> Option<CacheClock> {
        let now = self.clock_ms();
        let last_call = self.last_api_call_ms();
        let fresh =
            self.cache.from_shim && last_call.is_none_or(|c| self.cache.sample_at_ms + 2_000 >= c);
        if fresh {
            if !self.cache.warm {
                return Some(CacheClock {
                    remaining_ms: 0,
                    approx: false,
                });
            }
            if let Some(exp) = self.cache.expires_at_ms {
                return Some(CacheClock {
                    remaining_ms: (exp - now).max(0),
                    approx: false,
                });
            }
        }
        let last = last_call?;
        Some(CacheClock {
            remaining_ms: (last + self.cache_ttl_ms() - now).max(0),
            approx: true,
        })
    }

    /// Whether the prompt cache is warm right now, with the `≈` flag.
    pub fn cache_warm(&self) -> Option<(bool, bool)> {
        self.cache_clock().map(|c| (c.remaining_ms > 0, c.approx))
    }

    /// "Now" for elapsed-time arithmetic: frozen at the end for dead sessions.
    pub fn clock_ms(&self) -> i64 {
        if self.session.alive {
            self.now_ms
        } else {
            self.session.ended_at_ms.unwrap_or(self.now_ms)
        }
    }

    /// Total usage of all subagents.
    pub fn agents_usage(&self) -> crate::metrics::Usage {
        let mut u = crate::metrics::Usage::default();
        for a in self.agents.values() {
            u.add(&a.usage);
        }
        u
    }

    /// Feed one main-transcript line to every collector.
    pub fn apply(&mut self, line: &Line) {
        let turns_before = self.agg.turns.len();
        self.agg.push(line);
        if self.agg.turns.len() > turns_before {
            self.files.new_turn();
        }
        self.cost.push(line);
        self.tools.push(line);
        self.events.apply(line);
        self.files.push(line);
        self.prefix.push(line);
        let ts = match line {
            Line::PermissionMode(p) => {
                self.session.permission_mode = Some(p.permission_mode.clone());
                None
            }
            Line::User(u) => {
                self.note_origin(u.version.as_deref(), u.cwd.as_deref());
                u.timestamp.as_deref()
            }
            Line::Assistant(a) => {
                self.note_origin(a.version.as_deref(), a.cwd.as_deref());
                a.timestamp.as_deref()
            }
            Line::System(sys) => sys.timestamp.as_deref(),
            _ => None,
        };
        if let Some(ms) = ts.and_then(crate::metrics::cost::parse_ts_ms) {
            self.last_line_at_ms = Some(self.last_line_at_ms.map_or(ms, |m| m.max(ms)));
        }
        self.lines_seen += 1;
    }

    fn note_origin(&mut self, version: Option<&str>, cwd: Option<&str>) {
        if let Some(v) = version {
            if self.session.version.is_empty() {
                self.session.version = v.to_string();
            }
        }
        if let Some(c) = cwd {
            if self.session.cwd.as_os_str().is_empty() {
                self.session.cwd = PathBuf::from(c);
            }
            self.files.set_cwd(Path::new(c));
        }
    }

    /// Model currently in use.
    pub fn model(&self) -> Option<&str> {
        self.agg.model.as_deref()
    }

    /// The three bands of the context gauge for the current window.
    pub fn bands(&self) -> crate::metrics::context::Bands {
        crate::metrics::context::bands(self.context().window, &self.autocompact)
    }

    /// First turn after the last context boundary (1 when none).
    pub fn since_boundary_turn(&self) -> usize {
        self.agg
            .boundaries
            .last()
            .map(|b| b.turn.max(1))
            .unwrap_or(1)
    }

    /// What the context is made of since the last boundary.
    pub fn anatomy(&self) -> crate::metrics::context::Anatomy {
        let v = self.context();
        crate::metrics::context::anatomy(
            &self.agg,
            &self.tools,
            v.size,
            v.prefix,
            self.since_boundary_turn(),
        )
    }

    /// Harness tokens per human turn since the last boundary.
    pub fn harness_per_turn(&self) -> Option<(u64, bool)> {
        let since = self.since_boundary_turn();
        let turns: Vec<&crate::metrics::Turn> = self
            .agg
            .turns
            .iter()
            .filter(|t| t.number >= since && t.human)
            .collect();
        if turns.is_empty() {
            return None;
        }
        let total: u64 = turns.iter().map(|t| t.harness_tokens).sum();
        let approx = turns.iter().any(|t| t.harness_approx);
        Some((total / turns.len() as u64, approx))
    }

    /// Read the autocompact overrides for a live session: settings.json and
    /// the claude process environment.
    pub fn load_autocompact(&mut self) {
        let settings = crate::install::read_settings(&crate::install::settings_path());
        if let Some(pid) = self.session.pid {
            self.claude_env = crate::procenv::env_of(pid);
        }
        self.autocompact = crate::metrics::context::AutocompactConfig::from_sources(
            Some(&settings),
            &self.claude_env,
        );
    }

    /// Context-window view for the current model.
    pub fn context(&self) -> crate::metrics::ContextView {
        let learned = self
            .model()
            .and_then(|m| self.learned_thresholds.get(m).copied());
        let mut v = crate::metrics::context::view(
            &self.agg,
            self.context_window_exact,
            self.context_size_exact,
            learned,
        );
        if !v.threshold_learned {
            v.threshold = crate::metrics::context::bands(v.window, &self.autocompact).threshold;
        }
        v
    }

    pub fn is_hidden(&self, id: PanelId) -> bool {
        self.hidden.contains(&id)
    }

    /// Show `msg` in the footer for [`TOAST_MS`].
    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.now_ms + TOAST_MS));
    }

    /// The toast text if it has not expired.
    pub fn toast_text(&self) -> Option<&str> {
        match &self.toast {
            Some((m, until)) if *until > self.now_ms => Some(m.as_str()),
            _ => None,
        }
    }

    pub fn toggle_hidden(&mut self, id: PanelId) {
        if let Some(i) = self.hidden.iter().position(|&h| h == id) {
            self.hidden.remove(i);
        } else {
            self.hidden.push(id);
        }
    }
}

#[cfg(test)]
pub mod tests_support {
    use super::State;
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use std::path::Path;

    /// The session-a fixture folded into a fresh state.
    pub fn fixture_state() -> State {
        let mut s = State::new(Pricing::bundled());
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        for l in parse_file(path).unwrap() {
            s.apply(&l);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;

    fn response(at: &str, cache_write_1h: u64) -> Line {
        Line::parse(&format!(
            r#"{{"type":"assistant","timestamp":"{at}","message":{{"id":"m-{at}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":100000,"cache_creation_input_tokens":{cache_write_1h},"cache_creation":{{"ephemeral_1h_input_tokens":{cache_write_1h},"ephemeral_5m_input_tokens":0}}}}}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn cache_clock_prefers_a_fresh_shim_sample_and_falls_back_marked_approx() {
        let mut s = State::new(Pricing::bundled());
        let t0 = crate::metrics::cost::parse_ts_ms("2026-01-01T10:00:00Z").unwrap();
        assert_eq!(s.cache_clock(), None, "before the first call");
        s.apply(&response("2026-01-01T10:00:00Z", 5000));
        s.now_ms = t0 + 10 * 60_000;
        // No shim: last call + observed TTL (1 h), marked ≈.
        assert_eq!(
            s.cache_clock(),
            Some(CacheClock {
                remaining_ms: 50 * 60_000,
                approx: true
            })
        );
        assert_eq!(s.cache_warm(), Some((true, true)));
        // A shim sample newer than the last call: its expires_at, exact,
        // and the countdown runs on the clock between rewrites.
        let mut sample = crate::status::Sample::parse(&format!(
            r#"{{"session_id":"s","prompt_cache":{{"warm":true,"ttl":"1h","expires_at":{},"misses":2,"expected_rebuilds":1,"recache_tokens_if_cold":291664,"last_miss_cause":{{"causes":["model_changed"]}},"miss_causes":{{"model_changed":1,"ttl_expired_1h":1}}}},"effort":{{"level":"max"}},"thinking":{{"enabled":true}},"fast_mode":true,"pr":{{"number":7}}}}"#,
            (t0 + 3_600_000) / 1000
        ))
        .unwrap();
        sample.at_ms = t0 + 1_000;
        s.apply_status(&sample, &[]);
        assert!(s.cache.from_shim);
        assert_eq!(s.cache.last_miss_cause.as_deref(), Some("model_changed"));
        assert_eq!(s.cache.misses, 2);
        assert_eq!(s.status_facts.effort_level.as_deref(), Some("max"));
        assert_eq!(s.status_facts.thinking_enabled, Some(true));
        assert!(s.status_facts.fast_mode);
        assert_eq!(s.status_facts.pr_number, Some(7));
        assert_eq!(
            s.cache_clock(),
            Some(CacheClock {
                remaining_ms: 50 * 60_000,
                approx: false
            })
        );
        s.now_ms = t0 + 20 * 60_000;
        assert_eq!(s.cache_clock().unwrap().remaining_ms, 40 * 60_000);
        // A later API call the sample predates: the shim's figure is stale,
        // fall back to the observed TTL, marked ≈.
        s.apply(&response("2026-01-01T10:30:00Z", 0));
        s.now_ms = t0 + 31 * 60_000;
        assert_eq!(
            s.cache_clock(),
            Some(CacheClock {
                remaining_ms: 59 * 60_000,
                approx: true
            })
        );
        // A cold shim sample newer than the last call: cold, exact.
        let mut cold = crate::status::Sample::parse(r#"{"session_id":"s","prompt_cache":{"warm":false,"ttl":"1h","expires_at":null,"recache_tokens_if_cold":300000}}"#).unwrap();
        cold.at_ms = t0 + 31 * 60_000;
        s.apply_status(&cold, &[]);
        assert_eq!(
            s.cache_clock(),
            Some(CacheClock {
                remaining_ms: 0,
                approx: false
            })
        );
        assert_eq!(s.cache_warm(), Some((false, false)));
        assert_eq!(s.cache.recache_tokens_if_cold, 300_000);
    }
}
