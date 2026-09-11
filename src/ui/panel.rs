//! The panel contract and the shared frame drawing every panel uses.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use super::layout::PanelSpec;
use super::state::State;

/// Hotkey digit; 0 is the header (not toggleable), 1–9 are panels.
pub type PanelId = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

/// One bordered box on the dashboard.
pub trait Panel {
    fn id(&self) -> PanelId;
    fn title(&self) -> String;
    /// Right-aligned figure in the top border; shown even when collapsed.
    fn summary(&self, state: &State) -> Option<String>;
    /// Content rows needed to show everything (excluding the border).
    fn min_rows(&self) -> u16;
    /// Higher survives longer when rows are scarce.
    fn priority(&self) -> u8;
    /// Where the panel goes in the wide layout.
    fn placement(&self) -> super::layout::Placement;
    /// Soaks up leftover rows (tables, logs).
    fn flexible(&self) -> bool {
        false
    }
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
    /// Full-screen view when `state.overlay == Some(self.id())`.
    fn render_overlay(&self, _frame: &mut Frame, _area: Rect, _state: &State) {}

    /// Layout description derived from the trait methods.
    fn spec(&self) -> PanelSpec {
        PanelSpec {
            id: self.id(),
            min_rows: self.min_rows(),
            priority: self.priority(),
            placement: self.placement(),
            flexible: self.flexible(),
        }
    }
}

/// Colours the frame uses; the theme story replaces this with a real theme.
#[derive(Debug, Clone, Copy)]
pub struct FrameStyle {
    pub border: Style,
    pub border_focused: Style,
    pub hotkey: Style,
    pub title: Style,
    pub summary: Style,
}

impl Default for FrameStyle {
    fn default() -> Self {
        FrameStyle {
            border: Style::default().fg(Color::DarkGray),
            border_focused: Style::default().fg(Color::Cyan),
            hotkey: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            title: Style::default().add_modifier(Modifier::BOLD),
            summary: Style::default(),
        }
    }
}

/// Draw a panel's frame and return the inner area. A one-row area draws the
/// collapsed form: the title line only, in border style.
pub fn draw_frame(
    frame: &mut Frame,
    area: Rect,
    panel: &dyn Panel,
    state: &State,
    style: &FrameStyle,
) -> Option<Rect> {
    if area.height == 0 || area.width < 4 {
        return None;
    }
    let focused = state.focused == Some(panel.id());
    let border = if focused {
        style.border_focused
    } else {
        style.border
    };
    let summary = panel.summary(state);
    let title = title_line(panel, summary.as_deref(), style, border);

    if area.height == 1 {
        // Collapsed: "├─1 Context ─────── 67 % ┤"-style single line.
        let mut spans = vec![Span::styled("─", border)];
        spans.extend(title.spans);
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let fill = (area.width as usize).saturating_sub(used);
        spans.push(Span::styled("─".repeat(fill), border));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return None;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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
) -> Line<'static> {
    let mut spans = Vec::new();
    if panel.id() > 0 {
        spans.push(Span::styled(panel.id().to_string(), style.hotkey));
        spans.push(Span::styled(" ", border));
    }
    spans.push(Span::styled(panel.title(), style.title));
    if let Some(s) = summary {
        spans.push(Span::styled(" ", border));
        spans.push(Span::styled("─".to_string(), border));
        spans.push(Span::styled(" ", border));
        spans.push(Span::styled(s.to_string(), style.summary));
    }
    spans.push(Span::styled(" ", border));
    Line::from(spans)
}
