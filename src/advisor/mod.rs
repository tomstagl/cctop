//! The Advisor: deterministic rules that turn observed patterns into one
//! concrete change to how the user works. No model call, no network.
//!
//! Engine v2 (coach PRD §6.4). Every rule has an urgency class — NOW (act
//! within this turn) › NEXT (act at the next boundary) › LATER (a turn-end
//! retrospective) — an `acted` predicate naming the observable completion,
//! a hard TTL and a cooldown. One nudge occupies the *slot*; a same-class
//! newcomer waits in the queue and only a higher class pre-empts. At most
//! one newly promoted NEXT/LATER nudge per human turn, three in any ten,
//! one LATER per two; a nudge that held the slot for three human turns
//! unacted snoozes itself for the session. Nothing nudges on a machine
//! turn, in a bot loop or in a `-p` run. Snoozes and the fire records
//! persist in `~/.cctop/<session>.advisor.json` so `query`, the MCP tool
//! and the pane agree with the TUI.

pub mod rules;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::State;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Saving {
    /// Tokens saved per remaining turn.
    Tokens(u64),
    /// Tokens saved once (a single re-write, one cold call).
    OneOff(u64),
    /// Wall-clock seconds saved per remaining turn.
    Seconds(u64),
    /// Avoids a hard stop (rate limit, lost context); ranked above tokens.
    Avoids,
}

impl Saving {
    /// Ranking key: tokens × the API calls still expected in the session
    /// (`remaining_calls`, floor 10); bigger is more urgent. A one-off
    /// saving is not multiplied.
    pub fn rank(self, remaining_calls: u64) -> u64 {
        let n = remaining_calls.max(10);
        match self {
            Saving::Avoids => u64::MAX,
            // A second of waiting ≈ 2k tokens of value, so the two compare.
            Saving::Seconds(s) => s.saturating_mul(2_000).saturating_mul(n),
            Saving::Tokens(t) => t.saturating_mul(n),
            Saving::OneOff(t) => t,
        }
    }

    pub fn label(self) -> String {
        match self {
            Saving::Tokens(t) => format!("~{}/turn", crate::ui::fmt::tokens(t)),
            Saving::OneOff(t) => format!("~{} once", crate::ui::fmt::tokens(t)),
            Saving::Seconds(s) => {
                format!("~{}/turn", crate::ui::fmt::duration_ms(s as i64 * 1000))
            }
            Saving::Avoids => "avoids a hard stop".into(),
        }
    }
}

/// How soon the person should act. `Ord`: `Now < Next < Later`, so a
/// smaller class is the higher one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Urgency {
    /// Act within this turn (pre-empts the slot).
    Now,
    /// Act at the next boundary (turn end, next prompt).
    Next,
    /// A turn-end retrospective.
    Later,
}

impl Urgency {
    pub fn label(self) -> &'static str {
        match self {
            Urgency::Now => "NOW",
            Urgency::Next => "NEXT",
            Urgency::Later => "LATER",
        }
    }
}

/// What the action text is, so a surface knows how to offer it: fill the
/// prompt for `Prompt` and `Slash`, show a settings snippet for
/// `AllowRule` and `Setting`, name a key for `Key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Prompt,
    Slash,
    AllowRule,
    Setting,
    Key,
    /// Nothing to type: read and decide.
    Advice,
}

impl ActionKind {
    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Prompt => "prompt",
            ActionKind::Slash => "slash",
            ActionKind::AllowRule => "allow-rule",
            ActionKind::Setting => "setting",
            ActionKind::Key => "key",
            ActionKind::Advice => "advice",
        }
    }
}

/// When a fired nudge expires on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ttl {
    /// When the turn it fired in ends (NOW default).
    TurnEnd,
    /// When the next human prompt arrives (NEXT default; NOW rules that
    /// fire at turn end).
    NextPrompt,
    /// When the turn after the next prompt ends: for a turn-end nudge whose
    /// act is the next turn's first calls (verify-gap).
    NextTurnEnd,
    /// Three human turns later (LATER default).
    ThreeTurns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Advice {
    pub rule: &'static str,
    /// The family (`cache-miss`, `verify-gap`…): the slot holds one per
    /// family, and the published order (§6.4) is by family.
    pub family: &'static str,
    pub urgency: Urgency,
    /// Line 1 of the nudge (the coach cuts it to 52 cells).
    pub headline: String,
    pub evidence: String,
    /// Line 2: what to do.
    pub action: String,
    /// The exact text to fill or copy (a prompt, a slash command, a rule);
    /// empty when there is nothing to type.
    pub action_text: String,
    pub action_kind: ActionKind,
    pub saving: Saving,
    /// Key into the explanation text ([`rules::explain`]).
    pub doc_key: &'static str,
    /// Human turn the rule first fired in (filled by the engine).
    pub since_turn: usize,
    /// How many human turns the trigger looks back over.
    pub window_turns: usize,
    /// What retires it, in words (`turn end`, `next prompt`, `an Agent
    /// spawn`).
    pub retires_on: &'static str,
    /// Rule-private snapshot taken when it fired (a count, a tier), read
    /// back by the rule's `acted` predicate.
    pub mark: u64,
    /// Never takes the slot: it waits in the `next` row (plan-first, the
    /// context cost below 200 k).
    pub next_row_only: bool,
}

impl Advice {
    /// A nudge with the identity set and everything else empty; rules fill
    /// the text, the saving and the retirement words.
    pub fn new(rule: &'static str, family: &'static str, urgency: Urgency) -> Advice {
        Advice {
            rule,
            family,
            urgency,
            headline: String::new(),
            evidence: String::new(),
            action: String::new(),
            action_text: String::new(),
            action_kind: ActionKind::Advice,
            saving: Saving::Tokens(0),
            doc_key: rule,
            since_turn: 0,
            window_turns: 1,
            retires_on: match urgency {
                Urgency::Now => "turn end",
                Urgency::Next => "next prompt",
                Urgency::Later => "3 turns",
            },
            mark: 0,
            next_row_only: false,
        }
    }
}

pub trait Rule: Send + Sync {
    /// `A01` …
    fn id(&self) -> &'static str;
    fn family(&self) -> &'static str;
    fn urgency(&self) -> Urgency;
    fn evaluate(&self, state: &State) -> Option<Advice>;
    /// The observable completion: true once the person did the thing.
    fn acted(&self, _state: &State, _fired: &Advice) -> bool {
        false
    }
    fn ttl(&self) -> Ttl {
        match self.urgency() {
            Urgency::Now => Ttl::TurnEnd,
            Urgency::Next => Ttl::NextPrompt,
            Urgency::Later => Ttl::ThreeTurns,
        }
    }
    /// Human turns of silence after the nudge retires.
    fn cooldown_turns(&self) -> usize {
        5
    }
}

/// The published intra-class order (§6.4): earlier wins.
pub const ORDER_NOW: &[&str] = &[
    "turn-died",
    "waiting",
    "cache-countdown",
    "failure-cascade",
    "denial-streak",
    "correction-streak",
    "commit-unchecked",
    "destructive-git",
    "warm-switch",
    "double-steer",
];
pub const ORDER_NEXT: &[&str] = &[
    "cold-resume",
    "verify-gap",
    "context-reset",
    "post-compaction",
    "loop-armed",
    "review-before-merge",
    "commit-tail",
    "explore-delegate",
    "cold-switch",
    "permission-wait",
    "rate-limit",
];
pub const ORDER_LATER: &[&str] = &[
    "cache-miss",
    "agent-context",
    "screenshot-heavy",
    "prefix-tip",
    "cache-expiry",
    "runaway-result",
    "reread",
    "idle-mcp",
    "thinking",
    "long-foreground",
    "fresh-input",
    "chatty",
    "subagent-model",
    "agents-waste",
    "hook-overhead",
];

fn order_index(urgency: Urgency, family: &str) -> usize {
    let table = match urgency {
        Urgency::Now => ORDER_NOW,
        Urgency::Next => ORDER_NEXT,
        Urgency::Later => ORDER_LATER,
    };
    table
        .iter()
        .position(|f| *f == family)
        .unwrap_or(table.len())
}

/// Claude Code's own tips (`tipsHistory` ids) that say what a family
/// would say: a tip shown in the last ten startups mutes the family.
pub const TIP_FOR_FAMILY: &[(&str, &[&str])] = &[
    ("permission-wait", &["permissions"]),
    ("denial-streak", &["permissions"]),
    (
        "correction-streak",
        &["double-esc", "double-esc-code-restore"],
    ),
    ("plan-first", &["plan-mode-for-complex-tasks"]),
    ("loop-armed", &["loop-command-nudge"]),
    (
        "explore-delegate",
        &["subagent-fanout-nudge", "ctx:too-many-subagents"],
    ),
    (
        "review-before-merge",
        &["ultrareview-post-commit", "ultrareview-awareness"],
    ),
    ("thinking", &["config-thinking-mode", "tab-toggle-thinking"]),
    ("idle-mcp", &["plugin-disuse-review"]),
    ("waiting", &["btw-side-question"]),
];

/// Who is at the keyboard, as far as the transcript and the spool tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    Interactive,
    /// A Stop hook keeps the turn going (Ralph, `/loop`): loop-health only.
    Loop,
    /// The last prompt was machine-originated (`system` / `sdk`, a task
    /// notification): no nudges.
    Machine,
    /// A named agent persona or a team: no `/clear` or reply advice.
    Team,
    /// `-p` / SDK entrypoint: no nudges.
    Workflow,
    /// The last prompt came through the Remote Control bridge: replies may
    /// come from the other device, so no reply advice. Not derived yet — a
    /// bridge prompt has no transcript marker on 2.1.270.
    Remote,
}

impl SessionMode {
    pub fn label(self) -> &'static str {
        match self {
            SessionMode::Interactive => "interactive",
            SessionMode::Loop => "loop",
            SessionMode::Machine => "machine",
            SessionMode::Team => "team",
            SessionMode::Workflow => "workflow",
            SessionMode::Remote => "remote",
        }
    }

    /// Derive from what the transcript and the spool say.
    pub fn derive(state: &State) -> SessionMode {
        if state.agg.turns.last().is_some_and(|t| !t.human) {
            return SessionMode::Machine;
        }
        // A Stop hook blocked the last turns from ending: a bot loop.
        let blocked = state
            .agg
            .turns
            .iter()
            .rev()
            .take(3)
            .filter(|t| t.hook_blocked)
            .count();
        if blocked >= 2 {
            return SessionMode::Loop;
        }
        if state
            .claude_env
            .get("CLAUDE_CODE_ENTRYPOINT")
            .is_some_and(|e| e.starts_with("sdk"))
        {
            return SessionMode::Workflow;
        }
        if state.agg.agent_setting.is_some() || !state.teammates.is_empty() {
            return SessionMode::Team;
        }
        // `bridge-session` only says Remote Control is registered, and a
        // prompt from the phone carries no marker of its own (2.1.270 writes
        // `promptSource` ∈ typed / queued / suggestion_accepted / system /
        // sdk), so `Remote` stays dormant until one exists.
        SessionMode::Interactive
    }
}

/// One fire of one rule, kept for the measurement of the coach (US-012).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireRecord {
    pub rule: String,
    pub family: String,
    pub class: Urgency,
    /// Human turn it was promoted in.
    pub turn: usize,
    pub shown_at_ms: i64,
    /// How it retired (`acted`, `expired`, `snoozed`, `retired`,
    /// `pre-empted`) and when.
    pub retired: Option<(String, i64)>,
    pub acted_delay_ms: Option<i64>,
    pub snoozed: bool,
    pub session_mode: SessionMode,
    /// The surface that showed it: `tui-coach`, `tui-dashboard`, `pane`,
    /// `query`, or `none` (the coach off, or a headless reader). An
    /// "exposed fire" — the denominator of every rate — is any but `none`.
    #[serde(default = "surface_none")]
    pub surface: String,
    /// Milliseconds since the person's last input when it fired.
    #[serde(default)]
    pub human_idle_ms: Option<i64>,
    /// `x` / `X` delay after the showing; under 2 s is a reflex dismissal.
    #[serde(default)]
    pub time_to_x_ms: Option<i64>,
    /// The snooze was for the session (`X`, or the third `x`).
    #[serde(default)]
    pub snoozed_session: bool,
    /// The coach view was left within 10 s of the promotion.
    #[serde(default)]
    pub toggled_away: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub project: String,
}

fn surface_none() -> String {
    "none".into()
}

/// A retired nudge, for the coach's lifecycle rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub rule: &'static str,
    pub family: &'static str,
    pub at_ms: i64,
    /// `acted`, `expired`, `snoozed`, `retired`, `pre-empted`.
    pub what: &'static str,
    pub detail: String,
}

/// The slot occupant.
#[derive(Debug, Clone, PartialEq)]
pub struct Occupant {
    pub advice: Advice,
    pub fired_at_ms: i64,
    /// Human turn it was promoted in.
    pub promoted_turn: usize,
    /// `agg.turns.len()` at promotion, for the turn-end TTL.
    turn_index: usize,
    /// The person pressed Enter: waiting for the acted predicate.
    pub acting: bool,
    record: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct Snooze {
    /// Human turn until which the rule is quiet; `None` = the session.
    until_turn: Option<usize>,
    count: u8,
}

/// The slot as the writer left it, so a reader (`query`, the pane) shows
/// the same nudge instead of ranking afresh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedOccupant {
    pub rule: String,
    pub fired_at_ms: i64,
    pub promoted_turn: usize,
    pub turn_index: usize,
    pub acting: bool,
    pub record: usize,
}

/// What `~/.cctop/<session>.advisor.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Persisted {
    pub version: u32,
    #[serde(default)]
    snoozed: HashMap<String, Snooze>,
    #[serde(default)]
    first_fired: HashMap<String, usize>,
    #[serde(default)]
    pub records: Vec<FireRecord>,
    #[serde(default)]
    pub occupant: Option<PersistedOccupant>,
    /// `(human turn, class)` of the last promotions, for the budget.
    #[serde(default)]
    promotions: Vec<(usize, Urgency)>,
    #[serde(default)]
    cooldown_until: HashMap<String, usize>,
    /// `on` / `off`: the exposure arm this session was assigned to.
    #[serde(default = "exposure_on")]
    pub exposure: String,
    /// What the coach cost this session (the TUI writes it).
    #[serde(default)]
    pub cost: crate::coach_stats::Cost,
}

fn exposure_on() -> String {
    "on".into()
}

/// A rule held back by its own record (US-012's demotion rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Demotion {
    /// False positives past the bar: the next row only.
    NextRow,
    /// Precision collapsed on this Claude Code version: LATER.
    Later,
}

/// One Events row the engine wants written (`kind=coach`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoachEvent {
    pub at_ms: i64,
    pub text: String,
}

/// Evaluates every rule and keeps the slot, the queue and the records.
pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
    /// The slot occupant first, then the queue, ranked.
    pub current: Vec<Advice>,
    pub occupant: Option<Occupant>,
    snoozed: HashMap<&'static str, Snooze>,
    /// Human turn until which a retired rule stays quiet.
    cooldown_until: HashMap<&'static str, usize>,
    /// Rule id → human turn it first fired (continuously).
    first_fired: HashMap<&'static str, usize>,
    /// `(human turn, class)` of every promotion, for the budget.
    promotions: VecDeque<(usize, Urgency)>,
    pub records: Vec<FireRecord>,
    /// The last retired nudges, newest first (at most eight).
    pub recent: Vec<Lifecycle>,
    /// Rules held back this tick and why.
    pub suppressed: Vec<(&'static str, String)>,
    pub session_mode: SessionMode,
    /// Events rows produced since the last `drain_events`.
    pending_events: Vec<CoachEvent>,
    /// The writer's occupant, adopted on the next evaluation while its rule
    /// still fires.
    adopt: Option<PersistedOccupant>,
    /// The surface this engine draws for (`tui-coach`, `tui-dashboard`,
    /// `pane`, `query`, `none`), stamped on every fire record.
    pub surface: String,
    /// Whether nudges are shown at all this session (`--coach off` records
    /// the fires and shows nothing: the control arm of the measurement).
    pub exposed: bool,
    /// Rules demoted by their own record (US-012): to the next row, or to
    /// LATER after a precision collapse on this Claude Code version.
    pub demoted: HashMap<&'static str, Demotion>,
    /// The coach's own cost this session, persisted beside the records.
    pub cost: crate::coach_stats::Cost,
    /// Where the state persists, when it does.
    pub path: Option<PathBuf>,
    /// This process holds the single-writer lock (the TUI); others read,
    /// and write only while no live lock exists.
    pub writer: bool,
    dirty: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new(rules::all())
    }
}

/// `~/.cctop/<session>.advisor.json`.
pub fn persist_path(home: &Path, session_id: &str) -> PathBuf {
    home.join(format!("{session_id}.advisor.json"))
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("lock")
}

/// Snoozes a reader could not apply itself, queued for the writer.
fn requests_path(path: &Path) -> PathBuf {
    path.with_extension("requests")
}

static READER_SURFACE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Name the surface a one-shot reader draws for (`cctop query coach
/// --surface pane`); `query` when unnamed. Set once per process.
pub fn set_reader_surface(surface: &str) {
    let _ = READER_SURFACE.set(surface.to_string());
}

pub fn reader_surface() -> String {
    READER_SURFACE
        .get()
        .cloned()
        .unwrap_or_else(|| "query".into())
}

/// `$HOME/.cctop`, where the advisor state lives.
pub fn default_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".cctop"))
}

impl Engine {
    pub fn new(rules: Vec<Box<dyn Rule>>) -> Engine {
        Engine {
            rules,
            current: Vec::new(),
            occupant: None,
            snoozed: HashMap::new(),
            cooldown_until: HashMap::new(),
            first_fired: HashMap::new(),
            promotions: VecDeque::new(),
            records: Vec::new(),
            recent: Vec::new(),
            suppressed: Vec::new(),
            session_mode: SessionMode::Interactive,
            pending_events: Vec::new(),
            adopt: None,
            surface: "none".into(),
            exposed: true,
            demoted: HashMap::new(),
            cost: crate::coach_stats::Cost::default(),
            path: None,
            writer: false,
            dirty: false,
        }
    }

    /// The engine every one-shot consumer uses (`query`, MCP, the report,
    /// `advise`): attached to the live session's persisted state as a
    /// reader, evaluated once. Fixture files get no persistence. The
    /// surface stamped on a fire it promotes itself (no dashboard running)
    /// is [`reader_surface`]'s: `query`, or `pane` when the pane asked.
    pub fn for_state(state: &State) -> Engine {
        let mut e = Engine {
            surface: reader_surface(),
            ..Default::default()
        };
        if state.session.pid.is_some() && !state.session.session_id.is_empty() {
            if let Some(home) = default_home() {
                e.attach(&home, &state.session.session_id, false);
            }
        }
        e.evaluate(state);
        e
    }

    /// Attach the persisted state of a session. `writer` = this process is
    /// the TUI and takes the single-writer lock; anyone else reads, and
    /// writes only while no live lock exists.
    pub fn attach(&mut self, home: &Path, session_id: &str, writer: bool) {
        let path = persist_path(home, session_id);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(p) = serde_json::from_str::<Persisted>(&text) {
                self.load(p);
            }
        }
        self.writer = writer && take_lock(&lock_path(&path));
        self.path = Some(path);
    }

    fn load(&mut self, p: Persisted) {
        let ids: Vec<&'static str> = self.rules.iter().map(|r| r.id()).collect();
        for (rule, s) in p.snoozed {
            if let Some(id) = ids.iter().find(|id| **id == rule) {
                self.snoozed.insert(id, s);
            }
        }
        for (rule, t) in p.first_fired {
            if let Some(id) = ids.iter().find(|id| **id == rule) {
                self.first_fired.insert(id, t);
            }
        }
        for (rule, t) in p.cooldown_until {
            if let Some(id) = ids.iter().find(|id| **id == rule) {
                self.cooldown_until.insert(id, t);
            }
        }
        self.promotions = p.promotions.into_iter().collect();
        self.records = p.records;
        self.adopt = p.occupant.filter(|o| o.record < self.records.len());
        // A reader follows the writer's arm; the writer sets its own.
        if !self.writer {
            self.exposed = p.exposure != "off";
        }
        self.cost = p.cost;
    }

    /// Apply the demotions the record earned (`coach_stats::demotions`).
    pub fn demote(&mut self, demotions: &std::collections::BTreeMap<String, Demotion>) {
        let ids: Vec<&'static str> = self.rules.iter().map(|r| r.id()).collect();
        for (rule, d) in demotions {
            if let Some(id) = ids.iter().find(|id| **id == rule) {
                self.demoted.insert(id, *d);
            }
        }
    }

    /// Something worth saving changed outside the engine (the cost).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// A prompt went to the session through the socket: the coach's cost.
    pub fn note_send(&mut self, chars: usize) {
        self.cost.socket_sends += 1;
        self.cost.socket_chars += chars;
        self.dirty = true;
    }

    fn persisted(&self) -> Persisted {
        Persisted {
            version: 2,
            snoozed: self
                .snoozed
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            first_fired: self
                .first_fired
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            records: self.records.clone(),
            occupant: self.occupant.as_ref().map(|o| PersistedOccupant {
                rule: o.advice.rule.to_string(),
                fired_at_ms: o.fired_at_ms,
                promoted_turn: o.promoted_turn,
                turn_index: o.turn_index,
                acting: o.acting,
                record: o.record,
            }),
            promotions: self.promotions.iter().copied().collect(),
            cooldown_until: self
                .cooldown_until
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            exposure: if self.exposed { "on" } else { "off" }.into(),
            cost: self.cost.clone(),
        }
    }

    /// Write the state if it changed and this process may write.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        if !self.writer && lock_alive(&lock_path(&path)) {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.persisted()) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        self.dirty = false;
    }

    /// Release the writer lock (the TUI on exit).
    pub fn release(&mut self) {
        if let (true, Some(path)) = (self.writer, &self.path) {
            let lock = lock_path(path);
            if std::fs::read_to_string(&lock)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                == Some(std::process::id())
            {
                let _ = std::fs::remove_file(lock);
            }
            self.writer = false;
        }
    }

    /// Re-run every rule against `state`.
    pub fn evaluate(&mut self, state: &State) {
        let turn = state.agg.human_turns();
        let now = state.clock_ms();
        self.session_mode = SessionMode::derive(state);
        self.suppressed.clear();

        // Snoozes and cooldowns that ran out.
        self.snoozed
            .retain(|_, s| s.until_turn.is_none_or(|u| u > turn));
        self.cooldown_until.retain(|_, u| *u > turn);

        // A reader takes over the writer's slot while its rule still fires.
        if let (None, Some(p)) = (&self.occupant, self.adopt.take()) {
            if let Some(a) = self
                .rules
                .iter()
                .find(|r| r.id() == p.rule)
                .and_then(|r| r.evaluate(state))
            {
                self.occupant = Some(Occupant {
                    advice: a,
                    fired_at_ms: p.fired_at_ms,
                    promoted_turn: p.promoted_turn,
                    turn_index: p.turn_index,
                    acting: p.acting,
                    record: p.record,
                });
            }
        }

        // Every rule that fires, with its first-fired turn; a demoted rule
        // fires into the next row, or as LATER.
        let mut fired: Vec<Advice> = Vec::new();
        let mut firing: Vec<&'static str> = Vec::new();
        for r in &self.rules {
            let id = r.id();
            let Some(mut a) = r.evaluate(state) else {
                continue;
            };
            firing.push(id);
            a.since_turn = *self.first_fired.entry(id).or_insert(turn);
            if let Some(s) = self.snoozed.get(id) {
                let why = match s.until_turn {
                    Some(u) => format!("snoozed until turn {u}"),
                    None => "snoozed for the session".into(),
                };
                self.suppressed.push((id, why));
                continue;
            }
            if let Some(u) = self.cooldown_until.get(id) {
                self.suppressed
                    .push((id, format!("cooldown until turn {u}")));
                continue;
            }
            match self.demoted.get(id) {
                Some(Demotion::NextRow) => a.next_row_only = true,
                Some(Demotion::Later) => a.urgency = Urgency::Later,
                None => {}
            }
            fired.push(a);
        }
        self.first_fired.retain(|id, _| firing.contains(id));

        // Suppression by mode and by moment (NOW is exempt from the moment
        // rules; the machine, workflow and loop modes silence everything).
        let away_ms = state.last_line_at_ms.map(|t| now - t).unwrap_or(0);
        let mode = self.session_mode;
        let mut kept = Vec::new();
        for a in fired {
            let why = match mode {
                SessionMode::Machine => Some("machine turn".to_string()),
                SessionMode::Workflow => Some("workflow / -p session".to_string()),
                SessionMode::Loop => Some("bot loop".to_string()),
                SessionMode::Team
                    if matches!(
                        a.family,
                        "context-reset" | "waiting" | "cold-resume" | "post-compaction"
                    ) =>
                {
                    Some("team session".to_string())
                }
                SessionMode::Remote if a.family == "waiting" => Some("remote session".to_string()),
                _ => None,
            }
            .or_else(|| {
                if a.urgency == Urgency::Now {
                    return None;
                }
                if turn <= 2 {
                    Some("first two turns".to_string())
                } else if away_ms > 2 * 3_600_000 && a.family != "cold-resume" {
                    Some("away > 2 h".to_string())
                } else {
                    None
                }
            })
            .or_else(|| {
                TIP_FOR_FAMILY
                    .iter()
                    .find(|(f, _)| *f == a.family)
                    .and_then(|(_, tips)| tips.iter().find(|t| state.tips_recent.contains(**t)))
                    .map(|t| format!("Claude Code showed its `{t}` tip recently"))
            });
            match why {
                Some(w) => self.suppressed.push((a.rule, w)),
                None => kept.push(a),
            }
        }
        let mut fired = kept;

        // Retire the occupant: acted, predicate false, three strikes, TTL.
        if let Some(occ) = self.occupant.clone() {
            let rule = self.rules.iter().find(|r| r.id() == occ.advice.rule);
            let still = fired.iter().any(|a| a.rule == occ.advice.rule);
            let acted = rule.is_some_and(|r| r.acted(state, &occ.advice));
            let turn_ended = state.agg.turns.len() > occ.turn_index
                || state
                    .agg
                    .turns
                    .get(occ.turn_index.wrapping_sub(1))
                    .is_some_and(|t| t.duration_ms.is_some());
            let expired = match rule.map(|r| r.ttl()).unwrap_or(Ttl::NextPrompt) {
                Ttl::TurnEnd => turn > occ.promoted_turn || turn_ended,
                Ttl::NextPrompt => turn > occ.promoted_turn,
                Ttl::NextTurnEnd => {
                    turn > occ.promoted_turn + 1
                        || (turn == occ.promoted_turn + 1
                            && state
                                .agg
                                .current_turn()
                                .is_some_and(|t| t.duration_ms.is_some()))
                }
                Ttl::ThreeTurns => turn >= occ.promoted_turn + 3,
            };
            let strikes = !acted && turn >= occ.promoted_turn + 3;
            let reason = if acted {
                Some("acted")
            } else if !still {
                Some("retired")
            } else if strikes {
                Some("snoozed")
            } else if expired {
                Some("expired")
            } else {
                None
            };
            if let Some(reason) = reason {
                let cooldown = rule.map(|r| r.cooldown_turns()).unwrap_or(5);
                self.cooldown_until.insert(occ.advice.rule, turn + cooldown);
                if reason == "snoozed" {
                    self.snoozed.insert(
                        occ.advice.rule,
                        Snooze {
                            until_turn: None,
                            count: 3,
                        },
                    );
                }
                let rule_id = occ.advice.rule;
                self.occupant = None;
                self.retire(occ, reason, now);
                fired.retain(|a| a.rule != rule_id);
            }
        }
        // The occupant's own rule is not queued behind itself.
        let occupant_rule = self.occupant.as_ref().map(|o| o.advice.rule);
        fired.retain(|a| Some(a.rule) != occupant_rule);

        // Rank: class, the published order, then the saving.
        let remaining = remaining_calls(state);
        fired.sort_by(|a, b| {
            a.urgency
                .cmp(&b.urgency)
                .then_with(|| a.next_row_only.cmp(&b.next_row_only))
                .then_with(|| {
                    order_index(a.urgency, a.family).cmp(&order_index(b.urgency, b.family))
                })
                .then_with(|| b.saving.rank(remaining).cmp(&a.saving.rank(remaining)))
        });

        // Promote, within the budget.
        // The first slot-eligible candidate (next-row-only rules wait in
        // the queue whatever their rank).
        let eligible = fired.iter().position(|a| !a.next_row_only);
        if eligible.is_some_and(|i| self.promotable(Some(&fired[i]), turn)) {
            let a = fired.remove(eligible.unwrap_or(0));
            if let Some(old) = self.occupant.take() {
                self.retire(old, "pre-empted", now);
            }
            let last_input = state
                .agg
                .current_turn()
                .and_then(|t| t.last_human_input_at.as_deref().or(t.started_at.as_deref()))
                .and_then(crate::metrics::cost::parse_ts_ms);
            self.records.push(FireRecord {
                rule: a.rule.into(),
                family: a.family.into(),
                class: a.urgency,
                turn,
                shown_at_ms: now,
                retired: None,
                acted_delay_ms: None,
                snoozed: false,
                session_mode: self.session_mode,
                surface: if self.exposed {
                    self.surface.clone()
                } else {
                    "none".into()
                },
                human_idle_ms: last_input.map(|t| (now - t).max(0)),
                time_to_x_ms: None,
                snoozed_session: false,
                toggled_away: false,
                version: state.session.version.clone(),
                model: state.model().unwrap_or("").to_string(),
                project: state.session.cwd.to_string_lossy().into_owned(),
            });
            self.pending_events.push(CoachEvent {
                at_ms: now,
                text: format!(
                    "{} {} fired · {}",
                    a.urgency.label(),
                    a.rule,
                    crate::ui::fmt::clip(&a.headline, 60)
                ),
            });
            self.promotions.push_back((turn, a.urgency));
            while self.promotions.len() > 20 {
                self.promotions.pop_front();
            }
            self.occupant = Some(Occupant {
                advice: a,
                fired_at_ms: now,
                promoted_turn: turn,
                turn_index: state.agg.turns.len(),
                acting: false,
                record: self.records.len() - 1,
            });
            self.dirty = true;
        }

        let mut current: Vec<Advice> = Vec::with_capacity(fired.len() + 1);
        if let Some(o) = &self.occupant {
            let mut a = o.advice.clone();
            // The occupant's text follows its evidence while it still fires.
            if let Some(fresh) = self
                .rules
                .iter()
                .find(|r| r.id() == a.rule)
                .and_then(|r| r.evaluate(state))
            {
                a.headline = fresh.headline;
                a.evidence = fresh.evidence;
                a.action = fresh.action;
                a.action_text = fresh.action_text;
                a.saving = fresh.saving;
            }
            current.push(a);
        }
        current.extend(fired);
        self.current = current;
    }

    /// Whether the top candidate may take the slot now.
    fn promotable(&self, cand: Option<&Advice>, turn: usize) -> bool {
        let Some(cand) = cand else {
            return false;
        };
        if let Some(o) = &self.occupant {
            // Only a higher class pre-empts an occupant.
            if cand.urgency >= o.advice.urgency {
                return false;
            }
        }
        if cand.urgency == Urgency::Now {
            return true;
        }
        let promoted_this_turn = self.promotions.iter().any(|(t, _)| *t == turn);
        let in_last_ten = self
            .promotions
            .iter()
            .filter(|(t, _)| *t + 10 > turn)
            .count();
        let later_in_last_two = self
            .promotions
            .iter()
            .any(|(t, c)| *c == Urgency::Later && *t + 2 > turn);
        !(promoted_this_turn
            || in_last_ten >= 3
            || (cand.urgency == Urgency::Later && later_in_last_two))
    }

    fn retire(&mut self, occ: Occupant, reason: &'static str, now: i64) {
        let fam = occ.advice.family;
        let detail = match reason {
            "acted" => format!(
                "{fam} → done {} later",
                crate::ui::fmt::duration_ms(now - occ.fired_at_ms)
            ),
            "expired" => format!("{fam} · expired unacted"),
            "snoozed" => format!("{fam} · snoozed"),
            "pre-empted" => format!("{fam} · a higher class took the slot"),
            _ => format!("{fam} · condition cleared"),
        };
        if let Some(r) = self.records.get_mut(occ.record) {
            r.retired = Some((reason.to_string(), now));
            if reason == "acted" {
                r.acted_delay_ms = Some(now - occ.fired_at_ms);
            }
            if reason == "snoozed" {
                r.snoozed = true;
            }
        }
        if reason != "pre-empted" {
            self.pending_events.push(CoachEvent {
                at_ms: now,
                text: format!("{} {reason} · {detail}", occ.advice.rule),
            });
        } else {
            // A pre-empted nudge may come straight back when the slot frees.
            self.cooldown_until.remove(occ.advice.rule);
        }
        self.recent.insert(
            0,
            Lifecycle {
                rule: occ.advice.rule,
                family: fam,
                at_ms: now,
                what: reason,
                detail,
            },
        );
        self.recent.truncate(8);
        self.dirty = true;
    }

    /// Events rows produced since the last call (`kind=coach`).
    pub fn drain_events(&mut self) -> Vec<CoachEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// The Events rows not yet drained: a reader that evaluates once (`cctop
    /// query`) folds them into the events body itself, so its rows are the
    /// TUI's, which drained them into `State::events`.
    pub fn pending_events(&self) -> &[CoachEvent] {
        &self.pending_events
    }

    /// `x`: snooze for five human turns; the third snooze of a rule lasts
    /// the session. Returns what happened, for the toast.
    pub fn snooze(&mut self, rule: &'static str, turn: usize, now: i64) -> String {
        let s = self.snoozed.entry(rule).or_default();
        s.count = s.count.saturating_add(1);
        let text = if s.count >= 3 {
            s.until_turn = None;
            format!("{rule} snoozed for the session (third snooze)")
        } else {
            s.until_turn = Some(turn + 5);
            format!("{rule} snoozed for 5 turns")
        };
        self.mark_snoozed(rule, now);
        text
    }

    /// `X`: snooze for the rest of the session.
    pub fn snooze_session(&mut self, rule: &'static str, now: i64) -> String {
        self.snoozed.insert(
            rule,
            Snooze {
                until_turn: None,
                count: 3,
            },
        );
        self.mark_snoozed(rule, now);
        format!("{rule} snoozed for the session")
    }

    fn mark_snoozed(&mut self, rule: &'static str, now: i64) {
        let session_wide = self
            .snoozed
            .get(rule)
            .is_some_and(|s| s.until_turn.is_none());
        if self
            .occupant
            .as_ref()
            .is_some_and(|o| o.advice.rule == rule)
        {
            let occ = self.occupant.take().unwrap();
            if let Some(r) = self.records.get_mut(occ.record) {
                r.time_to_x_ms = Some((now - occ.fired_at_ms).max(0));
                r.snoozed_session = session_wide;
            }
            self.retire(occ, "snoozed", now);
        } else {
            self.pending_events.push(CoachEvent {
                at_ms: now,
                text: format!("{rule} snoozed"),
            });
        }
        self.current.retain(|a| a.rule != rule);
        self.dirty = true;
    }

    /// A snooze from a surface that may not hold the lock (`cctop query
    /// coach --snooze`, the pane, the MCP tool): applied here when this
    /// process may write, else queued for the writer to pick up on its next
    /// tick. Returns what happened, for the caller's message.
    pub fn snooze_request(
        &mut self,
        rule: &'static str,
        session_wide: bool,
        turn: usize,
        now: i64,
    ) -> String {
        let queued = self
            .path
            .as_ref()
            .filter(|p| !self.writer && lock_alive(&lock_path(p)));
        match queued {
            Some(path) => {
                let line = serde_json::json!({"rule": rule, "session": session_wide, "at_ms": now});
                let req = requests_path(path);
                let mut text = std::fs::read_to_string(&req).unwrap_or_default();
                text.push_str(&line.to_string());
                text.push('\n');
                if std::fs::write(&req, text).is_ok() {
                    // Reflect it locally so this reader's own output agrees.
                    self.snoozed.insert(
                        rule,
                        Snooze {
                            until_turn: (!session_wide).then_some(turn + 5),
                            count: if session_wide { 3 } else { 1 },
                        },
                    );
                    self.current.retain(|a| a.rule != rule);
                    if self
                        .occupant
                        .as_ref()
                        .is_some_and(|o| o.advice.rule == rule)
                    {
                        self.occupant = None;
                    }
                    format!("{rule} snooze queued for the running dashboard")
                } else {
                    format!("{rule}: could not queue the snooze")
                }
            }
            None => {
                let text = if session_wide {
                    self.snooze_session(rule, now)
                } else {
                    self.snooze(rule, turn, now)
                };
                self.save();
                text
            }
        }
    }

    /// The writer applies the snoozes readers queued.
    pub fn poll_requests(&mut self, turn: usize, now: i64) -> Vec<String> {
        let mut out = Vec::new();
        let Some(path) = self.path.clone().filter(|_| self.writer) else {
            return out;
        };
        let req = requests_path(&path);
        let Ok(text) = std::fs::read_to_string(&req) else {
            return out;
        };
        let _ = std::fs::remove_file(&req);
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let rule = v.get("rule").and_then(|r| r.as_str()).unwrap_or("");
            let Some(id) = self.rules.iter().map(|r| r.id()).find(|id| *id == rule) else {
                continue;
            };
            let session_wide = v.get("session").and_then(|s| s.as_bool()).unwrap_or(false);
            out.push(if session_wide {
                self.snooze_session(id, now)
            } else {
                self.snooze(id, turn, now)
            });
        }
        out
    }

    /// The rule id in the catalog for `rule`, if any.
    pub fn rule_id(&self, rule: &str) -> Option<&'static str> {
        self.rules.iter().map(|r| r.id()).find(|id| *id == rule)
    }

    /// `Enter`: the person is acting on the occupant.
    pub fn acting(&mut self) {
        if let Some(o) = self.occupant.as_mut() {
            o.acting = true;
        }
    }

    /// The queued nudge that would take the slot next, with the condition
    /// that promotes it.
    pub fn next_up(&self) -> Option<(&Advice, &'static str)> {
        let a = self
            .current
            .get(if self.occupant.is_some() { 1 } else { 0 })?;
        let cond = match (self.occupant.as_ref(), a.urgency) {
            _ if a.next_row_only => "next-row only",
            (Some(o), Urgency::Now) if o.advice.urgency != Urgency::Now => "on its NOW event",
            (Some(o), Urgency::Next) if o.advice.urgency == Urgency::Later => {
                "at the next boundary"
            }
            (Some(_), _) => "when the slot frees",
            (None, Urgency::Now) => "now",
            (None, _) => "next prompt",
        };
        Some((a, cond))
    }

    /// Rules snoozed right now: `(rule, until turn or None for the session)`.
    pub fn snoozed(&self) -> Vec<(&'static str, Option<usize>)> {
        let mut v: Vec<_> = self
            .snoozed
            .iter()
            .map(|(k, s)| (*k, s.until_turn))
            .collect();
        v.sort();
        v
    }

    /// The SessionEnd tally: `(fired, acted, snoozed)`.
    /// The coach view was left at `now`: within 10 s of the occupant's
    /// promotion that is a toggle-away, kept on its record.
    pub fn note_view_left(&mut self, now: i64) {
        if let Some(o) = &self.occupant {
            if now - o.fired_at_ms <= 10_000 {
                if let Some(r) = self.records.get_mut(o.record) {
                    r.toggled_away = true;
                    self.dirty = true;
                }
            }
        }
    }

    pub fn tally(&self) -> (usize, usize, usize) {
        let fired = self.records.len();
        let acted = self
            .records
            .iter()
            .filter(|r| r.retired.as_ref().is_some_and(|(w, _)| w == "acted"))
            .count();
        let snoozed = self.records.iter().filter(|r| r.snoozed).count();
        (fired, acted, snoozed)
    }

    pub fn rule_ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|r| r.id()).collect()
    }
}

/// Expected remaining API calls: the baseline's median calls per session
/// minus the calls so far, floor 10 (300 when no baseline exists).
fn remaining_calls(state: &State) -> u64 {
    let median = state
        .baseline
        .as_ref()
        .and_then(|b| b.calls_per_session)
        .unwrap_or(300.0) as u64;
    median.saturating_sub(state.agg.api_calls() as u64).max(10)
}

/// Write our pid into the lock unless another live process holds it.
fn take_lock(lock: &Path) -> bool {
    if lock_holder(lock).is_some_and(|pid| pid != std::process::id()) {
        return false;
    }
    if let Some(dir) = lock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(lock, std::process::id().to_string()).is_ok()
}

/// The live pid a lock file names, if any.
fn lock_holder(lock: &Path) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(lock).ok()?.trim().parse().ok()?;
    crate::registry::pid_alive(pid).then_some(pid)
}

/// A lock file names a live process (a reader must not write).
fn lock_alive(lock: &Path) -> bool {
    lock_holder(lock).is_some()
}

#[cfg(test)]
pub mod tests_support {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;

    /// A state with `n` human turns, one small API call each, one minute
    /// apart, the clock 30 s after the last prompt.
    pub fn state_with_turns(n: usize) -> State {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        for i in 0..n {
            let (h, m) = (i / 60, i % 60);
            s.apply(&Line::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T{h:02}:{m:02}:00Z","promptId":"p{i}","promptSource":"typed","message":{{"role":"user","content":"go"}}}}"#
            )).unwrap());
            s.apply(&Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T{h:02}:{m:02}:01Z","message":{{"id":"m{i}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":5}}}}}}"#
            )).unwrap());
        }
        let last = n.saturating_sub(1);
        s.now_ms = crate::metrics::cost::parse_ts_ms(&format!(
            "2026-01-01T{:02}:{:02}:30Z",
            last / 60,
            last % 60
        ))
        .unwrap();
        s
    }

    /// A rule that always fires with a fixed class and saving.
    pub struct Fixed(pub &'static str, pub &'static str, pub Urgency, pub Saving);
    impl Rule for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn family(&self) -> &'static str {
            self.1
        }
        fn urgency(&self) -> Urgency {
            self.2
        }
        fn evaluate(&self, _: &State) -> Option<Advice> {
            let mut a = Advice::new(self.0, self.1, self.2);
            a.headline = format!("{} fires", self.0);
            a.saving = self.3;
            Some(a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{state_with_turns, Fixed};
    use super::*;
    use crate::metrics::Pricing;

    #[test]
    fn empty_session_fires_nothing_and_the_catalog_is_recalibrated() {
        let mut e = Engine::default();
        e.evaluate(&State::new(Pricing::bundled()));
        assert!(e.current.is_empty());
        assert!(e.occupant.is_none());
        let ids = e.rule_ids();
        assert_eq!(ids.len(), 36, "{ids:?}");
        assert!(!ids.contains(&"A15"), "A15 retired for A38");
        assert!(!ids.contains(&"A18"), "A18 retired for A42");
        for id in [
            "A32", "A33", "A36", "A38", "A40", "A41", "A42", "A45", "A47",
        ] {
            assert!(ids.contains(&id), "{id} is a slot rule");
        }
        for id in ["A34", "A46"] {
            assert!(ids.contains(&id), "{id} is next-row only");
        }
        assert!(
            !ids.contains(&"A05") && ids.contains(&"A25"),
            "A05 became A25"
        );
    }

    #[test]
    fn classes_then_the_published_order_rank_the_queue() {
        let mut e = Engine::new(vec![
            Box::new(Fixed(
                "L",
                "prefix-tip",
                Urgency::Later,
                Saving::Tokens(5_000),
            )),
            Box::new(Fixed("X", "verify-gap", Urgency::Next, Saving::Seconds(10))),
            Box::new(Fixed("W", "waiting", Urgency::Now, Saving::Avoids)),
            Box::new(Fixed("D", "turn-died", Urgency::Now, Saving::Avoids)),
        ]);
        let s = state_with_turns(3);
        e.evaluate(&s);
        let order: Vec<_> = e.current.iter().map(|a| a.rule).collect();
        assert_eq!(order, ["D", "W", "X", "L"]);
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "D");
        let (next, cond) = e.next_up().unwrap();
        assert_eq!((next.rule, cond), ("W", "when the slot frees"));
        assert_eq!(e.drain_events().len(), 1, "one fire row");
        assert!(e.drain_events().is_empty());
    }

    #[test]
    fn only_a_higher_class_preempts_and_the_occupant_refreshes() {
        // A NEXT occupant; a NOW newcomer pre-empts it, a LATER waits.
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Switch(std::sync::Arc<AtomicBool>);
        impl Rule for Switch {
            fn id(&self) -> &'static str {
                "W"
            }
            fn family(&self) -> &'static str {
                "waiting"
            }
            fn urgency(&self) -> Urgency {
                Urgency::Now
            }
            fn evaluate(&self, s: &State) -> Option<Advice> {
                self.0
                    .load(Ordering::Relaxed)
                    .then(|| Fixed("W", "waiting", Urgency::Now, Saving::Avoids).evaluate(s))
                    .flatten()
            }
        }
        let toggle = std::sync::Arc::new(AtomicBool::new(false));
        let mut e = Engine::new(vec![
            Box::new(Fixed("X", "verify-gap", Urgency::Next, Saving::Seconds(10))),
            Box::new(Fixed(
                "L",
                "prefix-tip",
                Urgency::Later,
                Saving::Tokens(5_000),
            )),
            Box::new(Switch(toggle.clone())),
        ]);
        let s = state_with_turns(3);
        e.evaluate(&s);
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "X");
        e.evaluate(&s);
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "X", "L waits");
        toggle.store(true, Ordering::Relaxed);
        e.evaluate(&s);
        assert_eq!(
            e.occupant.as_ref().unwrap().advice.rule,
            "W",
            "NOW pre-empts"
        );
        assert_eq!(e.recent[0].what, "pre-empted");
        toggle.store(false, Ordering::Relaxed);
        e.evaluate(&s);
        // W's predicate went false: retired; X may come straight back (its
        // pre-emption set no cooldown) and the per-turn budget allows one
        // promotion per turn — X was promoted this turn already, so the
        // slot stays empty until the next prompt.
        assert_eq!(e.recent[0].what, "retired");
        assert!(e.occupant.is_none());
        e.evaluate(&state_with_turns(4));
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "X");
    }

    #[test]
    fn budget_one_promotion_per_turn_three_per_ten_one_later_per_two() {
        let rules: Vec<Box<dyn Rule>> = vec![
            Box::new(Fixed("A", "verify-gap", Urgency::Next, Saving::Tokens(1))),
            Box::new(Fixed(
                "B",
                "context-reset",
                Urgency::Next,
                Saving::Tokens(1),
            )),
            Box::new(Fixed(
                "C",
                "explore-delegate",
                Urgency::Next,
                Saving::Tokens(1),
            )),
            Box::new(Fixed("L", "prefix-tip", Urgency::Later, Saving::Tokens(1))),
        ];
        let mut e = Engine::new(rules);
        let mut promoted = Vec::new();
        for n in 1..=14 {
            let s = state_with_turns(n);
            e.evaluate(&s);
            if let Some(o) = &e.occupant {
                if o.promoted_turn == n && promoted.last() != Some(&(n, o.advice.rule)) {
                    promoted.push((n, o.advice.rule));
                }
            }
        }
        assert!(
            promoted.iter().all(|(n, _)| *n >= 3),
            "silent first two turns: {promoted:?}"
        );
        assert!(promoted.len() >= 3, "{promoted:?}");
        for start in 1..=14 {
            let in_window = promoted
                .iter()
                .filter(|(n, _)| *n >= start && *n < start + 10)
                .count();
            assert!(in_window <= 3, "{promoted:?}");
        }
        let later: Vec<_> = promoted.iter().filter(|(_, r)| *r == "L").collect();
        for w in later.windows(2) {
            assert!(w[1].0 - w[0].0 >= 2, "{later:?}");
        }
        // The saving rank is a tiebreak inside the published order, and a
        // one-off saving is not multiplied by the remaining calls.
        assert!(Saving::Tokens(100).rank(50) > Saving::OneOff(1_000).rank(50));
        assert_eq!(Saving::OneOff(1_000).rank(50), 1_000);
    }

    #[test]
    fn snooze_x_and_shift_x_and_three_strikes_persist_in_the_tally() {
        let mut e = Engine::new(vec![Box::new(Fixed(
            "A",
            "verify-gap",
            Urgency::Next,
            Saving::Tokens(1),
        ))]);
        let s = state_with_turns(4);
        e.evaluate(&s);
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "A");
        assert_eq!(e.snooze("A", 4, s.clock_ms()), "A snoozed for 5 turns");
        assert!(e.occupant.is_none());
        assert_eq!(e.snoozed(), vec![("A", Some(9))]);
        assert_eq!(e.recent[0].what, "snoozed");
        e.evaluate(&state_with_turns(6));
        assert!(e.occupant.is_none(), "still snoozed");
        assert!(e
            .suppressed
            .iter()
            .any(|(r, w)| *r == "A" && w.starts_with("snoozed until")));
        e.evaluate(&state_with_turns(10));
        assert!(e.occupant.is_some(), "snooze over");
        assert_eq!(e.snooze_session("A", 0), "A snoozed for the session");
        assert_eq!(e.snoozed(), vec![("A", None)]);
        e.evaluate(&state_with_turns(30));
        assert!(e.occupant.is_none());
        assert_eq!(e.tally(), (2, 0, 2));

        // Three strikes: a LATER nudge held unacted for three turns snoozes
        // itself for the session.
        let mut e = Engine::new(vec![Box::new(Fixed(
            "L",
            "prefix-tip",
            Urgency::Later,
            Saving::Tokens(1),
        ))]);
        e.evaluate(&state_with_turns(3));
        assert!(e.occupant.is_some());
        e.evaluate(&state_with_turns(6));
        assert!(e.occupant.is_none());
        assert_eq!(e.snoozed(), vec![("L", None)]);
        assert_eq!(e.recent[0].what, "snoozed");
        let ev = e.drain_events();
        assert!(
            ev.iter().any(|x| x.text.starts_with("LATER L fired")),
            "{ev:?}"
        );
        assert!(
            ev.iter().any(|x| x.text.starts_with("L snoozed ·")),
            "{ev:?}"
        );
    }

    #[test]
    fn acted_retires_with_the_delay_and_expiry_is_by_class() {
        struct ActsOnTurn5;
        impl Rule for ActsOnTurn5 {
            fn id(&self) -> &'static str {
                "V"
            }
            fn family(&self) -> &'static str {
                "verify-gap"
            }
            fn urgency(&self) -> Urgency {
                Urgency::Next
            }
            fn evaluate(&self, s: &State) -> Option<Advice> {
                Fixed("V", "verify-gap", Urgency::Next, Saving::Tokens(1)).evaluate(s)
            }
            fn acted(&self, s: &State, _: &Advice) -> bool {
                s.agg.human_turns() >= 5
            }
            fn ttl(&self) -> Ttl {
                Ttl::ThreeTurns
            }
        }
        let mut e = Engine::new(vec![Box::new(ActsOnTurn5)]);
        e.evaluate(&state_with_turns(3));
        assert!(e.occupant.is_some());
        e.evaluate(&state_with_turns(4));
        assert!(e.occupant.is_some(), "three-turn TTL");
        e.evaluate(&state_with_turns(5));
        assert!(e.occupant.is_none());
        assert_eq!(e.recent[0].what, "acted");
        assert!(e.records[0].acted_delay_ms.is_some());
        assert_eq!(e.tally(), (1, 1, 0));
        // Cooldown: quiet for five turns after retiring.
        e.evaluate(&state_with_turns(7));
        assert!(e.occupant.is_none());
        assert!(e
            .suppressed
            .iter()
            .any(|(r, w)| *r == "V" && w.starts_with("cooldown until turn 10")));
        e.evaluate(&state_with_turns(10));
        assert!(e.occupant.is_some());
        // A NEXT rule with the default TTL expires at the next prompt.
        let mut e = Engine::new(vec![Box::new(Fixed(
            "N",
            "verify-gap",
            Urgency::Next,
            Saving::Tokens(1),
        ))]);
        e.evaluate(&state_with_turns(3));
        e.evaluate(&state_with_turns(4));
        assert_eq!(e.recent[0].what, "expired");
    }

    #[test]
    fn machine_turns_loops_and_the_first_two_turns_are_silent() {
        let mut e = Engine::new(vec![
            Box::new(Fixed("X", "verify-gap", Urgency::Next, Saving::Tokens(1))),
            Box::new(Fixed("W", "waiting", Urgency::Now, Saving::Avoids)),
        ]);
        let s = state_with_turns(1);
        e.evaluate(&s);
        assert_eq!(
            e.occupant.as_ref().unwrap().advice.rule,
            "W",
            "NOW is exempt from the first-two-turns rule"
        );
        assert!(e
            .suppressed
            .iter()
            .any(|(r, w)| *r == "X" && w == "first two turns"));
        // A task-notification turn: nothing at all.
        let mut s = state_with_turns(3);
        s.apply(&crate::transcript::Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:05:00Z","promptId":"pm","promptSource":"system","origin":{"kind":"task-notification"},"message":{"role":"user","content":"<task-notification>done</task-notification>"}}"#).unwrap());
        let mut e = Engine::new(vec![Box::new(Fixed(
            "W",
            "waiting",
            Urgency::Now,
            Saving::Avoids,
        ))]);
        e.evaluate(&s);
        assert_eq!(e.session_mode, SessionMode::Machine);
        assert!(e.occupant.is_none());
        assert!(e.suppressed.iter().any(|(_, w)| w == "machine turn"));
        // A Stop hook that blocked the last two turns from ending: a loop.
        let mut s = state_with_turns(2);
        s.apply(&crate::transcript::Line::parse(r#"{"type":"system","subtype":"stop_hook_summary","timestamp":"2026-01-01T00:01:02Z","hookInfos":[{"command":"ralph","durationMs":10}],"hookErrors":[],"preventedContinuation":true}"#).unwrap());
        s.apply(&crate::transcript::Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:02:00Z","promptId":"p9","promptSource":"typed","message":{"role":"user","content":"go"}}"#).unwrap());
        s.apply(&crate::transcript::Line::parse(r#"{"type":"system","subtype":"stop_hook_summary","timestamp":"2026-01-01T00:02:02Z","hookInfos":[{"command":"ralph","durationMs":10}],"hookErrors":[],"preventedContinuation":true}"#).unwrap());
        assert_eq!(SessionMode::derive(&s), SessionMode::Loop);
        let mut e = Engine::new(vec![Box::new(Fixed(
            "W",
            "waiting",
            Urgency::Now,
            Saving::Avoids,
        ))]);
        e.evaluate(&s);
        assert!(e.occupant.is_none());
        assert!(e.suppressed.iter().any(|(_, w)| w == "bot loop"));
        // A tip Claude Code showed recently mutes the matching family.
        let mut s = state_with_turns(4);
        s.tips_recent.insert("permissions".into());
        let mut e = Engine::new(vec![Box::new(Fixed(
            "P",
            "permission-wait",
            Urgency::Next,
            Saving::Seconds(5),
        ))]);
        e.evaluate(&s);
        assert!(e.occupant.is_none());
        assert!(e
            .suppressed
            .iter()
            .any(|(_, w)| w.contains("`permissions` tip")));
    }

    /// Two NOW rules fire; the writer promoted the lower-ranked one first
    /// and keeps it (a same-class newcomer waits). A reader ranking afresh
    /// would show the other: it adopts the writer's slot instead.
    #[test]
    fn a_reader_adopts_the_writers_occupant() {
        let home = std::env::temp_dir().join(format!("cctop-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let waiting = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        struct Switch(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Rule for Switch {
            fn id(&self) -> &'static str {
                "W"
            }
            fn family(&self) -> &'static str {
                "waiting"
            }
            fn urgency(&self) -> Urgency {
                Urgency::Now
            }
            fn evaluate(&self, _: &State) -> Option<Advice> {
                self.0
                    .load(std::sync::atomic::Ordering::SeqCst)
                    .then(|| Advice::new("W", "waiting", Urgency::Now))
            }
        }
        let mk = |w: &std::sync::Arc<std::sync::atomic::AtomicBool>| {
            Engine::new(vec![
                Box::new(Fixed(
                    "C",
                    "correction-streak",
                    Urgency::Now,
                    Saving::Avoids,
                )) as Box<dyn Rule>,
                Box::new(Switch(w.clone())),
            ])
        };
        let mut writer = mk(&waiting);
        writer.attach(&home, "s2", true);
        let s = state_with_turns(4);
        writer.evaluate(&s);
        assert_eq!(writer.occupant.as_ref().unwrap().advice.rule, "C");
        waiting.store(true, std::sync::atomic::Ordering::SeqCst);
        writer.evaluate(&s);
        assert_eq!(
            writer.occupant.as_ref().unwrap().advice.rule,
            "C",
            "a same-class newcomer waits in next"
        );
        assert_eq!(writer.current[1].rule, "W");
        writer.save();
        let mut reader = mk(&waiting);
        reader.attach(&home, "s2", false);
        reader.evaluate(&s);
        assert_eq!(
            reader.occupant.as_ref().unwrap().advice.rule,
            "C",
            "the reader shows the writer's slot"
        );
        assert_eq!(
            reader.current.iter().map(|a| a.rule).collect::<Vec<_>>(),
            ["C", "W"]
        );
        assert_eq!(reader.records.len(), 1, "no second fire record");
        // Without a file the same reader ranks afresh: waiting first.
        let mut fresh = mk(&waiting);
        fresh.evaluate(&s);
        assert_eq!(fresh.occupant.as_ref().unwrap().advice.rule, "W");
        writer.release();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// US-012's measurement hooks: the control arm records a fire under
    /// `surface: none` and shows nothing; a snooze keeps its delay; leaving
    /// the coach view within 10 s is a toggle-away; a demoted rule fires
    /// into the next row, or as LATER.
    #[test]
    fn control_arm_time_to_x_toggle_away_and_demotions() {
        let mk = || {
            Engine::new(vec![
                Box::new(Fixed("A", "verify-gap", Urgency::Next, Saving::Tokens(1)))
                    as Box<dyn Rule>,
            ])
        };
        let mut s = state_with_turns(4);
        s.session.version = "2.1.270".into();
        let mut e = mk();
        e.surface = "tui-coach".into();
        e.exposed = false;
        e.evaluate(&s);
        assert!(e.occupant.is_some(), "the engine keeps working");
        assert_eq!(e.records[0].surface, "none");
        assert_eq!(e.records[0].version, "2.1.270");
        assert_eq!(e.records[0].model.as_str(), "claude-opus-5");
        assert_eq!(e.records[0].human_idle_ms, Some(30_000));
        let c = crate::coach::snapshot(&s, &e);
        assert!(c.nudge.is_none() && c.next.is_none() && !c.exposed);
        assert!(
            c.quiet_row.starts_with("  coach off (control arm)"),
            "{}",
            c.quiet_row
        );
        assert!(e.persisted().exposure == "off");
        // Exposed: the surface is stamped; x 1.5 s later is a reflex.
        let mut e = mk();
        e.surface = "tui-dashboard".into();
        e.evaluate(&s);
        assert_eq!(e.records[0].surface, "tui-dashboard");
        let shown = e.records[0].shown_at_ms;
        e.snooze("A", 4, shown + 1_500);
        assert_eq!(e.records[0].time_to_x_ms, Some(1_500));
        assert!(!e.records[0].snoozed_session);
        let mut e = mk();
        e.surface = "tui-coach".into();
        e.evaluate(&s);
        let shown = e.records[0].shown_at_ms;
        e.note_view_left(shown + 5_000);
        assert!(e.records[0].toggled_away);
        e.snooze_session("A", shown + 20_000);
        assert!(e.records[0].snoozed_session);
        assert_eq!(e.records[0].time_to_x_ms, Some(20_000));
        // Demotions from the record.
        let mut e = mk();
        e.demote(&[("A".to_string(), Demotion::NextRow)].into_iter().collect());
        e.evaluate(&s);
        assert!(e.occupant.is_none(), "next-row only");
        assert!(e.current[0].next_row_only);
        let mut e = mk();
        e.demote(&[("A".to_string(), Demotion::Later)].into_iter().collect());
        e.evaluate(&s);
        assert_eq!(e.current[0].urgency, Urgency::Later);
        // A reader follows the writer's arm from the file.
        let home = std::env::temp_dir().join(format!("cctop-arm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let mut writer = mk();
        writer.attach(&home, "s3", true);
        writer.exposed = false;
        writer.evaluate(&s);
        writer.note_send(120);
        writer.save();
        let mut reader = mk();
        reader.attach(&home, "s3", false);
        assert!(!reader.exposed, "the control arm, as the file says");
        assert_eq!(reader.cost.socket_sends, 1);
        assert_eq!(reader.cost.socket_chars, 120);
        writer.release();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn persistence_roundtrip_and_single_writer() {
        let home = std::env::temp_dir().join(format!("cctop-advisor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let mk = || {
            Engine::new(vec![Box::new(Fixed(
                "A",
                "verify-gap",
                Urgency::Next,
                Saving::Tokens(1),
            ))])
        };
        let mut e = mk();
        e.attach(&home, "s1", true);
        assert!(e.writer, "took the lock");
        e.evaluate(&state_with_turns(4));
        e.snooze("A", 4, 0);
        e.save();
        let read = || -> Persisted {
            serde_json::from_str(&std::fs::read_to_string(persist_path(&home, "s1")).unwrap())
                .unwrap()
        };
        let p = read();
        assert_eq!(p.version, 2);
        assert_eq!(p.snoozed["A"].until_turn, Some(9));
        assert_eq!(p.records.len(), 1);
        assert_eq!(p.records[0].retired.as_ref().unwrap().0, "snoozed");
        // A second process (a query) reads the same state and may not write
        // while the lock is alive.
        let mut q = mk();
        q.attach(&home, "s1", false);
        assert!(!q.writer);
        assert_eq!(q.snoozed(), vec![("A", Some(9))]);
        q.evaluate(&state_with_turns(6));
        assert!(q.occupant.is_none(), "the query sees the TUI's snooze");
        q.snooze_session("A", 0);
        q.save();
        assert_eq!(
            read().snoozed["A"].until_turn,
            Some(9),
            "the reader could not write over the writer"
        );
        // A reader's snooze request is queued while the writer lives, and
        // the writer applies it on its next poll.
        let mut q2 = mk();
        q2.attach(&home, "s1", false);
        assert_eq!(
            q2.snooze_request("A", true, 6, 0),
            "A snooze queued for the running dashboard"
        );
        assert_eq!(q2.snoozed(), vec![("A", None)], "the reader reflects it");
        assert_eq!(
            read().snoozed["A"].until_turn,
            Some(9),
            "the file is the writer's"
        );
        let applied = e.poll_requests(6, 0);
        assert_eq!(applied, vec!["A snoozed for the session".to_string()]);
        e.save();
        assert_eq!(read().snoozed["A"].until_turn, None);
        assert!(e.poll_requests(6, 0).is_empty(), "consumed");
        e.release();
        q.dirty = true;
        q.save();
        assert_eq!(
            read().snoozed["A"].until_turn,
            None,
            "no lock: the reader may write"
        );
        // Without a writer the request applies directly (and a rule already
        // snoozed for the session stays so: the count persisted).
        let mut q3 = mk();
        q3.attach(&home, "s1", false);
        assert_eq!(
            q3.snooze_request("A", false, 20, 0),
            "A snoozed for the session (third snooze)"
        );
        assert_eq!(read().snoozed["A"].until_turn, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// `cctop advise --session fixtures/session-b.jsonl`: the ranked set the
    /// real catalog produces on fixture B, the slot first.
    #[test]
    fn fixture_b_ranked_set() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut s = State::new(Pricing::bundled());
        s.session = crate::ui::state::SessionInfo::from_fixture(&path);
        for l in crate::transcript::parse_file(&path).unwrap() {
            s.apply(&l);
        }
        s.session.ended_at_ms = s.last_line_at_ms;
        let e = Engine::for_state(&s);
        let ranked: Vec<(&str, &str)> = e
            .current
            .iter()
            .map(|a| (a.urgency.label(), a.rule))
            .collect();
        assert_eq!(ranked, [("LATER", "A10")], "{ranked:?}");
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "A10");
        assert_eq!(
            e.session_mode,
            SessionMode::Interactive,
            "bridge registered, prompts typed"
        );
        assert!(e.path.is_none(), "a fixture file never persists");
        assert!(e.suppressed.is_empty(), "{:?}", e.suppressed);
    }

    /// The advisor must be pure: rules only read `State`. The sources of the
    /// engine and every rule must not reach for processes or the network,
    /// and no rule reads prompt text.
    #[test]
    fn advisor_spawns_nothing_and_reads_no_prompt_text() {
        for src in [
            include_str!("mod.rs"),
            include_str!("rules/mod.rs"),
            include_str!("rules/token.rs"),
            include_str!("rules/events.rs"),
            include_str!("rules/outcome.rs"),
        ] {
            let body = src.split("#[cfg(test)]").next().unwrap_or(src);
            for forbidden in [
                "process::Command",
                "Command::new",
                "TcpStream",
                "reqwest",
                "std::net",
                "tokio::net",
                ".content.text()",
                "message.content.text",
                "prompt_text",
                ".description",
            ] {
                assert!(!body.contains(forbidden), "advisor uses {forbidden}");
            }
        }
        // And evaluating on a real session is side-effect free (pure fn of state).
        let mut e = Engine::default();
        let s = crate::ui::state::tests_support::fixture_state();
        e.evaluate(&s);
        let a = e.current.clone();
        e.evaluate(&s);
        assert_eq!(a, e.current);
    }
}
