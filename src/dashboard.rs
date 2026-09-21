//! The dashboard object — Console (PRD dashboard-v2 §4): one snapshot of
//! `State` and the Advisor engine that the TUI's dashboard and the pane's
//! Overview draw. A header line, six cells (the glance: each a whole-area
//! target that opens a body), the act line (the coach's slot) and eight
//! bodies, of which a surface shows the one it has open. The object is
//! colour-free (a slice carries a step, not a colour) and width-free:
//! every line is full length and each surface cuts once at its own width.

use serde::Serialize;

use crate::advisor::Engine;
use crate::coach::{self, Coach, Level};
use crate::metrics::context::Source;
use crate::ui::fmt;
use crate::ui::state::State;

/// A segment's role; every surface maps it to its own colour. `S0`–`S2`
/// are the composition ramp's steps (`Theme::series(3)`: what cannot
/// change this session, what the next boundary drops, what the person can
/// move) — the pane, which has no ramp, carries them by weight and the
/// alternating fill glyph alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Fg,
    Dim,
    Accent,
    Ok,
    Warn,
    Crit,
    Bold,
    S0,
    S1,
    S2,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Seg {
    pub text: String,
    pub tone: Tone,
}

pub type Line = Vec<Seg>;

pub fn seg(text: impl Into<String>, tone: Tone) -> Seg {
    Seg {
        text: text.into(),
        tone,
    }
}

pub fn fg(text: impl Into<String>) -> Seg {
    seg(text, Tone::Fg)
}

pub fn dim(text: impl Into<String>) -> Seg {
    seg(text, Tone::Dim)
}

/// The text of a line, joined.
pub fn text_of(line: &Line) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

/// Cells a line takes.
pub fn width_of(line: &Line) -> usize {
    line.iter().map(|s| s.text.chars().count()).sum()
}

/// Cut a line at `cells`, an ellipsis on the segment that crosses the edge.
/// The surfaces' one cut, at their own width.
pub fn cut(line: Line, cells: usize) -> Line {
    let mut out: Line = Vec::new();
    let mut used = 0;
    for s in line {
        if used >= cells {
            break;
        }
        let w = s.text.chars().count();
        if used + w <= cells {
            used += w;
            out.push(s);
        } else {
            // A figure the object already clipped (`python3 -…`) does not
            // grow a second ellipsis when the cut lands on its first.
            let text = fmt::clip(&s.text, cells - used).replace("……", "…");
            out.push(seg(text, s.tone));
            used = cells;
        }
    }
    out
}

/// ` · ` between parts, dim.
pub fn joined(parts: Vec<Line>) -> Line {
    let mut out = Vec::new();
    for (i, p) in parts.into_iter().enumerate() {
        if i > 0 {
            out.push(dim(" · "));
        }
        out.extend(p);
    }
    out
}

/// Pad `line` with dim spaces up to column `at` (a column stop).
fn at(line: &mut Line, at: usize) {
    let w = width_of(line);
    if at > w {
        line.push(dim(" ".repeat(at - w)));
    }
}

/// `text` right-aligned so it ends at column `end`.
fn rt(line: &mut Line, end: usize, text: impl Into<String>, tone: Tone) {
    let text = text.into();
    at(line, end.saturating_sub(text.chars().count()));
    line.push(seg(text, tone));
}

/// A `w`-cell bar of `pct` per cent: never empty for a non-zero share,
/// never full for one under 100 (the prototype's rule).
fn bar(pct: f64, w: usize, tone: Tone) -> Vec<Seg> {
    let mut f = ((pct / 100.0) * w as f64).round().max(0.0) as usize;
    f = f.min(w);
    if f == 0 && pct > 0.0 {
        f = 1;
    }
    if f == w && pct < 100.0 && w > 0 {
        f = w - 1;
    }
    vec![seg("▇".repeat(f), tone), dim("▁".repeat(w - f))]
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseCell {
    /// `●` while a turn runs, `○` idle, `◆` waiting.
    pub glyph: char,
    pub word: String,
    /// The turn's elapsed time, `—` before the first turn: the header's
    /// phase cell is `● WORKING 52:11`.
    pub elapsed: String,
    /// The state line's tokens after the word (the act line reads them
    /// when there is no nudge).
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Header {
    pub session: String,
    pub model: String,
    pub version: String,
    pub turn: usize,
    pub elapsed: String,
    pub cwd: String,
    pub pr: Option<u64>,
    /// `<member>@<team>` when the attached session is itself a teammate
    /// (its lines carry `agentName` / `teamName`); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    pub phase: PhaseCell,
    /// The header as one line: `cctop  claude-opus-5 · turn 14 · 1:12:08 · ~/code/cctop · PR #142`.
    pub line: String,
}

/// One cell of the glance: a whole-area target. The hotkey is the
/// engine's to draw (`1: label`); cctop draws no digit chrome. Three
/// forms by the width a surface has: `label` at ≥ 80 body columns, `mid`
/// at cells ≥ 28 wide, `short` below.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cell {
    pub key: char,
    pub id: &'static str,
    /// The body the cell opens (its id).
    pub opens: &'static str,
    /// `ctx 35% · 616k left` — segments, so a cell can be several Buttons
    /// sharing one press.
    pub label: Line,
    /// `ctx 35% 616k left`.
    pub mid: Line,
    /// `ctx 35%`.
    pub short: Line,
}

/// The act line: the coach's slot, whole. A pending wait pre-empts the
/// nudge; with neither, the state line's tokens; with nothing running,
/// the first turn's start line or the quiet row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Act {
    pub key: char,
    pub opens: &'static str,
    /// `▸ steer window — 4 calls, silent 1:56`; wrapped by the surface,
    /// never cut mid-fact.
    pub line: Line,
    /// The form for a narrow surface: `▸ steer window · 4 calls`.
    pub short: Line,
    /// `NOW · call 12` for a nudge, empty otherwise.
    pub tag: String,
    /// The nudge's rule id, when the line is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nudge: Option<&'static str>,
    /// A permission wait or a question pre-empts the nudge and paints the
    /// line `crit`.
    pub blocked: bool,
    /// The nudge's action is being carried out (`acting`).
    pub acting: bool,
}

/// One slice of a composition bar: the step is `Theme::series(3)`'s index
/// (0 fixed, 1 transient, 2 the person's), never a colour (FR-2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slice {
    pub label: &'static str,
    pub tokens: u64,
    pub step: u8,
}

/// One body: full-length rows a surface cuts at its width and draws under
/// the rule line, and the slices its bar rows were drawn from (empty when
/// the body has no bar).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Body {
    pub key: char,
    pub id: &'static str,
    pub title: &'static str,
    /// The body's own keys, for the rule line's right half.
    pub keys: &'static str,
    pub rows: Vec<Line>,
    pub slices: Vec<Slice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    pub schema: u32,
    pub header: Header,
    /// Six, keys `1`–`6`, in fixed positions (FR-4).
    pub cells: Vec<Cell>,
    pub act: Act,
    /// Eight, keys `1`–`6`, `a`, `0`; a surface shows the one it has open
    /// and the object does not know which — the TUI and the pane can each
    /// hold a different one.
    pub bodies: Vec<Body>,
    /// Nudges are shown this session (`false` on the control arm).
    pub exposed: bool,
    pub session_mode: crate::advisor::SessionMode,
    /// The coach's one-line forms, for the inline strip (L1 / L2).
    pub lines: coach::StatusLines,
    /// The session's whole spend (`cost_combined`), with its provenance.
    pub cost_combined: Option<crate::query::CostValue>,
}

/// The eight bodies' ids, in key order (`1`–`6`, `a`, `0`).
pub const BODY_IDS: [&str; 8] = [
    "context", "limits", "cache", "cost", "work", "tools", "advisor", "events",
];

/// The key that opens each body, in [`BODY_IDS`] order.
pub const BODY_KEYS: [char; 8] = ['1', '2', '3', '4', '5', '6', 'a', '0'];

/// The advisor and events bodies (`a` and `0`, the way home) as indexes.
pub const ADVISOR_BODY: usize = 6;
pub const EVENTS_BODY: usize = 7;

/// The full-screen panel behind each body (`Enter`), in [`BODY_IDS`]
/// order: the nine panels collapse into the eight bodies, and every one
/// of them stays reachable.
pub const BODY_PANELS: [u8; 8] = [1, 3, 2, 2, 7, 5, 9, 8];

/// The whole object.
pub fn snapshot(state: &State, engine: &Engine) -> Dashboard {
    let c = coach::snapshot(state, engine);
    Dashboard {
        schema: 2,
        header: header(state, &c),
        cells: cells(state, &c),
        act: act(state, &c),
        bodies: vec![
            body_context(state, &c),
            body_limits(state, &c),
            body_cache(state, &c),
            body_cost(state),
            body_work(state, &c),
            body_tools(state),
            body_advisor(&c, engine),
            body_events(state, engine),
        ],
        exposed: c.exposed,
        session_mode: engine.session_mode,
        lines: c.lines.clone(),
        cost_combined: state
            .cost_combined()
            .map(|c| crate::query::CostValue::new(c, "cost_combined")),
    }
}

fn header(state: &State, c: &Coach) -> Header {
    let s = &state.session;
    let elapsed = match (
        s.started_at_ms.or(state.agg.turns.first().and_then(|t| {
            t.started_at
                .as_deref()
                .and_then(crate::metrics::cost::parse_ts_ms)
        })),
        state.clock_ms(),
    ) {
        (Some(start), now) => fmt::duration_ms(now - start),
        _ => "—".into(),
    };
    let word = c.state.kind.trim_start_matches("◆ ").to_string();
    let glyph = if c.state.kind.starts_with('◆') {
        '◆'
    } else if c.state.kind == "IDLE" {
        '○'
    } else {
        '●'
    };
    let cwd = fmt::shorten_home(&s.cwd);
    let pr = state.status_facts.pr_number.or(state.agg.pr_number);
    let mut parts = vec![c.model.clone(), format!("turn {}", c.turn), elapsed.clone()];
    if !s.alive {
        // A dead session: when it ended, beside the elapsed time.
        parts.push(
            format!(
                "ENDED {}",
                s.ended_at_ms.map(fmt::clock_hhmm).unwrap_or_default()
            )
            .trim_end()
            .to_string(),
        );
    }
    parts.push(cwd.clone());
    let team = state
        .agg
        .team
        .as_ref()
        .map(|(team, member)| format!("{member}@{team}"));
    if let Some(t) = &team {
        parts.push(t.clone());
    }
    if let Some(n) = pr {
        parts.push(format!("PR #{n}"));
    }
    if !s.version.is_empty() {
        parts.push(format!("v{}", s.version));
    }
    Header {
        session: s.name.clone(),
        model: c.model.clone(),
        version: s.version.clone(),
        turn: c.turn,
        elapsed,
        cwd,
        pr,
        team,
        phase: PhaseCell {
            glyph,
            word,
            elapsed: state
                .agg
                .current_turn()
                .and_then(|t| t.elapsed_ms(state.clock_ms()))
                .map(fmt::duration_ms)
                .unwrap_or_else(|| "—".into()),
            tokens: c.state.tokens.clone(),
        },
        line: format!("cctop  {}", parts.join(" · ")),
    }
}

fn light<'a>(c: &'a Coach, id: &str) -> Option<&'a coach::Light> {
    c.lights.iter().find(|l| l.id == id)
}

fn level_tone(level: Level) -> Tone {
    match level {
        Level::Quiet => Tone::Fg,
        Level::Watch => Tone::Warn,
        Level::Act => Tone::Crit,
    }
}

fn step_tone(step: u8) -> Tone {
    match step {
        0 => Tone::S0,
        1 => Tone::S1,
        _ => Tone::S2,
    }
}

fn plural(n: usize, one: &str, many: &str) -> &'static str {
    // The callers pass literals; keep the signature simple.
    let _ = (one, many);
    if n == 1 {
        ""
    } else {
        "s"
    }
}

// ------------------------------------------------------------------ cells

/// A cell from its two parts: `first` is the short form, `first · second`
/// the wide one, `first second` the middle one (a bare space, as the
/// prototype compacts them).
fn cell(key: char, id: &'static str, first: Line, second: Option<Line>) -> Cell {
    let (label, mid) = match second {
        Some(s) => {
            let mut label = first.clone();
            label.push(dim(" · "));
            label.extend(s.clone());
            let mut mid = first.clone();
            mid.push(dim(" "));
            mid.extend(s);
            (label, mid)
        }
        None => (first.clone(), first.clone()),
    };
    Cell {
        key,
        id,
        opens: id,
        label,
        mid,
        short: first,
    }
}

fn cells(state: &State, c: &Coach) -> Vec<Cell> {
    let now = state.clock_ms();
    // 1 · context: the percentage in the light's level, what is left
    // before autocompact.
    let v = state.context();
    let ctx_level = light(c, "context").map(|l| l.level).unwrap_or(Level::Quiet);
    let pct = if v.window > 0 {
        format!("{:.0}%", v.ratio() * 100.0)
    } else {
        "—".into()
    };
    let left = v.threshold.saturating_sub(v.size);
    let context = cell(
        '1',
        "context",
        vec![fg("ctx "), seg(pct, level_tone(ctx_level))],
        (v.window > 0).then(|| vec![fg(format!("{} left", fmt::tokens(left)))]),
    );
    // 2 · limits: the 5-hour window and its reset.
    let limits = match &state.limits {
        Some(l) => cell(
            '2',
            "limits",
            vec![
                fg("5h "),
                seg(
                    format!("{:.0}%", l.five_hour_pct),
                    if l.five_hour_pct >= 80.0 {
                        Tone::Warn
                    } else {
                        Tone::Fg
                    },
                ),
            ],
            l.five_hour_resets_at_ms
                .map(|r| vec![fg(format!("↻ {}", coach::short_duration(r - now)))]),
        ),
        None => cell(
            '2',
            "limits",
            vec![fg("5h "), dim("—")],
            Some(vec![dim("no status line")]),
        ),
    };
    // 3 · cache: the countdown and the TTL.
    let ttl = if state.cache_ttl_ms() >= 3_600_000 {
        "1h"
    } else {
        "5m"
    };
    let cache = match state.cache_clock() {
        Some(k) if k.remaining_ms > 0 => cell(
            '3',
            "cache",
            vec![
                fg("cache "),
                seg(
                    format!(
                        "{}{}",
                        coach::short_duration(k.remaining_ms),
                        if k.approx { "≈" } else { "" }
                    ),
                    Tone::Ok,
                ),
            ],
            Some(vec![fg(format!("{ttl} TTL"))]),
        ),
        Some(_) => cell(
            '3',
            "cache",
            vec![fg("cache "), seg("cold", Tone::Warn)],
            Some(vec![fg(format!("{ttl} TTL"))]),
        ),
        None => cell('3', "cache", vec![fg("cache "), dim("—")], None),
    };
    // 4 · cost: the session's whole spend and the burn rate.
    let rates = crate::metrics::cost::rates(&state.agg, state.cost.pricing(), now);
    let cost = match state.cost_breakdown() {
        Some(b) => cell(
            '4',
            "cost",
            vec![fg(format!(
                "spend {}{}",
                if b.headline.approx { "≈" } else { "" },
                fmt::usd(b.headline.usd)
            ))],
            rates
                .usd_per_hour
                .map(|h| vec![fg(format!("≈{}/h", fmt::usd(h)))]),
        ),
        None => cell('4', "cost", vec![fg("spend "), dim("—")], None),
    };
    // 5 · work: the last check and the rework light's state.
    let check = match state.last_check() {
        Some((cmd, ok, at)) => vec![seg(
            format!(
                "{} {} {}",
                if ok { "✓" } else { "✗" },
                fmt::clip(&cmd, 10),
                coach::short_duration(now - at)
            ),
            if ok { Tone::Ok } else { Tone::Crit },
        )],
        None => vec![fg("✓ "), dim("none")],
    };
    let rework =
        light(c, "rework").map(|l| vec![fg("rework "), seg(l.number.clone(), level_tone(l.level))]);
    let work = cell('5', "work", check, rework);
    // 6 · tools: the calls and the errors.
    let calls = state.tools.calls.len();
    let errors = state.tools.calls.iter().filter(|c| c.is_error).count();
    let mut tools = cell(
        '6',
        "tools",
        vec![fg(format!("{calls} calls"))],
        (errors > 0).then(|| vec![seg(format!("{errors} err"), Tone::Warn)]),
    );
    // The short form keeps both figures: `159c 1 err`.
    if errors > 0 {
        tools.short = vec![
            fg(format!("{calls}c ")),
            seg(format!("{errors} err"), Tone::Warn),
        ];
    }
    vec![context, limits, cache, cost, work, tools]
}

// -------------------------------------------------------------------- act

fn act(state: &State, c: &Coach) -> Act {
    let quiet = |line: Line| Act {
        key: 'a',
        opens: "advisor",
        short: line.clone(),
        line,
        tag: String::new(),
        nudge: None,
        blocked: false,
        acting: false,
    };
    if !c.exposed {
        return quiet(vec![dim(
            "coach off (control arm) · fires recorded, nothing shown",
        )]);
    }
    // A wait pre-empts everything: the person is what the session waits on.
    if let Some(w) = state.waiting() {
        use crate::ui::state::WaitingKind;
        let since = coach::short_duration(state.clock_ms() - w.since_ms);
        let what = match w.kind {
            WaitingKind::Permission => "permission prompt open",
            WaitingKind::Question => "Claude asked a question",
            WaitingKind::Notification => "Claude needs your input",
            WaitingKind::Asked => "Claude asked, no reply yet",
        };
        return Act {
            key: 'a',
            opens: "advisor",
            line: vec![
                seg("◆ ", Tone::Crit),
                seg(format!("WAITING {since}"), Tone::Crit),
                dim(" — "),
                fg(what),
            ],
            short: vec![
                seg("◆ ", Tone::Crit),
                seg(format!("WAITING {since}"), Tone::Crit),
            ],
            tag: String::new(),
            nudge: None,
            blocked: true,
            acting: false,
        };
    }
    if let Some(n) = &c.nudge {
        let headline = n.line1.trim_start_matches("▸ ").trim().to_string();
        let action = n.line2.trim().to_string();
        let tone = match n.class {
            crate::advisor::Urgency::Now => Tone::Crit,
            crate::advisor::Urgency::Next => Tone::Warn,
            crate::advisor::Urgency::Later => Tone::Fg,
        };
        let tag = n
            .evidence
            .trim()
            .split(" · ")
            .take(2)
            .collect::<Vec<_>>()
            .join(" · ")
            .replace("fired at call", "call")
            .replace("since turn", "turn");
        let mut line = vec![seg("▸ ", tone), seg(headline.clone(), Tone::Bold)];
        if !action.is_empty() {
            line.push(dim(" — "));
            line.push(fg(action));
        }
        return Act {
            key: 'a',
            opens: "advisor",
            line,
            short: vec![seg("▸ ", tone), seg(headline, Tone::Bold)],
            tag,
            nudge: Some(n.id),
            blocked: false,
            acting: n.acting,
        };
    }
    // No nudge: while a turn runs, the state line's tokens, the steer
    // window first when it is open (`▸ steer window — 4 calls, silent
    // 1:56`); idle, the start line or the quiet row.
    let tokens = &c.state.tokens;
    if !tokens.is_empty() && c.state.kind != "IDLE" && c.state.kind != "LOOP" {
        let marker = tokens.iter().find(|t| t.starts_with('▸'));
        let rest: Vec<&str> = tokens
            .iter()
            .filter(|t| !t.starts_with('▸'))
            .map(String::as_str)
            .collect();
        return match marker {
            Some(m) => {
                let m = m.trim_start_matches("▸ ");
                let mut line = vec![seg("▸ ", Tone::Ok), seg(m, Tone::Bold)];
                if !rest.is_empty() {
                    line.push(dim(format!(" — {}", rest.join(", "))));
                }
                let mut short = vec![seg("▸ ", Tone::Ok), seg(m, Tone::Bold)];
                if let Some(r) = rest.first() {
                    short.push(dim(format!(" · {r}")));
                }
                Act {
                    short,
                    ..quiet(line)
                }
            }
            None => {
                let line = vec![dim("· "), fg(rest.join(" · "))];
                let short = vec![dim("· "), fg(rest.first().copied().unwrap_or(""))];
                Act {
                    short,
                    ..quiet(line)
                }
            }
        };
    }
    match &c.start_line {
        Some(l) => quiet(vec![dim(l.clone())]),
        None => quiet(vec![dim("quiet · nothing to act on")]),
    }
}

// ----------------------------------------------------------------- bodies

/// The light's three detail lines, dim, after a blank row.
fn light_lines(rows: &mut Vec<Line>, c: &Coach, id: &str) {
    if let Some(l) = light(c, id) {
        rows.push(Vec::new());
        for line in &l.lines {
            rows.push(vec![dim(format!("  {line}"))]);
        }
    }
}

/// The context body: the window, the five slices by agency with the
/// series step each is drawn in, the counters.
fn body_context(state: &State, c: &Coach) -> Body {
    let v = state.context();
    // One residency for the whole body: the slices are derived from it and
    // the source rows below read it again.
    let r = state.residency();
    let a = crate::metrics::context::Anatomy::from(&r);
    let est = if v.window_exact { "" } else { " est" };
    let approx = if a.approx { "≈" } else { "" };
    let mut rows: Vec<Line> = Vec::new();
    let mut top = vec![
        seg(format!("  {}", fmt::tokens(v.size)), Tone::Bold),
        dim(" of "),
        fg(format!("{}{est}", fmt::tokens(v.window))),
        dim("   autocompact "),
        fg(fmt::tokens(v.threshold)),
        dim("   "),
        seg(
            fmt::tokens(v.threshold.saturating_sub(v.size)),
            level_tone(light(c, "context").map(|l| l.level).unwrap_or(Level::Quiet)),
        ),
        dim(" left"),
    ];
    match v.velocity {
        Some(vel) => top.push(dim(format!(
            "   +{}/turn",
            fmt::tokens(vel.max(0.0) as u64)
        ))),
        None => top.push(dim("   velocity — (one turn)")),
    }
    rows.push(top);
    rows.push(Vec::new());
    let slices = vec![
        Slice {
            label: "prefix",
            tokens: a.prefix,
            step: 0,
        },
        Slice {
            label: "harness",
            tokens: a.harness,
            step: 0,
        },
        Slice {
            label: "thinking",
            tokens: a.thinking,
            step: 1,
        },
        Slice {
            label: "inputs",
            tokens: a.tool_inputs,
            step: 2,
        },
        Slice {
            label: "results",
            tokens: a.tool_results,
            step: 2,
        },
    ];
    let max = slices.iter().map(|s| s.tokens).max().unwrap_or(0).max(1);
    const BAR: usize = 14;
    for (i, s) in slices.iter().enumerate() {
        let mut l: Line = Vec::new();
        at(&mut l, 2);
        l.push(fg(s.label));
        rt(
            &mut l,
            16,
            format!("{approx}{}", fmt::tokens(s.tokens)),
            Tone::Bold,
        );
        at(&mut l, 18);
        let f = (((s.tokens as f64 / max as f64) * BAR as f64).round() as usize).min(BAR);
        let glyph = if i % 2 == 0 { "▇" } else { "▆" };
        l.push(seg(glyph.repeat(f), step_tone(s.step)));
        l.push(dim("▁".repeat(BAR - f)));
        rows.push(l);
    }
    // What the `results` slice is made of, plus what no slice names (the
    // prompts, the remainder): the `m` inspector's rows, cut to the dock.
    // The same residency model, so the two surfaces never disagree.
    let mut src: Vec<(&str, u64)> = Source::ALL[..6]
        .iter()
        .map(|s| (s.label(), r.source(*s)))
        .collect();
    src.push((Source::Prompts.label(), r.source(Source::Prompts)));
    src.push(("other", r.other));
    if src.iter().any(|(_, t)| *t > 0) {
        rows.push(Vec::new());
        rows.push(vec![dim(format!(
            "  since {} · {} call{}",
            crate::ui::sources_view::since_text(&r),
            r.calls_since,
            if r.calls_since == 1 { "" } else { "s" }
        ))]);
        for (label, tokens) in src {
            if tokens == 0 && label != "other" {
                continue;
            }
            let mut l: Line = Vec::new();
            at(&mut l, 2);
            l.push(fg(label));
            // The inspector's own columns, narrowed for the dock: the
            // longest label ("agent returns") ends at 15, so the figure
            // still has air at 24.
            rt(
                &mut l,
                24,
                format!("{approx}{}", fmt::tokens(tokens)),
                Tone::Bold,
            );
            if let Some(pct) = (tokens * 100).checked_div(r.size) {
                rt(&mut l, 29, format!("{pct}%"), Tone::Dim);
            }
            rows.push(l);
        }
        rows.push(vec![dim(format!(
            "  {}",
            crate::ui::sources_view::mode_text(&r)
        ))]);
    }

    rows.push(Vec::new());
    let rereads = state
        .files
        .files
        .values()
        .filter(|f| f.reread_warning())
        .count();
    let stale = state.files.files.values().filter(|f| f.stale).count();
    let mut counters = vec![
        dim("  compactions "),
        fg(v.compactions.len().to_string()),
        dim("   re-reads "),
        seg(
            rereads.to_string(),
            if rereads > 0 { Tone::Warn } else { Tone::Fg },
        ),
        dim("   stale "),
        seg(
            stale.to_string(),
            if stale > 0 { Tone::Warn } else { Tone::Fg },
        ),
    ];
    if let Some(n) = v.turns_until_compaction() {
        counters.push(dim("   autocompact in "));
        counters.push(fg(format!("~{} turns", n.ceil() as u64)));
    }
    rows.push(counters);
    light_lines(&mut rows, c, "context");
    Body {
        key: '1',
        id: "context",
        title: "context",
        keys: "Enter panel",
        rows,
        slices,
    }
}

/// The limits body: the two windows as meters, then what the light says.
fn body_limits(state: &State, c: &Coach) -> Body {
    let now = state.clock_ms();
    let mut rows: Vec<Line> = Vec::new();
    let meter = |label: &str, pct: f64, note: String| -> Line {
        let mut l: Line = Vec::new();
        at(&mut l, 2);
        l.push(fg(label));
        rt(&mut l, 18, format!("{pct:.0} %"), Tone::Bold);
        at(&mut l, 20);
        l.extend(bar(
            pct,
            20,
            if pct >= 80.0 { Tone::Warn } else { Tone::Ok },
        ));
        at(&mut l, 42);
        l.push(dim(note));
        l
    };
    match &state.limits {
        Some(l) => {
            rows.push(meter(
                "5 hours",
                l.five_hour_pct,
                l.five_hour_resets_at_ms
                    .map(|r| format!("resets {}", coach::short_duration(r - now)))
                    .unwrap_or_default(),
            ));
            rows.push(meter(
                "7 days",
                l.seven_day_pct,
                l.seven_day_resets_at_ms
                    .map(|r| format!("resets {}", coach::short_duration(r - now)))
                    .unwrap_or_default(),
            ));
            match (l.exhaustion_ms, l.five_hour_resets_at_ms) {
                (Some(ex), Some(reset)) if ex < reset => rows.push(vec![seg(
                    format!(
                        "  exhausted in {} — before the reset",
                        coach::short_duration(ex - now)
                    ),
                    Tone::Warn,
                )]),
                (Some(ex), _) => rows.push(vec![dim(format!(
                    "  exhausted in {}",
                    coach::short_duration(ex - now)
                ))]),
                _ => {}
            }
        }
        None => rows.push(vec![
            dim("  no status line — "),
            fg("cctop install"),
            dim(" adds the exact 5-hour and 7-day figures"),
        ]),
    }
    if let Some((kind, _, _)) = state.rate_limit_hit() {
        rows.push(vec![seg(
            format!("  rate limited ({})", kind.replace('_', " ")),
            Tone::Crit,
        )]);
    }
    rows.push(Vec::new());
    if let Some(m) = state.model() {
        let tier = crate::harness_facts::usage_weight::tier(m);
        rows.push(vec![
            dim("  weight   "),
            seg(
                format!("×{tier:.0}"),
                if tier > 1.0 { Tone::Warn } else { Tone::Fg },
            ),
            dim(format!(
                " {} — why the bar moves faster than the dollars",
                fmt::model_short(m).split('-').next().unwrap_or("")
            )),
        ]);
    }
    let flags = state.behaviour_flags();
    if flags.long_context_count > 0 {
        rows.push(vec![
            dim("  long ctx "),
            seg(format!("{:.0} %", flags.long_context_pct), Tone::Warn),
            dim(" of usage was above 150k context"),
        ]);
    }
    if let Some(p) = state.status_facts.spend_limit_pct {
        rows.push(vec![dim("  spend limit "), fg(format!("{p:.0} %"))]);
    }
    if state.other_live_sessions > 0 {
        rows.push(vec![dim(format!(
            "  {} other live session{}: account-wide, they draw from the same pool",
            state.other_live_sessions,
            plural(state.other_live_sessions, "", "s")
        ))]);
    } else {
        rows.push(vec![dim(
            "  account-wide: other sessions draw from the same pool",
        )]);
    }
    for tip in flags.tips().into_iter().take(1) {
        rows.push(vec![dim(format!("  {tip}"))]);
    }
    light_lines(&mut rows, c, "limits");
    Body {
        key: '2',
        id: "limits",
        title: "limits",
        keys: "Enter panel",
        rows,
        slices: Vec::new(),
    }
}

/// The cache body: the countdown, the misses and the hit ratio, the
/// re-write at stake, then the light's three lines.
fn body_cache(state: &State, c: &Coach) -> Body {
    let ttl = if state.cache_ttl_ms() >= 3_600_000 {
        "1h"
    } else {
        "5m"
    };
    let mut rows: Vec<Line> = Vec::new();
    rows.push(match state.cache_clock() {
        Some(k) if k.remaining_ms > 0 => vec![
            seg("  warm ", Tone::Ok),
            seg(coach::short_duration(k.remaining_ms), Tone::Ok),
            dim(format!("{} left of a ", if k.approx { " ≈" } else { "" })),
            fg(ttl),
            dim(" entry"),
        ],
        Some(_) => vec![
            seg("  cold", Tone::Warn),
            dim(" — the next call re-writes the context at the write price"),
        ],
        None => vec![dim("  no cache reading yet")],
    });
    rows.push(Vec::new());
    let misses = state.agg.cache_misses.len() as u64 + state.cache.misses;
    rows.push(vec![
        dim("  misses      "),
        seg(
            misses.to_string(),
            if misses > 0 { Tone::Warn } else { Tone::Ok },
        ),
        dim(state
            .agg
            .cache_misses
            .last()
            .map(|m| format!("   last {} ({})", m.kind, fmt::tokens(m.tokens)))
            .unwrap_or_default()),
    ]);
    rows.push(match state.agg.total.cache_hit_ratio() {
        Some(r) => vec![
            dim("  hit ratio   "),
            seg(
                format!("{:.0} %", r * 100.0),
                if r >= 0.8 {
                    Tone::Ok
                } else if r >= 0.5 {
                    Tone::Warn
                } else {
                    Tone::Crit
                },
            ),
        ],
        None => vec![dim("  hit ratio   —")],
    });
    rows.push(vec![
        dim("  re-write    "),
        seg(fmt::tokens(state.context().size), Tone::Warn),
        dim(" if it goes cold before the next call"),
    ]);
    light_lines(&mut rows, c, "cache");
    rows.push(Vec::new());
    rows.push(vec![dim(
        "  reply before the countdown ends, or the next call pays the re-write",
    )]);
    Body {
        key: '3',
        id: "cache",
        title: "cache",
        keys: "Enter panel",
        rows,
        slices: Vec::new(),
    }
}

/// The cost body: the spend with its provenance, the token mix as bars,
/// the rates, who spent it.
fn body_cost(state: &State) -> Body {
    let now = state.clock_ms();
    let u = state.agg.total;
    let mut rows: Vec<Line> = Vec::new();
    match state.cost_breakdown() {
        Some(b) => rows.push(vec![
            seg(
                format!(
                    "  {}{}",
                    if b.headline.approx { "≈" } else { "" },
                    fmt::usd(b.headline.usd)
                ),
                Tone::Bold,
            ),
            seg(
                "   API-equivalent list price — a subscription is not billed per token",
                Tone::Warn,
            ),
        ]),
        None => rows.push(vec![dim("  cost — (no price for this model)")]),
    }
    rows.push(Vec::new());
    let slices = vec![
        Slice {
            label: "cache read",
            tokens: u.cache_read,
            step: 2,
        },
        Slice {
            label: "cache write",
            tokens: u.cache_write(),
            step: 1,
        },
        Slice {
            label: "output",
            tokens: u.output,
            step: 2,
        },
        Slice {
            label: "└ thinking",
            tokens: u.thinking,
            step: 1,
        },
        Slice {
            label: "fresh input",
            tokens: u.input,
            step: 0,
        },
    ];
    let max = slices.iter().map(|s| s.tokens).max().unwrap_or(0).max(1);
    const BAR: usize = 18;
    for s in &slices {
        let mut l: Line = Vec::new();
        at(&mut l, 2);
        l.push(fg(s.label));
        at(&mut l, 16);
        let f = (((s.tokens as f64 / max as f64) * BAR as f64).round() as usize)
            .clamp(usize::from(s.tokens > 0), BAR);
        l.push(seg("▇".repeat(f), step_tone(s.step)));
        l.push(dim("▁".repeat(BAR - f)));
        at(&mut l, 16 + BAR + 2);
        l.push(seg(fmt::tokens(s.tokens), Tone::Bold));
        rows.push(l);
    }
    rows.push(Vec::new());
    let rates = crate::metrics::cost::rates(&state.agg, state.cost.pricing(), now);
    let mut line: Line = Vec::new();
    match u.cache_hit_ratio() {
        Some(r) => {
            line.push(dim("  cache hit "));
            line.push(seg(
                format!("{:.0} %", r * 100.0),
                if r >= 0.8 {
                    Tone::Ok
                } else if r >= 0.5 {
                    Tone::Warn
                } else {
                    Tone::Crit
                },
            ));
        }
        None => line.push(dim("  cache hit —")),
    }
    let misses = state.agg.cache_misses.len() as u64 + state.cache.misses;
    line.push(dim("   misses "));
    line.push(seg(
        misses.to_string(),
        if misses > 0 { Tone::Warn } else { Tone::Ok },
    ));
    if let Some(h) = rates.usd_per_hour {
        line.push(dim("   burn "));
        line.push(seg(format!("≈{}/h", fmt::usd(h)), Tone::Bold));
    }
    if let Some(g) = state.gradient() {
        line.push(dim("   $/call "));
        line.push(fg(format!(
            "≈{}{}",
            coach::usd_short(g.per_call),
            if g.cold { " cold" } else { "" }
        )));
        line.push(dim("   next 30c "));
        line.push(fg(format!("≈{}", coach::usd_short(g.next_30_calls))));
    }
    rows.push(line);
    if let Some(b) = state.cost_breakdown() {
        let mut parts: Vec<Line> = Vec::new();
        if let Some(ledger) = b.ledger {
            parts.push(vec![dim("ledger "), fg(fmt::usd(ledger))]);
        }
        if let Some(since) = b.since {
            parts.push(vec![dim("since "), fg(format!("≈{}", fmt::usd(since.usd)))]);
        }
        if let Some(a) = b.agents {
            parts.push(vec![
                dim("agents "),
                fg(format!(
                    "≈{} ({:.0} %)",
                    fmt::usd(a.usd),
                    b.agents_share * 100.0
                )),
            ]);
        }
        if let Some(t) = b.team {
            parts.push(vec![
                dim("team "),
                fg(format!(
                    "{}{} ({:.0} %, {} of {} read)",
                    if t.approx { "≈" } else { "" },
                    fmt::usd(t.usd),
                    b.team_share * 100.0,
                    b.team_read.0,
                    b.team_read.1
                )),
            ]);
        }
        if !parts.is_empty() {
            let mut prov: Line = vec![dim("  ")];
            prov.extend(joined(parts));
            rows.push(prov);
        }
    }
    let owners: Vec<Line> = state
        .attribution_top(3)
        .into_iter()
        .filter(|(_, share)| *share >= 0.05)
        .map(|(owner, share)| vec![fg(format!("{owner} {:.0} %", share * 100.0))])
        .collect();
    if !owners.is_empty() {
        let mut l: Line = vec![dim("  spent by ")];
        l.extend(joined(owners));
        rows.push(l);
    }
    Body {
        key: '4',
        id: "cost",
        title: "cost",
        keys: "Enter panel",
        rows,
        slices,
    }
}

/// The work body: the last check, the rework light, the files, the git
/// state, the turn's counters — what Turn, Files and the rework light
/// said between them.
fn body_work(state: &State, c: &Coach) -> Body {
    let now = state.clock_ms();
    let mut rows: Vec<Line> = Vec::new();
    let edits = state.edits_since_check();
    rows.push(match state.last_check() {
        Some((cmd, ok, at)) => vec![
            seg(
                if ok { "  ✓ " } else { "  ✗ " },
                if ok { Tone::Ok } else { Tone::Crit },
            ),
            seg(cmd, Tone::Bold),
            dim(format!(
                "  {} {} ago  ·  {edits} edit{} since",
                if ok { "passed" } else { "failed" },
                coach::short_duration(now - at),
                plural(edits, "", "s")
            )),
        ],
        None => vec![
            dim("  ✓ "),
            seg("no check yet this session", Tone::Bold),
            dim(format!(
                "  ·  {edits} unchecked edit{}",
                plural(edits, "", "s")
            )),
        ],
    });
    rows.push(Vec::new());
    if let Some(l) = light(c, "rework") {
        rows.push(vec![
            dim("  rework   "),
            seg(l.number.clone(), level_tone(l.level)),
            dim(format!("   {}", l.text)),
        ]);
        if let Some(fails) = l.lines.get(1) {
            rows.push(vec![dim(format!("           {fails}"))]);
        }
    }
    let files = &state.files.files;
    let stale = files.values().filter(|f| f.stale).count();
    let ide = files.values().map(|f| f.ide_edits).sum::<usize>();
    let mut fl: Line = vec![
        dim("  files    "),
        seg(format!("{} touched", files.len()), Tone::Bold),
    ];
    if ide > 0 {
        fl.push(dim(format!("   {ide} IDE edit{}", plural(ide, "", "s"))));
    }
    if stale > 0 {
        fl.push(dim("  ·  "));
        fl.push(seg(
            format!("{stale} stale file{}", plural(stale, "", "s")),
            Tone::Warn,
        ));
    }
    rows.push(fl);
    let mut rereads: Vec<_> = files.values().filter(|f| f.reread_warning()).collect();
    rereads.sort_by_key(|f| std::cmp::Reverse(f.reads_since_edit));
    if !rereads.is_empty() {
        let mut l: Line = vec![dim("  re-reads ")];
        let parts: Vec<Line> = rereads
            .iter()
            .take(3)
            .map(|f| {
                vec![seg(
                    format!(
                        "{} ×{} ⚠",
                        std::path::Path::new(&f.path)
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        f.reads_since_edit
                    ),
                    Tone::Warn,
                )]
            })
            .collect();
        l.extend(joined(parts));
        rows.push(l);
    }
    let (a, d) = state.files.total_lines();
    let mut git: Line = vec![dim("  git      "), fg(format!("+{a}/−{d}"))];
    match (state.uncommitted, state.last_commit()) {
        (Some((ua, ud, n)), _) if n > 0 => git.push(dim(format!(
            "   uncommitted +{ua} −{ud} across {n} file{}",
            plural(n, "", "s")
        ))),
        (Some(_), Some((at, _, _))) => git.push(dim(format!(
            "   nothing uncommitted · commit {} ago",
            coach::short_duration(now - at)
        ))),
        (Some(_), None) => git.push(dim("   nothing uncommitted")),
        (None, Some((at, _, _))) => git.push(dim(format!(
            "   commit {} ago",
            coach::short_duration(now - at)
        ))),
        (None, None) => {}
    }
    rows.push(git);
    let (checkpoints, bash_writes) = state.rewind_points();
    if checkpoints > 0 || bash_writes > 0 {
        rows.push(vec![dim(format!(
            "  rewind   {checkpoints} checkpoint{} this turn{}",
            plural(checkpoints, "", "s"),
            if bash_writes > 0 {
                format!(
                    " · {bash_writes} bash write{} uncovered",
                    plural(bash_writes, "", "s")
                )
            } else {
                String::new()
            }
        ))]);
    }
    if let Some(t) = state.agg.current_turn() {
        let mut turn: Line = vec![
            dim("  turn     "),
            fg(format!(
                "{} api calls · {} tool calls",
                t.api_calls, t.tool_calls
            )),
        ];
        if t.api_ms > 0 || t.tool_ms > 0 {
            turn.push(dim(format!(
                "   api {} / tools {}",
                fmt::duration_ms(t.api_ms),
                fmt::duration_ms(t.tool_ms)
            )));
        }
        if t.steers > 0 {
            turn.push(dim(format!("   steers {}", t.steers)));
        }
        if t.denials > 0 {
            turn.push(seg(format!("   denials {}", t.denials), Tone::Warn));
        }
        rows.push(turn);
    }
    Body {
        key: '5',
        id: "work",
        title: "work",
        keys: "Enter panel",
        rows,
        slices: Vec::new(),
    }
}

/// The tools body: a table by tool and Bash class, the totals, what is
/// running, and the agents (Agents rides tools).
fn body_tools(state: &State) -> Body {
    let now = state.clock_ms();
    let mut rows: Vec<Line> = Vec::new();
    let mut h: Line = Vec::new();
    at(&mut h, 2);
    h.push(dim("TOOL / CLASS"));
    rt(&mut h, 27, "CALLS", Tone::Dim);
    rt(&mut h, 34, "ERR", Tone::Dim);
    rt(&mut h, 44, "p50", Tone::Dim);
    rt(&mut h, 53, "→CTX", Tone::Dim);
    rt(&mut h, 62, "LAST", Tone::Dim);
    rows.push(h);
    let mut stats: Vec<_> = state.tools.by_name_and_class().into_values().collect();
    stats.sort_by_key(|t| std::cmp::Reverse(t.calls));
    let ctx_mark = stats.iter().map(|t| t.tokens_to_ctx).max().unwrap_or(0);
    for t in stats.iter().take(6) {
        let mut l: Line = Vec::new();
        at(&mut l, 2);
        l.push(fg(fmt::clip(&t.name, 22)));
        rt(&mut l, 27, t.calls.to_string(), Tone::Bold);
        rt(
            &mut l,
            34,
            t.errors.to_string(),
            if t.errors > 0 { Tone::Warn } else { Tone::Dim },
        );
        rt(
            &mut l,
            44,
            t.p50_ms
                .map(|ms| format!("{}{}", if t.approx { "≈" } else { "" }, fmt::short_ms(ms)))
                .unwrap_or_else(|| "—".into()),
            Tone::Fg,
        );
        rt(
            &mut l,
            53,
            fmt::tokens(t.tokens_to_ctx),
            if ctx_mark > 0 && t.tokens_to_ctx == ctx_mark {
                Tone::Warn
            } else {
                Tone::Fg
            },
        );
        rt(
            &mut l,
            62,
            t.last_call_at
                .map(|at| coach::short_duration(now - at))
                .unwrap_or_else(|| "—".into()),
            Tone::Dim,
        );
        rows.push(l);
    }
    if stats.is_empty() {
        rows.push(vec![dim("  no tool calls yet")]);
    }
    rows.push(Vec::new());
    let calls = state.tools.calls.len();
    let errors = state.tools.calls.iter().filter(|c| c.is_error).count();
    let denied = state.agg.turns.iter().map(|t| t.denials).sum::<usize>();
    let mut totals: Line = vec![seg(format!("  {calls} calls"), Tone::Bold)];
    match state.tools.errors_by_class().first() {
        Some((k, n)) => {
            totals.push(dim(format!(
                "   {errors} error{} ",
                plural(errors, "", "s")
            )));
            totals.push(seg(format!("{} {n}", k.label()), Tone::Warn));
        }
        None => totals.push(dim("   0 errors")),
    }
    totals.push(dim(format!("   {denied} denied")));
    if let Some(c) = state.tools.running() {
        totals.push(seg("   ▸ running ", Tone::Accent));
        totals.push(fg(format!(
            "{} {}",
            c.name,
            fmt::clip(&c.input_summary, 24)
        )));
        totals.push(seg(
            format!("  {}", fmt::duration_ms(now - c.started_at.unwrap_or(now))),
            Tone::Bold,
        ));
    }
    rows.push(totals);
    if let Some(top) = state.tools.top_ctx(1).first() {
        rows.push(vec![
            dim("  top ctx  "),
            fg(format!(
                "{} {}",
                top.name,
                fmt::clip(&top.input_summary, 30)
            )),
            seg(
                format!("  {}", fmt::tokens(top.result_tokens_est)),
                Tone::Warn,
            ),
        ]);
    }
    // The agents, by waste then spend (agent PRD §4.5); the team after them.
    let agent_rows = crate::agent_ledger::rows(state, crate::agent_ledger::Sort::Waste, false);
    let team_rows =
        crate::agent_ledger::teammate_rows(state, crate::agent_ledger::Sort::Waste, false);
    let team = crate::agent_ledger::team_totals(state, &team_rows);
    if !agent_rows.is_empty() || team.is_some() || !state.procs.mcp.is_empty() {
        rows.push(Vec::new());
    }
    if !agent_rows.is_empty() {
        let totals = crate::agent_ledger::totals(&agent_rows);
        let mut l: Line = vec![dim("  agents   ")];
        let mut parts: Vec<Line> = vec![vec![seg(
            format!("{} ({} running)", agent_rows.len(), totals.running),
            Tone::Bold,
        )]];
        if let Some((usd, share)) = state.agents_cost() {
            parts.push(vec![fg(format!(
                "≈{} ({:.0} %)",
                fmt::usd(usd),
                share * 100.0
            ))]);
        }
        let failed = agent_rows
            .iter()
            .filter(|r| r.state == crate::agents::State::Failed)
            .count();
        if failed > 0 {
            parts.push(vec![seg(format!("{failed} failed"), Tone::Crit)]);
        }
        if totals.waste_usd > 0.0 {
            parts.push(vec![seg(
                format!("wasted ≈{}", fmt::usd(totals.waste_usd)),
                Tone::Warn,
            )]);
        }
        l.extend(joined(parts));
        rows.push(l);
        for r in agent_rows.iter().take(3) {
            rows.push(vec![dim(format!(
                "           {} {} {} {}",
                fmt::clip(&r.agent_type, 12),
                r.elapsed_ms.map(fmt::duration_ms).unwrap_or_default(),
                fmt::tokens(r.tokens),
                format!("{:?}", r.state).to_lowercase()
            ))]);
        }
    }
    if let Some(t) = team {
        let mut l: Line = vec![dim("  team     ")];
        let mut parts: Vec<Line> = vec![vec![seg(
            match t.cost {
                Some(c) => format!("{}{}", if c.approx { "≈" } else { "" }, fmt::usd(c.usd)),
                None => format!("{} members", t.members),
            },
            Tone::Bold,
        )]];
        if t.read < t.members {
            parts.push(vec![seg(
                format!("{} of {} read", t.read, t.members),
                Tone::Warn,
            )]);
        }
        if t.waste_usd > 0.0 {
            parts.push(vec![seg(
                format!("wasted ≈{}", fmt::usd(t.waste_usd)),
                Tone::Warn,
            )]);
        }
        l.extend(joined(parts));
        rows.push(l);
    }
    if !state.procs.mcp.is_empty() {
        let mut l: Line = vec![dim("  mcp      ")];
        let parts: Vec<Line> = state
            .procs
            .mcp
            .iter()
            .take(4)
            .map(|m| {
                vec![fg(format!(
                    "{} {}",
                    fmt::clip(&m.name, 12),
                    fmt::bytes(m.rss_bytes)
                ))]
            })
            .collect();
        l.extend(joined(parts));
        if !state.mcp_needs_auth.is_empty() {
            l.push(seg("  ! auth", Tone::Warn));
        }
        rows.push(l);
    }
    let bg = state.session.background_tasks.len() + state.tasks.len();
    if bg > 0 {
        rows.push(vec![dim(format!(
            "  {bg} background task{}",
            plural(bg, "", "s")
        ))]);
    }
    Body {
        key: '6',
        id: "tools",
        title: "tools",
        keys: "Enter panel",
        rows,
        slices: Vec::new(),
    }
}

/// The advisor body: the nudge whole — headline, action, evidence — then
/// its class, what is next, what is snoozed, and the recent fires.
fn body_advisor(c: &Coach, engine: &Engine) -> Body {
    let mut rows: Vec<Line> = Vec::new();
    match &c.nudge {
        Some(n) => {
            rows.push(vec![
                seg("  ▸ ", Tone::Ok),
                seg(n.line1.trim_start_matches("▸ ").trim(), Tone::Bold),
            ]);
            if !n.line2.trim().is_empty() {
                rows.push(vec![dim(format!("    {}", n.line2.trim()))]);
            }
            if !n.evidence.trim().is_empty() {
                rows.push(vec![dim(format!("    {}", n.evidence.trim()))]);
            }
            if !n.saving.is_empty() {
                rows.push(vec![dim(format!("    saves {}", n.saving))]);
            }
            rows.push(Vec::new());
            rows.push(vec![
                dim("  class    "),
                seg(
                    n.class.label(),
                    match n.class {
                        crate::advisor::Urgency::Now => Tone::Crit,
                        crate::advisor::Urgency::Next => Tone::Warn,
                        crate::advisor::Urgency::Later => Tone::Ok,
                    },
                ),
                dim(format!("   {} · retires {}", n.id, n.retires_on)),
            ]);
        }
        None => {
            rows.push(vec![
                seg("  ▸ ", Tone::Ok),
                seg(
                    if c.exposed {
                        "nothing to act on"
                    } else {
                        "coach off (control arm)"
                    },
                    Tone::Bold,
                ),
            ]);
            if let Some(l) = &c.start_line {
                rows.push(vec![dim(format!("    {l}"))]);
            }
            rows.push(Vec::new());
        }
    }
    rows.push(vec![dim(format!("  {}", c.next_row))]);
    rows.push(vec![dim(format!("  {}", c.snoozed_row))]);
    let more: Vec<Line> = engine
        .current
        .iter()
        .skip(1)
        .take(3)
        .map(|a| {
            vec![dim(format!(
                "  {} {} {}",
                a.urgency.label(),
                a.rule,
                a.headline
            ))]
        })
        .collect();
    if !more.is_empty() {
        rows.push(Vec::new());
        rows.push(vec![dim("  also")]);
        rows.extend(more);
    }
    if !c.recent.is_empty() {
        rows.push(Vec::new());
        rows.push(vec![dim("  recent")]);
        for r in c.recent.iter().take(3) {
            rows.push(vec![dim(format!("  {}", r.row))]);
        }
    }
    Body {
        key: 'a',
        id: "advisor",
        title: "advisor",
        keys: "Enter coach",
        rows,
        slices: Vec::new(),
    }
}

/// The events body: the newest first, `hh:mm  kind  text`. The coach's
/// fires the engine has not drained yet (a one-shot reader's) are folded
/// in, so `cctop query` and the TUI show the same rows.
fn body_events(state: &State, engine: &Engine) -> Body {
    let mut all: Vec<crate::events::Event> = state.events.tail(40).into_iter().cloned().collect();
    for e in engine.pending_events() {
        all.push(crate::events::Event {
            at: e.at_ms,
            kind: crate::events::Kind::Coach,
            text: e.text.clone(),
        });
    }
    all.sort_by_key(|e| e.at);
    let tail = &all[all.len().saturating_sub(40)..];
    let mut rows: Vec<Line> = tail
        .iter()
        .rev()
        .map(|e| {
            let mut l: Line = Vec::new();
            at(&mut l, 2);
            l.push(dim(fmt::clock_hhmm(e.at)));
            at(&mut l, 9);
            l.push(seg(
                e.kind.label(),
                match e.kind {
                    crate::events::Kind::Api => Tone::Crit,
                    crate::events::Kind::Perm | crate::events::Kind::Cost => Tone::Warn,
                    _ => Tone::Accent,
                },
            ));
            // The widest kind (`compact`) is seven cells from column 9.
            at(&mut l, 17);
            l.push(fg(e.text.clone()));
            l
        })
        .collect();
    if rows.is_empty() {
        rows.push(vec![dim("  no events yet")]);
    }
    Body {
        key: '0',
        id: "events",
        title: "events",
        keys: "Enter panel",
        rows,
        slices: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;

    fn fixture_b() -> (State, Engine) {
        fixture_at("b", usize::MAX)
    }

    /// The first `n` lines of a fixture, the clock at its last line.
    fn fixture_at(name: &str, n: usize) -> (State, Engine) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("fixtures/session-{name}.jsonl"));
        let mut s = State::new(Pricing::bundled());
        s.session = crate::ui::state::SessionInfo::from_fixture(&path);
        for l in crate::transcript::parse_file(&path).unwrap().iter().take(n) {
            s.apply(l);
        }
        s.session.ended_at_ms = s.last_line_at_ms;
        let e = Engine::for_state(&s);
        (s, e)
    }

    fn body<'a>(d: &'a Dashboard, id: &str) -> &'a Body {
        d.bodies.iter().find(|b| b.id == id).unwrap()
    }

    /// The text of a body's rows, one string per row.
    fn body_text(b: &Body) -> Vec<String> {
        b.rows
            .iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    }

    /// The context body carries the `m` inspector's breakdown too, so the
    /// pane answers "what put it there" without a second surface: the six
    /// result kinds, the prompts, the remainder, the reference and the
    /// mode — the same `residency()` the TUI reads, never a second model.
    #[test]
    fn the_context_body_breaks_the_results_slice_into_its_sources() {
        let (s, e) = fixture_at("a", usize::MAX);
        let r = s.residency();
        let d = snapshot(&s, &e);
        let text = body_text(body(&d, "context")).join("\n");

        // The reference and the call count name where the window starts.
        assert!(
            text.contains(&format!(
                "since {}",
                crate::ui::sources_view::since_text(&r)
            )),
            "{text}"
        );
        assert!(text.contains(&format!("{} calls", r.calls_since)), "{text}");
        // Fixture A's results are mostly MCP: the slice said `results`, the
        // rows say which kind.
        assert!(text.contains("mcp results"), "{text}");
        assert!(text.contains("files"), "{text}");
        assert!(text.contains("prompts"), "{text}");
        assert!(text.contains("other"), "{text}");
        // The figures are the residency's own, to the token.
        assert!(
            text.contains(&fmt::tokens(r.source(Source::McpResults))),
            "{text}"
        );
        // A source with nothing in the window is not a row.
        assert_eq!(r.source(Source::Web), 0, "fixture A fetched nothing");
        assert!(
            !text.contains("web"),
            "an empty source draws no row: {text}"
        );
        // The mode says how the prefix was arrived at.
        assert!(
            text.contains(&crate::ui::sources_view::mode_text(&r)),
            "{text}"
        );
    }

    /// PRD dashboard-v2 §3.2's second pair (US-101): the header said
    /// `WORKING · silent 1:56` while row 4 said `elapsed 0:00 · 159 api
    /// calls`. Fixture E is that session's first 66 lines: a 205 ms attempt
    /// that Claude Code closed with `turn_duration`, a `/login`, then the
    /// same prompt re-driven with no new user line. The turn reopens on the
    /// next response, so the header's phase word, its elapsed and the work
    /// body's turn line agree at every line.
    #[test]
    fn fixture_e_header_and_work_body_read_one_open_turn() {
        // The attempt: closed, 205 ms, nothing counted.
        let (s, e) = fixture_at("e", 20);
        let t = s.agg.current_turn().unwrap();
        assert_eq!(t.duration_ms, Some(205));
        assert_eq!((t.reopened, t.api_calls), (0, 0));
        let d = snapshot(&s, &e);
        assert_eq!(d.header.phase.word, "IDLE");
        assert_eq!(d.header.phase.elapsed, "0:00");

        // The re-driven prompt's first response: the turn is open again.
        let (s, e) = fixture_at("e", 27);
        let t = s.agg.current_turn().unwrap();
        assert_eq!((t.duration_ms, t.reopened, t.api_calls), (None, 1, 1));
        assert!(crate::coach::turn_running(&s));
        let d = snapshot(&s, &e);
        assert_eq!(d.header.phase.word, "THINKING");
        assert_eq!(d.header.phase.elapsed, "1:06");

        // Six calls later: one phase word, one elapsed, on both.
        let (s, e) = fixture_at("e", 66);
        let d = snapshot(&s, &e);
        assert_eq!(d.header.phase.word, "EXPLORING");
        assert_eq!(d.header.phase.word, coach::snapshot(&s, &e).state.kind);
        assert_eq!(d.header.phase.elapsed, "1:50");
        let work = body(&d, "work");
        let turn = work
            .rows
            .iter()
            .map(text_of)
            .find(|r| r.starts_with("  turn"))
            .unwrap();
        assert!(
            turn.starts_with("  turn     6 api calls · 6 tool calls"),
            "{turn}"
        );
        let t = s.agg.current_turn().unwrap();
        assert_eq!(t.elapsed_ms(s.clock_ms()), Some(110_844));
        assert_eq!(t.reopened, 1);
    }

    #[test]
    fn a_teammate_attached_as_the_main_session_names_its_team() {
        // Fixture D's real teammate, read as the attached session: a normal
        // session whose header carries `<member>@<team>`; the lead's (and
        // A's, B's, C's) header has no `team` at all.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/session-d/teammates/85424f6c-500e-5993-641b-0a66d897a888.jsonl");
        let mut s = State::new(Pricing::bundled());
        s.session = crate::ui::state::SessionInfo::from_fixture(&path);
        for l in crate::transcript::parse_file(&path).unwrap() {
            s.apply(&l);
        }
        s.session.ended_at_ms = s.last_line_at_ms;
        let e = Engine::for_state(&s);
        let d = snapshot(&s, &e);
        assert_eq!(
            d.header.team.as_deref(),
            Some("diff-pane-research@session-afd065d3")
        );
        assert!(
            d.header
                .line
                .contains(" · diff-pane-research@session-afd065d3 · "),
            "{}",
            d.header.line
        );
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["header"]["team"], "diff-pane-research@session-afd065d3");
        let (s, e) = fixture_b();
        let d = snapshot(&s, &e);
        assert_eq!(d.header.team, None);
        let v = serde_json::to_value(&d).unwrap();
        assert!(v["header"].get("team").is_none());
    }

    /// Schema 2 (US-102): six cells with contiguous keys, the act line,
    /// eight bodies with the keys `1`–`6`, `a`, `0`, nothing width-aware.
    #[test]
    fn fixture_b_console_object() {
        let (s, e) = fixture_b();
        let d = snapshot(&s, &e);
        assert_eq!(d.schema, 2);
        assert_eq!(d.cells.len(), 6);
        for (i, c) in d.cells.iter().enumerate() {
            assert_eq!(c.key, char::from(b'1' + i as u8));
            assert_eq!(c.id, BODY_IDS[i]);
            assert_eq!(c.opens, c.id);
            assert!(!text_of(&c.short).is_empty());
            assert!(width_of(&c.short) <= width_of(&c.mid));
            assert!(width_of(&c.mid) <= width_of(&c.label));
        }
        assert_eq!(text_of(&d.cells[0].label), "ctx 14% · 425k left");
        assert_eq!(text_of(&d.cells[0].mid), "ctx 14% 425k left");
        assert_eq!(text_of(&d.cells[0].short), "ctx 14%");
        assert_eq!(text_of(&d.cells[1].label), "5h — · no status line");
        assert_eq!(text_of(&d.cells[2].label), "cache 59m≈ · 1h TTL");
        assert!(
            text_of(&d.cells[3].label).starts_with("spend $18.7 · ≈$"),
            "{}",
            text_of(&d.cells[3].label)
        );
        assert_eq!(
            text_of(&d.cells[4].label),
            "✓ python3 -… 9h01 · rework 1 open"
        );
        assert_eq!(text_of(&d.cells[5].label), "140 calls · 6 err");
        assert_eq!(text_of(&d.cells[5].short), "140c 6 err");
        assert_eq!(d.bodies.len(), 8);
        for (i, b) in d.bodies.iter().enumerate() {
            assert_eq!(b.key, BODY_KEYS[i]);
            assert_eq!(b.id, BODY_IDS[i]);
            assert!(!b.rows.is_empty(), "{}", b.id);
        }
        // Full length, never cut here: the widest row is wider than the
        // old `ROW_WIDTH` (118) ever allowed.
        let widest = d
            .bodies
            .iter()
            .flat_map(|b| b.rows.iter())
            .map(width_of)
            .max()
            .unwrap();
        assert!(widest > 118, "{widest}");
        assert_eq!((d.act.key, d.act.opens), ('a', "advisor"));
        // Fixture B's last moment holds a LATER nudge: the act line is it.
        assert_eq!(d.act.nudge, Some("A10"));
        assert_eq!(d.act.tag, "LATER · turn 6");
        assert!(
            text_of(&d.act.line).starts_with("▸ `cd lorem_ipsum_dolor_sit_amet…` blocked the turn"),
            "{}",
            text_of(&d.act.line)
        );
        assert!(text_of(&d.act.line).contains(" — queue: 'run builds"));
        assert!(!text_of(&d.act.short).contains(" — "));
        assert!(!d.act.blocked);
        // The context body carries the five slices with their steps.
        let ctx = body(&d, "context");
        assert_eq!(
            ctx.slices
                .iter()
                .map(|s| (s.label, s.step))
                .collect::<Vec<_>>(),
            [
                ("prefix", 0),
                ("harness", 0),
                ("thinking", 1),
                ("inputs", 2),
                ("results", 2)
            ]
        );
        assert!(ctx
            .rows
            .iter()
            .any(|r| r.iter().any(|s| s.tone == Tone::S2)));
        assert!(
            text_of(&ctx.rows[0]).starts_with("  142k of 1.00M est   autocompact "),
            "{}",
            text_of(&ctx.rows[0])
        );
        assert_eq!(body(&d, "cost").slices.len(), 5);
        assert!(text_of(&body(&d, "cost").rows[0]).starts_with("  $18.7   API-equivalent"));
        assert!(text_of(&body(&d, "tools").rows[0]).starts_with("  TOOL / CLASS"));
        assert!(
            text_of(&body(&d, "tools").rows[1]).starts_with("  Bash·"),
            "{}",
            text_of(&body(&d, "tools").rows[1])
        );
        assert!(
            text_of(&body(&d, "events").rows[0]).contains("api"),
            "{}",
            text_of(&body(&d, "events").rows[0])
        );
        assert_eq!(d.header.phase.word, "COMMITTING");
        assert_eq!(d.header.phase.glyph, '●');
        assert!(
            d.header
                .line
                .starts_with("cctop  claude-sonnet-5 · turn 6 · "),
            "{}",
            d.header.line
        );
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["schema"], 2);
        assert!(v.get("tiles").is_none() && v.get("rows").is_none());
        assert_eq!(v["cells"][0]["key"], "1");
        assert_eq!(v["bodies"][6]["key"], "a");
        assert_eq!(v["bodies"][7]["key"], "0");
        assert_eq!(v["bodies"][0]["slices"][0]["step"], 0);
    }

    /// The act line pre-empts: a wait paints it crit over any nudge; with
    /// nothing to act on, the state line's tokens lead with the steer
    /// window (fixture B's cold moment: `PLANNING · 1 call · ▸ steer window
    /// · +83 ctx`).
    #[test]
    fn act_line_forms() {
        let (s, e) = fixture_at("b", 789);
        let d = snapshot(&s, &e);
        if d.act.nudge.is_none() {
            assert!(
                text_of(&d.act.line).starts_with("▸ steer window — 1 call, +83 ctx"),
                "{}",
                text_of(&d.act.line)
            );
            assert_eq!(text_of(&d.act.short), "▸ steer window · 1 call");
        }
        let (s, e) = fixture_at("b", 788);
        let mut s = s;
        s.now_ms = s.last_line_at_ms.unwrap() + 240_000;
        s.clock_override = true;
        let d = snapshot(&s, &e);
        assert!(d.act.blocked, "{}", text_of(&d.act.line));
        assert!(
            text_of(&d.act.line).starts_with("◆ WAITING "),
            "{}",
            text_of(&d.act.line)
        );
        assert_eq!(d.act.line[0].tone, Tone::Crit);
    }

    #[test]
    fn cut_keeps_tones_and_marks_the_edge() {
        let line = vec![fg("abcdef"), dim("ghij")];
        let c = cut(line.clone(), 8);
        assert_eq!(text_of(&c), "abcdefg…");
        assert_eq!(c[1].tone, Tone::Dim);
        assert_eq!(cut(line, 20).len(), 2);
        let mut l: Line = vec![fg("ab")];
        at(&mut l, 5);
        rt(&mut l, 9, "xy", Tone::Bold);
        assert_eq!(text_of(&l), "ab     xy");
        assert_eq!(text_of(&bar(0.0, 5, Tone::Ok)), "▁▁▁▁▁");
        assert_eq!(text_of(&bar(1.0, 5, Tone::Ok)), "▇▁▁▁▁");
        assert_eq!(text_of(&bar(99.0, 5, Tone::Ok)), "▇▇▇▇▁");
        assert_eq!(text_of(&bar(100.0, 5, Tone::Ok)), "▇▇▇▇▇");
    }
}
