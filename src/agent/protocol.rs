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
    /// Proxy a tenant-scoped AIC HTTP call through the daemon's token cache
    /// and connection pool. See [`ApiCallRequest`].
    ApiCall(ApiCallRequest),
    /// Tell the agent to clean up the socket and exit.
    Shutdown,
}

/// Proxy a tenant-scoped AIC HTTP call. The daemon owns the bearer
/// token cache + connection pool, so the TUI and CLI both go through
/// here for every read and write — keeps token/HTTP machinery in one
/// place.
///
/// Carried as [`Request::ApiCall`]. With an internally-tagged enum, serde
/// flattens these fields into the same object as `"op":"api_call"` — the
/// JSON shape is the wire contract, pinned by the tests below.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ApiCallRequest {
    pub tenant: String,
    pub method: String,
    pub path: String,
    pub body: Option<serde_json::Value>,
    /// Forwarded to the prod-confirm guard; callers ask the user (modal in
    /// TUI, `--yes` flag in CLI) first and pass `true` when greenlit.
    pub confirmed_prod: bool,
    /// Optional content type for non-JSON request bodies. The body is
    /// carried as a JSON string when this is set.
    #[serde(default)]
    pub content_type: Option<String>,
    /// Override the `Accept-API-Version` header for this call. `None`
    /// keeps the default `resource=1.0` (ESVs/secrets). AM scripts need
    /// `protocol=2.0,resource=1.0`; IDM config endpoints set their own.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Optional optimistic-concurrency revision for verified conditional
    /// write families. Most API calls leave this unset.
    #[serde(default)]
    pub if_match: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-shape pin: a fully-populated `ApiCall` must serialise through
    /// `WireRequest::current` — the bytes that actually go on the socket —
    /// to this exact object. Assert against a pasted literal, not a
    /// round-trip — a round-trip happily absorbs a field rename, which is
    /// the failure this test exists to catch.
    #[test]
    fn api_call_serialises_fully_populated_to_today_s_wire_shape() {
        let wire = WireRequest::current(Request::ApiCall(ApiCallRequest {
            tenant: "sandbox".into(),
            method: "PUT".into(),
            path: "/openidm/internal/role/x".into(),
            body: Some(serde_json::json!({"name": "ops"})),
            confirmed_prod: true,
            content_type: Some("application/json".into()),
            api_version: Some("resource=1.0".into()),
            if_match: Some("\"1\"".into()),
        }));

        let actual = serde_json::to_value(&wire).expect("serialise");
        let expected = serde_json::json!({
            "protocol_version": 2,
            "op": "api_call",
            "tenant": "sandbox",
            "method": "PUT",
            "path": "/openidm/internal/role/x",
            "body": {"name": "ops"},
            "confirmed_prod": true,
            "content_type": "application/json",
            "api_version": "resource=1.0",
            "if_match": "\"1\"",
        });
        assert_eq!(actual, expected);
    }

    /// Minimal call: body and the three `#[serde(default)]` optionals are
    /// unset. Serde still emits them as `null` (no `skip_serializing_if`);
    /// that is today's shape and must stay.
    #[test]
    fn api_call_serialises_minimal_to_today_s_wire_shape() {
        let wire = WireRequest::current(Request::ApiCall(ApiCallRequest {
            tenant: "sandbox".into(),
            method: "GET".into(),
            path: "/environment/variables".into(),
            body: None,
            confirmed_prod: false,
            content_type: None,
            api_version: None,
            if_match: None,
        }));

        let actual = serde_json::to_value(&wire).expect("serialise");
        let expected = serde_json::json!({
            "protocol_version": 2,
            "op": "api_call",
            "tenant": "sandbox",
            "method": "GET",
            "path": "/environment/variables",
            "body": null,
            "confirmed_prod": false,
            "content_type": null,
            "api_version": null,
            "if_match": null,
        });
        assert_eq!(actual, expected);
    }

    /// An older CLI that omits the three `#[serde(default)]` fields (and
    /// never heard of `if_match`) must still parse. Deserialises as
    /// `Request` — the same type the daemon matches on after peeling
    /// `WireRequest`.
    #[test]
    fn api_call_deserialises_today_s_wire_shape_with_optionals_absent() {
        let from_an_older_client = serde_json::json!({
            "op": "api_call",
            "tenant": "sandbox",
            "method": "GET",
            "path": "/environment/variables",
            "body": null,
            "confirmed_prod": false,
        });

        let decoded: Request = serde_json::from_value(from_an_older_client)
            .expect("older client payload without optional fields must parse");

        match decoded {
            Request::ApiCall(ApiCallRequest {
                tenant,
                method,
                path,
                body,
                confirmed_prod,
                content_type,
                api_version,
                if_match,
            }) => {
                assert_eq!(tenant, "sandbox");
                assert_eq!(method, "GET");
                assert_eq!(path, "/environment/variables");
                assert!(body.is_none());
                assert!(!confirmed_prod);
                assert!(content_type.is_none());
                assert!(api_version.is_none());
                assert!(if_match.is_none());
            }
            other => panic!("expected an api_call, got {other:?}"),
        }
    }

    /// The reason an *additive* protocol change still needs a `PROTOCOL_VERSION`
    /// bump: serde ignores fields it does not know. An older daemon handed a
    /// request carrying a field it was never compiled with does not complain —
    /// it drops the field and does the work without it. For `if_match` that
    /// means performing an unconditional write while the caller believes it is
    /// protected by a revision precondition, which is worse than an error.
    ///
    /// If this test ever fails because unknown fields become an error, additive
    /// changes stop needing a version bump and this constraint can be revisited.
    #[test]
    fn unknown_fields_are_silently_ignored_so_additive_changes_need_a_bump() {
        let from_a_newer_client = serde_json::json!({
            "op": "api_call",
            "tenant": "sandbox",
            "method": "PUT",
            "path": "/openidm/internal/role/x",
            "body": null,
            "confirmed_prod": false,
            "content_type": null,
            "api_version": null,
            "a_field_this_build_has_never_heard_of": "surprise",
        });

        let decoded: Request = serde_json::from_value(from_a_newer_client)
            .expect("serde accepted the request despite the unknown field");

        match decoded {
            Request::ApiCall(ApiCallRequest { if_match, .. }) => assert!(
                if_match.is_none(),
                "a field this build does not know cannot populate anything"
            ),
            other => panic!("expected an api_call, got {other:?}"),
        }
    }
}
