//! Context: how full the window is, how fast it fills, when autocompact hits.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Panel, PanelId};
use crate::ui::state::State;
use crate::ui::widgets::{band_style, gauge, sparkline};

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
        4
    }
    fn priority(&self) -> u8 {
        90
    }
    fn placement(&self) -> Placement {
        Placement::Left
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let v = state.context();
        let dim = Style::default().fg(Color::DarkGray);
        let accent = Style::default().fg(Color::Cyan);
        let width = inner.width.saturating_sub(2) as usize;

        let mut g = vec![Span::raw(" ")];
        g.extend(gauge(v.ratio(), width, band_style(v.ratio(), 0.6, 0.8)));

        let est = if v.window_exact { "" } else { " est" };
        let breakdown = Line::from(vec![
            Span::raw(format!(
                " {} / {}{est}   ",
                fmt::tokens(v.size),
                fmt::tokens(v.window)
            )),
            Span::styled(
                format!(
                    "prefix {} · msgs {} · free {}",
                    fmt::tokens(v.prefix),
                    fmt::tokens(v.messages()),
                    fmt::tokens(v.free())
                ),
                dim,
            ),
        ]);

        let mut trend = vec![
            Span::raw(" "),
            Span::styled(sparkline(&v.history, 12), accent),
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
                    band_style(1.0 - (n / 10.0).min(1.0), 0.6, 0.8),
                ));
                trend.push(Span::styled(mark, dim));
            }
        } else if v.history.len() < 2 {
            trend.push(Span::styled("  waiting for a second turn", dim));
        } else {
            trend.push(Span::styled("  not growing", dim));
        }

        let compactions = match v.compactions.last() {
            Some(c) => Line::from(vec![
                Span::raw(format!(" compactions {} ", v.compactions.len())),
                Span::styled(
                    format!("(turn {}, −{})", c.turn, fmt::tokens(c.before - c.after)),
                    dim,
                ),
            ]),
            None => Line::from(vec![Span::raw(" compactions 0")]),
        };

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(g),
                breakdown,
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
        assert!(out.contains("prefix 60k · msgs 335k · free 603k"), "{out}");
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
    }
}
