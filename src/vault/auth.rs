//! Shared authentication helpers used by both the CLI and the TUI.
//!
//! The actual crypto lives in `crate::config::unlock_with_password` and
//! `crate::config::unlock_with_security_key` — this module wraps them in
//! async-friendly shapes (Argon2 and FIDO2 both block) and provides the
//! "after a successful unlock, hand the DEK to the agent" step. Anything
//! UI-specific (rpassword prompts, ratatui input modes) stays in the
//! respective frontends; everything they share lives here.

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use crate::agent::{AgentClient, Request as AgentRequest, Response as AgentResponse};
use crate::config::crypto::Dek;
use crate::config::wraps::WrapsFile;
use crate::config::{self, VaultArtifact};
use crate::logs::LogKeyMap;
use crate::{Error, Result};

/// Decrypt the per-tenant log API key map with the just-derived DEK. Kept here
/// so both unlock paths hydrate `UnlockOk` the same way.
pub(crate) fn decrypt_log_keys(dek: &Dek) -> Result<LogKeyMap> {
    match config::load_artifact_bytes(VaultArtifact::LogKeys, Some(dek))? {
        Some(bytes) if !bytes.is_empty() => Ok(serde_json::from_slice(&bytes)?),
        _ => Ok(LogKeyMap::new()),
    }
}

/// Successful unlock payload. The TUI also passes this through an event
/// channel, hence `pub` and `Clone`-free fields (the contents move).
#[derive(Debug)]
pub struct UnlockOk {
    pub dek: Dek,
    pub jwks: HashMap<String, serde_json::Value>,
    pub log_keys: LogKeyMap,
}

/// Unlock with a master password. Argon2 is pushed to a blocking thread.
pub async fn unlock_password(password: String) -> Result<UnlockOk> {
    let (dek, jwks) = tokio::task::spawn_blocking(move || config::unlock_with_password(&password))
        .await
        .map_err(|e| Error::Crypto(format!("unlock task: {e}")))??;
    let log_keys = decrypt_log_keys(&dek)?;
    Ok(UnlockOk {
        dek,
        jwks,
        log_keys,
    })
}

/// Unlock by tapping any enrolled security key. Builds an allowList from
/// every credential in `wraps_file` and makes one `getAssertion` call —
/// the device picks the credential it actually holds. PIN goes to the
/// device once, regardless of how many keys are enrolled.
pub async fn unlock_security_key(wraps_file: WrapsFile, pin: String) -> Result<UnlockOk> {
    let (dek, jwks) = tokio::task::spawn_blocking(move || {
        let pin_opt = if pin.is_empty() {
            None
        } else {
            Some(pin.as_str())
        };
        config::unlock_with_security_key(&wraps_file, pin_opt)
    })
    .await
    .map_err(|e| Error::Crypto(format!("unlock task: {e}")))??;
    let log_keys = decrypt_log_keys(&dek)?;
    Ok(UnlockOk {
        dek,
        jwks,
        log_keys,
    })
}

/// Hand a freshly-derived DEK to the agent so subsequent processes (other
/// CLI invocations, the TUI re-opening) skip the unlock screen. Returns
/// `Ok(())` on success; the caller decides whether agent failures are
/// fatal — the TUI tends to log-and-continue, the CLI surfaces them.
pub async fn put_dek_to_agent(dek: &Dek) -> Result<()> {
    let dek_b64 = B64.encode(dek.as_bytes());
    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&AgentRequest::PutDek { dek_b64 }).await? {
        AgentResponse::Ok => Ok(()),
        AgentResponse::Error { message } => {
            Err(Error::Auth(format!("agent rejected DEK: {message}")))
        }
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

/// Tell the agent to load `.aic-edit/keys.plain` — the no-encryption unlock
/// path. Surfaces a clear error when the file is missing (e.g. the user is
/// in encrypted mode but called this anyway) so the caller can show
/// something actionable.
pub async fn unlock_plain_agent() -> Result<()> {
    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&AgentRequest::UnlockPlain).await? {
        AgentResponse::Ok => Ok(()),
        AgentResponse::Error { message } => Err(Error::Auth(format!(
            "agent rejected UnlockPlain: {message}"
        ))),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}
