//! Events: the newest-last log, with a full-height searchable view.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::events::{Event, Kind};
use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::State;

pub struct Events;

fn kind_style(k: Kind) -> Style {
    let c = match k {
        Kind::Tool => Color::Green,
        Kind::Hook | Kind::Agent => Color::Cyan,
        Kind::Perm | Kind::Note | Kind::Compact | Kind::Away => Color::Yellow,
        Kind::Api => Color::Red,
    };
    Style::default().fg(c)
}

fn clock(at: i64) -> String {
    let s = at.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn line_for(e: &Event, width: usize, highlight: Option<&str>) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray);
    let text = fmt::clip(&e.text, width.saturating_sub(17));
    let mut spans = vec![
        Span::styled(format!(" {} ", clock(e.at)), dim),
        Span::styled(format!("{:<7}", e.kind.label()), kind_style(e.kind)),
    ];
    match highlight.filter(|h| !h.is_empty()) {
        Some(h) if text.to_lowercase().contains(&h.to_lowercase()) => {
            let lower = text.to_lowercase();
            let i = lower.find(&h.to_lowercase()).unwrap();
            spans.push(Span::raw(text[..i].to_string()));
            spans.push(Span::styled(
                text[i..i + h.len()].to_string(),
                Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(text[i + h.len()..].to_string()));
        }
        _ => spans.push(Span::raw(text)),
    }
    Line::from(spans)
}

impl Panel for Events {
    fn id(&self) -> PanelId {
        8
    }
    fn title(&self) -> String {
        "Events".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let ui = &state.events_ui;
        match &ui.search {
            Some(q) => Some(format!("/{q}{}", if ui.editing { "▏" } else { "" })),
            None => Some(format!("{}", state.events.len())),
        }
    }
    fn min_rows(&self) -> u16 {
        4
    }
    fn priority(&self) -> u8 {
        95
    }
    fn placement(&self) -> Placement {
        Placement::Bottom
    }
    fn flexible(&self) -> bool {
        true
    }
    fn captures_input(&self, state: &State) -> bool {
        state.events_ui.editing || state.events_ui.search.is_some()
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        let total = state.events.len();
        let in_overlay = state.overlay == Some(self.id());
        let ui = &mut state.events_ui;
        if ui.editing {
            match key.code {
                KeyCode::Esc => {
                    ui.editing = false;
                    ui.search = None;
                }
                KeyCode::Enter => {
                    ui.editing = false;
                    ui.jump_to_match(&state.events, true);
                }
                KeyCode::Backspace => {
                    if let Some(q) = ui.search.as_mut() {
                        q.pop();
                    }
                }
                KeyCode::Char(c) => ui.search.get_or_insert_with(String::new).push(c),
                _ => {}
            }
            return Handled::Yes;
        }
        match key.code {
            KeyCode::Enter if !in_overlay => {
                state.overlay = Some(self.id());
                state.events_ui.scroll = None;
            }
            KeyCode::Char('/') => {
                ui.editing = true;
                ui.search.get_or_insert_with(String::new);
            }
            KeyCode::Char('n') if ui.search.is_some() => ui.jump_to_match(&state.events, true),
            KeyCode::Char('N') if ui.search.is_some() => ui.jump_to_match(&state.events, false),
            KeyCode::Esc if ui.search.is_some() => ui.search = None,
            KeyCode::Char('j') | KeyCode::Down => {
                let cur = ui.scroll.unwrap_or(total.saturating_sub(1));
                ui.scroll = Some((cur + 1).min(total.saturating_sub(1)));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let cur = ui.scroll.unwrap_or(total.saturating_sub(1));
                ui.scroll = Some(cur.saturating_sub(1));
            }
            KeyCode::Char('G') => ui.scroll = None,
            KeyCode::Char('g') => ui.scroll = Some(0),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let n = inner.height as usize;
        let lines: Vec<Line> = state
            .events
            .tail(n)
            .into_iter()
            .map(|e| line_for(e, inner.width as usize, None))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, state: &State) {
        let ui = &state.events_ui;
        let total = state.events.len();
        let title = match &ui.search {
            Some(q) => format!(
                " Events {total} — /{q}{}  (n next, N prev, Esc) ",
                if ui.editing { "▏" } else { "" }
            ),
            None => format!(" Events {total}  (j/k scroll, / search, Esc back) "),
        };
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = inner.height as usize;
        // `scroll` is the index of the bottom-most visible event; None = follow.
        let bottom = ui.scroll.unwrap_or(total.saturating_sub(1));
        let start = (bottom + 1).saturating_sub(rows);
        let lines: Vec<Line> = state
            .events
            .iter()
            .skip(start)
            .take(rows)
            .map(|e| line_for(e, inner.width as usize, ui.search.as_deref()))
            .collect();
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
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn events_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 51);
        assert!(out.contains("8 Events ─ "), "{out}");
        // Newest at the bottom: the fixture's last line is the cost-state.
        let last = out.lines().rev().find(|l| l.contains("│ 10:")).unwrap();
        assert!(last.contains("api    cost-state $9.90"), "{out}");
        assert!(out.contains("tool   mcp:claude-in-chrome ✓"), "{out}");
        // Older kinds exist in the log even if scrolled out of the panel.
        assert!(app
            .state
            .events
            .iter()
            .any(|e| e.text.starts_with("Stop hooks 1 ran")));
        assert!(app
            .state
            .events
            .iter()
            .any(|e| e.text.starts_with("turn done in")));
    }

    #[test]
    fn overlay_scroll_and_search() {
        let mut app = fixture_app();
        app.state.focused = Some(8);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(8));
        let out = render_to_string(&app, 80, 24);
        assert!(
            out.contains("Events ") && out.contains("j/k scroll"),
            "{out}"
        );
        // Search for the failing Bash calls.
        app.handle_key(key('/'));
        for c in "✗ error".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let out = render_to_string(&app, 80, 24);
        assert!(out.contains("/✗ error"), "{out}");
        assert!(out.contains("Bash ✗ error"), "{out}");
        let first = app.state.events_ui.scroll;
        app.handle_key(key('n'));
        assert_ne!(
            app.state.events_ui.scroll, first,
            "n moves to the next match"
        );
        app.handle_key(key('g'));
        assert_eq!(app.state.events_ui.scroll, Some(0));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.events_ui.search, None);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
    }
}
