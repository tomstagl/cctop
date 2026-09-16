//! `L`: pick another session from the registry.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::registry::{Session, Status};
use crate::ui::fmt;
use crate::ui::State;

#[derive(Debug, Clone)]
pub struct Row {
    pub session: Session,
    pub alive: bool,
    pub model: Option<String>,
    pub context_pct: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct PickerUi {
    pub rows: Vec<Row>,
    pub selected: usize,
    pub show_dead: bool,
}

impl PickerUi {
    /// Rows from the registry, with model and context % from status files.
    pub fn load(sessions: Vec<Session>) -> PickerUi {
        let rows = sessions
            .into_iter()
            .map(|s| {
                let status = crate::status::Watcher::new(&s.session_id);
                let model = status
                    .latest
                    .as_ref()
                    .and_then(|l| l.model.display_name.clone().or(l.model.id.clone()));
                let context_pct = status.latest.as_ref().and_then(|l| {
                    let cw = &l.context_window;
                    (cw.context_window_size > 0).then(|| {
                        cw.total_input_tokens as f64 / cw.context_window_size as f64 * 100.0
                    })
                });
                Row {
                    alive: s.is_alive(),
                    session: s,
                    model,
                    context_pct,
                }
            })
            .collect();
        PickerUi {
            rows,
            selected: 0,
            show_dead: false,
        }
    }

    pub fn visible(&self) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|r| self.show_dead || r.alive)
            .collect()
    }
}

/// Keys while the picker is open. Returns the chosen session on Enter.
pub fn handle_key(key: KeyEvent, state: &mut State) -> Option<Session> {
    let ui = state.picker.as_mut()?;
    let n = ui.visible().len();
    match key.code {
        KeyCode::Esc => state.picker = None,
        KeyCode::Char('d') => {
            ui.show_dead = !ui.show_dead;
            ui.selected = 0;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ui.selected = (ui.selected + 1).min(n.saturating_sub(1))
        }
        KeyCode::Char('k') | KeyCode::Up => ui.selected = ui.selected.saturating_sub(1),
        KeyCode::Enter => {
            let chosen = ui.visible().get(ui.selected).map(|r| r.session.clone());
            state.picker = None;
            return chosen;
        }
        _ => {}
    }
    None
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let Some(ui) = &state.picker else { return };
    let rows = ui.visible();
    let w = area.width.min(96);
    let h = (rows.len() as u16 + 4).clamp(5, area.height);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    frame.render_widget(Clear, rect);
    let block = Block::default().borders(Borders::ALL).title(format!(
        " sessions — {} shown{}  (Enter attach, d {} dead, Esc) ",
        rows.len(),
        if ui.show_dead { " incl. dead" } else { "" },
        if ui.show_dead { "hide" } else { "show" }
    ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let dim = state.theme.dim();
    let now = state.now_ms;
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {:<16} {:<28} {:<6} {:<10} {:>7} {:>5}",
            "NAME", "CWD", "STATE", "MODEL", "UP", "CTX"
        ),
        dim,
    ))];
    for (i, r) in rows.iter().enumerate() {
        let s = &r.session;
        let state_txt = if !r.alive {
            "dead"
        } else {
            match s.status() {
                Status::Busy => "busy",
                Status::Idle => "idle",
                Status::Other => "?",
            }
        };
        let up = if s.started_at > 0 {
            fmt::duration_ms(now - s.started_at as i64)
        } else {
            "—".into()
        };
        let ctx = r
            .context_pct
            .map(|p| format!("{p:.0} %"))
            .unwrap_or_else(|| "—".into());
        let mut line = Line::from(format!(
            " {:<16} {:<28} {:<6} {:<10} {:>7} {:>5}",
            fmt::clip(&s.name, 16),
            fmt::clip(&fmt::shorten_home(&s.cwd), 28),
            state_txt,
            fmt::clip(r.model.as_deref().unwrap_or("—"), 10),
            up,
            ctx
        ));
        if !r.alive {
            line = line.style(dim);
        }
        if i == ui.selected {
            line = line.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::ui::state::{SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn picker_lists_registry_rows_and_toggles_dead() {
        let path = fixtures().join("session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        let sessions = crate::registry::list(&fixtures().join("sessions"));
        app.state.picker = Some(crate::ui::picker::PickerUi::load(sessions));
        // Fixture pids may or may not exist on this machine (one of them is a
        // real registry entry), so count instead of assuming.
        let alive = app
            .state
            .picker
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.alive)
            .count();
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains(&format!("sessions — {alive} shown")), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("sessions — 3 shown incl. dead"), "{out}");
        assert!(
            out.contains("cctop-46") && out.contains("finsight-3") && out.contains("old-1"),
            "{out}"
        );
        assert!(out.contains("dead"), "pid 4194000 is never alive: {out}");
        // Rows are newest-first: cctop-46, finsight-3, old-1.
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.state.picker.is_none());
        assert_eq!(
            app.pending_switch.as_ref().map(|s| s.name.as_str()),
            Some("finsight-3")
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::NONE));
        assert!(
            app.state.picker.is_some(),
            "L reopens with the real registry"
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.state.picker.is_none());
    }

    #[test]
    fn switching_sessions_rebuilds_collectors_without_leaking_tailers() {
        let _serial = crate::tail::TEST_TAILERS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = fixtures().join("session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        let baseline = crate::tail::live_count();
        crate::attach::attach(&mut app, &path, SessionInfo::from_fixture(&path), true);
        // One tailer for the transcript plus one per subagent file (the fixture has one).
        let attached = crate::tail::live_count();
        assert!(attached > baseline, "transcript tailer alive");
        let hooks_a = app.tick_hooks.len();
        app.state.view = crate::ui::state::View::Coach;
        let start = std::time::Instant::now();
        crate::attach::attach(&mut app, &path, SessionInfo::from_fixture(&path), true);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(300),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(
            crate::tail::live_count(),
            attached,
            "old tailers dropped, new ones alive"
        );
        assert_eq!(app.tick_hooks.len(), hooks_a, "same collector set");
        assert_eq!(
            app.state.view,
            crate::ui::state::View::Coach,
            "UI preferences survive"
        );
        assert_eq!(app.state.lines_seen, 0, "session state is fresh");
        drop(app);
        assert_eq!(crate::tail::live_count(), baseline);
    }
}
