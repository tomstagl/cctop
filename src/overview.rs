//! The overview object (mod-pane US-002): one entry per chart — context,
//! spend, cost per turn, cache, tool calls — each with its title, figure,
//! verdict, a chart spec (data, never geometry), the readouts beside it and
//! its legend. The TUI and the pane draw it verbatim; `cctop query overview`
//! prints it.

pub mod spec;

use serde::Serialize;

pub use spec::{Axis, Cell, ChartSpec, Forecast, Kind, Marker, Point, Role, Series, Threshold};

use std::collections::BTreeMap;

use crate::advisor::rules::{ident, spoken};
use crate::metrics::cost::parse_ts_ms;
use crate::metrics::Turn;
use crate::requests::LocatedCause;
use crate::ui::fmt;
use crate::ui::State;
use crate::verdict::{self, money, Verdict};

/// Per-turn charts show this many turns by default.
pub const DEFAULT_MAX_TURNS: usize = 50;
/// Context and spend cover the whole session in at most this many points.
pub const BUCKETS: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Overview {
    pub schema: u32,
    pub charts: Vec<Chart>,
    pub footer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chart {
    pub id: &'static str,
    pub title: &'static str,
    pub figure: String,
    pub verdict: Verdict,
    /// Where the figure was read and how long ago (`per Claude Code, 12s
    /// ago`), or the cache's countdown in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `last N turns` when a per-turn chart shows only the latest turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub spec: ChartSpec,
    /// A one-row bar drawn with the chart: context's composition
    /// (`stacked_bar`), the cache's countdown (`progress`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar: Option<ChartSpec>,
    pub readouts: Vec<Readout>,
    pub legend: Vec<Legend>,
    /// The cache chart's "How to avoid each rebuild": one row per located
    /// rebuild of the turns shown that a habit avoids.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prevent: Vec<Prevent>,
    /// The cells drawn from `spec` at its default size, for a surface that
    /// cannot run the renderer; filled once the renderer exists.
    pub fallback_cells: Vec<Cell>,
}

/// What hovering (or selecting) a point says, at the point's `x`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Readout {
    pub x: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Prevent {
    pub turn: usize,
    /// The located cause's kind (`tools_changed`, `prefix_rewritten`,
    /// `params_changed`, `expired`) and, for a parameter, which one.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<&'static str>,
    /// What changed, in words.
    pub cause: String,
    pub tokens: u64,
    pub habit: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Legend {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub max_turns: usize,
    /// The chain's cost (this session and the ones it came from), PRD 4.
    pub chain_usd: Option<f64>,
}

pub fn build(state: &State) -> Overview {
    build_with(
        state,
        &Options {
            max_turns: DEFAULT_MAX_TURNS,
            chain_usd: None,
        },
    )
}

pub fn build_with(state: &State, opts: &Options) -> Overview {
    let max_turns = opts.max_turns.max(1);
    let all: Vec<&Turn> = state.agg.turns.iter().collect();
    let shown = &all[all.len().saturating_sub(max_turns)..];
    let label = (all.len() > shown.len()).then(|| match shown.len() {
        1 => "last 1 turn".to_string(),
        n => format!("last {n} turns"),
    });
    let per_turn = PerTurn::of(state, shown);
    let footer = match state.cost_combined() {
        Some(c) => format!(
            "spent so far {}{} · what this session would cost at API prices",
            if c.approx { "≈" } else { "" },
            money(c.usd)
        ),
        None => "spent so far — (no price for this model)".to_string(),
    };
    Overview {
        schema: 1,
        charts: vec![
            context(state),
            spend(state, opts.chain_usd),
            cost_per_turn(state, &per_turn, label.clone()),
            cache(state, &per_turn, label.clone()),
            tool_calls(state, &per_turn, label),
        ],
        footer,
    }
}

fn axis(from: f64, to: f64, unit: &str) -> Axis {
    Axis {
        from,
        to: to.max(from),
        unit: unit.to_string(),
    }
}

fn point(x: f64, y: f64) -> Point {
    Point { x, y, role: None }
}

fn legend(role: Role, text: impl Into<String>) -> Legend {
    Legend {
        role,
        text: text.into(),
    }
}

/// At most [`BUCKETS`] points: consecutive points grouped, each group the
/// last `x` and the highest `y` (a peak is never lost, none is invented).
fn bucket(points: Vec<Point>) -> Vec<Point> {
    if points.len() <= BUCKETS {
        return points;
    }
    let size = points.len().div_ceil(BUCKETS);
    points
        .chunks(size)
        .map(|c| {
            let y = c.iter().map(|p| p.y).fold(f64::MIN, f64::max);
            point(c[c.len() - 1].x, y)
        })
        .collect()
}

fn turn_end_ms(t: &Turn) -> Option<i64> {
    t.last_at
        .as_deref()
        .or(t.started_at.as_deref())
        .and_then(parse_ts_ms)
}

// -- context --------------------------------------------------------------

fn context(state: &State) -> Chart {
    let v = state.context();
    let verdict = verdict::context(state);
    let turns: Vec<&Turn> = state.agg.turns.iter().filter(|t| t.api_calls > 0).collect();
    let points = bucket(
        turns
            .iter()
            .map(|t| point(t.number as f64, t.context_size as f64))
            .collect(),
    );
    let last = turns.last().map(|t| t.number as f64);
    let mut spec_x_to = last.unwrap_or(1.0);
    let mut forecast = None;
    let mut legend_rows = vec![legend(Role::Accent, "context so far")];
    if let (Some(last), Some(vel), Some(n)) = (
        last,
        v.velocity.filter(|v| *v > 0.0),
        verdict::trusted_turns_left(state),
    ) {
        // Forecast to the threshold, no further ahead than the session so far.
        let horizon = n.ceil().clamp(1.0, last.max(10.0));
        let deltas: Vec<f64> = turns
            .windows(2)
            .filter(|w| !v.compactions.iter().any(|c| c.turn == w[1].number))
            .map(|w| w[1].context_size as f64 - w[0].context_size as f64)
            .filter(|d| *d > 0.0)
            .collect();
        let recent = &deltas[deltas.len().saturating_sub(5)..];
        let lo = recent.iter().copied().fold(vel, f64::min);
        let hi = recent.iter().copied().fold(vel, f64::max);
        let at = |rate: f64| (v.size as f64 + rate * horizon).min(v.window as f64);
        forecast = Some(Forecast {
            from: point(last, v.size as f64),
            to: point(last + horizon, at(vel)),
            low: at(lo),
            high: at(hi),
            role: Role::Dim,
        });
        spec_x_to = last + horizon;
        legend_rows.push(legend(
            Role::Dim,
            format!("forecast · +{} a turn", fmt::tokens(vel.round() as u64)),
        ));
    }
    let mut spec = ChartSpec::new(
        Kind::AreaForecast,
        axis(1.0, spec_x_to, "turn"),
        axis(0.0, v.window as f64, "tokens"),
    );
    spec.series.push(Series {
        name: "context".into(),
        role: Role::Accent,
        points: points.clone(),
    });
    spec.forecast = forecast;
    if v.window > 0 {
        spec.thresholds.push(Threshold {
            name: "auto-compact".into(),
            y: v.threshold as f64,
            role: Role::Crit,
        });
        legend_rows.push(legend(
            Role::Crit,
            format!("auto-compact at {}", fmt::tokens(v.threshold)),
        ));
    }
    for c in &v.compactions {
        spec.markers.push(Marker {
            kind: "compaction".into(),
            x: c.turn as f64,
            y: Some(c.after as f64),
            row: None,
            role: Role::Warn,
        });
    }
    if let Some(last) = last {
        spec.markers.push(Marker {
            kind: "now".into(),
            x: last,
            y: Some(v.size as f64),
            row: None,
            role: Role::Fg,
        });
    }
    // One readout per point drawn.
    let mut readouts = Vec::new();
    let mut prev: Option<f64> = None;
    for p in &points {
        let mut text = format!("Turn {} · {}", p.x, fmt::tokens(p.y as u64));
        if let Some(d) = prev.map(|q| p.y - q).filter(|d| *d != 0.0) {
            text.push_str(&format!(
                " ({}{})",
                if d > 0.0 { "+" } else { "−" },
                fmt::tokens(d.abs() as u64)
            ));
        }
        if v.compactions.iter().any(|c| c.turn as f64 == p.x) {
            text.push_str(" · compacted");
        }
        readouts.push(Readout { x: p.x, text });
        prev = Some(p.y);
    }
    // The composition from Claude Code's own count (PRD 3), with its age.
    let mut note = None;
    let bar = crate::dashboard::Composition::of(state).map(|c| {
        if let Some((_, _, _, at)) = state.current_context_capture() {
            note = Some(if at > 0 {
                format!(
                    "{}, {} ago",
                    c.source,
                    crate::coach::ago(state.clock_ms().saturating_sub(at))
                )
            } else {
                c.source.to_string()
            });
        }
        let mut bar = ChartSpec::new(
            Kind::StackedBar,
            axis(0.0, v.window as f64, "tokens"),
            axis(0.0, 1.0, "tokens"),
        );
        for (name, tokens) in &c.items {
            let role = composition_role(name);
            bar.series.push(Series {
                name: name.clone(),
                role,
                points: vec![point(0.0, *tokens as f64)],
            });
            legend_rows.push(legend(role, format!("{} {}", name, fmt::tokens(*tokens))));
        }
        bar.finish()
    });
    let window = verdict::window_words(v.window);
    Chart {
        id: "context",
        title: "Context window",
        figure: format!(
            "{} of {window} · {:.0} %",
            fmt::tokens(v.size),
            v.ratio() * 100.0
        ),
        verdict,
        note,
        label: None,
        spec: spec.finish(),
        bar,
        readouts,
        legend: legend_rows,
        prevent: Vec::new(),
        fallback_cells: Vec::new(),
    }
}

/// A `/context` category's role: what cannot change this session (S0),
/// what the person adds and can drop (S2), free space and the reserve.
fn composition_role(name: &str) -> Role {
    match name.to_lowercase().as_str() {
        "messages" => Role::S2,
        "mcp tools" | "skills" | "custom agents" | "memory files" => Role::S1,
        "free" => Role::Dim,
        "buffer" => Role::Warn,
        _ => Role::S0,
    }
}

// -- spend ----------------------------------------------------------------

fn spend(state: &State, chain_usd: Option<f64>) -> Chart {
    let verdict = verdict::spend(state);
    let facts = verdict::SpendFacts::of(state);
    let costs: BTreeMap<usize, f64> = verdict::turn_costs(state).into_iter().collect();
    let t0 = state
        .agg
        .turns
        .first()
        .and_then(|t| t.started_at.as_deref())
        .and_then(parse_ts_ms);
    let minutes = |ms: i64| t0.map_or(0.0, |t0| (ms - t0).max(0) as f64 / 60_000.0);
    let mut cum = 0.0;
    let mut raw = Vec::new();
    let mut at_turn: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    for t in &state.agg.turns {
        let (Some(c), Some(end)) = (costs.get(&t.number), turn_end_ms(t)) else {
            continue;
        };
        if raw.is_empty() {
            raw.push(point(0.0, 0.0));
        }
        cum += c;
        let p = point(minutes(end), cum);
        at_turn.insert(t.number, (p.x, p.y));
        raw.push(p);
    }
    // The line is the turns priced one by one (≈); the figure is the
    // session's own count. Where they part, a readout names both.
    let points = bucket(raw);
    let now = if points.is_empty() {
        0.0
    } else {
        minutes(state.clock_ms()).max(points.last().map_or(0.0, |p| p.x))
    };
    let mut x_to = now.max(1.0);
    let mut y_to = cum;
    let mut legend_rows = vec![legend(Role::Accent, "spent")];
    let mut figure = match facts.total {
        Some(t) => money(t),
        None => "—".to_string(),
    };
    let mut spec_forecast = None;
    if let Some(rate) = facts.per_min.filter(|r| *r > 0.0 && !points.is_empty()) {
        // To the session's first hour, or an hour from now once it is
        // within 15 minutes of that.
        let (end, when) = if now + 15.0 <= 60.0 {
            (60.0, "at 1h")
        } else {
            (now + 60.0, "in an hour")
        };
        let average = if now > 0.0 { cum / now } else { rate };
        let at = |r: f64| cum + r * (end - now);
        spec_forecast = Some(Forecast {
            from: point(now, cum),
            to: point(end, at(rate)),
            low: at(rate.min(average)),
            high: at(rate.max(average)),
            role: Role::Dim,
        });
        x_to = end;
        y_to = y_to.max(at(rate.max(average)));
        // The figure's forecast adds the line's forecast to the figure.
        let ahead = at(rate) - cum;
        figure.push_str(&format!(
            " · ≈ {} {when}",
            money(facts.total.unwrap_or(cum) + ahead)
        ));
        legend_rows.push(legend(
            Role::Dim,
            format!("forecast · {} a minute", money(rate)),
        ));
    }
    let mut spec = ChartSpec::new(
        Kind::CumulativeForecast,
        axis(0.0, x_to, "min"),
        axis(0.0, y_to, "USD"),
    );
    spec.series.push(Series {
        name: "spend".into(),
        role: Role::Accent,
        points,
    });
    spec.forecast = spec_forecast;
    // The budget line of a `--max-budget-usd` run (PRD 4).
    if let Some((_, total)) = facts.budget {
        spec.thresholds.push(Threshold {
            name: "budget".into(),
            y: total,
            role: Role::Warn,
        });
        spec.y.to = spec.y.to.max(total);
        let mut text = format!("budget {}", money(total));
        if let Some(m) = facts.halt_min() {
            text.push_str(&format!(" · halts in ~{:.0} min", m.ceil()));
        }
        legend_rows.push(legend(Role::Warn, text));
    }
    // The account's spend limit (PRD 2): a line where the limit is, and the
    // limits panel's text at now.
    let mut readouts = Vec::new();
    if let Some((_, text)) = &facts.spend_limit {
        if let Some((_, Some(limit))) = state.status_facts.spend_limit_usd {
            spec.thresholds.push(Threshold {
                name: "spend limit".into(),
                y: limit,
                role: Role::Warn,
            });
        }
        legend_rows.push(legend(Role::Warn, text.clone()));
        readouts.push(Readout {
            x: now,
            text: format!("now · {text}"),
        });
    }
    if let Some(total) = facts.total.filter(|t| *t > 0.0 && cum > 0.0) {
        if ((cum - total) / total).abs() > verdict::gate(&verdict::SPEND_PRICED_GAP) {
            readouts.push(Readout {
                x: now,
                text: format!(
                    "now · priced turns {} · Claude Code's own count {}",
                    money(cum),
                    money(total)
                ),
            });
        }
    }
    for j in verdict::spend_jumps(state) {
        let Some((x, y)) = at_turn.get(&j.turn).copied() else {
            continue;
        };
        let role = jump_role(j.kind);
        spec.markers.push(Marker {
            kind: "jump".into(),
            x,
            y: Some(y),
            row: None,
            role,
        });
        readouts.push(Readout {
            x,
            text: format!(
                "turn {} · {} {}",
                j.turn,
                j.kind.words().trim_start_matches("a "),
                money(j.usd)
            ),
        });
    }
    // The chain's total (PRD 4), beside this session's.
    if let (Some(chain), Some(total)) = (chain_usd, facts.total) {
        readouts.push(Readout {
            x: now,
            text: format!(
                "now · {} this session · {} with the sessions it came from",
                money(total),
                money(chain)
            ),
        });
    }
    Chart {
        id: "spend",
        title: "Spend",
        figure,
        verdict,
        note: None,
        label: None,
        spec: spec.finish(),
        bar: None,
        readouts,
        legend: legend_rows,
        prevent: Vec::new(),
        fallback_cells: Vec::new(),
    }
}

fn jump_role(k: verdict::JumpKind) -> Role {
    match k {
        verdict::JumpKind::Agent => Role::S1,
        verdict::JumpKind::Rebuild => Role::Crit,
        verdict::JumpKind::Turn => Role::Warn,
    }
}

// -- the per-turn charts --------------------------------------------------

/// What the three per-turn charts read for the turns shown.
struct PerTurn<'a> {
    turns: Vec<&'a Turn>,
    costs: BTreeMap<usize, f64>,
    /// USD of the agents each turn launched.
    agents: BTreeMap<usize, f64>,
    /// The first located miss of each turn.
    located: BTreeMap<usize, &'a crate::requests::LocatedMiss>,
    /// The API's own miss reason, first of each turn.
    miss_label: BTreeMap<usize, String>,
    first_call_turn: Option<usize>,
}

impl<'a> PerTurn<'a> {
    fn of(state: &'a State, shown: &[&'a Turn]) -> PerTurn<'a> {
        let mut located: BTreeMap<usize, &crate::requests::LocatedMiss> = BTreeMap::new();
        for m in state.requests.misses() {
            if let Some(t) = m.turn {
                located.entry(t).or_insert(m);
            }
        }
        PerTurn {
            turns: shown.to_vec(),
            costs: verdict::turn_costs(state).into_iter().collect(),
            agents: verdict::agent_costs(state),
            located,
            // The API's own reason, for a miss the request chain did not locate.
            miss_label: state
                .agg
                .miss_causes_by_turn
                .iter()
                .rev()
                .map(|(t, c)| (*t, c.clone()))
                .collect(),
            first_call_turn: state
                .agg
                .turns
                .iter()
                .find(|t| t.api_calls > 0)
                .map(|t| t.number),
        }
    }

    fn x_axis(&self) -> Axis {
        let first = self.turns.first().map_or(1.0, |t| t.number as f64);
        let last = self.turns.last().map_or(1.0, |t| t.number as f64);
        axis(first, last, "turn")
    }
}

/// A located cause in words (PRD 1).
fn cause_words(c: &LocatedCause) -> String {
    match c {
        LocatedCause::ToolsChanged { added, removed, .. } => {
            // An MCP tool is named first: it is the change a habit avoids.
            let mcp = |v: &Vec<String>| v.iter().find(|n| n.starts_with("mcp__")).cloned();
            match (mcp(added), mcp(removed), added.first(), removed.first()) {
                (Some(a), _, _, _) => format!("a new MCP tool loaded ({})", ident(&a)),
                (None, Some(r), _, _) => format!("an MCP tool was removed ({})", ident(&r)),
                (None, None, Some(a), _) => format!("a new tool loaded ({})", ident(a)),
                (None, None, None, Some(r)) => format!("a tool was removed ({})", ident(r)),
                (None, None, None, None) => "a tool's schema changed".to_string(),
            }
        }
        LocatedCause::PrefixRewritten { .. } => "an earlier message changed".to_string(),
        LocatedCause::ParamsChanged { field, from, to } => {
            let v = |s: &Option<String>, model: bool| match s {
                Some(s) if model => fmt::model_short(s),
                Some(s) => s.clone(),
                None => "none".to_string(),
            };
            match *field {
                "effort" => format!("effort changed from {} to {}", v(from, false), v(to, false)),
                "model" => format!(
                    "the model changed from {} to {}",
                    v(from, true),
                    v(to, true)
                ),
                "betas" => "the beta flags changed".to_string(),
                other => format!("{other} changed"),
            }
        }
        LocatedCause::SystemChanged { .. } => "the system prompt changed".to_string(),
        LocatedCause::Expired { gap_ms, .. } => {
            format!("the cache expired after {} idle", spoken(*gap_ms as i64))
        }
        LocatedCause::ThinkingDrop { .. } => {
            "Claude Code dropped thinking from the request".to_string()
        }
        LocatedCause::Unlocated => "of a change cctop could not locate".to_string(),
    }
}

fn cost_per_turn(state: &State, pt: &PerTurn, label: Option<String>) -> Chart {
    let verdict = verdict::cost_per_turn(state);
    let facts = verdict::CostTurnFacts::of(state);
    let long = state.status_facts.exceeds_200k_tokens;
    let mut spec = ChartSpec::new(Kind::Bars, pt.x_axis(), axis(0.0, 0.0, "USD"));
    let mut points = Vec::new();
    let mut readouts = Vec::new();
    let (mut any_agent, mut any_miss, mut any_long) = (false, false, false);
    let live = state.agg.current_turn().map(|t| t.number);
    let jumps: Vec<usize> = verdict::spend_jumps(state).iter().map(|j| j.turn).collect();
    for t in &pt.turns {
        let now = if live == Some(t.number) && state.session.alive {
            " (now)"
        } else {
            ""
        };
        // A turn without a price has no bar: a $0 bar would be a claim.
        let usd = match pt.costs.get(&t.number) {
            Some(u) => *u,
            None if t.api_calls == 0 => 0.0,
            None => {
                readouts.push(Readout {
                    x: t.number as f64,
                    text: format!("Turn {}{now} · unpriced.", t.number),
                });
                continue;
            }
        };
        let agent = pt.agents.get(&t.number).copied().filter(|a| *a > 0.0);
        let miss = pt.located.contains_key(&t.number) || pt.miss_label.contains_key(&t.number);
        let role = if miss {
            any_miss = true;
            Some(Role::Crit)
        } else if agent.is_some() {
            any_agent = true;
            Some(Role::S1)
        } else {
            None
        };
        points.push(Point {
            x: t.number as f64,
            y: usd,
            role,
        });
        spec.y.to = spec.y.to.max(usd);
        // A `>200k` marker on the turns past it (PRD 2: the status line's flag).
        if long && t.context_size > 200_000 {
            any_long = true;
            spec.markers.push(Marker {
                kind: "over_200k".into(),
                x: t.number as f64,
                y: Some(usd),
                row: None,
                role: Role::Warn,
            });
        }
        if t.api_calls == 0 {
            readouts.push(Readout {
                x: t.number as f64,
                text: format!("Turn {}{now} · no API call.", t.number),
            });
            continue;
        }
        let mut text = format!("Turn {}{now} · {}.", t.number, money(usd));
        let mut normal = true;
        if let Some(a) = agent {
            text.push_str(&format!(" Claude started a helper agent ({}).", money(a)));
            normal = false;
        }
        if miss {
            match pt.located.get(&t.number) {
                Some(m) => text.push_str(&format!(
                    " The cache was rebuilt: {}.",
                    cause_words(&m.cause).trim_start_matches("of ")
                )),
                None => text.push_str(" The cache was rebuilt."),
            }
            normal = false;
        }
        // Retries from the trace spans (PRD 7).
        if let Some((n, ms)) = state.turn_retries(t) {
            text.push_str(&format!(
                " {} {}, +{}.",
                n,
                if n == 1 { "retry" } else { "retries" },
                fmt::added_s(ms)
            ));
            normal = false;
        }
        if long && t.context_size > 200_000 {
            text.push_str(&format!(
                " {} of context, past 200k.",
                fmt::tokens(t.context_size)
            ));
            normal = false;
        }
        if normal && jumps.contains(&t.number) {
            text.push_str(&format!(
                " A costly turn: {} API {}, up to {} of context.",
                t.api_calls,
                if t.api_calls == 1 { "call" } else { "calls" },
                fmt::tokens(t.context_size)
            ));
        } else if normal && t.api_calls > 0 {
            text.push_str(" A normal turn.");
        }
        readouts.push(Readout {
            x: t.number as f64,
            text,
        });
    }
    spec.series.push(Series {
        name: "cost".into(),
        role: Role::Accent,
        points,
    });
    let mut legend_rows = vec![legend(Role::Accent, "a turn")];
    if any_agent {
        legend_rows.push(legend(Role::S1, "with a helper agent"));
    }
    if any_miss {
        legend_rows.push(legend(Role::Crit, "cache rebuilt"));
    }
    if any_long {
        legend_rows.push(legend(Role::Warn, "past 200k"));
    }
    Chart {
        id: "cost_per_turn",
        title: "Cost per turn",
        figure: match facts.typical() {
            Some(t) => format!("≈ {} typical", money(t)),
            None => "—".to_string(),
        },
        verdict,
        note: None,
        label,
        spec: spec.finish(),
        bar: None,
        readouts,
        legend: legend_rows,
        prevent: Vec::new(),
        fallback_cells: Vec::new(),
    }
}

fn cache(state: &State, pt: &PerTurn, label: Option<String>) -> Chart {
    let verdict = verdict::cache_with(state, label.as_ref().map(|_| pt.x_axis().from as usize));
    let facts = verdict::CacheFacts::of(state);
    let mut spec = ChartSpec::new(Kind::Pills, pt.x_axis(), axis(0.0, 1.0, "turn"));
    let mut points = Vec::new();
    let mut readouts = Vec::new();
    let mut prevent = Vec::new();
    let (mut any_miss, mut any_first, mut any_drop) = (false, false, false);
    for t in &pt.turns {
        let n = t.number;
        let misses: Vec<&crate::requests::LocatedMiss> = state
            .requests
            .misses()
            .iter()
            .filter(|m| m.turn == Some(n))
            .collect();
        for m in &misses {
            let field = match &m.cause {
                LocatedCause::ParamsChanged { field, .. } => Some(*field),
                _ => None,
            };
            if let Some(habit) = verdict::habit_of(&m.cause) {
                prevent.push(Prevent {
                    turn: n,
                    kind: m.cause.kind(),
                    field,
                    cause: cause_words(&m.cause),
                    tokens: m.tokens,
                    habit,
                });
            }
        }
        let (role, mut text) = if t.api_calls == 0 {
            (Some(Role::Dim), format!("Turn {n}: no API call."))
        } else if let Some(m) = misses.first() {
            any_miss = true;
            let tokens: u64 = misses.iter().map(|m| m.tokens).sum();
            let mut text = format!(
                "Turn {n}: rebuilt because {} · {} rebuilt",
                cause_words(&m.cause),
                fmt::tokens(tokens)
            );
            if misses.len() > 1 {
                text.push_str(&format!(" in {} rebuilds", misses.len()));
            }
            // A settings change marks the miss (PRD 5).
            if misses.iter().any(|m| m.after_config_change.is_some()) {
                text.push_str(", after a settings change");
            }
            text.push('.');
            (Some(Role::Crit), text)
        } else if pt.first_call_turn == Some(n) {
            any_first = true;
            (
                Some(Role::Dim),
                format!("Turn {n}: the first turn writes the cache."),
            )
        } else if let Some(l) = pt.miss_label.get(&n) {
            any_miss = true;
            (Some(Role::Crit), format!("Turn {n}: rebuilt ({l})."))
        } else {
            (None, format!("Turn {n}: the cache was reused."))
        };
        // Thinking drops on the strip (PRD 4).
        if t.thinking_dropped_blocks > 0 {
            any_drop = true;
            spec.markers.push(Marker {
                kind: "thinking_dropped".into(),
                x: n as f64,
                y: None,
                row: None,
                role: Role::Warn,
            });
            text.push_str(&format!(
                " Thinking dropped: {} {}.",
                t.thinking_dropped_blocks,
                if t.thinking_dropped_blocks == 1 {
                    "block"
                } else {
                    "blocks"
                }
            ));
        }
        points.push(Point {
            x: n as f64,
            y: 1.0,
            role,
        });
        readouts.push(Readout { x: n as f64, text });
    }
    spec.series.push(Series {
        name: "turns".into(),
        role: Role::Ok,
        points,
    });
    let mut legend_rows = vec![legend(Role::Ok, "reused")];
    if any_miss {
        legend_rows.push(legend(Role::Crit, "rebuilt"));
    }
    if any_first {
        legend_rows.push(legend(Role::Dim, "first turn writes the cache"));
    }
    if any_drop {
        legend_rows.push(legend(Role::Warn, "thinking dropped"));
    }
    // The countdown while the cache is warm.
    let ttl = state.cache_ttl_ms();
    let (bar, note) = match (state.cache_clock(), state.cache_warm()) {
        (Some(k), Some((true, _))) if k.remaining_ms > 0 && pt.first_call_turn.is_some() => {
            let band = verdict::gate(if ttl >= 3_600_000 {
                &verdict::CACHE_COUNTDOWN_1H
            } else {
                &verdict::CACHE_COUNTDOWN_5M
            }) as i64;
            let mut bar = ChartSpec::new(
                Kind::Progress,
                axis(0.0, ttl as f64, "ms"),
                axis(0.0, 1.0, "ms"),
            );
            bar.series.push(Series {
                name: "warm".into(),
                role: if k.remaining_ms <= band {
                    Role::Warn
                } else {
                    Role::Ok
                },
                points: vec![point(k.remaining_ms as f64, 1.0)],
            });
            (
                Some(bar.finish()),
                Some(format!(
                    "stays warm {}{}",
                    if k.approx { "≈ " } else { "" },
                    spoken(k.remaining_ms)
                )),
            )
        }
        _ => (None, None),
    };
    Chart {
        id: "cache",
        title: "Prompt cache",
        figure: match facts.hit.filter(|_| facts.turns > 1) {
            Some(h) => format!("reused {:.0} %", h * 100.0),
            None => "—".to_string(),
        },
        verdict,
        note,
        label,
        spec: spec.finish(),
        bar,
        readouts,
        legend: legend_rows,
        prevent,
        fallback_cells: Vec::new(),
    }
}

/// Heat-map rows: the five most used tools of the turns shown, the rest as
/// `other`, and skill shell commands (PRD 5) as their own row.
const HEAT_ROWS: usize = 5;

fn tool_calls(state: &State, pt: &PerTurn, label: Option<String>) -> Chart {
    let verdict = verdict::tool_calls(state);
    let shown: Vec<usize> = pt.turns.iter().map(|t| t.number).collect();
    let calls: Vec<&crate::tools::Call> = state
        .tools
        .calls
        .iter()
        .filter(|c| shown.contains(&c.turn))
        .collect();
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &calls {
        *totals.entry(c.name.as_str()).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let top: Vec<&str> = ranked.iter().take(HEAT_ROWS).map(|(n, _)| *n).collect();
    let row_of = |name: &str| top.iter().position(|n| *n == name).unwrap_or(top.len());
    let mut names: Vec<String> = top.iter().map(|n| n.to_string()).collect();
    if ranked.len() > HEAT_ROWS {
        names.push("other".into());
    }
    // count[row][turn index], failed[row][turn index]
    let mut count = vec![vec![0usize; shown.len()]; names.len()];
    let mut failed = vec![vec![false; shown.len()]; names.len()];
    for c in &calls {
        let (r, i) = (row_of(&c.name), shown.iter().position(|n| *n == c.turn));
        if let Some(i) = i {
            count[r][i] += 1;
            failed[r][i] |= c.is_error && c.denial.is_none();
        }
    }
    // Skill shell commands per turn, from the hook events' times.
    let starts: Vec<(usize, i64)> = state
        .agg
        .turns
        .iter()
        .filter_map(|t| Some((t.number, parse_ts_ms(t.started_at.as_deref()?)?)))
        .collect();
    let mut skill = vec![0usize; shown.len()];
    let mut any_skill = false;
    for e in state.events.iter() {
        if e.kind != crate::events::Kind::Hook || !e.text.starts_with("skill shell: ") {
            continue;
        }
        let turn = starts
            .iter()
            .rev()
            .find(|(_, at)| *at <= e.at)
            .map(|(n, _)| *n);
        if let Some(i) = turn.and_then(|t| shown.iter().position(|n| *n == t)) {
            skill[i] += 1;
            any_skill = true;
        }
    }
    let skill_row = (any_skill || !state.skill_shell.is_empty()).then_some(names.len());
    let mut spec = ChartSpec::new(Kind::Heat, pt.x_axis(), axis(0.0, 0.0, "calls"));
    for (r, name) in names.iter().enumerate() {
        spec.series.push(Series {
            name: name.clone(),
            role: Role::Accent,
            points: shown
                .iter()
                .enumerate()
                .map(|(i, n)| point(*n as f64, count[r][i] as f64))
                .collect(),
        });
        for (i, n) in shown.iter().enumerate() {
            if failed[r][i] {
                spec.markers.push(Marker {
                    kind: "failed".into(),
                    x: *n as f64,
                    y: None,
                    row: Some(r),
                    role: Role::Crit,
                });
            }
        }
    }
    if skill_row.is_some() {
        spec.series.push(Series {
            name: "skill shell".into(),
            role: Role::S1,
            points: shown
                .iter()
                .enumerate()
                .map(|(i, n)| point(*n as f64, skill[i] as f64))
                .collect(),
        });
    }
    spec.y.to = spec
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y))
        .fold(0.0, f64::max);
    let readouts = shown
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let k: usize = (0..names.len()).map(|r| count[r][i]).sum();
            let mut parts: Vec<(usize, &str)> = (0..names.len())
                .filter(|r| count[*r][i] > 0)
                .map(|r| (count[r][i], names[r].as_str()))
                .collect();
            parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            let f = calls
                .iter()
                .filter(|c| c.turn == *n && c.is_error && c.denial.is_none())
                .count();
            let mut text = format!("Turn {n} · {k} {}", if k == 1 { "call" } else { "calls" });
            if !parts.is_empty() {
                text.push_str(": ");
                text.push_str(
                    &parts
                        .iter()
                        .map(|(c, name)| format!("{name} {c}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            if f > 0 {
                text.push_str(&format!(", {f} failed"));
            }
            if skill[i] > 0 {
                text.push_str(&format!(", {} skill shell", skill[i]));
            }
            text.push('.');
            Readout { x: *n as f64, text }
        })
        .collect();
    let mut legend_rows = vec![legend(Role::Accent, "brighter = more calls")];
    if !spec.markers.is_empty() {
        legend_rows.push(legend(Role::Crit, "failed"));
    }
    if skill_row.is_some() {
        legend_rows.push(legend(Role::S1, "skill shell commands"));
    }
    let total = state.tools.calls.len();
    Chart {
        id: "tool_calls",
        title: "Tool calls per turn",
        figure: format!("{total} {}", if total == 1 { "call" } else { "calls" }),
        verdict,
        note: None,
        label,
        spec: spec.finish(),
        bar: None,
        readouts,
        legend: legend_rows,
        prevent: Vec::new(),
        fallback_cells: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::rules::fixtures::{prompt, response, tool};
    use crate::metrics::Pricing;

    const IDS: [&str; 5] = ["context", "spend", "cost_per_turn", "cache", "tool_calls"];

    fn fixture(name: &str) -> State {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
        crate::load::state_from(&path, crate::ui::state::SessionInfo::from_fixture(&path))
    }

    fn chart<'a>(o: &'a Overview, id: &str) -> &'a Chart {
        o.charts.iter().find(|c| c.id == id).unwrap()
    }

    fn sane(o: &Overview) {
        assert_eq!(
            o.charts.iter().map(|c| c.id).collect::<Vec<_>>(),
            IDS.to_vec()
        );
        for c in &o.charts {
            assert!(c.spec.is_finite(), "{}: {:?}", c.id, c.spec);
            if let Some(b) = &c.bar {
                assert!(b.is_finite(), "{}: {b:?}", c.id);
            }
            let json = serde_json::to_string(c).unwrap();
            assert!(!json.contains("NaN") && !json.contains("inf"), "{json}");
            assert!(
                c.spec.x.from <= c.spec.x.to && c.spec.y.from <= c.spec.y.to,
                "{}",
                c.id
            );
        }
    }

    /// Review focus 1: a session with no turn and one with one turn.
    #[test]
    fn zero_and_one_turn_sessions_are_valid_and_calm() {
        let s = State::new(Pricing::bundled());
        let o = build(&s);
        sane(&o);
        for c in &o.charts {
            assert_eq!(c.verdict.level, verdict::Level::Good, "{}", c.id);
            assert!(c.verdict.todo.is_none(), "{}: {:?}", c.id, c.verdict);
            assert!(
                c.spec.series.iter().all(|s| s.points.is_empty()),
                "{}",
                c.id
            );
        }
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T10:00:00.000Z"));
        for l in tool("t1", "2026-01-01T10:00:05.000Z", "Read", "/w/a.rs", 4000) {
            s.apply(&l);
        }
        s.apply(&response("r1", "2026-01-01T10:00:10.000Z", 5, 30_000, 0));
        let o = build(&s);
        sane(&o);
        for c in &o.charts {
            assert_ne!(
                c.verdict.level,
                verdict::Level::Act,
                "{}: {:?}",
                c.id,
                c.verdict
            );
        }
        assert_eq!(chart(&o, "cost_per_turn").spec.series[0].points.len(), 1);
        assert_eq!(
            chart(&o, "cache").readouts[0].text,
            "Turn 1: the first turn writes the cache."
        );
    }

    /// Review focus 3: 300 turns — the per-turn charts keep the last N with
    /// a label, context and spend cover the whole session in buckets.
    #[test]
    fn a_long_session_keeps_the_last_turns_and_buckets_the_rest() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..300u64 {
            let (h, m) = (10 + i / 60, i % 60);
            let ts = |sec: u64| format!("2026-01-01T{h:02}:{m:02}:{sec:02}.000Z");
            s.apply(&prompt(&ts(0)));
            for l in tool(&format!("t{i}"), &ts(10), "Bash", "ls", 200) {
                s.apply(&l);
            }
            s.apply(&response(
                &format!("r{i}"),
                &ts(20),
                5,
                1_000,
                20_000 + i * 500,
            ));
        }
        let o = build_with(
            &s,
            &Options {
                max_turns: 40,
                chain_usd: None,
            },
        );
        sane(&o);
        for id in ["cost_per_turn", "cache", "tool_calls"] {
            let c = chart(&o, id);
            assert_eq!(c.label.as_deref(), Some("last 40 turns"), "{id}");
            for series in &c.spec.series {
                assert_eq!(series.points.len(), 40, "{id} {}", series.name);
                assert_eq!(series.points[0].x, 261.0, "{id}");
            }
            assert!(c.readouts.iter().all(|r| r.x >= 261.0), "{id}");
            assert!(c.readouts.len() <= 40, "{id}");
        }
        for id in ["context", "spend"] {
            let c = chart(&o, id);
            assert_eq!(c.label, None, "{id}");
            let p = &c.spec.series[0].points;
            assert!(p.len() <= BUCKETS, "{id}: {}", p.len());
            assert!(p.len() >= BUCKETS / 2, "{id}: {}", p.len());
        }
        // Context's buckets reach from the first turn to the last.
        let ctx = &chart(&o, "context").spec;
        assert_eq!(ctx.series[0].points.last().unwrap().x, 300.0);
        assert!(ctx.series[0].points[0].x <= 3.0);
        // The whole session's figure stays in the header.
        assert_eq!(chart(&o, "tool_calls").figure, "300 calls");
        // Short sessions carry no label.
        let o = build(&fixture("fixtures/session-b.jsonl"));
        assert!(o.charts.iter().all(|c| c.label.is_none()));
    }

    #[test]
    fn the_cache_chart_says_how_to_avoid_each_located_rebuild() {
        let fixture_r = fixture("fixtures/session-r.jsonl");
        let o = build(&fixture_r);
        sane(&o);
        let c = chart(&o, "cache");
        assert!(!c.prevent.is_empty());
        // Turn 1's tools change loaded MCP tools beside the built-in
        // EnterWorktree: the MCP tool is named, and it is avoidable.
        assert!(c.prevent.iter().any(|p| p.kind == "tools_changed"));
        assert!(c.readouts[0]
            .text
            .starts_with("Turn 1: rebuilt because a new MCP tool loaded (mcp__"));
        for p in &c.prevent {
            let m = fixture_r
                .requests
                .misses()
                .iter()
                .find(|m| m.turn == Some(p.turn) && m.cause.kind() == p.kind)
                .unwrap();
            assert_eq!(Some(p.habit), verdict::habit_of(&m.cause), "{p:?}");
            assert!(
                c.readouts
                    .iter()
                    .any(|r| r.x == p.turn as f64 && r.text.contains("rebuilt because")),
                "{p:?}"
            );
        }
        // Expired misses: the habit is to reply within the TTL.
        assert!(c
            .prevent
            .iter()
            .any(|p| p.kind == "expired" && p.habit.starts_with("Reply before")));
        let rebuilt: Vec<&Point> = c.spec.series[0]
            .points
            .iter()
            .filter(|p| p.role == Some(Role::Crit))
            .collect();
        assert!(rebuilt.len() >= c.prevent.len());
    }

    #[test]
    fn overview_snapshots() {
        for (name, file) in [
            ("b", "fixtures/session-b.jsonl"),
            ("r", "fixtures/session-r.jsonl"),
        ] {
            let o = build(&fixture(file));
            sane(&o);
            insta::assert_snapshot!(
                format!("overview_{name}"),
                serde_json::to_string_pretty(&o).unwrap()
            );
        }
    }

    /// Fixture B just autocompacted (720k → 142k): the growth rate from
    /// before is no forecast, and the coach's light is quiet — so is the
    /// verdict. Its spend line is the priced turns, and where that parts
    /// from Claude Code's own count a readout names both.
    #[test]
    fn fixture_b_after_compaction_is_calm_and_names_both_spend_figures() {
        let s = fixture("fixtures/session-b.jsonl");
        let o = build(&s);
        let ctx = chart(&o, "context");
        assert_ne!(ctx.verdict.level, verdict::Level::Act, "{:?}", ctx.verdict);
        assert_eq!(
            crate::coach::context_light(&s).level,
            crate::coach::Level::Quiet
        );
        assert!(ctx.spec.forecast.is_none());
        let spend = chart(&o, "spend");
        let line_end = spend.spec.series[0].points.last().unwrap().y;
        assert!(spend.readouts.iter().any(|r| r.text
            == format!(
                "now · priced turns {} · Claude Code's own count $18.75",
                money(line_end)
            )));
        // A jump readout carries the bar's own figure.
        let bars = &chart(&o, "cost_per_turn").spec.series[0].points;
        for r in spend
            .readouts
            .iter()
            .filter(|r| r.text.starts_with("turn "))
        {
            let turn: usize = r.text[5..].split(' ').next().unwrap().parse().unwrap();
            let bar = bars.iter().find(|p| p.x == turn as f64).unwrap();
            assert!(r.text.ends_with(&money(bar.y)), "{} vs {}", r.text, bar.y);
        }
    }

    /// A turn on a model with no price has no bar and says so.
    #[test]
    fn an_unpriced_turn_has_no_bar() {
        use crate::transcript::Line;
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T10:00:00.000Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:10.000Z","message":{"id":"u1","model":"claude-nopriced-9","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":5,"cache_creation_input_tokens":3000,"cache_read_input_tokens":0,"output_tokens":10}}}"#).unwrap());
        let o = build(&s);
        sane(&o);
        let c = chart(&o, "cost_per_turn");
        assert!(c.spec.series[0].points.is_empty());
        assert_eq!(c.readouts[0].text, "Turn 1 · unpriced.");
    }

    #[test]
    fn a_cut_of_one_turn_is_singular() {
        let s = fixture("fixtures/session-b.jsonl");
        let o = build_with(
            &s,
            &Options {
                max_turns: 1,
                chain_usd: None,
            },
        );
        assert_eq!(chart(&o, "cache").label.as_deref(), Some("last 1 turn"));
    }

    #[test]
    fn footer_names_the_spend_at_api_prices() {
        let o = build(&fixture("fixtures/session-b.jsonl"));
        assert!(
            o.footer.starts_with("spent so far $")
                && o.footer
                    .ends_with(" · what this session would cost at API prices"),
            "{}",
            o.footer
        );
    }
}
