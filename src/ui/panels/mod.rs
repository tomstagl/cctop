//! The dashboard panels, one module each.

pub mod context;
pub mod events;
pub mod header;
pub mod tokens;
pub mod tools;

use super::Panel;

/// Every panel in hotkey order. Panels that later stories add slot in here.
pub fn all() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(header::Header),
        Box::new(context::Context),
        Box::new(tokens::Tokens),
        Box::new(tools::Tools),
        Box::new(events::Events),
    ]
}
