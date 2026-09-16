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
//! The read half ([`api`], [`spec`]) lists and shows entities over the
//! `realm-config/saml2` JSON API and exports their standard metadata. Two
//! things about it are worth knowing before touching either:
//!
//! - **`exportmetadata.jsp` is unauthenticated**, so `metadata export` runs
//!   against a locked daemon and must never be handed the service-account
//!   bearer.
//! - **A failed export is HTTP 200.** [`spec::classify_export`] is the only
//!   thing separating an error message from a file on disk.
//!
//! Nothing here writes. Create, import, delete and circle-of-trust membership
//! are later slices, and the `cotlist` split in `docs/api/06-saml.md` is why.

pub mod api;
pub mod cli;
pub mod metadata;
pub mod spec;
