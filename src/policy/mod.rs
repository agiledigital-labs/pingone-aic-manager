//! AM policies: resource types, policy sets and the PDP.
//!
//! Endpoint shapes, the three-way create asymmetry, the subject/condition
//! catalogs and the measured URL-wildcard semantics are all in
//! `docs/api/21-am-policies.md`. Two of those are load-bearing here:
//!
//! - **Create is not one verb.** Resource types create with `PUT /{id}`;
//!   policies and policy sets 404 on a `PUT` to a name that does not exist and
//!   create only with `POST ?_action=create`. [`api`] hides that behind
//!   `upsert_*` so no caller has to remember which is which.
//! - **`actions: {}` is ambiguous** — no policy applied, and the API will not
//!   say whether the resource missed, the subject failed or a condition did.
//!   [`spec::diagnose`] reconstructs the likely reason from the set, its
//!   resource types and its policies, which is the whole reason `aic policy
//!   eval` exists rather than a `curl` alias.

pub mod api;
pub mod cli;
pub mod spec;
