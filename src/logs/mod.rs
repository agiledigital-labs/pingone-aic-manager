//! AIC audit/debug log fetch and API-key management vertical.
//!
//! Log authentication and endpoint shapes are documented in
//! `docs/api/08-logs.md`; CLI routing enters through [`cli`].

pub mod api;
pub mod cli;
pub mod ops;
pub mod state;
