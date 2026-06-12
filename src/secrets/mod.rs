//! ESV secrets: versioned, write-only tenant configuration with list, create,
//! version management, delete, undo, TUI, and CLI surfaces.
//!
//! The feature is split by responsibility:
//! - [`state`] owns pure data, plans, rows, and derived UI state.
//! - [`ops`] runs mutations, refresh/result application, and undo work.
//! - [`screen`] owns nested input modes, events, and key handling.
//! - [`view`] renders the secrets half of the ESV dashboard.
//! - [`cli`] implements `aic esv secret` commands.
//!
//! The shared-poll seam is deliberate: [`crate::esv::ops::apply_refresh`]
//! forwards the secret payload to [`ops::apply_refresh`]. Secret values are
//! write-only, so successful writes are reconciled by refetching metadata
//! rather than caching plaintext.
//!
//! API ground truth: `docs/api/03-esvs.md`. Secret values are never returned;
//! creation encoding and placeholder behavior are immutable, while value
//! changes create new versions. See CLAUDE.md for project-wide rules.

pub mod cli;
pub mod ops;
pub mod screen;
pub mod state;
pub mod view;
