//! SAML 2.0 entity providers and circles of trust.
//!
//! The endpoint surface, the base64url entity-id encoding and — above all —
//! the circle-of-trust membership split that REST cannot show you are in
//! `docs/api/06-saml.md`. Read it before adding anything that writes.
//!
//! [`metadata`] is deliberately the offline half: it parses, inspects and
//! rewrites metadata XML without a tenant, a bearer or a network call, so the
//! transforms an import depends on can be tested against committed fixtures
//! rather than against a federation that breaks silently. [`metadata::MetadataBundle`]
//! is the import-shaped view of a file — **n** entities, because one
//! `EntitiesDescriptor` aggregate is one call and n creations.
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
//! `import` is the verb that can break a live federation, and the shape of it
//! follows from one fact: **`cotlist` damage cannot be detected, before or
//! after.** So the command refuses rather than recovers — every entity id is
//! preflighted against both collections and any collision refuses the whole
//! operation, with no `--force` that deletes and re-imports (the delete
//! cascades through every circle of trust and the `cotlist` cannot be read
//! back) and no `--cot` (AM discards it with a 200). `--dry-run` stops by
//! holding no [`spec::ImportPermit`] rather than by returning in front of the
//! write. And the report claims only what AM said: `importedEntities`
//! compared as a **set** against the parsed ids, a fresh list of the realm
//! after a failure because a failed aggregate import is not a rollback, and
//! [`spec::IMPORT_COTLIST_CAVEAT`] in place of the post-import tick nothing
//! could stand behind.
//!
//! The other writes are deliberately narrow: `create-hosted` and `delete`.
//! Both exist to cover for something AM will not do —
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
//! **Certificate rotation lives in [`rotate`]**, and the reason it is a
//! submodule rather than another verb here is that it is barely about SAML: an
//! AIC SAML signing certificate is not in the entity at all. The entity holds a
//! `secretIdIdentifier`, which is a label into AM's secret store, and on AIC
//! that label is backed by an ESV secret — so a rollover adds an ESV secret
//! *version* and spans `saml/`, `esv/` and `secretmap/`. There is nothing to
//! upload and no metadata to push.

pub mod api;
pub mod cli;
pub mod metadata;
pub mod rotate;
pub mod spec;
