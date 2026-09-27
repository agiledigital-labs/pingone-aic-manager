//! `aic saml cert` — list, add and remove a **remote** entity's signing
//! certificates, and the certificate-update half of `aic saml import`.
//!
//! Built on one measured primitive (`docs/api/06-saml.md`, "Updating a remote
//! entity's certificates"): `?_action=importEntity` with
//! `updateType: UPDATE_CERTIFICATES` replaces every `<KeyDescriptor>` of each
//! role the document names, encryption included, and ignores everything
//! else in a role the entity already has. It is also careless in four ways a
//! client has to cover for, and each is a guard in [`spec`]:
//!
//! - **The document's `entityID` is the target**, and the remote collection
//!   writes to a hosted entity too — so the entity is confirmed `remote` from
//!   the realm's list first, and the sent document's `entityID` must match.
//! - **A role the entity lacks is added whole**, endpoints and all — so the
//!   sent document's roles must be a subset of the entity's.
//! - **Zero signing certificates is accepted** — so every role keeps at least
//!   one, with no `--force`.
//! - **Encryption keys are replaced too** — so the document sent is the
//!   entity's **own export** with signing key descriptors spliced in or out
//!   ([`crate::saml::metadata::key_layout`]), never re-serialised and, for an
//!   import, never the uploaded file. A certificate from a file reaches the
//!   document as a key descriptor written fresh from its DER.
//!
//! Then the rules every write here follows: the export is re-read
//! immediately before the write and a single differing byte refuses it; the
//! write needs a [`spec::CertPermit`] only [`spec::authorize`] mints, so a
//! `--dry-run` cannot reach it; and success is reported only after a fresh
//! export publishes exactly the planned `(role, use, sha256)` **set**. The
//! entity JSON cannot confirm anything — certificates are not in it, and its
//! `_rev` does not move.
//!
//! Hosted entities are out of scope: their certificates come from the
//! secret store, and [`crate::saml::rotate`] is the verb that owns them.

pub mod cli;
pub mod import;
pub mod ops;
pub mod spec;
#[cfg(test)]
mod testdoc;
