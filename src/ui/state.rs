//! Everything the panels read. Collectors fill it through [`State::apply`];
//! panels only read it, except for UI-local fields (focus, hidden, sort keys)
//! which key handlers mutate.

use std::path::{Path, PathBuf};

use crate::metrics::cost::Cost;
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
    /// Permission prompts per rule-shaped key (`Bash(gh api)`, `Edit`),
    /// with the rule Claude Code itself suggested, verbatim.
    pub permission_asks: std::collections::BTreeMap<String, PermissionAsk>,
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

/// A `PreModelSwitch` hook event.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSwitch {
    pub at_ms: i64,
    pub from: String,
    pub to: String,
    /// `command`, `picker`, `auto`…
    pub source: Option<String>,
    pub cache_warm: Option<bool>,
    pub context_tokens: Option<u64>,
    pub estimated_cache_write_usd: Option<f64>,
}

/// One permission-prompt shape and what Claude Code offered to allow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionAsk {
    pub count: usize,
    /// `permission_suggestions[].rules[].ruleContent` as `Tool(content)`.
    pub rule: Option<String>,
}

/// The rule-shaped key of a permission prompt: `Bash(<argv0> <sub>)` for
/// Bash (the subcommand only when it is a word, not a flag or a path),
/// else the tool name.
pub fn permission_key(tool: &str, command: Option<&str>) -> String {
    if tool != "Bash" {
        return tool.to_string();
    }
    let Some(cmd) = command else {
        return "Bash".to_string();
    };
    let mut words = cmd.split_whitespace();
    let argv0 = words.next().unwrap_or("");
    let sub = words
        .next()
        .filter(|w| !w.starts_with('-') && !w.contains('/') && !w.contains('=') && w.len() <= 20)
        .map(|w| format!(" {w}"))
        .unwrap_or_default();
    if argv0.is_empty() {
        "Bash".to_string()
    } else {
        format!("Bash({argv0}{sub})")
    }
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
    /// Whether that moment falls in an hour you usually work (from the
    /// baseline's active hours); `None` without a baseline or projection.
    pub exhaustion_in_active_hours: Option<bool>,
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

/// What the model waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitingKind {
    /// A permission dialog is open.
    Permission,
    /// An `AskUserQuestion` / `ExitPlanMode` call has no answer yet.
    Question,
    /// Claude Code notified (`idle_prompt`, `agent_needs_input`).
    Notification,
    /// The turn ended with a question mark and nothing came back.
    Asked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting {
    pub kind: WaitingKind,
    pub since_ms: i64,
}

/// A cache countdown, clock-driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheClock {
    /// Milliseconds until the entry expires (0 when it already has).
    pub remaining_ms: i64,
    /// From the last API call + observed TTL rather than the shim.
    pub approx: bool,
}

/// The session's money as Panel 2, dashboard row 2, `cctop report` and
/// `cctop query` print it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostBreakdown {
    /// Combined (main + agents after the ledger) with `a` on; `current()`
    /// with it off.
    pub headline: Cost,
    /// Always the combined figure (`×N$` compares it with the combined
    /// 7-day baseline whatever `a` says).
    pub combined: Cost,
    /// Claude Code's `cost-state.totalCostUSD`, when one exists.
    pub ledger: Option<f64>,
    /// Priced after the ledger's moment: main responses, and the agents'
    /// calls when they are included; every priced part without a ledger.
    pub since: Option<Cost>,
    /// Every agent call priced — the whole session's agent spend, part of
    /// which the ledger already holds.
    pub agents: Option<Cost>,
    /// `agents ÷ combined`.
    pub agents_share: f64,
    /// Any agent has usage: the breakdown line is drawn.
    pub any_agents: bool,
    /// Tokens of agents on models the price table does not know: in no
    /// dollar figure above, named on the breakdown line instead.
    pub unpriced_agent_tokens: u64,
}

/// What the main transcript says about one subagent: its launch and its
/// task notification. Keyed by the agent id, which is also the
/// notification's `<task-id>`.
#[derive(Debug, Clone, Default)]
pub struct AgentLink {
    pub tool_use_id: Option<String>,
    pub launched_turn: Option<usize>,
    pub agent_type: Option<String>,
    pub resolved_model: Option<String>,
    /// An `Agent` result with this id was seen, so a placeholder row is
    /// justified while (or if ever) the transcript is missing.
    pub spawned: bool,
    pub notified: Option<crate::transcript::TaskNotification>,
    /// Length of a synchronous result's content.
    pub sync_result_chars: Option<usize>,
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
    /// Claude Code's own session analysis (`usage-data`), counts and
    /// verdicts only; live sessions of this machine, never a fixture.
    pub insights: Option<crate::insights::Insights>,
    /// The newest `usage-data` mtime applied, for the re-read on change.
    pub insights_mtime: Option<i64>,
    pub agg: Aggregate,
    pub cost: CostTracker,
    pub tools: tools::Stats,
    /// Subagents of this session, by id.
    pub agents: std::collections::BTreeMap<String, crate::agents::Agent>,
    /// The main transcript's side of each agent, by agent id: the launch
    /// (`Agent` result) and the task notification that ended it. Applied
    /// to `agents` whenever they are (re)loaded, so a placeholder or a
    /// notification is never lost to the watcher's next poll.
    pub agent_links: std::collections::BTreeMap<String, AgentLink>,
    /// A `Workflow` run's task notification, by run id (`wf_…`) when the
    /// launch was seen, else by its task id.
    pub workflow_notifications:
        std::collections::BTreeMap<String, crate::transcript::TaskNotification>,
    /// Workflow runs under `subagents/workflows/`, with their failures.
    pub workflow_journals: Vec<crate::agents::WorkflowJournal>,
    /// Members of this session's team, when it leads one.
    pub teammates: Vec<crate::agents::Teammate>,
    /// The team's own transcripts, read as sessions (team PRD): each
    /// member's aggregate, ledger and liveness. `None` for a session that
    /// leads no team.
    pub team: Option<crate::team::Team>,
    /// MCP servers Claude Code reported as needing auth, pending or failed
    /// (`deferred_tools_delta`).
    pub mcp_needs_auth: Vec<String>,
    pub mcp_failed: Vec<String>,
    /// Rate limits, when the status-line shim is installed.
    pub limits: Option<Limits>,
    /// `(epoch ms, 5 h used %)` samples for the exhaustion projection.
    pub limits_series_5h: Vec<(i64, f64)>,
    /// Other busy sessions in the registry (they share the rate limit).
    pub other_live_sessions: usize,
    /// Every other live session: `(name, busy, epoch ms its status last
    /// changed)`, for "idle since" on the Limits panel.
    pub other_sessions: Vec<(String, bool, i64)>,
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
    /// `git diff --numstat HEAD`: `(added, removed, files)` not committed.
    pub uncommitted: Option<(u64, u64, usize)>,
    /// Exact context figures from the status line (shim), when present.
    pub context_window_exact: Option<u64>,
    pub context_size_exact: Option<u64>,
    /// Autocompact threshold learned from an observed compaction, by model.
    pub learned_thresholds: std::collections::BTreeMap<String, u64>,
    /// Autocompact overrides from settings and the claude process environment.
    pub autocompact: crate::metrics::context::AutocompactConfig,
    /// The allowlisted variables of the claude process (`—` when absent).
    pub claude_env: std::collections::BTreeMap<String, String>,
    /// `permissions.allow` from settings, as written.
    pub allow_rules: Vec<String>,
    /// What `~/.claude/history.jsonl` says about this session and project
    /// (slash commands and paste sizes only).
    pub history: crate::history::History,
    /// Model switches the hook spool reported (`PreModelSwitch`):
    /// `(epoch ms, from, to, source, cache warm, context tokens)`.
    pub model_switches: Vec<ModelSwitch>,
    /// Claude Code's own tips shown in the last ten startups (`tipsHistory`
    /// ids): the coach does not repeat what its host just said.
    pub tips_recent: std::collections::BTreeSet<String>,
    // -- ui
    /// Wall clock for the frame being rendered (epoch ms).
    pub now_ms: i64,
    /// `now_ms` is the clock even for a dead session (`CCTOP_FAKE_NOW`):
    /// renders of a fixture at a chosen moment.
    pub clock_override: bool,
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
    /// The panel open full-screen (its digit), closed with Esc.
    pub open: Option<PanelId>,
    /// A view the open panel owns on top of itself (`Enter`: a ledger, a
    /// call's detail, an explanation), closed with Esc.
    pub overlay: Option<PanelId>,
    pub tools_ui: ToolsUi,
    pub events: crate::events::Log,
    pub events_ui: EventsUi,
    pub ledger_ui: crate::ui::ledger_view::LedgerUi,
    pub agents_ui: crate::ui::agents_view::AgentsUi,
    /// Times the agents view was opened this session (A48's `acted`).
    pub agents_view_opens: u64,
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
    /// The draft may go over the messaging socket (`S`): a question or a
    /// prompt-class action, never a settings snippet.
    pub ask_send_ok: bool,
    /// Dashboard or coach.
    pub view: View,
    pub coach_ui: CoachUi,
    /// The session's messaging socket, from the registry.
    pub messaging_socket: Option<std::path::PathBuf>,
    /// Your last-7-days medians, when computed.
    pub baseline: Option<crate::baseline::Baseline>,
    /// Advice from the Advisor engine: the slot occupant first, then the
    /// ranked queue.
    pub advice: Vec<crate::advisor::Advice>,
    pub advice_index: usize,
    /// Snoozes the panel asked for: `(rule, for the session)`; the app
    /// hands them to the engine.
    pub advice_snoozed: Vec<(&'static str, bool)>,
    /// The person pressed Enter on the occupant: the engine marks it acting.
    pub advice_acting: bool,
    /// What the engine says beside the list.
    pub advice_view: AdviceView,
}

/// The engine's state beside the advice list, for the panel and the query.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdviceView {
    pub session_mode: Option<crate::advisor::SessionMode>,
    /// `advice[0]` is the slot occupant (not merely the top of the queue).
    pub has_occupant: bool,
    pub acting: bool,
    /// What promotes the queued nudge (`next prompt`, `when the slot frees`).
    pub next_condition: Option<&'static str>,
    pub snoozed: Vec<(&'static str, Option<usize>)>,
    pub suppressed: Vec<(&'static str, String)>,
    pub recent: Vec<crate::advisor::Lifecycle>,
}

/// Which full-screen composition the TUI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Dashboard,
    Coach,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Dashboard => "dashboard",
            View::Coach => "coach",
        }
    }
    pub fn parse(s: &str) -> Option<View> {
        match s {
            "dashboard" => Some(View::Dashboard),
            "coach" => Some(View::Coach),
            _ => None,
        }
    }
}

/// The coach view's own state (peek, detail, overlays, units).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoachUi {
    /// `n` / `N`: which ranked nudge the slot shows (0 = the occupant).
    pub peek: usize,
    /// `1`–`4`: a light's three-line detail in the slot area.
    pub light: Option<usize>,
    /// `e`: the explain overlay.
    pub why: bool,
    /// `l`: the lifecycle log overlay.
    pub lifecycle: bool,
    /// `$`: rate-limit units instead of dollars on the context light.
    pub limit_units: bool,
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
                self.link_agent(&agent_id);
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
                let key = permission_key(
                    ev.tool_name.as_deref().unwrap_or("tool"),
                    p.get("command").and_then(|v| v.as_str()),
                );
                let suggested = p
                    .get("permission_suggestions")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter(|s| s.get("type").and_then(|t| t.as_str()) == Some("addRules"))
                    .filter_map(|s| s.get("rules").and_then(|r| r.as_array()))
                    .flatten()
                    .find_map(|r| {
                        let tool = r.get("toolName").and_then(|t| t.as_str())?;
                        let content = r.get("ruleContent").and_then(|c| c.as_str());
                        Some(match content {
                            Some(c) if !c.is_empty() => format!("{tool}({c})"),
                            _ => tool.to_string(),
                        })
                    });
                let ask = self.session.permission_asks.entry(key).or_default();
                ask.count += 1;
                if suggested.is_some() {
                    ask.rule = suggested;
                }
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
                if ev.event == "PreModelSwitch" {
                    self.model_switches.push(ModelSwitch {
                        at_ms: at,
                        from: from.clone(),
                        to: to.clone(),
                        source: pstr("source"),
                        cache_warm: warm,
                        context_tokens: p.get("context_tokens").and_then(|v| v.as_u64()),
                        estimated_cache_write_usd: usd,
                    });
                }
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
                    // A warm-cache switch re-writes the context: a priced moment.
                    kind: if warm == Some(true) {
                        Kind::Cost
                    } else {
                        Kind::Api
                    },
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
                exhaustion_in_active_hours: None,
            });
            self.limits_series_5h = series_5h.to_vec();
            let ex = crate::metrics::limits::exhaustion(
                &self.limits_series_5h,
                s.at_ms.max(self.now_ms),
            );
            // The projection extends the last half hour's slope; whether you
            // usually work at that hour says whether it will happen.
            let active = ex.and_then(|ex| {
                let hours = &self.baseline.as_ref()?.active_hours;
                let peak = *hours.iter().max()?;
                (peak > 0).then(|| hours[crate::ui::fmt::local_hour(ex) % 24] * 4 >= peak)
            });
            if let Some(l) = self.limits.as_mut() {
                l.exhaustion_ms = ex;
                l.exhaustion_in_active_hours = active;
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

    /// The rate-limit hit the transcript recorded, if the last API error was
    /// a 429: `(kind, resets_at epoch ms, low-priority retry seconds)`.
    pub fn rate_limit_hit(&self) -> Option<(String, Option<i64>, Option<u64>)> {
        let e = self.agg.api_errors.last()?;
        if e.error.as_deref() != Some("rate_limit") && e.status != Some(429) {
            return None;
        }
        // Only the newest error counts, and only while nothing succeeded since.
        let err_at =
            e.at.as_deref()
                .and_then(crate::metrics::cost::parse_ts_ms)?;
        if self.last_api_call_ms().is_some_and(|c| c > err_at) {
            return None;
        }
        Some((
            e.rate_limit_type
                .clone()
                .unwrap_or_else(|| "rate limit".into()),
            e.resets_at.map(|s| (s * 1000.0) as i64),
            e.low_priority_retry_after_s,
        ))
    }

    /// Take what `~/.claude.json` says about the account and this directory.
    /// Read (or re-read, when a file changed) Claude Code's `usage-data`:
    /// live sessions of this machine only, never a fixture.
    pub fn refresh_insights(&mut self, dir: &std::path::Path) {
        if self.session.pid.is_none() {
            return;
        }
        let newest = crate::insights::newest_mtime(dir);
        if newest.is_some() && newest == self.insights_mtime {
            return;
        }
        self.insights_mtime = newest;
        self.insights = crate::insights::load(dir);
    }

    /// The dim line for a session's first turn: the project's `/insights`
    /// medians and the previous session's cost here, `None` afterwards.
    pub fn start_line(&self) -> Option<String> {
        if self.agg.human_turns() > 1 {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(i) = &self.insights {
            if let Some(l) = i.start_line(&self.session.cwd, self.clock_ms()) {
                parts.push(l);
            }
        }
        if let Some(p) = &self.previous_session {
            let mut last: Vec<String> = Vec::new();
            if let Some(c) = p.cost_usd {
                last.push(format!("${c:.2}"));
            }
            if let Some(d) = p.duration_ms {
                last.push(crate::ui::fmt::duration_ms(d as i64));
            }
            if let (Some(a), Some(r)) = (p.lines_added, p.lines_removed) {
                last.push(format!("+{a}/−{r}"));
            }
            if !last.is_empty() {
                parts.push(format!("last session {}", last.join(" ")));
            }
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    pub fn apply_claude_home(&mut self, v: &serde_json::Value) {
        if let Some(t) = crate::claude_home::rate_limit_tier(v) {
            self.session.tier = Some(t);
        }
        self.previous_session = crate::claude_home::previous_session(v, &self.session.cwd);
        self.tips_recent = crate::claude_home::tips_recent(v, 10);
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
        if self.session.alive || self.clock_override {
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
        if let Line::Assistant(a) = line {
            for b in &a.message.content {
                if let crate::transcript::AssistantBlock::ToolUse { name, input, .. } = b {
                    if name == "Bash" {
                        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(form) = crate::phase::destructive_git(cmd) {
                            if let Some(at) = a
                                .timestamp
                                .as_deref()
                                .and_then(crate::metrics::cost::parse_ts_ms)
                            {
                                let dirty = self.session.git_dirty;
                                self.events.push(crate::events::Event {
                                    at,
                                    kind: crate::events::Kind::Note,
                                    text: format!(
                                        "destructive git: {form}{}",
                                        if dirty { " on a dirty tree" } else { "" }
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Line::Attachment(att) = line {
            if let crate::transcript::AttachmentKind::DeferredToolsDelta {
                needs_auth_mcp_servers,
                failed_mcp_servers,
                ..
            } = att.kind()
            {
                self.mcp_needs_auth = needs_auth_mcp_servers;
                self.mcp_failed = failed_mcp_servers;
            }
        }
        self.cost.push(line);
        let spawns_before = self.tools.agent_spawns.len();
        self.tools.push(line);
        if self.tools.agent_spawns.len() > spawns_before {
            let sync_chars = match line {
                Line::User(u) => match u.tool_use_detail() {
                    Some(crate::transcript::ToolUseDetail::Agent(ag)) if ag.usage.is_some() => {
                        Some(ag.result_chars)
                    }
                    _ => None,
                },
                _ => None,
            };
            // One spawn per `tool_result` block on the line.
            let new: Vec<tools::AgentSpawn> = self.tools.agent_spawns[spawns_before..].to_vec();
            for spawn in &new {
                self.note_agent_spawn(spawn, sync_chars);
            }
        }
        if let Some(n) = line.task_notification() {
            self.note_task_notification(n);
        }
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

    /// Take the watcher's agents, keeping what the hook spool attributed to
    /// agents whose transcript is not on disk (yet), and re-applying what
    /// the main transcript knows about each (`agent_links`).
    pub fn merge_agents(
        &mut self,
        fresh: &std::collections::BTreeMap<String, crate::agents::Agent>,
    ) {
        for (id, a) in fresh {
            let mut a = a.clone();
            if let Some(h) = self.agents.get(id) {
                a.hook_tool_calls = h.hook_tool_calls;
                a.hook_tool_ms = h.hook_tool_ms;
                a.hook_tool_errors = h.hook_tool_errors;
                a.hook_pre_calls = h.hook_pre_calls;
                a.carry_hook_state(h);
            }
            self.agents.insert(id.clone(), a);
        }
        let ids: Vec<String> = self.agent_links.keys().cloned().collect();
        for id in &ids {
            self.link_agent(id);
        }
    }

    /// An `Agent` result landed: remember the launch by agent id.
    fn note_agent_spawn(&mut self, spawn: &tools::AgentSpawn, sync_result_chars: Option<usize>) {
        let Some(id) = spawn.agent_id.clone() else {
            return;
        };
        // A teammate's `agent_id` is `<name>@<team>`; the team PRD reads it.
        if id.contains('@') {
            return;
        }
        let link = self.agent_links.entry(id.clone()).or_default();
        link.tool_use_id = Some(spawn.tool_use_id.clone());
        link.launched_turn = Some(spawn.turn);
        link.agent_type = spawn.agent_type.clone().or(link.agent_type.take());
        link.resolved_model = spawn.resolved_model.clone().or(link.resolved_model.take());
        link.spawned = true;
        if sync_result_chars.is_some() {
            link.sync_result_chars = sync_result_chars;
        }
        self.link_agent(&id);
    }

    /// A `<task-notification>` landed, by whichever of Claude Code's three
    /// deliveries: an agent's goes to its link (and its `Agent`), a
    /// workflow run's to `workflow_notifications`, a background shell
    /// command's is dropped — it never creates an agent.
    fn note_task_notification(&mut self, n: crate::transcript::TaskNotification) {
        if n.workflow.is_some() {
            let run = self
                .tools
                .workflow_launches
                .iter()
                .find(|(tu, task, _)| {
                    n.tool_use_id.as_deref() == Some(tu.as_str())
                        || task.as_deref() == Some(n.task_id.as_str())
                })
                .and_then(|(_, _, run)| run.clone())
                .unwrap_or_else(|| n.task_id.clone());
            self.workflow_notifications.insert(run, n);
            return;
        }
        let known =
            self.agent_links.contains_key(&n.task_id) || self.agents.contains_key(&n.task_id);
        let launcher = n
            .tool_use_id
            .as_deref()
            .and_then(|id| self.tools.get(id))
            .map(|c| c.name.clone());
        let is_agent =
            known || launcher.as_deref() == Some("Agent") || (launcher.is_none() && n.is_agent());
        if !is_agent {
            return;
        }
        let id = n.task_id.clone();
        let link = self.agent_links.entry(id.clone()).or_default();
        if link.tool_use_id.is_none() {
            link.tool_use_id = n.tool_use_id.clone();
        }
        if launcher.as_deref() == Some("Agent") {
            link.spawned = true;
        }
        link.notified = Some(n);
        self.link_agent(&id);
    }

    /// Apply one link to its agent, creating a placeholder for a launched
    /// agent whose transcript is not there (yet).
    fn link_agent(&mut self, id: &str) {
        let Some(link) = self.agent_links.get(id).cloned() else {
            return;
        };
        let agent = match self.agents.get_mut(id) {
            Some(a) => a,
            None if link.spawned => self.agents.entry(id.to_string()).or_insert_with(|| {
                crate::agents::Agent::new(
                    id,
                    crate::agents::Meta {
                        agent_type: link.agent_type.clone().unwrap_or_default(),
                        model: link.resolved_model.clone().unwrap_or_default(),
                        tool_use_id: link.tool_use_id.clone(),
                        ..Default::default()
                    },
                )
            }),
            None => return,
        };
        if agent.tool_use_id.is_none() {
            agent.tool_use_id = link.tool_use_id.clone();
        }
        agent.launched_turn = link.launched_turn.or(agent.launched_turn);
        if link.notified.is_some() {
            agent.notified = link.notified.clone();
        }
        if link.sync_result_chars.is_some() {
            agent.sync_result_chars = link.sync_result_chars;
        }
        if agent.agent_type.is_empty() {
            agent.agent_type = link.agent_type.clone().unwrap_or_default();
        }
        if agent.model.is_empty() {
            agent.model = link.resolved_model.clone().unwrap_or_default();
        }
    }

    /// The last commit Claude Code summarised: `(epoch ms, sha)`, and the
    /// edits since it.
    pub fn last_commit(&self) -> Option<(i64, String, usize)> {
        let (at, sha, _) = self.tools.commits.last()?;
        let edits = self
            .tools
            .calls
            .iter()
            .filter(|c| {
                c.class == crate::phase::ToolClass::Implement
                    && c.started_at.is_some_and(|s| s > *at)
            })
            .count();
        Some((*at, sha.clone(), edits))
    }

    /// Rewind points (file checkpoints) in the current turn, and the Bash
    /// writes of the turn that no checkpoint covers.
    pub fn rewind_points(&self) -> (usize, usize) {
        let t = self.agg.current_turn();
        let checkpoints = t.map(|t| t.checkpoints).unwrap_or(0);
        let since = t.map(|t| t.number).unwrap_or(0);
        let bash_writes = self
            .tools
            .calls
            .iter()
            .filter(|c| {
                c.turn >= since
                    && c.name == "Bash"
                    && c.bash_class == Some(crate::phase::BashClass::Implement)
            })
            .count();
        (checkpoints, bash_writes)
    }

    /// The re-read tax of a tool result: how many API calls have re-read it
    /// since it landed, and what that cost at the cache-read price.
    pub fn reread_tax(&self, call: &crate::tools::Call) -> Option<(usize, f64)> {
        let at = call.finished_at?;
        let rereads = self
            .agg
            .calls
            .iter()
            .filter(|c| c.at_ms.is_some_and(|t| t > at))
            .count();
        let price = self.cost.pricing().price(self.model()?)?.cache_read();
        Some((
            rereads,
            call.result_tokens_est as f64 * rereads as f64 * price / 1e6,
        ))
    }

    /// Deepest spawn depth among the agents (Claude Code caps it at 3).
    pub fn agent_depth(&self) -> u32 {
        self.agents
            .values()
            .map(|a| a.spawn_depth)
            .max()
            .unwrap_or(0)
    }

    /// The last confirmed test run: `(command summary, passed, epoch ms)`.
    pub fn last_check(&self) -> Option<(String, bool, i64)> {
        self.tools
            .calls
            .iter()
            .rev()
            .find(|c| c.is_confirmed_test() && c.finished_at.is_some())
            .map(|c| {
                (
                    crate::ui::fmt::clip(&c.input_summary, 24),
                    c.test_marker == crate::transcript::TestMarker::Passed,
                    c.finished_at.unwrap_or(0),
                )
            })
    }

    /// Source edits (Edit / Write / MultiEdit / NotebookEdit) in the current
    /// turn.
    pub fn edits_this_turn(&self) -> usize {
        self.files.files.values().map(|f| f.edits_this_turn).sum()
    }

    /// Edits since the last confirmed test run (or since the start).
    pub fn edits_since_check(&self) -> usize {
        let since = self.last_check().map(|(_, _, at)| at).unwrap_or(i64::MIN);
        self.tools
            .calls
            .iter()
            .filter(|c| {
                c.class == crate::phase::ToolClass::Implement
                    && c.started_at.is_some_and(|s| s > since)
            })
            .count()
    }

    /// What the model is waiting on, if anything.
    pub fn waiting(&self) -> Option<Waiting> {
        if self.session.permission_pending {
            return Some(Waiting {
                kind: WaitingKind::Permission,
                since_ms: self
                    .session
                    .permission_waiting_since_ms
                    .unwrap_or(self.clock_ms()),
            });
        }
        if let Some(c) = self.tools.running() {
            if matches!(c.name.as_str(), "AskUserQuestion" | "ExitPlanMode") {
                return Some(Waiting {
                    kind: WaitingKind::Question,
                    since_ms: c.started_at.unwrap_or(self.clock_ms()),
                });
            }
        }
        if let Some((kind, at)) = &self.session.notification {
            if matches!(kind.as_str(), "agent_needs_input" | "idle_prompt") {
                return Some(Waiting {
                    kind: WaitingKind::Notification,
                    since_ms: *at,
                });
            }
        }
        let t = self.agg.current_turn()?;
        if t.ended_with_question && t.duration_ms.is_some() {
            let at = t
                .last_at
                .as_deref()
                .and_then(crate::metrics::cost::parse_ts_ms)?;
            return Some(Waiting {
                kind: WaitingKind::Asked,
                since_ms: at,
            });
        }
        None
    }

    /// Interrupts this session and the output tokens the cut turns had
    /// produced.
    pub fn interrupts_summary(&self) -> (usize, u64) {
        let cut: u64 = self
            .agg
            .turns
            .iter()
            .filter(|t| t.interrupted_after_calls.is_some())
            .map(|t| t.usage.output)
            .sum();
        (self.agg.interrupts.len(), cut)
    }

    /// The cost gradient at the current context, priced cold when the cache
    /// is (or the shim says the next call re-writes).
    pub fn gradient(&self) -> Option<crate::metrics::cost::Gradient> {
        let cold = self.cache_warm().is_some_and(|(warm, _)| !warm);
        self.gradient_priced(cold)
    }

    /// The gradient at the warm (or the cold) price, whatever the cache
    /// state is now.
    pub fn gradient_priced(&self, cold: bool) -> Option<crate::metrics::cost::Gradient> {
        let model = self.model()?;
        let ttl = match self.cache_ttl_ms() {
            3_600_000 => crate::transcript::CacheTtl::OneHour,
            _ => crate::transcript::CacheTtl::FiveMinutes,
        };
        crate::metrics::cost::gradient(
            self.cost.pricing(),
            model,
            self.context().size,
            crate::metrics::cost::median_output(&self.agg),
            crate::metrics::cost::p50_calls_per_turn(&self.agg),
            cold,
            ttl,
        )
    }

    /// Where the tokens went: the top `n` owners (skills, plugins, agents,
    /// MCP servers, idle turns) by share of all input tokens, including the
    /// subagents' own usage under `agents`.
    pub fn attribution_top(&self, n: usize) -> Vec<(String, f64)> {
        let agents = self.agents_usage().total_input();
        let main = self.agg.total.total_input();
        let total = (main + agents) as f64;
        if total == 0.0 {
            return Vec::new();
        }
        let mut v: Vec<(String, f64)> = self
            .agg
            .attribution
            .iter()
            .map(|(k, u)| (k.clone(), u.total_input() as f64 / total))
            .collect();
        if agents > 0 {
            v.push(("agents".to_string(), agents as f64 / total));
        }
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    /// Subagent spend: `(usd, share of the session)` when priceable — the
    /// whole session's agent calls priced, over the combined figure (the
    /// ledger holds the agents' earlier calls, so the share is not derived
    /// by adding them to it again).
    pub fn agents_cost(&self) -> Option<(f64, f64)> {
        let combined = self.cost.combined(self.agents.values());
        self.agents_share_of(combined)
    }

    /// `(agents' priced spend, share of `combined`)`; `None` when no agent
    /// is priceable.
    fn agents_share_of(&self, combined: Option<Cost>) -> Option<(f64, f64)> {
        let pricing = self.cost.pricing();
        let mut usd = 0.0;
        let mut any = false;
        for a in self.agents.values() {
            if let Some(c) = pricing.estimate(&a.usage, &a.model) {
                usd += c;
                any = true;
            }
        }
        if !any {
            return None;
        }
        let total = combined.map(|c| c.usd).unwrap_or(usd);
        Some((usd, if total > 0.0 { usd / total } else { 0.0 }))
    }

    /// The session's money in the parts the surfaces print (agent PRD
    /// §4.2): the headline (combined, or main-only with `a` off), the
    /// ledger, what was priced after it, and the agents' whole spend.
    pub fn cost_breakdown(&self) -> Option<CostBreakdown> {
        let include = self.tokens_include_agents;
        // The agents' calls are walked once for the part after the ledger
        // and once for the whole; `combined` is derived, not recomputed.
        let agents_after = self.cost.agents_after(self.agents.values());
        let current = self.cost.current();
        let combined = match (current, agents_after) {
            (Some(c), Some(a)) => Some(c.plus(a)),
            (Some(c), None) => Some(c),
            (None, a) => a,
        };
        let headline = if include { combined } else { current }?;
        let since = match (self.cost.since(), agents_after.filter(|_| include)) {
            (Some(m), Some(a)) => Some(m.plus(a)),
            (Some(m), None) => Some(m),
            (None, a) => a,
        };
        let agents = self.agents_share_of(combined);
        let pricing = self.cost.pricing();
        let unpriced_agent_tokens = self
            .agents
            .values()
            .filter(|a| pricing.price(&a.model).is_none())
            .map(|a| a.usage.total())
            .sum();
        Some(CostBreakdown {
            headline,
            combined: combined.unwrap_or(headline),
            ledger: self.cost.authoritative.as_ref().map(|c| c.total_cost_usd),
            since,
            agents: agents.map(|(usd, _)| Cost::priced(usd)),
            agents_share: agents.map(|(_, s)| s).unwrap_or(0.0),
            any_agents: self.agents_usage().total() > 0,
            unpriced_agent_tokens,
        })
    }

    /// `/usage`'s behaviour flags for this session.
    pub fn behaviour_flags(&self) -> crate::metrics::cost::BehaviourFlags {
        let agent_weight: f64 = self
            .agents
            .values()
            .map(|a| crate::metrics::cost::limit_weight(&a.usage, &a.model))
            .sum();
        let active_ms = self
            .session
            .started_at_ms
            .map(|s| self.clock_ms() - s)
            .unwrap_or(0);
        crate::metrics::cost::BehaviourFlags::compute(
            &self.agg,
            agent_weight,
            self.other_live_sessions + 1,
            active_ms,
        )
    }

    /// Read the autocompact overrides for a live session: settings.json and
    /// the claude process environment; price the enabled plugins with the
    /// catalog cache.
    pub fn load_autocompact(&mut self) {
        let settings = crate::install::read_settings(&crate::install::settings_path());
        let catalog = std::env::var_os("HOME").and_then(|h| {
            crate::claude_home::read_at(
                &std::path::PathBuf::from(h).join(".claude/plugins/plugin-catalog-cache.json"),
            )
        });
        let claude_json = crate::claude_home::read();
        let model = self.model().unwrap_or("claude-opus-5").to_string();
        self.prefix
            .scan_plugins(&settings, catalog.as_ref(), claude_json.as_ref(), &model);
        if let Some(pid) = self.session.pid {
            self.claude_env = crate::procenv::env_of(pid);
        }
        self.autocompact = crate::metrics::context::AutocompactConfig::from_sources(
            Some(&settings),
            &self.claude_env,
        );
        self.allow_rules = settings
            .get("permissions")
            .and_then(|p| p.get("allow"))
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
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

    /// The main-transcript side of an agent: its `Agent` launch (the call
    /// and the `async_launched` result) and the notification that ends it.
    fn launch(agent_id: &str, tool_use_id: &str, at: &str) -> [Line; 2] {
        [
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"{at}","message":{{"id":"m-{tool_use_id}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"{tool_use_id}","name":"Agent","input":{{"subagent_type":"Explore","description":"d","prompt":"p"}}}}],"usage":{{"output_tokens":10}}}}}}"#
            ))
            .unwrap(),
            Line::parse(&format!(
                r#"{{"type":"user","timestamp":"{at}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"launched"}}]}},"toolUseResult":{{"status":"async_launched","isAsync":true,"agentId":"{agent_id}","resolvedModel":"claude-haiku-4-5-20251001","outputFile":"/tmp/o","description":"d","prompt":"p"}}}}"#
            ))
            .unwrap(),
        ]
    }

    fn notification_text(task_id: &str, tool_use_id: Option<&str>, status: &str) -> String {
        let tu = tool_use_id
            .map(|t| format!("<tool-use-id>{t}</tool-use-id>"))
            .unwrap_or_default();
        format!(
            "<task-notification><task-id>{task_id}</task-id>{tu}<output-file>/tmp/o</output-file><status>{status}</status><summary>s</summary><result>four</result></task-notification>"
        )
    }

    #[test]
    fn task_notifications_reach_agents_by_every_delivery_and_shell_tasks_never_do() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","promptSource":"typed","origin":{"kind":"human"},"message":{"role":"user","content":"go"}}"#).unwrap());
        for l in launch("0000000000000000a", "toolu_a", "2026-01-01T00:00:01Z") {
            s.apply(&l);
        }
        // The launch alone makes a placeholder with the call's type, the
        // resolved model, the tool_use id and the launching turn.
        let a = &s.agents["0000000000000000a"];
        assert_eq!(a.agent_type, "Explore");
        assert_eq!(a.model, "claude-haiku-4-5-20251001");
        assert_eq!(a.tool_use_id.as_deref(), Some("toolu_a"));
        assert_eq!(a.launched_turn, Some(1));
        assert_eq!(a.notified, None);
        assert!(s.agent_links["0000000000000000a"].spawned);

        // 1. As a user line.
        let text = notification_text("0000000000000000a", Some("toolu_a"), "completed");
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:01:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        let n = s.agents["0000000000000000a"].notified.clone().unwrap();
        assert_eq!(n.status, crate::transcript::TaskStatus::Completed);
        assert_eq!(n.result_chars, Some(4));

        // 2. As a queue operation (the model was busy) and 3. the queued
        // command it becomes — the same notification, keyed by task id.
        for l in launch("0000000000000000b", "toolu_b", "2026-01-01T00:02:00Z") {
            s.apply(&l);
        }
        let text = notification_text("0000000000000000b", Some("toolu_b"), "killed");
        s.apply(&Line::parse(&format!(
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-01-01T00:03:00Z","content":{}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        assert_eq!(
            s.agents["0000000000000000b"]
                .notified
                .as_ref()
                .unwrap()
                .status,
            crate::transcript::TaskStatus::Killed
        );
        s.apply(&Line::parse(&format!(
            r#"{{"type":"attachment","timestamp":"2026-01-01T00:03:01Z","attachment":{{"type":"queued_command","prompt":{},"commandMode":"prompt","origin":{{"kind":"task-notification"}}}}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        assert_eq!(
            s.agent_links.len(),
            2,
            "a repeat overwrites, never duplicates"
        );

        // A background shell command's notification: the launching call was
        // a Bash, so no agent, no link.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:04:00Z","message":{"id":"m-sh","model":"claude-opus-5","content":[{"type":"tool_use","id":"toolu_sh","name":"Bash","input":{"command":"make check","run_in_background":true}}],"usage":{"output_tokens":10}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:04:01Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_sh","content":"bg"}]}}"#).unwrap());
        let text = notification_text("baqaldmpb", Some("toolu_sh"), "failed");
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:05:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        assert_eq!(s.agents.len(), 2);
        assert!(!s.agent_links.contains_key("baqaldmpb"));
        // One with no tool_use id and no launch in the transcript: not an
        // agent by its id shape either.
        let text = notification_text("xyz123abc", None, "completed");
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:06:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        assert_eq!(s.agent_links.len(), 2);
        // An agent-shaped id with no launch seen (a cut transcript): the
        // link waits, no placeholder.
        let text = notification_text("0000000000000000c", None, "completed");
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:07:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(&text).unwrap()
        )).unwrap());
        assert!(s.agent_links.contains_key("0000000000000000c"));
        assert!(!s.agents.contains_key("0000000000000000c"));

        // The watcher's agents replace the placeholders; the links follow.
        let mut fresh = std::collections::BTreeMap::new();
        for id in [
            "0000000000000000a",
            "0000000000000000b",
            "0000000000000000c",
        ] {
            fresh.insert(
                id.to_string(),
                crate::agents::Agent::new(
                    id,
                    crate::agents::Meta {
                        agent_type: "Explore".into(),
                        ..Default::default()
                    },
                ),
            );
        }
        s.merge_agents(&fresh);
        assert_eq!(
            s.agents["0000000000000000a"]
                .notified
                .as_ref()
                .unwrap()
                .status,
            crate::transcript::TaskStatus::Completed
        );
        assert_eq!(
            s.agents["0000000000000000b"]
                .notified
                .as_ref()
                .unwrap()
                .status,
            crate::transcript::TaskStatus::Killed
        );
        assert_eq!(
            s.agents["0000000000000000b"].launched_turn,
            Some(2),
            "launched after the first notification's machine turn"
        );
        assert_eq!(
            s.agents["0000000000000000c"]
                .notified
                .as_ref()
                .unwrap()
                .status,
            crate::transcript::TaskStatus::Completed,
            "the waiting link attaches when the agent appears"
        );
    }

    #[test]
    fn workflow_notifications_are_keyed_by_run() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"m-wf","model":"claude-opus-5","content":[{"type":"tool_use","id":"toolu_wf","name":"Workflow","input":{"script":"x"}}],"usage":{"output_tokens":10}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:01Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_wf","content":"launched"}]},"toolUseResult":{"status":"async_launched","taskId":"wquxwsh3","taskType":"workflow","workflowName":"review","runId":"wf_89cf8717-8a9","summary":"s","transcriptDir":"/d","scriptPath":"/s"}}"#).unwrap());
        assert_eq!(
            s.tools.workflow_launches,
            vec![(
                "toolu_wf".to_string(),
                Some("wquxwsh3".to_string()),
                Some("wf_89cf8717-8a9".to_string())
            )]
        );
        let text = "<task-notification><task-id>wquxwsh3</task-id><tool-use-id>toolu_wf</tool-use-id><status>completed</status><summary>s</summary><result>r</result><usage><agent_count>9</agent_count><agents_done>7</agents_done><agents_error>2</agents_error><agents_skipped>0</agents_skipped><agents_empty_result>1</agents_empty_result><subagent_tokens>5</subagent_tokens><tool_uses>3</tool_uses><duration_ms>9</duration_ms></usage></task-notification>";
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:09:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(text).unwrap()
        )).unwrap());
        let n = &s.workflow_notifications["wf_89cf8717-8a9"];
        assert_eq!(n.workflow.unwrap().empty_result, 1);
        assert!(s.agents.is_empty(), "a workflow run is not an agent");
    }
}
