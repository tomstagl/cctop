//! Terminal UI: panels, layout and the application state they render.

pub mod panel;
pub mod state;

pub use panel::{Handled, Panel, PanelId};
pub use state::State;
pub mod agents_view;
pub mod coach_view;
pub mod dashboard;
pub mod fmt;
pub mod ledger_view;
pub mod panels;
pub mod picker;
pub mod prefix_view;
pub mod sources_view;
pub mod widgets;
