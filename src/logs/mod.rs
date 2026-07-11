//! AIC audit/debug log fetch, local sync, and API-key management vertical.
//!
//! File map:
//! - [`api`] = log-API transport. It deliberately bypasses the agent
//!   `ApiCall` path because the logs API uses separate `x-api-key` /
//!   `x-api-secret` auth and rate limits from the bearer-authenticated API
//!   families.
//! - [`cli`] = `aic logs` command parsing and dispatch.
//! - [`db`] = optional per-tenant DuckDB cache for raw events, sync cursors,
//!   compact state, journey rollup tables, and offline search.
//! - [`ops`] = sync engine and noise filter.
//! - [`journey`] = `aic logs compact` rollup logic.
//!
//! Cross-feature seams:
//! - Log keys live in the encrypted vault artifact
//!   [`crate::config::VaultArtifact::LogKeys`], persisted as a JSON map and
//!   reached over the agent via `Request::{PutSecret,GetSecret,RemoveSecret}`.
//! - Onboard bootstrap mints log API keys from an admin session via
//!   [`crate::onboard::bootstrap::mint_log_key_via_session`].
//! - Log-key persistence uses the shared config registry.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::AgentClient;
use crate::config::VaultArtifact;
use crate::{Error, Result};

pub mod api;
pub mod cli;
#[cfg(feature = "logs-store")]
pub mod db;
#[cfg(feature = "logs-store")]
pub mod journey;
pub mod ops;

/// The vault-artifact `kind` these keys live under.
fn kind() -> &'static str {
    VaultArtifact::LogKeys.kind()
}

/// Attach the log-key-specific remediation to the shared "no secret stored"
/// error — the user-facing string that used to live baked into the error enum.
fn with_remediation(err: Error) -> Error {
    match err {
        Error::SecretMissing { tenant, .. } => Error::Config(format!(
            "no log API key stored for tenant {tenant} — run `aic logs key set`"
        )),
        other => other,
    }
}

/// Store or replace a tenant's log API key pair via the agent's generic secret
/// verbs. `agent` is consumed (one request per connection).
pub async fn put_log_key(agent: AgentClient, tenant: &str, pair: &LogKeyPair) -> Result<()> {
    agent
        .put_secret(kind(), tenant, serde_json::to_value(pair)?)
        .await
}

/// Fetch a tenant's log API key pair. A missing key surfaces the
/// `aic logs key set` remediation.
pub async fn get_log_key(agent: AgentClient, tenant: &str) -> Result<LogKeyPair> {
    let value = agent
        .get_secret(kind(), tenant)
        .await
        .map_err(with_remediation)?;
    Ok(serde_json::from_value(value)?)
}

/// Remove a tenant's stored log API key pair.
pub async fn remove_log_key(agent: AgentClient, tenant: &str) -> Result<()> {
    agent.remove_secret(kind(), tenant).await
}

/// A console-issued log API key pair for one tenant. The secret is shown only
/// once at creation, so we persist it; [`Debug`] is hand-written to keep it out
/// of logs and panics.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogKeyPair {
    pub api_key_id: String,
    pub api_key_secret: String,
}

impl std::fmt::Debug for LogKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogKeyPair")
            .field("api_key_id", &self.api_key_id)
            .field("api_key_secret", &"<hidden>")
            .finish()
    }
}

/// Per-tenant log API keys, keyed by tenant name.
pub type LogKeyMap = HashMap<String, LogKeyPair>;
