//! Newline-delimited JSON protocol between CLI and agent.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Liveness probe. Cheap; doesn't require unlock.
    Ping,
    /// Decrypt `keys.enc` with the supplied master password and hold the JWK
    /// map in memory for the lifetime of the agent (or until idle timeout).
    Unlock { password: String },
    /// Drop the decrypted JWK map and any cached tokens.
    Lock,
    /// Report unlock state, project dir, tenant list, cached-token expirations,
    /// and time remaining before idle-lock.
    Status,
    /// Return a valid bearer token for the named tenant, minting one if the
    /// cached token is missing or within 60s of expiry.
    GetToken { tenant: String },
    /// Tell the agent to clean up the socket and exit.
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Pong {
        version: String,
        pid: u32,
    },
    Status(StatusInfo),
    Token {
        access_token: String,
        expires_at: i64,
    },
    Error {
        message: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StatusInfo {
    pub unlocked: bool,
    pub project_dir: String,
    pub tenants: Vec<String>,
    pub cached_tokens: Vec<CachedTokenInfo>,
    pub idle_remaining_secs: u64,
    pub idle_timeout_secs: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CachedTokenInfo {
    pub tenant: String,
    pub expires_at: i64,
}
