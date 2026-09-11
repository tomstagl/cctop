//! The dashboard panels, one module each.

pub mod agents;
pub mod context;
pub mod events;
pub mod files;
pub mod header;
pub mod limits;
pub mod tokens;
pub mod tools;
pub mod turn;

use super::Panel;

/// Every panel in hotkey order. Panels that later stories add slot in here.
pub fn all() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(header::Header),
        Box::new(context::Context),
        Box::new(tokens::Tokens),
        Box::new(limits::LimitsPanel),
        Box::new(turn::TurnPanel),
        Box::new(tools::Tools),
        Box::new(agents::Agents),
        Box::new(files::FilesPanel),
        Box::new(events::Events),
    ]
}
