//! Context: how full the window is, how fast it fills, when autocompact hits.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crossterm::event::{KeyCode, KeyEvent};

use crate::metrics::context::Band;
use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::State;
use crate::ui::widgets::{band_style, sparkline, stacked_bar};

pub struct Context;

impl Panel for Context {
    fn id(&self) -> PanelId {
        1
    }
    fn title(&self) -> String {
        "Context".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let v = state.context();
        let est = if v.window_exact { "" } else { " est" };
        Some(format!("{:.0} %{est}", v.ratio() * 100.0))
    }
    fn min_rows(&self) -> u16 {
        5
    }
    fn priority(&self) -> u8 {
        90
    }
    fn placement(&self) -> Placement {
        Placement::Left
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        use crate::ui::state::ContextView;
        if state.overlay == Some(self.id()) {
            return match state.context_view {
                ContextView::Ledger => crate::ui::ledger_view::handle_key(key, state),
                ContextView::Prefix => Handled::No,
            };
        }
        match key.code {
            KeyCode::Enter => {
                state.context_view = ContextView::Ledger;
                crate::ui::ledger_view::open(state);
                Handled::Yes
            }
            KeyCode::Char('i') => {
                state.context_view = ContextView::Prefix;
                state.overlay = Some(self.id());
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, state: &State) {
        match state.context_view {
            crate::ui::state::ContextView::Ledger => {
                crate::ui::ledger_view::render(frame, area, state)
            }
            crate::ui::state::ContextView::Prefix => {
                crate::ui::prefix_view::render(frame, area, state)
            }
        }
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let v = state.context();
        let bands = state.bands();
        let anatomy = state.anatomy();
        let dim = state.theme.dim();
        let accent = state.theme.accent();
        let width = inner.width.saturating_sub(2) as usize;

        // The stacked bar: what the context is made of, in the band's colour.
        let band = bands.band(v.size);
        let band_colour = match band {
            Band::Ok => state.theme.ok(),
            Band::Warn => state.theme.warn(),
            Band::Blocked => state.theme.crit(),
        };
        let fg = ratatui::style::Style::default().fg(state.theme.fg);
        let parts: Vec<(u64, ratatui::style::Style)> =
            anatomy.slices().iter().map(|(_, t)| (*t, fg)).collect();
        let mut g = vec![Span::raw(" ")];
        g.extend(stacked_bar(&state.theme, &parts, v.window, width));

        let est = if v.window_exact { "" } else { " est" };
        let approx = if anatomy.approx { "≈" } else { "" };
        let breakdown = Line::from(vec![
            Span::styled(
                format!(" {} / {}{est}", fmt::tokens(v.size), fmt::tokens(v.window)),
                band_colour,
            ),
            Span::styled(
                format!(
                    "   prefix {} · inputs {approx}{} · results {approx}{} · thinking {} · harness {approx}{} · prose {approx}{}",
                    fmt::tokens(anatomy.prefix),
                    fmt::tokens(anatomy.tool_inputs),
                    fmt::tokens(anatomy.tool_results),
                    fmt::tokens(anatomy.thinking),
                    fmt::tokens(anatomy.harness),
                    fmt::tokens(anatomy.prose),
                ),
                dim,
            ),
        ]);
        // Claude Code's own footer wording, and the precompute marker.
        let mut footer = vec![
            Span::raw(" "),
            Span::styled(bands.footer(v.size), band_colour),
        ];
        if bands.precompute_armed(v.size) {
            footer.push(Span::styled("  precompute armed", state.theme.warn()));
        }
        if state.autocompact.source != "default" {
            footer.push(Span::styled(
                format!("  ({} override)", state.autocompact.source),
                dim,
            ));
        }
        if let Some((per_turn, approx)) = state.harness_per_turn() {
            footer.push(Span::styled(
                format!(
                    "  harness {}{}/turn",
                    if approx { "≈" } else { "" },
                    fmt::tokens(per_turn)
                ),
                dim,
            ));
        }

        let mut trend = vec![
            Span::raw(" "),
            Span::styled(sparkline(&state.theme, &v.history, 12), accent),
        ];
        if v.velocity > 0.0 {
            trend.push(Span::raw(format!(
                "  +{}/turn",
                fmt::tokens(v.velocity as u64)
            )));
            if let Some(n) = v.turns_until_compaction() {
                let mark = if v.threshold_learned { "" } else { " est" };
                trend.push(Span::raw(" → autocompact in "));
                trend.push(Span::styled(
                    format!("~{} turns", n.ceil() as u64),
                    band_style(&state.theme, 1.0 - (n / 10.0).min(1.0), 0.6, 0.8),
                ));
                trend.push(Span::styled(mark, dim));
            }
        } else if v.history.len() < 2 {
            trend.push(Span::styled("  waiting for a second turn", dim));
        } else {
            trend.push(Span::styled("  not growing", dim));
        }

        let compactions = match v.compactions.last() {
            Some(c) => {
                let detail = match c.duration_ms {
                    Some(ms) => format!(
                        "({} {} → {} in {})",
                        if c.trigger.is_empty() {
                            "auto"
                        } else {
                            &c.trigger
                        },
                        fmt::tokens(c.before),
                        fmt::tokens(c.after),
                        fmt::duration_ms(ms as i64)
                    ),
                    None => format!(
                        "(turn {}, −{} ≈ inferred)",
                        c.turn,
                        fmt::tokens(c.before - c.after)
                    ),
                };
                Line::from(vec![
                    Span::raw(format!(" compactions {} ", v.compactions.len())),
                    Span::styled(detail, dim),
                ])
            }
            None => Line::from(vec![Span::raw(" compactions 0")]),
        };

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(g),
                breakdown,
                Line::from(footer),
                Line::from(trend),
                compactions,
            ]),
            inner,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{SessionInfo, State};
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
        app
    }

    #[test]
    fn context_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("1 Context ─ 40 % est"), "{out}");
        assert!(out.contains("396k / 1.00M est"), "{out}");
        assert!(out.contains("prefix 60k · inputs ≈"), "{out}");
        assert!(out.contains("59% until auto-compact"), "{out}");
        assert!(out.contains("harness ≈"), "{out}");
        assert!(out.contains("/turn → autocompact in ~"), "{out}");
        assert!(out.contains("compactions 0"), "{out}");
    }

    #[test]
    fn exact_window_drops_est_and_bands_colour() {
        let mut app = fixture_app();
        app.state.context_window_exact = Some(500_000);
        app.state.context_size_exact = Some(420_000);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("1 Context ─ 84 %"), "{out}");
        assert!(!out.contains("84 % est"), "{out}");
        assert!(out.contains("420k / 500k"), "{out}");
        assert!(out.contains("precompute armed"), "{out}");
        // Inside the warn band the footer switches to Claude Code's wording.
        app.state.context_size_exact = Some(470_000);
        let out = render_to_string(&app, 80, 51);
        assert!(
            out.contains("Context low (3% remaining) · Run /compact"),
            "{out}"
        );
    }
}
