//! Terminal UI: panels, layout and the application state they render.

pub mod layout;
pub mod panel;
pub mod state;

pub use layout::{solve, Layout, Mode, PanelSpec, Placement};
pub use panel::{Handled, Panel, PanelId};
pub use state::State;
pub mod fmt;
pub mod ledger_view;
pub mod panels;
pub mod widgets;
