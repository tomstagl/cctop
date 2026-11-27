//! The overview in the TUI (mod-pane US-005): body 1 of the dashboard drawn
//! from the overview object. Context across the width, then spend beside
//! cost per turn, then the cache beside the tool calls; below
//! [`PAIRS`] columns the pairs stack. Every chart is the cells of
//! `cctop_charts::render_cells`, the same function the pane's Raster comes
//! from; verdicts are wrapped, never cut. Labels are terminal text beside
//! whole-cell charts, as in the approved boards (POC 6 and 8).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use cctop_charts::{Cell, Kind, Role};

use crate::dashboard::{self, Tone};
use crate::overview::{Chart, Legend, Options, Overview};
use crate::theme::Theme;
use crate::ui::dashboard::wrap;
use crate::ui::fmt;
use crate::ui::State;
use crate::verdict::{money, Level};

/// Two columns from this many columns wide; the pairs stack below.
pub const PAIRS: u16 = 100;
/// The y-axis labels' column (`412k`, `$20.00`) beside a chart.
const GUTTER: usize = 6;
/// Cells a turn takes in a per-turn chart: two for the mark, one of gap.
const TURN_CELLS: usize = 3;
/// The cache countdown bar's width.
const COUNTDOWN: usize = 30;
/// The widest tool name the heat map's labels keep.
const NAME_MAX: usize = 12;

/// One run of cells in one style.
fn cell_spans<'a>(t: &Theme, cells: &[Cell]) -> Vec<Span<'a>> {
    let mut out: Vec<Span> = Vec::new();
    let mut run = String::new();
    let mut style = Style::default();
    for c in cells {
        let s = Style::default()
            .fg(t.cell_color(c.fg))
            .bg(t.cell_color(c.bg));
        if !run.is_empty() && s != style {
            out.push(Span::styled(std::mem::take(&mut run), style));
        }
        style = s;
        // Without half blocks a painted cell is a `#`.
        let painted = c.ch != ' ' || c.bg.is_some();
        run.push(if t.ascii && painted { '#' } else { c.ch });
    }
    if !run.is_empty() {
        out.push(Span::styled(run, style));
    }
    out
}

/// A chart drawn at `cols` × `rows`: one span list per row.
fn chart_rows<'a>(
    t: &Theme,
    spec: &cctop_charts::ChartSpec,
    cols: u16,
    rows: u16,
) -> Vec<Vec<Span<'a>>> {
    cctop_charts::render_cells(spec, cols, rows)
        .chunks(cols.max(1) as usize)
        .map(|r| cell_spans(t, r))
        .collect()
}

fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

fn pad(n: usize) -> Span<'static> {
    Span::raw(" ".repeat(n))
}

fn role_color(t: &Theme, role: Role) -> Color {
    t.cell_color(Some(cctop_charts::Theme::CELLS.role(role)))
}

/// The turns a per-turn chart's x axis spans.
fn turns_in(c: &Chart) -> usize {
    ((c.spec.x.to - c.spec.x.from).max(0.0) as usize + 1).max(1)
}

/// The cells a chart is drawn at when `avail` cells are free beside its
/// axis labels: the whole width, except a per-turn chart, which takes three
/// cells a turn when the turns are few.
pub fn chart_size(c: &Chart, avail: usize) -> (u16, u16) {
    let avail = avail.clamp(1, 512);
    let (_, rows) = crate::overview::default_size(&c.spec);
    let per_turn = |n: usize| avail.min(n * TURN_CELLS).max(1);
    let cols = match c.spec.kind {
        Kind::Bars | Kind::Pills => per_turn(turns_in(c)),
        Kind::Heat => avail.min(turns_in(c) * 2).max(1),
        _ => avail,
    };
    (cols as u16, rows)
}

/// A y-axis label: tokens as `412k`, dollars compact once they pass $100
/// (`$123`, `$1.2k`) so the gutter stays narrow.
fn axis_label(unit: &str, v: f64) -> String {
    match unit {
        "tokens" => fmt::tokens(v.max(0.0) as u64),
        "USD" if v == 0.0 => "$0".to_string(),
        "USD" if v >= 1000.0 => format!("${:.1}k", v / 1000.0),
        "USD" if v >= 100.0 => format!("${v:.0}"),
        "USD" => money(v),
        _ => format!("{v:.0}"),
    }
}

/// The three y labels a chart with an axis column shows.
fn y_labels(c: &Chart, rows: u16) -> [String; 3] {
    let y = &c.spec.y;
    [
        axis_label(&y.unit, y.to),
        if rows >= 6 {
            axis_label(&y.unit, (y.from + y.to) / 2.0)
        } else {
            String::new()
        },
        axis_label(&y.unit, y.from),
    ]
}

/// Where a chart sits in its section: the label column to its left, and
/// the cells it is drawn at. One function for the renderer and the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    pub gutter: usize,
    pub cols: u16,
    pub rows: u16,
}

/// The tool names' column beside the heat map.
fn names_gutter(c: &Chart) -> usize {
    c.spec
        .series
        .iter()
        .map(|s| fmt::clip(&s.name, NAME_MAX).chars().count())
        .max()
        .unwrap_or(0)
        + 1
}

/// `c`'s [`Dims`] in a section `w` cells wide: the y axis' column is as
/// wide as its widest label (at least [`GUTTER`]), the cache's strip has
/// none, the heat map's holds the tool names.
pub fn dims(c: &Chart, w: usize) -> Dims {
    let gutter = match c.id {
        "cache" => 0,
        "tool_calls" => names_gutter(c),
        _ => {
            let (_, rows) = crate::overview::default_size(&c.spec);
            y_labels(c, rows)
                .iter()
                .map(|l| l.chars().count() + 1)
                .max()
                .unwrap_or(GUTTER)
                .max(GUTTER)
        }
    };
    let (cols, rows) = chart_size(c, w.saturating_sub(gutter));
    Dims { gutter, cols, rows }
}

/// The screen split: context across `inner` cells, the pairs in two
/// columns of `left` and `right` from [`PAIRS`] columns wide, else stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub inner: usize,
    pub two: bool,
    pub left: usize,
    pub right: usize,
}

pub fn layout(width: u16) -> Layout {
    let inner = (width as usize).saturating_sub(2).max(1);
    // The rule between the columns takes three cells.
    let left = inner.saturating_sub(3) / 2;
    Layout {
        inner,
        two: width >= PAIRS,
        left,
        right: inner.saturating_sub(3) - left,
    }
}

impl Layout {
    /// The cells a chart's section is drawn in.
    pub fn section_width(&self, id: &str) -> usize {
        match (self.two, id) {
            (true, "spend" | "cache") => self.left,
            (true, "cost_per_turn" | "tool_calls") => self.right,
            _ => self.inner,
        }
    }
}

/// The overview for a terminal `width` columns wide: the per-turn charts
/// keep as many turns as fit at two cells a turn (§5.2), not a fixed 50.
pub fn build(state: &State, width: u16) -> Overview {
    let l = layout(width);
    let narrowest = if l.two { l.right } else { l.inner };
    crate::overview::build_with(
        state,
        &Options {
            // The heat map's tool names take the widest column.
            max_turns: (narrowest.saturating_sub(NAME_MAX + 1) / 2).max(1),
            chain_usd: state.chain_usd(),
            fallback_cells: false,
        },
    )
}

fn level_tone(l: Level) -> Tone {
    match l {
        Level::Good => Tone::Ok,
        Level::Watch => Tone::Warn,
        Level::Act => Tone::Crit,
    }
}

/// `Title  figure` with the note (or the "last N turns" label) right-aligned.
fn title_line<'a>(t: &Theme, c: &Chart, w: usize) -> Line<'a> {
    // The cache's note (the countdown) is drawn once, beside its bar.
    let note = c.note.clone().filter(|_| c.id != "cache");
    let right = note.or_else(|| c.label.clone()).unwrap_or_default();
    let figure = fmt::clip(&c.figure, w.saturating_sub(c.title.chars().count() + 2));
    let used = c.title.chars().count() + 2 + figure.chars().count();
    let right = if right.chars().count() + 2 + used <= w {
        right
    } else {
        String::new()
    };
    let gap = w.saturating_sub(used + right.chars().count());
    Line::from(vec![
        Span::styled(
            c.title.to_string(),
            Style::default()
                .fg(t.fg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(t.coach_text(&figure), t.accent()),
        pad(gap),
        Span::styled(t.coach_text(&right), t.dim()),
    ])
}

/// The verdict: `Good` / `Watch` / `Act` in its colour, the sentence, then
/// the `Do:` in bold, wrapped under the word.
fn verdict_lines<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let v = &c.verdict;
    let label = v.level.label();
    let mut segs = vec![dashboard::seg(format!("{label} "), level_tone(v.level))];
    segs.push(dashboard::fg(v.text.clone()));
    if let Some(todo) = &v.todo {
        segs.push(dashboard::fg(" "));
        segs.push(dashboard::seg(todo.clone(), Tone::Bold));
    }
    wrap(t, &segs, w, label.chars().count() + 1)
}

/// `■ text` per legend entry, the swatch in the chart's own colour, wrapped
/// at `w`; an entry longer than the row is clipped.
fn legend_lines<'a>(t: &Theme, items: &[Legend], w: usize) -> Vec<Line<'a>> {
    let mut rows: Vec<Vec<Span>> = vec![Vec::new()];
    let mut used = 0usize;
    for l in items {
        let text = t.coach_text(&fmt::clip(&l.text, w.saturating_sub(2)));
        let n = 2 + text.chars().count();
        if used > 0 && used + 2 + n > w {
            rows.push(Vec::new());
            used = 0;
        }
        let row = rows.last_mut().unwrap();
        if used > 0 {
            row.push(pad(2));
            used += 2;
        }
        let swatch = if t.ascii { "#" } else { "■" };
        row.push(Span::styled(
            swatch,
            Style::default().fg(role_color(t, l.role)),
        ));
        row.push(Span::styled(format!(" {text}"), t.dim()));
        used += n;
    }
    rows.into_iter()
        .filter(|r| !r.is_empty())
        .map(Line::from)
        .collect()
}

/// Turn numbers under a per-turn chart, one per slot where they fit.
fn turn_numbers<'a>(t: &Theme, c: &Chart, indent: usize, cols: usize) -> Line<'a> {
    let n = turns_in(c);
    let first = c.spec.x.from as usize;
    let mut text = String::new();
    for i in 0..n {
        let at = i * cols / n;
        let label = (first + i).to_string();
        if at > text.chars().count() || (at == 0 && text.is_empty()) {
            text.push_str(&" ".repeat(at - text.chars().count()));
        } else {
            continue;
        }
        if at + label.len() <= cols {
            text.push_str(&label);
            text.push(' ');
        }
    }
    let text = text.trim_end().to_string();
    Line::from(vec![pad(indent), Span::styled(text, t.dim())])
}

/// A chart with a y-axis gutter on its left: the top, middle and bottom
/// labels of the axis, the cells, the x-axis ends beneath.
fn gutter_chart<'a>(t: &Theme, c: &Chart, w: usize, numbers: bool) -> Vec<Line<'a>> {
    let Dims { gutter, cols, rows } = dims(c, w);
    let body = chart_rows(t, &c.spec, cols, rows);
    let last = body.len().saturating_sub(1);
    let [top, mid, bottom] = y_labels(c, rows);
    let mut out: Vec<Line> = Vec::new();
    for (i, cells) in body.into_iter().enumerate() {
        let label = if i == 0 {
            top.clone()
        } else if i == last {
            bottom.clone()
        } else if i == last / 2 {
            mid.clone()
        } else {
            String::new()
        };
        let mut spans = vec![Span::styled(format!("{label:<gutter$}"), t.dim())];
        spans.extend(cells);
        out.push(Line::from(spans));
    }
    if numbers {
        out.push(turn_numbers(t, c, gutter, cols as usize));
    } else {
        let from = format!(
            "{} {}",
            c.spec.x.unit,
            axis_label(&c.spec.x.unit, c.spec.x.from)
        );
        let to = axis_label(&c.spec.x.unit, c.spec.x.to);
        let gap = (cols as usize).saturating_sub(from.chars().count() + to.chars().count());
        out.push(Line::from(vec![
            pad(gutter),
            Span::styled(format!("{from}{}{to}", " ".repeat(gap)), t.dim()),
        ]));
    }
    out
}

/// The chart's last readout (the latest turn), wrapped dim under it.
fn latest_readout<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let Some(r) = c.readouts.last() else {
        return Vec::new();
    };
    let mut rows = wrap(t, &[dashboard::dim(r.text.clone())], w, 2);
    rows.truncate(2);
    rows
}

fn blank<'a>() -> Line<'a> {
    Line::from("")
}

fn context<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    if let Some(bar) = &c.bar {
        for cells in chart_rows(t, bar, w.clamp(1, 512) as u16, 1) {
            out.push(Line::from(cells));
        }
        out.extend(legend_lines(t, &c.bar_legend, w));
    }
    out.extend(legend_lines(t, &c.legend, w));
    out.extend(gutter_chart(t, c, w, false));
    out.extend(latest_readout(t, c, w));
    out
}

fn spend<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, &c.legend, w));
    out.extend(gutter_chart(t, c, w, false));
    out
}

fn cost_per_turn<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, &c.legend, w));
    out.extend(gutter_chart(t, c, w, true));
    out.extend(latest_readout(t, c, w));
    out
}

fn cache<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, &c.legend, w));
    let Dims { cols, rows, .. } = dims(c, w);
    for cells in chart_rows(t, &c.spec, cols, rows) {
        out.push(Line::from(cells));
    }
    out.push(turn_numbers(t, c, 0, cols as usize));
    if let (Some(note), Some(bar)) = (&c.note, &c.bar) {
        let label = format!("{note} ");
        let room = w.saturating_sub(label.chars().count()).clamp(1, COUNTDOWN);
        let mut spans = vec![Span::styled(t.coach_text(&label), t.dim())];
        if let Some(cells) = chart_rows(t, bar, room as u16, 1).pop() {
            spans.extend(cells);
        }
        out.push(Line::from(spans));
    }
    if !c.prevent.is_empty() {
        out.push(blank());
        out.push(Line::from(Span::styled(
            "How to avoid each rebuild",
            t.dim().add_modifier(ratatui::style::Modifier::BOLD),
        )));
        for p in &c.prevent {
            let head = format!("turn {}", p.turn);
            let indent = 9usize.max(head.chars().count() + 1);
            let cause = format!("{} · {}", p.cause, fmt::tokens(p.tokens));
            out.push(Line::from(vec![
                Span::styled(head.clone(), t.crit()),
                pad(indent - head.chars().count()),
                Span::styled(
                    t.coach_text(&fmt::clip(&cause, w.saturating_sub(indent))),
                    t.dim(),
                ),
            ]));
            out.push(Line::from(vec![
                pad(indent),
                Span::styled(
                    t.coach_text(&fmt::clip(p.habit, w.saturating_sub(indent))),
                    Style::default().fg(t.fg),
                ),
            ]));
        }
    }
    out
}

fn tool_calls<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, &c.legend, w));
    let names: Vec<String> = c
        .spec
        .series
        .iter()
        .map(|s| fmt::clip(&s.name, NAME_MAX))
        .collect();
    let Dims { gutter, cols, rows } = dims(c, w);
    for (i, cells) in chart_rows(t, &c.spec, cols, rows).into_iter().enumerate() {
        let name = names.get(i).cloned().unwrap_or_default();
        let mut spans = vec![Span::styled(
            format!("{name:>width$} ", width = gutter - 1),
            t.dim(),
        )];
        spans.extend(cells);
        out.push(Line::from(spans));
    }
    out.push(turn_numbers(t, c, gutter, cols as usize));
    out
}

fn section<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    match c.id {
        "context" => context(t, c, w),
        "spend" => spend(t, c, w),
        "cost_per_turn" => cost_per_turn(t, c, w),
        "cache" => cache(t, c, w),
        _ => tool_calls(t, c, w),
    }
}

/// Two blocks side by side, a rule between: the shorter padded with blanks.
fn pair<'a>(t: &Theme, left: Vec<Line<'a>>, right: Vec<Line<'a>>, lw: usize) -> Vec<Line<'a>> {
    let n = left.len().max(right.len());
    let (mut left, mut right) = (left.into_iter(), right.into_iter());
    (0..n)
        .map(|_| {
            let mut spans = left.next().map(|l| l.spans).unwrap_or_default();
            spans.push(pad(lw.saturating_sub(width_of(&spans))));
            spans.push(Span::styled(
                t.coach_text(" │ "),
                Style::default().fg(t.border),
            ));
            spans.extend(right.next().map(|l| l.spans).unwrap_or_default());
            Line::from(spans)
        })
        .collect()
}

fn rule<'a>(t: &Theme, w: usize) -> Line<'a> {
    Line::from(Span::styled(
        t.coach_text(&"─".repeat(w)),
        Style::default().fg(t.border),
    ))
}

fn indent(lines: Vec<Line<'_>>) -> Vec<Line<'_>> {
    lines
        .into_iter()
        .map(|mut l| {
            l.spans.insert(0, pad(1));
            l
        })
        .collect()
}

/// The overview's rows at `width`: no height limit, the caller cuts.
pub fn rows<'a>(t: &Theme, o: &Overview, width: u16) -> Vec<Line<'a>> {
    let l = layout(width);
    let by = |id: &str| o.charts.iter().find(|c| c.id == id);
    let mut out: Vec<Line> = Vec::new();
    let mut add = |lines: Vec<Line<'a>>| out.extend(indent(lines));
    if let Some(c) = by("context") {
        add(section(t, c, l.inner));
        add(vec![rule(t, l.inner)]);
    }
    for (a, b) in [("spend", "cost_per_turn"), ("cache", "tool_calls")] {
        let (Some(a), Some(b)) = (by(a), by(b)) else {
            continue;
        };
        if l.two {
            add(pair(
                t,
                section(t, a, l.left),
                section(t, b, l.right),
                l.left,
            ));
        } else {
            add(section(t, a, l.inner));
            add(vec![blank()]);
            add(section(t, b, l.inner));
        }
        add(vec![rule(t, l.inner)]);
    }
    add(vec![Line::from(Span::styled(
        t.coach_text(&o.footer),
        t.dim(),
    ))]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::ui::state::{SessionInfo, State};
    use std::path::Path;

    fn load(name: &str) -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = crate::theme::Caps::full();
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        app.set_theme("default-dark");
        app
    }

    fn text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn overview_snapshots() {
        let app = load("session-b.jsonl");
        for w in [80u16, 122, 190] {
            let lines = rows(&app.state.theme, &build(&app.state, w), w);
            for l in &lines {
                assert!(width_of(&l.spans) <= w as usize, "{w}: {l:?}");
            }
            insta::assert_snapshot!(format!("overview_b_{w}"), text(&lines));
        }
    }

    /// Fixture T has 56 turns: at 122 columns the per-turn charts keep what
    /// fits at two cells a turn (26), labelled, not a fixed fifty.
    #[test]
    fn a_long_session_keeps_the_turns_that_fit_at_two_cells_each() {
        let app = load("session-t.jsonl");
        for w in [122u16, 190] {
            let o = build(&app.state, w);
            let l = layout(w);
            let narrowest = if l.two { l.right } else { l.inner };
            let cost = o.charts.iter().find(|c| c.id == "cost_per_turn").unwrap();
            let shown = turns_in(cost);
            assert!(shown > 20, "{w}: {shown}");
            assert_eq!(shown, (narrowest - NAME_MAX - 1) / 2, "{w}");
            assert_eq!(
                cost.label.as_deref(),
                Some(format!("last {shown} turns").as_str())
            );
            for c in o
                .charts
                .iter()
                .filter(|c| c.id != "context" && c.id != "spend")
            {
                let d = dims(c, l.section_width(c.id));
                assert!(
                    d.cols as usize >= turns_in(c) * 2,
                    "{w} {}: {} cells for {} turns",
                    c.id,
                    d.cols,
                    turns_in(c)
                );
            }
        }
        let lines = rows(&app.state.theme, &build(&app.state, 122), 122);
        insta::assert_snapshot!("overview_t_122", text(&lines));
    }

    /// Every chart of the overview is `render_cells` of its spec, cell for
    /// cell: the rows appear in the output as the very spans that cells
    /// give, and those spans expand back to the cells (the colour of
    /// `None` is the terminal's default).
    #[test]
    fn charts_are_the_cells_the_renderer_draws() {
        let app = load("session-b.jsonl");
        let t = &app.state.theme;
        for w in [80u16, 122, 190] {
            let o = build(&app.state, w);
            let lines = rows(t, &o, w);
            let l = layout(w);
            for c in &o.charts {
                let Dims { cols, rows: n, .. } = dims(c, l.section_width(c.id));
                let cells = cctop_charts::render_cells(&c.spec, cols, n);
                for (r, row) in chart_rows(t, &c.spec, cols, n).iter().enumerate() {
                    assert!(
                        lines
                            .iter()
                            .any(|l| l.spans.windows(row.len()).any(|win| win == row.as_slice())),
                        "{w} {} row {r} is not drawn",
                        c.id
                    );
                    let back: Vec<_> = row
                        .iter()
                        .flat_map(|sp| sp.content.chars().map(|ch| (ch, sp.style.fg, sp.style.bg)))
                        .collect();
                    let want: Vec<_> = cells[r * cols as usize..(r + 1) * cols as usize]
                        .iter()
                        .map(|c| (c.ch, Some(t.cell_color(c.fg)), Some(t.cell_color(c.bg))))
                        .collect();
                    assert_eq!(back, want, "{w} {} row {r}", c.id);
                }
            }
        }
    }

    /// Below 100 columns the pairs stack: no rule between columns.
    #[test]
    fn pairs_stack_below_a_hundred_columns() {
        let app = load("session-b.jsonl");
        let t = &app.state.theme;
        let has_rule = |w| {
            text(&rows(t, &build(&app.state, w), w))
                .lines()
                .any(|l| l.contains(" │ "))
        };
        assert!(!has_rule(99));
        assert!(has_rule(100));
    }

    /// A spend axis past $100 widens the label column: every chart row
    /// starts its cells at the same column, and the labels stay compact.
    #[test]
    fn a_wide_spend_axis_keeps_the_cells_aligned() {
        let app = load("session-b.jsonl");
        let t = &app.state.theme;
        let mut o = build(&app.state, 122);
        let spend = o.charts.iter_mut().find(|c| c.id == "spend").unwrap();
        for top in [123.4, 1234.5, 12345.0] {
            spend.spec.y.to = top;
            let lines = gutter_chart(t, spend, 58, false);
            let first: Vec<usize> = lines
                .iter()
                .map(|l| l.spans[0].content.chars().count())
                .collect();
            assert!(first.iter().all(|n| *n == first[0]), "{top}: {first:?}");
            let label = lines[0].spans[0].content.trim().to_string();
            assert!(label.starts_with('$') && label.len() <= 6, "{label}");
            assert_eq!(dims(spend, 58).gutter, first[0]);
            for l in &lines {
                assert!(width_of(&l.spans) <= 58, "{top}: {l:?}");
            }
        }
        assert_eq!(axis_label("USD", 123.4), "$123");
        assert_eq!(axis_label("USD", 1234.5), "$1.2k");
    }

    // -- PRD 1–7 contributions, present and absent (US-011) -----------------

    fn shown(app: &App) -> String {
        text(&rows(&app.state.theme, &build(&app.state, 190), 190))
    }

    /// Each fact is drawn when the state has it and gone when it does not;
    /// the same objects the pane draws (tests/pane/overview-charts.test.ts).
    #[test]
    fn prd_contributions_are_drawn_when_present_and_gone_when_absent() {
        use crate::ui::state::tests_support::llm_span;
        let mut app = load("session-b.jsonl");
        let base = shown(&app);
        for needle in [
            "thinking dropped",
            "budget $25.00",
            "past 200k",
            "spend limit",
            "retries",
            "Hooks add",
            "How to avoid each rebuild",
            "per /context",
        ] {
            let expect = needle == "per /context";
            assert_eq!(base.contains(needle), expect, "{needle}\n{base}");
        }
        let st = &mut app.state;
        let turn = st.agg.turns.last().unwrap().clone();
        st.agg.turns.last_mut().unwrap().thinking_dropped_blocks = 2;
        st.budget_usd = Some((10.0, 25.0, 15.0));
        st.status_facts.exceeds_200k_tokens = true;
        st.status_facts.spend_limit_usd = Some((412.0, Some(500.0)));
        st.status_facts.spend_limit_period = Some("monthly".into());
        let start_ns = (crate::metrics::cost::parse_ts_ms(turn.started_at.as_deref().unwrap())
            .unwrap() as u64
            + 1_000)
            * 1_000_000;
        st.apply_otel(&crate::otel::SessionData {
            spans: vec![
                llm_span(&turn.request_ids[0], 3, 16),
                crate::otel::Span::Hook {
                    ids: crate::otel::SpanIds {
                        trace_id: "tr".into(),
                        span_id: "h".into(),
                        parent_span_id: None,
                        start_ns,
                        end_ns: start_ns + 900_000_000,
                    },
                    hook_event: Some("Stop".into()),
                    hook_name: None,
                    num_hooks: Some(1),
                    duration_ms: Some(900),
                    num_blocking: None,
                },
            ]
            .into(),
            ..Default::default()
        });
        let with = shown(&app);
        for needle in [
            "thinking dropped",
            "budget $25.00",
            "past 200k",
            "spend limit",
            "Hooks add",
            "per /context",
        ] {
            assert!(with.contains(needle), "{needle}\n{with}");
        }
        // Located rebuilds: fixture R has them, B does not.
        let r = shown(&load("session-r.jsonl"));
        assert!(r.contains("How to avoid each rebuild"), "{r}");
        // A session with no capture draws no composition.
        assert!(!shown(&load("session-a.jsonl")).contains("per /context"));
        // Skill shell: its own heat-map row.
        let mut app = load("session-b.jsonl");
        let at = crate::metrics::cost::parse_ts_ms(
            app.state.agg.turns[0].started_at.as_deref().unwrap(),
        )
        .unwrap();
        app.state.apply_hook(&crate::hooks::HookEvent {
            at: at + 5_000,
            event: "PreSkillShellCommand".into(),
            session_id: "s".into(),
            tool_use_id: None,
            tool_name: None,
            payload: serde_json::json!({"command_name": "review", "tool_input": {"command": "git diff"}}),
        });
        assert!(shown(&app).contains("skill shell commands"));
    }
}
