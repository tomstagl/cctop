//! Tokens & Cost: what was sent and generated, how much was cached, what it cost.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::metrics::{cost, Usage};
use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::State;
use crate::ui::widgets::{band_style, sparkline};

pub struct Tokens;

impl Tokens {
    fn usage(state: &State) -> Usage {
        let mut u = state.agg.total;
        if state.tokens_include_agents {
            u.add(&state.agents_usage());
        }
        u
    }
}

impl Panel for Tokens {
    fn id(&self) -> PanelId {
        2
    }
    fn title(&self) -> String {
        "Tokens & Cost".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        Some(fmt::tokens(Self::usage(state).total()))
    }
    fn min_rows(&self) -> u16 {
        7
    }
    fn priority(&self) -> u8 {
        80
    }
    fn placement(&self) -> Placement {
        Placement::Left
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        if key.code == KeyCode::Enter {
            state.context_view = crate::ui::state::ContextView::Ledger;
            crate::ui::ledger_view::open(state);
            return Handled::Yes;
        }
        if key.code == KeyCode::Char('a') {
            state.tokens_include_agents = !state.tokens_include_agents;
            let msg = if state.tokens_include_agents {
                "tokens: main + subagents"
            } else {
                "tokens: main session only"
            };
            state.set_toast(msg);
            return Handled::Yes;
        }
        Handled::No
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let u = Self::usage(state);
        let dim = Style::default().fg(Color::DarkGray);
        let accent = Style::default().fg(Color::Cyan);
        let rows: [(&str, u64); 5] = [
            ("cache read ", u.cache_read),
            ("cache write", u.cache_write()),
            ("fresh in   ", u.input),
            ("output     ", u.output),
            (" └ thinking", u.thinking),
        ];
        let max = rows.iter().map(|r| r.1).max().unwrap_or(0).max(1);
        // label(11) + space + bar + space + value(6) inside the panel.
        let bar_w = inner.width.saturating_sub(1 + 11 + 1 + 1 + 6 + 1) as usize;
        let mut lines: Vec<Line> = rows
            .iter()
            .map(|(label, v)| {
                let n = ((*v as f64 / max as f64) * bar_w as f64).round() as usize;
                Line::from(vec![
                    Span::raw(format!(" {label} ")),
                    Span::styled("▇".repeat(n), accent),
                    Span::raw(" ".repeat(bar_w.saturating_sub(n))),
                    Span::raw(format!(" {:>6}", fmt::tokens(*v))),
                ])
            })
            .collect();

        // cache hit · cost (burn) · input rate
        let mut l6 = vec![Span::raw(" cache hit ")];
        match u.cache_hit_ratio() {
            Some(r) => l6.push(Span::styled(
                format!("{:.0} %", r * 100.0),
                band_style(1.0 - r, 0.2, 0.5),
            )),
            None => l6.push(Span::styled("—", dim)),
        }
        l6.push(Span::styled("  ·  ", dim));
        match state.cost.current() {
            Some(c) => {
                let approx = if c.approx { "≈ " } else { "" };
                l6.push(Span::raw(format!("{approx}{}", fmt::usd(c.usd))));
            }
            None => l6.push(Span::styled("cost —", dim)),
        }
        let rates = cost::rates(&state.agg, state.cost.pricing(), state.clock_ms());
        if let Some(h) = rates.usd_per_hour {
            l6.push(Span::styled(format!(" ({}/h)", fmt::usd(h)), dim));
        }
        l6.push(Span::styled("  ·  ", dim));
        l6.push(Span::raw(format!(
            "in {}/min",
            fmt::tokens(rates.input_tokens_per_min as u64)
        )));
        lines.push(Line::from(l6));

        // per-turn sparkline + last turn
        let per_turn: Vec<u64> = state
            .agg
            .turns
            .iter()
            .filter(|t| t.api_calls > 0)
            .map(|t| t.usage.total())
            .collect();
        let mut l7 = vec![
            Span::raw(" per turn "),
            Span::styled(sparkline(&per_turn, 14), accent),
        ];
        if let Some(t) = state.agg.turns.iter().rev().find(|t| t.api_calls > 0) {
            let mut s = format!("   last turn {}", fmt::tokens(t.usage.total()));
            let priced: Vec<f64> = t
                .models
                .iter()
                .filter_map(|m| state.cost.pricing().estimate(&t.usage, m))
                .collect();
            if let Some(c) = priced.first() {
                s.push_str(&format!(" · {}", fmt::usd(*c)));
            }
            l7.push(Span::raw(s));
        }
        if !state.tokens_include_agents {
            l7.push(Span::styled("  main only", dim));
        }
        lines.push(Line::from(l7));

        frame.render_widget(Paragraph::new(lines), inner);
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
        app.state.agents = crate::agents::load(&path.with_extension(""));
        app
    }

    #[test]
    fn tokens_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 51);
        // Main + the fork subagent's usage.
        assert!(out.contains("2 Tokens & Cost ─ 3"), "{out}");
        assert!(out.contains("cache read  ▇"), "{out}");
        assert!(out.contains("output"), "{out}");
        assert!(out.contains("└ thinking"), "{out}");
        assert!(out.contains("cache hit 9"), "{out}");
        assert!(out.contains("$9.9"), "{out}");
        assert!(out.contains("/h)"), "{out}");
        assert!(out.contains("in ") && out.contains("/min"), "{out}");
        assert!(out.contains("per turn ▁"), "{out}");
        assert!(out.contains("last turn "), "{out}");
    }

    #[test]
    fn a_toggles_subagent_inclusion_when_focused() {
        let mut app = fixture_app();
        let with = app.state.agg.total.total() + app.state.agents_usage().total();
        assert!(
            app.state.agents_usage().total() > 0,
            "fixture has a subagent"
        );
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains(&crate::ui::fmt::tokens(with)), "{out}");
        app.state.focused = Some(2);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!app.state.tokens_include_agents);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("main only"), "{out}");
        assert!(
            out.contains(&crate::ui::fmt::tokens(app.state.agg.total.total())),
            "{out}"
        );
    }
}
