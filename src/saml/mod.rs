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

pub mod cli;
pub mod metadata;
