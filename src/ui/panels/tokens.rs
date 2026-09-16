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
            u.add(&state.team_usage());
        }
        u
    }
}

/// `team ≈$31.15 (76 %, 5 of 5)`, or `(… 4 of 5 read)` when a transcript
/// is missing — the breakdown line's team part (team PRD §4.2), shared with
/// dashboard row 2 and `cctop report`.
pub fn team_part(t: cost::Cost, share: f64, (read, members): (usize, usize)) -> String {
    let mark = if t.approx { "≈" } else { "" };
    let read = if read < members {
        format!("{read} of {members} read")
    } else {
        format!("{read} of {members}")
    };
    format!(
        "team {mark}{} ({:.0} %, {read})",
        fmt::usd(t.usd),
        share * 100.0
    )
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
            let msg = match (state.tokens_include_agents, state.team.is_some()) {
                (true, true) => "tokens: main + subagents + team",
                (true, false) => "tokens: main + subagents",
                (false, _) => "tokens: main session only",
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
        // The headline is the session's whole spend — the ledger, the main
        // responses after it and the agents' calls after it — main-only
        // with `a` off (agent PRD §4.2).
        let breakdown = state.cost_breakdown();
        match breakdown {
            Some(b) => {
                let approx = if b.headline.approx { "≈ " } else { "" };
                l6.push(Span::raw(format!("{approx}{}", fmt::usd(b.headline.usd))));
            }
            None => l6.push(Span::styled("cost —", dim)),
        }
        let any_agents = breakdown.is_some_and(|b| b.any_agents);
        let any_team = breakdown.is_some_and(|b| b.team.is_some());
        let rates = cost::rates(&state.agg, state.cost.pricing(), state.clock_ms());
        if let Some(h) = rates.usd_per_hour {
            // The rate is the main transcript's: agents have no turn, and
            // a team's burn rate is a follow-up (team PRD §11).
            let suffix = if any_agents || any_team { " main" } else { "" };
            l6.push(Span::styled(format!(" ({}/h{suffix})", fmt::usd(h)), dim));
        }
        l6.push(Span::styled("  ·  ", dim));
        l6.push(Span::raw(format!(
            "in {}/min",
            fmt::tokens(rates.input_tokens_per_min as u64)
        )));
        if breakdown.is_some() {
            // Subscription plans have no per-token bill: the dollars on
            // this line are the list price of the same calls (registry
            // `cost`). Last, so a narrow panel clips the label, not a figure.
            l6.push(Span::styled("  ·  API-equivalent", dim));
        }
        lines.push(Line::from(l6));

        // Where the headline comes from, when agents or a team spent
        // anything: the first two figures add up to it; `agents` is the
        // whole session's agent spend as a share of the combined figure,
        // `team` the teammates' own figures (team PRD §4.2).
        if let Some(b) = breakdown.filter(|b| b.any_agents || b.team.is_some()) {
            let mut l = vec![Span::raw(" ")];
            match b.ledger {
                Some(ledger) => {
                    l.push(Span::raw(format!("ledger {}", fmt::usd(ledger))));
                    if let Some(since) = b.since {
                        l.push(Span::styled(" · ", dim));
                        l.push(Span::raw(format!("since ≈{}", fmt::usd(since.usd))));
                    }
                }
                None => {
                    let main = state.cost.current().map(|c| c.usd).unwrap_or(0.0);
                    l.push(Span::raw(format!("main ≈{}", fmt::usd(main))));
                }
            }
            if b.any_agents {
                match b.agents {
                    Some(a) => {
                        l.push(Span::styled(" · ", dim));
                        l.push(Span::raw(format!(
                            "agents ≈{} ({:.0} %)",
                            fmt::usd(a.usd),
                            b.agents_share * 100.0
                        )));
                    }
                    None => {
                        l.push(Span::styled(" · ", dim));
                        l.push(Span::styled(
                            format!("agents — {}", fmt::tokens(state.agents_usage().total())),
                            dim,
                        ));
                    }
                }
            }
            // Agents on a model the table does not know are in no dollar
            // figure: say so beside the ones that are.
            if b.agents.is_some() && b.unpriced_agent_tokens > 0 {
                l.push(Span::styled(
                    format!(" · unpriced {}", fmt::tokens(b.unpriced_agent_tokens)),
                    dim,
                ));
            }
            // The team: `≈` only while a teammate works or is priced after
            // its ledger; `N of M read` names the transcripts not found.
            if let Some(t) = b.team {
                l.push(Span::styled(" · ", dim));
                l.push(Span::raw(team_part(t, b.team_share, b.team_read)));
            }
            lines.push(Line::from(l));
        }

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
            // The baseline's cost per turn is `cost-state ÷ turns`, and the
            // ledger holds the agents' calls, so the live side is the
            // combined figure whatever `a` says.
            let usd = crate::baseline::Baseline::multiplier(
                breakdown.map(|c| c.combined.usd / turns),
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
        let out = render_to_string(&app, 72, 51);
        // Main + the fork subagent's usage.
        assert!(out.contains("2 Tokens & Cost ─ 3"), "{out}");
        assert!(out.contains("cache read  ▇"), "{out}");
        assert!(out.contains("output"), "{out}");
        assert!(out.contains("└ thinking"), "{out}");
        assert!(out.contains("cache hit 9"), "{out}");
        // The headline is combined: the ledger ($9.90) plus the fork's 7
        // own calls, which post-date the ledger's moment on this fixture
        // (its fork was taken from a later session), so it is `≈`.
        assert!(out.contains("≈ $10.0 ("), "{out}");
        assert!(out.contains("/h main)"), "{out}");
        assert!(out.contains("in ") && out.contains("/min"), "{out}");
        // The dollars are list price, not a bill: said at the line's end.
        let out80 = render_to_string(&app, 80, 51);
        assert!(out80.contains("/min  ·  API-equivalent"), "{out80}");
        assert!(
            out.contains("ledger $9.90 · since ≈$0.13 · agents ≈$0.13 (1 %)"),
            "{out}"
        );
        assert!(out.contains("per turn ▁"), "{out}");
        assert!(out.contains("last turn "), "{out}");
        assert!(
            !out.contains("  agents $"),
            "the fragment left line 10: {out}"
        );
        // With `a` off the headline is the ledger plus the main responses
        // after it, exact here; the breakdown line's `since` is main-only.
        app.state.tokens_include_agents = false;
        let out = render_to_string(&app, 72, 51);
        assert!(out.contains("  $9.90 ("), "{out}");
        assert!(out.contains("ledger $9.90 · agents ≈$0.13 (1 %)"), "{out}");
        assert!(out.contains("main only"), "{out}");
    }

    /// Fixture D loaded the way `cctop run --session` loads it: the lead's
    /// lines, its (absent) subagents and its team beside the file.
    fn fixture_app_d() -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-d.jsonl");
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
        app.state.team = crate::load::load_team(&path, &app.state);
        app
    }

    #[test]
    fn tokens_panel_on_fixture_d_folds_the_team_in() {
        let mut app = fixture_app_d();
        assert!(app.state.team.is_some());
        app.state.open = Some(2);
        insta::assert_snapshot!("tokens_d_120x30", render_to_string(&app, 120, 30));
        insta::assert_snapshot!("tokens_d_56x20", render_to_string(&app, 56, 20));
        let out = render_to_string(&app, 120, 30);
        // The headline is the lead's exact ledger plus the team's part,
        // `≈` because one teammate still runs (priced) and one has no
        // transcript; the breakdown says so with `2 of 3 read`.
        assert!(out.contains("≈ $10.6 ("), "{out}");
        assert!(out.contains("/h main)"), "{out}");
        assert!(
            out.contains("ledger $10.2 · team ≈$0.48 (5 %, 2 of 3 read)"),
            "{out}"
        );
        assert!(!out.contains("agents"), "no subagents on D: {out}");
        // The team's tokens are a top-3 share on line 10.
        assert!(out.contains("where:") && out.contains("team "), "{out}");
        // `a` takes the team out with the agents: the lead's ledger alone,
        // exact.
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!app.state.tokens_include_agents);
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("  $10.2 ("), "{out}");
        assert!(out.contains("main only"), "{out}");
        assert!(
            out.contains("ledger $10.2 · team ≈$0.48 (5 %, 2 of 3 read)"),
            "the breakdown still names the team: {out}"
        );
        assert_eq!(
            app.state.toast.as_ref().map(|t| t.0.as_str()),
            Some("tokens: main session only")
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(
            app.state.toast.as_ref().map(|t| t.0.as_str()),
            Some("tokens: main + subagents + team")
        );
    }

    #[test]
    fn breakdown_line_without_a_ledger_and_without_agents() {
        // Before the fixture's cost-state: every part is priced and the
        // line names main and agents.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path)
            .unwrap()
            .into_iter()
            .take_while(|l| !matches!(l, crate::transcript::Line::CostState(_)))
        {
            app.feed(l);
        }
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app.state.agents = crate::agents::load(&path.with_extension(""));
        app.state.open = Some(2);
        let out = render_to_string(&app, 72, 51);
        assert!(out.contains(" main ≈$"), "{out}");
        assert!(out.contains(" · agents ≈$0.13 (1 %)"), "{out}");
        // An agent on a model the table does not know is named by its
        // tokens beside the priced ones, never folded into a dollar figure.
        let mut odd = app.state.agents["a9a92645226d3a561"].clone();
        odd.id = "odd".into();
        odd.model = "claude-unknown-9".into();
        app.state.agents.insert("odd".into(), odd);
        let out = render_to_string(&app, 90, 51);
        assert!(out.contains("agents ≈$0.13 (1 %) · unpriced 485k"), "{out}");
        app.state.agents.remove("odd");
        // No agents: the line is not drawn and nothing else moves (FR-6).
        app.state.agents.clear();
        let out = render_to_string(&app, 72, 51);
        assert!(!out.contains("main ≈$") && !out.contains("agents"), "{out}");
        assert!(out.contains("/h)  ·  in "), "no `main` suffix: {out}");
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
        // The 7-day baseline's cost per turn is `cost-state ÷ turns` and
        // the ledger holds the agents' calls, so the live side is the
        // combined figure: $10.03 over 14 turns against $0.33.
        assert!(out.contains("×2.2$"), "{out}");
        app.state.tokens_include_agents = false;
        let out = render_to_string(&app, 72, 70);
        assert!(out.contains("×2.2$"), "combined whatever `a` says: {out}");
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
