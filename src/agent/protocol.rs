//! Newline-delimited JSON protocol between CLI and agent.

use serde::{Deserialize, Serialize};

/// A request plus the wire version understood by its sender.
///
/// Flattening preserves the existing `{"op": ...}` request shape and adds one
/// field, avoiding changes at every request construction site.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct WireRequest<T> {
    /// Zero denotes a legacy request that did not carry a version.
    #[serde(default)]
    pub protocol_version: u32,
    #[serde(flatten)]
    pub request: T,
}

impl<T> WireRequest<T> {
    pub fn current(request: T) -> Self {
        Self {
            protocol_version: super::PROTOCOL_VERSION,
            request,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Liveness probe. Cheap; doesn't require unlock.
    Ping,
    /// Cache an already-derived 32-byte DEK (base64). The CLI and TUI both
    /// derive the DEK themselves (Argon2 from a password, or hmac-secret
    /// from a security key) via `crate::vault::auth::*` and hand it to the agent —
    /// the daemon never sees the password or PIN. There used to be a
    /// `Request::Unlock { password }` that did the Argon2 work inside the
    /// daemon; it was removed to keep one canonical unlock path.
    PutDek { dek_b64: String },
    /// Load `.aic/keys.plain` into the agent. Used when the user
    /// opted out of encryption (`settings.encrypt_keys = false`); there's
    /// no DEK, the file is already plaintext, and the agent just holds
    /// the JWK map in memory. CLI/TUI call this in place of `PutDek` when
    /// they detect plain mode on startup.
    UnlockPlain,
    /// Return the cached DEK (`Response::Dek`) or `Response::Locked` if there
    /// is no encrypted session. The TUI calls this on startup to skip the
    /// unlock screen when the daemon already holds a DEK. **Plain mode
    /// also answers `Locked`** — there's no DEK to return — so callers
    /// shouldn't infer "agent is locked" from a `Locked` reply without
    /// checking `Status::unlocked` too.
    GetDek,
    /// Drop the cached DEK, JWK map, and any cached tokens.
    Lock,
    /// Report unlock state, project dir, tenant list, cached-token expirations,
    /// and time remaining before idle-lock.
    Status,
    /// Replace the daemon idle-lock timeout in seconds.
    SetIdleTimeout { secs: u64 },
    /// Return a valid bearer token for the named tenant, minting one if the
    /// cached token is missing or within 60s of expiry.
    GetToken { tenant: String },
    /// Store or replace an encrypted vault artifact's per-tenant secret. `kind`
    /// names the artifact ([`crate::config::VaultArtifact::kind`], e.g.
    /// `"log-keys"`); `value` is the opaque JSON payload the feature serialised.
    PutSecret {
        kind: String,
        tenant: String,
        value: serde_json::Value,
    },
    /// Return the stored secret for `(kind, tenant)`.
    GetSecret { kind: String, tenant: String },
    /// Remove the stored secret for `(kind, tenant)`.
    RemoveSecret { kind: String, tenant: String },
    /// Proxy a tenant-scoped AIC HTTP call. The daemon owns the bearer
    /// token cache + connection pool, so the TUI and CLI both go through
    /// here for every read and write — keeps token/HTTP machinery in one
    /// place. `confirmed_prod` is forwarded to the prod-confirm guard;
    /// callers ask the user (modal in TUI, `--yes` flag in CLI) first and
    /// pass `true` when greenlit.
    ApiCall {
        tenant: String,
        method: String,
        path: String,
        body: Option<serde_json::Value>,
        confirmed_prod: bool,
        /// Optional content type for non-JSON request bodies. The body is
        /// carried as a JSON string when this is set.
        #[serde(default)]
        content_type: Option<String>,
        /// Override the `Accept-API-Version` header for this call. `None`
        /// keeps the default `resource=1.0` (ESVs/secrets). AM scripts need
        /// `protocol=2.0,resource=1.0`; IDM config endpoints set their own.
        #[serde(default)]
        api_version: Option<String>,
        /// Optional optimistic-concurrency revision for verified conditional
        /// write families. Most API calls leave this unset.
        #[serde(default)]
        if_match: Option<String>,
    },
    /// Tell the agent to clean up the socket and exit.
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    /// Reply to `GetSecret` — the opaque JSON payload for `(kind, tenant)`.
    Secret {
        value: serde_json::Value,
    },
    /// Reply to `GetSecret` when no secret is stored for `(kind, tenant)`.
    /// Distinct from `Error` so the caller can map it to
    /// [`crate::Error::SecretMissing`] and attach its own remediation text.
    SecretMissing {
        kind: String,
        tenant: String,
    },
    /// Reply to `GetDek` when a DEK is cached. Base64 of 32 raw bytes.
    Dek {
        dek_b64: String,
    },
    /// Reply to `ApiCall` — parsed JSON body from a successful AIC response.
    Json {
        value: serde_json::Value,
    },
    /// AIC returned a non-success HTTP status. Distinct from `Error` so the
    /// caller can pattern-match on it (e.g. retry on 401, distinguish 404
    /// from a protocol failure).
    ApiError {
        status: u16,
        body: String,
    },
    /// `ApiCall` was made with `confirmed_prod: false` against a prod-themed
    /// tenant. Caller should surface a confirmation modal (TUI) or refuse
    /// without `--yes` (CLI) and retry with `confirmed_prod: true`.
    ProdConfirmRequired,
    /// Reply to `GetDek` (or any op that needs an unlocked session) when the
    /// agent isn't holding a DEK. Distinct from `Error` so the TUI can quietly
    /// fall through to the unlock screen.
    Locked,
    /// The request used a different CLI-to-agent wire protocol version.
    ProtocolMismatch {
        expected: u32,
        received: u32,
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
