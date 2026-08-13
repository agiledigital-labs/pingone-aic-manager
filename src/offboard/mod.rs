//! Tenant offboarding: plan what is safe to remove when a tenant entry is
//! deleted.
//!
//! A service-account bearer cannot delete a service account
//! (`docs/api/00-auth.md`, "Deleting a service account") or a log API key
//! (`docs/api/08-logs.md`). The only sanctioned remote action is removing this
//! install's kid from the shared Trusted JWT issuer
//! ([`crate::jwtbearer::ops::remove_key_from_issuer`]).
//!
//! This slice is the pure planner. Slice B (CLI) and slice C (TUI) populate
//! [`spec::Inventory`] from disk and drive [`spec::plan`]; they must not
//! re-decide what is safe. Two tenant entries can point at one AIC tenant and
//! share individual credentials even when `sa_id`s differ, so every local
//! purge is refused when a survivor still depends on the same resource
//! identity.
//!
//! File map:
//! - [`spec`] = input types and the sharing guard. No I/O.

pub mod spec;
