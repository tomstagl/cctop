//! The application: owns the panels and the state, handles keys, draws a
//! frame, and runs either the interactive terminal loop or one headless
//! render for tests and `--once`.

use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};

use crate::transcript::Line;
use crate::ui::layout::{self, Mode};
use crate::ui::panel::{draw_frame, Handled, Panel, PanelId};
use crate::ui::State;

/// Applies a transcript line to every collector in the state.
pub type Sink = Box<dyn FnMut(&Line, &mut State)>;
/// Clock-driven collector run before each frame.
pub type TickHook = Box<dyn FnMut(&mut State)>;

pub const REFRESH_DEFAULT_MS: u64 = 250;
pub const REFRESH_MIN_MS: u64 = 100;
pub const REFRESH_MAX_MS: u64 = 2_000;

/// A key binding for the help overlay and the footer.
pub struct Binding {
    pub keys: &'static str,
    pub action: &'static str,
}

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: "?",
        action: "help",
    },
    Binding {
        keys: "1-9",
        action: "toggle panel",
    },
    Binding {
        keys: "Tab / Shift-Tab",
        action: "focus next / previous panel",
    },
    Binding {
        keys: "Esc",
        action: "clear focus / close overlay",
    },
    Binding {
        keys: "p",
        action: "pause updates",
    },
    Binding {
        keys: "w",
        action: "narrow / wide layout",
    },
    Binding {
        keys: "+ / -",
        action: "render interval 100 ms … 2 s",
    },
    Binding {
        keys: "q / Ctrl-C",
        action: "quit (never touches the session)",
    },
];

pub struct App {
    pub panels: Vec<Box<dyn Panel>>,
    pub state: State,
    pub mode_override: Option<Mode>,
    pub refresh_ms: u64,
    pub help: bool,
    pub quit: bool,
    buffered: Vec<Line>,
    /// Applies a transcript line to every collector.
    sink: Sink,
    /// Run before every frame: liveness, git, process stats.
    pub tick_hooks: Vec<TickHook>,
    /// Transcript followers for the attached session.
    pub sources: Vec<Box<dyn LineSource>>,
    /// Session chosen in the picker, to be attached by the loop.
    pub pending_switch: Option<crate::registry::Session>,
    alerts: crate::alerts::Engine,
    advisor: crate::advisor::Engine,
    /// Send critical alerts to the desktop (`--notify`).
    pub desktop_notify: bool,
    /// Persisted preferences; written back when panels or theme change.
    pub config: crate::config::Config,
    /// Terminal capabilities the theme is reduced to.
    pub caps: crate::theme::Caps,
    /// Current theme name (bundled or user).
    pub theme_name: String,
    /// Where user themes live (hot-reloaded).
    pub user_theme_dir: Option<std::path::PathBuf>,
    last_theme_check: Option<Instant>,
    /// Write config changes to disk (off in tests and headless runs).
    pub persist_config: bool,
}

impl App {
    pub fn new(panels: Vec<Box<dyn Panel>>, sink: Sink) -> App {
        App {
            panels,
            state: State::default(),
            mode_override: None,
            refresh_ms: REFRESH_DEFAULT_MS,
            help: false,
            quit: false,
            buffered: Vec::new(),
            sink,
            tick_hooks: Vec::new(),
            sources: Vec::new(),
            pending_switch: None,
            alerts: crate::alerts::Engine::default(),
            advisor: crate::advisor::Engine::default(),
            desktop_notify: false,
            config: crate::config::Config::default(),
            caps: crate::theme::Caps::full(),
            theme_name: "default-dark".into(),
            user_theme_dir: None,
            last_theme_check: None,
            persist_config: false,
        }
    }

    /// Apply a theme by name for the current capabilities; unknown names fall
    /// back to the bundled default.
    pub fn set_theme(&mut self, name: &str) {
        let t = crate::theme::Theme::find(name, self.user_theme_dir.as_deref())
            .unwrap_or_default()
            .for_caps(self.caps);
        self.theme_name = if t.name.is_empty() {
            "default-dark".into()
        } else {
            t.name.clone()
        };
        self.state.theme = t;
    }

    /// Apply the config: layout, theme, refresh, hidden panels, notify.
    pub fn apply_config(&mut self, config: crate::config::Config) {
        self.mode_override = config.layout_mode();
        self.refresh_ms = config.refresh_ms.clamp(REFRESH_MIN_MS, REFRESH_MAX_MS);
        self.state.hidden = config.hidden_panels.clone();
        self.desktop_notify = self.desktop_notify || config.notify;
        let theme = config.theme.clone();
        self.config = config;
        self.set_theme(&theme);
    }

    /// Persist the current panel/theme choices.
    pub fn save_config(&mut self) {
        self.config.hidden_panels = self.state.hidden.clone();
        self.config.theme = self.theme_name.clone();
        self.config.refresh_ms = self.refresh_ms;
        self.config.layout = match self.mode_override {
            Some(Mode::Narrow) => "narrow".into(),
            Some(Mode::Wide) => "wide".into(),
            None => "auto".into(),
        };
        if self.persist_config {
            self.config.save();
        }
    }

    /// Re-read a user theme file if it changed (checked at most every 2 s).
    pub fn hot_reload_theme(&mut self) {
        let due = self
            .last_theme_check
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if !due {
            return;
        }
        self.last_theme_check = Some(Instant::now());
        let Some(dir) = &self.user_theme_dir else {
            return;
        };
        if let Some(t) = crate::theme::Theme::user_themes(dir)
            .into_iter()
            .find(|t| t.name == self.theme_name)
        {
            let t = t.for_caps(self.caps);
            if t != self.state.theme {
                self.state.theme = t;
            }
        }
    }

    /// Run the tick hooks (clock-driven collectors), then the alert rules.
    pub fn tick(&mut self) {
        for h in self.tick_hooks.iter_mut() {
            h(&mut self.state);
        }
        self.evaluate_alerts();
    }

    /// The switch the loop owes, with the toast that explains it: the
    /// picker's choice first, else the new session id the registry shows
    /// for this session's pid (`/clear` rewrote the entry under the running
    /// process, and the old transcript never grows again).
    pub fn take_switch(&mut self) -> Option<(crate::registry::Session, String)> {
        if let Some(chosen) = self.pending_switch.take() {
            let toast = format!("attached to {}", chosen.name);
            return Some((chosen, toast));
        }
        let next = self.state.rotated_to.take()?;
        let short: String = next.session_id.chars().take(8).collect();
        Some((
            next,
            format!("session id changed (/clear): attached to {short}"),
        ))
    }

    /// Re-attach to `session`, keeping the UI preferences, and show `toast`.
    pub fn switch_to(&mut self, session: &crate::registry::Session, toast: String) {
        let info = crate::ui::state::SessionInfo::from_registry(session);
        let transcript = crate::transcript_path(session);
        crate::attach::attach(self, &transcript, info, true);
        self.state.set_toast(toast);
    }

    /// Fire any alert whose threshold was just crossed, and refresh the advice.
    pub fn evaluate_alerts(&mut self) {
        let fired = self.alerts.evaluate(&self.state);
        if !fired.is_empty() {
            crate::alerts::deliver(&fired, &mut self.state, self.desktop_notify);
        }
        let now = self.state.clock_ms();
        let turn = self.state.agg.human_turns();
        for (rule, session) in std::mem::take(&mut self.state.advice_snoozed) {
            let toast = if session {
                self.advisor.snooze_session(rule, now)
            } else {
                self.advisor.snooze(rule, turn, now)
            };
            self.state.set_toast(toast);
        }
        if std::mem::take(&mut self.state.advice_acting) {
            self.advisor.acting();
        }
        for toast in self.advisor.poll_requests(turn, now) {
            self.state.set_toast(toast);
        }
        self.advisor.evaluate(&self.state);
        for ev in self.advisor.drain_events() {
            self.state.events.push(crate::events::Event {
                at: ev.at_ms,
                kind: crate::events::Kind::Coach,
                text: ev.text,
            });
        }
        self.advisor.save();
        self.state.advice = self.advisor.current.clone();
        self.state.advice_view = crate::ui::state::AdviceView {
            session_mode: Some(self.advisor.session_mode),
            has_occupant: self.advisor.occupant.is_some(),
            acting: self.advisor.occupant.as_ref().is_some_and(|o| o.acting),
            next_condition: self.advisor.next_up().map(|(_, c)| c),
            snoozed: self.advisor.snoozed(),
            suppressed: self.advisor.suppressed.clone(),
            recent: self.advisor.recent.clone(),
        };
        if self.state.advice_index >= self.state.advice.len() {
            self.state.advice_index = 0;
        }
    }

    /// Attach the advisor to the session's persisted state under `home`
    /// (`~/.cctop`), as the writer when this is the live TUI.
    pub fn attach_advisor(&mut self, home: &std::path::Path, writer: bool) {
        self.advisor.release();
        let mut engine = crate::advisor::Engine::default();
        let id = self.state.session.session_id.clone();
        if !id.is_empty() {
            engine.attach(home, &id, writer);
        }
        self.advisor = engine;
    }

    /// The SessionEnd tally: `(fired, acted, snoozed)`.
    pub fn advisor_tally(&self) -> (usize, usize, usize) {
        self.advisor.tally()
    }

    /// Release the advisor's writer lock (on exit).
    pub fn release_advisor(&mut self) {
        self.advisor.save();
        self.advisor.release();
    }

    /// Feed one transcript line; buffered while paused.
    pub fn feed(&mut self, line: Line) {
        if self.state.paused {
            self.buffered.push(line);
            self.state.paused_pending = self.buffered.len();
        } else {
            (self.sink)(&line, &mut self.state);
            self.state.lines_seen += 1;
        }
    }

    fn toggle_pause(&mut self) {
        self.state.paused = !self.state.paused;
        if !self.state.paused {
            for line in std::mem::take(&mut self.buffered) {
                (self.sink)(&line, &mut self.state);
                self.state.lines_seen += 1;
            }
            self.state.paused_pending = 0;
        }
    }

    fn visible_ids(&self) -> Vec<PanelId> {
        layout::NARROW_ORDER
            .iter()
            .copied()
            .filter(|id| *id > 0 && !self.state.is_hidden(*id))
            .filter(|id| self.panels.iter().any(|p| p.id() == *id))
            .collect()
    }

    fn cycle_focus(&mut self, forward: bool) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .state
            .focused
            .and_then(|f| ids.iter().position(|&i| i == f));
        let next = match (cur, forward) {
            (None, true) => 0,
            (None, false) => ids.len() - 1,
            (Some(i), true) => (i + 1) % ids.len(),
            (Some(i), false) => (i + ids.len() - 1) % ids.len(),
        };
        self.state.focused = Some(ids[next]);
    }

    fn route_to(&mut self, id: PanelId, key: KeyEvent) -> Handled {
        let mut state = std::mem::take(&mut self.state);
        let handled = self
            .panels
            .iter_mut()
            .find(|p| p.id() == id)
            .map(|p| p.handle_key(key, &mut state))
            .unwrap_or(Handled::No);
        self.state = state;
        handled
    }

    /// Global keys first; anything else goes to the focused panel. A panel
    /// that captures input (overlay, text field) sees every key first.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.help {
            self.help = false;
            return;
        }
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl_c {
            self.quit = true;
            return;
        }
        if self.state.picker.is_some() {
            if let Some(chosen) = crate::ui::picker::handle_key(key, &mut self.state) {
                self.pending_switch = Some(chosen);
            }
            return;
        }
        if let Some((_, text)) = self.state.ask.clone() {
            match key.code {
                KeyCode::Esc => self.state.ask = None,
                KeyCode::Enter => {
                    let msg = match crate::ask::copy_to_clipboard(&text) {
                        Ok(tool) => format!("copied with {tool} — paste it into Claude Code"),
                        Err(e) => format!("could not copy: {e}"),
                    };
                    self.state.set_toast(msg);
                    self.state.ask = None;
                }
                KeyCode::Char('S') => {
                    let msg = match &self.state.messaging_socket {
                        Some(sock) => {
                            let token = self.state.session.pid.and_then(|pid| {
                                crate::registry::default_dir()
                                    .and_then(|d| crate::ask::peer_token(&d, pid))
                            });
                            match crate::ask::send_over_socket(sock, token.as_deref(), &text) {
                                Ok(_) => "sent to the session as a peer message".to_string(),
                                Err(e) => format!("send failed: {e} — copied instead?"),
                            }
                        }
                        None => "no messaging socket — use Enter to copy".to_string(),
                    };
                    self.state.set_toast(msg);
                    self.state.ask = None;
                }
                _ => {}
            }
            return;
        }
        if let Some(id) = self.state.overlay {
            // The panel gets first refusal (e.g. Esc clears its search);
            // an unhandled Esc closes the overlay.
            if self.route_to(id, key) == Handled::No && key.code == KeyCode::Esc {
                self.state.overlay = None;
            }
            return;
        }
        if let Some(id) = self.state.focused {
            let captures = self
                .panels
                .iter()
                .find(|p| p.id() == id)
                .is_some_and(|p| p.captures_input(&self.state));
            if captures && self.route_to(id, key) == Handled::Yes {
                return;
            }
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') if ctrl_c || key.code == KeyCode::Char('q') => {
                self.quit = true
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.cycle_focus(true),
            KeyCode::BackTab => self.cycle_focus(false),
            KeyCode::Esc => self.state.focused = None,
            KeyCode::Char('p') => {
                self.toggle_pause();
                let msg = if self.state.paused {
                    "paused"
                } else {
                    "resumed"
                };
                self.state.set_toast(msg);
            }
            KeyCode::Char('a')
                if self.state.focused.is_some_and(|f| f != 2) || self.state.focused.is_none() =>
            {
                let panel = self.state.focused.unwrap_or(0);
                match crate::ask::compose(&self.state, panel) {
                    Some(text) => self.state.ask = Some((panel, text)),
                    None => self.state.set_toast("nothing to ask about on this panel"),
                }
            }
            KeyCode::Char('L') => {
                let sessions = crate::registry::default_dir()
                    .map(|d| crate::registry::list(&d))
                    .unwrap_or_default();
                self.state.picker = Some(crate::ui::picker::PickerUi::load(sessions));
            }
            KeyCode::Char('t') => {
                let names = crate::theme::Theme::names(self.user_theme_dir.as_deref());
                let i = names
                    .iter()
                    .position(|n| *n == self.theme_name)
                    .unwrap_or(0);
                let next = names[(i + 1) % names.len()].clone();
                self.set_theme(&next);
                self.state.set_toast(format!("theme: {next}"));
                self.save_config();
            }
            KeyCode::Char('w') => {
                self.mode_override = Some(match self.mode_override {
                    Some(Mode::Wide) => Mode::Narrow,
                    Some(Mode::Narrow) => Mode::Wide,
                    None => Mode::Wide,
                });
                self.state
                    .set_toast(format!("layout: {:?}", self.mode_override.unwrap()));
                self.save_config();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.refresh_ms = (self.refresh_ms + 100).min(REFRESH_MAX_MS);
                self.state
                    .set_toast(format!("refresh {} ms", self.refresh_ms));
            }
            KeyCode::Char('-') => {
                self.refresh_ms = self.refresh_ms.saturating_sub(100).max(REFRESH_MIN_MS);
                self.state
                    .set_toast(format!("refresh {} ms", self.refresh_ms));
            }
            KeyCode::Char(c @ '1'..='9') => {
                let id = c as u8 - b'0';
                self.state.toggle_hidden(id);
                if self.state.focused == Some(id) {
                    self.state.focused = None;
                }
                self.save_config();
            }
            _ => {
                if let Some(id) = self.state.focused {
                    self.route_to(id, key);
                }
            }
        }
    }

    /// Draw one frame.
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let specs: Vec<_> = self.panels.iter().map(|p| p.spec()).collect();
        let lay = layout::solve(area, &specs, &self.state.hidden, self.mode_override);
        for (id, rect) in &lay.rects {
            let Some(panel) = self.panels.iter().find(|p| p.id() == *id) else {
                continue;
            };
            if let Some(inner) = draw_frame(frame, *rect, panel.as_ref(), &self.state) {
                panel.render(frame, inner, &self.state);
            }
        }
        self.draw_footer(frame, lay.footer);
        if let Some(id) = self.state.overlay {
            if let Some(panel) = self.panels.iter().find(|p| p.id() == id) {
                let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
                frame.render_widget(Clear, body);
                panel.render_overlay(frame, body, &self.state);
            }
        }
        if let Some((panel, text)) = &self.state.ask {
            self.draw_ask(frame, area, *panel, text);
        }
        if self.state.picker.is_some() {
            crate::ui::picker::render(frame, area, &self.state);
        }
        if self.help {
            self.draw_help(frame, area);
        }
    }

    fn draw_ask(&self, frame: &mut Frame, area: Rect, panel: PanelId, text: &str) {
        use ratatui::widgets::Wrap;
        let w = area.width.min(72);
        let cols = w.saturating_sub(2).max(1) as usize;
        let rows = |n: usize| n.div_ceil(cols) as u16;
        let h = (rows(text.chars().count()) + rows(110) + 4).min(area.height);
        let rect = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        frame.render_widget(Clear, rect);
        let has_socket = self.state.messaging_socket.is_some();
        let title = format!(
            " ask about panel {panel}  —  Enter copy · {}Esc cancel ",
            if has_socket { "S send · " } else { "" }
        );
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        let mut lines = vec![TLine::from(text.to_string()), TLine::from("")];
        lines.push(TLine::from(Span::styled(
            if has_socket {
                "Enter copies the draft to the clipboard. S sends it over the session socket (arrives as a peer message)."
            } else {
                "Enter copies the draft to the clipboard; paste it into Claude Code. (no messaging socket)"
            },
            self.state.theme.dim(),
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        if area.height == 0 {
            return;
        }
        let dim = self.state.theme.dim();
        let line = if let Some(t) = self.state.toast_text() {
            TLine::from(Span::styled(format!(" {t}"), self.state.theme.warn()))
        } else {
            let mut spans = vec![Span::raw(" ")];
            if self.state.paused {
                spans.push(Span::styled(
                    format!("PAUSED +{} ", self.state.paused_pending),
                    Style::default()
                        .fg(self.state.theme.bg)
                        .bg(self.state.theme.warn),
                ));
            }
            spans.push(Span::styled(
                "?help 1-9 panels ⇥focus a ask t theme L sessions q quit",
                dim,
            ));
            TLine::from(spans)
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let w = area.width.min(56);
        let h = (BINDINGS.len() as u16 + 4).min(area.height);
        let rect = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        frame.render_widget(Clear, rect);
        let mut lines = vec![TLine::from("")];
        for b in BINDINGS {
            lines.push(TLine::from(vec![
                Span::styled(
                    format!("  {:<16}", b.keys),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(b.action),
            ]));
        }
        lines.push(TLine::from(Span::styled(
            "  any key to close",
            self.state.theme.dim(),
        )));
        let block = Block::default().borders(Borders::ALL).title(" cctop keys ");
        frame.render_widget(Paragraph::new(lines).block(block), rect);
    }
}

// --------------------------------------------------------------- headless

/// Render one frame at `width`×`height` and return it as plain text
/// (one line per row, trailing spaces trimmed).
pub fn render_to_string(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| app.draw(f)).expect("draw");
    buffer_to_string(term.backend().buffer())
}

/// Render one frame as text with ANSI true-colour escapes (for recordings).
pub fn render_to_ansi(app: &App, width: u16, height: u16) -> String {
    use ratatui::style::{Color, Modifier};
    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| app.draw(f)).expect("draw");
    let buf = term.backend().buffer();
    let code = |c: Color, fg: bool| -> String {
        let base = if fg { 38 } else { 48 };
        match c {
            Color::Rgb(r, g, b) => format!("\x1b[{base};2;{r};{g};{b}m"),
            Color::Indexed(i) => format!("\x1b[{base};5;{i}m"),
            Color::Reset => format!("\x1b[{}m", if fg { 39 } else { 49 }),
            _ => String::new(),
        }
    };
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut last: Option<(Color, Color, Modifier)> = None;
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            let cur = (cell.fg, cell.bg, cell.modifier);
            if last != Some(cur) {
                out.push_str("\x1b[0m");
                out.push_str(&code(cell.fg, true));
                out.push_str(&code(cell.bg, false));
                if cell.modifier.contains(Modifier::BOLD) {
                    out.push_str("\x1b[1m");
                }
                if cell.modifier.contains(Modifier::REVERSED) {
                    out.push_str("\x1b[7m");
                }
                last = Some(cur);
            }
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

pub fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut row = String::new();
        for x in 0..buf.area.width {
            row.push_str(buf[(x, y)].symbol());
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

/// Parse `--keys "Tab,Enter,n,?"` into key events.
pub fn parse_keys(spec: &str) -> Vec<KeyEvent> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let code = match s {
                "Enter" => KeyCode::Enter,
                "Tab" => KeyCode::Tab,
                "BackTab" | "Shift-Tab" => KeyCode::BackTab,
                "Esc" => KeyCode::Esc,
                "Up" => KeyCode::Up,
                "Down" => KeyCode::Down,
                "Left" => KeyCode::Left,
                "Right" => KeyCode::Right,
                "Space" => KeyCode::Char(' '),
                s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
                other => KeyCode::Char(other.chars().next().unwrap_or(' ')),
            };
            KeyEvent::new(code, KeyModifiers::NONE)
        })
        .collect()
}

/// Parse `WxH`.
pub fn parse_size(s: &str) -> Option<(u16, u16)> {
    let (w, h) = s.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

// ------------------------------------------------------------ interactive

/// Restores the terminal even on panic.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> std::io::Result<TerminalGuard> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = std::io::stdout().flush();
    }
}

/// Where transcript lines come from while the loop runs.
pub trait LineSource {
    /// Drain everything available right now.
    fn drain(&mut self) -> Vec<Line>;
}

impl LineSource for crate::tail::Tailer {
    fn drain(&mut self) -> Vec<Line> {
        let mut v = Vec::new();
        while let Some(l) = self.try_recv() {
            v.push(l);
        }
        v
    }
}

/// Run the interactive loop until the user quits.
pub fn run_tui(mut app: App) -> std::io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut term = Terminal::new(backend)?;
    term.clear()?;
    let mut last_render = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;
    while !app.quit {
        let tick = Duration::from_millis(app.refresh_ms);
        // Data first.
        let mut batch = Vec::new();
        for s in app.sources.iter_mut() {
            batch.extend(s.drain());
        }
        for line in batch {
            app.feed(line);
            dirty = true;
        }
        if dirty {
            app.state.now_ms = now_ms();
            app.evaluate_alerts();
        }
        // Then input, waiting at most until the next render is due.
        let wait = tick
            .saturating_sub(last_render.elapsed())
            .max(Duration::from_millis(10));
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k) if k.kind != event::KeyEventKind::Release => {
                    app.handle_key(k);
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
        if let Some((session, toast)) = app.take_switch() {
            app.switch_to(&session, toast);
            dirty = true;
        }
        // Clock-driven values (elapsed, toasts) change every tick.
        if dirty || last_render.elapsed() >= tick {
            app.state.now_ms = now_ms();
            app.hot_reload_theme();
            app.tick();
            term.draw(|f| app.draw(f))?;
            last_render = Instant::now();
            dirty = false;
        }
    }
    app.release_advisor();
    Ok(())
}

pub fn now_ms() -> i64 {
    // A fixed clock makes headless renders reproducible (demo, snapshots).
    if let Some(v) = std::env::var_os("CCTOP_FAKE_NOW") {
        if let Some(ms) = v.to_str().and_then(|s| s.parse::<i64>().ok()) {
            return ms;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Draw a frame with a `TestBackend`-independent terminal (used by callers
/// that already own a terminal).
pub fn draw_once<B: Backend>(term: &mut Terminal<B>, app: &App) -> std::io::Result<()> {
    term.draw(|f| app.draw(f))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::Placement;

    struct Stub {
        id: PanelId,
        keys: std::cell::Cell<usize>,
    }
    impl Panel for Stub {
        fn id(&self) -> PanelId {
            self.id
        }
        fn title(&self) -> String {
            format!("Stub{}", self.id)
        }
        fn summary(&self, s: &State) -> Option<String> {
            Some(format!("{} lines", s.lines_seen))
        }
        fn min_rows(&self) -> u16 {
            2
        }
        fn priority(&self) -> u8 {
            50 + self.id
        }
        fn placement(&self) -> Placement {
            if self.id == 8 {
                Placement::Bottom
            } else {
                Placement::Left
            }
        }
        fn render(&self, frame: &mut Frame, inner: Rect, _s: &State) {
            frame.render_widget(Paragraph::new(format!("content {}", self.id)), inner);
        }
        fn handle_key(&mut self, _k: KeyEvent, _s: &mut State) -> Handled {
            self.keys.set(self.keys.get() + 1);
            Handled::Yes
        }
    }

    #[test]
    fn take_switch_prefers_the_picker_and_names_a_rotation() {
        let registry = crate::registry::list(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sessions"),
        );
        let chosen = registry
            .iter()
            .find(|s| s.name == "cctop-46")
            .unwrap()
            .clone();
        let mut rotated = registry
            .iter()
            .find(|s| s.name == "finsight-3")
            .unwrap()
            .clone();
        rotated.session_id = "9d337130-79ad-4df0-8bd5-1365210208b1".into();

        let mut a = app();
        assert!(a.take_switch().is_none());

        // Both pending: the picker's choice first, the rotation on the next pass.
        a.pending_switch = Some(chosen.clone());
        a.state.rotated_to = Some(rotated.clone());
        let (first, toast) = a.take_switch().unwrap();
        assert_eq!(first.session_id, chosen.session_id);
        assert_eq!(toast, "attached to cctop-46");
        let (second, toast) = a.take_switch().unwrap();
        assert_eq!(second.session_id, rotated.session_id);
        assert_eq!(toast, "session id changed (/clear): attached to 9d337130");
        assert!(a.take_switch().is_none(), "both consumed");
    }

    fn app() -> App {
        let panels: Vec<Box<dyn Panel>> = [1u8, 2, 8]
            .into_iter()
            .map(|id| {
                Box::new(Stub {
                    id,
                    keys: std::cell::Cell::new(0),
                }) as Box<dyn Panel>
            })
            .collect();
        App::new(panels, Box::new(|_l, _s| {}))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn headless_render_shows_panels_and_footer() {
        let out = render_to_string(&app(), 40, 16);
        assert!(out.contains("Stub1"), "{out}");
        assert!(out.contains("content 1"));
        assert!(out.contains("0 lines"));
        assert!(out.contains("?help 1-9 panels"), "{out}");
        assert_eq!(out.lines().count(), 16);
    }

    #[test]
    fn toggle_hides_panel_and_help_overlay_lists_bindings() {
        let mut a = app();
        a.handle_key(key('2'));
        let out = render_to_string(&a, 40, 16);
        assert!(!out.contains("Stub2"));
        a.handle_key(key('2'));
        assert!(render_to_string(&a, 40, 16).contains("Stub2"));
        a.handle_key(key('?'));
        let out = render_to_string(&a, 60, 20);
        assert!(out.contains("cctop keys"));
        for b in BINDINGS {
            assert!(out.contains(b.action), "missing {}", b.action);
        }
        a.handle_key(key('x'));
        assert!(!a.help);
    }

    #[test]
    fn focus_cycles_and_routes_keys_to_panel() {
        let mut a = app();
        assert_eq!(a.state.focused, None);
        a.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(a.state.focused, Some(1));
        a.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(a.state.focused, Some(2));
        a.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(a.state.focused, Some(1));
        a.handle_key(key('j'));
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(a.state.focused, None);
    }

    #[test]
    fn pause_buffers_and_resume_applies() {
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let c2 = count.clone();
        let mut a = App::new(Vec::new(), Box::new(move |_l, _s| c2.set(c2.get() + 1)));
        let line = || Line::parse(r#"{"type":"queue-operation","operation":"enqueue"}"#).unwrap();
        a.feed(line());
        assert_eq!(count.get(), 1);
        a.handle_key(key('p'));
        assert!(a.state.paused);
        a.feed(line());
        a.feed(line());
        assert_eq!(count.get(), 1);
        assert_eq!(a.state.paused_pending, 2);
        assert!(
            render_to_string(&a, 40, 5).contains("PAUSED +2") || a.state.toast_text().is_some()
        );
        a.handle_key(key('p'));
        assert_eq!(count.get(), 3);
        assert_eq!(a.state.lines_seen, 3);
    }

    #[test]
    fn refresh_bounds_layout_toggle_and_quit() {
        let mut a = app();
        for _ in 0..30 {
            a.handle_key(key('+'));
        }
        assert_eq!(a.refresh_ms, REFRESH_MAX_MS);
        for _ in 0..30 {
            a.handle_key(key('-'));
        }
        assert_eq!(a.refresh_ms, REFRESH_MIN_MS);
        a.handle_key(key('w'));
        assert_eq!(a.mode_override, Some(Mode::Wide));
        a.handle_key(key('w'));
        assert_eq!(a.mode_override, Some(Mode::Narrow));
        a.state.now_ms = 0;
        a.state.set_toast("hello");
        assert!(render_to_string(&a, 40, 6).contains("hello"));
        a.state.now_ms = 5_000;
        assert!(!render_to_string(&a, 40, 6).contains("hello"));
        a.handle_key(key('q'));
        assert!(a.quit);
        let mut b = app();
        b.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(b.quit);
    }

    #[test]
    fn key_and_size_parsing() {
        let k = parse_keys("Tab, Enter,n,?");
        assert_eq!(k.len(), 4);
        assert_eq!(k[0].code, KeyCode::Tab);
        assert_eq!(k[1].code, KeyCode::Enter);
        assert_eq!(k[2].code, KeyCode::Char('n'));
        assert_eq!(k[3].code, KeyCode::Char('?'));
        assert_eq!(parse_size("60x51"), Some((60, 51)));
        assert_eq!(parse_size("bad"), None);
    }
}
