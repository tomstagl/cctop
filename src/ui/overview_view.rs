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
use crate::overview::{Chart, Overview};
use crate::theme::Theme;
use crate::ui::dashboard::wrap;
use crate::ui::fmt;
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

fn axis_label(unit: &str, v: f64) -> String {
    match unit {
        "tokens" => fmt::tokens(v.max(0.0) as u64),
        "USD" if v == 0.0 => "$0".to_string(),
        "USD" => money(v),
        _ => format!("{v:.0}"),
    }
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
    let right = c
        .note
        .clone()
        .or_else(|| c.label.clone())
        .unwrap_or_default();
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
/// at `w`.
fn legend_lines<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut rows: Vec<Vec<Span>> = vec![Vec::new()];
    let mut used = 0usize;
    for l in &c.legend {
        let text = t.coach_text(&l.text);
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
    let (cols, rows) = chart_size(c, w.saturating_sub(GUTTER));
    let body = chart_rows(t, &c.spec, cols, rows);
    let last = body.len().saturating_sub(1);
    let mut out: Vec<Line> = Vec::new();
    for (i, cells) in body.into_iter().enumerate() {
        let label = if i == 0 {
            axis_label(&c.spec.y.unit, c.spec.y.to)
        } else if i == last {
            axis_label(&c.spec.y.unit, c.spec.y.from)
        } else if rows >= 6 && i == last / 2 {
            axis_label(&c.spec.y.unit, (c.spec.y.from + c.spec.y.to) / 2.0)
        } else {
            String::new()
        };
        let mut spans = vec![Span::styled(format!("{label:<GUTTER$}"), t.dim())];
        spans.extend(cells);
        out.push(Line::from(spans));
    }
    if numbers {
        out.push(turn_numbers(t, c, GUTTER, cols as usize));
    } else {
        let from = format!(
            "{} {}",
            c.spec.x.unit,
            axis_label(&c.spec.x.unit, c.spec.x.from)
        );
        let to = axis_label(&c.spec.x.unit, c.spec.x.to);
        let gap = (cols as usize).saturating_sub(from.chars().count() + to.chars().count());
        out.push(Line::from(vec![
            pad(GUTTER),
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
    }
    out.extend(legend_lines(t, c, w));
    out.extend(gutter_chart(t, c, w, false));
    out.extend(latest_readout(t, c, w));
    out
}

fn spend<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, c, w));
    out.extend(gutter_chart(t, c, w, false));
    out
}

fn cost_per_turn<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, c, w));
    out.extend(gutter_chart(t, c, w, true));
    out.extend(latest_readout(t, c, w));
    out
}

fn cache<'a>(t: &Theme, c: &Chart, w: usize) -> Vec<Line<'a>> {
    let mut out = vec![title_line(t, c, w)];
    out.extend(verdict_lines(t, c, w));
    out.extend(legend_lines(t, c, w));
    let (cols, rows) = chart_size(c, w);
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
    out.extend(legend_lines(t, c, w));
    let names: Vec<String> = c
        .spec
        .series
        .iter()
        .map(|s| fmt::clip(&s.name, NAME_MAX))
        .collect();
    let gutter = names.iter().map(|n| n.chars().count()).max().unwrap_or(0) + 1;
    let rows = c.spec.series.len().clamp(1, 8) as u16;
    let (cols, _) = chart_size(c, w.saturating_sub(gutter));
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
    let inner = (width as usize).saturating_sub(2).max(1);
    let by = |id: &str| o.charts.iter().find(|c| c.id == id);
    let mut out: Vec<Line> = Vec::new();
    let mut add = |lines: Vec<Line<'a>>| out.extend(indent(lines));
    let two = width >= PAIRS;
    // Each column is half the width less the rule's three cells.
    let lw = inner.saturating_sub(3) / 2;
    let rw = inner.saturating_sub(3) - lw;
    if let Some(c) = by("context") {
        add(section(t, c, inner));
        add(vec![rule(t, inner)]);
    }
    for (a, b) in [("spend", "cost_per_turn"), ("cache", "tool_calls")] {
        let (Some(a), Some(b)) = (by(a), by(b)) else {
            continue;
        };
        if two {
            add(pair(t, section(t, a, lw), section(t, b, rw), lw));
        } else {
            add(section(t, a, inner));
            add(vec![blank()]);
            add(section(t, b, inner));
        }
        add(vec![rule(t, inner)]);
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

    fn fixture_b() -> (App, Overview) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = crate::theme::Caps::full();
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        app.set_theme("default-dark");
        let o = crate::overview::build(&app.state);
        (app, o)
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
        let (app, o) = fixture_b();
        for w in [80u16, 122, 190] {
            let lines = rows(&app.state.theme, &o, w);
            for l in &lines {
                assert!(width_of(&l.spans) <= w as usize, "{w}: {l:?}");
            }
            insta::assert_snapshot!(format!("overview_b_{w}"), text(&lines));
        }
    }

    /// Every chart of the overview is `render_cells` of its spec, cell for
    /// cell: the rows appear in the output as the very spans that cells
    /// give, and those spans expand back to the cells (the colour of
    /// `None` is the terminal's default).
    #[test]
    fn charts_are_the_cells_the_renderer_draws() {
        let (app, o) = fixture_b();
        let t = &app.state.theme;
        for w in [80u16, 122, 190] {
            let lines = rows(t, &o, w);
            let inner = w as usize - 2;
            let lw = (inner - 3) / 2;
            let rw = inner - 3 - lw;
            for c in &o.charts {
                let avail = match (w >= PAIRS, c.id) {
                    (_, "context") | (false, _) => inner,
                    (true, "spend" | "cache") => lw,
                    (true, _) => rw,
                };
                let (cols, rows_n) = match c.id {
                    "cache" => chart_size(c, avail),
                    "tool_calls" => {
                        let g = c
                            .spec
                            .series
                            .iter()
                            .map(|s| fmt::clip(&s.name, NAME_MAX).chars().count())
                            .max()
                            .unwrap_or(0)
                            + 1;
                        (
                            chart_size(c, avail - g).0,
                            c.spec.series.len().clamp(1, 8) as u16,
                        )
                    }
                    "context" => (chart_size(c, avail - GUTTER).0, chart_size(c, avail).1),
                    _ => (chart_size(c, avail - GUTTER).0, chart_size(c, avail).1),
                };
                let cells = cctop_charts::render_cells(&c.spec, cols, rows_n);
                let expected = chart_rows(t, &c.spec, cols, rows_n);
                for (r, row) in expected.iter().enumerate() {
                    assert!(
                        lines
                            .iter()
                            .any(|l| l.spans.windows(row.len()).any(|win| win == row.as_slice())),
                        "{w} {} row {r} is not drawn",
                        c.id
                    );
                    // The spans are the cells.
                    let mut back = Vec::new();
                    for sp in row {
                        for ch in sp.content.chars() {
                            back.push((ch, sp.style.fg, sp.style.bg));
                        }
                    }
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
        let (app, o) = fixture_b();
        let t = &app.state.theme;
        let has_rule = |w| text(&rows(t, &o, w)).lines().any(|l| l.contains(" │ "));
        assert!(!has_rule(99));
        assert!(has_rule(100));
    }
}
