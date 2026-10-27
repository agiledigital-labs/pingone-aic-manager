//! `aic promotion`: lock state, promotion status and reports (including the
//! provisional one), the live `verify`, and the write cycle — `lock`,
//! `dry-run`, `promote`, `unlock`. The writes are gated in `lifecycle.rs`:
//! each starts only from a measured state and needs a permit that state and
//! consent mint. Endpoint shapes and the measurements behind every
//! classification are in `docs/api/23-promotion.md`.

pub mod api;
pub mod cli;
pub mod lifecycle;
pub mod spec;
pub mod verify;
