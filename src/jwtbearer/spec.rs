//! TUI-free input types and pure Trusted JWT Issuer transforms.
//!
//! In particular, [`merge_jwk_set`] and [`remove_from_jwk_set`] are deliberately
//! independent of the HTTP layer so key management shares parser and
//! validation rules.

use base64::Engine as _;
use jsonwebtoken::Header;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use url::form_urlencoded::Serializer;
use uuid::Uuid;

use crate::config::TenantTheme;
use crate::{Error, Result};

const SERVER_FIELDS: [&str; 4] = ["_id", "_rev", "_type", "_provider"];

/// OAuth2 client authentication used at the token endpoint.
///
/// `private_key_jwt` can be added as another variant without changing the
/// meaning of the `--client-auth` flag or the request-building boundary.
/// The default is `client_secret_post`, which is **not** AM's own client
/// template default (`client_secret_basic`) and not the method RFC 6749 §2.3.1
/// prefers. It is chosen so `aic oauth create` and `aic auth` agree out of the
/// box: `create` writes the matching method, and this default keeps `aic auth`
/// behaving as it did before the flag existed. Pass
/// `--client-auth client-secret-basic` for a client configured AM's way.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClientAuthMethod {
    #[default]
    ClientSecretPost,
    ClientSecretBasic,
}

/// A complete token-endpoint request, including any client-owned credential.
#[derive(Debug, PartialEq, Eq)]
pub struct TokenRequest {
    pub body: String,
    pub authorization: Option<String>,
}

/// Parse a string-valued AM `jwkSet`, treating null and empty values as an
/// empty set. Malformed JSON or a malformed JWKS shape is reported so setup
/// cannot silently discard someone else's keys.
pub fn parse_jwk_set(existing: Option<&str>) -> Result<Value> {
    let Some(existing) = existing.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(json!({"keys": []}));
    };
    let value: Value = serde_json::from_str(existing)?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("Trusted JWT issuer jwkSet must be a JSON object".into()))?;
    let keys = object
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::Config("Trusted JWT issuer jwkSet must contain a keys array".into())
        })?;
    if keys.iter().any(|key| !key.is_object()) {
        return Err(Error::Config(
            "Trusted JWT issuer jwkSet keys must be JSON objects".into(),
        ));
    }
    Ok(value)
}

/// Return the string-valued JWK set carried by an AM issuer read, including
/// the inherited-read wrapper shape.
pub fn issuer_jwk_set(issuer: &Value) -> Option<&str> {
    issuer.get("jwkSet").and_then(Value::as_str).or_else(|| {
        issuer
            .get("jwkSet")
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
    })
}

/// Read `allowedSubjects` from an AM issuer object.
///
/// Reads come back wrapped as `{"inherited": false, "value": …}`; write bodies
/// and templates carry a plain array. Both shapes are accepted.
pub fn issuer_allowed_subjects(issuer: &Value) -> Vec<String> {
    let entries = issuer
        .get("allowedSubjects")
        .and_then(Value::as_array)
        .or_else(|| {
            issuer
                .get("allowedSubjects")
                .and_then(|value| value.get("value"))
                .and_then(Value::as_array)
        });
    entries
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an issuer's `allowedSubjects` list restricts minting.
///
/// Restricted if and only if at least one entry is non-empty after trimming.
/// In particular `[""]` and `["  "]` are unrestricted — the same realm-wide
/// behaviour as `[]`. See `docs/api/17-jwt-bearer-user-tokens.md`, "The exact
/// restriction rule".
pub fn issuer_is_restricted(subjects: &[String]) -> bool {
    subjects.iter().any(|subject| !subject.trim().is_empty())
}

/// Trim a subject and refuse the empty-string trap.
///
/// An empty or whitespace-only entry is what turns a restricted issuer into a
/// realm-wide one (`[""]` is unrestricted). Rejecting it here means no write
/// path can introduce one.
pub fn parse_subject(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Config(
            "subject cannot be empty or whitespace-only; an empty allowedSubjects entry makes the issuer unrestricted"
                .into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Whether `--id` looks like a UUID. A miss is almost certainly a username
/// passed to the wrong flag; callers warn rather than refuse, because
/// `resourceOwnerIdentityClaim` is configurable and a non-UUID subject is
/// legal in principle. No network.
pub fn id_is_uuid_shaped(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

/// Return the `issuer` claim from an AM issuer object, wrapped or plain.
pub fn issuer_name(issuer: &Value) -> Option<&str> {
    issuer
        .get("issuer")
        .and_then(Value::as_str)
        .or_else(|| {
            issuer
                .get("issuer")
                .and_then(|value| value.get("value"))
                .and_then(Value::as_str)
        })
        .filter(|value| !value.is_empty())
}

/// Next `allowedSubjects` after an idempotent, order-preserving add.
///
/// Incoming values are trimmed and empty ones refused. Existing empty or
/// whitespace-only entries are dropped so this path cannot write one.
pub fn subjects_after_add(existing: &[String], incoming: &[String]) -> Result<Vec<String>> {
    let mut next: Vec<String> = existing
        .iter()
        .filter(|subject| !subject.trim().is_empty())
        .cloned()
        .collect();
    for raw in incoming {
        let subject = parse_subject(raw)?;
        if !next.iter().any(|existing| existing == &subject) {
            next.push(subject);
        }
    }
    Ok(next)
}

/// Next `allowedSubjects` after removing the given subjects.
///
/// Absent subjects are a no-op, not an error. Incoming empty values are still
/// refused — they are never a real subject. Existing empty entries are dropped
/// so this path cannot write one.
pub fn subjects_after_remove(existing: &[String], incoming: &[String]) -> Result<Vec<String>> {
    let remove = incoming
        .iter()
        .map(|raw| parse_subject(raw))
        .collect::<Result<Vec<_>>>()?;
    Ok(existing
        .iter()
        .filter(|subject| {
            !subject.trim().is_empty() && !remove.iter().any(|candidate| candidate == *subject)
        })
        .cloned()
        .collect())
}

/// Assemble `--id` values and already-looked-up usernames into the list we
/// write. Username resolution is `lookup_username` + [`user_id_from_lookup`];
/// this only concatenates so a username never lands in `allowedSubjects`.
pub fn subjects_from_resolved(
    ids: &[String],
    username_lookups: &[(String, Value)],
) -> Result<Vec<String>> {
    let mut subjects = Vec::with_capacity(ids.len() + username_lookups.len());
    for id in ids {
        subjects.push(parse_subject(id)?);
    }
    for (username, response) in username_lookups {
        subjects.push(user_id_from_lookup(username, response)?);
    }
    Ok(subjects)
}

/// Human-readable `subjects list` lines. An unrestricted issuer must not
/// render as a blank line — that reads as "one empty subject" rather than
/// "mints for everyone".
pub fn subjects_list_lines(subjects: &[String]) -> Vec<String> {
    if issuer_is_restricted(subjects) {
        subjects.to_vec()
    } else {
        vec!["unrestricted — this issuer can mint for every user in the realm".into()]
    }
}

/// On a production-themed tenant, refuse a write that would leave the issuer
/// unrestricted. `remedy` is the next action; the gate itself does not know
/// which verb called it.
pub fn ensure_production_write_restricted(
    theme: TenantTheme,
    subjects_after_write: &[String],
    remedy: &str,
) -> Result<()> {
    if theme != TenantTheme::Production || issuer_is_restricted(subjects_after_write) {
        return Ok(());
    }
    Err(Error::Config(format!(
        "refusing to leave the Trusted JWT issuer unrestricted on a production-themed tenant; {remedy}"
    )))
}

/// `aic auth` reads the issuer only on production. Lower environments are the
/// hot path and do not read it at all.
pub fn mint_reads_issuer(theme: TenantTheme) -> bool {
    theme == TenantTheme::Production
}

/// Mint-time companion to [`ensure_production_write_restricted`]: on
/// production, refuse unless the issuer is already restricted. Does not check
/// that the requested subject is in the list — AM enforces that.
pub fn ensure_mint_allowed(theme: TenantTheme, subjects: &[String]) -> Result<()> {
    if !mint_reads_issuer(theme) || issuer_is_restricted(subjects) {
        return Ok(());
    }
    Err(Error::Config(
        "refusing to mint on a production-themed tenant whose Trusted JWT issuer is unrestricted; restrict it with `aic jwt-bearer subjects add` before minting"
            .into(),
    ))
}

/// Return the public JWK array from an AM issuer read, without adding local
/// metadata. The result is safe for `--json`: it contains public halves only.
pub fn jwk_set_keys(issuer: &Value) -> Result<Vec<Value>> {
    let jwks = parse_jwk_set(issuer_jwk_set(issuer))?;
    Ok(jwks
        .get("keys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// Find one public JWK by KID, reporting the set's actual KIDs on a miss.
pub fn jwk_set_key(existing: Option<&str>, kid: &str) -> Result<Value> {
    let jwks = parse_jwk_set(existing)?;
    let keys = jwks.get("keys").and_then(Value::as_array).ok_or_else(|| {
        Error::Config("Trusted JWT issuer jwkSet must contain a keys array".into())
    })?;
    keys.iter()
        .find(|key| key.get("kid").and_then(Value::as_str) == Some(kid))
        .cloned()
        .ok_or_else(|| missing_kid_error(kid, keys))
}

/// Remove one public JWK by KID while retaining all other keys and members.
pub fn remove_from_jwk_set(existing: Option<&str>, kid: &str) -> Result<String> {
    let mut jwks = parse_jwk_set(existing)?;
    let keys = jwks
        .get_mut("keys")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            Error::Config("Trusted JWT issuer jwkSet must contain a keys array".into())
        })?;
    let index = keys
        .iter()
        .position(|key| key.get("kid").and_then(Value::as_str) == Some(kid))
        .ok_or_else(|| missing_kid_error(kid, keys))?;
    keys.remove(index);
    Ok(serde_json::to_string(&jwks)?)
}

fn missing_kid_error(kid: &str, keys: &[Value]) -> Error {
    let published_kids = keys
        .iter()
        .map(|key| {
            key.get("kid")
                .and_then(Value::as_str)
                .unwrap_or("<missing kid>")
        })
        .collect::<Vec<_>>();
    Error::Config(format!(
        "Trusted JWT issuer jwkSet does not contain kid {kid:?}; published kids: {published_kids:?}"
    ))
}

/// Add or replace one public JWK by its `kid`, preserving every other key and
/// every member (including the non-standard `aic_*` attribution members).
pub fn merge_jwk_set(existing: Option<&str>, incoming: Value) -> Result<String> {
    if !incoming.is_object() {
        return Err(Error::Config("public JWK must be a JSON object".into()));
    }
    let kid = incoming
        .get("kid")
        .and_then(Value::as_str)
        .filter(|kid| !kid.is_empty())
        .ok_or_else(|| Error::Config("public JWK must contain a non-empty kid".into()))?;
    let mut jwks = parse_jwk_set(existing)?;
    let keys = jwks
        .get_mut("keys")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            Error::Config("Trusted JWT issuer jwkSet must contain a keys array".into())
        })?;

    if let Some(existing_key) = keys.iter_mut().find(|key| {
        key.get("kid")
            .and_then(Value::as_str)
            .is_some_and(|existing_kid| existing_kid == kid)
    }) {
        *existing_key = incoming;
    } else {
        keys.push(incoming);
    }
    Ok(serde_json::to_string(&jwks)?)
}

/// Convert the vault record to the portable private JWK shape used by export.
/// The record wrapper is deliberately not part of the file format: JOSE
/// consumers expect the key members at the top level.
pub fn export_private_jwk(record: &crate::jwtbearer::KeyRecord) -> Result<Value> {
    if record.kid.is_empty() {
        return Err(Error::Config(
            "stored Trusted JWT private key has an empty kid".into(),
        ));
    }
    let mut jwk = record.private_jwk.clone();
    validate_private_rsa_jwk(&jwk)?;
    if let Some(jwk_kid) = jwk.get("kid").and_then(Value::as_str)
        && jwk_kid != record.kid
    {
        return Err(Error::Config(
            "stored Trusted JWT private JWK kid does not match its key record".into(),
        ));
    }
    jwk.as_object_mut()
        .ok_or_else(|| Error::Config("private JWK must be a JSON object".into()))?
        .insert("kid".into(), Value::String(record.kid.clone()));
    Ok(jwk)
}

/// Validate and convert one portable private JWK into the local vault record.
/// Checking `d` here makes a public-only paste fail before it can be stored.
pub fn import_private_jwk(jwk: Value) -> Result<crate::jwtbearer::KeyRecord> {
    validate_private_rsa_jwk(&jwk)?;
    let kid = jwk
        .get("kid")
        .and_then(Value::as_str)
        .filter(|kid| !kid.is_empty())
        .ok_or_else(|| Error::Config("private JWK must contain a non-empty kid".into()))?
        .to_string();
    Ok(crate::jwtbearer::KeyRecord {
        kid,
        private_jwk: jwk,
    })
}

/// Return whether an issuer's string-valued JWK set contains `kid`.
pub fn jwk_set_contains(issuer: &Value, kid: &str) -> Result<bool> {
    Ok(jwk_set_keys(issuer)?.iter().any(|key| {
        key.get("kid")
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate == kid)
    }))
}

fn validate_private_rsa_jwk(jwk: &Value) -> Result<()> {
    if jwk.get("kty").and_then(Value::as_str) != Some("RSA") {
        return Err(Error::Config("JWK must be an RSA private key".into()));
    }
    for field in ["n", "e", "d"] {
        if jwk
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(Error::Config(format!(
                "JWK must be an RSA private key; missing private member {field:?}"
            )));
        }
    }
    crate::aic::auth::jwk_to_encoding_key(jwk).map(|_| ())
}

/// Convert AM's inherited read wrappers and server fields into a plain PUT
/// body, then apply the fields that are load-bearing for this feature.
/// How long AM may serve a cached copy of the issuer's `jwkSet`, in ms.
///
/// AM's template default is 3600000 (one hour), which is wrong for this
/// feature. The cache is not just a read optimisation — it bounds how long a
/// key that has been *removed* keeps minting tokens. Verified 2026-08-07: a
/// removed key still minted immediately after the write landed
/// (`docs/api/17-jwt-bearer-user-tokens.md`).
///
/// An hour of that is untenable for a capability that exists for fast iteration
/// in lower environments and is refused outright on production. One minute
/// bounds it to something a person will sit through, at the cost of an extra
/// key-set read per minute per realm. AM accepted the shorter value on write
/// (verified 2026-08-07); note that lowering it does **not** rescue an entry
/// already cached under the old TTL.
const JWKS_CACHE_TIMEOUT_MS: u64 = 60_000;

pub fn issuer_body(source: Value, issuer: &str, jwk_set: String) -> Result<Value> {
    if issuer.trim().is_empty() {
        return Err(Error::Config("Trusted JWT issuer cannot be empty".into()));
    }
    let mut source = unwrap_inherited(source);
    let object = source
        .as_object_mut()
        .ok_or_else(|| Error::Config("Trusted JWT issuer body must be a JSON object".into()))?;
    for field in SERVER_FIELDS {
        object.remove(field);
    }
    object.insert("issuer".into(), Value::String(issuer.to_string()));
    object.insert("jwkSet".into(), Value::String(jwk_set));
    // Preserve an existing restriction list. Blanking it on every write would
    // silently re-open a previously restricted issuer on the next setup/rotate.
    // Default only when the key is absent (create-from-template already has []).
    object.entry("allowedSubjects").or_insert_with(|| json!([]));
    // This names the assertion claim that narrows a requested grant: an
    // assertion claiming `scope: "openid"` while requesting `openid profile`
    // receives only `openid`; it never grants scopes by itself. `sub` must
    // remain the user's UUID so IDM accepts the resulting token.
    object.insert("consentedScopesClaim".into(), Value::String("scope".into()));
    object.insert(
        "resourceOwnerIdentityClaim".into(),
        Value::String("sub".into()),
    );
    object.insert("jwksCacheTimeout".into(), json!(JWKS_CACHE_TIMEOUT_MS));
    Ok(source)
}

/// [`issuer_body`] plus an explicit `allowedSubjects` overwrite.
///
/// `issuer_body` preserves an existing list (`or_insert`). A subjects edit
/// must set the new list *after* that preserve, or `rm` would write the old
/// list back. Empty or whitespace-only entries are refused so this path
/// cannot produce the `[""]` trap.
pub fn issuer_body_with_subjects(
    source: Value,
    issuer: &str,
    jwk_set: String,
    subjects: Vec<String>,
) -> Result<Value> {
    if subjects.iter().any(|subject| subject.trim().is_empty()) {
        return Err(Error::Config(
            "subject cannot be empty or whitespace-only; an empty allowedSubjects entry makes the issuer unrestricted"
                .into(),
        ));
    }
    let mut body = issuer_body(source, issuer, jwk_set)?;
    body.as_object_mut()
        .ok_or_else(|| Error::Config("Trusted JWT issuer body must be a JSON object".into()))?
        .insert("allowedSubjects".into(), json!(subjects));
    Ok(body)
}

pub const MAX_ASSERTION_TTL_SECS: i64 = 180;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserAssertionClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

/// Build user assertion claims. Scope is deliberately absent: AM treats
/// consentedScopesClaim as a ceiling, while requested scopes belong in the
/// exchange form.
pub fn user_assertion_claims(
    issuer: &str,
    subject: &str,
    audience: &str,
    now: i64,
) -> UserAssertionClaims {
    UserAssertionClaims {
        iss: issuer.to_string(),
        sub: subject.to_string(),
        aud: audience.to_string(),
        iat: now,
        exp: now + MAX_ASSERTION_TTL_SECS,
        jti: Uuid::new_v4().to_string(),
    }
}

pub fn user_assertion_header(kid: &str) -> Header {
    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_string());
    header
}

pub fn sign_user_assertion(
    issuer: &str,
    subject: &str,
    audience: &str,
    now: i64,
    kid: &str,
    private_jwk: &Value,
) -> Result<String> {
    let claims = user_assertion_claims(issuer, subject, audience, now);
    let header = user_assertion_header(kid);
    let key = crate::aic::auth::jwk_to_encoding_key(private_jwk)?;
    jsonwebtoken::encode(&header, &claims, &key).map_err(|error| Error::Auth(error.to_string()))
}

pub fn token_request(
    client_id: &str,
    client_secret: Option<&str>,
    client_auth: ClientAuthMethod,
    assertion: &str,
    scopes: &[String],
) -> TokenRequest {
    let mut form = Serializer::new(String::new());
    form.append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
        .append_pair("client_id", client_id)
        .append_pair("assertion", assertion);
    if let (ClientAuthMethod::ClientSecretPost, Some(secret)) = (client_auth, client_secret) {
        form.append_pair("client_secret", secret);
    }
    if !scopes.is_empty() {
        form.append_pair("scope", &scopes.join(" "));
    }
    let authorization = match (client_auth, client_secret) {
        (ClientAuthMethod::ClientSecretBasic, Some(secret)) => {
            let credential = format!("{}:{}", form_component(client_id), form_component(secret));
            Some(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(credential)
            ))
        }
        _ => None,
    };
    TokenRequest {
        body: form.finish(),
        authorization,
    }
}

fn form_component(value: &str) -> String {
    // Serializer always emits the empty field name followed by `=`; removing
    // that delimiter leaves exactly one application/x-www-form-urlencoded
    // component, including `+` for spaces and UTF-8 percent encoding.
    Serializer::new(String::new())
        .append_pair("", value)
        .finish()
        .strip_prefix('=')
        .expect("a form pair with an empty name always starts with '='")
        .to_string()
}

pub fn username_lookup_path(realm: &str, username: &str) -> String {
    let filter = format!("userName eq \"{username}\"");
    let query = Serializer::new(String::new())
        .append_pair("_queryFilter", &filter)
        .append_pair("_fields", "_id")
        .finish();
    format!("/openidm/managed/{realm}_user?{query}")
}

pub fn user_id_from_lookup(username: &str, response: &Value) -> Result<String> {
    let results = response
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::Config(format!(
                "username lookup for {username:?} returned an unexpected response"
            ))
        })?;
    match results.as_slice() {
        [] => Err(Error::Config(format!(
            "username lookup for {username:?} returned no users; check the username"
        ))),
        [_] => results[0]
            .get("_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::Config(format!(
                    "username lookup for {username:?} returned a user without _id"
                ))
            }),
        _ => Err(Error::Config(format!(
            "username lookup for {username:?} returned multiple users; use --as-id"
        ))),
    }
}

/// Turn the verified AM OAuth error strings into the next action an operator
/// can take, while leaving unrelated transport/API errors intact.
pub fn map_token_error(error: Error) -> Error {
    let Error::Api { status, body } = error else {
        return error;
    };
    let parsed = serde_json::from_str::<Value>(&body).ok();
    let code = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let description = parsed
        .as_ref()
        .and_then(|value| value.get("error_description"))
        .and_then(Value::as_str)
        .unwrap_or(&body);
    let lower = description.to_ascii_lowercase();
    let message = if code == "invalid_client" {
        Some(
            "AM rejected client authentication; supply --client-secret-stdin, verify --client-auth matches the client's tokenEndpointAuthMethod, and verify the client allows the JWT-bearer grant with `aic oauth grant add <client-id> urn:ietf:params:oauth:grant-type:jwt-bearer`".to_string(),
        )
    } else if code == "unauthorized_client" || lower.contains("grant not allowed") {
        Some("AM rejected the grant; add urn:ietf:params:oauth:grant-type:jwt-bearer with `aic oauth grant add <client-id> urn:ietf:params:oauth:grant-type:jwt-bearer`".to_string())
    } else if lower.contains("unknown jwt issuer") {
        Some("AM does not know this JWT issuer; run aic jwt-bearer setup".to_string())
    } else if lower.contains("issuer is not authorized to grant consent for this subject") {
        Some("the issuer is restricted to specific subjects; this user is not allowed".to_string())
    } else if lower.contains("not able to read user information") {
        Some("AM could not find the user; check the id/username".to_string())
    } else if lower.contains("incorrect audience in jwt") {
        Some(
            "AM rejected the audience; this is an internal bug because it comes from discovery"
                .to_string(),
        )
    } else {
        None
    };
    match message {
        Some(message) => Error::Config(format!("{message}; AM error_description: {description}")),
        None => Error::Api { status, body },
    }
}

fn unwrap_inherited(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(unwrap_inherited).collect()),
        Value::Object(object) => {
            let wrapped_value = object.get("value").cloned();
            if object.len() == 2 && object.contains_key("inherited") {
                if let Some(wrapped_value) = wrapped_value {
                    return unwrap_inherited(wrapped_value);
                }
            }
            let mut unwrapped = Map::new();
            for (key, value) in object {
                unwrapped.insert(key, unwrap_inherited(value));
            }
            Value::Object(unwrapped)
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::jwtbearer::KeyRecord;

    use super::*;

    fn stub_private_jwk() -> Value {
        json!({
            "kty": "RSA",
            "n": "AKpuDplQxCLK-rAKU07dO7TzfBy7kEzc_dfqET0uBLRifyWGGIX4IphLDLoY4-BcNWAP6hLRRRIP_FkYu1m2MPPuO7pOaua8ZaCzfyjXWt77G0OZP493fXndGKam3sy-UTKgMIN5DfJ557CCPFFN3IEX5I8QzUye8CRCnuyXBP-h",
            "e": "AQAB",
            "d": "HBCThtut8KzMK0EIBuyXcGzH-1NHp-CcTHnW7OQvEiVGGr_COg1qZPm21s5SeBe3EmKMgRzE6vyG6YURFOzTkpLEMsjPTMUDXFIuKatyEu0xs6pSIiRRaQkauVheDbezVdd0w2FiZbcYhcfH4ShJZOLpH8u0H7sCkKtQehc0uyE",
            "p": "ANpSI07iIr2jcZdzXDX5ul13A-x4Add0A07RGa-1VtPaXKEMHSvzEyXwSn0p-EH-LKUi9eB1puqd_Ii_1WV1OuM",
            "q": "AMfX_9KCXu9iUYQrx3frtsGWkJyC8LjQBogeH2UNnBzrCJldEtijhz08W_Rtak--5SQMflEUYx2Ww8R4rR6czqs"
        })
    }

    #[test]
    fn private_jwk_export_import_round_trip_preserves_attribution() {
        let record = KeyRecord {
            kid: "portable-kid".into(),
            private_jwk: {
                let mut jwk = stub_private_jwk();
                jwk["kid"] = json!("portable-kid");
                jwk["aic_owner"] = json!("owner");
                jwk["aic_host"] = json!("host");
                jwk["aic_created"] = json!("created");
                jwk
            },
        };
        let exported = export_private_jwk(&record).unwrap();
        assert_eq!(exported["kid"], "portable-kid");
        assert_eq!(exported["aic_owner"], "owner");
        assert_eq!(exported["aic_host"], "host");
        assert_eq!(exported["aic_created"], "created");

        let file_contents = serde_json::to_string(&exported).unwrap();
        let imported = import_private_jwk(serde_json::from_str(&file_contents).unwrap()).unwrap();
        assert_eq!(imported, record);
    }

    #[test]
    fn private_jwk_validation_rejects_public_and_missing_kid_inputs() {
        let mut public = stub_private_jwk();
        public.as_object_mut().unwrap().remove("d");
        let cases = [
            (public, "RSA private key"),
            (stub_private_jwk(), "non-empty kid"),
            (json!({"kty": "EC", "kid": "wrong"}), "RSA private key"),
        ];
        for (jwk, expected) in cases {
            let error = import_private_jwk(jwk).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn jwk_set_contains_checks_the_default_issuer_shape() {
        let issuer = json!({
            "jwkSet": {"value": "{\"keys\":[{\"kid\":\"portable-kid\"}]}"}
        });
        assert!(jwk_set_contains(&issuer, "portable-kid").unwrap());
        assert!(!jwk_set_contains(&issuer, "other-kid").unwrap());
    }

    #[test]
    fn merge_adds_a_key_without_dropping_existing_keys() {
        let existing = r#"{"keys":[{"kid":"old","aic_owner":"other"}]}"#;
        let merged = merge_jwk_set(Some(existing), json!({"kid":"new","aic_owner":"me"})).unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(merged["keys"].as_array().unwrap().len(), 2);
        assert_eq!(merged["keys"][0]["kid"], "old");
        assert_eq!(merged["keys"][1]["aic_owner"], "me");
    }

    #[test]
    fn merge_replaces_matching_kid_and_leaves_other_keys_untouched() {
        let existing = r#"{"keys":[{"kid":"same","old":true},{"kid":"other","keep":true}]}"#;
        let merged = merge_jwk_set(Some(existing), json!({"kid":"same","new":true})).unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(merged["keys"].as_array().unwrap().len(), 2);
        assert_eq!(merged["keys"][0], json!({"kid":"same","new":true}));
        assert_eq!(merged["keys"][1], json!({"kid":"other","keep":true}));
    }

    #[test]
    fn remove_from_jwk_set_handles_the_key_set_edges_without_key_generation() {
        let cases = [
            (
                Some(
                    r#"{"keys":[{"kid":"old","aic_owner":"owner"},{"kid":"keep","aic_host":"host"}]}"#,
                ),
                "old",
                json!({"keys":[{"kid":"keep","aic_host":"host"}]}),
            ),
            (
                Some(r#"{"keys":[{"kid":"only","aic_created":"created"}]}"#),
                "only",
                json!({"keys": []}),
            ),
            (Some(r#"{"keys":[]}"#), "missing", Value::Null),
            (None, "missing", Value::Null),
            (Some(""), "missing", Value::Null),
        ];

        for (existing, kid, expected) in cases {
            let result = remove_from_jwk_set(existing, kid);
            if expected.is_null() {
                let error = result.unwrap_err();
                assert!(error.to_string().contains("published kids"), "{error}");
            } else {
                let actual: Value = serde_json::from_str(&result.unwrap()).unwrap();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn remove_from_jwk_set_preserves_unremoved_keys_and_attribution_members() {
        let existing = r#"{"keys":[{"kid":"remove","aic_owner":"one","custom":{"x":1}},{"kid":"keep","aic_host":"host","aic_created":"created"}]}"#;
        let removed = remove_from_jwk_set(Some(existing), "remove").unwrap();
        let removed: Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(
            removed["keys"][0],
            json!({"kid":"keep","aic_host":"host","aic_created":"created"})
        );
    }

    #[test]
    fn empty_and_malformed_jwk_sets_are_handled_as_results() {
        assert_eq!(
            merge_jwk_set(Some(""), json!({"kid":"new"})).unwrap(),
            r#"{"keys":[{"kid":"new"}]}"#
        );
        assert!(merge_jwk_set(Some("not json"), json!({"kid":"new"})).is_err());
        let error = merge_jwk_set(Some(""), json!("not a jwk")).unwrap_err();
        assert!(error.to_string().contains("must be a JSON object"));
    }

    #[test]
    fn production_writes_are_refused_only_when_the_result_would_be_unrestricted() {
        let remedy = "do the next thing";
        let unrestricted = [Vec::<String>::new(), vec!["".into()], vec!["  ".into()]];
        for subjects in unrestricted {
            let error =
                ensure_production_write_restricted(TenantTheme::Production, &subjects, remedy)
                    .unwrap_err();
            assert!(error.to_string().contains("unrestricted"), "{error}");
            assert!(error.to_string().contains(remedy), "{error}");
            assert!(
                ensure_production_write_restricted(TenantTheme::Sandbox, &subjects, remedy).is_ok(),
                "sandbox must allow {subjects:?}"
            );
        }
        assert!(
            ensure_production_write_restricted(
                TenantTheme::Production,
                &["user-uuid".into()],
                remedy
            )
            .is_ok()
        );
        assert!(
            ensure_production_write_restricted(
                TenantTheme::Production,
                &["".into(), "user-uuid".into()],
                remedy
            )
            .is_ok()
        );
    }

    #[test]
    fn mint_reads_the_issuer_on_production_only() {
        assert!(mint_reads_issuer(TenantTheme::Production));
        assert!(!mint_reads_issuer(TenantTheme::Sandbox));
        assert!(!mint_reads_issuer(TenantTheme::Development));
        assert!(!mint_reads_issuer(TenantTheme::Staging));
    }

    #[test]
    fn mint_is_refused_on_production_unless_the_issuer_is_restricted() {
        assert!(ensure_mint_allowed(TenantTheme::Sandbox, &[]).is_ok());
        assert!(ensure_mint_allowed(TenantTheme::Sandbox, &["".into()]).is_ok());
        assert!(ensure_mint_allowed(TenantTheme::Production, &[]).is_err());
        assert!(ensure_mint_allowed(TenantTheme::Production, &["".into()]).is_err());
        assert!(ensure_mint_allowed(TenantTheme::Production, &["user-uuid".into()]).is_ok());
        let error = ensure_mint_allowed(TenantTheme::Production, &["  ".into()]).unwrap_err();
        assert!(error.to_string().contains("subjects add"), "{error}");
    }

    #[test]
    fn issuer_body_unwraps_reads_and_keeps_attribution_members() {
        let body = issuer_body(
            json!({
                "_id": "issuer",
                "issuer": {"inherited": false, "value": "old"},
                "jwkSet": {"inherited": false, "value": "old-set"},
                "custom": {"inherited": false, "value": {"aic_host": "host"}}
            }),
            "aic-agent",
            "{\"keys\":[]}".into(),
        )
        .unwrap();
        assert!(body.get("_id").is_none());
        assert_eq!(body["issuer"], "aic-agent");
        assert_eq!(body["allowedSubjects"], json!([]));
        assert_eq!(body["consentedScopesClaim"], "scope");
        assert_eq!(body["resourceOwnerIdentityClaim"], "sub");
        assert_eq!(body["custom"]["aic_host"], "host");
        assert_eq!(body["jwksCacheTimeout"], 60_000);
    }

    /// A restricted issuer must survive the next setup/rotate; blanking the
    /// list on every write was re-opening the realm-wide mint boundary.
    #[test]
    fn issuer_body_preserves_existing_allowed_subjects() {
        let body = issuer_body(
            json!({
                "allowedSubjects": {"inherited": false, "value": ["user-uuid-1"]},
            }),
            "aic-agent",
            "{\"keys\":[]}".into(),
        )
        .unwrap();
        assert_eq!(body["allowedSubjects"], json!(["user-uuid-1"]));
    }

    #[test]
    fn issuer_body_keeps_template_empty_allowed_subjects() {
        let body = issuer_body(
            json!({"allowedSubjects": []}),
            "aic-agent",
            "{\"keys\":[]}".into(),
        )
        .unwrap();
        assert_eq!(body["allowedSubjects"], json!([]));
    }

    #[test]
    fn issuer_body_defaults_missing_allowed_subjects_to_empty() {
        let body = issuer_body(json!({}), "aic-agent", "{\"keys\":[]}".into()).unwrap();
        assert_eq!(body["allowedSubjects"], json!([]));
    }

    /// The write must overwrite AM's one-hour template default rather than
    /// round-tripping whatever the existing object happens to carry — an issuer
    /// created before this change should be corrected by the next `setup`.
    #[test]
    fn issuer_body_overrides_an_inherited_hour_long_cache_timeout() {
        let body = issuer_body(
            json!({"jwksCacheTimeout": {"inherited": false, "value": 3_600_000}}),
            "aic-agent",
            "{\"keys\":[]}".into(),
        )
        .unwrap();

        assert_eq!(body["jwksCacheTimeout"], 60_000);
    }

    #[test]
    fn issuer_allowed_subjects_reads_plain_and_wrapped_shapes() {
        assert_eq!(
            issuer_allowed_subjects(&json!({"allowedSubjects": ["a", "b"]})),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            issuer_allowed_subjects(&json!({
                "allowedSubjects": {"inherited": false, "value": ["wrapped"]}
            })),
            vec!["wrapped".to_string()]
        );
        assert!(issuer_allowed_subjects(&json!({})).is_empty());
    }

    /// `!subjects.is_empty()` would treat `[""]` as restricted while AM still
    /// mints for every user in the realm. Pin the trim rule from the verified
    /// table in docs/api/17-jwt-bearer-user-tokens.md.
    #[test]
    fn issuer_is_restricted_follows_the_verified_trim_rule() {
        let cases = [
            (Vec::<String>::new(), false),
            (vec!["".into()], false),
            (vec!["  ".into()], false),
            (vec!["user-uuid".into()], true),
            (vec!["".into(), "user-uuid".into()], true),
        ];
        for (subjects, restricted) in cases {
            assert_eq!(
                issuer_is_restricted(&subjects),
                restricted,
                "subjects={subjects:?}"
            );
            // A length-only check is the bug this pins: it disagrees on [""].
            if subjects == [""] {
                assert!(
                    !subjects.is_empty() && !issuer_is_restricted(&subjects),
                    "length-only check must not pass for [\"\"]"
                );
            }
        }
    }

    #[test]
    fn empty_or_whitespace_subjects_cannot_be_written() {
        for raw in ["", "   ", "\t"] {
            assert!(parse_subject(raw).is_err(), "parse {raw:?}");
            assert!(
                subjects_after_add(&[], &[raw.into()]).is_err(),
                "add {raw:?}"
            );
            assert!(
                subjects_after_remove(&["keep".into()], &[raw.into()]).is_err(),
                "rm {raw:?}"
            );
            assert!(
                issuer_body_with_subjects(
                    json!({}),
                    "aic-agent",
                    "{\"keys\":[]}".into(),
                    vec![raw.into()],
                )
                .is_err(),
                "body {raw:?}"
            );
        }
        let added = subjects_after_add(&["".into(), "  ".into()], &["user-uuid".into()]).unwrap();
        assert_eq!(added, vec!["user-uuid".to_string()]);
        let removed =
            subjects_after_remove(&["".into(), "user-uuid".into()], &["user-uuid".into()]).unwrap();
        assert!(removed.is_empty());
    }

    #[test]
    fn subjects_add_is_idempotent_and_preserves_order() {
        let existing = vec!["first".into(), "second".into()];
        let added = subjects_after_add(
            &existing,
            &["second".into(), "third".into(), "first".into()],
        )
        .unwrap();
        assert_eq!(
            added,
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );
        let unchanged = subjects_after_remove(&existing, &["absent".into()]).unwrap();
        assert_eq!(unchanged, existing);
    }

    #[test]
    fn username_is_resolved_to_a_uuid_before_it_reaches_the_list() {
        let lookup = json!({"result": [{"_id": "45565631-0000-4000-8000-000000000000"}]});
        let subjects =
            subjects_from_resolved(&["already-a-uuid".into()], &[("user.0".into(), lookup)])
                .unwrap();
        assert_eq!(
            subjects,
            vec![
                "already-a-uuid".to_string(),
                "45565631-0000-4000-8000-000000000000".to_string()
            ]
        );
        assert!(!subjects.iter().any(|subject| subject == "user.0"));
    }

    #[test]
    fn subjects_add_leaves_published_kids_unchanged() {
        let source = json!({
            "issuer": "aic-agent",
            "jwkSet": {"inherited": false, "value": r#"{"keys":[{"kid":"keep-me"},{"kid":"also"}]}"#},
            "allowedSubjects": ["already"],
        });
        let kids_before: Vec<String> = jwk_set_keys(&source)
            .unwrap()
            .iter()
            .map(|key| key["kid"].as_str().unwrap().to_string())
            .collect();
        let jwk_set = issuer_jwk_set(&source).unwrap().to_string();
        let added =
            subjects_after_add(&issuer_allowed_subjects(&source), &["new-uuid".into()]).unwrap();
        let body = issuer_body_with_subjects(source, "aic-agent", jwk_set, added).unwrap();
        let kids_after: Vec<String> = jwk_set_keys(&body)
            .unwrap()
            .iter()
            .map(|key| key["kid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kids_before, kids_after);
        assert_eq!(kids_after, vec!["keep-me", "also"]);
        assert_eq!(body["allowedSubjects"], json!(["already", "new-uuid"]));
    }

    /// `issuer_body` preserves `allowedSubjects`. Setting the edited list
    /// after that preserve is what makes `subjects rm` actually remove.
    #[test]
    fn issuer_body_with_subjects_overwrites_the_preserved_list() {
        let source = json!({
            "allowedSubjects": {"inherited": false, "value": ["keep", "drop"]},
            "jwkSet": r#"{"keys":[{"kid":"k1"}]}"#,
        });
        let jwk_set = issuer_jwk_set(&source).unwrap().to_string();
        let next =
            subjects_after_remove(&issuer_allowed_subjects(&source), &["drop".into()]).unwrap();
        let body = issuer_body_with_subjects(source, "aic-agent", jwk_set, next).unwrap();
        assert_eq!(body["allowedSubjects"], json!(["keep"]));
        assert_eq!(jwk_set_keys(&body).unwrap()[0]["kid"], "k1");
    }

    #[test]
    fn subjects_list_makes_an_unrestricted_issuer_obvious() {
        let unrestricted = subjects_list_lines(&["".into()]);
        assert_eq!(unrestricted.len(), 1);
        assert!(unrestricted[0].contains("unrestricted"));
        assert!(!unrestricted[0].trim().is_empty());
        assert_eq!(
            subjects_list_lines(&["user-uuid".into()]),
            vec!["user-uuid".to_string()]
        );
    }

    #[test]
    fn non_uuid_ids_are_detectable_without_a_network_call() {
        assert!(id_is_uuid_shaped("45565631-0000-4000-8000-000000000000"));
        assert!(!id_is_uuid_shaped("user.0"));
        assert!(!id_is_uuid_shaped("not-a-uuid"));
    }

    #[test]
    fn user_claims_have_no_scope_and_short_expiry() {
        let claims = user_assertion_claims("aic-agent", "user-id", "https://tenant:443/aud", 100);
        let json = serde_json::to_value(&claims).unwrap();
        assert_eq!(json["iss"], "aic-agent");
        assert_eq!(json["sub"], "user-id");
        assert_eq!(json["aud"], "https://tenant:443/aud");
        assert!(json.get("scope").is_none());
        assert!(claims.exp - claims.iat <= MAX_ASSERTION_TTL_SECS);
        assert!(!claims.jti.is_empty());
    }

    #[test]
    fn assertion_header_selects_the_stored_kid_and_rs256() {
        let header = user_assertion_header("stored-kid");
        assert_eq!(header.alg, jsonwebtoken::Algorithm::RS256);
        assert_eq!(header.kid.as_deref(), Some("stored-kid"));
    }

    #[test]
    fn client_secret_post_request_is_byte_identical_to_the_original_form() {
        let request = token_request(
            "client",
            Some("secret"),
            ClientAuthMethod::ClientSecretPost,
            "assertion",
            &["openid".into(), "profile".into()],
        );
        assert_eq!(
            request.body,
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&client_id=client&assertion=assertion&client_secret=secret&scope=openid+profile"
        );
        assert_eq!(request.authorization, None);
    }

    #[test]
    fn client_secret_basic_uses_an_authorization_header_only() {
        let request = token_request(
            "client",
            Some("secret"),
            ClientAuthMethod::ClientSecretBasic,
            "assertion",
            &[],
        );

        assert_eq!(
            request.authorization.as_deref(),
            Some("Basic Y2xpZW50OnNlY3JldA==")
        );
        assert_eq!(
            request.body,
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&client_id=client&assertion=assertion"
        );
        assert!(!request.body.contains("client_secret"));
    }

    #[test]
    fn client_secret_basic_percent_encodes_colons_and_non_ascii_before_joining() {
        let request = token_request(
            "client:id",
            Some("së:cret"),
            ClientAuthMethod::ClientSecretBasic,
            "assertion",
            &[],
        );

        assert_eq!(
            request.authorization.as_deref(),
            Some("Basic Y2xpZW50JTNBaWQ6cyVDMyVBQiUzQWNyZXQ=")
        );
    }

    #[test]
    fn public_client_request_sends_no_secret_under_either_method() {
        for method in [
            ClientAuthMethod::ClientSecretBasic,
            ClientAuthMethod::ClientSecretPost,
        ] {
            let request = token_request("public", None, method, "assertion", &[]);
            assert_eq!(request.authorization, None);
            assert!(!request.body.contains("client_secret"));
        }
    }

    #[test]
    fn username_lookup_uses_the_realm_user_collection() {
        let path = username_lookup_path("alpha", "a user");
        assert_eq!(
            path,
            "/openidm/managed/alpha_user?_queryFilter=userName+eq+%22a+user%22&_fields=_id"
        );
    }

    #[test]
    fn username_lookup_distinguishes_zero_and_multiple_results() {
        let zero = user_id_from_lookup("missing", &json!({"result": []})).unwrap_err();
        let multiple = user_id_from_lookup(
            "duplicate",
            &json!({"result": [{"_id": "one"}, {"_id": "two"}]}),
        )
        .unwrap_err();
        assert!(zero.to_string().contains("no users"));
        assert!(multiple.to_string().contains("multiple users"));
        assert_ne!(zero.to_string(), multiple.to_string());
    }

    #[test]
    fn token_errors_name_the_fix_for_each_verified_am_failure() {
        let cases = [
            (
                r#"{"error":"invalid_grant","error_description":"Unknown JWT issuer"}"#,
                "jwt-bearer setup",
            ),
            (
                r#"{"error":"invalid_grant","error_description":"Issuer is not authorized to grant consent for this subject"}"#,
                "restricted to specific subjects",
            ),
            (
                r#"{"error":"invalid_grant","error_description":"Not able to read user information."}"#,
                "check the id/username",
            ),
            (
                r#"{"error":"invalid_grant","error_description":"incorrect audience in JWT"}"#,
                "internal bug",
            ),
            (
                r#"{"error":"invalid_client","error_description":"Client authentication failed"}"#,
                "--client-auth",
            ),
            (
                r#"{"error":"unauthorized_client","error_description":"grant not allowed"}"#,
                "aic oauth grant add",
            ),
        ];
        for (body, expected) in cases {
            let error = map_token_error(Error::Api {
                status: 400,
                body: body.into(),
            });
            assert!(error.to_string().contains(expected), "{error}");
            assert!(
                error.to_string().contains("AM error_description:"),
                "{error}"
            );
        }
    }

    #[test]
    fn fixed_jwk_can_sign_an_assertion_without_runtime_key_generation() {
        let jwk = stub_private_jwk();
        let token = sign_user_assertion("aic-agent", "user-id", "audience", 100, "fixed-kid", &jwk)
            .unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.kid.as_deref(), Some("fixed-kid"));
        assert_eq!(header.alg, jsonwebtoken::Algorithm::RS256);
    }
}
