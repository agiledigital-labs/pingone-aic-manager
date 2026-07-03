//! IDM managed-object record store and query vertical.
//!
//! File map:
//! - [`api`] = verified IDM record/config HTTP wrappers.
//! - [`cli`] = `aic idm` read-only inspection and SQL runner.
//! - [`db`] = local SQLite store, schema derivation, and record helpers.
//! - [`ops`] = background refresh and cache orchestration.
//! - [`screen`] / [`view`] / [`state`] = the TUI tab surface.
//!
//! The TUI tab is a registered placeholder: `view`/`ops`/`screen` are stubs
//! and the feature surface is `aic idm` (`cli.rs`). `aic idm` still uses the
//! managed seam for schema/relationship inspection via
//! `crate::managed::api::get_managed` and
//! `crate::managed::state::is_relationship_property`.
//!
//! Managed-object schema and record endpoint shapes are documented in
//! `docs/api/10-managed-objects.md`.

pub mod api;
pub mod cli;
pub mod db;
pub mod ops;
pub mod screen;
pub mod state;
pub mod view;
