//! Environment variables (ESVs): tenant-wide configuration values with list,
//! edit, delete, apply/restart, undo, TUI, and CLI surfaces.
//!
//! The feature is split by responsibility:
//! - [`api`] contains the verified AIC HTTP wrappers.
//! - [`state`] owns pure data, plans, outcomes, and derived UI state.
//! - [`ops`] runs refresh/write/restart/undo work and applies async results.
//! - [`screen`] owns nested input modes, events, and key handling.
//! - [`view`] renders the variables half of the ESV dashboard.
//! - [`cli`] implements `aic esv` variable commands.
//!
//! ESV variables and secrets deliberately share one poll: [`ops::apply_refresh`]
//! forwards the secret payload to [`crate::screens::secret::apply_refresh`].
//! Secrets remain under `screens::secret` until their next migration phase.
//!
//! API ground truth: `docs/api/03-esvs.md`. Variables have no `_rev`; writes
//! use content comparison against the last observed body for conflict checks,
//! following the project-wide ESV rule in CLAUDE.md §5.

pub mod api;
pub mod cli;
pub mod ops;
pub mod screen;
pub mod state;
pub mod view;
