//! Full-height turn ledger overlay, opened from the Context or Tokens panel.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::ledger::{self, Sort};
use crate::ui::fmt;
use crate::ui::panel::Handled;
use crate::ui::State;

/// The panel id that owns the ledger overlay (Context).
pub const OWNER: u8 = 1;

#[derive(Debug, Clone, Default)]
pub struct LedgerUi {
    pub sort: Sort,
    pub ascending: bool,
    pub selected: usize,
    /// Showing the selected turn's tool calls.
    pub detail: bool,
}

pub fn open(state: &mut State) {
    state.overlay = Some(OWNER);
    state.ledger_ui.detail = false;
}

/// Keys while the ledger is open. `Handled::No` on Esc closes it (App).
pub fn handle_key(key: KeyEvent, state: &mut State) -> Handled {
    let n = state.agg.turns.len();
    let ui = &mut state.ledger_ui;
    match key.code {
        KeyCode::Esc if ui.detail => ui.detail = false,
        KeyCode::Enter if !ui.detail && n > 0 => ui.detail = true,
        KeyCode::Char('s') => {
            ui.sort = ui.sort.next();
            ui.ascending = false;
            ui.selected = 0;
        }
        KeyCode::Char('S') => ui.ascending = !ui.ascending,
        KeyCode::Char('j') | KeyCode::Down => {
            ui.selected = (ui.selected + 1).min(n.saturating_sub(1))
        }
        KeyCode::Char('k') | KeyCode::Up => ui.selected = ui.selected.saturating_sub(1),
        KeyCode::Char('g') => ui.selected = 0,
        KeyCode::Char('G') => ui.selected = n.saturating_sub(1),
        _ => return Handled::No,
    }
    Handled::Yes
}

fn clock(ms: Option<i64>) -> String {
    match ms {
        Some(at) => {
            let s = at.div_euclid(1000).rem_euclid(86_400);
            format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
        }
        None => "—".into(),
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let ui = &state.ledger_ui;
    let rows = ledger::sorted(state, ui.sort, ui.ascending);
    if ui.detail {
        return render_detail(frame, area, state, rows.get(ui.selected).map(|r| r.turn));
    }
    let dim = Style::default().fg(Color::DarkGray);
    let title = format!(
        " Turn ledger — {} turns · ↕{}{}  (s/S sort, j/k, Enter calls, Esc back) ",
        rows.len(),
        ui.sort.label(),
        if ui.ascending { "↑" } else { "↓" }
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {:>3} {:<8} {:>6} {:>4} {:>7} {:>7} {:>6} {:>6} {:>6} {:>7} {:<6} {:<8} {}",
            "#",
            "START",
            "DUR",
            "API",
            "READ",
            "WRITE",
            "FRESH",
            "OUT",
            "THINK",
            "COST",
            "EFFORT",
            "MODEL",
            "TOOLS"
        ),
        dim,
    ))];
    let body = inner.height.saturating_sub(1) as usize;
    let first = ui.selected.saturating_sub(body.saturating_sub(1));
    for (i, r) in rows.iter().enumerate().skip(first).take(body) {
        let model = r.model.trim_start_matches("claude-").to_string();
        let mut line = Line::from(format!(
            " {:>2}{} {:<8} {:>6} {:>4} {:>7} {:>7} {:>6} {:>6} {:>6} {:>7} {:<6} {:<8} {}",
            r.turn,
            if r.compaction { "C" } else { " " },
            clock(r.started_at_ms),
            r.duration_ms
                .map(fmt::duration_ms)
                .unwrap_or_else(|| "—".into()),
            r.api_calls,
            fmt::tokens(r.usage.cache_read),
            fmt::tokens(r.usage.cache_write()),
            fmt::tokens(r.usage.input),
            fmt::tokens(r.usage.output),
            fmt::tokens(r.usage.thinking),
            r.cost_usd.map(fmt::usd).unwrap_or_else(|| "—".into()),
            fmt::clip(&r.effort, 6),
            fmt::clip(&model, 8),
            r.tools
        ));
        if i == ui.selected {
            line = line.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_detail(frame: &mut Frame, area: Rect, state: &State, turn: Option<usize>) {
    let Some(turn) = turn else { return };
    let calls = ledger::calls_of(state, turn);
    let block = Block::default().borders(Borders::ALL).title(format!(
        " Turn {turn} — {} tool calls  (Esc back) ",
        calls.len()
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {:<16} {:<34} {:>8} {:>10}  ERR",
            "TOOL", "INPUT", "DUR", "TOKENS→CTX"
        ),
        dim,
    ))];
    for c in calls.iter().take(inner.height.saturating_sub(1) as usize) {
        let dur = c
            .duration_ms
            .map(|d| {
                format!(
                    "{}{}",
                    fmt::short_ms(d),
                    if c.approx_duration { "≈" } else { "" }
                )
            })
            .unwrap_or_else(|| "▶".into());
        let mut line = Line::from(format!(
            " {:<16} {:<34} {:>8} {:>10}  {}",
            fmt::clip(&c.name, 16),
            fmt::clip(&c.input_summary, 34),
            dur,
            fmt::tokens(c.result_tokens_est),
            if c.is_error { "✗" } else { "" }
        ));
        if c.is_error {
            line = line.style(Style::default().fg(Color::Red));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
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
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn enter_on_context_or_tokens_opens_ledger_with_fixture_rows() {
        for panel in [1u8, 2] {
            let mut app = fixture_app();
            app.state.focused = Some(panel);
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(app.state.overlay, Some(1), "panel {panel}");
            let out = render_to_string(&app, 120, 30);
            assert!(out.contains("Turn ledger — 15 turns"), "{out}");
            assert!(out.contains("START"), "{out}");
            // Turn 3: 95.8 s, 8 API calls, medium effort, sonnet.
            let t3 = out
                .lines()
                .find(|l| l.contains("  3  "))
                .expect("turn 3 row");
            assert!(t3.contains("1:35"), "{t3}");
            assert!(t3.contains("   8 "), "{t3}");
            assert!(t3.contains("medium"), "{t3}");
            assert!(t3.contains("sonnet-5"), "{t3}");
            assert!(t3.contains("×"), "{t3}");
        }
    }

    #[test]
    fn sort_select_detail_and_close() {
        let mut app = fixture_app();
        app.state.focused = Some(1);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // Sort by cost desc: the most expensive turn first.
        for _ in 0..7 {
            app.handle_key(key('s'));
        }
        assert_eq!(app.state.ledger_ui.sort, crate::ledger::Sort::Cost);
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("↕COST↓"), "{out}");
        // Select the top row and open its calls.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.state.ledger_ui.detail);
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("tool calls"), "{out}");
        assert!(out.contains("TOKENS→CTX"), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.state.ledger_ui.detail);
        assert_eq!(
            app.state.overlay,
            Some(1),
            "first Esc leaves the detail only"
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
    }
}
