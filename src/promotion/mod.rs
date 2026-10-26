//! `aic promotion`: read-only promotion diagnostics — lock state, promotion
//! status, and promotion reports, including the provisional one.
//!
//! Nothing here locks, promotes or unlocks. Endpoint shapes and the
//! measurements behind every classification are in
//! `docs/api/23-promotion.md`.

pub mod api;
pub mod cli;
pub mod spec;
pub mod verify;
