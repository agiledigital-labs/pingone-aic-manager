//! Trusted JWT Issuer setup for RFC 7523 user-token grants.
//!
//! API ground truth: [`docs/api/17-jwt-bearer-user-tokens.md`].
//!
//! File map:
//! - [`api`] = realm-scoped `TrustedJwtIssuer` HTTP wrappers.
//! - [`spec`] = TUI-free JWK-set, subject-list, and issuer-body transforms.
//! - [`ops`] = key generation, merge/retry orchestration, and vault storage.
//! - [`cli`] = `aic jwt-bearer` parsing and output.
//!
//! Private key records use the generic agent secret verbs and the
//! [`crate::config::VaultArtifact::JwtBearerKeys`] artifact.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentClient;
use crate::config::VaultArtifact;
use crate::{Error, Result};

pub mod api;
pub mod cli;
pub mod ops;
pub mod spec;

fn kind() -> &'static str {
    VaultArtifact::JwtBearerKeys.kind()
}

/// The private RSA JWK and its opaque tenant-side key id for one tenant.
///
/// The manual [`Debug`] implementation is required because this value crosses
/// the agent boundary and may appear in diagnostics while still containing the
/// signing key.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyRecord {
    pub kid: String,
    pub private_jwk: Value,
}

impl fmt::Debug for KeyRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyRecord")
            .field("kid", &self.kid)
            .field("private_jwk", &"<hidden>")
            .finish()
    }
}

/// Store the one local Trusted JWT key record for a tenant.
pub async fn put_key(agent: AgentClient, tenant: &str, record: &KeyRecord) -> Result<()> {
    agent
        .put_secret(kind(), tenant, serde_json::to_value(record)?)
        .await
}

/// Fetch the local Trusted JWT key record, if one has not been stored yet.
pub async fn get_key(agent: AgentClient, tenant: &str) -> Result<Option<KeyRecord>> {
    match agent.get_secret(kind(), tenant).await {
        Ok(value) => Ok(Some(serde_json::from_value(value)?)),
        Err(Error::SecretMissing { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::KeyRecord;

    #[test]
    fn key_record_round_trips_without_debugging_private_material() {
        let record = KeyRecord {
            kid: "opaque-kid".into(),
            private_jwk: json!({"kty": "RSA", "d": "do-not-print"}),
        };
        let serialized = serde_json::to_string(&record).unwrap();
        let round_trip: KeyRecord = serde_json::from_str(&serialized).unwrap();

        assert_eq!(record, round_trip);
        let debug = format!("{record:?}");
        assert!(debug.contains("<hidden>"));
        assert!(!debug.contains("do-not-print"));
    }
}
