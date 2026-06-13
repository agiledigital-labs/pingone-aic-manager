//! IDM managed objects: the per-tenant schema (object types, properties,
//! event hooks) behind `managed/alpha_user` and friends.
//!
//! The feature is split by responsibility:
//! - [`api`] contains the verified config wrappers (whole-document GET/PUT —
//!   the `managed` config has no `_rev` and no partial patch).
//! - [`cli`] implements `aic managed` (read-only inspection in this slice;
//!   schema property editing is the planned follow-up).
//!
//! **Hook scripts are synced by the scripts feature, not here**: they are
//! `Kind::IdmManagedHook` units addressed as `managed/<object>.<hookKey>`
//! (e.g. `aic script pull managed/alpha_user.onCreate` →
//! `idm/managed/alpha_user/onCreate.cjs`, typed + linted like any workspace
//! script). See `crate::scripts::managed_hooks` for the shared-document
//! read-modify-write semantics.
//!
//! API ground truth: `docs/api/10-managed-objects.md` (hook bindings:
//! `docs/api/bindings/managed-hooks-idm.json`). Quirks that shape this
//! module: PUT replaces the entire document; a 200 applies with a lag
//! (~seconds); file-backed hooks are read-only markers.

pub mod api;
pub mod cli;
