//! Tools: the process table. One row per tool name, sortable and filterable,
//! with a detail view of recent calls.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tools::{Call, ToolStats};
use crate::ui::fmt;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::{State, ToolSort};

pub struct Tools;

impl Tools {
    /// Sorted, filtered rows.
    pub fn rows(state: &State) -> Vec<ToolStats> {
        let ui = &state.tools_ui;
        let mut rows: Vec<ToolStats> = state
            .tools
            .by_name_and_class()
            .into_values()
            .filter(|t| match &ui.filter {
                Some(f) if !f.is_empty() => t.name.to_lowercase().contains(&f.to_lowercase()),
                _ => true,
            })
            .collect();
        rows.sort_by(|a, b| {
            let ord = match ui.sort {
                ToolSort::Calls => a.calls.cmp(&b.calls),
                ToolSort::Errors => a.errors.cmp(&b.errors),
                ToolSort::P50 => a.p50_ms.cmp(&b.p50_ms),
                ToolSort::P95 => a.p95_ms.cmp(&b.p95_ms),
                ToolSort::Last => a.last_call_at.cmp(&b.last_call_at),
                ToolSort::Tokens => a.tokens_to_ctx.cmp(&b.tokens_to_ctx),
                ToolSort::Name => b.name.cmp(&a.name),
            };
            if ui.ascending { ord } else { ord.reverse() }.then_with(|| a.name.cmp(&b.name))
        });
        rows
    }

    fn selected_name(state: &State) -> Option<String> {
        Self::rows(state)
            .get(state.tools_ui.selected)
            .map(|r| r.name.clone())
    }

    fn dur(ms: Option<u64>, approx: bool) -> String {
        match ms {
            Some(m) => format!("{}{}", fmt::short_ms(m), if approx { "≈" } else { "" }),
            None => "—".into(),
        }
    }
}

const HEADER: &str = " TOOL          N  ERR    p50    p95   LAST  TOKENS→CTX  IN→CTX";

impl Panel for Tools {
    fn id(&self) -> PanelId {
        5
    }
    fn title(&self) -> String {
        "Tools".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let ui = &state.tools_ui;
        let tail = match &ui.filter {
            Some(f) => format!(" · f:{f}{}", if ui.editing { "▏" } else { "" }),
            None if ui.sort != ToolSort::Calls || ui.ascending => {
                format!(
                    " · ↕{}{}",
                    ui.sort.label(),
                    if ui.ascending { "↑" } else { "↓" }
                )
            }
            None => String::new(),
        };
        Some(format!("{} calls{tail}", state.tools.calls.len()))
    }
    fn captures_input(&self, state: &State) -> bool {
        // While a filter exists, Esc must reach the panel (to clear it)
        // before the global Esc drops focus.
        state.tools_ui.editing || state.tools_ui.filter.is_some()
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        let n = Self::rows(state).len();
        let in_overlay = state.overlay == Some(self.id());
        let ui = &mut state.tools_ui;
        if ui.editing {
            match key.code {
                KeyCode::Esc => {
                    ui.editing = false;
                    ui.filter = None;
                }
                KeyCode::Enter => ui.editing = false,
                KeyCode::Backspace => {
                    if let Some(f) = ui.filter.as_mut() {
                        f.pop();
                    }
                }
                KeyCode::Char(c) => ui.filter.get_or_insert_with(String::new).push(c),
                _ => {}
            }
            ui.selected = 0;
            return Handled::Yes;
        }
        match key.code {
            KeyCode::Char('s') => {
                ui.sort = ui.sort.next();
                ui.ascending = false;
            }
            KeyCode::Char('S') => ui.ascending = !ui.ascending,
            KeyCode::Char('f') => {
                ui.editing = true;
                ui.filter.get_or_insert_with(String::new);
            }
            // In the detail view Esc closes the view; the filter survives.
            KeyCode::Esc if ui.filter.is_some() && !in_overlay => ui.filter = None,
            KeyCode::Char('j') | KeyCode::Down => {
                ui.selected = (ui.selected + 1).min(n.saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => ui.selected = ui.selected.saturating_sub(1),
            KeyCode::Enter => {
                if n > 0 {
                    state.overlay = Some(self.id());
                }
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let accent = state.theme.accent();
        let focused = state.overlay == Some(self.id());
        let ui = &state.tools_ui;
        let now = state.clock_ms();
        let rows = Self::rows(state);

        let mut lines: Vec<Line> = Vec::new();
        let header = HEADER.to_string();
        let mut hdr = vec![Span::styled(header, dim)];
        if let Some(f) = &ui.filter {
            let cursor = if ui.editing { "▏" } else { "" };
            hdr.push(Span::styled(format!("  f:{f}{cursor}"), accent));
        } else {
            hdr.push(Span::styled(
                format!(
                    "  ↕{}{}",
                    ui.sort.label(),
                    if ui.ascending { "↑" } else { "↓" }
                ),
                dim,
            ));
        }
        lines.push(Line::from(hdr));

        let body_rows = inner.height.saturating_sub(2) as usize; // header + pinned lines
        let first = ui.selected.saturating_sub(body_rows.saturating_sub(1));
        for (i, t) in rows.iter().enumerate().skip(first).take(body_rows) {
            let last = if t.running > 0 {
                "▶now".to_string()
            } else {
                t.last_call_at
                    .map(|at| fmt::duration_ms(now - at))
                    .unwrap_or_else(|| "—".into())
            };
            let err_style = match t.errors {
                0 => Style::default(),
                1..=2 => state.theme.warn(),
                _ => state.theme.crit(),
            };
            let cut = if t.truncated > 0 { "⊘" } else { " " };
            let mut spans = vec![
                Span::raw(format!(" {:<12}{:>3}  ", fmt::clip(&t.name, 12), t.calls)),
                Span::styled(format!("{:>3}", t.errors), err_style),
                Span::raw(format!(
                    "  {:>6} {:>6}  {:>5}  {:>8}{cut} {:>7}",
                    Self::dur(t.p50_ms, t.approx),
                    Self::dur(t.p95_ms, t.approx),
                    last,
                    fmt::tokens(t.tokens_to_ctx),
                    fmt::tokens(t.input_tokens)
                )),
            ];
            if last == "▶now" {
                spans[2] = Span::styled(spans[2].content.to_string(), accent);
            }
            let mut line = Line::from(spans);
            if focused && i == ui.selected {
                line = line.style(Style::default().add_modifier(Modifier::REVERSED));
            }
            lines.push(line);
        }

        let top: Vec<String> = state
            .tools
            .top_ctx(3)
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut s = format!(
                    "{} {} {}{}",
                    c.name,
                    fmt::clip(&c.input_summary, 30),
                    fmt::tokens(c.result_tokens_est),
                    if c.truncated { "⊘" } else { "" }
                );
                // The re-read tax on the largest result: every later call
                // re-reads it.
                if i == 0 {
                    if let Some((n, usd)) = state.reread_tax(c) {
                        if n > 0 {
                            s.push_str(&format!(" (re-read ×{n} ≈{})", fmt::usd(usd)));
                        }
                    }
                }
                s
            })
            .collect();
        let top_line = Line::from(vec![
            Span::styled(" top ctx: ", dim),
            Span::raw(fmt::clip(
                &top.join(" · "),
                inner.width.saturating_sub(11) as usize,
            )),
        ]);
        // Errors by Claude Code's class, when there are any.
        let errors = state.tools.errors_by_class();
        let err_line = (!errors.is_empty()).then(|| {
            let parts: Vec<String> = errors
                .iter()
                .take(4)
                .map(|(k, n)| format!("{} {n}", k.label()))
                .collect();
            Line::from(vec![
                Span::styled(" errors: ", dim),
                Span::styled(
                    fmt::clip(&parts.join(" · "), inner.width.saturating_sub(10) as usize),
                    state.theme.warn(),
                ),
            ])
        });
        // Pin the summary lines to the last rows.
        let pinned = 1 + err_line.is_some() as usize;
        while lines.len() + pinned < inner.height as usize {
            lines.push(Line::from(""));
        }
        lines.truncate(inner.height.saturating_sub(pinned as u16) as usize);
        if let Some(e) = err_line {
            lines.push(e);
        }
        lines.push(top_line);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn has_overlay(&self) -> bool {
        true
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, state: &State) {
        let Some(name) = Self::selected_name(state) else {
            return;
        };
        let now = state.clock_ms();
        // `Bash·test` rows select the Bash calls of that class.
        let (tool, class) = match name.split_once('·') {
            Some((t, c)) => (t.to_string(), Some(c.to_string())),
            None => (name.clone(), None),
        };
        let calls: Vec<&Call> = state
            .tools
            .calls
            .iter()
            .rev()
            .filter(|c| c.name == tool)
            .filter(|c| {
                class
                    .as_deref()
                    .is_none_or(|k| c.bash_class.is_some_and(|b| b.label() == k))
            })
            .take(20)
            .collect();
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {name} — last {} calls  (Esc back) ", calls.len()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let dim = state.theme.dim();
        let mut lines = vec![Line::from(Span::styled(
            format!(
                " {:<8} {:<32} {:>8} {:>8}  {}",
                "AGO", "INPUT", "DUR", "TOKENS", "ERR"
            ),
            dim,
        ))];
        for c in calls {
            let ago = c
                .started_at
                .map(|s| fmt::duration_ms(now - s))
                .unwrap_or_else(|| "—".into());
            let dur = if c.is_running() {
                "▶now".to_string()
            } else {
                Self::dur(c.duration_ms, c.approx_duration)
            };
            let input = match &c.mcp_tool {
                Some(t) => format!("{t}: {}", c.input_summary),
                None => c.input_summary.clone(),
            };
            let err = if c.is_error { "✗" } else { "" };
            let mut line = Line::from(format!(
                " {:<8} {:<32} {:>8} {:>8}  {}",
                ago,
                fmt::clip(&input, 32),
                dur,
                fmt::tokens(c.result_tokens_est),
                err
            ));
            if c.is_error {
                line = line.style(state.theme.crit());
            }
            lines.push(line);
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{SessionInfo, State, ToolSort};
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
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn tools_panel_on_fixture() {
        let mut app = fixture_app();
        app.state.open = Some(5);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("5 Tools ─ 257 calls"), "{out}");
        assert!(
            out.contains("TOOL          N  ERR    p50    p95   LAST  TOKENS→CTX"),
            "{out}"
        );
        // Sorted by calls desc: the chrome MCP server (214) first, RemoteTrigger
        // (16), then Bash split by class — every fixture-A command is
        // `make check`, a test.
        let chrome = out
            .lines()
            .position(|l| l.contains("mcp:claude-…214"))
            .expect("chrome row");
        let bash = out
            .lines()
            .position(|l| l.contains("Bash·test    15"))
            .expect("bash row");
        assert!(chrome < bash, "{out}");
        assert!(out.contains("≈"), "durations are approximate: {out}");
        assert!(out.contains("top ctx: "), "{out}");
        let wide = render_to_string(&app, 120, 70);
        assert!(wide.contains("IN→CTX"), "{wide}");
        assert!(
            wide.contains("errors: Other 4 · Denied 1"),
            "anonymised texts: {wide}"
        );
        assert!(wide.contains("(re-read ×"), "{wide}");
    }

    #[test]
    fn sort_filter_and_detail_overlay() {
        let mut app = fixture_app();
        app.state.open = Some(5);
        app.handle_key(key('s'));
        assert_eq!(app.state.tools_ui.sort, ToolSort::Errors);
        app.handle_key(key('S'));
        assert!(app.state.tools_ui.ascending);
        // Filter to Bash only.
        app.handle_key(key('f'));
        assert!(app.state.tools_ui.editing);
        for c in "bash".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.state.tools_ui.editing);
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("f:bash"), "{out}");
        assert!(out.contains("Bash·test    15"), "{out}");
        assert!(!out.contains("RemoteTrigg"), "{out}");
        // Enter opens the detail overlay for the selected (only) row.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(5));
        let out = render_to_string(&app, 80, 30);
        assert!(out.contains("Bash·test — last 15 calls"), "{out}");
        assert!(out.contains("make check"), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
        // Esc with a filter active clears it.
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.tools_ui.filter, None);
        assert!(render_to_string(&app, 60, 51).contains("RemoteTrigg"));
    }

    #[test]
    fn running_tool_shows_now() {
        let mut app = fixture_app();
        app.state.open = Some(5);
        app.feed(crate::transcript::Line::parse(r#"{"type":"assistant","timestamp":"2026-08-27T10:20:00Z","message":{"id":"mrun","model":"claude-sonnet-5","content":[{"type":"tool_use","id":"trun","name":"Bash","input":{"command":"sleep 99"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("▶now"), "{out}");
    }
}
