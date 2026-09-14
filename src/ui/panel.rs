//! The panel contract and the shared frame drawing every panel uses.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::state::State;

/// Hotkey digit; 0 is the header (not toggleable), 1–9 are panels.
pub type PanelId = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

/// One panel: a ledger row on the dashboard, a full-screen view behind
/// its digit.
pub trait Panel {
    fn id(&self) -> PanelId;
    fn title(&self) -> String;
    /// The figure in the frame's top border.
    fn summary(&self, state: &State) -> Option<String>;
    /// Draw the content inside `inner` (the frame is drawn by the caller).
    fn render(&self, frame: &mut Frame, inner: Rect, state: &State);
    fn handle_key(&mut self, _key: KeyEvent, _state: &mut State) -> Handled {
        Handled::No
    }
    /// True while the panel wants every key (an inline text field, an
    /// overlay): global bindings are suspended except Ctrl-C.
    fn captures_input(&self, _state: &State) -> bool {
        false
    }
    /// The panel owns a full-screen view of its own (a ledger, a table, a
    /// log); otherwise its digit draws `render` under the frame.
    fn has_overlay(&self) -> bool {
        false
    }
    /// Full-screen view when `state.overlay == Some(self.id())` and
    /// `has_overlay()`.
    fn render_overlay(&self, _frame: &mut Frame, _area: Rect, _state: &State) {}
}

/// Styles the frame derives from the theme.
#[derive(Debug, Clone, Copy)]
pub struct FrameStyle {
    pub border: Style,
    pub border_focused: Style,
    pub hotkey: Style,
    pub title: Style,
    pub summary: Style,
}

impl FrameStyle {
    pub fn from_theme(t: &crate::theme::Theme) -> FrameStyle {
        FrameStyle {
            border: Style::default().fg(t.border),
            border_focused: Style::default().fg(t.border_focused),
            hotkey: Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            title: Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            summary: Style::default().fg(t.fg),
        }
    }
}

impl Default for FrameStyle {
    fn default() -> Self {
        FrameStyle::from_theme(&crate::theme::Theme::default())
    }
}

/// Draw a panel's frame and return the inner area. A one-row area draws the
/// collapsed form: the title line only, in border style.
pub fn draw_frame(frame: &mut Frame, area: Rect, panel: &dyn Panel, state: &State) -> Option<Rect> {
    if area.height == 0 || area.width < 4 {
        return None;
    }
    let t = &state.theme;
    let style = FrameStyle::from_theme(t);
    let border = style.border;
    let summary = panel.summary(state);
    let title = title_line(panel, summary.as_deref(), &style, border, t.hline());

    if area.height == 1 {
        // Collapsed: "├─1 Context ─────── 67 % ┤"-style single line.
        let mut spans = vec![Span::styled(t.hline(), border)];
        spans.extend(title.spans);
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let fill = (area.width as usize).saturating_sub(used);
        spans.push(Span::styled(t.hline().repeat(fill), border));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return None;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(t.border_set())
        .border_style(border)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    Some(inner)
}

fn title_line(
    panel: &dyn Panel,
    summary: Option<&str>,
    style: &FrameStyle,
    border: Style,
    hline: &'static str,
) -> Line<'static> {
    let mut spans = Vec::new();
    if panel.id() > 0 {
        spans.push(Span::styled(panel.id().to_string(), style.hotkey));
        spans.push(Span::styled(" ", border));
    }
    spans.push(Span::styled(panel.title(), style.title));
    if let Some(s) = summary {
        spans.push(Span::styled(" ", border));
        spans.push(Span::styled(hline, border));
        spans.push(Span::styled(" ", border));
        spans.push(Span::styled(s.to_string(), style.summary));
    }
    spans.push(Span::styled(" ", border));
    Line::from(spans)
}
