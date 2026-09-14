//! The dashboard object (plan B, "Big figures"): one snapshot of `State`
//! and the Advisor engine that the TUI's dashboard and the pane's Overview
//! draw — a header line, four tiles (the coach's lights, enlarged), the
//! nudge line and nine borderless ledger rows, one per panel. Rows carry
//! tagged segments and are cut at [`ROW_WIDTH`] cells here, so both
//! surfaces truncate identically.

use serde::Serialize;

use crate::advisor::Engine;
use crate::coach::{self, Coach, Level};
use crate::ui::fmt;
use crate::ui::state::State;

/// Where a ledger row is cut.
pub const ROW_WIDTH: usize = 118;

/// A segment's role; every surface maps it to its own colour.
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

/// Cut a line at `cells`, an ellipsis on the segment that crosses the edge.
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
            out.push(seg(fmt::clip(&s.text, cells - used), s.tone));
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseCell {
    /// `●` while a turn runs, `○` idle, `◆` waiting.
    pub glyph: char,
    pub word: String,
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
    pub phase: PhaseCell,
    /// The header as one line: `cctop  claude-opus-5 · turn 14 · 1:12:08 · ~/code/cctop · PR #142`.
    pub line: String,
}

/// One tile: a coach light drawn large.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tile {
    pub id: &'static str,
    pub level: Level,
    pub glyph: char,
    /// The block-digit text (`41`, `62`, `55`, `3`, `—`).
    pub figure: String,
    pub unit: &'static str,
    pub name: &'static str,
    pub sub1: String,
    pub sub2: String,
    pub source: &'static str,
    pub approx: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NudgeLine {
    pub id: &'static str,
    pub class: crate::advisor::Urgency,
    /// `▸ headline — action`.
    pub line: String,
    /// `NOW · call 12` / `LATER · turn 6`.
    pub tag: String,
    pub acting: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub digit: u8,
    pub name: &'static str,
    pub values: Line,
    pub detail: Line,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    pub schema: u32,
    pub header: Header,
    pub tiles: Vec<Tile>,
    pub nudge: Option<NudgeLine>,
    pub rows: Vec<Row>,
    pub session_mode: crate::advisor::SessionMode,
    /// The coach's one-line forms, for the narrow tiles (L1 / L2).
    pub lines: coach::StatusLines,
}

/// The whole object.
pub fn snapshot(state: &State, engine: &Engine) -> Dashboard {
    let c = coach::snapshot(state, engine);
    let tiles = c.lights.iter().map(|l| tile(l, state)).collect();
    let nudge = c.nudge.as_ref().map(|n| NudgeLine {
        id: n.id,
        class: n.class,
        line: format!("{} — {}", n.line1.trim_start_matches("▸ "), n.line2.trim()),
        tag: n
            .evidence
            .trim()
            .split(" · ")
            .take(2)
            .collect::<Vec<_>>()
            .join(" · ")
            .replace("fired at call", "call")
            .replace("since turn", "turn"),
        acting: n.acting,
    });
    let rows = vec![
        row_context(state),
        row_tokens(state),
        row_limits(state),
        row_turn(state, &c),
        row_tools(state),
        row_agents(state),
        row_files(state),
        row_events(state),
        row_advisor(&c, engine),
    ]
    .into_iter()
    .map(|r| Row {
        values: cut(r.values, ROW_WIDTH),
        detail: cut(r.detail, ROW_WIDTH),
        ..r
    })
    .collect();
    Dashboard {
        schema: 1,
        header: header(state, &c),
        tiles,
        nudge,
        rows,
        session_mode: engine.session_mode,
        lines: c.lines.clone(),
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
        phase: PhaseCell {
            glyph,
            word,
            tokens: c.state.tokens.clone(),
        },
        line: format!("cctop  {}", parts.join(" · ")),
    }
}

fn tile(l: &coach::Light, state: &State) -> Tile {
    let v = state.context();
    let (figure, sub1, sub2) = match l.id {
        "context" => (
            l.figure.map(|f| format!("{f:.0}")).unwrap_or("—".into()),
            format!("{} of {}", fmt::tokens(v.size), fmt::tokens(v.window)),
            l.text.split(" · ").nth(1).unwrap_or("").to_string(),
        ),
        "cache" => {
            let parts: Vec<&str> = l.text.split(" · ").collect();
            let misses = l
                .lines
                .first()
                .and_then(|s| s.split(" · ").nth(1))
                .unwrap_or("")
                .to_string();
            (
                l.figure.map(|f| format!("{f:.0}")).unwrap_or("—".into()),
                match parts.first() {
                    Some(p) if p.starts_with("warm") || p.starts_with("cold in") => {
                        format!("{} TTL", p.replace(" ≈", ""))
                    }
                    Some(p) => (*p).to_string(),
                    None => String::new(),
                }
                .replace("warm ", "warm · ")
                .replace("cold in ", "cold in · "),
                misses,
            )
        }
        "limits" => {
            let parts: Vec<&str> = l.text.split(" · ").collect();
            (
                l.figure.map(|f| format!("{f:.0}")).unwrap_or("—".into()),
                parts
                    .iter()
                    .take(2)
                    .map(|p| p.replace("5h ", "5h · "))
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .replacen(l.number.as_str(), "", 1)
                    .replace("5h ·  · ", "5h · ")
                    .trim_matches(|c| c == ' ' || c == '·')
                    .to_string(),
                parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
            )
        }
        _ => {
            let parts: Vec<&str> = l.text.split(" · ").collect();
            let (first, second) = match parts.as_slice() {
                [a] => (a.to_string(), String::new()),
                [a, b, ..] if a.starts_with("edits") => (a.to_string(), b.to_string()),
                [a, rest @ ..] => (
                    a.replacen(' ', " · ", 1),
                    rest.iter()
                        .find(|p| p.starts_with("edits"))
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                ),
                [] => (String::new(), String::new()),
            };
            (l.number.clone(), first, second)
        }
    };
    Tile {
        id: l.id,
        level: l.level,
        glyph: l.glyph,
        figure,
        unit: l.unit,
        name: l.id,
        sub1,
        sub2,
        source: l.source,
        approx: l.approx,
    }
}

fn row_context(state: &State) -> Row {
    let v = state.context();
    let a = state.anatomy();
    let approx = if a.approx { "≈" } else { "" };
    let bar = crate::coach::WIDTH.min(40);
    let filled = if v.window == 0 {
        0
    } else {
        ((v.size as f64 / v.window as f64) * bar as f64).round() as usize
    }
    .min(bar);
    let values = vec![
        seg("▇".repeat(filled), Tone::Accent),
        dim("▁".repeat(bar - filled)),
        fg(format!(
            " prefix {} · inputs {approx}{} · results {approx}{} · thinking {} · harness {approx}{}",
            fmt::tokens(a.prefix),
            fmt::tokens(a.tool_inputs),
            fmt::tokens(a.tool_results),
            fmt::tokens(a.thinking),
            fmt::tokens(a.harness)
        )),
    ];
    let mut parts: Vec<Line> = vec![vec![fg(format!(
        "+{}/turn",
        fmt::tokens(v.velocity.max(0.0) as u64)
    ))]];
    parts.push(vec![fg(format!(
        "autocompact {}",
        fmt::tokens(v.threshold)
    ))]);
    parts.push(vec![fg(format!(
        "{} left",
        fmt::tokens(v.threshold.saturating_sub(v.size))
    ))]);
    parts.push(vec![fg(format!("compactions {}", v.compactions.len()))]);
    if let Some(b) = state.agg.boundaries.last() {
        if let Some(at) = b.at.as_deref().and_then(crate::metrics::cost::parse_ts_ms) {
            parts.push(vec![dim(format!(
                "since {:?} {}",
                b.kind,
                fmt::clock_hhmm(at)
            )
            .to_lowercase())]);
        }
    }
    let rereads: usize = state
        .files
        .files
        .values()
        .filter(|f| f.reread_warning())
        .count();
    parts.push(vec![fg(format!("re-reads {rereads}"))]);
    Row {
        digit: 1,
        name: "Context",
        values,
        detail: joined(parts)
            .into_iter()
            .map(|s| Seg {
                tone: Tone::Dim,
                ..s
            })
            .collect(),
    }
}

fn row_tokens(state: &State) -> Row {
    let u = state.agg.total;
    let mut parts: Vec<Line> = vec![
        vec![fg(format!("cache read {}", fmt::tokens(u.cache_read)))],
        vec![fg(format!("write {}", fmt::tokens(u.cache_write())))],
        vec![fg(format!("output {}", fmt::tokens(u.output)))],
    ];
    match state.cache_clock() {
        Some(c) if c.remaining_ms > 0 => parts.push(vec![seg(
            format!(
                "warm {}{}",
                coach::short_duration(c.remaining_ms),
                if c.approx { " ≈" } else { "" }
            ),
            Tone::Ok,
        )]),
        Some(_) => parts.push(vec![seg("cold", Tone::Warn)]),
        None => {}
    }
    let misses = state.agg.cache_misses.len() as u64 + state.cache.misses;
    if misses > 0 {
        let last = state
            .agg
            .cache_misses
            .last()
            .map(|m| format!(" ({} {})", m.kind, fmt::tokens(m.tokens)))
            .unwrap_or_default();
        parts.push(vec![seg(format!("misses {misses}{last}"), Tone::Warn)]);
    }
    let values = joined(parts);
    let mut detail: Vec<Line> = Vec::new();
    if let Some(c) = state.cost.current() {
        detail.push(vec![fg(format!(
            "{}{}",
            if c.approx { "≈" } else { "" },
            fmt::usd(c.usd)
        ))]);
    }
    let rates = crate::metrics::cost::rates(&state.agg, state.cost.pricing(), state.clock_ms());
    if let Some(h) = rates.usd_per_hour {
        detail.push(vec![fg(format!("{}/h", fmt::usd(h)))]);
    }
    if let Some(g) = state.gradient() {
        detail.push(vec![fg(format!(
            "$/call ≈{}{}",
            coach::usd_short(g.per_call),
            if g.cold { " cold" } else { "" }
        ))]);
        detail.push(vec![fg(format!(
            "$/turn ≈{}",
            coach::usd_short(g.per_turn)
        ))]);
        detail.push(vec![fg(format!(
            "next 30c ≈{}",
            coach::usd_short(g.next_30_calls)
        ))]);
    }
    if let Some((usd, share)) = state.agents_cost() {
        detail.push(vec![fg(format!(
            "agents {} ({:.0} %)",
            fmt::usd(usd),
            share * 100.0
        ))]);
    }
    for (name, share) in state.attribution_top(1) {
        if name != "idle" {
            detail.push(vec![dim(format!("{name} {:.0} %", share * 100.0))]);
        }
    }
    Row {
        digit: 2,
        name: "Tokens",
        values,
        detail: joined(detail),
    }
}

fn row_limits(state: &State) -> Row {
    let now = state.clock_ms();
    let mut parts: Vec<Line> = Vec::new();
    match &state.limits {
        Some(l) => {
            parts.push(vec![seg(
                format!(
                    "5h {:.0} %{}",
                    l.five_hour_pct,
                    l.five_hour_resets_at_ms
                        .map(|r| format!(" ↻ {}", coach::short_duration(r - now)))
                        .unwrap_or_default()
                ),
                if l.five_hour_pct >= 80.0 {
                    Tone::Warn
                } else {
                    Tone::Fg
                },
            )]);
            parts.push(vec![fg(format!(
                "7d {:.0} %{}",
                l.seven_day_pct,
                l.seven_day_resets_at_ms
                    .map(|r| format!(" ↻ {}", coach::short_duration(r - now)))
                    .unwrap_or_default()
            ))]);
        }
        None => parts.push(vec![dim("no status line")]),
    }
    if let Some(m) = state.model() {
        let tier = crate::harness_facts::usage_weight::tier(m);
        parts.push(vec![dim(format!(
            "weight ×{tier:.0} ({})",
            fmt::model_short(m).split('-').next().unwrap_or("")
        ))]);
    }
    let flags = state.behaviour_flags();
    if flags.long_context_count > 0 {
        parts.push(vec![fg(format!(
            "long_context {:.0} %",
            flags.long_context_pct
        ))]);
    }
    if state.other_live_sessions > 0 {
        parts.push(vec![fg(format!(
            "other sessions {}",
            state.other_live_sessions
        ))]);
    }
    let mut detail: Vec<Line> = Vec::new();
    if let Some(l) = &state.limits {
        match (l.exhaustion_ms, l.five_hour_resets_at_ms) {
            (Some(ex), Some(reset)) if ex < reset => detail.push(vec![seg(
                format!(
                    "exhausted in {} — before the reset",
                    coach::short_duration(ex - now)
                ),
                Tone::Warn,
            )]),
            (Some(ex), _) => detail.push(vec![dim(format!(
                "exhausted in {}",
                coach::short_duration(ex - now)
            ))]),
            _ => {}
        }
    }
    if let Some((kind, _, _)) = state.rate_limit_hit() {
        detail.push(vec![seg(
            format!("rate limited ({})", kind.replace('_', " ")),
            Tone::Crit,
        )]);
    }
    if let Some(p) = state.status_facts.spend_limit_pct {
        detail.push(vec![fg(format!("spend limit {p:.0} %"))]);
    }
    for tip in flags.tips().into_iter().take(1) {
        detail.push(vec![dim(tip)]);
    }
    Row {
        digit: 3,
        name: "Limits",
        values: joined(parts),
        detail: joined(detail),
    }
}

fn row_turn(state: &State, c: &Coach) -> Row {
    let t = state.agg.current_turn();
    let mut parts: Vec<Line> = vec![vec![seg(
        c.state.line.clone(),
        if c.state.kind.starts_with('◆') {
            Tone::Warn
        } else {
            Tone::Accent
        },
    )]];
    if let Some(t) = t {
        if t.api_ms > 0 || t.tool_ms > 0 {
            parts.push(vec![fg(format!(
                "api ≈{} · tools {}",
                fmt::duration_ms(t.api_ms),
                fmt::duration_ms(t.tool_ms)
            ))]);
        }
        if t.steers > 0 {
            parts.push(vec![fg(format!("steers {}", t.steers))]);
        }
        if t.denials > 0 {
            parts.push(vec![seg(format!("denials {}", t.denials), Tone::Warn)]);
        }
    }
    let bg = state.session.background_tasks.len() + state.tasks.len();
    if bg > 0 {
        parts.push(vec![fg(format!("background {bg}"))]);
    }
    let mut detail: Vec<Line> = Vec::new();
    if let Some(t) = t {
        detail.push(vec![dim(format!(
            "elapsed {} · {} api calls · {} tool calls",
            t.elapsed_ms(state.clock_ms())
                .map(fmt::duration_ms)
                .unwrap_or_else(|| "—".into()),
            t.api_calls,
            t.tool_calls
        ))]);
        if t.hook_runs > 0 {
            detail.push(vec![dim(format!(
                "hooks {} ({})",
                t.hook_runs,
                fmt::short_ms(t.hook_ms)
            ))]);
        }
    }
    if let Some(w) = state.waiting() {
        detail.push(vec![seg(
            format!(
                "waiting {} ({:?})",
                coach::short_duration(state.clock_ms() - w.since_ms),
                w.kind
            )
            .to_lowercase(),
            Tone::Warn,
        )]);
    }
    Row {
        digit: 4,
        name: "Turn",
        values: joined(parts),
        detail: joined(detail),
    }
}

fn row_tools(state: &State) -> Row {
    let calls = state.tools.calls.len();
    let errors = state.tools.calls.iter().filter(|c| c.is_error).count();
    let mut parts: Vec<Line> = vec![vec![fg(format!("{calls} calls"))]];
    if errors > 0 {
        parts.push(vec![seg(format!("{errors} err"), Tone::Warn)]);
    }
    for (class, n, err) in state.tools.bash_by_class().into_iter().take(3) {
        let label = format!("{:?}", class).to_lowercase();
        parts.push(vec![fg(if err > 0 {
            format!("{label} {n} ({err} ✗)")
        } else {
            format!("{label} {n}")
        })]);
    }
    let mut by_name: Vec<_> = state
        .tools
        .by_name()
        .into_values()
        .filter(|t| t.name != "Bash")
        .collect();
    by_name.sort_by(|a, b| b.calls.cmp(&a.calls));
    for t in by_name.iter().take(3) {
        parts.push(vec![fg(format!("{} {}", t.name, t.calls))]);
    }
    if let Some(top) = state.tools.top_ctx(1).first() {
        parts.push(vec![dim(format!(
            "top ctx {} {} {}",
            top.name,
            fmt::clip(&top.input_summary, 16),
            fmt::tokens(top.result_tokens_est)
        ))]);
    }
    let mut detail: Vec<Line> = Vec::new();
    for (k, n) in state.tools.errors_by_class().into_iter().take(4) {
        detail.push(vec![dim(format!("{} {n}", k.label()))]);
    }
    if let Some(c) = state.tools.running() {
        detail.push(vec![seg(
            format!(
                "running {} {} {}",
                c.name,
                fmt::clip(&c.input_summary, 20),
                fmt::duration_ms(state.clock_ms() - c.started_at.unwrap_or(state.clock_ms()))
            ),
            Tone::Accent,
        )]);
    }
    Row {
        digit: 5,
        name: "Tools",
        values: joined(parts),
        detail: joined(detail),
    }
}

fn row_agents(state: &State) -> Row {
    let now = state.clock_ms();
    let mut parts: Vec<Line> = Vec::new();
    let running = state
        .agents
        .values()
        .filter(|a| a.state(now) == crate::agents::State::Running)
        .count();
    if !state.agents.is_empty() {
        parts.push(vec![fg(format!("{running} run"))]);
        if let Some((usd, share)) = state.agents_cost() {
            parts.push(vec![fg(format!("{:.0} %", share * 100.0))]);
            let mut agents: Vec<_> = state.agents.values().collect();
            agents.sort_by_key(|a| std::cmp::Reverse(a.usage.total()));
            for a in agents.iter().take(3) {
                parts.push(vec![fg(format!(
                    "{} {} {}",
                    fmt::clip(&a.agent_type, 8),
                    a.elapsed_ms(now).map(fmt::duration_ms).unwrap_or_default(),
                    fmt::tokens(a.usage.total())
                ))]);
            }
            parts.push(vec![fg(fmt::usd(usd))]);
        }
        let failed = state
            .agents
            .values()
            .filter(|a| a.state(now) == crate::agents::State::Failed)
            .count();
        if failed > 0 {
            parts.push(vec![seg(format!("{failed} failed"), Tone::Crit)]);
        }
    }
    if !state.mcp_needs_auth.is_empty() {
        parts.push(vec![seg("! auth", Tone::Warn)]);
    }
    if parts.is_empty() {
        parts.push(vec![dim("—")]);
    }
    let mut detail: Vec<Line> = Vec::new();
    for m in state.procs.mcp.iter().take(4) {
        detail.push(vec![dim(format!(
            "mcp {} {}",
            fmt::clip(&m.name, 12),
            fmt::bytes(m.rss_bytes)
        ))]);
    }
    if !state.tasks.is_empty() {
        detail.push(vec![fg(format!("{} background", state.tasks.len()))]);
    }
    Row {
        digit: 6,
        name: "Agents",
        values: joined(parts),
        detail: joined(detail),
    }
}

fn row_files(state: &State) -> Row {
    let n = state.files.files.len();
    let mut parts: Vec<Line> = vec![vec![fg(format!("{n} touched"))]];
    let mut files: Vec<_> = state.files.files.values().collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.last_touch_ms));
    for f in files.iter().take(2) {
        let name = std::path::Path::new(&f.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut cell = name;
        if f.edits > 0 {
            cell.push_str(&format!(" E×{}", f.edits));
        }
        if f.writes > 0 {
            cell.push_str(&format!(" W×{}", f.writes));
        }
        if f.reads > 0 && f.edits == 0 && f.writes == 0 {
            cell.push_str(&format!(" R×{}", f.reads));
        }
        if f.ide_edits > 0 {
            cell.push_str(" IDE edit");
        }
        parts.push(vec![fg(cell)]);
    }
    let (a, d) = state.files.total_lines();
    if a + d > 0 {
        parts.push(vec![fg(format!("+{a}/−{d}"))]);
    }
    if let Some((at, _, _)) = state.last_commit() {
        parts.push(vec![fg(format!(
            "commit {} ago",
            coach::short_duration(state.clock_ms() - at)
        ))]);
    }
    let mut detail: Vec<Line> = Vec::new();
    let rereads: Vec<String> = files
        .iter()
        .filter(|f| f.reread_warning())
        .take(2)
        .map(|f| {
            format!(
                "{} re-read ×{}",
                std::path::Path::new(&f.path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                f.reads_since_edit
            )
        })
        .collect();
    if !rereads.is_empty() {
        detail.push(vec![seg(rereads.join(" · "), Tone::Warn)]);
    }
    if let Some((a, d, n)) = state.uncommitted {
        detail.push(vec![dim(if n == 0 {
            "clean".to_string()
        } else {
            format!("uncommitted +{a} −{d} across {n} files")
        })]);
    }
    let stale = files.iter().filter(|f| f.stale).count();
    if stale > 0 {
        detail.push(vec![seg(format!("{stale} stale"), Tone::Warn)]);
    }
    Row {
        digit: 7,
        name: "Files",
        values: joined(parts),
        detail: joined(detail),
    }
}

fn row_events(state: &State) -> Row {
    let tail = state.events.tail(6);
    let cell = |e: &crate::events::Event| -> Line {
        vec![
            dim(format!("{} ", fmt::clock_hhmm(e.at))),
            seg(
                format!("{} ", e.kind.label()),
                match e.kind {
                    crate::events::Kind::Api => Tone::Crit,
                    crate::events::Kind::Coach => Tone::Accent,
                    crate::events::Kind::Perm | crate::events::Kind::Cost => Tone::Warn,
                    _ => Tone::Dim,
                },
            ),
            fg(fmt::clip(&e.text, 40)),
        ]
    };
    let n = tail.len();
    let (older, newer) = tail.split_at(n.saturating_sub(3));
    let values: Vec<Line> = newer.iter().rev().map(|e| cell(e)).collect();
    let detail: Vec<Line> = older.iter().rev().map(|e| cell(e)).collect();
    Row {
        digit: 8,
        name: "Events",
        values: if values.is_empty() {
            vec![dim("no events yet")]
        } else {
            joined(values)
        },
        detail: joined(detail),
    }
}

fn row_advisor(c: &Coach, engine: &Engine) -> Row {
    let mut parts: Vec<Line> = Vec::new();
    match &c.nudge {
        Some(n) => parts.push(vec![seg(
            format!("{} {}", n.class.label(), n.id),
            match n.class {
                crate::advisor::Urgency::Now => Tone::Crit,
                crate::advisor::Urgency::Next => Tone::Warn,
                crate::advisor::Urgency::Later => Tone::Fg,
            },
        )]),
        None => parts.push(vec![dim("nothing to fix")]),
    }
    parts.push(vec![dim(c.next_row.replace("next     ", "next "))]);
    parts.push(vec![dim(c.snoozed_row.replace("snoozed  ", "snoozed "))]);
    let mut detail: Vec<Line> = Vec::new();
    for a in engine.current.iter().skip(1).take(3) {
        detail.push(vec![dim(format!(
            "{} {} {}",
            a.urgency.label(),
            a.rule,
            fmt::clip(&a.headline, 40)
        ))]);
    }
    if c.queued == 0 && !c.recent.is_empty() {
        detail.push(vec![dim(c.recent[0].row.clone())]);
    }
    Row {
        digit: 9,
        name: "Advisor",
        values: joined(parts),
        detail: joined(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;

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

    #[test]
    fn fixture_b_dashboard_object() {
        let (s, e) = fixture_b();
        let d = snapshot(&s, &e);
        assert_eq!(d.rows.len(), 9);
        for (i, r) in d.rows.iter().enumerate() {
            assert_eq!(r.digit as usize, i + 1);
            assert!(
                text_of(&r.values).chars().count() <= ROW_WIDTH,
                "{}",
                text_of(&r.values)
            );
            assert!(
                text_of(&r.detail).chars().count() <= ROW_WIDTH,
                "{}",
                text_of(&r.detail)
            );
        }
        assert_eq!(d.tiles.len(), 4);
        let t = &d.tiles[0];
        assert_eq!((t.id, t.figure.as_str(), t.unit), ("context", "14", "%"));
        assert_eq!(t.sub1, "142k of 1.00M");
        assert_eq!(t.sub2, "≈$.03/call");
        assert_eq!((d.tiles[1].figure.as_str(), d.tiles[1].unit), ("59", "m"));
        assert_eq!(d.tiles[1].sub1, "warm · 59m (1h) TTL");
        assert_eq!(d.tiles[1].sub2, "misses 0");
        assert_eq!(
            (d.tiles[2].figure.as_str(), d.tiles[2].sub1.as_str()),
            ("—", "no status line")
        );
        assert_eq!(d.tiles[3].figure, "1");
        assert_eq!(d.tiles[3].sub1, "1 · correction");
        assert_eq!(d.tiles[3].sub2, "edits 3 ✓ none 9h00");
        assert!(
            d.header
                .line
                .starts_with("cctop  claude-sonnet-5 · turn 6 · "),
            "{}",
            d.header.line
        );
        assert_eq!(d.header.phase.word, "COMMITTING");
        assert_eq!(d.header.phase.glyph, '●');
        let n = d.nudge.as_ref().unwrap();
        assert!(
            n.line.starts_with(
                "`cd lorem_ipsum_dolor_sit_amet…` blocked the turn… — queue: 'run builds"
            ),
            "{}",
            n.line
        );
        assert_eq!(n.tag, "LATER · turn 6");
        assert!(
            text_of(&d.rows[0].values).contains("prefix 49k"),
            "{}",
            text_of(&d.rows[0].values)
        );
        assert!(
            text_of(&d.rows[0].detail).starts_with("+"),
            "{}",
            text_of(&d.rows[0].detail)
        );
        assert!(
            text_of(&d.rows[1].values).starts_with("cache read "),
            "{}",
            text_of(&d.rows[1].values)
        );
        assert!(
            text_of(&d.rows[2].values).starts_with("no status line · weight ×3 (sonnet)"),
            "{}",
            text_of(&d.rows[2].values)
        );
        assert!(
            text_of(&d.rows[3].values).starts_with("COMMITTING · 4c +180"),
            "{}",
            text_of(&d.rows[3].values)
        );
        assert!(
            text_of(&d.rows[4].values).starts_with("140 calls · 6 err"),
            "{}",
            text_of(&d.rows[4].values)
        );
        assert_eq!(text_of(&d.rows[5].values), "—");
        assert!(
            text_of(&d.rows[6].values).starts_with("44 touched · "),
            "{}",
            text_of(&d.rows[6].values)
        );
        assert!(
            text_of(&d.rows[7].values).contains("api cost-state"),
            "{}",
            text_of(&d.rows[7].values)
        );
        assert!(
            text_of(&d.rows[8].values).starts_with("LATER A10 · next —"),
            "{}",
            text_of(&d.rows[8].values)
        );
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["rows"][0]["values"][0]["tone"], "accent");
        assert_eq!(v["tiles"][0]["level"], "quiet");
    }

    #[test]
    fn cut_keeps_tones_and_marks_the_edge() {
        let line = vec![fg("abcdef"), dim("ghij")];
        let c = cut(line.clone(), 8);
        assert_eq!(text_of(&c), "abcdefg…");
        assert_eq!(c[1].tone, Tone::Dim);
        assert_eq!(cut(line, 20).len(), 2);
    }
}
