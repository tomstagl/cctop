//! Limits: 5 h / 7 d usage, resets, and whether you run out first.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
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

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let now = state.clock_ms();
        // A 429 in the transcript is exact, shim or not.
        let hit_line = state.rate_limit_hit().map(|(kind, resets, retry)| {
            let mut spans = vec![Span::styled(
                format!(" ● rate limited ({})", kind.replace('_', " ")),
                state.theme.crit(),
            )];
            if let Some(r) = resets {
                spans.push(Span::styled(
                    format!(
                        " · resets {} (in {})",
                        fmt::clock_hhmm(r),
                        fmt::duration_ms(r - now)
                    ),
                    dim,
                ));
            }
            if let Some(s) = retry {
                spans.push(Span::styled(format!(" · low-priority retry {s} s"), dim));
            }
            Line::from(spans)
        });
        let Some(l) = &state.limits else {
            let mut lines = vec![Line::from(Span::styled(" — run cctop install", dim))];
            if let Some(h) = hit_line {
                lines.insert(0, h);
            }
            frame.render_widget(Paragraph::new(lines), inner);
            return;
        };
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
        // The 429 state when hit; else the spend limit and the other sessions.
        if let Some(h) = hit_line {
            lines.push(h);
            frame.render_widget(Paragraph::new(lines), inner);
            return;
        }
        let mut l4: Vec<Span> = Vec::new();
        if let Some(p) = state.status_facts.spend_limit_pct {
            l4.push(Span::raw(" spend limit "));
            l4.push(Span::styled(
                format!("{p:.0} %"),
                band_style(&state.theme, p / 100.0, 0.6, 0.85),
            ));
        }
        if !state.other_sessions.is_empty() {
            let mut parts: Vec<String> = state
                .other_sessions
                .iter()
                .take(3)
                .map(|(name, busy, since)| {
                    let name = if name.is_empty() {
                        "session"
                    } else {
                        name.as_str()
                    };
                    let since = if *since > 0 {
                        format!(" {}", fmt::duration_ms(now - since))
                    } else {
                        String::new()
                    };
                    format!(
                        "{} {}{since}",
                        fmt::clip(name, 12),
                        if *busy { "busy" } else { "idle" }
                    )
                })
                .collect();
            if state.other_sessions.len() > 3 {
                parts.push(format!("+{}", state.other_sessions.len() - 3));
            }
            l4.push(Span::styled(
                format!("  others: {}", parts.join(" · ")),
                dim,
            ));
        }
        if !l4.is_empty() {
            lines.push(Line::from(l4));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::{parse_file, Line};
    use crate::ui::state::{Limits, SessionInfo, State};

    #[test]
    fn a_429_line_shows_the_reset_without_the_shim() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path).unwrap() {
            app.feed(l);
        }
        app.feed(Line::parse(r#"{"type":"assistant","timestamp":"2026-08-27T10:30:00Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"limit"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1787830200,"lowPriorityRetryAfterSeconds":20}}"#).unwrap());
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app.state.open = Some(3);
        let hit = app.state.rate_limit_hit().unwrap();
        assert_eq!(hit.0, "five_hour");
        let out = render_to_string(&app, 120, 70);
        assert!(
            out.contains(
                "● rate limited (five hour) · resets 11:30 (in 1h 00m) · low-priority retry 20 s"
            ),
            "{out}"
        );
        // A later successful call clears it.
        app.feed(Line::parse(r#"{"type":"assistant","timestamp":"2026-08-27T10:40:00Z","message":{"id":"ok1","model":"claude-sonnet-5","content":[{"type":"text","text":"back"}],"usage":{"input_tokens":5}}}"#).unwrap());
        assert!(app.state.rate_limit_hit().is_none());
        app.state.other_sessions = vec![(
            "pane-work".into(),
            false,
            app.state.last_line_at_ms.unwrap() - 600_000,
        )];
        app.state.status_facts.spend_limit_pct = Some(40.0);
        app.state.limits = Some(Limits {
            five_hour_pct: 42.0,
            seven_day_pct: 17.0,
            ..Default::default()
        });
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        let out = render_to_string(&app, 120, 70);
        assert!(
            out.contains("spend limit 40 %  others: pane-work idle 10:00"),
            "{out}"
        );
    }
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
        let mut app = fixture_app();
        app.state.open = Some(3);
        let out = render_to_string(&app, 60, 60);
        assert!(out.contains("3 Limits ─"), "{out}");
        assert!(out.contains("— run cctop install"), "{out}");
    }

    #[test]
    fn gauges_countdown_projection_and_other_sessions() {
        let mut app = fixture_app();
        app.state.open = Some(3);
        let now = app.state.clock_ms();
        app.state.limits = Some(Limits {
            five_hour_pct: 62.0,
            seven_day_pct: 23.0,
            five_hour_resets_at_ms: Some(now + 6_480_000), // 1h 48m
            seven_day_resets_at_ms: Some(now + 356_400_000), // 4d 03h
            exhaustion_ms: None,
            exhaustion_in_active_hours: None,
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
