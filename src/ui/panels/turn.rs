//! Turn: what the current turn is doing and waiting on.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::panel::{Panel, PanelId};
use crate::ui::state::State;

pub struct TurnPanel;

impl Panel for TurnPanel {
    fn id(&self) -> PanelId {
        4
    }
    fn title(&self) -> String {
        "Turn".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        state
            .agg
            .current_turn()
            .and_then(|t| t.elapsed_ms(state.clock_ms()))
            .map(fmt::duration_ms)
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let accent = state.theme.accent();
        let amber = state.theme.warn();
        let now = state.clock_ms();
        let turn = state.agg.current_turn();

        // Line 1: the phase (the classifier over the last calls, labelled
        // as such — the header's word is the same classifier), then the
        // turn's own facts: elapsed · api calls · api vs tool time · retries
        let mut l1 = vec![Span::raw(" ")];
        if let Some((phase, run)) = state.tools.phase_now() {
            if turn.is_some_and(|t| t.duration_ms.is_none()) || state.tools.running().is_some() {
                let word = match state.waiting() {
                    Some(w) if w.kind != crate::ui::state::WaitingKind::Asked => {
                        "WAITING".to_string()
                    }
                    _ => phase.word().to_string(),
                };
                l1.push(Span::styled("phase ", dim));
                l1.push(Span::styled(format!("{word} ×{run}"), accent));
                l1.push(Span::styled(" · ", dim));
            }
        }
        match turn {
            Some(t) => {
                let el = t
                    .elapsed_ms(now)
                    .map(fmt::duration_ms)
                    .unwrap_or_else(|| "—".into());
                l1.push(Span::raw(format!(
                    "turn elapsed {el} · api {} calls",
                    t.api_calls
                )));
                if t.api_ms > 0 || t.tool_ms > 0 {
                    l1.push(Span::styled(
                        format!(
                            " · api {} / tools {}",
                            fmt::duration_ms(t.api_ms),
                            fmt::duration_ms(t.tool_ms)
                        ),
                        dim,
                    ));
                }
            }
            None => l1.push(Span::styled("no turn yet", dim)),
        }
        if let Some(ttft) = state.otel.as_ref().and_then(|o| o.last_ttft_ms()) {
            l1.push(Span::styled(
                format!(" · ttft {}", fmt::short_ms(ttft)),
                dim,
            ));
        }
        if let Some(c) = &state.cost.authoritative {
            let retry = c
                .total_api_duration
                .saturating_sub(c.total_api_duration_without_retries);
            if retry > 0 {
                l1.push(Span::styled(
                    format!(" · retries {}", fmt::duration_ms(retry as i64)),
                    amber,
                ));
            }
        }

        // Line 2: what is running, or what the model waits on
        let mut l2 = vec![Span::raw(" ")];
        use crate::ui::state::WaitingKind;
        if let Some(w) = state.waiting() {
            let since = fmt::duration_ms(now - w.since_ms);
            let text = match w.kind {
                WaitingKind::Permission => format!("◆ waiting for permission for {since}"),
                WaitingKind::Question => format!("◆ waiting {since} · Claude asked a question"),
                WaitingKind::Notification => format!("◆ waiting {since} · needs your input"),
                WaitingKind::Asked => format!("◆ asked a question {since} ago · no reply yet"),
            };
            l2.push(Span::styled(text, amber));
        } else if let Some(c) = state.tools.running() {
            let el = c.started_at.map(|st| now - st).unwrap_or(0);
            let pid = state
                .procs
                .running_command
                .as_ref()
                .filter(|_| c.name == "Bash")
                .map(|rc| format!("  pid {}", rc.pid))
                .unwrap_or_default();
            l2.push(Span::styled(
                format!("● {} {}  ", c.name, fmt::duration_ms(el)),
                accent,
            ));
            l2.push(Span::raw(fmt::clip(&c.input_summary, 30)));
            l2.push(Span::styled(pid, dim));
        } else if state.session.alive && turn.is_some_and(|t| t.duration_ms.is_none()) {
            l2.push(Span::styled("● thinking", accent));
        } else {
            // Not the coach's IDLE (no turn running): only that no call is.
            l2.push(Span::styled("no call running", dim));
        }

        // Line 3: edits ✓ last check · steers · interrupts
        let mut l2b = vec![Span::raw(" ")];
        let edits = state.edits_this_turn();
        l2b.push(Span::raw(format!("edits {edits}")));
        match state.last_check() {
            Some((cmd, passed, at)) => {
                let mark = if passed { "✓" } else { "✗" };
                let style = if passed {
                    state.theme.ok()
                } else {
                    state.theme.crit()
                };
                l2b.push(Span::styled(format!(" {mark} {cmd}"), style));
                l2b.push(Span::styled(
                    format!(" {} ago", fmt::duration_ms(now - at)),
                    dim,
                ));
                let unchecked = state.edits_since_check();
                if unchecked > 0 {
                    l2b.push(Span::styled(format!(" · {unchecked} unchecked"), amber));
                }
            }
            None => l2b.push(Span::styled(" ✓ none", if edits > 0 { amber } else { dim })),
        }
        if let Some(t) = turn {
            if t.steers > 0 {
                l2b.push(Span::styled(format!(" · steers {}", t.steers), dim));
            }
            if !t.human {
                l2b.push(Span::styled(" · machine turn", dim));
            }
        }
        let (interrupts, cut) = state.interrupts_summary();
        if interrupts > 0 {
            l2b.push(Span::styled(
                format!(" · interrupts {interrupts} ({} out cut)", fmt::tokens(cut)),
                dim,
            ));
        }

        // Line 4: hooks by command · permission waits · queued · background · goal
        let mut l3 = vec![Span::raw(" ")];
        match turn {
            Some(t) if t.hook_runs > 0 => {
                l3.push(Span::raw(format!(
                    "hooks {} runs · {}ms",
                    t.hook_runs, t.hook_ms
                )));
                if let Some((cmd, ms)) = state
                    .agg
                    .hook_ms_by_command
                    .iter()
                    .max_by_key(|(_, ms)| **ms)
                {
                    l3.push(Span::styled(
                        format!(" ({} {}ms)", fmt::clip(cmd, 14), ms),
                        dim,
                    ));
                }
                if t.hook_errors > 0 {
                    l3.push(Span::styled(
                        format!(" ({} failed)", t.hook_errors),
                        state.theme.crit(),
                    ));
                }
                if t.hook_blocked {
                    l3.push(Span::styled(" · a hook blocked the stop", amber));
                }
            }
            _ => l3.push(Span::styled("hooks —", dim)),
        }
        l3.push(Span::styled("   ", dim));
        if state.session.hooks_installed {
            l3.push(Span::raw(format!(
                "permission waits {} · {}",
                state.session.permission_waits,
                fmt::duration_ms(state.session.permission_wait_ms)
            )));
        } else {
            l3.push(Span::styled("permission waits —", dim));
        }
        if state.agg.queued_prompts > 0 {
            l3.push(Span::styled(
                format!("   queued {}", state.agg.queued_prompts),
                amber,
            ));
        }
        if !state.session.background_tasks.is_empty() {
            l3.push(Span::styled(
                format!("   background {}", state.session.background_tasks.len()),
                accent,
            ));
        } else if let Some(n) = turn
            .and_then(|t| t.pending_background_agents)
            .filter(|n| *n > 0)
        {
            l3.push(Span::styled(format!("   background agents {n}"), accent));
        }
        if let Some(g) = &state.agg.goal {
            let mut s = format!("   goal {}", if g.met { "met" } else { "open" });
            if let (Some(i), Some(t)) = (g.iterations, g.tokens) {
                s.push_str(&format!(" · {i} it · {}", fmt::tokens(t)));
            }
            l3.push(Span::styled(
                s,
                if g.met { state.theme.ok() } else { amber },
            ));
        }

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(l1),
                Line::from(l2),
                Line::from(l2b),
                Line::from(l3),
            ]),
            inner,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::{parse_file, Line};
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
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app
    }

    fn fixture_b_app() -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
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
    fn turn_panel_on_fixture_b_shows_phase_check_and_interrupt() {
        let mut app = fixture_b_app();
        app.state.open = Some(4);
        let out = render_to_string(&app, 120, 70);
        // The spliced tail: two gitOperation commits end the call list.
        assert!(out.contains("COMMITTING ×"), "{out}");
        assert!(out.contains("edits 0 ✓ "), "{out}");
        assert!(out.contains("unchecked"), "{out}");
        assert!(out.contains("interrupts 1 ("), "{out}");
        assert!(out.contains("out cut)"), "{out}");
    }

    #[test]
    fn waiting_states_and_goal() {
        use crate::ui::state::WaitingKind;
        let mut app = fixture_b_app();
        app.state.open = Some(4);
        assert!(app.state.waiting().is_none());
        // A pending AskUserQuestion is the strongest signal.
        app.feed(Line::parse(r#"{"type":"assistant","timestamp":"2026-09-15T00:00:00Z","message":{"id":"q1","model":"claude-opus-5","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"q":"which?"}]}}],"usage":{"input_tokens":1}}}"#).unwrap());
        app.state.session.ended_at_ms = app.state.last_line_at_ms.map(|t| t + 240_000);
        let w = app.state.waiting().unwrap();
        assert_eq!(w.kind, WaitingKind::Question);
        let out = render_to_string(&app, 120, 70);
        assert!(
            out.contains("◆ waiting 4:00 · Claude asked a question"),
            "{out}"
        );
        assert!(out.contains("WAITING ×"), "{out}");
        // Answered: nothing pending; a turn that ended asking is "asked".
        app.feed(Line::parse(r#"{"type":"user","timestamp":"2026-09-15T00:01:00Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"b"}]},"toolUseResult":{"questions":[{}],"answers":{"q":"b"}}}"#).unwrap());
        app.feed(Line::parse(r#"{"type":"assistant","timestamp":"2026-09-15T00:01:05Z","message":{"id":"a2","model":"claude-opus-5","content":[{"type":"text","text":"Shall I continue?"}],"stop_reason":"end_turn","usage":{"input_tokens":1}}}"#).unwrap());
        app.feed(Line::parse(r#"{"type":"system","subtype":"turn_duration","durationMs":65000,"timestamp":"2026-09-15T00:01:05Z"}"#).unwrap());
        app.state.session.ended_at_ms = app.state.last_line_at_ms.map(|t| t + 30_000);
        assert_eq!(app.state.waiting().unwrap().kind, WaitingKind::Asked);
        // A goal_status attachment shows on the last row.
        app.feed(Line::parse(r#"{"type":"attachment","timestamp":"2026-09-15T00:01:06Z","attachment":{"type":"goal_status","met":false,"condition":"c","iterations":3,"durationMs":1000,"tokens":60858}}"#).unwrap());
        let out = render_to_string(&app, 120, 70);
        assert!(
            out.contains("◆ asked a question 0:30 ago · no reply yet"),
            "{out}"
        );
        assert!(out.contains("goal open · 3 it · 60k"), "{out}");
    }

    #[test]
    fn turn_panel_on_fixture() {
        let mut app = fixture_app();
        app.state.open = Some(4);
        let out = render_to_string(&app, 100, 60);
        // The session ended with an interrupt, which is not a turn: the
        // panel shows the last real turn (52 API calls, cut after 19:54).
        assert!(out.contains("4 Turn ─ 19:54"), "{out}");
        assert!(out.contains("turn elapsed 19:54 · api 52 calls"), "{out}");
        assert!(out.contains("no call running"), "{out}");
        assert!(out.contains("hooks —   permission waits —"), "{out}");
        // Retry time from the fixture's cost-state (2.9 s).
        assert!(out.contains("retries 0:02"), "{out}");
    }

    #[test]
    fn running_tool_hooks_and_permission_wait() {
        let mut app = fixture_app();
        app.state.open = Some(4);
        app.feed(Line::parse(r#"{"type":"user","timestamp":"2026-08-27T10:20:00Z","message":{"role":"user","content":"go"}}"#).unwrap());
        app.feed(Line::parse(r#"{"type":"assistant","timestamp":"2026-08-27T10:20:02Z","message":{"id":"mrun","model":"claude-sonnet-5","content":[{"type":"tool_use","id":"trun","name":"Bash","input":{"command":"cargo test --workspace"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        app.state.session.ended_at_ms = app.state.last_line_at_ms.map(|t| t + 48_000);
        app.state.procs.running_command = Some(crate::procs::RunningCommand {
            pid: 8812,
            cmdline: "cargo test --workspace".into(),
            elapsed_s: 48,
        });
        let out = render_to_string(&app, 64, 60);
        assert!(
            out.contains("phase WORKING ×1 · turn elapsed 0:50 · api 1 calls · api 0:02"),
            "{out}"
        );
        assert!(
            out.contains("● Bash 0:48  cargo test --workspace  pid 8812"),
            "{out}"
        );

        app.state.session.hooks_installed = true;
        app.state.session.permission_waits = 1;
        app.state.session.permission_wait_ms = 12_000;
        app.state.session.permission_pending = true;
        app.state.session.permission_waiting_since_ms =
            app.state.session.ended_at_ms.map(|t| t - 5_000);
        let out = render_to_string(&app, 64, 60);
        assert!(out.contains("◆ waiting for permission for 0:05"), "{out}");
        assert!(out.contains("permission waits 1 · 0:12"), "{out}");
        // Queued prompt.
        app.feed(Line::parse(r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-27T10:20:10Z"}"#).unwrap());
        assert!(render_to_string(&app, 64, 60).contains("queued 1"));
        // Turn 3 of the fixture had hooks: render with that turn current.
        let mut early = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        early.state = State::new(Pricing::bundled());
        let lines =
            parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl"))
                .unwrap();
        for l in lines {
            early.feed(l);
            if early.state.agg.turns.len() == 3 && early.state.agg.turns[2].hook_runs > 0 {
                break;
            }
        }
        early.state.session.ended_at_ms = early.state.last_line_at_ms;
        early.state.open = Some(4);
        let out = render_to_string(&early, 64, 60);
        assert!(out.contains("hooks 1 runs · 60ms"), "{out}");
    }
}
