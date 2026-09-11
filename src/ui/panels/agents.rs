//! Agents & MCP: subagents, MCP server processes, background tasks.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::agents::State as AgentState;
use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Panel, PanelId};
use crate::ui::state::State;

pub struct Agents;

/// No calls for longer than this counts as idle.
const IDLE_MS: i64 = 5 * 60 * 1000;

impl Panel for Agents {
    fn id(&self) -> PanelId {
        6
    }
    fn title(&self) -> String {
        "Agents & MCP".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let now = state.clock_ms();
        let running = state
            .agents
            .values()
            .filter(|a| a.state(now) == AgentState::Running)
            .count();
        let mut parts = Vec::new();
        if !state.agents.is_empty() {
            parts.push(format!("{running}/{} agents", state.agents.len()));
        }
        if !state.procs.mcp.is_empty() {
            parts.push(format!("{} mcp", state.procs.mcp.len()));
        }
        if !state.tasks.is_empty() {
            parts.push(format!("{} bg", state.tasks.len()));
        }
        Some(if parts.is_empty() {
            "—".into()
        } else {
            parts.join(" · ")
        })
    }
    fn min_rows(&self) -> u16 {
        5
    }
    fn priority(&self) -> u8 {
        50
    }
    fn placement(&self) -> Placement {
        Placement::Right
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let accent = state.theme.accent();
        let now = state.clock_ms();
        let mut lines: Vec<Line> = Vec::new();

        // Subagents, newest first.
        let mut agents: Vec<_> = state.agents.values().collect();
        agents.sort_by_key(|a| std::cmp::Reverse(a.started_at));
        for a in agents {
            let (glyph, style) = match a.state(now) {
                AgentState::Running => ("◐", accent),
                AgentState::Done => ("✓", state.theme.ok()),
                AgentState::Failed => ("✗", state.theme.crit()),
            };
            let elapsed = a.elapsed_ms(now).map(fmt::duration_ms).unwrap_or_default();
            let model = a
                .model
                .trim_start_matches("claude-")
                .split('-')
                .next()
                .unwrap_or("")
                .to_string();
            lines.push(Line::from(vec![
                Span::styled(format!(" {glyph} "), style),
                Span::raw(format!("{:<8} ", fmt::clip(&a.agent_type, 8))),
                Span::styled(format!("{:<28} ", fmt::clip(&a.description, 28)), dim),
                Span::raw(format!(
                    "{:>5} {:>5} {}",
                    elapsed,
                    fmt::tokens(a.usage.total()),
                    model
                )),
            ]));
        }

        // MCP servers.
        let stats = state.tools.by_name();
        for m in &state.procs.mcp {
            let key = format!("mcp:{}", m.name);
            let (calls, p95, last) = stats
                .get(&key)
                .map(|s| (s.calls, s.p95_ms, s.last_call_at))
                .unwrap_or((0, None, None));
            let tail = match last {
                Some(at) if now - at <= IDLE_MS => match p95 {
                    Some(p) => format!("p95 {}", fmt::short_ms(p)),
                    None => String::new(),
                },
                Some(at) => format!("idle {}", fmt::duration_ms(now - at)),
                None => format!("idle {}", fmt::duration_ms(m.elapsed_s as i64 * 1000)),
            };
            let restarts = if m.restarts > 0 {
                format!(" ↻{}", m.restarts)
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::raw(format!(" mcp {:<12} ", fmt::clip(&m.name, 12))),
                Span::styled(
                    format!(
                        "pid {:<6} {:>7}  {:>2} calls  {tail}{restarts}",
                        m.pid,
                        fmt::bytes(m.rss_bytes),
                        calls
                    ),
                    if tail.starts_with("idle") {
                        dim
                    } else {
                        Style::default()
                    },
                ),
            ]));
        }
        for name in &state.procs.mcp_missing {
            if state.mcp_exited.contains(name) {
                lines.push(Line::from(Span::styled(
                    format!(" mcp {:<12} exited", fmt::clip(name, 12)),
                    state.theme.crit(),
                )));
            }
        }

        // Background tasks.
        for t in &state.tasks {
            let elapsed = fmt::duration_ms(now - t.started_at_ms);
            let status = t
                .status
                .as_deref()
                .map(|s| format!(" {s}"))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::raw(format!(" bg  {:<6} ", fmt::clip(&t.kind, 6))),
                Span::styled(format!("{:<28} ", fmt::clip(&t.description, 28)), dim),
                Span::raw(format!(
                    "{:>5}  task {}{status}",
                    elapsed,
                    fmt::clip(&t.id, 8)
                )),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                " no subagents, MCP servers or background tasks",
                dim,
            )));
        }
        lines.truncate(inner.height as usize);
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::procs::{McpServer, Snapshot};
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
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app.state.agents = crate::agents::load(&path.with_extension(""));
        app
    }

    #[test]
    fn agents_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 60);
        assert!(out.contains("6 Agents & MCP ─ 0/1 agents"), "{out}");
        assert!(
            out.contains("✓ fork     Check whether a setup step …"),
            "{out}"
        );
        assert!(out.contains("sonnet"), "{out}");
    }

    #[test]
    fn mcp_rows_show_calls_idle_and_exit() {
        let mut app = fixture_app();
        let now = app.state.clock_ms();
        app.state.procs = Snapshot {
            mcp: vec![
                McpServer {
                    name: "claude-in-chrome".into(),
                    pid: 8231,
                    rss_bytes: 41 << 20,
                    cpu_pct: 0.0,
                    elapsed_s: 900,
                    restarts: 0,
                },
                McpServer {
                    name: "playwright".into(),
                    pid: 8244,
                    rss_bytes: 188 << 20,
                    cpu_pct: 0.0,
                    elapsed_s: 720,
                    restarts: 2,
                },
            ],
            mcp_missing: vec!["github".into()],
            ..Default::default()
        };
        app.state.mcp_exited = vec!["github".into()];
        app.state.tasks = vec![crate::tasks::Task {
            id: "b7f3a9".into(),
            kind: "bash".into(),
            description: "cargo build --release".into(),
            started_at_ms: now - 118_000,
            status: Some("running".into()),
        }];
        let out = render_to_string(&app, 72, 60);
        assert!(
            out.contains("mcp claude-in-c… pid 8231     41 MB  214 calls  idle 9:25"),
            "{out}"
        );
        assert!(
            out.contains("mcp playwright   pid 8244    188 MB   0 calls  idle 12:00 ↻2"),
            "{out}"
        );
        assert!(out.contains("mcp github       exited"), "{out}");
        assert!(
            out.contains("bg  bash   cargo build --release         1:58  task b7f3a9 running"),
            "{out}"
        );
        // Within 5 min of the last call the row shows p95 instead of idle.
        app.state.session.ended_at_ms = app.state.tools.by_name()["mcp:claude-in-chrome"]
            .last_call_at
            .map(|t| t + 1000);
        let out = render_to_string(&app, 72, 60);
        assert!(out.contains("214 calls  p95"), "{out}");
    }
}
