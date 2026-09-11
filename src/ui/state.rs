//! Everything the panels read. Collectors fill it through [`State::apply`];
//! panels only read it, except for UI-local fields (focus, hidden, sort keys)
//! which key handlers mutate.

use std::path::PathBuf;

use crate::metrics::{Aggregate, CostTracker, Pricing};
use crate::tools;
use crate::transcript::Line;
use crate::ui::panel::PanelId;

/// How long a toast stays in the footer.
pub const TOAST_MS: i64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionStatus {
    Busy,
    Idle,
    #[default]
    Unknown,
}

/// Identity and liveness of the attached session, from the registry (live)
/// or the transcript (fixture).
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub name: String,
    pub session_id: String,
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    pub version: String,
    pub status: SessionStatus,
    pub started_at_ms: Option<i64>,
    /// False once the pid is gone; values freeze and the header shows ENDED.
    pub alive: bool,
    /// When the session was seen ended (epoch ms).
    pub ended_at_ms: Option<i64>,
    /// Subscription plan from the status line, when the shim is installed.
    pub plan: Option<String>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub cpu_pct: Option<f32>,
    pub rss_bytes: Option<u64>,
    pub permission_mode: Option<String>,
    /// A PermissionRequest hook fired and no tool has completed since.
    pub permission_pending: bool,
}

impl SessionInfo {
    /// A live session from the registry.
    pub fn from_registry(s: &crate::registry::Session) -> SessionInfo {
        use crate::registry::Status;
        SessionInfo {
            name: s.name.clone(),
            session_id: s.session_id.clone(),
            pid: Some(s.pid),
            cwd: s.cwd.clone(),
            version: s.version.clone(),
            status: match s.status() {
                Status::Busy => SessionStatus::Busy,
                Status::Idle => SessionStatus::Idle,
                Status::Other => SessionStatus::Unknown,
            },
            started_at_ms: (s.started_at > 0).then_some(s.started_at as i64),
            alive: s.is_alive(),
            ..Default::default()
        }
    }

    /// A transcript file with no live process behind it.
    pub fn from_fixture(path: &std::path::Path) -> SessionInfo {
        SessionInfo {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            alive: false,
            ..Default::default()
        }
    }

    /// Re-check liveness; on the first miss, freeze the clock at `now_ms`.
    pub fn refresh_alive(&mut self, now_ms: i64) {
        let Some(pid) = self.pid else {
            return;
        };
        let alive = crate::registry::pid_alive(pid);
        if self.alive && !alive {
            self.ended_at_ms = Some(now_ms);
        }
        self.alive = alive;
    }
}

#[derive(Debug, Default)]
pub struct State {
    // -- collectors
    pub session: SessionInfo,
    pub agg: Aggregate,
    pub cost: CostTracker,
    pub tools: tools::Stats,
    /// Subagents of this session, by id.
    pub agents: std::collections::BTreeMap<String, crate::agents::Agent>,
    /// Exact context figures from the status line (shim), when present.
    pub context_window_exact: Option<u64>,
    pub context_size_exact: Option<u64>,
    /// Autocompact threshold learned from an observed compaction, by model.
    pub learned_thresholds: std::collections::BTreeMap<String, u64>,
    // -- ui
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
    /// Transcript lines applied so far.
    pub lines_seen: usize,
    /// Newest transcript timestamp seen (epoch ms).
    pub last_line_at_ms: Option<i64>,
    /// Tokens panel: include subagent usage (toggled with `a`).
    pub tokens_include_agents: bool,
}

impl State {
    pub fn new(pricing: Pricing) -> State {
        State {
            cost: CostTracker::new(pricing),
            tokens_include_agents: true,
            ..Default::default()
        }
    }

    /// "Now" for elapsed-time arithmetic: frozen at the end for dead sessions.
    pub fn clock_ms(&self) -> i64 {
        if self.session.alive {
            self.now_ms
        } else {
            self.session.ended_at_ms.unwrap_or(self.now_ms)
        }
    }

    /// Total usage of all subagents.
    pub fn agents_usage(&self) -> crate::metrics::Usage {
        let mut u = crate::metrics::Usage::default();
        for a in self.agents.values() {
            u.add(&a.usage);
        }
        u
    }

    /// Feed one main-transcript line to every collector.
    pub fn apply(&mut self, line: &Line) {
        self.agg.push(line);
        self.cost.push(line);
        self.tools.push(line);
        let ts = match line {
            Line::PermissionMode(p) => {
                self.session.permission_mode = Some(p.permission_mode.clone());
                None
            }
            Line::User(u) => {
                self.note_origin(u.version.as_deref(), u.cwd.as_deref());
                u.timestamp.as_deref()
            }
            Line::Assistant(a) => {
                self.note_origin(a.version.as_deref(), a.cwd.as_deref());
                a.timestamp.as_deref()
            }
            Line::System(sys) => sys.timestamp.as_deref(),
            _ => None,
        };
        if let Some(ms) = ts.and_then(crate::metrics::cost::parse_ts_ms) {
            self.last_line_at_ms = Some(self.last_line_at_ms.map_or(ms, |m| m.max(ms)));
        }
        self.lines_seen += 1;
    }

    fn note_origin(&mut self, version: Option<&str>, cwd: Option<&str>) {
        if let Some(v) = version {
            if self.session.version.is_empty() {
                self.session.version = v.to_string();
            }
        }
        if let Some(c) = cwd {
            if self.session.cwd.as_os_str().is_empty() {
                self.session.cwd = PathBuf::from(c);
            }
        }
    }

    /// Model currently in use.
    pub fn model(&self) -> Option<&str> {
        self.agg.model.as_deref()
    }

    /// Context-window view for the current model.
    pub fn context(&self) -> crate::metrics::ContextView {
        let learned = self
            .model()
            .and_then(|m| self.learned_thresholds.get(m).copied());
        crate::metrics::context::view(
            &self.agg,
            self.context_window_exact,
            self.context_size_exact,
            learned,
        )
    }

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
