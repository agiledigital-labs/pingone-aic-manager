//! `aic saml rotate` — roll the certificate a SAML role signs with.
//!
//! The finding this verb is built on is that **an AIC SAML signing certificate
//! is not in the SAML entity at all** (`docs/api/06-saml.md`). The entity holds
//! a *label identifier*; the key lives in AM's secret store under
//! `am.applications.federation.entity.providers.saml2.<identifier>.signing`,
//! and on AIC that label is backed by an **ESV secret**. So there is nothing to
//! upload and no metadata to push: the rotation is an ESV secret **version**
//! operation with one SAML field in front of it, and it spans three feature
//! directories — `saml/`, `esv/` and `secretmap/`.
//!
//! Measured end to end, with no restart and no cache flush, provided the ESV
//! secret was created with placeholders off: add a version and the export
//! carries **two** `<KeyDescriptor use="signing">`; disable the old version and
//! it drops back to one, within single-digit seconds.
//!
//! ## Four verbs, and why the work is split that way
//!
//! - **`status`** reads and correlates, and writes nothing. It is also the
//!   only honest place to say what cannot be known — see below.
//! - **`init`** is the one-time setup that has to touch all three stores: it
//!   `PUT`s the entity with a `secretIdIdentifier`, creates the `pem` ESV
//!   secret, and maps the label. It is separate from `stage` because it is the
//!   only path that performs a **full-replace entity `PUT`** — the call that
//!   answers 200 and deletes a whole role block when the body is wrong — and
//!   an ordinary rollover must never be able to reach it.
//! - **`stage`** adds an ESV secret version. It preserves the identifier and
//!   its mapping: certificate rotation is not entity repointing, which would
//!   leave an orphaned mapping nothing downstream removes.
//! - **`complete`** disables the old version. Separate from `stage` because
//!   the peer has to load the two-certificate metadata in between, and that is
//!   a human interval, not a timeout.
//!
//! **Nothing here destroys anything.** `complete` disables, which
//! `aic esv secret enable` undoes. Destroying the version and deleting the
//! mapping are irreversible, and they stay explicit and elsewhere
//! (`aic esv secret destroy`, `aic secretmap remove`).
//!
//! ## The thing `status` cannot know
//!
//! During the two-certificate window, **nothing readable says which published
//! certificate came from which ESV secret version**: secret values are
//! write-only and AM emits no `<ds:KeyName>`. The export lists the newest
//! ENABLED version first, but ordering is a convention, not an identity, and a
//! `complete` that trusted it would be one convention-change away from
//! retiring the new certificate and keeping the old. So [`journal`] records the
//! pairing at stage time — confirmed against the tenant's own export, never
//! from the bytes that were sent — and `complete` without such a record
//! requires **both** halves rather than guessing: `--retain <sha256>` for the
//! certificate to keep and `--disable-version <n>` for the version to disable.
//! A fingerprint alone names no version, so one without the other leaves the
//! choice to ordering, which is the thing being refused.
//!
//! With a record, the derivation is the command's and `--disable-version` may
//! only corroborate it — a named version that disagrees is refused, because
//! the record is the only evidence there is and the plan the flag would
//! produce disables the certificate `--retain` just named.
//!
//! ## Layout
//!
//! The vertical's standard seams, one directory deeper, because a rotation is
//! a feature that happens to live inside `saml/` rather than another view of
//! an entity: [`pem`] and [`spec`] are tenant-free, [`journal`] is the local
//! record, [`ops`] is the only module that performs I/O across the three
//! stores, and [`cli`] is the parser.

pub mod cli;
pub mod journal;
pub mod ops;
pub mod pem;
pub mod spec;
