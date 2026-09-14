//! The coach object ("Lights", coach PRD §6): one snapshot of `State` and
//! the Advisor engine that every surface draws verbatim — the state line,
//! four lights, the nudge slot, the `next` and `snoozed` rows, the
//! lifecycle rows. Text is cut at [`WIDTH`] cells here so the TUI, the pane
//! and `cctop query coach` show the same characters.

use serde::Serialize;

use crate::advisor::{ActionKind, Engine, SessionMode, Urgency};
use crate::phase::Phase;
use crate::ui::fmt;
use crate::ui::state::{State, WaitingKind};

/// The card's text width in cells: every line is cut here.
pub const WIDTH: usize = 52;

/// A light's level: read from the corner of the eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Quiet,
    Watch,
    Act,
}

impl Level {
    pub fn glyph(self) -> char {
        match self {
            Level::Quiet => '○',
            Level::Watch => '◐',
            Level::Act => '●',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Light {
    /// `context` / `cache` / `limits` / `rework`.
    pub id: &'static str,
    pub level: Level,
    pub glyph: char,
    /// The figure a tile enlarges (`412k`, `41m`, `62%`, `3`); `—` when the
    /// source is absent.
    pub number: String,
    /// The tile's numeric figure and its unit, for the block digits.
    pub figure: Option<f64>,
    pub unit: &'static str,
    /// The light's row after the glyph and name (`412k ▇▇▇▇▁▁▁▁▁▁ 41% ·
    /// ≈$.21/call`, `warm 41m (1h)`); the number is part of it.
    pub text: String,
    /// The same row in rate-limit units (`re-reads 412k/call`), for the
    /// context light when `$` is toggled.
    pub alt: Option<String>,
    /// Where the figure comes from (`transcript`, `status line`, `git`…).
    pub source: &'static str,
    pub approx: bool,
    /// The three-line detail the light's digit opens.
    pub lines: Vec<String>,
}

impl Light {
    /// The full row: `◐ context  412k ▇▇▇▇▁▁▁▁▁▁ 41% · ≈$.21/call`.
    pub fn row(&self) -> String {
        cut(&format!("{} {:<8} {}", self.glyph, self.id, self.text))
    }
}

/// The nudge slot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Nudge {
    pub id: &'static str,
    pub family: &'static str,
    pub class: Urgency,
    /// `▸ headline` (cut).
    pub line1: String,
    /// `  action` (cut).
    pub line2: String,
    /// The evidence row: class, fire point, queue depth.
    pub evidence: String,
    pub action_text: String,
    pub action_kind: ActionKind,
    pub since_turn: usize,
    pub retires_on: &'static str,
    pub acting: bool,
    pub fired_at_ms: i64,
    pub queued: usize,
    pub saving: String,
    pub explain: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Next {
    pub id: &'static str,
    pub family: &'static str,
    pub class: Urgency,
    pub headline: String,
    /// What promotes it (`next prompt`, `when the slot frees`).
    pub promotes: &'static str,
    /// The `next` row as drawn.
    pub row: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snoozed {
    pub id: &'static str,
    pub family: &'static str,
    /// Human turns left, or `None` for the session.
    pub turns_left: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recent {
    pub at_ms: i64,
    pub what: &'static str,
    pub id: &'static str,
    pub family: &'static str,
    pub detail: String,
    /// The lifecycle row as drawn: `20:41 acted   verify-gap → cargo test ran 31s later`.
    pub row: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateLine {
    /// `IMPLEMENTING`, `EXPLORING`, `WAITING`, `IDLE`, `LOOP`, …
    pub kind: String,
    /// The whole line (cut).
    pub line: String,
    /// The tokens after the phase word, unjoined.
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Coach {
    pub schema: u32,
    pub session: String,
    pub model: String,
    pub turn: usize,
    pub state: StateLine,
    pub lights: Vec<Light>,
    /// `agents 3 run · 86%` while agents run or their share is large.
    pub agents: Option<String>,
    pub nudge: Option<Nudge>,
    pub next: Option<Next>,
    pub snoozed: Vec<Snoozed>,
    pub recent: Vec<Recent>,
    pub suppressed: Vec<(String, String)>,
    pub session_mode: SessionMode,
    pub nudges_this_hour: usize,
    /// Rules the engine holds in its queue behind the occupant.
    pub queued: usize,
    /// The one-line forms for status bars: L0 (≥ 80 columns), L1 (≥ 40),
    /// L2 (glyphs only).
    pub lines: StatusLines,
    /// The `next` and `snoozed` rows as drawn.
    pub next_row: String,
    pub snoozed_row: String,
    pub quiet_row: String,
    /// The first turn's dim line: the project's `/insights` medians and
    /// the previous session here (`None` after the first turn).
    pub start_line: Option<String>,
    /// Nudges are shown this session (`false` on the control arm of the
    /// measurement, `cctop run --coach off|auto`).
    pub exposed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct StatusLines {
    pub l0: String,
    pub l1: String,
    pub l2: String,
}

/// Cut to [`WIDTH`] cells.
pub fn cut(s: &str) -> String {
    fmt::clip(s, WIDTH)
}

/// `$.21`, `$2.6`, `$18`: the coach's short money form.
pub fn usd_short(v: f64) -> String {
    if v >= 10.0 {
        format!("${v:.0}")
    } else if v >= 1.0 {
        format!("${v:.1}")
    } else {
        format!("${:.2}", v).replacen("$0.", "$.", 1)
    }
}

/// Minutes-first duration: `41m`, `2h10`, `4:12` under five minutes.
pub fn short_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 300 {
        format!("{}:{:02}", s / 60, s % 60)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h{:02}", s / 3600, (s % 3600) / 60)
    }
}

fn ago(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h{:02}", s / 3600, (s % 3600) / 60)
    }
}

/// The whole object.
pub fn snapshot(state: &State, engine: &Engine) -> Coach {
    let now = state.clock_ms();
    let lights = vec![
        context_light(state),
        cache_light(state),
        limits_light(state),
        rework_light(state),
    ];
    let queued = engine
        .current
        .len()
        .saturating_sub(engine.occupant.is_some() as usize);
    // The control arm (`--coach off`): the engine fires and records, the
    // surfaces show no nudge.
    let nudge = engine
        .occupant
        .as_ref()
        .filter(|_| engine.exposed)
        .map(|o| {
            let a = &engine.current[0];
            let fire_point = match a.urgency {
                Urgency::Now => {
                    let calls = state.agg.current_turn().map(|t| t.tool_calls).unwrap_or(0);
                    format!("fired at call {calls}")
                }
                _ => format!("since turn {}", a.since_turn),
            };
            Nudge {
                id: a.rule,
                family: a.family,
                class: a.urgency,
                line1: cut(&format!("▸ {}", a.headline)),
                line2: cut(&format!("  {}", a.action)),
                evidence: cut(&format!(
                    "  {} · {fire_point} · +{queued} queued{}{}",
                    a.urgency.label(),
                    if queued > 0 { " (n)" } else { "" },
                    if o.acting { " · acting…" } else { "" }
                )),
                action_text: a.action_text.clone(),
                action_kind: a.action_kind,
                since_turn: a.since_turn,
                retires_on: a.retires_on,
                acting: o.acting,
                fired_at_ms: o.fired_at_ms,
                queued,
                saving: a.saving.label(),
                explain: crate::advisor::rules::explain(a.doc_key),
            }
        });
    let next = engine
        .next_up()
        .filter(|_| engine.exposed)
        .map(|(a, promotes)| Next {
            id: a.rule,
            family: a.family,
            class: a.urgency,
            headline: a.headline.clone(),
            promotes,
            row: cut(&format!(
                "next     {} → {} · {}",
                a.family, promotes, a.headline
            )),
        });
    let turn = state.agg.human_turns();
    let ids = engine.rule_ids();
    let snoozed: Vec<Snoozed> = engine
        .snoozed()
        .into_iter()
        .map(|(id, until)| Snoozed {
            id,
            family: family_of(&ids, id),
            turns_left: until.map(|u| u.saturating_sub(turn)),
        })
        .collect();
    let recent: Vec<Recent> = engine
        .recent
        .iter()
        .map(|l| Recent {
            at_ms: l.at_ms,
            what: l.what,
            id: l.rule,
            family: l.family,
            detail: l.detail.clone(),
            row: cut(&format!(
                "{} {:<7} {}",
                fmt::clock_hhmm(l.at_ms),
                l.what,
                l.detail
            )),
        })
        .collect();
    let nudges_this_hour = engine
        .records
        .iter()
        .filter(|r| now - r.shown_at_ms <= 3_600_000)
        .count();
    let mut c = Coach {
        schema: 1,
        session: state.session.name.clone(),
        model: state.model().unwrap_or("—").to_string(),
        turn,
        state: state_line(state, engine.session_mode),
        agents: agents_cell(state),
        lights,
        nudge,
        next,
        snoozed,
        recent,
        suppressed: engine
            .suppressed
            .iter()
            .map(|(r, w)| (r.to_string(), w.clone()))
            .collect(),
        session_mode: engine.session_mode,
        nudges_this_hour,
        queued,
        lines: StatusLines::default(),
        next_row: String::new(),
        snoozed_row: String::new(),
        quiet_row: String::new(),
        start_line: state.start_line().map(|l| cut(&l)),
        exposed: engine.exposed,
    };
    c.lines = StatusLines {
        l0: c.line(80),
        l1: c.line(40),
        l2: c.line(0),
    };
    c.next_row = c.next_row();
    c.snoozed_row = c.snoozed_row();
    c.quiet_row = c.quiet_row();
    c
}

fn family_of(_ids: &[&'static str], id: &'static str) -> &'static str {
    crate::advisor::rules::all()
        .iter()
        .find(|r| r.id() == id)
        .map(|r| r.family())
        .unwrap_or("")
}

impl Coach {
    /// The `snoozed` row: `snoozed  cache-miss (4 turns) · prefix tip (session)`.
    pub fn snoozed_row(&self) -> String {
        if self.snoozed.is_empty() {
            return "snoozed  —".into();
        }
        let parts: Vec<String> = self
            .snoozed
            .iter()
            .map(|s| match s.turns_left {
                Some(n) => format!("{} ({n} turn{})", s.family, if n == 1 { "" } else { "s" }),
                None => format!("{} (session)", s.family),
            })
            .collect();
        cut(&format!("snoozed  {}", parts.join(" · ")))
    }

    /// The `next` row, or `next     —`.
    pub fn next_row(&self) -> String {
        self.next
            .as_ref()
            .map(|n| n.row.clone())
            .unwrap_or_else(|| "next     —".into())
    }

    /// The quiet line when nothing occupies the slot.
    pub fn quiet_row(&self) -> String {
        if !self.exposed {
            return cut(&format!(
                "  coach off (control arm) · {} fire{} recorded",
                self.nudges_this_hour,
                if self.nudges_this_hour == 1 { "" } else { "s" }
            ));
        }
        if let Some(l) = &self.start_line {
            return cut(&format!("  {l}"));
        }
        cut(&format!(
            "  quiet · nothing to act on · {} nudge{} this hour",
            self.nudges_this_hour,
            if self.nudges_this_hour == 1 { "" } else { "s" }
        ))
    }

    /// The status line at three lengths: L0 (≥ 80 columns), L1, L2.
    pub fn line(&self, columns: usize) -> String {
        let l = &self.lights;
        if columns >= 80 {
            let mut parts: Vec<String> = vec![
                format!("{}{} {}", l[0].glyph, l[0].number, l[0].detail_money()),
                format!("{}cache {}", l[1].glyph, l[1].number),
                format!("{}5h {}", l[2].glyph, l[2].number),
                format!("{}{}", l[3].glyph, l[3].short()),
            ];
            if let Some(n) = &self.nudge {
                parts.push(format!("▸ {}", n.line2.trim_start()));
            }
            parts.join(" · ")
        } else if columns >= 40 {
            format!(
                "{}{} {}{} {}{} {}{}{}",
                l[0].glyph,
                l[0].percent(),
                l[2].glyph,
                l[2].number,
                l[1].glyph,
                l[1].number,
                l[3].glyph,
                l[3].number,
                if self.nudge.is_some() { " ▸" } else { "" }
            )
        } else {
            l.iter().map(|x| x.glyph).collect()
        }
    }
}

impl Light {
    /// The `≈$.21/call` part of the context row, or nothing.
    fn detail_money(&self) -> String {
        self.text
            .split(" · ")
            .nth(1)
            .unwrap_or("")
            .trim_end_matches("/call")
            .to_string()
    }
    /// `41%` for the context light.
    fn percent(&self) -> String {
        self.text
            .split(" · ")
            .next()
            .and_then(|s| s.rsplit(' ').next())
            .unwrap_or("")
            .to_string()
    }
    /// The rework light's short form: `fails 3` / `edits 3`.
    fn short(&self) -> String {
        let d = self.text.split(" · ").next().unwrap_or("");
        match d.split_whitespace().collect::<Vec<_>>().as_slice() {
            [n, "fail" | "fails", ..] => format!("fails {n}"),
            [n, "correction" | "corrections", ..] => format!("corrections {n}"),
            [n, "blocked", ..] => format!("blocked {n}"),
            ["edits", n, ..] => format!("edits {n}"),
            _ => format!("rework {}", self.number),
        }
    }
}

/// `agents 3 run · 86%` while agents run or their share ≥ 25 %.
fn agents_cell(state: &State) -> Option<String> {
    let now = state.clock_ms();
    let running = state
        .agents
        .values()
        .filter(|a| a.state(now) == crate::agents::State::Running)
        .count();
    let share = state.agents_cost().map(|(_, s)| s).unwrap_or(0.0);
    if running == 0 && share < 0.25 {
        return None;
    }
    Some(format!("agents {running} run · {:.0}%", share * 100.0))
}

/// Whether a turn is running as of the clock: no `turn_duration` yet, not
/// interrupted, and the model has not stopped with `end_turn` while nothing
/// runs — or a call started after the turn's end line (the turn resumed).
/// A dead session is read as of its last line.
pub fn turn_running(state: &State) -> bool {
    let Some(t) = state.agg.current_turn() else {
        return false;
    };
    if let (Some(c), Some(ended)) = (
        state.tools.running(),
        t.ended_at
            .as_deref()
            .and_then(crate::metrics::cost::parse_ts_ms),
    ) {
        if c.started_at.is_some_and(|s| s > ended) {
            return true;
        }
    }
    t.duration_ms.is_none()
        && t.interrupted_after_calls.is_none()
        && !(t.last_stop_reason.as_deref() == Some("end_turn") && state.tools.running().is_none())
}

/// The turn ended cleanly: `end_turn`, no background work, no running agents.
fn clean_stop(state: &State) -> bool {
    let Some(t) = state.agg.current_turn() else {
        return false;
    };
    t.duration_ms.is_some()
        && t.last_stop_reason.as_deref() == Some("end_turn")
        && state.session.background_tasks.is_empty()
        && t.pending_background_agents.unwrap_or(0) == 0
        && !state
            .agents
            .values()
            .any(|a| a.state(state.clock_ms()) == crate::agents::State::Running)
}

pub fn context_light(state: &State) -> Light {
    let v = state.context();
    let bands = state.bands();
    let size = v.size;
    let window = v.window.max(1);
    let pct = size as f64 / window as f64 * 100.0;
    let level = if size >= bands.warn_at || (size >= 300_000 && clean_stop(state)) {
        Level::Act
    } else if size >= 150_000 {
        Level::Watch
    } else {
        Level::Quiet
    };
    let filled = ((pct / 10.0).round() as usize).min(10);
    let bar = format!("{}{}", "▇".repeat(filled), "▁".repeat(10 - filled));
    // The warm price is the figure; a cold cache adds what the next call
    // costs instead.
    let g = state.gradient_priced(false);
    let per_call = g
        .as_ref()
        .map(|g| format!("≈{}/call", usd_short(g.per_call)))
        .unwrap_or_else(|| "—/call".into());
    let cold = state
        .gradient()
        .filter(|g| g.cold)
        .map(|g| format!(" (cold: next call ≈{})", usd_short(g.per_call)))
        .unwrap_or_default();
    let mut lines = Vec::new();
    if let Some(g) = &g {
        lines.push(format!(
            "next {}c ≈{} · fresh ≈{}",
            g.calls_per_turn.round() as u64,
            usd_short(g.per_turn),
            usd_short(g.per_turn_at_100k)
        ));
    }
    lines.push(format!(
        "threshold {} · {} left · prefix {} ({:.0} %)",
        fmt::tokens(v.threshold),
        fmt::tokens(v.threshold.saturating_sub(size)),
        fmt::tokens(v.prefix),
        if size > 0 {
            v.prefix as f64 / size as f64 * 100.0
        } else {
            0.0
        }
    ));
    if let Some(b) = state.baseline.as_ref().filter(|b| b.sessions >= 3) {
        if let (Some(cost), Some(med)) = (
            state
                .cost
                .current()
                .map(|c| c.usd / state.agg.human_turns().max(1) as f64),
            b.cost_per_turn,
        ) {
            lines.push(format!(
                "turn ≈{} ×{:.1} 7d",
                usd_short(cost),
                if med > 0.0 { cost / med } else { 0.0 }
            ));
        }
    }
    Light {
        id: "context",
        level,
        glyph: level.glyph(),
        number: if size == 0 {
            "—".into()
        } else {
            fmt::tokens(size)
        },
        figure: (size > 0).then_some(pct.round()),
        unit: "%",
        text: format!(
            "{} {bar} {pct:.0}% · {per_call}{cold}",
            if size == 0 {
                "—".to_string()
            } else {
                fmt::tokens(size)
            }
        ),
        alt: Some(format!(
            "{} {bar} {pct:.0}% · re-reads {}/call",
            if size == 0 {
                "—".to_string()
            } else {
                fmt::tokens(size)
            },
            fmt::tokens(size)
        )),
        source: if v.window_exact {
            "status line"
        } else {
            "transcript"
        },
        approx: !v.window_exact || g.is_none(),
        lines,
    }
}

pub fn cache_light(state: &State) -> Light {
    let ttl_ms = state.cache_ttl_ms();
    let ttl = if ttl_ms >= 3_600_000 { "1h" } else { "5m" };
    let rewrite = if state.cache.from_shim && state.cache.recache_tokens_if_cold > 0 {
        state.cache.recache_tokens_if_cold
    } else {
        state.context().size
    };
    let idle = !turn_running(state);
    // The countdown band: the last five minutes of a 1 h entry, the last
    // two of a 5 m one (A19).
    let countdown_ms = if ttl_ms >= 3_600_000 {
        300_000
    } else {
        120_000
    };
    let (level, number, figure, unit, text) = match state.cache_clock() {
        None => (Level::Quiet, "—".to_string(), None, "m", String::new()),
        Some(c) if c.remaining_ms > 0 => {
            let left = c.remaining_ms;
            let approx = if c.approx { " ≈" } else { "" };
            if left <= countdown_ms {
                let level = if idle && rewrite >= 50_000 {
                    Level::Act
                } else {
                    Level::Watch
                };
                (
                    level,
                    short_duration(left),
                    Some((left / 60_000) as f64),
                    "m",
                    format!("cold in {} ({ttl}){approx}", short_duration(left)),
                )
            } else {
                (
                    Level::Quiet,
                    short_duration(left),
                    Some((left / 60_000) as f64),
                    "m",
                    format!("warm {} ({ttl}){approx}", short_duration(left)),
                )
            }
        }
        Some(_) => (
            if rewrite >= 100_000 && idle {
                Level::Act
            } else {
                Level::Watch
            },
            fmt::tokens(rewrite),
            Some((rewrite / 1000) as f64),
            "k",
            format!("cold · {} re-write", fmt::tokens(rewrite)),
        ),
    };
    let mut lines = Vec::new();
    let hit = state
        .agg
        .total
        .cache_hit_ratio()
        .map(|r| format!("hit ratio {:.0} %", r * 100.0))
        .unwrap_or_else(|| "hit ratio —".into());
    let misses = state.agg.cache_misses.len() as u64 + state.cache.misses;
    let last = state
        .agg
        .cache_misses
        .last()
        .map(|m| m.kind.clone())
        .or_else(|| state.cache.last_miss_cause.clone());
    lines.push(match last {
        Some(k) if misses > 0 => format!("{hit} · misses {misses} (last {k})"),
        _ => format!("{hit} · misses {misses}"),
    });
    lines.push(format!(
        "TTL {ttl} ({})",
        if state.cache.from_shim {
            "status line"
        } else if state.agg.observed_ttl.is_some() {
            "observed"
        } else {
            "assumed"
        }
    ));
    lines.push(format!(
        "re-write if cold {}{}",
        fmt::tokens(rewrite),
        state
            .cost
            .pricing()
            .price(state.model().unwrap_or(""))
            .map(|p| format!(" ≈{}", usd_short(rewrite as f64 * p.cache_write_5m() / 1e6)))
            .unwrap_or_default()
    ));
    Light {
        id: "cache",
        level,
        glyph: level.glyph(),
        number,
        figure,
        unit,
        text,
        alt: None,
        source: if state.cache.from_shim {
            "status line"
        } else {
            "transcript"
        },
        approx: !state.cache.from_shim,
        lines,
    }
}

pub fn limits_light(state: &State) -> Light {
    let now = state.clock_ms();
    let hit = state.rate_limit_hit();
    let spend = state
        .status_facts
        .spend_limit_pct
        .is_some_and(|p| p >= 100.0);
    let Some(l) = state.limits.as_ref() else {
        let level = if hit.is_some() || spend {
            Level::Act
        } else {
            Level::Quiet
        };
        return Light {
            id: "limits",
            level,
            glyph: level.glyph(),
            number: "—".into(),
            figure: None,
            unit: "%",
            text: match &hit {
                Some((kind, _, _)) => format!("— rate limited ({})", kind.replace('_', " ")),
                None if spend => "— spend limit hit".into(),
                None => "— no status line".into(),
            },
            alt: None,
            source: "status line",
            approx: false,
            lines: vec!["install the status line shim for the limits".into()],
        };
    };
    let before_reset = match (l.exhaustion_ms, l.five_hour_resets_at_ms) {
        (Some(ex), Some(reset)) => ex < reset,
        _ => false,
    };
    let level = if hit.is_some() || spend {
        Level::Act
    } else if before_reset || l.five_hour_pct >= 80.0 {
        Level::Watch
    } else {
        Level::Quiet
    };
    let mut parts = vec![format!(
        "5h {:.0}%{}",
        l.five_hour_pct,
        l.five_hour_resets_at_ms
            .map(|r| format!(" · ↻ {}", short_duration(r - now)))
            .unwrap_or_default()
    )];
    if l.seven_day_pct >= 60.0 {
        parts.push(format!("7d {:.0}%", l.seven_day_pct));
    }
    if let Some(a) = agents_cell(state) {
        parts.push(a);
    }
    if let Some((kind, _, _)) = &hit {
        parts.insert(0, format!("rate limited ({})", kind.replace('_', " ")));
    }
    let mut lines = vec![format!(
        "7d {:.0} %{} · other sessions {}",
        l.seven_day_pct,
        l.seven_day_resets_at_ms
            .map(|r| format!(" ↻ {}", short_duration(r - now)))
            .unwrap_or_default(),
        state.other_live_sessions
    )];
    lines.push(match l.exhaustion_ms {
        Some(ex) if before_reset => format!(
            "exhausted in {} — before the reset",
            short_duration(ex - now)
        ),
        Some(ex) => format!(
            "exhausted in {} — after the reset",
            short_duration(ex - now)
        ),
        None => "no exhaustion fit yet".into(),
    });
    let ran = state.agents.len();
    let failed = state
        .agents
        .values()
        .filter(|a| a.state(now) == crate::agents::State::Failed)
        .count();
    lines.push(match state.agents_cost() {
        Some((usd, _)) => format!("agents ≈{} · {ran} ran · {failed} failed", usd_short(usd)),
        None => format!("agents {ran} ran · {failed} failed"),
    });
    Light {
        id: "limits",
        level,
        glyph: level.glyph(),
        number: format!("{:.0}%", l.five_hour_pct),
        figure: Some(l.five_hour_pct.round()),
        unit: "%",
        text: parts.join(" · "),
        alt: None,
        source: "status line",
        approx: false,
        lines,
    }
}

/// Trailing consecutive failed calls of the current turn (denials
/// excluded): `(count, the failing command prefix)`.
fn fail_streak(state: &State) -> (usize, String) {
    let turn = state.agg.current_turn().map(|t| t.number).unwrap_or(0);
    let mut n = 0;
    let mut prefix = String::new();
    for c in state.tools.calls.iter().rev() {
        if c.turn != turn || c.finished_at.is_none() {
            if c.turn != turn {
                break;
            }
            continue;
        }
        if c.is_error && c.error_class != Some(crate::tools::ErrorClass::Denied) {
            n += 1;
            if prefix.is_empty() {
                prefix = c
                    .input_summary
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
            }
        } else {
            break;
        }
    }
    (n, fmt::clip(&prefix, 16))
}

/// Corrections in the last three human turns: interrupts and rejected calls.
fn corrections(state: &State) -> usize {
    let since = state.agg.turns.len().saturating_sub(3);
    let interrupts = state
        .agg
        .interrupts
        .iter()
        .filter(|(turn, _)| *turn > since)
        .count();
    let rejected = state
        .tools
        .calls
        .iter()
        .filter(|c| c.turn > since && c.error_class == Some(crate::tools::ErrorClass::UserRejected))
        .count();
    interrupts + rejected
}

pub fn rework_light(state: &State) -> Light {
    let now = state.clock_ms();
    let edits = state.edits_since_check();
    let check = state.last_check();
    // The first source edit since the last check, for the "unverified" age.
    let since_check = check.as_ref().map(|(_, _, at)| *at).unwrap_or(i64::MIN);
    let first_edit_at = state
        .tools
        .calls
        .iter()
        .filter(|c| {
            c.class == crate::phase::ToolClass::Implement
                && c.started_at.is_some_and(|s| s > since_check)
        })
        .filter_map(|c| c.started_at)
        .min();
    let calls_since_edit = first_edit_at
        .map(|at| {
            state
                .tools
                .calls
                .iter()
                .filter(|c| c.started_at.is_some_and(|s| s > at))
                .count()
        })
        .unwrap_or(0);
    let unverified_ms = first_edit_at.map(|at| now - at).unwrap_or(0);
    let (fails, fail_prefix) = fail_streak(state);
    let corrections = corrections(state);
    let blocked = state.agg.current_turn().map(|t| t.denials).unwrap_or(0);
    let destructive =
        state.session.git_dirty
            && state.tools.calls.iter().rev().take(3).any(|c| {
                c.name == "Bash" && crate::phase::destructive_git(&c.input_summary).is_some()
            });
    let commit_unchecked = state.last_commit().is_some_and(|(at, _, _)| {
        at > since_check && edits > 0 && first_edit_at.is_some_and(|f| f < at)
    });
    let pr_open_unreviewed = state.status_facts.pr_number.is_some()
        && state.status_facts.pr_review_state.is_none()
        && !state
            .prefix
            .invoked_skills
            .iter()
            .any(|s| s.contains("review"));
    let level = if fails >= 3 || blocked >= 2 || corrections >= 2 || destructive || commit_unchecked
    {
        Level::Act
    } else if (edits > 0 && (unverified_ms > 600_000 || calls_since_edit >= 14))
        || fails >= 2
        || pr_open_unreviewed
        || state.last_commit().is_some_and(|(_, _, e)| e >= 10)
    {
        Level::Watch
    } else {
        Level::Quiet
    };
    let mut parts: Vec<String> = Vec::new();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            many.to_string()
        }
    };
    if fails > 0 {
        parts.push(format!(
            "{fails} {} ▸{fail_prefix}",
            plural(fails, "fail", "fails")
        ));
    }
    if corrections > 0 {
        parts.push(format!(
            "{corrections} {}",
            plural(corrections, "correction", "corrections")
        ));
    }
    if blocked > 0 {
        parts.push(format!("{blocked} blocked"));
    }
    if destructive {
        parts.push("destructive git on a dirty tree".into());
    }
    let check_cell = match &check {
        Some((cmd, ok, at)) if since_check > i64::MIN && edits == 0 => {
            format!("✓ {} {} ago", fmt::clip(cmd, 12), ago(now - *at))
                .replace("✓ ", if *ok { "✓ " } else { "✗ " })
        }
        _ if edits > 0 => format!("✓ none {}", ago(unverified_ms)),
        _ => "✓ none".to_string(),
    };
    parts.push(format!("edits {edits} {check_cell}"));
    match (state.uncommitted, state.last_commit()) {
        (_, Some((at, _, _))) if commit_unchecked => {
            parts.push(format!("commit {} unchecked", ago(now - at)));
        }
        (Some((_, _, 0)), Some((at, _, _))) => {
            parts.push(format!("clean · commit {}", ago(now - at)));
        }
        (Some((a, d, n)), _) if n > 0 => parts.push(format!("+{a}/−{d}")),
        (None, Some((at, _, _))) => parts.push(format!("commit {}", ago(now - at))),
        _ => {}
    }
    let issues = fails + corrections + blocked;
    let number = if issues > 0 {
        issues.to_string()
    } else {
        edits.to_string()
    };
    let mut lines = vec![match &check {
        Some((cmd, ok, at)) => format!(
            "last check `{cmd}` {} {} ago",
            if *ok { "ok" } else { "failed" },
            ago(now - *at)
        ),
        None => "no check yet this session".into(),
    }];
    let errors: Vec<String> = state
        .tools
        .errors_by_class()
        .iter()
        .take(3)
        .map(|(k, n)| format!("{} {n}", k.label()))
        .collect();
    lines.push(if errors.is_empty() {
        "no failed calls".into()
    } else {
        format!("fails: {}", errors.join(" · "))
    });
    let (checkpoints, bash_writes) = state.rewind_points();
    lines.push(match state.uncommitted {
        Some((a, d, n)) => format!(
            "uncommitted +{a} −{d} · {n} files · rewind {checkpoints} checkpoints{}",
            if bash_writes > 0 {
                format!(" ({bash_writes} bash writes uncovered)")
            } else {
                String::new()
            }
        ),
        None => format!("rewind {checkpoints} checkpoints this turn"),
    });
    Light {
        id: "rework",
        level,
        glyph: level.glyph(),
        number,
        figure: Some(if issues > 0 { issues } else { edits } as f64),
        unit: "",
        text: parts.join(" · "),
        alt: None,
        source: "transcript",
        approx: false,
        lines,
    }
}

/// The state line: phase word, then the tokens the person reads next.
pub fn state_line(state: &State, mode: SessionMode) -> StateLine {
    let now = state.clock_ms();
    let turn = state.agg.current_turn();
    let mut tokens: Vec<String> = Vec::new();
    let kind: String;
    if mode == SessionMode::Loop {
        let fed = state.agg.turns.iter().filter(|t| t.hook_blocked).count();
        let avg = state
            .agg
            .turns
            .iter()
            .filter(|t| t.prompt_chars > 0)
            .map(|t| t.prompt_chars / 4)
            .sum::<usize>()
            .checked_div(
                state
                    .agg
                    .turns
                    .iter()
                    .filter(|t| t.prompt_chars > 0)
                    .count(),
            )
            .unwrap_or(0);
        kind = "LOOP".into();
        tokens.push(format!("stop hook re-fed prompt {fed}× (~{avg} tok each)"));
    } else if let Some(w) = state.waiting() {
        kind = "◆ WAITING".into();
        tokens.push(short_duration(now - w.since_ms));
        tokens.push(match w.kind {
            WaitingKind::Permission => "permission prompt open".into(),
            WaitingKind::Question => "Claude asked a question".into(),
            WaitingKind::Notification => "Claude needs your input".into(),
            WaitingKind::Asked => "Claude asked, no reply yet".into(),
        });
    } else if turn_running(state) {
        let phase = state
            .tools
            .phase_now()
            .map(|(p, _)| match p {
                // Never an inferred VERIFYING word: the check is quoted below.
                Phase::Verifying => "RUNNING",
                p => p.word(),
            })
            .unwrap_or("THINKING");
        kind = phase.to_string();
        if let Some(f) = state
            .files
            .files
            .values()
            .filter(|f| f.edits_this_turn >= 6)
            .max_by_key(|f| f.edits_this_turn)
        {
            let name = std::path::Path::new(&f.path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            tokens.push(format!("{name} ×{}", f.edits_this_turn));
        }
        if let Some(t) = turn {
            let run = t.calls_since_text;
            if run > 0 {
                let added: u64 = state
                    .tools
                    .calls
                    .iter()
                    .rev()
                    .take(run)
                    .map(|c| c.result_tokens_est)
                    .sum();
                tokens.push(format!("{run}c +{}", fmt::tokens(added)));
            }
            // Silence: since the model last wrote prose, or since the
            // person last spoke (the prompt, an answer), whichever is later.
            let silent_since = [
                t.last_text_at.as_deref(),
                t.last_human_input_at.as_deref(),
                t.started_at.as_deref(),
            ]
            .into_iter()
            .flatten()
            .filter_map(crate::metrics::cost::parse_ts_ms)
            .max();
            if let Some(s) = silent_since.filter(|s| now - s >= 60_000) {
                tokens.push(format!("silent {}", short_duration(now - s)));
            }
            if t.silent_reminders > 0 {
                tokens.push(format!("nudged {}×", t.silent_reminders));
            }
            if (1..=5).contains(&run) {
                tokens.push("▸ steer window".into());
            }
        }
    } else {
        kind = "IDLE".into();
        if let Some(t) = turn {
            if let Some(end) = t
                .last_at
                .as_deref()
                .and_then(crate::metrics::cost::parse_ts_ms)
            {
                tokens.push(ago(now - end));
            }
        }
        if let Some((cmd, ok, at)) = state.last_check() {
            tokens.push(format!(
                "{} {} {} ago",
                fmt::clip(&cmd, 14),
                if ok { "ok" } else { "failed" },
                ago(now - at)
            ));
        }
        if let Some((at, _, _)) = state.last_commit() {
            tokens.push(format!("committed {} ago", ago(now - at)));
        }
    }
    let line = if tokens.is_empty() {
        kind.clone()
    } else {
        let first_sep = if kind.starts_with("◆") || kind == "IDLE" {
            " "
        } else {
            " · "
        };
        format!("{kind}{first_sep}{}", tokens.join(" · "))
    };
    StateLine {
        kind,
        line: cut(&line),
        tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::tests_support::state_with_turns;
    use crate::metrics::Pricing;
    use crate::transcript::Line;

    fn fixture_b() -> (State, Engine) {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut s = State::new(Pricing::bundled());
        s.session = crate::ui::state::SessionInfo::from_fixture(&path);
        for l in crate::transcript::parse_file(&path).unwrap() {
            s.apply(&l);
        }
        s.session.ended_at_ms = s.last_line_at_ms;
        let e = Engine::for_state(&s);
        (s, e)
    }

    /// The first turn's dim line: `/insights` medians for the cwd and the
    /// previous session here; gone from the second human turn; never for a
    /// fixture (no pid), never the never-display text.
    #[test]
    fn start_line_on_the_first_turn_from_insights_and_the_previous_session() {
        let dir = crate::insights::tests::dir("coach");
        let mut s = state_with_turns(1);
        s.session.cwd = std::path::PathBuf::from("/p");
        s.refresh_insights(&dir);
        assert!(
            s.insights.is_none(),
            "a fixture has no pid: nothing is read"
        );
        s.session.pid = Some(1);
        s.refresh_insights(&dir);
        let mtime = s.insights_mtime;
        assert!(mtime.is_some() && s.insights.is_some());
        s.apply_claude_home(&serde_json::json!({
            "projects": {"/p": {"lastCost": 0.16, "lastDuration": 75000, "lastLinesAdded": 3, "lastLinesRemoved": 1, "lastSessionFirstPrompt": "NEVER-SHOWN"}}
        }));
        s.now_ms = s.insights.as_ref().unwrap().computed_at_ms + 86_400_000;
        let line = s.start_line().expect("first turn");
        assert!(line.starts_with("insights "), "{line}");
        assert!(line.ends_with(" · last session $0.16 1:15 +3/−1"), "{line}");
        assert!(!line.contains("NEVER-SHOWN"));
        let e = Engine::for_state(&s);
        let c = snapshot(&s, &e);
        assert_eq!(c.start_line.as_deref(), Some(cut(&line).as_str()));
        assert!(c.quiet_row.starts_with("  insights "), "{}", c.quiet_row);
        let d = crate::dashboard::snapshot(&s, &e);
        assert!(d.start_line.is_some() && d.nudge.is_none());
        // The privacy contract across every consumer: no never-display
        // field of `usage-data` or `~/.claude.json` reaches a query, the
        // report, the export or a rendered panel.
        let mut outputs = vec![
            crate::query::summary(&s).to_string(),
            crate::query::coach(&s, None).to_string(),
            crate::query::dashboard(&s).to_string(),
            crate::query::advice(&s).to_string(),
            crate::query::agents(&s).to_string(),
            crate::query::prefix(&s).to_string(),
            crate::report::markdown(&s, s.baseline.as_ref()),
            crate::report::export_json(&s).to_string(),
        ];
        let mut app = crate::app::App::new(
            crate::ui::panels::all(),
            Box::new(|l, st: &mut State| st.apply(l)),
        );
        app.state = s;
        app.caps = crate::theme::Caps::full();
        app.set_theme("default-dark");
        outputs.push(crate::app::render_to_string(&app, 120, 40));
        app.state.view = crate::ui::state::View::Coach;
        outputs.push(crate::app::render_to_string(&app, 56, 20));
        for p in 1..=9 {
            app.state.open = Some(p);
            outputs.push(crate::app::render_to_string(&app, 100, 30));
        }
        for out in &outputs {
            assert!(!out.contains("NEVER-SHOWN"), "{out}");
        }
        let mut s = app.state;
        // Unchanged files are not re-read; a second human turn ends the line.
        s.refresh_insights(&dir);
        assert_eq!(s.insights_mtime, mtime);
        let mut later = state_with_turns(2);
        later.session.pid = Some(1);
        later.session.cwd = std::path::PathBuf::from("/p");
        later.refresh_insights(&dir);
        assert_eq!(later.start_line(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_line_measures_at_most_52_cells_and_the_object_serialises() {
        let (s, e) = fixture_b();
        let c = snapshot(&s, &e);
        let mut lines = vec![
            c.state.line.clone(),
            c.next_row(),
            c.snoozed_row(),
            c.quiet_row(),
        ];
        lines.extend(c.lights.iter().map(Light::row));
        if let Some(n) = &c.nudge {
            lines.extend([n.line1.clone(), n.line2.clone(), n.evidence.clone()]);
        }
        lines.extend(c.recent.iter().map(|r| r.row.clone()));
        for l in &lines {
            assert!(
                l.chars().count() <= WIDTH,
                "{l:?} is {} cells",
                l.chars().count()
            );
        }
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["lights"].as_array().unwrap().len(), 4);
        assert_eq!(v["lights"][0]["id"], "context");
        assert_eq!(v["nudge"]["id"], "A10");
        assert_eq!(v["nudge"]["class"], "LATER");
        assert_eq!(v["session_mode"], "interactive");
        assert!(
            v["state"]["line"]
                .as_str()
                .unwrap()
                .starts_with("COMMITTING "),
            "the fixture ends mid-turn, after a commit"
        );
    }

    #[test]
    fn fixture_b_lights_and_rows() {
        let (s, e) = fixture_b();
        let c = snapshot(&s, &e);
        let rows: Vec<String> = c.lights.iter().map(Light::row).collect();
        assert_eq!(rows[0], "○ context  142k ▇▁▁▁▁▁▁▁▁▁ 14% · ≈$.03/call");
        assert_eq!(
            rows[1], "○ cache    warm 59m (1h) ≈",
            "observed 1h TTL, clock ≈"
        );
        assert_eq!(rows[2], "○ limits   — no status line");
        assert_eq!(
            rows[3], "● rework   1 correction · edits 3 ✓ none 9h00 · com…",
            "the interrupt, three unverified edits and an unchecked commit"
        );
        assert_eq!(c.next_row(), "next     —");
        assert_eq!(c.snoozed_row(), "snoozed  —");
        assert_eq!(
            c.line(80),
            "○142k ≈$.03 · ○cache 59m · ○5h — · ●corrections 1 · ▸ queue: 'run builds and test suites longer than a …"
        );
        assert_eq!(c.line(56), "○14% ○— ○59m ●1 ▸");
        assert_eq!(c.line(30), "○○○●");
        assert_eq!(c.queued, 0);
        let n = c.nudge.as_ref().unwrap();
        assert_eq!(
            n.line1,
            "▸ `cd lorem_ipsum_dolor_sit_amet…` blocked the turn…"
        );
        assert_eq!(n.evidence, "  LATER · since turn 6 · +0 queued");
        assert_eq!(
            c.lights[0].lines[1],
            "threshold 567k · 425k left · prefix 49k (35 %)"
        );
        assert_eq!(
            c.lights[1].lines,
            [
                "hit ratio 99 % · misses 0",
                "TTL 1h (observed)",
                "re-write if cold 142k ≈$.36"
            ]
        );
        assert_eq!(c.lights[3].lines[1], "fails: Denied 4 · Other 2");
        assert_eq!(
            c.state.line,
            "COMMITTING · 4c +180 · silent 3:09 · ▸ steer window"
        );
    }

    /// The six moments of fixture B, as `scripts/pane-fixtures.sh` emits
    /// them for the pane: the object the binary computes now must be the
    /// object on disk, line for line, so both surfaces draw the same text.
    #[test]
    fn fixture_b_moments_match_the_pane_fixtures() {
        const MOMENTS: &[(&str, usize, i64)] = &[
            ("explore", 214, 0),
            ("edits", 300, 0),
            ("denials", 761, 0),
            ("waiting", 788, 240),
            ("cold", 789, 0),
            ("idle", 738, 240),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("fixtures/session-b.jsonl");
        for (name, lines, plus) in MOMENTS {
            let info = crate::ui::state::SessionInfo::from_fixture(&path);
            let mut s = crate::load::state_from_prefix(&path, info, *lines);
            if *plus > 0 {
                s.clock_override = true;
                s.now_ms = s.last_line_at_ms.unwrap() + plus * 1000;
            }
            let e = Engine::for_state(&s);
            let c = snapshot(&s, &e);
            let file = root.join(format!("tests/pane/fixtures/coach-{name}.json"));
            let on_disk: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            let fresh = serde_json::to_value(&c).unwrap();
            assert_eq!(
                fresh["state"]["line"], on_disk["state"]["line"],
                "{name}: regenerate with scripts/pane-fixtures.sh fixtures/session-b.jsonl b"
            );
            for (i, l) in c.lights.iter().enumerate() {
                assert_eq!(
                    fresh["lights"][i], on_disk["lights"][i],
                    "{name} light {}",
                    l.id
                );
            }
            assert_eq!(fresh["nudge"], on_disk["nudge"], "{name} nudge");
            assert_eq!(fresh["next"], on_disk["next"], "{name} next");
        }
    }

    #[test]
    fn lights_by_level() {
        // Context: 412k on 1M with a turn running → watch; inside the warn
        // band → act; at 300k with a clean stop → act.
        let mut s = state_with_turns(3);
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:02:02Z","message":{"id":"big","model":"claude-opus-5","content":[{"type":"tool_use","id":"u","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":412000}}}"#).unwrap());
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:02:10Z").unwrap();
        let l = context_light(&s);
        assert_eq!(l.level, Level::Watch);
        assert_eq!(l.number, "412k");
        assert_eq!(l.text, "412k ▇▇▇▇▁▁▁▁▁▁ 41% · ≈$.21/call");
        assert_eq!(l.figure, Some(41.0));
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:02:11Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u","content":"x"}]}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:02:12Z","message":{"id":"end","model":"claude-opus-5","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":412000}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:02:13Z","durationMs":8000}"#).unwrap());
        assert_eq!(
            context_light(&s).level,
            Level::Act,
            "a clean stop above 300k"
        );
        let mut warn = state_with_turns(1);
        warn.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"w","model":"claude-opus-5","content":[{"type":"tool_use","id":"u","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":950000}}}"#).unwrap());
        assert_eq!(
            context_light(&warn).level,
            Level::Act,
            "inside the warn band"
        );
        assert_eq!(context_light(&state_with_turns(2)).level, Level::Quiet);

        // Cache: before any call → —; warm → quiet; cold with a big re-write
        // while idle → act.
        assert_eq!(cache_light(&State::new(Pricing::bundled())).number, "—");
        let mut warm = state_with_turns(2);
        warm.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:01:30Z").unwrap();
        let l = cache_light(&warm);
        assert_eq!(l.level, Level::Quiet);
        assert!(l.text.starts_with("warm 4:31 (5m) ≈"), "{}", l.text);
        warm.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:04:30Z").unwrap();
        let l = cache_light(&warm);
        assert_eq!(
            l.level,
            Level::Watch,
            "inside the 2-minute countdown of a 5m TTL"
        );
        assert!(l.text.starts_with("cold in 1:31 (5m)"), "{}", l.text);
        let mut cold = warn;
        cold.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:00:03Z","durationMs":3000}"#).unwrap());
        cold.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T01:00:00Z").unwrap();
        let l = cache_light(&cold);
        assert_eq!(l.level, Level::Act);
        assert_eq!(l.text, "cold · 950k re-write");
        assert_eq!(l.unit, "k");

        // Limits: none → —; 80 % → watch; a 429 → act.
        let mut s = state_with_turns(2);
        assert_eq!(limits_light(&s).number, "—");
        s.limits = Some(crate::ui::state::Limits {
            five_hour_pct: 82.0,
            seven_day_pct: 61.0,
            five_hour_resets_at_ms: Some(s.now_ms + 7_800_000),
            seven_day_resets_at_ms: None,
            exhaustion_ms: None,
            exhaustion_in_active_hours: None,
        });
        let l = limits_light(&s);
        assert_eq!(l.level, Level::Watch);
        assert_eq!(l.text, "5h 82% · ↻ 2h10 · 7d 61%");
        assert_eq!(l.number, "82%");
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:01:30Z","message":{"id":"e","model":"<synthetic>","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1789244400}}"#).unwrap());
        let l = limits_light(&s);
        assert_eq!(l.level, Level::Act);
        assert!(
            l.text.starts_with("rate limited (five hour) · 5h 82%"),
            "{}",
            l.text
        );
    }

    #[test]
    fn rework_light_streaks_and_checks() {
        let tool = |id: &str, ts: &str, cmd: &str, err: bool| -> [Line; 2] {
            [
                Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{"command":"{cmd}"}}}}],"usage":{{"output_tokens":1}}}}}}"#)).unwrap(),
                Line::parse(&format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"x","is_error":{err}}}]}}}}"#)).unwrap(),
            ]
        };
        let mut s = state_with_turns(2);
        let l = rework_light(&s);
        assert_eq!(l.level, Level::Quiet);
        assert_eq!(l.text, "edits 0 ✓ none");
        assert_eq!(l.number, "0");
        // An edit, then three failures in a row: act, with the prefix.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:01:02Z","message":{"id":"e1","model":"claude-opus-5","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/src/a.rs","old_string":"a","new_string":"b"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:01:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#).unwrap());
        for (i, cmd) in ["gh pr view 12", "gh pr view 12", "git push"]
            .iter()
            .enumerate()
        {
            for l in tool(&format!("f{i}"), "2026-01-01T00:01:10Z", cmd, true) {
                s.apply(&l);
            }
        }
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:12:00Z").unwrap();
        let l = rework_light(&s);
        assert_eq!(l.level, Level::Act);
        assert_eq!(l.text, "3 fails ▸git push · edits 1 ✓ none 10m");
        assert_eq!(l.number, "3");
        assert!(l.lines[1].starts_with("fails: "), "{:?}", l.lines);
        // A passing test run clears the edits and the streak (a success ends it).
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:12:10Z","message":{"id":"t","model":"claude-opus-5","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"cargo test"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:12:40Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"test result: ok. 12 passed; 0 failed"}]},"toolUseResult":{"stdout":"test result: ok. 12 passed; 0 failed","stderr":"","interrupted":false}}"#).unwrap());
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:18:40Z").unwrap();
        let l = rework_light(&s);
        assert_eq!(l.level, Level::Quiet);
        assert_eq!(l.text, "edits 0 ✓ cargo test 6m ago");
        assert!(
            l.lines[0].starts_with("last check `cargo test` ok 6m ago"),
            "{:?}",
            l.lines
        );
        // Fourteen calls after an unverified edit: watch.
        let mut w = state_with_turns(2);
        w.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:01:02Z","message":{"id":"e1","model":"claude-opus-5","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/src/a.rs","old_string":"a","new_string":"b"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        for i in 0..14 {
            for l in tool(&format!("c{i}"), "2026-01-01T00:01:10Z", "ls", false) {
                w.apply(&l);
            }
        }
        w.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:02:00Z").unwrap();
        assert_eq!(rework_light(&w).level, Level::Watch);
    }

    #[test]
    fn state_line_forms() {
        // Running: the phase word, the silent run, the steer window.
        let mut s = state_with_turns(2);
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:02:00Z","promptId":"p9","promptSource":"typed","message":{"role":"user","content":"go"}}"#).unwrap());
        for i in 0..3 {
            s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:02:0{i}Z","message":{{"id":"r{i}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"r{i}","name":"Read","input":{{"file_path":"/p/f{i}.rs"}}}}],"usage":{{"output_tokens":1}}}}}}"#)).unwrap());
            s.apply(&Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-01-01T00:02:0{i}Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"r{i}","content":"{}"}}]}}}}"#, "x".repeat(4000))).unwrap());
        }
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:05:50Z").unwrap();
        let st = state_line(&s, SessionMode::Interactive);
        assert_eq!(st.kind, "EXPLORING");
        assert_eq!(
            st.line,
            "EXPLORING · 3c +3.0k · silent 3:50 · ▸ steer window"
        );
        // Waiting on a question.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:05:55Z","message":{"id":"q","model":"claude-opus-5","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[]}}],"usage":{"output_tokens":1}}}"#).unwrap());
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:09:55Z").unwrap();
        let st = state_line(&s, SessionMode::Interactive);
        assert_eq!(st.line, "◆ WAITING 4:00 · Claude asked a question");
        // Idle after a clean turn end.
        let mut idle = state_with_turns(2);
        idle.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:01:02Z","durationMs":2000}"#).unwrap());
        idle.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:05:02Z").unwrap();
        let st = state_line(&idle, SessionMode::Interactive);
        assert_eq!(st.line, "IDLE 4m");
        // A loop.
        let st = state_line(&idle, SessionMode::Loop);
        assert!(
            st.line.starts_with("LOOP · stop hook re-fed prompt 0× "),
            "{}",
            st.line
        );
        assert_eq!(short_duration(3_600_000 * 2 + 600_000), "2h10");
        assert_eq!(usd_short(0.21), "$.21");
        assert_eq!(usd_short(2.6), "$2.6");
        assert_eq!(usd_short(18.6), "$19");
    }
}
