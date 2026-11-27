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

/// The keys every body shares, after its own, when the rule line is
/// expanded (`?`). Console has no footer: this is where the keys live.
pub const GLOBAL_KEYS: &str =
    "1-6 a 0 body  ·  Esc home  ·  c coach  ·  A ask  ·  t theme  ·  p pause  ·  L sessions  ·  q quit";
/// Three cells per row from this many columns; two below.
pub const THREE_CELLS: u16 = 80;
/// A cell's middle form from this many cells wide; the short one below.
pub const MID_CELL: usize = 28;
/// The act line's right-aligned tail (`a:advisor`) from this width.
pub const ACT_TAIL: u16 = 66;
/// The act line's full copy from this width; the short one below.
pub const ACT_FULL: u16 = 72;

pub(super) fn tone_style(t: &Theme, tone: Tone) -> Style {
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

/// The facts keep at least this many cells for the hooks badge to show.
pub const MIN_FACTS_WITH_BADGE: usize = 12;

/// The text between the facts and the phase cell: the hooks badge and the
/// data rules, two cells apart (the pane's `headerLine` in `views/overview.tsx` mirrors it with
/// an inline join).
pub fn header_badge(hooks: Option<&str>, rules: Option<&str>) -> String {
    [hooks, rules]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ")
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
    // The badges (hooks, then the data rules) sit between the facts and the
    // phase cell.
    let mut badge = header_badge(d.header.hooks.as_deref(), d.header.rules.as_deref());
    let room_for = |badge: &str| {
        let badge_room = if badge.is_empty() {
            0
        } else {
            badge.chars().count() + 2
        };
        (
            badge_room,
            width as i64 - (8 + phase.chars().count() + 2 + badge_room) as i64,
        )
    };
    // Too narrow to leave the facts MIN_FACTS_WITH_BADGE cells: the badge
    // goes first, the phase cell stays.
    if room_for(&badge).1 < MIN_FACTS_WITH_BADGE as i64 {
        badge.clear();
    }
    let (badge_room, facts_room) = room_for(&badge);
    let facts_room = facts_room.max(0) as usize;
    let facts = dashboard::fit_workspace(&facts, &d.header.workspace, facts_room);
    let facts = crate::ui::fmt::clip(&t.coach_text(&facts), facts_room);
    let pad = width.saturating_sub(8 + facts.chars().count() + phase.chars().count() + badge_room);
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
        Span::styled(
            if badge.is_empty() {
                badge
            } else {
                format!("{badge}  ")
            },
            t.warn(),
        ),
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
pub(super) fn wrap<'a>(t: &Theme, segs: &[Seg], width: usize, indent: usize) -> Vec<Line<'a>> {
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

/// Row 6: `─── title ─────── 0: home  ·  ? keys ───`; with `keys` (the
/// `?` toggle) the tail is the body's own keys and then the global ones,
/// cut at the width. Short widths lose `? keys`, then the word `home`.
fn rule_line<'a>(
    t: &Theme,
    body: &dashboard::Body,
    width: usize,
    home: bool,
    keys: bool,
) -> Line<'a> {
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
    let sep = || Span::styled(t.coach_text("  ·  "), border);
    let mut tail = vec![
        Span::styled(" ", border),
        Span::styled("0: ", key),
        Span::styled("home", t.dim()),
    ];
    if keys {
        let map = if body.keys.is_empty() {
            GLOBAL_KEYS.to_string()
        } else {
            format!("{}  ·  {GLOBAL_KEYS}", body.keys)
        };
        let room = width.saturating_sub(cells_of(&head) + cells_of(&tail) + 5 + 4);
        tail.push(sep());
        tail.push(Span::styled(
            crate::ui::fmt::clip(&t.coach_text(&map), room),
            t.dim(),
        ));
    } else if cells_of(&head) + cells_of(&tail) + 5 + 6 + 4 <= width {
        tail.push(sep());
        tail.push(Span::styled("?", t.accent().add_modifier(Modifier::BOLD)));
        tail.push(Span::styled(" keys", t.dim()));
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
    keys: bool,
) -> Vec<Line<'a>> {
    compose_scrolled(t, d, width, height, open, keys).0
}

/// The overview's rows from `scroll`, in `room` rows: a dim `↑ N more`
/// when rows are scrolled off the top and `↓ N more` when the bottom is
/// cut. Also the furthest `scroll` that still fills the room.
fn scrolled<'a>(
    t: &Theme,
    all: Vec<Line<'a>>,
    scroll: usize,
    room: usize,
) -> (Vec<Line<'a>>, usize) {
    let total = all.len();
    if total <= room || room < 3 {
        return (all.into_iter().take(room).collect(), 0);
    }
    let max = total - (room - 1);
    let off = scroll.min(max);
    let up = usize::from(off > 0);
    let mut cap = room - up;
    let down = off + cap < total;
    if down {
        cap -= 1;
    }
    let (up_ch, down_ch) = if t.ascii { ('^', 'v') } else { ('↑', '↓') };
    let hint =
        |arrow: char, n: usize| Line::from(Span::styled(format!(" {arrow} {n} more"), t.dim()));
    let mut out = Vec::new();
    if up == 1 {
        out.push(hint(up_ch, off));
    }
    out.extend(all.into_iter().skip(off).take(cap));
    if down {
        out.push(hint(down_ch, total - (off + cap)));
    }
    (out, max)
}

/// [`compose`], and how far the body can scroll (0 when it fits).
pub fn compose_scrolled<'a>(
    t: &Theme,
    d: &Dashboard,
    width: u16,
    height: u16,
    open: usize,
    keys: bool,
) -> (Vec<Line<'a>>, usize) {
    let w = width as usize;
    let h = height as usize;
    let open = open.min(d.bodies.len().saturating_sub(1));
    let mut out: Vec<Line> = vec![header(t, d, w)];
    // The workflow strip: above the cells, so on screen whichever body is
    // open; the body gets one row less.
    if let Some(row) = &d.header.workflow {
        out.push(Line::from(spans(t, row, w)));
    }
    out.extend(cell_rows(t, d, width, open));
    out.extend(act_rows(t, d, width, open));
    if let Some(body) = d.bodies.get(open) {
        out.push(rule_line(t, body, w, body.id == "events", keys));
        // Body 1 is the overview: the charts, drawn from the overview object.
        if let (Some(o), "context") = (&d.overview, body.id) {
            let room = h.saturating_sub(out.len());
            let (rows, max) = scrolled(
                t,
                crate::ui::overview_view::rows(t, o, width),
                d.scroll,
                room,
            );
            out.extend(rows);
            out.truncate(h);
            return (out, max);
        }
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
    (out, 0)
}

/// Draw Console into `area`: the whole of it, there is no footer.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    d: &Dashboard,
    t: &Theme,
    open: usize,
    keys: bool,
    max_scroll: &std::cell::Cell<usize>,
) {
    let (lines, max) = compose_scrolled(t, d, area.width, area.height, open, keys);
    max_scroll.set(max);
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::status::{Repo, Workspace};
    use crate::ui::state::{SessionInfo, State, StatusFacts};
    use std::path::Path;

    fn all_facts() -> StatusFacts {
        StatusFacts {
            workspace: Some(Workspace {
                added_dirs: vec!["/x/y".into()],
                git_worktree: Some("feat".into()),
                repo: Some(Repo {
                    host: Some("github.com".into()),
                    owner: Some("acme".into()),
                    name: Some("shop".into()),
                }),
                ..Default::default()
            }),
            output_style: Some("Concise".into()),
            agent_name: Some("reviewer".into()),
            remote_session_id: Some("r-1".into()),
            ..Default::default()
        }
    }

    fn dashboard_with(facts: StatusFacts) -> (App, Dashboard) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = crate::theme::Caps::full();
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        app.set_theme("default-dark");
        app.state.status_facts = facts;
        let d = app.dashboard();
        (app, d)
    }

    fn header_row(app: &App, d: &Dashboard, width: u16) -> String {
        let rows = compose(&app.state.theme, d, width, 24, 0, false);
        rows[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn tui_header_drops_from_right() {
        let (app, d) = dashboard_with(all_facts());
        assert_eq!(
            d.header.workspace,
            [
                "acme/shop",
                "wt feat",
                "+1 dir",
                "style Concise",
                "agent: reviewer",
                "remote"
            ]
        );
        let phase = format!(
            "{} {} {}",
            d.header.phase.glyph, d.header.phase.word, d.header.phase.elapsed
        );
        let mut last = usize::MAX;
        for w in [200u16, 140, 100, 80, 70, 60] {
            let row = header_row(&app, &d, w);
            assert!(row.chars().count() <= w as usize, "{w}: {row}");
            assert!(row.contains(&phase), "{w}: phase cut: {row}");
            let n = d
                .header
                .workspace
                .iter()
                .filter(|i| row.contains(i.as_str()))
                .count();
            assert!(n <= last, "{w}: items came back: {row}");
            last = n;
            // Dropped from the right: what shows is always a prefix.
            let shown = d
                .header
                .workspace
                .iter()
                .take_while(|i| row.contains(i.as_str()))
                .count();
            assert_eq!(n, shown, "{w}: not a prefix: {row}");
        }
        // At 60 columns `remote` is gone while the repo may remain.
        let narrow = header_row(&app, &d, 60);
        assert!(!narrow.contains("remote"), "{narrow}");
        let wide = header_row(&app, &d, 200);
        assert!(
            wide.contains("remote") && wide.contains("acme/shop"),
            "{wide}"
        );
        // A width that fits the repo but not `remote`.
        let mut seen = false;
        for w in 60..200u16 {
            let r = header_row(&app, &d, w);
            if r.contains("acme/shop") && !r.contains("remote") {
                seen = true;
            }
            assert!(!r.contains("remote") || r.contains("acme/shop"), "{w}: {r}");
        }
        assert!(seen);
    }

    #[test]
    fn header_hooks_badge_is_amber_when_events_are_missing() {
        let (mut app, d) = dashboard_with(all_facts());
        assert!(d.header.hooks.is_none());
        let plain = header_row(&app, &d, 120);
        assert!(!plain.contains("hooks:"), "{plain}");
        app.state.hooks_missing = crate::hooks::EVENTS[22..].to_vec();
        let d = app.dashboard();
        assert_eq!(d.header.hooks.as_deref(), Some("hooks: 7 new"));
        let rows = compose(&app.state.theme, &d, 120, 24, 0, false);
        let badge = rows[0]
            .spans
            .iter()
            .find(|s| s.content.contains("hooks: 7 new"))
            .expect("the badge span");
        assert_eq!(badge.style, app.state.theme.warn());
        let row = header_row(&app, &d, 120);
        assert_eq!(row.chars().count(), 120, "{row}");
        assert!(row.contains("hooks: 7 new  ● "), "{row}");
        insta::assert_snapshot!("header_hooks_missing_120", row);
        // Too narrow for the facts to keep a dozen cells: the badge goes
        // before the phase cell does.
        let narrow = header_row(&app, &d, 40);
        assert!(!narrow.contains("hooks:"), "{narrow}");
        assert!(narrow.contains(&d.header.phase.word), "{narrow}");
        insta::assert_snapshot!("header_hooks_missing_40", narrow);
    }

    #[test]
    fn header_shows_the_data_rules_beside_the_hooks_badge() {
        let (mut app, d) = dashboard_with(all_facts());
        assert!(d.header.rules.is_none());
        app.state.pane_facts = Some(crate::pane::PaneFacts {
            data_rules: Some(vec!["zdr".into(), "hipaa".into(), "acme-tier".into()]),
            ..Default::default()
        });
        let d = app.dashboard();
        assert_eq!(d.header.rules.as_deref(), Some("ZDR HIPAA rules"));
        let row = header_row(&app, &d, 120);
        assert!(row.contains("ZDR HIPAA rules  ● "), "{row}");
        insta::assert_snapshot!("header_rules_120", row);
        app.state.hooks_missing = crate::hooks::EVENTS[22..].to_vec();
        let d = app.dashboard();
        let row = header_row(&app, &d, 120);
        assert!(row.contains("hooks: 7 new  ZDR HIPAA rules  ● "), "{row}");
        insta::assert_snapshot!("header_rules_hooks_120", row);
        // Too narrow: the badges go before the phase cell does.
        let narrow = header_row(&app, &d, 40);
        assert!(!narrow.contains("ZDR"), "{narrow}");
        assert!(narrow.contains(&d.header.phase.word), "{narrow}");
        insta::assert_snapshot!("header_rules_hooks_40", narrow);
    }

    /// Body 1 as the pane still draws it from the dashboard object (its
    /// rows, no charts): the pane's row-identity test reads this snapshot
    /// until the pane draws the overview (mod-pane Task 7).
    #[test]
    fn body_one_rows_without_the_overview() {
        let (app, mut d) = dashboard_with(all_facts());
        d.overview = None;
        let text = compose(&app.state.theme, &d, 85, 24, 0, false)
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
            .join("\n");
        insta::assert_snapshot!("console_context_rows_85x24", text);
    }

    #[test]
    fn header_snapshots_with_all_items() {
        let (app, d) = dashboard_with(all_facts());
        for w in [80u16, 120, 200] {
            insta::assert_snapshot!(format!("header_workspace_{w}"), header_row(&app, &d, w));
        }
    }
}
