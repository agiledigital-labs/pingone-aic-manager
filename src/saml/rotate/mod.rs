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
//!   an ordinary rollover must never be able to reach it. It is a certificate
//!   change as well — mapping a label *replaces* the published default — so
//!   it confirms the way `stage` and `complete` do.
//! - **`stage`** adds an ESV secret version, **and it is to be treated as the
//!   cutover**: on the SP AuthnRequest-signing path the newest ENABLED version
//!   was signing by the first observation, ≤12 s after it was added (measured
//!   2026-09-22, `docs/api/06-saml.md`); IdP assertion signing is unmeasured
//!   and assumed the same. So the peer must already hold and trust the
//!   incoming certificate — which is what `--key-file` taking the operator's
//!   own key pair is for — and the two-certificate export that follows is a
//!   catch-up for a peer which refreshes metadata, not a window in which to
//!   prepare. It preserves the identifier and its mapping: certificate
//!   rotation is not entity repointing, which would leave an orphaned mapping
//!   nothing downstream removes. It asks for confirmation naming the incoming
//!   certificate. The way back is **emergency signer restoration** — re-add the
//!   old key pair, private key included, as a newer version, then disable the
//!   two superseded versions by hand (`spec::stage_plan_lines`).
//! - **`complete`** disables the old version, which stops publishing the old
//!   certificate and, on its usual path — the newest ENABLED version being the
//!   one kept — is **not expected to change the signer**. `--retain` naming the
//!   older certificate is the exception: it disables the signing version and
//!   is treated as a signer cutover back, and the plan says so. Separate from `stage`
//!   because AIC refuses to disable the latest version (`400 Cannot disable
//!   latest secret version`), so what can be retired is the old one — and
//!   because the interval in between is the catch-up, which is a human
//!   interval, not a timeout.
//!
//! ## One secret, one role
//!
//! A rollover is a change to the ESV **secret**, and AIC permits a secret
//! label to back several providers — so `stage` and `complete` mutate
//! something global while every check around them reads one entity. Each verb
//! therefore surveys **every realm** first ([`spec::survey_consumers`] refuses
//! exclusivity to a survey that skipped one) and refuses anything but "this
//! role and nobody else" ([`spec::exclusive_ok`]), which is the only minting
//! site for the `ExclusivityProof` every `authorize_*` consumes. It surveys
//! **again** inside `ops`' pre-write recheck, after the confirmation prompt,
//! and the write holds the proof minted there — so a consumer added while the
//! operator read the prompt is caught rather than cut over. The refusal names
//! the other consumers: the remedy differs per consumer, and "this secret is
//! shared" is not something an operator can act on.
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
//! The 2026-09-22 measurement narrows what is unknown without changing this:
//! on the SP path, the certificate *in use* was the newest ENABLED version's in
//! all five rounds — which was also the first one listed, so the two were not
//! separated — and [`spec::SIGNER_RULE`] states that as a rule to act on, not
//! as a mechanism. It did not make the export's
//! ordering an identity, and `complete` still derives a version to disable from
//! the record or from the operator, never from the order.
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
