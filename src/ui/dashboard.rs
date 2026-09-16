//! The dashboard — Console (PRD dashboard-v2 §4): a header that never
//! moves and one body that fills every remaining row. Row 1 is the
//! identity line with the phase cell right-aligned; rows 2–4 the six
//! cells, three per row at 80 columns and two below; then the act line,
//! wrapped rather than cut; the rule line naming the open body; and the
//! body's rows, each cut once here, at this width. Drawn from
//! [`crate::dashboard::Dashboard`], which is width-free: the column stops
//! below are the prototype's (`tasks/design-dashboard-v2/proto/`).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::dashboard::{self, Dashboard, Seg, Tone};
use crate::theme::Theme;

pub const FOOTER: &str = " ?help  1-6 a 0 body  Enter panel  Esc home  c coach  A ask  t theme  q";
/// Three cells per row from this many columns; two below.
pub const THREE_CELLS: u16 = 80;
/// A cell's middle form from this many cells wide; the short one below.
pub const MID_CELL: usize = 28;
/// The act line's right-aligned tail (`a:advisor`) from this width.
pub const ACT_TAIL: u16 = 66;
/// The act line's full copy from this width; the short one below.
pub const ACT_FULL: u16 = 72;

fn tone_style(t: &Theme, tone: Tone) -> Style {
    match tone {
        Tone::Fg => Style::default().fg(t.fg),
        Tone::Dim => t.dim(),
        Tone::Accent => t.accent(),
        Tone::Ok => t.ok(),
        Tone::Warn => t.warn(),
        Tone::Crit => t.crit(),
        Tone::Bold => Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        Tone::S0 | Tone::S1 | Tone::S2 => {
            let series = t.series(3);
            let i = match tone {
                Tone::S0 => 0,
                Tone::S1 => 1,
                _ => 2,
            };
            Style::default().fg(series[i])
        }
    }
}

/// Segments to spans, cut once at `width`.
fn spans<'a>(t: &Theme, segs: &[Seg], width: usize) -> Vec<Span<'a>> {
    dashboard::cut(segs.to_vec(), width)
        .into_iter()
        .map(|s| Span::styled(t.coach_text(&s.text), tone_style(t, s.tone)))
        .collect()
}

fn cells_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Row 1: ` cctop` bold, the session facts dim, the phase cell
/// (`● WORKING 52:11`) right-aligned in its colour.
fn header<'a>(t: &Theme, d: &Dashboard, width: usize) -> Line<'a> {
    let facts = d.header.line.trim_start_matches("cctop  ").to_string();
    let phase = t.coach_text(&format!(
        "{} {} {}",
        d.header.phase.glyph, d.header.phase.word, d.header.phase.elapsed
    ));
    let phase = crate::ui::fmt::clip(&phase, width.saturating_sub(10).min(40));
    let facts_room = width.saturating_sub(8 + phase.chars().count() + 2);
    let facts = crate::ui::fmt::clip(&t.coach_text(&facts), facts_room);
    let pad = width.saturating_sub(8 + facts.chars().count() + phase.chars().count());
    let phase_style = match d.header.phase.glyph {
        '◆' => t.warn(),
        '○' => t.dim(),
        _ => t.ok(),
    }
    .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" cctop  ", t.accent().add_modifier(Modifier::BOLD)),
        Span::styled(facts, t.dim()),
        Span::raw(" ".repeat(pad)),
        Span::styled(phase, phase_style),
    ])
}

/// The cell text form for a row of `per_row` cells at `width`.
fn cell_form(width: u16, cell_width: usize) -> fn(&dashboard::Cell) -> &dashboard::Line {
    if width >= THREE_CELLS {
        |c| &c.label
    } else if cell_width >= MID_CELL {
        |c| &c.mid
    } else {
        |c| &c.short
    }
}

/// Rows 2–4: the cells, `k:` in the accent, the open one in `ok`, padded
/// to the cell width.
fn cell_rows<'a>(t: &Theme, d: &Dashboard, width: u16, open: usize) -> Vec<Line<'a>> {
    let w = width as usize;
    let per_row = if width >= THREE_CELLS { 3 } else { 2 };
    let cw = w.saturating_sub(2) / per_row;
    let form = cell_form(width, cw);
    let mut out = Vec::new();
    for chunk in d.cells.chunks(per_row) {
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        for c in chunk {
            let active = d.bodies.get(open).is_some_and(|b| b.id == c.opens);
            let key_style = if active { t.ok() } else { t.accent() }.add_modifier(Modifier::BOLD);
            // `k: ` as the engine draws a plain Button's hotkey, then the
            // text, a cell of `cw`; a long text is cut a cell short so the
            // gap to the next cell survives.
            spans.push(Span::styled(format!("{}: ", c.key), key_style));
            let text = dashboard::cut(form(c).clone(), cw.saturating_sub(4));
            let used = 3 + dashboard::width_of(&text);
            for s in text {
                let style = if active {
                    t.ok().add_modifier(Modifier::BOLD)
                } else {
                    tone_style(t, s.tone)
                };
                spans.push(Span::styled(t.coach_text(&s.text), style));
            }
            if cw > used {
                spans.push(Span::raw(" ".repeat(cw - used)));
            }
        }
        out.push(Line::from(spans));
    }
    out
}

/// Word-wrap a line's segments into rows of at most `width` cells, never
/// cutting mid-fact (FR-7): a break falls at a space, or at a segment edge.
fn wrap<'a>(t: &Theme, segs: &[Seg], width: usize, indent: usize) -> Vec<Line<'a>> {
    let mut rows: Vec<Vec<Span>> = vec![Vec::new()];
    let mut used = 0usize;
    for s in segs {
        let style = tone_style(t, s.tone);
        let mut pending = String::new();
        let flush = |pending: &mut String, rows: &mut Vec<Vec<Span>>| {
            if !pending.is_empty() {
                rows.last_mut()
                    .unwrap()
                    .push(Span::styled(t.coach_text(&std::mem::take(pending)), style));
            }
        };
        for word in s.text.split_inclusive(' ') {
            let wlen = word.chars().count();
            if used + wlen > width && used > indent {
                flush(&mut pending, &mut rows);
                rows.push(vec![Span::raw(" ".repeat(indent))]);
                used = indent;
                let word = word.trim_start();
                pending.push_str(word);
                used += word.chars().count();
            } else {
                pending.push_str(word);
                used += wlen;
            }
        }
        flush(&mut pending, &mut rows);
    }
    rows.into_iter().map(Line::from).collect()
}

/// Row 5: the act line, `a:advisor` right-aligned at `ACT_TAIL` columns,
/// the short copy below `ACT_FULL`. Wrapped onto a second row rather than
/// cut when the copy is longer than the width.
fn act_rows<'a>(t: &Theme, d: &Dashboard, width: u16, open: usize) -> Vec<Line<'a>> {
    let w = width as usize;
    let text = if width >= ACT_FULL {
        &d.act.line
    } else {
        &d.act.short
    };
    let mut segs: Vec<Seg> = vec![dashboard::dim(" ")];
    segs.extend(text.iter().cloned());
    if !d.act.tag.is_empty() && width >= ACT_FULL {
        segs.push(dashboard::dim(format!("  {}", d.act.tag)));
    }
    if d.act.acting {
        segs.push(dashboard::dim(" · acting…"));
    }
    let tail: Vec<Span> = if width >= ACT_TAIL {
        let active = d.bodies.get(open).is_some_and(|b| b.id == "advisor");
        let key = if active { t.ok() } else { t.accent() }.add_modifier(Modifier::BOLD);
        vec![Span::styled("a: ", key), Span::styled("advisor ", t.dim())]
    } else {
        Vec::new()
    };
    let tail_w = cells_of(&tail);
    let mut rows = wrap(t, &segs, w.saturating_sub(tail_w + 1), 3);
    if rows.len() > 2 {
        rows.truncate(2);
    }
    if !tail.is_empty() {
        let first = &mut rows[0];
        let pad = w.saturating_sub(cells_of(&first.spans) + tail_w);
        first.spans.push(Span::raw(" ".repeat(pad)));
        first.spans.extend(tail);
    }
    rows
}

/// Row 6: `─── title ─────── 0 home  ·  keys ───`.
fn rule_line<'a>(t: &Theme, body: &dashboard::Body, width: usize, home: bool) -> Line<'a> {
    let border = Style::default().fg(t.border);
    let key = if home { t.ok() } else { t.accent() }.add_modifier(Modifier::BOLD);
    let head = vec![
        Span::styled(t.coach_text("─── "), border),
        Span::styled(
            body.title.to_string(),
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", border),
    ];
    let mut tail = vec![
        Span::styled(" ", border),
        Span::styled("0: ", key),
        Span::styled("home", t.dim()),
    ];
    // The keys give way first, then the word `home`, when the width is short.
    if !body.keys.is_empty() && cells_of(&head) + 12 + body.keys.chars().count() + 8 <= width {
        tail.push(Span::styled(t.coach_text("  ·  "), border));
        tail.push(Span::styled(body.keys.to_string(), t.dim()));
    }
    tail.push(Span::styled(t.coach_text(" ───"), border));
    if cells_of(&head) + cells_of(&tail) > width {
        tail = vec![
            Span::styled(" ", border),
            Span::styled("0", key),
            Span::styled(t.coach_text(" ───"), border),
        ];
    }
    let n = width.saturating_sub(cells_of(&head) + cells_of(&tail));
    let mut spans = head;
    spans.push(Span::styled(t.coach_text(&"─".repeat(n)), border));
    spans.extend(tail);
    Line::from(spans)
}

/// The rows for `width` × `height` (the footer excluded): the header, the
/// cells, the act line, the rule, then as many body rows as fit.
pub fn compose<'a>(
    t: &Theme,
    d: &Dashboard,
    width: u16,
    height: u16,
    open: usize,
) -> Vec<Line<'a>> {
    let w = width as usize;
    let h = height as usize;
    let open = open.min(d.bodies.len().saturating_sub(1));
    let mut out: Vec<Line> = vec![header(t, d, w)];
    out.extend(cell_rows(t, d, width, open));
    out.extend(act_rows(t, d, width, open));
    if let Some(body) = d.bodies.get(open) {
        out.push(rule_line(t, body, w, body.id == "events"));
        for row in &body.rows {
            if out.len() >= h {
                break;
            }
            let mut spans = spans(t, row, w);
            if spans.is_empty() {
                spans.push(Span::raw(""));
            }
            out.push(Line::from(spans));
        }
    }
    out.truncate(h);
    out
}

/// Draw the dashboard into `area` (the footer excluded).
pub fn render(frame: &mut Frame, area: Rect, d: &Dashboard, t: &Theme, open: usize) {
    let lines = compose(t, d, area.width, area.height, open);
    frame.render_widget(Paragraph::new(lines), area);
}
