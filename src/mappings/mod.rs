//! IDM sync mappings TUI vertical.
//!
//! File map:
//! - [`api`] = sync-mapping browse/recon HTTP wrappers.
//! - [`ops`] = background reconcile/pull and workspace-guard orchestration.
//! - [`screen`] / [`view`] / [`state`] = the TUI tab surface.
//!
//! Deliberate choice: there is no `cli.rs`. Script pull/push for embedded
//! mapping scripts stays in `crate::scripts::sync_mapping` via `aic script -k
//! sync`; this vertical is browse/reconcile only and also evicts the Scripts
//! list cache after a successful pull.
//!
//! Endpoint shapes are documented in `docs/api/16-sync-mappings.md`.

pub mod api;
pub mod ops;
pub mod screen;
pub mod state;
pub mod view;
