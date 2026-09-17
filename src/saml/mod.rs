//! SAML 2.0 entity providers and circles of trust.
//!
//! The endpoint surface, the base64url entity-id encoding and — above all —
//! the circle-of-trust membership split that REST cannot show you are in
//! `docs/api/06-saml.md`. Read it before adding anything that writes.
//!
//! [`metadata`] is deliberately the offline half: it parses, inspects and
//! rewrites metadata XML without a tenant, a bearer or a network call, so the
//! transforms an import depends on can be tested against committed fixtures
//! rather than against a federation that breaks silently.
//!
//! The read half ([`api`], [`spec`]) lists and shows entities and circles of
//! trust over the `realm-config` JSON API and exports an entity's standard
//! metadata. Two things about it are worth knowing before touching either:
//!
//! - **`exportmetadata.jsp` is unauthenticated**, so `metadata export` runs
//!   against a locked daemon and must never be handed the service-account
//!   bearer.
//! - **A failed export is HTTP 200.** [`spec::classify_export`] is the only
//!   thing separating an error message from a file on disk.
//!
//! The write half is deliberately narrow: `create-hosted` and `delete`, and
//! nothing else. Both exist to cover for something AM will not do —
//! `create-hosted` imposes the `entityId` and `metaAlias` AM does not require
//! (`{}` is a **201** with a UUID name, and a role block without
//! `services.metaAlias` is a 500 naming no field), and `delete` reads the
//! circle-of-trust collection first because an entity `DELETE` silently
//! rewrites every CoT that listed the entity — then reads it **again**
//! afterwards, because AM performs that cascade and the delete response says
//! nothing about it. Neither write reports a post-state it did not measure:
//! [`spec::created_lines`] separates the id AM echoed from the role and alias
//! that were only sent, and [`spec::cascade_outcome_lines`] is a diff of two
//! reads.
//!
//! **There is no circle-of-trust *write* verb, and `cot list` / `cot show` are
//! not membership.** AM stores membership twice — the CoT document's
//! `trustedProviders`, which REST returns, and each entity's `cotlist`
//! extended metadata, which REST never exposes and which the runtime trust
//! check actually reads. [`spec::COT_MEMBERSHIP_CAVEAT`] is the sentence every
//! CoT rendering carries for that reason; it is load-bearing, not decoration.
//!
//! `import` and certificate rotation are later slices.

pub mod api;
pub mod cli;
pub mod metadata;
pub mod spec;
