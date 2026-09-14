//! Tokens & Cost: what was sent and generated, how much was cached, what it cost.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::metrics::{cost, Usage};
use crate::ui::fmt;
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
        let dim = state.theme.dim();
        let accent = state.theme.accent();
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
                    Span::styled(state.theme.gauge_fill().repeat(n), accent),
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
                band_style(&state.theme, 1.0 - r, 0.2, 0.5),
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
            Span::styled(sparkline(&state.theme, &per_turn, 14), accent),
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
        if let Some(b) = state.baseline.as_ref().filter(|b| b.sessions > 0) {
            let turns = state.agg.human_turns().max(1) as f64;
            let tok = crate::baseline::Baseline::multiplier(
                Some(state.agg.total.total() as f64 / turns),
                b.tokens_per_turn,
            );
            let usd = crate::baseline::Baseline::multiplier(
                state.cost.current().map(|c| c.usd / turns),
                b.cost_per_turn,
            );
            let mut parts = Vec::new();
            if let Some(t) = tok {
                parts.push(format!("×{t:.1}t"));
            }
            if let Some(c) = usd {
                parts.push(format!("×{c:.1}$"));
            }
            if !parts.is_empty() {
                l7.push(Span::styled(format!(" · 7d {}", parts.join(" ")), dim));
            }
        }
        if !state.tokens_include_agents {
            l7.push(Span::styled("  main only", dim));
        }
        lines.push(Line::from(l7));

        // The cost gradient: what continuing costs at this context.
        let mut l8 = vec![Span::raw(" ")];
        match state.gradient() {
            Some(g) => {
                if g.cold {
                    l8.push(Span::styled("cold: ", state.theme.warn()));
                }
                l8.push(Span::raw(format!(
                    "≈{}/call · ≈{}/turn",
                    fmt::usd(g.per_call),
                    fmt::usd(g.per_turn)
                )));
                l8.push(Span::styled(
                    format!(" (≈{} at 100k)", fmt::usd(g.per_turn_at_100k)),
                    dim,
                ));
                l8.push(Span::raw(format!(
                    " · next 30c ≈{}",
                    fmt::usd(g.next_30_calls)
                )));
            }
            None => l8.push(Span::styled("$/call —", dim)),
        }
        lines.push(Line::from(l8));

        // The cache line: Claude Code's own diagnosis when the shim is there.
        let mut l9 = vec![Span::raw(" cache ")];
        match state.cache_clock() {
            Some(k) => {
                let mark = if k.approx { "≈" } else { "" };
                if k.remaining_ms > 0 {
                    l9.push(Span::styled(
                        format!("{mark}warm {}", fmt::duration_ms(k.remaining_ms)),
                        state.theme.ok(),
                    ));
                } else {
                    l9.push(Span::styled(format!("{mark}cold"), state.theme.warn()));
                    if state.cache.recache_tokens_if_cold > 0 {
                        l9.push(Span::styled(
                            format!(
                                " · {} re-write",
                                fmt::tokens(state.cache.recache_tokens_if_cold)
                            ),
                            dim,
                        ));
                    }
                }
                let ttl = if state.cache_ttl_ms() == 3_600_000 {
                    "1h"
                } else {
                    "5m"
                };
                l9.push(Span::styled(format!(" · TTL {ttl}"), dim));
                if state.cache.from_shim {
                    let mut s = format!(" · misses {}", state.cache.misses);
                    if let Some(c) = &state.cache.last_miss_cause {
                        s.push_str(&format!(" ({c})"));
                    }
                    if state.cache.expected_rebuilds > 0 {
                        s.push_str(&format!(" · rebuilds {}", state.cache.expected_rebuilds));
                    }
                    l9.push(Span::styled(s, dim));
                }
            }
            None => l9.push(Span::styled("—", dim)),
        }
        lines.push(Line::from(l9));

        // Where the tokens went, the agents' share, the limit weight.
        let mut l10 = vec![Span::raw(" ")];
        let top = state.attribution_top(3);
        if !top.is_empty() {
            let parts: Vec<String> = top
                .iter()
                .map(|(k, share)| format!("{k} {:.0} %", share * 100.0))
                .collect();
            l10.push(Span::styled(format!("where: {}", parts.join(" · ")), dim));
        }
        if let Some((usd, share)) = state.agents_cost() {
            l10.push(Span::raw(format!(
                "  agents {} ({:.0} %)",
                fmt::usd(usd),
                share * 100.0
            )));
        }
        let flags = state.behaviour_flags();
        if let Some(m) = state.model() {
            l10.push(Span::styled(
                format!(
                    "  weight ×{:.0}",
                    crate::harness_facts::usage_weight::tier(m)
                ),
                dim,
            ));
        }
        if let Some(tip) = flags.tips().first() {
            l10.push(Span::styled(format!("  {tip}"), state.theme.warn()));
        }
        lines.push(Line::from(l10));

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
        let mut app = fixture_app();
        app.state.open = Some(2);
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
    fn gradient_cache_and_attribution_rows() {
        let mut app = fixture_app();
        app.state.open = Some(2);
        let out = render_to_string(&app, 140, 70);
        assert!(out.contains("/call · ≈$"), "{out}");
        assert!(out.contains("at 100k) · next 30c ≈$"), "{out}");
        // No shim on a fixture: the clock is the last call + TTL, marked ≈.
        assert!(out.contains("cache ≈warm 50:33 · TTL 1h"), "{out}");
        assert!(out.contains("weight ×3"), "{out}");
        assert!(out.contains("of your usage was at >150k"), "{out}");
    }

    #[test]
    fn baseline_multipliers_render() {
        let mut app = fixture_app();
        app.state.open = Some(2);
        app.state.baseline = Some(crate::baseline::Baseline {
            sessions: 3,
            cost_per_turn: Some(0.33),
            tokens_per_turn: Some(1_000_000.0),
            ..Default::default()
        });
        let out = render_to_string(&app, 72, 70);
        assert!(out.contains("· 7d ×"), "{out}");
        assert!(out.contains("×2.1$"), "{out}");
    }

    #[test]
    fn a_toggles_subagent_inclusion_when_focused() {
        let mut app = fixture_app();
        app.state.open = Some(2);
        let with = app.state.agg.total.total() + app.state.agents_usage().total();
        assert!(
            app.state.agents_usage().total() > 0,
            "fixture has a subagent"
        );
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains(&crate::ui::fmt::tokens(with)), "{out}");
        app.state.open = Some(2);
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
