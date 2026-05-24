//! Newline-delimited JSON protocol between CLI and agent.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Liveness probe. Cheap; doesn't require unlock.
    Ping,
    /// Decrypt `keys.enc` with the supplied master password and hold the DEK
    /// (and the derived JWK map) in memory until idle timeout or `Lock`.
    Unlock { password: String },
    /// Cache an already-derived 32-byte DEK (base64). Used by the TUI after a
    /// security-key unlock — the device produces the DEK locally, so the
    /// daemon never sees the PIN or HMAC inputs.
    PutDek { dek_b64: String },
    /// Return the cached DEK (`Response::Dek`) or `Response::Locked` if there
    /// is no session. The TUI calls this on startup to skip the unlock screen
    /// when the daemon already holds a DEK.
    GetDek,
    /// Drop the cached DEK, JWK map, and any cached tokens.
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
    /// Reply to `GetDek` when a DEK is cached. Base64 of 32 raw bytes.
    Dek {
        dek_b64: String,
    },
    /// Reply to `GetDek` (or any op that needs an unlocked session) when the
    /// agent isn't holding a DEK. Distinct from `Error` so the TUI can quietly
    /// fall through to the unlock screen.
    Locked,
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
