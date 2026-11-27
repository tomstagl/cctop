//! One verdict per overview chart: Good, Watch or Act, one sentence and one
//! Do or Don't. Every threshold is in [`THRESHOLDS`], with where its number
//! comes from; every sentence is fixed, with holes for numbers and
//! structural names (a file's basename, a tool name) only.
//!
//! Each chart's verdict is a pure function of a small facts struct
//! (`*_from`), read from the state by `*Facts::of`, so every level is
//! testable without a transcript.

use serde::Serialize;

use crate::harness_facts::autocompact;
use crate::ui::fmt;
use crate::ui::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Good,
    Watch,
    Act,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Good => "Good",
            Level::Watch => "Watch",
            Level::Act => "Act",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    pub level: Level,
    /// One or two sentences, uncut.
    pub text: String,
    /// `Do: …` or `Don't: …`; absent when there is nothing to do yet.
    pub todo: Option<String>,
    /// The verdict's id in `src/metrics/registry.rs`.
    pub metric_id: &'static str,
}

/// Which way a measure gets worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// Worse as it grows (watch ≤ act).
    Above,
    /// Worse as it shrinks (watch ≥ act).
    Below,
}

/// One threshold of one chart's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Threshold {
    /// The chart id (`context`, `spend`, `cost_per_turn`, `cache`, `tool_calls`).
    pub chart: &'static str,
    pub id: &'static str,
    /// What is compared, in words.
    pub measure: &'static str,
    pub unit: &'static str,
    pub dir: Dir,
    pub watch: Option<f64>,
    pub act: Option<f64>,
    /// Where the number comes from.
    pub source: &'static str,
}

impl Threshold {
    /// The level `v` reaches on this threshold.
    pub fn level(&self, v: f64) -> Level {
        let past = |t: Option<f64>| {
            t.is_some_and(|t| match self.dir {
                Dir::Above => v >= t,
                Dir::Below => v <= t,
            })
        };
        if past(self.act) {
            Level::Act
        } else if past(self.watch) {
            Level::Watch
        } else {
            Level::Good
        }
    }
}

pub const CTX_TURNS_LEFT: Threshold = Threshold {
    chart: "context",
    id: "ctx_turns_left",
    measure: "turns until Claude Code compacts on its own, at the session's own growth per turn",
    unit: "turns",
    dir: Dir::Below,
    watch: Some(15.0),
    act: Some(5.0),
    source: "drafted; registry turns_until_compaction; coach PRD §5.1 A06 (re-trigger within threshold − 20k)",
};

pub const CTX_HEADROOM: Threshold = Threshold {
    chart: "context",
    id: "ctx_headroom",
    measure: "tokens left before the autocompact threshold",
    unit: "tokens",
    dir: Dir::Below,
    watch: None,
    act: Some(autocompact::WARN_TOKENS as f64),
    source: "harness_facts autocompact::WARN_TOKENS: Claude Code's own \"Context low\" band (registry context_band)",
};

pub const CTX_SIZE: Threshold = Threshold {
    chart: "context",
    id: "ctx_size",
    measure: "tokens in the window, re-read by every call",
    unit: "tokens",
    dir: Dir::Above,
    watch: Some(300_000.0),
    act: None,
    source: "coach PRD §3.1: calls above 300k are 26.5 % of calls and 53.6 % of spend; A23 escalates above 300k (tasks/research-coach/reader-context-cost-anatomy.txt)",
};

pub const CTX_MCP: Threshold = Threshold {
    chart: "context",
    id: "ctx_mcp",
    measure: "MCP tool schemas in every request, per Claude Code's /context",
    unit: "tokens",
    dir: Dir::Above,
    watch: Some(25_000.0),
    act: None,
    source: "drafted at half of A17's 50k prefix (advisor::rules::token::PREFIX_TOKENS); coach PRD §5.1 A07",
};

pub const SPEND_RATE: Threshold = Threshold {
    chart: "spend",
    id: "spend_rate",
    measure: "spend over the last 15 minutes, per minute (registry burn_rate)",
    unit: "USD/min",
    dir: Dir::Above,
    watch: Some(0.30),
    act: Some(1.00),
    source: "drafted; coach PRD §3.1: a turn at > 600k costs $7.32 against $0.41 below 100k",
};

pub const SPEND_BUDGET: Threshold = Threshold {
    chart: "spend",
    id: "spend_budget",
    measure: "share of the --max-budget-usd budget used",
    unit: "ratio",
    dir: Dir::Above,
    watch: Some(0.75),
    act: Some(0.90),
    source: "drafted; registry budget_usd",
};

pub const SPEND_HALT: Threshold = Threshold {
    chart: "spend",
    id: "spend_halt",
    measure: "minutes until the budget halts the session at the current rate",
    unit: "min",
    dir: Dir::Below,
    watch: Some(60.0),
    act: Some(15.0),
    source: "drafted; registry budget_halt_eta",
};

pub const COST_TURN_RATIO: Threshold = Threshold {
    chart: "cost_per_turn",
    id: "cost_turn_ratio",
    measure: "the last two turns' mean cost over the median of the turns before them",
    unit: "×",
    dir: Dir::Above,
    watch: Some(2.0),
    act: Some(4.0),
    source: "drafted; coach PRD §3.1: the context-size gradient (18× between < 100k and > 600k)",
};

pub const COST_TURN_FLOOR: Threshold = Threshold {
    chart: "cost_per_turn",
    id: "cost_turn_floor",
    measure: "the last two turns' mean cost, which must also pass this for the ratio to count",
    unit: "USD",
    dir: Dir::Above,
    watch: Some(0.50),
    act: Some(2.00),
    source: "coach PRD §3.1: a turn below 100k costs $0.41; > 600k $7.32",
};

pub const CACHE_HIT: Threshold = Threshold {
    chart: "cache",
    id: "cache_hit",
    measure: "share of input read from the cache (registry cache_hit_ratio)",
    unit: "ratio",
    dir: Dir::Below,
    watch: Some(0.80),
    act: Some(0.50),
    source: "the cost body's hit tones (src/dashboard.rs body_cost), so the surfaces agree; coach PRD §3.1: cold calls are 1.8 % of calls and 14.6 % of spend",
};

pub const TOOLS_FAILED: Threshold = Threshold {
    chart: "tool_calls",
    id: "tools_failed",
    measure: "share of the last three turns' calls that failed (denials excluded), from 6 calls",
    unit: "ratio",
    dir: Dir::Above,
    watch: Some(0.15),
    act: Some(0.30),
    source: "drafted; coach PRD §3.2: 40 failing pairs, 29 blind retries (A38)",
};

pub const TOOLS_HOOKS: Threshold = Threshold {
    chart: "tool_calls",
    id: "tools_hooks",
    measure: "hook time per turn, from OpenTelemetry trace spans",
    unit: "ms",
    dir: Dir::Above,
    watch: Some(500.0),
    act: Some(2_000.0),
    source: "mod-pane spec §7 (PRD 7: hook overhead over 500 ms a turn); the act step drafted",
};

pub const TOOLS_REREAD: Threshold = Threshold {
    chart: "tool_calls",
    id: "tools_reread",
    measure: "whole-file reads of one file with no edit between",
    unit: "reads",
    dir: Dir::Above,
    watch: Some(3.0),
    act: None,
    source: "registry file_rereads (⚠ at ≥ 3); coach PRD §5.1 A04",
};

pub const TOOLS_READS_PER_EDIT: Threshold = Threshold {
    chart: "tool_calls",
    id: "tools_reads_per_edit",
    measure: "reads per edit over the session, from 15 reads",
    unit: "×",
    dir: Dir::Above,
    watch: Some(3.0),
    act: None,
    source:
        "drafted; coach PRD §3.1: exploration is 32.5 % of calls and 51 % of context growth (A25)",
};

/// Every threshold, one row each: the table `docs/verdicts.md` is made of.
pub const THRESHOLDS: &[Threshold] = &[
    CTX_TURNS_LEFT,
    CTX_HEADROOM,
    CTX_SIZE,
    CTX_MCP,
    SPEND_RATE,
    SPEND_BUDGET,
    SPEND_HALT,
    COST_TURN_RATIO,
    COST_TURN_FLOOR,
    CACHE_HIT,
    TOOLS_FAILED,
    TOOLS_HOOKS,
    TOOLS_REREAD,
    TOOLS_READS_PER_EDIT,
];

/// Reads of the last three turns count for [`TOOLS_FAILED`] from this many calls.
const MIN_RECENT_CALLS: usize = 6;
/// [`TOOLS_READS_PER_EDIT`] counts from this many reads.
const MIN_READS: usize = 15;

fn verdict(level: Level, text: String, todo: Option<&str>, metric_id: &'static str) -> Verdict {
    Verdict {
        level,
        text,
        todo: todo.map(str::to_string),
        metric_id,
    }
}

fn worst(levels: &[Level]) -> Level {
    levels.iter().copied().max().unwrap_or(Level::Good)
}

/// `29 more turns`, `1 more turn`.
fn turns_word(n: f64) -> String {
    let n = n.ceil().max(1.0) as u64;
    format!("{n} more turn{}", if n == 1 { "" } else { "s" })
}

/// `1M`, `200k`: a window as people name it.
pub fn window_words(n: u64) -> String {
    if n >= 1_000_000 && n % 1_000_000 == 0 {
        format!("{}M", n / 1_000_000)
    } else {
        fmt::tokens(n)
    }
}

fn pct(r: f64) -> String {
    format!("{:.0} %", r * 100.0)
}

// -- context --------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextFacts {
    /// Turns that made an API call.
    pub turns: usize,
    pub size: u64,
    pub window: u64,
    /// The autocompact threshold.
    pub threshold: u64,
    pub turns_left: Option<f64>,
    /// MCP tools in the window, per Claude Code's own `/context` (PRD 3).
    pub mcp_tokens: Option<u64>,
    /// Messages ÷ the used window, per the same capture.
    pub messages_share: Option<f64>,
}

impl ContextFacts {
    pub fn of(state: &State) -> ContextFacts {
        let v = state.context();
        let cap = state.current_context_capture().map(|c| c.0);
        let used = |cats: &[(String, u64)]| cats.iter().map(|(_, t)| *t).sum::<u64>();
        let by = |name: &str| {
            cap.and_then(|c| {
                c.categories
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, t)| *t)
            })
        };
        ContextFacts {
            turns: v.history.len(),
            size: v.size,
            window: v.window,
            threshold: v.threshold,
            turns_left: v.turns_until_compaction(),
            mcp_tokens: cap.map(|_| by("MCP tools").unwrap_or(0)),
            messages_share: cap.and_then(|c| {
                let u = used(&c.categories);
                (u > 0).then(|| by("Messages").unwrap_or(0) as f64 / u as f64)
            }),
        }
    }
}

pub fn context(state: &State) -> Verdict {
    context_from(&ContextFacts::of(state))
}

pub fn context_from(f: &ContextFacts) -> Verdict {
    const ID: &str = "verdict_context";
    if f.turns == 0 || f.size == 0 {
        return verdict(
            Level::Good,
            "Nothing in the window yet: the first turn sets its size.".into(),
            None,
            ID,
        );
    }
    let headroom = f.threshold.saturating_sub(f.size) as f64;
    let by_turns = f
        .turns_left
        .map_or(Level::Good, |n| CTX_TURNS_LEFT.level(n));
    let by_room = CTX_HEADROOM.level(headroom);
    let by_size = CTX_SIZE.level(f.size as f64);
    let by_mcp = f
        .mcp_tokens
        .map_or(Level::Good, |t| CTX_MCP.level(t as f64));
    let level = worst(&[by_turns, by_room, by_size, by_mcp]);
    let size = fmt::tokens(f.size);
    let window = window_words(f.window);
    match level {
        Level::Act => {
            let text = if f.size >= f.threshold {
                format!("{size} of {window}: past the point where Claude Code compacts on its own.")
            } else {
                match f.turns_left {
                    Some(n) => format!(
                        "{size} of {window}: Claude Code compacts on its own in about {}.",
                        turns_word(n).replace(" more", "")
                    ),
                    None => format!(
                        "{size} of {window}: inside the last {} before Claude Code compacts on its own.",
                        fmt::tokens(autocompact::WARN_TOKENS)
                    ),
                }
            };
            verdict(
                level,
                text,
                Some("Do: /compact now with a focus, or write a hand-off note and /clear."),
                ID,
            )
        }
        Level::Watch => {
            if by_turns == Level::Watch {
                let n = f.turns_left.unwrap_or(0.0);
                verdict(
                    level,
                    format!(
                        "About {} before Claude Code compacts on its own.",
                        turns_word(n)
                    ),
                    Some("Do: /compact at the next break, not mid-task."),
                    ID,
                )
            } else if by_size == Level::Watch {
                verdict(
                    level,
                    format!("{size} in the window: every call re-reads all of it."),
                    Some("Do: when this task is done, write a hand-off note and /clear."),
                    ID,
                )
            } else {
                let mcp = f.mcp_tokens.unwrap_or(0);
                verdict(
                    level,
                    format!(
                        "MCP tools take {} ({}) of every request.",
                        fmt::tokens(mcp),
                        pct(mcp as f64 / f.window.max(1) as f64)
                    ),
                    Some("Do: turn off the MCP servers this task does not use (/mcp)."),
                    ID,
                )
            }
        }
        Level::Good => {
            let mut text = match f.turns_left {
                Some(n) => format!(
                    "Plenty of room: about {} before Claude Code compacts on its own.",
                    turns_word(n)
                ),
                None => format!(
                    "Plenty of room: {} free.",
                    fmt::tokens(f.window.saturating_sub(f.size))
                ),
            };
            if let Some(mcp) = f.mcp_tokens.filter(|t| *t > 0) {
                text.push_str(&format!(
                    " MCP tools take {} ({}), which is fine.",
                    fmt::tokens(mcp),
                    pct(mcp as f64 / f.window.max(1) as f64)
                ));
            }
            if f.messages_share.is_some_and(|s| s >= 0.8) {
                text.push_str(" Nearly all of it is your conversation.");
            }
            verdict(
                level,
                text,
                Some("Do: /compact when you finish this task, not mid-way."),
                ID,
            )
        }
    }
}

// -- spend ----------------------------------------------------------------

/// What a jump in spend was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JumpKind {
    /// A subagent launched in the turn.
    Agent,
    /// The cache was rebuilt in the turn.
    Rebuild,
    /// Neither: many calls, or a large context re-sent.
    Turn,
}

impl JumpKind {
    pub fn words(self) -> &'static str {
        match self {
            JumpKind::Agent => "a helper agent",
            JumpKind::Rebuild => "a cache rebuild",
            JumpKind::Turn => "a costly turn",
        }
    }
}

/// A turn that cost at least twice the typical turn (and $0.50).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jump {
    pub turn: usize,
    pub usd: f64,
    pub kind: JumpKind,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendFacts {
    /// The session's spend so far (`cost_combined`), `None` without a price.
    pub total: Option<f64>,
    /// Over the last 15 minutes.
    pub per_min: Option<f64>,
    pub jumps: Vec<Jump>,
    /// `(used, total)` of a `--max-budget-usd` budget (PRD 4).
    pub budget: Option<(f64, f64)>,
}

impl SpendFacts {
    pub fn of(state: &State) -> SpendFacts {
        let rates = crate::metrics::cost::rates(&state.agg, state.cost.pricing(), state.clock_ms());
        SpendFacts {
            total: state.cost_combined().map(|c| c.usd),
            per_min: rates.usd_per_hour.map(|h| h / 60.0),
            jumps: spend_jumps(state),
            budget: state.budget_usd.map(|(used, total, _)| (used, total)),
        }
    }

    /// Minutes until the budget halts the session at the current rate.
    pub fn halt_min(&self) -> Option<f64> {
        let (used, total) = self.budget?;
        let rate = self.per_min.filter(|r| *r > 0.0)?;
        Some((total - used).max(0.0) / rate)
    }
}

pub fn spend(state: &State) -> Verdict {
    spend_from(&SpendFacts::of(state))
}

/// `Two jumps: a helper agent in turn 3 ($1.85) and a cache rebuild in turn 11 ($2.31).`
fn jumps_sentence(jumps: &[Jump]) -> String {
    let shown: Vec<String> = jumps
        .iter()
        .rev()
        .take(2)
        .rev()
        .map(|j| {
            format!(
                "{} in turn {} ({})",
                j.kind.words(),
                j.turn,
                fmt::usd(j.usd)
            )
        })
        .collect();
    match (jumps.len(), shown.as_slice()) {
        (0, _) => String::new(),
        (1, [one]) => format!(" One jump: {one}."),
        (2, [a, b]) => format!(" Two jumps: {a} and {b}."),
        (n, [a, b]) => format!(" {n} jumps, the last two {a} and {b}."),
        _ => String::new(),
    }
}

pub fn spend_from(f: &SpendFacts) -> Verdict {
    const ID: &str = "verdict_spend";
    let Some(total) = f.total else {
        return verdict(
            Level::Good,
            "No price for this model, so spend is not known.".into(),
            None,
            ID,
        );
    };
    if total <= 0.0 {
        return verdict(Level::Good, "Nothing spent yet.".into(), None, ID);
    }
    let by_rate = f.per_min.map_or(Level::Good, |r| SPEND_RATE.level(r));
    let by_budget = f
        .budget
        .filter(|(_, t)| *t > 0.0)
        .map_or(Level::Good, |(u, t)| SPEND_BUDGET.level(u / t));
    let by_halt = f.halt_min().map_or(Level::Good, |m| SPEND_HALT.level(m));
    let level = worst(&[by_rate, by_budget, by_halt]);
    let rate = match f.per_min {
        Some(r) => format!("{} a minute.", fmt::usd(r)),
        None => format!(
            "{} so far, nothing in the last 15 minutes.",
            fmt::usd(total)
        ),
    };
    let jumps = jumps_sentence(&f.jumps);
    if by_budget.max(by_halt) == level && level != Level::Good {
        let (used, budget) = f.budget.unwrap_or_default();
        let halt = f
            .halt_min()
            .map(|m| format!(" At this rate it halts in about {:.0} minutes.", m.ceil()))
            .unwrap_or_default();
        return verdict(
            level,
            format!(
                "{} of the {} budget used.{halt}",
                fmt::usd(used),
                fmt::usd(budget)
            ),
            Some(if level == Level::Act {
                "Do: finish the current step now, or start again with a larger --max-budget-usd."
            } else {
                "Do: keep the remaining steps small; the budget stops the session when it runs out."
            }),
            ID,
        );
    }
    let todo = match level {
        Level::Act => {
            "Do: stop and /compact, or hand off and /clear; every turn re-reads the whole context."
        }
        Level::Watch => {
            "Do: /clear between unrelated tasks; a smaller context makes every turn cheaper."
        }
        Level::Good => "Do: /clear between unrelated tasks to keep it that way.",
    };
    verdict(level, format!("{rate}{jumps}"), Some(todo), ID)
}

// -- cost per turn --------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostTurnFacts {
    /// `(turn, USD)` of the turns that made an API call, priced, in order
    /// (the turn's own calls and the agents it launched).
    pub costs: Vec<(usize, f64)>,
    /// The context every turn now re-sends.
    pub context: u64,
    /// Requests passed 200k tokens (the status line's flag, PRD 2).
    pub over_200k: bool,
}

impl CostTurnFacts {
    pub fn of(state: &State) -> CostTurnFacts {
        CostTurnFacts {
            costs: turn_costs(state),
            context: state.context().size,
            over_200k: state.status_facts.exceeds_200k_tokens,
        }
    }

    /// The median turn: what the figure calls typical.
    pub fn typical(&self) -> Option<f64> {
        median(self.costs.iter().map(|(_, c)| *c).collect())
    }
}

pub fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

pub fn cost_per_turn(state: &State) -> Verdict {
    cost_per_turn_from(&CostTurnFacts::of(state))
}

pub fn cost_per_turn_from(f: &CostTurnFacts) -> Verdict {
    const ID: &str = "verdict_cost_per_turn";
    let todo_good =
        "Do: keep unrelated tasks apart with /clear; turns get dearer as the context grows.";
    match f.costs.as_slice() {
        [] => return verdict(Level::Good, "No priced turn yet.".into(), None, ID),
        [(_, c)] => {
            return verdict(
                Level::Good,
                format!("One turn so far, at {}.", fmt::usd(*c)),
                Some(todo_good),
                ID,
            )
        }
        _ => {}
    }
    let n = f.costs.len();
    let recent: Vec<f64> = f.costs[n.saturating_sub(2)..]
        .iter()
        .map(|(_, c)| *c)
        .collect();
    let before: Vec<f64> = f.costs[..n.saturating_sub(2)]
        .iter()
        .map(|(_, c)| *c)
        .collect();
    let mean = recent.iter().sum::<f64>() / recent.len() as f64;
    let ratio = median(before).filter(|t| *t > 0.0).map(|t| mean / t);
    let level = ratio.map_or(Level::Good, |r| {
        COST_TURN_RATIO.level(r).min(COST_TURN_FLOOR.level(mean))
    });
    let typical = f.typical().unwrap_or(0.0);
    let last_two = format!(
        "The last two turns cost {} and {}",
        fmt::usd(recent[0]),
        fmt::usd(recent[recent.len() - 1])
    );
    let long = if f.over_200k {
        " Requests are past 200k tokens."
    } else {
        ""
    };
    match level {
        Level::Act => verdict(
            level,
            format!(
                "{last_two}, {:.0}× the turns before: every turn now re-sends {}.{long}",
                ratio.unwrap_or(0.0),
                fmt::tokens(f.context)
            ),
            Some("Do: /compact now, or write a hand-off note and /clear."),
            ID,
        ),
        Level::Watch => verdict(
            level,
            format!(
                "{last_two}: every turn now re-sends {}.{long}",
                fmt::tokens(f.context)
            ),
            Some("Do: /compact at the next break."),
            ID,
        ),
        Level::Good => verdict(
            level,
            format!("Turns cost about {} each.{long}", fmt::usd(typical)),
            Some(todo_good),
            ID,
        ),
    }
}

// -- cache ----------------------------------------------------------------

/// A located cache miss (PRD 1), as the verdict and `prevent[]` need it.
#[derive(Debug, Clone, PartialEq)]
pub struct Miss {
    pub turn: usize,
    /// `LocatedCause::kind()`.
    pub kind: &'static str,
    /// The changed parameter of a `params_changed` miss (`effort`, `model`).
    pub field: Option<&'static str>,
    pub tokens: u64,
}

impl Miss {
    /// The habit that avoids this rebuild, for the four causes the person
    /// can prevent; `None` for the rest.
    pub fn habit(&self) -> Option<&'static str> {
        habit(self.kind, self.field)
    }
}

/// Located cause → the habit that avoids it. Fixed sentences.
pub fn habit(kind: &str, field: Option<&str>) -> Option<&'static str> {
    Some(match (kind, field) {
        ("tools_changed", _) => "Connect the MCP servers you need before your first prompt.",
        ("prefix_rewritten", _) => {
            "Steer forward with a new message instead of editing or rewinding an old one."
        }
        ("params_changed", Some("effort")) => {
            "Pick the effort level at the start, or change it right after /clear."
        }
        ("params_changed", Some("model")) => {
            "Pick the model at the start, or switch right after /clear."
        }
        ("expired", _) => "Reply before the cache expires, or /clear before a long break.",
        _ => return None,
    })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheFacts {
    /// Turns that made an API call: the first writes the cache, so a hit
    /// ratio says something from the second on.
    pub turns: usize,
    pub hit: Option<f64>,
    pub misses: Vec<Miss>,
    /// What the avoidable rebuilds cost at the cache-write price.
    pub avoidable_usd: Option<f64>,
    /// `5 minutes` or `1 hour`.
    pub ttl_words: &'static str,
}

impl CacheFacts {
    pub fn of(state: &State) -> CacheFacts {
        let misses: Vec<Miss> = state
            .requests
            .misses()
            .iter()
            .filter_map(|m| {
                let field = match &m.cause {
                    crate::requests::LocatedCause::ParamsChanged { field, .. } => Some(*field),
                    _ => None,
                };
                Some(Miss {
                    turn: m.turn?,
                    kind: m.cause.kind(),
                    field,
                    tokens: m.tokens,
                })
            })
            .collect();
        let avoidable: u64 = misses
            .iter()
            .filter(|m| m.habit().is_some())
            .map(|m| m.tokens)
            .sum();
        CacheFacts {
            turns: state.agg.turns.iter().filter(|t| t.api_calls > 0).count(),
            hit: state
                .cache
                .hit_ratio
                .or_else(|| state.agg.total.cache_hit_ratio()),
            avoidable_usd: crate::advisor::rules::usd(
                state,
                avoidable,
                crate::advisor::rules::PriceKind::CacheWrite,
            ),
            misses,
            ttl_words: match state.cache_ttl_ms() {
                3_600_000 => "1 hour",
                _ => "5 minutes",
            },
        }
    }
}

pub fn cache(state: &State) -> Verdict {
    cache_from(&CacheFacts::of(state))
}

pub fn cache_from(f: &CacheFacts) -> Verdict {
    const ID: &str = "verdict_cache";
    let Some(hit) = f.hit.filter(|_| f.turns > 1) else {
        return verdict(
            Level::Good,
            "The first turn writes the cache; nothing to reuse yet.".into(),
            None,
            ID,
        );
    };
    let level = CACHE_HIT.level(hit);
    let avoidable: Vec<&Miss> = f.misses.iter().filter(|m| m.habit().is_some()).collect();
    let tokens: u64 = avoidable.iter().map(|m| m.tokens).sum();
    let price = f
        .avoidable_usd
        .map(|u| format!(", about {}", fmt::usd(u)))
        .unwrap_or_default();
    let good = level == Level::Good;
    let but = if good { "But " } else { "" };
    let all = if good { "all" } else { "All" };
    let the = if good { "the" } else { "The" };
    let rebuilds = match (avoidable.len(), f.misses.len()) {
        (0, 0) => String::new(),
        (0, 1) => " One rebuild, with no habit that avoids it.".into(),
        (0, n) => format!(" {n} rebuilds, none with a habit that avoids it."),
        (1, 1) => format!(
            " {but}{the} one rebuild ({} tokens{price}) was avoidable. How, below.",
            fmt::tokens(tokens)
        ),
        (k, n) if k == n => format!(
            " {but}{all} {n} rebuilds ({} tokens{price}) were avoidable. How, below.",
            fmt::tokens(tokens)
        ),
        (k, n) => format!(
            " {but}{k} of {n} rebuilds ({} tokens{price}) were avoidable. How, below.",
            fmt::tokens(tokens)
        ),
    };
    // The habit of the costliest avoidable cause, else keeping it warm.
    let top = avoidable
        .iter()
        .max_by_key(|m| m.tokens)
        .and_then(|m| m.habit());
    let warm = format!("Do: reply within {} so the cache stays warm.", f.ttl_words);
    let todo = top
        .map(|h| format!("Do: {}", lower_first(h)))
        .unwrap_or(warm);
    let text = if good {
        format!("Reused {}.{rebuilds}", pct(hit))
    } else {
        format!("Only {} reused.{rebuilds}", pct(hit))
    };
    Verdict {
        level,
        text,
        todo: Some(todo),
        metric_id: ID,
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

// -- tool calls -----------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolFacts {
    pub calls: usize,
    pub failed: usize,
    /// Whole-file reads (Read, and Bash readers) and edits (Edit, Write).
    pub reads: usize,
    pub edits: usize,
    /// The most re-read file: basename, whole reads with no edit between,
    /// the tokens its reads put in the window.
    pub reread: Option<(String, usize, u64)>,
    /// Calls and failures (denials excluded) of the last three turns.
    pub recent_calls: usize,
    pub recent_failed: usize,
    /// Mean hook time per turn with hooks (PRD 7; trace spans only).
    pub hook_ms_per_turn: Option<u64>,
}

impl ToolFacts {
    pub fn of(state: &State) -> ToolFacts {
        let calls = &state.tools.calls;
        let failed = |c: &crate::tools::Call| c.is_error && c.denial.is_none();
        let last_turns: Vec<usize> = {
            let mut t: Vec<usize> = calls.iter().map(|c| c.turn).collect();
            t.sort_unstable();
            t.dedup();
            t.into_iter().rev().take(3).collect()
        };
        let recent: Vec<&crate::tools::Call> = calls
            .iter()
            .filter(|c| last_turns.contains(&c.turn))
            .collect();
        let reread = state
            .files
            .files
            .values()
            .filter(|f| f.reads_since_edit > 0)
            .max_by_key(|f| (f.reads_since_edit, std::cmp::Reverse(f.path.clone())))
            .map(|f| {
                let tokens: u64 = calls
                    .iter()
                    .filter(|c| c.path.as_deref() == Some(f.path.as_str()) && !c.cleared)
                    .filter(|c| c.class == crate::phase::ToolClass::Explore)
                    .map(|c| c.result_tokens_est)
                    .sum();
                (basename(&f.path), f.reads_since_edit, tokens)
            });
        let hooks = state.hook_ms_by_turn();
        let hook_total: u64 = hooks
            .iter()
            .flat_map(|(_, by, _)| by.values())
            .fold(0, |a: u64, b| a.saturating_add(*b));
        ToolFacts {
            calls: calls.len(),
            failed: calls.iter().filter(|c| failed(c)).count(),
            reads: state
                .files
                .files
                .values()
                .map(|f| f.reads.saturating_sub(f.cheap_reads))
                .sum(),
            edits: state.files.files.values().map(|f| f.edits + f.writes).sum(),
            reread,
            recent_calls: recent.len(),
            recent_failed: recent.iter().filter(|c| failed(c)).count(),
            hook_ms_per_turn: (!hooks.is_empty()).then(|| hook_total / hooks.len() as u64),
        }
    }
}

/// A path's last component: the structural name the words may carry,
/// cut at 40 cells.
pub fn basename(path: &str) -> String {
    let b = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    fmt::clip(&b, 40)
}

pub fn tool_calls(state: &State) -> Verdict {
    tool_calls_from(&ToolFacts::of(state))
}

pub fn tool_calls_from(f: &ToolFacts) -> Verdict {
    const ID: &str = "verdict_tool_calls";
    if f.calls == 0 {
        return verdict(Level::Good, "No tool calls yet.".into(), None, ID);
    }
    let by_failed = if f.recent_calls >= MIN_RECENT_CALLS {
        TOOLS_FAILED.level(f.recent_failed as f64 / f.recent_calls as f64)
    } else {
        Level::Good
    };
    let by_hooks = f
        .hook_ms_per_turn
        .map_or(Level::Good, |ms| TOOLS_HOOKS.level(ms as f64));
    let by_reread = f
        .reread
        .as_ref()
        .map_or(Level::Good, |(_, n, _)| TOOLS_REREAD.level(*n as f64));
    let by_ratio = if f.reads >= MIN_READS {
        TOOLS_READS_PER_EDIT.level(f.reads as f64 / f.edits.max(1) as f64)
    } else {
        Level::Good
    };
    let level = worst(&[by_failed, by_hooks, by_reread, by_ratio]);
    if level == Level::Good {
        return verdict(
            level,
            format!(
                "{} {}, {} failed.",
                f.calls,
                if f.calls == 1 { "call" } else { "calls" },
                f.failed
            ),
            Some("Do: ask Claude to search (grep) before it reads whole files."),
            ID,
        );
    }
    if by_failed == level {
        return verdict(
            level,
            format!(
                "{} of the last {} calls failed.",
                f.recent_failed, f.recent_calls
            ),
            Some("Don't: let it retry blind; give Claude the missing fact, or run the command yourself."),
            ID,
        );
    }
    if by_hooks == level {
        return verdict(
            level,
            format!(
                "Hooks add {} to every turn.",
                fmt::added_s(f.hook_ms_per_turn.unwrap_or(0))
            ),
            Some("Do: make slow hooks async, or narrow their matcher."),
            ID,
        );
    }
    let mut text = format!(
        "{} {} for {} {}",
        f.reads,
        if f.reads == 1 { "read" } else { "reads" },
        f.edits,
        if f.edits == 1 { "edit" } else { "edits" }
    );
    if let Some((name, n, tokens)) = f.reread.as_ref().filter(|_| by_reread == level) {
        text.push_str(&format!(
            "; {name} was read whole {n} times ({}).",
            fmt::tokens(*tokens)
        ));
    } else {
        text.push('.');
    }
    verdict(
        level,
        text,
        Some("Do: ask Claude to search (grep) before it reads whole files."),
        ID,
    )
}

// -- shared ---------------------------------------------------------------

/// USD of the agents each turn launched, priced.
pub fn agent_costs(state: &State) -> std::collections::BTreeMap<usize, f64> {
    let mut agents: std::collections::BTreeMap<usize, f64> = Default::default();
    for a in state.agents.values() {
        let (Some(turn), Some(usd)) = (
            a.launched_turn,
            state.cost.pricing().estimate(&a.usage, &a.model),
        ) else {
            continue;
        };
        *agents.entry(turn).or_default() += usd;
    }
    agents
}

/// `(turn, USD)` for every turn that made an API call: its own calls priced
/// as the ledger prices them (at its first model) plus the agents it
/// launched. Unpriced turns are left out.
pub fn turn_costs(state: &State) -> Vec<(usize, f64)> {
    let agents = agent_costs(state);
    state
        .agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .filter_map(|t| {
            let own = state.cost.pricing().estimate(&t.usage, t.models.first()?)?;
            Some((
                t.number,
                own + agents.get(&t.number).copied().unwrap_or(0.0),
            ))
        })
        .collect()
}

/// The turns among `costs` that cost at least [`COST_TURN_RATIO`]'s watch
/// times the median and [`COST_TURN_FLOOR`]'s watch, unexplained for now
/// ([`JumpKind::Turn`]); [`spend_jumps`] names them.
fn jumps(costs: &[(usize, f64)]) -> Vec<Jump> {
    let Some(typ) = median(costs.iter().map(|(_, c)| *c).collect()) else {
        return Vec::new();
    };
    let ratio = COST_TURN_RATIO.watch.unwrap_or(2.0);
    let floor = COST_TURN_FLOOR.watch.unwrap_or(0.5);
    costs
        .iter()
        .filter(|(_, c)| costs.len() >= 3 && *c >= typ * ratio && *c >= floor)
        .map(|(turn, usd)| Jump {
            turn: *turn,
            usd: *usd,
            kind: JumpKind::Turn,
        })
        .collect()
}

/// The jumps of the session, each named by what cost the most in its
/// turn: the agents it launched (priced) or its located cache rebuilds (at
/// the cache-write price), else a costly turn.
pub fn spend_jumps(state: &State) -> Vec<Jump> {
    let mut v = jumps(&turn_costs(state));
    for j in &mut v {
        let agents: f64 = state
            .agents
            .values()
            .filter(|a| a.launched_turn == Some(j.turn))
            .filter_map(|a| state.cost.pricing().estimate(&a.usage, &a.model))
            .sum();
        let rebuilt: u64 = state
            .requests
            .misses()
            .iter()
            .filter(|m| m.turn == Some(j.turn))
            .map(|m| m.tokens)
            .sum();
        let rebuild = crate::advisor::rules::usd(
            state,
            rebuilt,
            crate::advisor::rules::PriceKind::CacheWrite,
        )
        .unwrap_or(0.0);
        j.kind = if agents > 0.0 && agents >= rebuild {
            JumpKind::Agent
        } else if rebuilt > 0 {
            JumpKind::Rebuild
        } else {
            JumpKind::Turn
        };
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_are_ordered_and_sourced() {
        let mut ids = std::collections::HashSet::new();
        for t in THRESHOLDS {
            assert!(ids.insert(t.id), "duplicate {}", t.id);
            assert!(!t.source.is_empty() && !t.measure.is_empty(), "{}", t.id);
            assert!(t.watch.is_some() || t.act.is_some(), "{}", t.id);
            if let (Some(w), Some(a)) = (t.watch, t.act) {
                match t.dir {
                    Dir::Above => assert!(w <= a, "{}", t.id),
                    Dir::Below => assert!(w >= a, "{}", t.id),
                }
            }
            assert!(
                ["context", "spend", "cost_per_turn", "cache", "tool_calls"].contains(&t.chart),
                "{}",
                t.id
            );
        }
    }

    /// The registry's verdict entries name every threshold of their chart.
    #[test]
    fn the_registry_names_every_threshold() {
        for t in THRESHOLDS {
            let m = crate::metrics::registry::get(&format!("verdict_{}", t.chart))
                .unwrap_or_else(|| panic!("verdict_{} not registered", t.chart));
            assert!(m.formula.contains(t.id), "{} lacks {}", m.id, t.id);
        }
    }

    // -- context: one test per level

    fn ctx() -> ContextFacts {
        ContextFacts {
            turns: 14,
            size: 412_000,
            window: 1_000_000,
            threshold: 967_000,
            turns_left: Some(29.2),
            mcp_tokens: Some(10_000),
            messages_share: Some(0.9),
        }
    }

    #[test]
    fn context_good() {
        let mut f = ctx();
        f.size = 212_000;
        let v = context_from(&f);
        assert_eq!(v.level, Level::Good);
        assert_eq!(
            v.text,
            "Plenty of room: about 30 more turns before Claude Code compacts on its own. MCP tools take 10k (1 %), which is fine. Nearly all of it is your conversation."
        );
        assert_eq!(
            v.todo.as_deref(),
            Some("Do: /compact when you finish this task, not mid-way.")
        );
    }

    #[test]
    fn context_watch() {
        // 412k is past the 300k re-read mark.
        let v = context_from(&ctx());
        assert_eq!(v.level, Level::Watch);
        assert_eq!(v.text, "412k in the window: every call re-reads all of it.");
        // Few turns left wins the sentence.
        let mut f = ctx();
        f.turns_left = Some(9.0);
        let v = context_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert!(v.text.starts_with("About 9 more turns"), "{}", v.text);
        // A heavy MCP share alone.
        let mut f = ctx();
        f.size = 100_000;
        f.mcp_tokens = Some(40_000);
        let v = context_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert!(v.text.starts_with("MCP tools take 40k"), "{}", v.text);
        assert!(v.todo.unwrap().contains("/mcp"));
    }

    #[test]
    fn context_act() {
        let mut f = ctx();
        f.turns_left = Some(3.0);
        let v = context_from(&f);
        assert_eq!(v.level, Level::Act);
        assert_eq!(
            v.text,
            "412k of 1M: Claude Code compacts on its own in about 3 turns."
        );
        // Inside Claude Code's own warn band, with no velocity.
        let mut f = ctx();
        f.size = 950_000;
        f.turns_left = None;
        let v = context_from(&f);
        assert_eq!(v.level, Level::Act);
        assert!(v.text.contains("inside the last 20k"), "{}", v.text);
    }

    // -- spend

    fn spend_f() -> SpendFacts {
        SpendFacts {
            total: Some(14.82),
            per_min: Some(0.12),
            jumps: vec![],
            budget: None,
        }
    }

    #[test]
    fn spend_good() {
        let v = spend_from(&spend_f());
        assert_eq!(v.level, Level::Good);
        assert_eq!(v.text, "$0.12 a minute.");
    }

    #[test]
    fn spend_watch() {
        let mut f = spend_f();
        f.per_min = Some(0.36);
        f.jumps = vec![
            Jump {
                turn: 3,
                usd: 1.85,
                kind: JumpKind::Agent,
            },
            Jump {
                turn: 11,
                usd: 2.31,
                kind: JumpKind::Rebuild,
            },
        ];
        let v = spend_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert_eq!(
            v.text,
            "$0.36 a minute. Two jumps: a helper agent in turn 3 ($1.85) and a cache rebuild in turn 11 ($2.31)."
        );
        // A budget three quarters used.
        let mut f = spend_f();
        f.budget = Some((8.0, 10.0));
        let v = spend_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert!(
            v.text.starts_with("$8.00 of the $10.0 budget used."),
            "{}",
            v.text
        );
    }

    #[test]
    fn spend_act() {
        let mut f = spend_f();
        f.per_min = Some(1.5);
        assert_eq!(spend_from(&f).level, Level::Act);
        // The budget halts within 15 minutes.
        let mut f = spend_f();
        f.per_min = Some(0.25);
        f.budget = Some((5.0, 8.0));
        let v = spend_from(&f);
        assert_eq!(v.level, Level::Act);
        assert!(v.text.contains("halts in about 12 minutes"), "{}", v.text);
    }

    // -- cost per turn

    fn costs(v: &[f64]) -> CostTurnFacts {
        CostTurnFacts {
            costs: v.iter().enumerate().map(|(i, c)| (i + 1, *c)).collect(),
            context: 412_000,
            over_200k: false,
        }
    }

    #[test]
    fn cost_per_turn_good() {
        let v = cost_per_turn_from(&costs(&[0.5, 0.6, 0.55, 0.6, 0.65]));
        assert_eq!(v.level, Level::Good);
        assert_eq!(v.text, "Turns cost about $0.60 each.");
    }

    #[test]
    fn cost_per_turn_watch() {
        let v = cost_per_turn_from(&costs(&[0.5, 0.6, 0.55, 0.6, 1.74, 2.21]));
        assert_eq!(v.level, Level::Watch);
        assert_eq!(
            v.text,
            "The last two turns cost $1.74 and $2.21: every turn now re-sends 412k."
        );
        assert_eq!(v.todo.as_deref(), Some("Do: /compact at the next break."));
        // Twice a cheap turn is not dear: the floor holds it at Good.
        let v = cost_per_turn_from(&costs(&[0.05, 0.05, 0.05, 0.2, 0.2]));
        assert_eq!(v.level, Level::Good);
    }

    #[test]
    fn cost_per_turn_act() {
        let mut f = costs(&[0.5, 0.6, 0.55, 0.6, 3.1, 3.4]);
        f.over_200k = true;
        let v = cost_per_turn_from(&f);
        assert_eq!(v.level, Level::Act);
        assert!(v.text.contains("6× the turns before"), "{}", v.text);
        assert!(
            v.text.ends_with("Requests are past 200k tokens."),
            "{}",
            v.text
        );
    }

    // -- cache

    fn cache_f(hit: f64, misses: Vec<Miss>) -> CacheFacts {
        CacheFacts {
            turns: 14,
            hit: Some(hit),
            misses,
            avoidable_usd: Some(2.3),
            ttl_words: "1 hour",
        }
    }

    fn miss(turn: usize, kind: &'static str, field: Option<&'static str>, tokens: u64) -> Miss {
        Miss {
            turn,
            kind,
            field,
            tokens,
        }
    }

    #[test]
    fn cache_good() {
        let v = cache_f(
            0.94,
            vec![
                miss(4, "prefix_rewritten", None, 64_000),
                miss(9, "params_changed", Some("effort"), 188_000),
                miss(11, "tools_changed", None, 112_000),
            ],
        );
        let v = cache_from(&v);
        assert_eq!(v.level, Level::Good);
        assert_eq!(
            v.text,
            "Reused 94 %. But all 3 rebuilds (364k tokens, about $2.30) were avoidable. How, below."
        );
        assert_eq!(
            v.todo.as_deref(),
            Some("Do: pick the effort level at the start, or change it right after /clear.")
        );
        // No misses: keep it warm.
        let v = cache_from(&cache_f(0.97, vec![]));
        assert_eq!(v.text, "Reused 97 %.");
        assert_eq!(
            v.todo.as_deref(),
            Some("Do: reply within 1 hour so the cache stays warm.")
        );
    }

    #[test]
    fn cache_watch() {
        let v = cache_from(&cache_f(0.7, vec![miss(6, "thinking_drop", None, 50_000)]));
        assert_eq!(v.level, Level::Watch);
        assert_eq!(
            v.text,
            "Only 70 % reused. One rebuild, with no habit that avoids it."
        );
    }

    #[test]
    fn cache_act() {
        let v = cache_from(&cache_f(0.4, vec![miss(6, "expired", None, 150_000)]));
        assert_eq!(v.level, Level::Act);
        assert!(
            v.text.starts_with("Only 40 % reused. The one rebuild"),
            "{}",
            v.text
        );
        assert_eq!(
            v.todo.as_deref(),
            Some("Do: reply before the cache expires, or /clear before a long break.")
        );
    }

    #[test]
    fn every_avoidable_cause_has_a_habit() {
        for (kind, field) in [
            ("tools_changed", None),
            ("prefix_rewritten", None),
            ("params_changed", Some("effort")),
            ("params_changed", Some("model")),
            ("expired", None),
        ] {
            assert!(habit(kind, field).is_some(), "{kind} {field:?}");
        }
        for kind in ["system_changed", "thinking_drop", "unlocated"] {
            assert!(habit(kind, None).is_none(), "{kind}");
        }
        assert!(habit("params_changed", Some("betas")).is_none());
    }

    // -- tool calls

    fn tools_f() -> ToolFacts {
        ToolFacts {
            calls: 40,
            failed: 1,
            reads: 10,
            edits: 6,
            reread: Some(("state.rs".into(), 1, 9_000)),
            recent_calls: 12,
            recent_failed: 1,
            hook_ms_per_turn: None,
        }
    }

    #[test]
    fn tool_calls_good() {
        let v = tool_calls_from(&tools_f());
        assert_eq!(v.level, Level::Good);
        assert_eq!(v.text, "40 calls, 1 failed.");
    }

    #[test]
    fn tool_calls_watch() {
        let mut f = tools_f();
        f.reads = 46;
        f.edits = 18;
        f.reread = Some(("state.rs".into(), 3, 41_000));
        let v = tool_calls_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert_eq!(
            v.text,
            "46 reads for 18 edits; state.rs was read whole 3 times (41k)."
        );
        // Hook overhead over 500 ms a turn (PRD 7).
        let mut f = tools_f();
        f.hook_ms_per_turn = Some(800);
        let v = tool_calls_from(&f);
        assert_eq!(v.level, Level::Watch);
        assert_eq!(v.text, "Hooks add 0.8 s to every turn.");
    }

    #[test]
    fn tool_calls_act() {
        let mut f = tools_f();
        f.recent_failed = 5;
        let v = tool_calls_from(&f);
        assert_eq!(v.level, Level::Act);
        assert_eq!(v.text, "5 of the last 12 calls failed.");
        assert!(v.todo.unwrap().starts_with("Don't:"));
    }

    /// Review focus 1: nothing alarming before anything happened.
    #[test]
    fn empty_facts_are_good_and_quiet() {
        for v in [
            context_from(&ContextFacts::default()),
            spend_from(&SpendFacts {
                total: Some(0.0),
                ..Default::default()
            }),
            spend_from(&SpendFacts::default()),
            cost_per_turn_from(&CostTurnFacts::default()),
            cache_from(&CacheFacts::default()),
            tool_calls_from(&ToolFacts::default()),
        ] {
            assert_eq!(v.level, Level::Good, "{v:?}");
            assert!(v.todo.is_none(), "{v:?}");
            assert!(!v.text.contains("NaN") && !v.text.contains("inf"), "{v:?}");
        }
    }
}
