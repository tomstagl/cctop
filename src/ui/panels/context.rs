//! Context: how full the window is, how fast it fills, when autocompact hits.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crossterm::event::{KeyCode, KeyEvent};

use crate::metrics::context::Band;
use crate::ui::fmt;
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

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        use crate::ui::state::ContextView;
        if state.overlay == Some(self.id()) {
            return match state.context_view {
                ContextView::Ledger => crate::ui::ledger_view::handle_key(key, state),
                ContextView::Prefix => Handled::No,
                ContextView::Sources => crate::ui::sources_view::handle_key(key, state),
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
                state.inspector_opens += 1;
                Handled::Yes
            }
            KeyCode::Char('m') => {
                state.context_view = ContextView::Sources;
                state.sources_files = false;
                state.overlay = Some(self.id());
                state.inspector_opens += 1;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn has_overlay(&self) -> bool {
        true
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, state: &State) {
        match state.context_view {
            crate::ui::state::ContextView::Ledger => {
                crate::ui::ledger_view::render(frame, area, state)
            }
            crate::ui::state::ContextView::Prefix => {
                crate::ui::prefix_view::render(frame, area, state)
            }
            crate::ui::state::ContextView::Sources => {
                crate::ui::sources_view::render(frame, area, state)
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
        // The slices in order of agency, coloured by the theme's series
        // ramp (PRD dashboard-v2 §5): the fixed part (prefix, harness) on
        // step 0, the transient part (thinking, dropped at the next
        // boundary) on step 1, the part the person can move (inputs,
        // results, prose) on step 2; what no slice claims is step 0. The
        // status palette is for thresholds and says nothing here.
        let series = state.theme.series(3);
        let step = |i: usize| ratatui::style::Style::default().fg(series[i]);
        let parts: Vec<(u64, ratatui::style::Style)> = vec![
            (anatomy.prefix, step(0)),
            (anatomy.harness, step(0)),
            (anatomy.thinking, step(1)),
            (anatomy.tool_inputs, step(2)),
            (anatomy.tool_results, step(2)),
            (anatomy.prose, step(2)),
            (anatomy.unattributed, step(0)),
        ];
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
                    "   prefix {} · harness {approx}{} · thinking {} · inputs {approx}{} · results {approx}{} · prose {approx}{}",
                    fmt::tokens(anatomy.prefix),
                    fmt::tokens(anatomy.harness),
                    fmt::tokens(anatomy.thinking),
                    fmt::tokens(anatomy.tool_inputs),
                    fmt::tokens(anatomy.tool_results),
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
        if let Some(velocity) = v.velocity.filter(|v| *v > 0.0) {
            trend.push(Span::raw(format!(
                "  +{}/turn",
                fmt::tokens(velocity as u64)
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
        let mut app = fixture_app();
        app.state.open = Some(1);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("1 Context ─ 40 % est"), "{out}");
        assert!(out.contains("396k / 1.00M est"), "{out}");
        assert!(out.contains("prefix 60k · harness ≈"), "{out}");
        assert!(out.contains("59% until auto-compact"), "{out}");
        assert!(out.contains("harness ≈"), "{out}");
        assert!(out.contains("/turn → autocompact in ~"), "{out}");
        assert!(out.contains("compactions 0"), "{out}");
    }

    /// The bar's cells carry only the theme's series ramp (PRD dashboard-v2
    /// US-105): the buffer, not the text — `dashboard_ascii` has no colour
    /// to check. Panel 1's bar is the second row of the frame.
    #[test]
    fn the_bar_uses_only_the_series_ramp() {
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;
        use ratatui::Terminal;
        let mut app = fixture_app();
        app.state.open = Some(1);
        app.set_theme("default-dark");
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let series = app.state.theme.series(3);
        let empty = app.state.theme.dim;
        let mut filled = std::collections::BTreeSet::new();
        for x in 2..78u16 {
            let cell = &buf[(x, 1)];
            let fg = cell.fg;
            match cell.symbol() {
                "▇" | "▆" => {
                    assert!(series.contains(&fg), "cell {x}: {fg:?} not in {series:?}");
                    filled.insert(format!("{fg:?}"));
                }
                "▁" => assert_eq!(fg, empty, "cell {x}"),
                other => panic!("cell {x}: {other:?} is not a bar glyph"),
            }
        }
        assert!(
            filled.len() >= 2,
            "fixture A's bar spans more than one step: {filled:?}"
        );
        assert!(!series.contains(&app.state.theme.ok) && !series.contains(&app.state.theme.crit));
        assert_eq!(series[0], Color::Rgb(0x51, 0x6A, 0x69));
    }

    #[test]
    fn exact_window_drops_est_and_bands_colour() {
        let mut app = fixture_app();
        app.state.open = Some(1);
        app.state.context_window_exact = Some(500_000);
        app.state.context_size_exact = Some(420_000);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("1 Context ─ 84 %"), "{out}");
        assert!(!out.contains("84 % est"), "{out}");
        assert!(out.contains("420k / 500k"), "{out}");
        assert!(out.contains("precompute armed"), "{out}");
        // Inside the warn band the footer switches to Claude Code's wording
        // (500k: effective 480k, threshold 467k, the band from 447k).
        app.state.context_size_exact = Some(450_000);
        let out = render_to_string(&app, 80, 51);
        assert!(
            out.contains("Context low (3% remaining) · Run /compact"),
            "{out}"
        );
    }
}
