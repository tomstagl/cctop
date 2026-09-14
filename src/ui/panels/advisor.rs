//! Advisor: one evidence-backed recommendation at a time — the coach's
//! slot occupant first, then the ranked queue. `x` snoozes a rule for five
//! human turns (the third time for the session), `X` for the session;
//! `Enter` opens the explanation and marks the occupant as being acted on.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::State;

pub struct AdvisorPanel;

impl Panel for AdvisorPanel {
    fn id(&self) -> PanelId {
        9
    }
    fn title(&self) -> String {
        "Advisor".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let n = state.advice.len();
        let snoozed = state.advice_view.snoozed.len();
        let tail = if snoozed > 0 {
            format!(" · {snoozed} snoozed")
        } else {
            String::new()
        };
        Some(if n == 0 {
            format!("nothing to fix{tail}")
        } else {
            format!("{} of {n}{tail}", state.advice_index.min(n - 1) + 1)
        })
    }
    fn min_rows(&self) -> u16 {
        2
    }
    fn priority(&self) -> u8 {
        75
    }
    fn placement(&self) -> Placement {
        Placement::Bottom
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        let n = state.advice.len();
        match key.code {
            KeyCode::Char('n') if n > 0 => state.advice_index = (state.advice_index + 1) % n,
            KeyCode::Char('N') if n > 0 => state.advice_index = (state.advice_index + n - 1) % n,
            KeyCode::Char(c @ ('x' | 'X')) if n > 0 => {
                let i = state.advice_index.min(n - 1);
                let rule = state.advice[i].rule;
                state.advice_snoozed.push((rule, c == 'X'));
                state.advice.remove(i);
                state.advice_index = 0;
                state.set_toast(if c == 'X' {
                    format!("{rule} snoozed for the session")
                } else {
                    format!("{rule} snoozed for 5 turns")
                });
            }
            KeyCode::Enter if n > 0 && state.overlay.is_none() => {
                if state.advice_index == 0 && state.advice_view.has_occupant {
                    state.advice_acting = true;
                }
                state.overlay = Some(self.id());
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let amber = state.theme.warn();
        let accent = state.theme.accent();
        let Some(a) = state
            .advice
            .get(state.advice_index.min(state.advice.len().saturating_sub(1)))
        else {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " no recommendation right now — the session looks efficient",
                    dim,
                ))),
                inner,
            );
            return;
        };
        let w = inner.width.saturating_sub(4) as usize;
        let is_slot = state.advice_index == 0 && state.advice_view.has_occupant;
        let class = format!(
            "{} {}{} ",
            a.urgency.label(),
            a.rule,
            if is_slot && state.advice_view.acting {
                " acting…"
            } else {
                ""
            }
        );
        let l1 = Line::from(vec![
            Span::styled(if is_slot { " ▸ " } else { " · " }, amber),
            Span::styled(class.clone(), dim),
            Span::raw(fmt::clip(
                &a.headline,
                w.saturating_sub(class.chars().count()),
            )),
        ]);
        let tail = format!("  {} · n next · x snooze", a.saving.label());
        let action_w = w.saturating_sub(tail.chars().count());
        let l2 = Line::from(vec![
            Span::raw("   "),
            Span::styled(fmt::clip(&a.action, action_w), accent),
            Span::styled(tail, dim),
        ]);
        frame.render_widget(Paragraph::new(vec![l1, l2]), inner);
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, state: &State) {
        let Some(a) = state
            .advice
            .get(state.advice_index.min(state.advice.len().saturating_sub(1)))
        else {
            return;
        };
        let block = Block::default().borders(Borders::ALL).title(format!(
            " {} {} — {}  (Esc back) ",
            a.urgency.label(),
            a.rule,
            fmt::clip(&a.headline, 60)
        ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let dim = state.theme.dim();
        let mut lines = vec![
            Line::from(vec![
                Span::styled(" Evidence  ", dim),
                Span::raw(a.evidence.clone()),
            ]),
            Line::from(vec![
                Span::styled(" Action    ", dim),
                Span::raw(a.action.clone()),
            ]),
        ];
        if !a.action_text.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!(" {:<10}", a.action_kind.label()), dim),
                Span::raw(a.action_text.clone()),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled(" Saving    ", dim),
            Span::raw(a.saving.label()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Retires   ", dim),
            Span::raw(format!(
                "on {} · since turn {} · window {} turn{}",
                a.retires_on,
                a.since_turn,
                a.window_turns,
                if a.window_turns == 1 { "" } else { "s" }
            )),
        ]));
        let v = &state.advice_view;
        if !v.snoozed.is_empty() {
            let parts: Vec<String> = v
                .snoozed
                .iter()
                .map(|(r, until)| match until {
                    Some(t) => format!("{r} until turn {t}"),
                    None => format!("{r} this session"),
                })
                .collect();
            lines.push(Line::from(vec![
                Span::styled(" Snoozed   ", dim),
                Span::raw(parts.join(" · ")),
            ]));
        }
        if !v.recent.is_empty() {
            let parts: Vec<String> = v
                .recent
                .iter()
                .take(3)
                .map(|l| format!("{} {}", l.rule, l.what))
                .collect();
            lines.push(Line::from(vec![
                Span::styled(" Recent    ", dim),
                Span::raw(parts.join(" · ")),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" Why       ", dim),
            Span::raw(crate::advisor::rules::explain(a.doc_key)),
        ]));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn fixture_app() -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path).unwrap() {
            app.feed(l);
        }
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app.tick();
        app
    }

    #[test]
    fn advisor_panel_on_fixture_cycles_dismisses_and_explains() {
        let mut app = fixture_app();
        let out = render_to_string(&app, 60, 60);
        // The fixture has a 214-call chrome session: A03 (runaway result) or A05 may fire.
        assert!(out.contains("9 Advisor ─ "), "{out}");
        if app.state.advice.is_empty() {
            assert!(out.contains("no recommendation right now"), "{out}");
            return;
        }
        assert!(out.contains("▸ "), "{out}");
        assert!(out.contains("n next"), "{out}");
        let n = app.state.advice.len();
        app.state.focused = Some(9);
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.state.advice_index, 1 % n);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(9));
        let out = render_to_string(&app, 80, 24);
        assert!(out.contains("Evidence") && out.contains("Why"), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let rule = app.state.advice[app.state.advice_index].rule;
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.state.advice.len(), n - 1);
        assert_eq!(app.state.advice_snoozed, vec![(rule, false)]);
        app.tick();
        assert!(
            app.state.advice.iter().all(|a| a.rule != rule),
            "snoozed rule stays out after re-evaluation"
        );
        assert_eq!(app.state.advice_view.snoozed.len(), 1);
        assert!(
            app.state.advice_view.snoozed[0].1.is_some(),
            "five turns, not the session"
        );
        assert!(
            app.state
                .events
                .iter()
                .any(|e| e.kind == crate::events::Kind::Coach && e.text.contains("snoozed")),
            "the snooze is an Events row"
        );
        assert!(render_to_string(&app, 60, 60).contains("1 snoozed"));
    }
}
