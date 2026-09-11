//! cctop library: collectors, metrics and UI. The `cctop` binary is a thin
//! CLI over this crate so that every number is reachable from tests and from
//! `cctop query`.

pub mod discover;
pub mod metrics;
pub mod registry;
pub mod tail;
pub mod transcript;
