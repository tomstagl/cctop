//! The coach view ("Lights", coach PRD §6.2): the 56-column card drawn
//! from [`crate::coach::Coach`] — the state line, four light rows, the
//! nudge slot (or a light's detail, or the quiet row with the last
//! lifecycle rows), the `next` and `snoozed` rows. At ≥ 100 columns the
//! why / lifecycle column sits beside the card; at ≤ 40 columns the 2 × 2
//! form; when rows are scarce the blank row goes first, then `snoozed`,
//! then `next`, and the slot never drops below two lines. The nudge's
//! full words (name, body, fix prompt) are wrapped into the rows left
//! below the slot, or into the why column when it sits beside the card.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::coach::{Coach, Full, Level, WIDTH};
use crate::theme::Theme;
use crate::ui::state::{CoachUi, State};

/// The card's outer width: borders and one cell of padding each side.
pub const CARD: u16 = WIDTH as u16 + 4;
/// Below this width the 2 × 2 form is drawn.
pub const NARROW: u16 = 40;
/// From this width the why column sits beside the card.
pub const WIDE: u16 = 100;

pub const FOOTER: &str = " c dashboard  Enter act  x snooze  e why  1-4 light  q";

fn level_style(t: &Theme, level: Level) -> Style {
    match level {
        Level::Quiet => Style::default().fg(t.fg),
        Level::Watch => t.warn(),
        Level::Act => t.crit(),
    }
}

/// One card row: `│ text…padded │`.
fn row<'a>(t: &Theme, inner: usize, spans: Vec<Span<'a>>) -> Line<'a> {
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = inner.saturating_sub(used);
    let mut v = vec![Span::styled(t.coach_text("│ "), t.dim())];
    v.extend(spans);
    v.push(Span::raw(" ".repeat(pad)));
    v.push(Span::styled(t.coach_text(" │"), t.dim()));
    Line::from(v)
}

/// `s` broken at spaces into lines of at most `width` cells, the first
/// led by `first`, the rest by `rest`; a word longer than a line stands
/// alone (the row clips it).
fn wrap(s: &str, width: usize, first: &str, rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in s.split_whitespace() {
        let prefix = if out.is_empty() { first } else { rest };
        if !cur.is_empty()
            && prefix.chars().count() + cur.chars().count() + 1 + w.chars().count() > width
        {
            out.push(format!("{prefix}{cur}"));
            cur.clear();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        let prefix = if out.is_empty() { first } else { rest };
        out.push(format!("{prefix}{cur}"));
    }
    out
}

/// The lead of the fix prompt's first row, and of its other rows.
const FIX_LEAD: (&str, &str) = ("fix ↵ ", "      ");

/// The nudge's full words in the card, for `rows` rows: the name, the
/// body and the fix prompt, wrapped at the card's width, after a blank row
/// when all of it fits; else cut with a pointer to `e`. Nothing under
/// three rows.
fn advice_rows<'a>(t: &Theme, inner: usize, full: &Full, rows: usize) -> Vec<Line<'a>> {
    if rows < 3 {
        return Vec::new();
    }
    let mut v: Vec<(String, Style)> = vec![(
        full.name.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )];
    for s in wrap(&full.body, inner, "", "") {
        v.push((s, Style::default()));
    }
    if let Some(f) = &full.fix_prompt {
        for s in wrap(f, inner, FIX_LEAD.0, FIX_LEAD.1) {
            v.push((s, t.accent()));
        }
    }
    match v.len().cmp(&rows) {
        std::cmp::Ordering::Greater => {
            v.truncate(rows - 1);
            v.push(("… e shows the whole advice".into(), t.dim()));
        }
        std::cmp::Ordering::Less => v.insert(0, (String::new(), Style::default())),
        std::cmp::Ordering::Equal => {}
    }
    v.into_iter()
        .map(|(s, style)| {
            if s.is_empty() {
                row(t, inner, vec![])
            } else {
                plain(t, inner, &s, style)
            }
        })
        .collect()
}

/// The queued nudge `n` peeks at, when it does.
fn peeked<'s>(ui: &CoachUi, state: &'s State) -> Option<(usize, &'s crate::advisor::Advice)> {
    let i = ui.peek.min(state.advice.len().saturating_sub(1));
    (ui.peek > 0 && i > 0)
        .then(|| state.advice.get(i).map(|a| (i, a)))
        .flatten()
}

fn plain<'a>(t: &Theme, inner: usize, text: &str, style: Style) -> Line<'a> {
    let text = crate::ui::fmt::clip(&t.coach_text(text), inner);
    row(t, inner, vec![Span::styled(text, style)])
}

fn separator<'a>(t: &Theme, inner: usize) -> Line<'a> {
    Line::from(Span::styled(
        t.coach_text(&format!("├{}┤", "─".repeat(inner + 2))),
        t.dim(),
    ))
}

fn title<'a>(t: &Theme, inner: usize, c: &Coach) -> Line<'a> {
    let left = format!("coach ─ {} ", c.session);
    let right = format!(
        " {} · turn {} ",
        crate::ui::fmt::model_short(&c.model),
        c.turn
    );
    let fill = (inner + 2).saturating_sub(left.chars().count() + right.chars().count());
    Line::from(vec![
        Span::styled(t.coach_text("╭"), t.dim()),
        Span::styled(
            t.coach_text(&left),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(t.coach_text(&"─".repeat(fill)), t.dim()),
        Span::styled(t.coach_text(&right), t.dim()),
        Span::styled(t.coach_text("╮"), t.dim()),
    ])
}

fn bottom<'a>(t: &Theme, inner: usize) -> Line<'a> {
    Line::from(Span::styled(
        t.coach_text(&format!("╰{}╯", "─".repeat(inner + 2))),
        t.dim(),
    ))
}

/// The state line's style: WAITING amber, LOOP dim, else the accent on the
/// phase word.
fn state_line<'a>(t: &Theme, inner: usize, c: &Coach) -> Line<'a> {
    let text = crate::ui::fmt::clip(&t.coach_text(&c.state.line), inner);
    let (word, rest) = match text.split_once(" ") {
        Some((w, r)) if c.state.kind.starts_with('◆') => {
            // `◆ WAITING …`: the glyph and the word together.
            let (w2, r2) = r.split_once(' ').unwrap_or((r, ""));
            (format!("{w} {w2}"), r2.to_string())
        }
        Some((w, r)) => (w.to_string(), r.to_string()),
        None => (text.clone(), String::new()),
    };
    let style = if c.state.kind.starts_with('◆') {
        t.warn()
    } else if c.state.kind == "LOOP" || c.state.kind == "IDLE" {
        t.dim()
    } else {
        t.accent()
    };
    let mut spans = vec![Span::styled(word, style)];
    if !rest.is_empty() {
        spans.push(Span::raw(format!(" {rest}")));
    }
    row(t, inner, spans)
}

/// The slot area: the peeked nudge (three rows), a light's detail, or the
/// quiet row with the last lifecycle rows. Returns at least two rows.
fn slot_rows<'a>(t: &Theme, inner: usize, c: &Coach, ui: &CoachUi, state: &State) -> Vec<Line<'a>> {
    if let Some(i) = ui.light.filter(|i| (1..=4).contains(i)) {
        let l = &c.lights[i - 1];
        let mut v = vec![row(
            t,
            inner,
            vec![
                Span::styled(t.coach_text(&l.glyph.to_string()), level_style(t, l.level)),
                Span::styled(
                    format!(" {} ", l.id),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("· {}{}", l.source, if l.approx { " ≈" } else { "" }),
                    t.dim(),
                ),
            ],
        )];
        for line in l.lines.iter().take(3) {
            v.push(plain(t, inner, &format!("  {line}"), Style::default()));
        }
        while v.len() < 3 {
            v.push(row(t, inner, vec![]));
        }
        return v;
    }
    // `n` peeks down the queue without promoting.
    if let Some((i, a)) = peeked(ui, state) {
        let evidence = format!("  {} · queued #{i} · n next · Esc back", a.urgency.label());
        return vec![
            plain(
                t,
                inner,
                &format!("▸ {}", a.headline),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            plain(t, inner, &format!("  {}", a.action), t.accent()),
            plain(t, inner, &evidence, t.dim()),
        ];
    }
    match &c.nudge {
        Some(n) => vec![
            plain(
                t,
                inner,
                &n.line1,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            plain(t, inner, &n.line2, t.accent()),
            plain(t, inner, &n.line3, t.dim()),
        ],
        None => {
            let mut v = vec![plain(t, inner, &c.quiet_row(), t.dim())];
            for r in c.recent.iter().take(2) {
                v.push(plain(t, inner, &format!("  {}", r.row), t.dim()));
            }
            while v.len() < 3 {
                v.push(row(t, inner, vec![]));
            }
            v
        }
    }
}

/// The 56-column card (or narrower, cut) for `area.height` rows.
pub fn card_lines<'a>(
    t: &Theme,
    width: u16,
    height: u16,
    c: &Coach,
    ui: &CoachUi,
    state: &State,
) -> Vec<Line<'a>> {
    let inner = (width.min(CARD) as usize).saturating_sub(4).max(8);
    let mut top = vec![
        title(t, inner, c),
        state_line(t, inner, c),
        separator(t, inner),
    ];
    for l in &c.lights {
        let text = match (&l.alt, ui.limit_units) {
            (Some(alt), true) => alt.clone(),
            _ => l.text.clone(),
        };
        let text = crate::ui::fmt::clip(
            &t.coach_text(&format!("{:<8} {text}", l.id)),
            inner.saturating_sub(2),
        );
        top.push(row(
            t,
            inner,
            vec![
                Span::styled(t.coach_text(&l.glyph.to_string()), level_style(t, l.level)),
                Span::raw(" "),
                Span::styled(text, level_style(t, l.level)),
            ],
        ));
    }
    top.push(separator(t, inner));
    let mut slot = slot_rows(t, inner, c, ui, state);
    let blank = row(t, inner, vec![]);
    let next = plain(t, inner, &c.next_row(), t.dim());
    let snoozed = plain(t, inner, &c.snoozed_row(), t.dim());
    let bottom = bottom(t, inner);
    // What fits: the slot never drops below two lines; the blank row goes
    // first, then `snoozed`, then `next`.
    let fixed = top.len() + 1; // + bottom
    let avail = (height as usize).saturating_sub(fixed);
    let mut tail: Vec<Line> = Vec::new();
    let mut slot_len = slot.len();
    if avail >= slot_len + 3 {
        tail.extend([blank, next, snoozed]);
    } else if avail >= slot_len + 2 {
        tail.extend([next, snoozed]);
    } else if avail > slot_len {
        tail.push(next);
    } else {
        slot_len = avail.max(2).min(slot.len());
        slot.truncate(slot_len);
    }
    // The full words fill the rows left between the slot and the tail,
    // unless the why column beside the card shows them.
    let shown = match (ui.light, peeked(ui, state)) {
        (Some(_), _) => None,
        (None, Some((_, a))) => Some(Full::of(a)),
        (None, None) => c.nudge.as_ref().map(|n| n.full.clone()),
    };
    let spare = avail.saturating_sub(slot.len() + tail.len());
    let advice = match shown {
        Some(full) if width < WIDE => advice_rows(t, inner, &full, spare),
        _ => Vec::new(),
    };
    let mut lines = top;
    lines.extend(slot);
    lines.extend(advice);
    lines.extend(tail);
    lines.push(bottom);
    lines
}

/// The 2 × 2 form at ≤ 40 columns: the state line, two rows of glyph +
/// number pairs, the nudge wrapped.
pub fn narrow_lines<'a>(t: &Theme, width: u16, c: &Coach) -> Vec<Line<'a>> {
    let inner = (width as usize).saturating_sub(4).max(8);
    let l = &c.lights;
    let half = inner / 2;
    let pair = |a: String, b: String| format!("{:<w$}{}", a, b, w = half);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(t.coach_text("╭"), t.dim()),
            Span::styled(
                t.coach_text(&crate::ui::fmt::clip(
                    &format!(
                        "coach ── {} · turn {} ",
                        crate::ui::fmt::model_short(&c.model),
                        c.turn
                    ),
                    inner + 1,
                )),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                t.coach_text(
                    &"─".repeat(
                        (inner + 2).saturating_sub(
                            format!(
                                "coach ── {} · turn {} ",
                                crate::ui::fmt::model_short(&c.model),
                                c.turn
                            )
                            .chars()
                            .count()
                            .min(inner + 1),
                        ),
                    ),
                ),
                t.dim(),
            ),
            Span::styled(t.coach_text("╮"), t.dim()),
        ]),
        state_line(t, inner, c),
        plain(
            t,
            inner,
            &pair(
                format!("{} ctx {} {}", l[0].glyph, l[0].number, l[0].money()),
                format!("{} cache {}", l[1].glyph, l[1].number),
            ),
            Style::default(),
        ),
        plain(
            t,
            inner,
            &pair(
                format!("{} 5h {}", l[2].glyph, l[2].number),
                format!("{} {}", l[3].glyph, l[3].short_form()),
            ),
            Style::default(),
        ),
        separator(t, inner),
    ];
    match &c.nudge {
        Some(n) => {
            for s in wrap(n.line1.trim_start_matches("▸ "), inner, "▸ ", "  ") {
                lines.push(plain(
                    t,
                    inner,
                    &s,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
            }
            for s in wrap(n.line2.trim(), inner, "  ", "  ") {
                lines.push(plain(t, inner, &s, t.accent()));
            }
        }
        None => lines.push(plain(t, inner, &c.quiet_row(), t.dim())),
    }
    lines.push(bottom(t, inner));
    lines
}

impl crate::coach::Light {
    /// `≈$.21` from the context row.
    fn money(&self) -> String {
        self.text
            .split(" · ")
            .nth(1)
            .unwrap_or("")
            .trim_end_matches("/call")
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string()
    }
    /// `fails 3 ✓no` / `edits 3 ✓no` / `edits 0 ✓6m`.
    fn short_form(&self) -> String {
        let first = self.text.split(" · ").next().unwrap_or("");
        let check = self
            .text
            .split(" · ")
            .find(|p| p.contains('✓') || p.contains('✗'))
            .and_then(|p| p.split(['✓', '✗']).nth(1))
            .map(|s| s.trim().split(' ').next().unwrap_or("").to_string())
            .unwrap_or_default();
        let head: String = first
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let head = match head.split_once(' ') {
            Some((n, w)) if n.chars().all(|c| c.is_ascii_digit()) => format!("{w} {n}"),
            _ => head,
        };
        let check = if check.is_empty() {
            String::new()
        } else {
            format!(" ✓{}", if check == "none" { "no" } else { &check })
        };
        format!("{head}{check}")
    }
}

/// The why column (≥ 100 columns) or overlay (`e`): the nudge's name and
/// full words — headline, body, evidence, fix prompt — then the
/// retirement rule, the saving, the doc text, the last retired nudges.
pub fn why_lines<'a>(t: &Theme, width: usize, c: &Coach) -> Vec<Line<'a>> {
    let dim = t.dim();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    let wrapped = |s: &str| wrap(s, width, "", "");
    match &c.nudge {
        Some(n) => {
            let f = &n.full;
            lines.push(Line::from(Span::styled(t.coach_text(f.name), bold)));
            lines.push(Line::from(Span::styled(
                t.coach_text(&format!("{} {} · {}", n.class.label(), n.id, n.family)),
                dim,
            )));
            for s in wrapped(&t.coach_text(&f.headline)) {
                lines.push(Line::from(s));
            }
            lines.push(Line::from(""));
            for s in wrapped(&t.coach_text(&f.body)) {
                lines.push(Line::from(s));
            }
            if let Some(fix) = &f.fix_prompt {
                lines.push(Line::from(""));
                for s in wrap(&t.coach_text(fix), width, FIX_LEAD.0, FIX_LEAD.1) {
                    lines.push(Line::from(Span::styled(s, t.accent())));
                }
            }
            lines.push(Line::from(""));
            for (i, e) in f.evidence.iter().enumerate() {
                lines.push(Line::from(vec![
                    Span::styled(if i == 0 { "evidence  " } else { "          " }, dim),
                    Span::raw(t.coach_text(e)),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("slot      ", dim),
                Span::raw(t.coach_text(n.line3.trim())),
            ]));
            lines.push(Line::from(vec![
                Span::styled("retires   ", dim),
                Span::raw(format!("on {} · since turn {}", n.retires_on, n.since_turn)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("saving    ", dim),
                Span::raw(format!("{} (an estimate)", n.saving)),
            ]));
            if !n.action_text.is_empty() && f.fix_prompt.as_ref() != Some(&n.action_text) {
                lines.push(Line::from(vec![
                    Span::styled(format!("{:<10}", n.action_kind.label()), dim),
                    Span::raw(t.coach_text(&n.action_text)),
                ]));
            }
            lines.push(Line::from(""));
            for s in wrapped(n.explain) {
                lines.push(Line::from(Span::styled(s, dim)));
            }
        }
        None => lines.push(Line::from(Span::styled(
            "nothing to explain — no nudge in the slot",
            dim,
        ))),
    }
    if !c.recent.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("recent", bold)));
        for r in c.recent.iter().take(3) {
            lines.push(Line::from(Span::styled(t.coach_text(&r.row), dim)));
        }
    }
    lines
}

/// The lifecycle overlay (`l`): every retired nudge, newest first.
pub fn lifecycle_lines<'a>(t: &Theme, c: &Coach) -> Vec<Line<'a>> {
    let dim = t.dim();
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} nudges this hour · {} queued · {}",
            c.nudges_this_hour,
            c.queued,
            c.session_mode.label()
        ),
        dim,
    ))];
    if c.recent.is_empty() {
        lines.push(Line::from(Span::styled("no nudge has retired yet", dim)));
    }
    for r in &c.recent {
        lines.push(Line::from(vec![
            Span::raw(t.coach_text(&crate::ui::fmt::clock_hhmm(r.at_ms))),
            Span::styled(
                format!(" {:<9}", r.what),
                if r.what == "acted" { t.ok() } else { dim },
            ),
            Span::raw(format!(
                "{} {} · {}",
                r.id,
                r.family,
                t.coach_text(&r.detail)
            )),
        ]));
    }
    if !c.suppressed.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "held back",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for (rule, why) in &c.suppressed {
            lines.push(Line::from(Span::styled(format!("{rule} · {why}"), dim)));
        }
    }
    lines
}

/// Draw the view into `area` (the footer excluded).
pub fn render(frame: &mut Frame, area: Rect, c: &Coach, state: &State) {
    let t = &state.theme;
    let ui = &state.coach_ui;
    if area.width <= NARROW {
        let lines = narrow_lines(t, area.width, c);
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    let lines = card_lines(t, area.width, area.height, c, ui, state);
    let card_w = area.width.min(CARD);
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(area.x, area.y, card_w, area.height),
    );
    if area.width >= WIDE {
        let x = area.x + card_w + 2;
        let w = area.width.saturating_sub(card_w + 2);
        let lines = why_lines(t, w as usize, c);
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            Rect::new(x, area.y, w, area.height),
        );
    }
    if ui.why || ui.lifecycle {
        let w = area.width.min(72);
        let h = area.height.min(20);
        let rect = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        frame.render_widget(Clear, rect);
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_set(t.border_set())
            .title(if ui.why {
                " why  (Esc back) "
            } else {
                " lifecycle  (Esc back) "
            });
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        let lines = if ui.why {
            why_lines(t, inner.width as usize, c)
        } else {
            lifecycle_lines(t, c)
        };
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{parse_keys, render_to_string, App};
    use crate::ui::state::{SessionInfo, State, View};
    use std::path::Path;

    /// Fixture B at a moment (`lines`), the clock `plus` seconds past the
    /// last fed line, the coach view open.
    fn app_at(lines: usize, plus: i64) -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = crate::theme::Caps::full();
        app.state.view = View::Coach;
        crate::attach::attach_headless_prefix(
            &mut app,
            &path,
            SessionInfo::from_fixture(&path),
            lines,
        );
        if plus > 0 {
            app.state.clock_override = true;
            app.state.now_ms = app.state.last_line_at_ms.unwrap() + plus * 1000;
            app.tick();
        }
        app.set_theme("default-dark");
        app
    }

    #[test]
    fn six_moments_at_56x20() {
        for (name, lines, plus) in [
            ("explore", 214, 0),
            ("edits", 300, 0),
            ("denials", 761, 0),
            ("waiting", 788, 240),
            ("cold", 789, 0),
            ("idle", 738, 240),
        ] {
            let app = app_at(lines, plus);
            insta::assert_snapshot!(
                format!("coach_{name}_56x20"),
                render_to_string(&app, 56, 20)
            );
        }
    }

    /// The text of a rendered frame's columns `from..to`, rows joined, the
    /// card's borders and the wrap's line breaks gone.
    fn flat(out: &str, from: usize, to: usize) -> String {
        out.lines()
            .map(|l| {
                let s: String = l.chars().skip(from).take(to - from).collect();
                s.trim_matches(|c: char| c == '│' || c.is_whitespace())
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn full_words_wrapped_at_56_and_122_columns() {
        let app = app_at(761, 0);
        let c = crate::coach::snapshot(&app.state, app.advisor());
        let f = &c.nudge.as_ref().unwrap().full;
        let fix = f
            .fix_prompt
            .as_deref()
            .expect("the deny rule has a fix prompt");
        // 56 columns: the card alone, the words below the slot.
        let narrow = render_to_string(&app, 56, 32);
        insta::assert_snapshot!("coach_full_56x32", narrow);
        let text = flat(&narrow, 0, 56);
        assert!(text.contains(f.name), "{text}");
        assert!(text.contains(&f.body), "{text}");
        assert!(text.contains(&format!("fix ↵ {fix}")), "{text}");
        for row in narrow.lines() {
            assert!(row.chars().count() <= 56, "{row}");
        }
        // 122 columns: the why column beside the card carries them.
        let wide = render_to_string(&app, 122, 32);
        insta::assert_snapshot!("coach_full_122x32", wide);
        let col = flat(&wide, 58, 122);
        assert!(col.contains(f.name), "{col}");
        assert!(col.contains(&f.body), "{col}");
        assert!(col.contains(&format!("fix ↵ {fix}")), "{col}");
        assert!(!flat(&wide, 0, 56).contains(f.name), "not twice");
        // Rows scarce: cut with a pointer to the why overlay.
        let short = render_to_string(&app, 56, 20);
        assert!(
            short.contains(f.name) && short.contains("… e shows the whole advice"),
            "{short}"
        );
    }

    #[test]
    fn narrow_wide_short_and_ascii_forms() {
        let app = app_at(761, 0);
        insta::assert_snapshot!("coach_denials_40x14", render_to_string(&app, 40, 14));
        insta::assert_snapshot!("coach_denials_110x18", render_to_string(&app, 110, 18));
        // Rows scarce: the blank row goes first, then snoozed, then next.
        let short = render_to_string(&app, 56, 14);
        assert!(!short.contains("snoozed  "), "{short}");
        assert!(short.contains("next     "), "{short}");
        let shorter = render_to_string(&app, 56, 12);
        assert!(!shorter.contains("next     "), "{shorter}");
        assert!(
            shorter.contains("▸ rm is denied"),
            "the slot stays: {shorter}"
        );
        let mut ascii = app_at(761, 0);
        ascii.caps = crate::theme::Caps {
            ascii: true,
            ..crate::theme::Caps::full()
        };
        ascii.set_theme("default-dark");
        let out = render_to_string(&ascii, 56, 20);
        assert!(out.contains("! rework"), "{out}");
        assert!(!out.contains('●') && !out.contains('╭'), "{out}");
    }

    /// The sync contract: the state line, the four light rows and the
    /// slot's two lines are the coach object's text, byte for byte.
    #[test]
    fn card_text_is_the_coach_object() {
        let app = app_at(788, 240);
        let c = crate::coach::snapshot(&app.state, app.advisor());
        let out = render_to_string(&app, 56, 20);
        let rows: Vec<&str> = out.lines().collect();
        let cell = |r: usize| rows[r].trim_start_matches('│').trim_end_matches('│').trim();
        assert_eq!(cell(1), c.state.line);
        for (i, l) in c.lights.iter().enumerate() {
            assert_eq!(cell(3 + i), l.row());
        }
        let n = c.nudge.as_ref().unwrap();
        assert_eq!(cell(8), n.line1);
        assert_eq!(cell(9), n.line2.trim());
        assert_eq!(cell(10), n.line3.trim());
    }

    #[test]
    fn keys_peek_detail_why_lifecycle_units_snooze_and_act() {
        let mut app = app_at(788, 240);
        let base = render_to_string(&app, 56, 20);
        assert!(base.contains("▸ 2 corrections in a row"), "{base}");
        // n peeks at the queued nudge (the question, a same-class newcomer
        // waiting in next) without promoting it.
        for k in parse_keys("n") {
            app.handle_key(k);
        }
        let out = render_to_string(&app, 56, 20);
        assert!(out.contains("WAITING 4:00"), "{out}");
        assert!(out.contains("queued #1"), "{out}");
        assert_eq!(app.state.coach_ui.peek, 1);
        // 1 opens the context light's detail in the slot area; 1 again closes it.
        for k in parse_keys("1") {
            app.handle_key(k);
        }
        let out = render_to_string(&app, 56, 20);
        assert!(out.contains("threshold 567k"), "{out}");
        assert_eq!(app.state.coach_ui.peek, 0);
        for k in parse_keys("1") {
            app.handle_key(k);
        }
        assert!(app.state.coach_ui.light.is_none());
        // $ flips the context row to rate-limit units.
        for k in parse_keys("$") {
            app.handle_key(k);
        }
        assert!(render_to_string(&app, 56, 20).contains("re-reads 193k/call"));
        for k in parse_keys("$") {
            app.handle_key(k);
        }
        // e: the why overlay; l: the lifecycle log; Esc closes them.
        for k in parse_keys("e") {
            app.handle_key(k);
        }
        let out = render_to_string(&app, 56, 20);
        assert!(
            out.contains("why  (Esc back)") && out.contains("retires"),
            "{out}"
        );
        for k in parse_keys("Esc,l") {
            app.handle_key(k);
        }
        let out = render_to_string(&app, 56, 20);
        assert!(out.contains("lifecycle  (Esc back)"), "{out}");
        for k in parse_keys("Esc") {
            app.handle_key(k);
        }
        assert_eq!(app.state.view, View::Coach, "Esc closed the overlay only");
        // Enter opens the act popup pre-filled with the fix prompt (the
        // key itself is the card's to name); a prompt may be sent.
        for k in parse_keys("Enter") {
            app.handle_key(k);
        }
        let (panel, text) = app.state.ask.clone().expect("act popup");
        assert_eq!(panel, 9);
        let c = crate::coach::snapshot(&app.state, app.advisor());
        assert_eq!(
            Some(&text),
            c.nudge.as_ref().unwrap().full.fix_prompt.as_ref()
        );
        assert!(
            text.starts_with("Before you change anything else"),
            "{text}"
        );
        assert!(app.state.ask_send_ok);
        assert!(app.state.advice_acting || app.state.advice_view.acting);
        for k in parse_keys("Esc") {
            app.handle_key(k);
        }
        // x snoozes the slot occupant; the engine picks it up on the next tick.
        for k in parse_keys("x") {
            app.handle_key(k);
        }
        app.tick();
        assert_eq!(app.state.advice_view.snoozed.len(), 1);
        let out = render_to_string(&app, 56, 20);
        assert!(
            out.contains("snoozed  correction-streak (5 turns)"),
            "{out}"
        );
        // c returns to the dashboard; c again opens the coach.
        for k in parse_keys("c") {
            app.handle_key(k);
        }
        assert_eq!(app.state.view, View::Dashboard);
        for k in parse_keys("c") {
            app.handle_key(k);
        }
        assert_eq!(app.state.view, View::Coach);
        assert_eq!(app.config.view, "coach");
    }
}
