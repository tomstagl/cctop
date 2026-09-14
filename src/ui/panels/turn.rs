//! Turn: what the current turn is doing and waiting on.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::layout::Placement;
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
    fn min_rows(&self) -> u16 {
        3
    }
    fn priority(&self) -> u8 {
        60
    }
    fn placement(&self) -> Placement {
        Placement::Left
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let accent = state.theme.accent();
        let amber = state.theme.warn();
        let now = state.clock_ms();
        let turn = state.agg.current_turn();

        // Line 1: elapsed · api calls · api vs tool time · retries
        let mut l1 = vec![Span::raw(" ")];
        match turn {
            Some(t) => {
                let el = t
                    .elapsed_ms(now)
                    .map(fmt::duration_ms)
                    .unwrap_or_else(|| "—".into());
                l1.push(Span::raw(format!(
                    "elapsed {el}   api {} calls",
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

        // Line 2: what is running
        let mut l2 = vec![Span::raw(" ")];
        if state.session.permission_pending {
            let since = state
                .session
                .permission_waiting_since_ms
                .map(|s| format!(" for {}", fmt::duration_ms(now - s)))
                .unwrap_or_default();
            l2.push(Span::styled(
                format!("◆ waiting for permission{since}"),
                amber,
            ));
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
            l2.push(Span::styled("idle", dim));
        }

        // Line 3: hooks · permission waits · queued
        let mut l3 = vec![Span::raw(" ")];
        match turn {
            Some(t) if t.hook_runs > 0 => {
                l3.push(Span::raw(format!(
                    "hooks {} runs · {}ms",
                    t.hook_runs, t.hook_ms
                )));
                if t.hook_errors > 0 {
                    l3.push(Span::styled(
                        format!(" ({} failed)", t.hook_errors),
                        state.theme.crit(),
                    ));
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

        frame.render_widget(
            Paragraph::new(vec![Line::from(l1), Line::from(l2), Line::from(l3)]),
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

    #[test]
    fn turn_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 80, 60);
        // The session ended with an interrupt, which is not a turn: the
        // panel shows the last real turn (52 API calls, cut after 19:54).
        assert!(out.contains("4 Turn ─ 19:54"), "{out}");
        assert!(out.contains("elapsed 19:54   api 52 calls"), "{out}");
        assert!(out.contains("idle"), "{out}");
        assert!(out.contains("hooks —   permission waits —"), "{out}");
        // Retry time from the fixture's cost-state (2.9 s).
        assert!(out.contains("retries 0:02"), "{out}");
    }

    #[test]
    fn running_tool_hooks_and_permission_wait() {
        let mut app = fixture_app();
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
            out.contains("elapsed 0:50   api 1 calls · api 0:02"),
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
        let out = render_to_string(&early, 64, 60);
        assert!(out.contains("hooks 1 runs · 60ms"), "{out}");
    }
}
