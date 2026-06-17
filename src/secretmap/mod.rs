//! Secret mapping (ESV secret -> AM secret label) API and CLI support.
//!
//! Endpoint shapes and version headers are documented in
//! `docs/api/15-secret-mappings.md`.

pub mod api;
pub mod cli;
pub mod labels;
pub mod ops;
pub mod screen;
pub mod state;
pub mod view;
