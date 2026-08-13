//! Tenant offboarding: plan what is safe to remove when a tenant entry is
//! deleted.
//!
//! A service-account bearer cannot delete a service account
//! (`docs/api/00-auth.md`, "Deleting a service account") or a log API key
//! (`docs/api/08-logs.md`). The only sanctioned remote action is removing this
//! install's kid from the shared Trusted JWT issuer
//! ([`crate::jwtbearer::ops::remove_key_from_issuer`]).
//!
//! Slice B (CLI) and slice C (TUI) populate [`spec::Inventory`] from disk
//! and drive [`spec::plan`]; they must not re-decide what is safe. Two
//! tenant entries can point at one AIC tenant and share individual
//! credentials even when `sa_id`s differ, so every local purge is refused
//! when a survivor still depends on the same resource identity.
//!
//! File map:
//! - [`spec`] = input types and the sharing guard. No I/O.
//! - [`ops`] = probe + execute. The only network call is unpublishing the
//!   local Trusted JWT kid.
//! - [`cli`] = `aic ctx rm`.
//! - [`screen`] / [`view`] = the env-picker delete modal (`T` then `d`).

pub mod cli;
pub mod ops;
pub mod screen;
pub mod spec;
pub mod view;
