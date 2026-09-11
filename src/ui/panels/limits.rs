//! Limits: 5 h / 7 d usage, resets, and whether you run out first.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Panel, PanelId};
use crate::ui::state::State;
use crate::ui::widgets::{band_style, gauge};

pub struct LimitsPanel;

impl Panel for LimitsPanel {
    fn id(&self) -> PanelId {
        3
    }
    fn title(&self) -> String {
        "Limits".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let others = match state.other_live_sessions {
            0 => String::new(),
            n => format!(" · {n} other{}", if n == 1 { "" } else { "s" }),
        };
        state.limits.as_ref().map(|l| {
            format!(
                "5h {:.0} % · 7d {:.0} %{others}",
                l.five_hour_pct, l.seven_day_pct
            )
        })
    }
    fn min_rows(&self) -> u16 {
        3
    }
    fn priority(&self) -> u8 {
        70
    }
    fn placement(&self) -> Placement {
        Placement::Left
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let Some(l) = &state.limits else {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(" — run cctop install", dim))),
                inner,
            );
            return;
        };
        let now = state.clock_ms();
        let gauge_w = inner.width.saturating_sub(24) as usize;
        let row = |label: &str, pct: f64, resets: Option<i64>| {
            let mut spans = vec![Span::raw(format!(" {label}  "))];
            spans.extend(gauge(
                &state.theme,
                pct / 100.0,
                gauge_w,
                band_style(&state.theme, pct / 100.0, 0.6, 0.85),
            ));
            spans.push(Span::styled(
                format!("  {pct:>3.0} %"),
                band_style(&state.theme, pct / 100.0, 0.6, 0.85),
            ));
            if let Some(r) = resets {
                spans.push(Span::styled(
                    format!("  ↺ {}", fmt::duration_ms(r - now)),
                    dim,
                ));
            }
            Line::from(spans)
        };
        let mut lines = vec![row("5 h", l.five_hour_pct, l.five_hour_resets_at_ms)];

        let mut l2 = vec![Span::raw("      ")];
        match crate::metrics::limits::exhaustion(&state.limits_series_5h, now) {
            Some(ex) => {
                l2.push(Span::styled(
                    format!("at this rate: exhausted in {}", fmt::duration_ms(ex - now)),
                    dim,
                ));
                match l.five_hour_resets_at_ms {
                    Some(reset) if ex >= reset => {
                        l2.push(Span::styled(", after reset ", dim));
                        l2.push(Span::styled("✓", state.theme.ok()));
                    }
                    Some(_) => {
                        l2.push(Span::styled(", before reset ", dim));
                        l2.push(Span::styled("✗", state.theme.crit()));
                    }
                    None => {}
                }
            }
            None => l2.push(Span::styled("projection — (need 3 samples in 30 min)", dim)),
        }
        let others = state.other_live_sessions;
        if others > 0 {
            l2.push(Span::styled(
                format!(
                    "  · {others} other live session{}",
                    if others == 1 { "" } else { "s" }
                ),
                dim,
            ));
        }
        lines.push(Line::from(l2));
        lines.push(row("7 d", l.seven_day_pct, l.seven_day_resets_at_ms));
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{Limits, SessionInfo, State};
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
        app
    }

    #[test]
    fn without_shim_shows_install_hint() {
        let out = render_to_string(&fixture_app(), 60, 60);
        assert!(out.contains("3 Limits ─"), "{out}");
        assert!(out.contains("— run cctop install"), "{out}");
    }

    #[test]
    fn gauges_countdown_projection_and_other_sessions() {
        let mut app = fixture_app();
        let now = app.state.clock_ms();
        app.state.limits = Some(Limits {
            five_hour_pct: 62.0,
            seven_day_pct: 23.0,
            five_hour_resets_at_ms: Some(now + 6_480_000), // 1h 48m
            seven_day_resets_at_ms: Some(now + 356_400_000), // 4d 03h
            exhaustion_ms: None,
        });
        // 1 % per minute over the last 10 minutes → 38 min to 100 %: before the reset.
        app.state.limits_series_5h = (0..10)
            .map(|i| (now - (9 - i) * 60_000, 53.0 + i as f64))
            .collect();
        app.state.other_live_sessions = 2;
        let out = render_to_string(&app, 64, 60);
        assert!(
            out.contains("3 Limits ─ 5h 62 % · 7d 23 % · 2 others"),
            "{out}"
        );
        assert!(out.contains("5 h  ▇"), "{out}");
        assert!(out.contains(" 62 %  ↺ 1h 48m"), "{out}");
        assert!(out.contains(" 23 %  ↺ 4d 03h"), "{out}");
        assert!(out.contains("exhausted in 38:"), "{out}");
        assert!(out.contains(", before reset ✗"), "{out}");
        // Slow burn → after the reset.
        app.state.limits_series_5h = (0..10)
            .map(|i| (now - (9 - i) * 60_000, 61.0 + i as f64 * 0.1))
            .collect();
        let out = render_to_string(&app, 64, 60);
        assert!(out.contains(", after reset ✓"), "{out}");
        app.state.limits_series_5h.truncate(2);
        assert!(render_to_string(&app, 64, 60).contains("projection —"));
    }
}
