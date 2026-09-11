//! Header: who we are attached to and what it is doing right now.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Panel, PanelId};
use crate::ui::state::{SessionStatus, State};

pub struct Header;

impl Header {
    fn status_pill(state: &State) -> Span<'static> {
        let s = &state.session;
        let amber = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        if state.paused {
            return Span::styled("⏸ PAUSED", amber);
        }
        if !s.alive {
            let at = s.ended_at_ms.map(fmt::clock_hhmm).unwrap_or_default();
            return Span::styled(
                format!("■ ENDED {at}").trim_end().to_string(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if s.permission_pending {
            return Span::styled("◆ WAITING", amber);
        }
        match s.status {
            SessionStatus::Busy => Span::styled(
                "● BUSY",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            SessionStatus::Idle => Span::styled("○ IDLE", Style::default().fg(Color::DarkGray)),
            SessionStatus::Unknown => Span::styled("○ ?", Style::default().fg(Color::DarkGray)),
        }
    }
}

impl Panel for Header {
    fn id(&self) -> PanelId {
        0
    }
    fn title(&self) -> String {
        "cctop".to_string()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let model = state.model().unwrap_or("—");
        let v = if state.session.version.is_empty() {
            String::new()
        } else {
            format!(" · v{}", state.session.version)
        };
        let name = if state.session.name.is_empty() {
            String::new()
        } else {
            format!("{} ── ", state.session.name)
        };
        Some(format!("{name}{model}{v}"))
    }
    fn min_rows(&self) -> u16 {
        2
    }
    fn priority(&self) -> u8 {
        255
    }
    fn placement(&self) -> Placement {
        Placement::Top
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = Style::default().fg(Color::DarkGray);
        let accent = Style::default().fg(Color::Cyan);
        let s = &state.session;
        let now = if s.alive {
            state.now_ms
        } else {
            s.ended_at_ms.unwrap_or(state.now_ms)
        };

        // Line 1: status · turn · elapsed · running tool · cwd · branch
        let mut l1: Vec<Span> = vec![Span::raw(" "), Self::status_pill(state), Span::raw("  ")];
        let turn = state.agg.turns.len();
        l1.push(Span::raw(format!("turn {turn}")));
        if let Some(t) = state.agg.current_turn() {
            if let Some(el) = t.elapsed_ms(now) {
                l1.push(Span::raw(format!("  {}", fmt::duration_ms(el))));
            }
        }
        if let Some(c) = state.tools.running() {
            let el = c.started_at.map(|st| now - st).unwrap_or(0);
            l1.push(Span::styled(
                format!("  ▶ {} {}", c.name, fmt::duration_ms(el)),
                accent,
            ));
        } else if let Some(rc) = &state.procs.running_command {
            l1.push(Span::styled(
                format!(
                    "  ▶ {} {}",
                    fmt::clip(&rc.cmdline, 20),
                    fmt::duration_ms(rc.elapsed_s as i64 * 1000)
                ),
                accent,
            ));
        }
        if !s.cwd.as_os_str().is_empty() {
            l1.push(Span::raw(format!("  {}", fmt::shorten_home(&s.cwd))));
        }
        if let Some(b) = &s.git_branch {
            l1.push(Span::raw(format!(" {b}")));
            if s.git_dirty {
                l1.push(Span::styled("*", Style::default().fg(Color::Yellow)));
            }
        }

        // Line 2: permission mode · effort · plan · uptime · cost · cpu/rss
        let mut parts: Vec<Span> = Vec::new();
        parts.push(Span::raw(
            s.permission_mode.clone().unwrap_or_else(|| "—".into()),
        ));
        let effort = state
            .agg
            .turns
            .iter()
            .rev()
            .find_map(|t| t.effort.clone())
            .unwrap_or_else(|| "—".into());
        parts.push(Span::raw(effort));
        parts.push(Span::raw(s.plan.clone().unwrap_or_else(|| "—".into())));
        if let Some(st) = s.started_at_ms {
            parts.push(Span::raw(format!("up {}", fmt::duration_ms(now - st))));
        }
        match state.cost.current() {
            Some(c) => {
                let approx = if c.approx { "≈ API " } else { "" };
                parts.push(Span::raw(format!("{approx}{}", fmt::usd(c.usd))));
            }
            None => parts.push(Span::styled("cost —", dim)),
        }
        match (s.cpu_pct, s.rss_bytes) {
            (Some(cpu), Some(rss)) => {
                parts.push(Span::raw(format!("{cpu:.0}% {}", fmt::bytes(rss))))
            }
            _ => parts.push(Span::styled("cpu —", dim)),
        }
        let mut l2: Vec<Span> = vec![Span::raw(" ")];
        for (i, p) in parts.into_iter().enumerate() {
            if i > 0 {
                l2.push(Span::styled(" · ", dim));
            }
            l2.push(p);
        }

        frame.render_widget(Paragraph::new(vec![Line::from(l1), Line::from(l2)]), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{SessionInfo, SessionStatus, State};
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
    fn header_shows_fixture_session_values() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 30);
        let l0 = out.lines().next().unwrap();
        assert!(l0.contains("cctop"), "{out}");
        assert!(
            l0.contains("session-a ── claude-sonnet-5 · v2.1.247"),
            "{out}"
        );
        let l1 = out.lines().nth(1).unwrap();
        assert!(l1.contains("■ ENDED 10:17"), "{out}");
        assert!(l1.contains("turn 15"), "{out}");
        assert!(l1.contains("/home/user/project"), "{out}");
        let l2 = out.lines().nth(2).unwrap();
        assert!(l2.contains("auto · medium · — · $9.90 · cpu —"), "{out}");
    }

    #[test]
    fn header_live_states() {
        let mut app = fixture_app();
        app.state.session.alive = true;
        app.state.session.status = SessionStatus::Busy;
        app.state.session.started_at_ms = app.state.last_line_at_ms.map(|t| t - 4_320_000);
        app.state.session.git_branch = Some("main".into());
        app.state.session.git_dirty = true;
        app.state.session.plan = Some("Max".into());
        app.state.session.cpu_pct = Some(3.1);
        app.state.session.rss_bytes = Some(412 << 20);
        app.state.now_ms = app.state.last_line_at_ms.unwrap();
        let out = render_to_string(&app, 70, 30);
        assert!(out.contains("● BUSY"), "{out}");
        assert!(out.contains("/home/user/project main*"), "{out}");
        assert!(out.contains("Max · up 1h 12m · $9.90 · 3% 412 MB"), "{out}");

        app.state.session.permission_pending = true;
        assert!(render_to_string(&app, 70, 30).contains("◆ WAITING"));
        app.state.session.status = SessionStatus::Idle;
        app.state.session.permission_pending = false;
        assert!(render_to_string(&app, 70, 30).contains("○ IDLE"));
        app.state.paused = true;
        assert!(render_to_string(&app, 70, 30).contains("⏸ PAUSED"));
    }

    #[test]
    fn ended_session_freezes_clock_and_keeps_rendering() {
        let mut app = fixture_app();
        app.state.session.alive = false;
        let ended = app.state.session.ended_at_ms.unwrap();
        app.state.now_ms = ended + 3_600_000;
        let a = render_to_string(&app, 60, 30);
        app.state.now_ms = ended + 7_200_000;
        let b = render_to_string(&app, 60, 30);
        assert_eq!(a, b, "values must not move once the session ended");
        assert!(a.contains("ENDED"));
    }
}
