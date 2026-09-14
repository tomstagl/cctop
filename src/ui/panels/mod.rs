//! The dashboard panels, one module each.

pub mod advisor;
pub mod agents;
pub mod context;
pub mod events;
pub mod files;
pub mod limits;
pub mod tokens;
pub mod tools;
pub mod turn;

use super::Panel;

/// Every panel in digit order (the header is the dashboard's own line).
pub fn all() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(context::Context),
        Box::new(tokens::Tokens),
        Box::new(limits::LimitsPanel),
        Box::new(turn::TurnPanel),
        Box::new(tools::Tools),
        Box::new(agents::Agents),
        Box::new(files::FilesPanel),
        Box::new(advisor::AdvisorPanel),
        Box::new(events::Events),
    ]
}
