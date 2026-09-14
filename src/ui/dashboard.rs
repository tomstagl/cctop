//! The dashboard (plan B, "Big figures"): four levels of type on one
//! screen without frames — the header line, four tiles of block digits
//! (the coach's lights), the nudge line, and the nine borderless ledger
//! rows whose digit opens the panel full-screen. Drawn from
//! [`crate::dashboard::Dashboard`]; rows give way in a fixed order when
//! the terminal is short (blank separators, then the detail rows, then
//! the nudge's action half, then the tiles collapse to the coach's L1
//! line) and the four tiles and nine value rows are never dropped.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::coach::Level;
use crate::dashboard::{Dashboard, Seg, Tone};
use crate::theme::Theme;
use crate::ui::widgets::big_digits;

pub const FOOTER: &str = " ?help  1-9 open a panel full-screen  c coach  a ask  t theme  q";
/// Four tiles in a row (30 cells each) from this width; two per row from
/// `TWO_TILES`.
pub const FOUR_TILES: u16 = 120;
pub const TWO_TILES: u16 = 60;
/// The coach's L1 line stands in for the tiles from here; L2 below it.
pub const L1_TILES: u16 = 40;
/// Cells a tile takes in the four-in-a-row form.
pub const TILE_WIDTH: usize = 30;
/// The ledger's left column: the digit and the name.
const LEDGER_GUTTER: usize = 13;

fn tone_style(t: &Theme, tone: Tone) -> Style {
    match tone {
        Tone::Fg => Style::default().fg(t.fg),
        Tone::Dim => t.dim(),
        Tone::Accent => t.accent(),
        Tone::Ok => t.ok(),
        Tone::Warn => t.warn(),
        Tone::Crit => t.crit(),
        Tone::Bold => Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
    }
}

fn level_style(t: &Theme, level: Level) -> Style {
    match level {
        Level::Quiet => Style::default().fg(t.fg),
        Level::Watch => t.warn(),
        Level::Act => t.crit(),
    }
}

fn spans<'a>(t: &Theme, segs: &[Seg], width: usize) -> Vec<Span<'a>> {
    let cut = crate::dashboard::cut(segs.to_vec(), width);
    cut.into_iter()
        .map(|s| Span::styled(t.coach_text(&s.text), tone_style(t, s.tone)))
        .collect()
}

/// The header line: `cctop` bold, the session facts, the phase cell
/// right-aligned.
fn header<'a>(t: &Theme, d: &Dashboard, width: usize) -> Line<'a> {
    let facts = d.header.line.trim_start_matches("cctop  ").to_string();
    let mut phase = format!("{} {}", d.header.phase.glyph, d.header.phase.word);
    if !d.header.phase.tokens.is_empty() {
        phase.push_str(" · ");
        phase.push_str(&d.header.phase.tokens.join(" · "));
    }
    // The phase cell keeps its word; the facts give way first.
    let phase = crate::ui::fmt::clip(&t.coach_text(&phase), width.saturating_sub(10).min(48));
    let facts_room = width.saturating_sub(8 + phase.chars().count() + 2);
    let facts = crate::ui::fmt::clip(&t.coach_text(&facts), facts_room);
    let left_w = 8 + facts.chars().count();
    let pad = width.saturating_sub(left_w + phase.chars().count());
    let phase_style = if d.header.phase.glyph == '◆' {
        t.warn()
    } else if d.header.phase.glyph == '○' {
        t.dim()
    } else {
        t.accent()
    };
    Line::from(vec![
        Span::styled(" cctop  ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(facts),
        Span::raw(" ".repeat(pad)),
        Span::styled(phase, phase_style),
    ])
}

/// One tile's three rows at `cells` wide: the block digits in the level
/// colour, the unit dim on the baseline, the glyph + name bold beside
/// them, then the two sub-lines.
fn tile_rows<'a>(t: &Theme, tile: &crate::dashboard::Tile, cells: usize) -> [Line<'a>; 3] {
    let digits = big_digits(t, &tile.figure);
    let dw = digits.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let style = level_style(t, tile.level);
    let text_x = dw + 2 + tile.unit.chars().count().max(1) + 1;
    let text_w = cells.saturating_sub(text_x + 1);
    let row = |i: usize, text: String, text_style: Style| -> Line<'a> {
        let mut v = vec![Span::styled(format!(" {:<w$}", digits[i], w = dw), style)];
        // The unit sits on the baseline row, one cell after the digits.
        let unit = if i == 2 { tile.unit } else { "" };
        v.push(Span::styled(
            format!(" {:<w$} ", unit, w = tile.unit.chars().count().max(1)),
            t.dim(),
        ));
        v.push(Span::styled(
            format!(
                "{:<w$}",
                crate::ui::fmt::clip(&t.coach_text(&text), text_w),
                w = text_w
            ),
            text_style,
        ));
        Line::from(v)
    };
    [
        row(
            0,
            format!("{} {}", tile.glyph, tile.name),
            style.add_modifier(Modifier::BOLD),
        ),
        row(1, tile.sub1.clone(), Style::default().fg(t.fg)),
        row(2, tile.sub2.clone(), Style::default().fg(t.fg)),
    ]
}

/// Tiles side by side: `per_row` of them, `cells` each.
fn tile_block<'a>(t: &Theme, d: &Dashboard, per_row: usize, cells: usize) -> Vec<Line<'a>> {
    let mut out: Vec<Line> = Vec::new();
    for chunk in d.tiles.chunks(per_row) {
        let rows: Vec<[Line; 3]> = chunk.iter().map(|tile| tile_rows(t, tile, cells)).collect();
        for i in 0..3 {
            let mut spans = Vec::new();
            for r in &rows {
                spans.extend(r[i].spans.iter().cloned());
            }
            out.push(Line::from(spans));
        }
        if d.tiles.len() > per_row {
            out.push(Line::from(""));
        }
    }
    if out.last().is_some_and(|l| l.spans.is_empty()) {
        out.pop();
    }
    out
}

fn nudge_line<'a>(t: &Theme, d: &Dashboard, width: usize, action: bool) -> Line<'a> {
    let Some(n) = &d.nudge else {
        return Line::from(Span::styled(" quiet · nothing to act on", t.dim()));
    };
    let text = if action {
        n.line.clone()
    } else {
        n.line.split(" — ").next().unwrap_or(&n.line).to_string()
    };
    let tag = format!("{}{}", n.tag, if n.acting { " · acting…" } else { "" });
    let room = width.saturating_sub(tag.chars().count() + 4);
    let text = crate::ui::fmt::clip(&t.coach_text(&text), room);
    let pad = width.saturating_sub(3 + text.chars().count() + tag.chars().count());
    let bold = Style::default().fg(t.fg).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(t.coach_text(" ▸ "), t.warn()),
        Span::styled(text, bold),
        Span::raw(" ".repeat(pad)),
        Span::styled(tag, t.dim()),
    ])
}

fn ledger_row<'a>(t: &Theme, r: &crate::dashboard::Row, width: usize, detail: bool) -> Line<'a> {
    let mut v = vec![
        Span::styled(
            format!(" {}", r.digit),
            t.accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {:<10}", r.name), t.dim()),
    ];
    v.extend(spans(t, &r.values, width.saturating_sub(LEDGER_GUTTER)));
    if detail {
        // Names and the first value only, below 40 columns.
        let text: String = v.iter().map(|s| s.content.to_string()).collect();
        let first = text.split(" · ").next().unwrap_or(&text).to_string();
        return Line::from(Span::raw(crate::ui::fmt::clip(&first, width)));
    }
    Line::from(v)
}

fn detail_row<'a>(t: &Theme, r: &crate::dashboard::Row, width: usize) -> Line<'a> {
    let mut v = vec![Span::raw(" ".repeat(LEDGER_GUTTER))];
    let mut segs = r.detail.clone();
    for s in &mut segs {
        if s.tone == Tone::Fg {
            s.tone = Tone::Dim;
        }
    }
    v.extend(spans(t, &segs, width.saturating_sub(LEDGER_GUTTER)));
    Line::from(v)
}

/// The body lines for `width` × `height` (the footer excluded).
pub fn compose<'a>(t: &Theme, d: &Dashboard, width: u16, height: u16) -> Vec<Line<'a>> {
    let w = width as usize;
    let h = height as usize;
    let mut tiles: Vec<Line> = if width >= FOUR_TILES {
        tile_block(t, d, 4, TILE_WIDTH.min(w / 4))
    } else if width >= TWO_TILES {
        tile_block(t, d, 2, w / 2)
    } else if width >= L1_TILES {
        vec![Line::from(Span::raw(
            t.coach_text(&format!(" {}", d.lines.l1)),
        ))]
    } else {
        vec![Line::from(Span::raw(
            t.coach_text(&format!(" {}", d.lines.l2)),
        ))]
    };
    let values: Vec<Line> = d
        .rows
        .iter()
        .map(|r| ledger_row(t, r, w, width < L1_TILES))
        .collect();
    let details: Vec<Line> = d.rows.iter().map(|r| detail_row(t, r, w)).collect();
    // What the height allows, in the order things give way.
    let fixed = 1 + tiles.len() + 1 + values.len();
    let mut blanks = 3;
    let mut with_detail = width >= TWO_TILES;
    let mut action = true;
    let need = |blanks: usize, with_detail: bool, tiles_len: usize| {
        1 + blanks + tiles_len + 1 + values.len() + if with_detail { details.len() } else { 0 }
    };
    if need(blanks, with_detail, tiles.len()) > h {
        blanks = 0;
    }
    if need(blanks, with_detail, tiles.len()) > h {
        with_detail = false;
    }
    if need(blanks, with_detail, tiles.len()) > h {
        action = false;
    }
    if need(blanks, with_detail, tiles.len()) > h && tiles.len() > 1 {
        tiles = vec![Line::from(Span::raw(
            t.coach_text(&format!(" {}", d.lines.l1)),
        ))];
    }
    let _ = fixed;
    let mut out: Vec<Line> = vec![header(t, d, w)];
    if blanks > 0 {
        out.push(Line::from(""));
    }
    out.extend(tiles);
    if blanks > 0 {
        out.push(Line::from(""));
    }
    out.push(nudge_line(t, d, w, action));
    if blanks > 0 {
        out.push(Line::from(""));
    }
    for (i, v) in values.into_iter().enumerate() {
        out.push(v);
        if with_detail {
            out.push(details[i].clone());
        }
    }
    out.truncate(h);
    out
}

/// Draw the dashboard into `area` (the footer excluded).
pub fn render(frame: &mut Frame, area: Rect, d: &Dashboard, t: &Theme) {
    let lines = compose(t, d, area.width, area.height);
    frame.render_widget(Paragraph::new(lines), area);
}
