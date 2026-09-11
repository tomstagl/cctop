//! Everything the panels read. Collectors fill it; panels only read it, except
//! for UI-local fields (focus, sort keys) which key handlers mutate.

use crate::ui::panel::PanelId;

#[derive(Debug, Default, Clone)]
pub struct State {
    /// Panel that receives keys; `None` = global.
    pub focused: Option<PanelId>,
    /// Panels the user toggled off with their hotkey digit.
    pub hidden: Vec<PanelId>,
    /// Wall clock for the frame being rendered (epoch ms).
    pub now_ms: i64,
    /// Footer replacement: message and the epoch ms it expires.
    pub toast: Option<(String, i64)>,
    /// Updates are buffered, not applied.
    pub paused: bool,
    /// Lines buffered while paused.
    pub paused_pending: usize,
    /// Transcript lines applied so far (all collectors see the same stream).
    pub lines_seen: usize,
}

/// How long a toast stays in the footer.
pub const TOAST_MS: i64 = 3_000;

impl State {
    pub fn is_hidden(&self, id: PanelId) -> bool {
        self.hidden.contains(&id)
    }

    /// Show `msg` in the footer for [`TOAST_MS`].
    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.now_ms + TOAST_MS));
    }

    /// The toast text if it has not expired.
    pub fn toast_text(&self) -> Option<&str> {
        match &self.toast {
            Some((m, until)) if *until > self.now_ms => Some(m.as_str()),
            _ => None,
        }
    }

    pub fn toggle_hidden(&mut self, id: PanelId) {
        if let Some(i) = self.hidden.iter().position(|&h| h == id) {
            self.hidden.remove(i);
        } else {
            self.hidden.push(id);
        }
    }
}
