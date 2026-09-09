//! Verified HTTP wrappers for AM OAuth2 clients.
//! See `docs/api/05-oauth2-oidc.md`.

use std::path::is_separator;

use serde_json::{Value, json};
use url::form_urlencoded::Serializer;

use crate::oauth::spec::sanitize_for_write;
use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.1,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn clients_path(realm: &str) -> String {
    format!("{}/realm-config/agents/OAuth2Client", realm_path(realm))
}

fn provider_path(realm: &str) -> String {
    format!("{}/realm-config/services/oauth-oidc", realm_path(realm))
}

fn validate_client_id(id: &str) -> Result<()> {
    if id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "oauth client id {id:?} contains a path separator"
        )));
    }
    Ok(())
}

/// The fields a listing row is built from.
///
/// Measured 2026-09-09: `_fields` takes `group/field` paths on this collection
/// and returns the nested values **unwrapped** — `"clientType": "Confidential"`
/// rather than the `{inherited, value}` envelope a single-client `GET` returns.
/// So a row reads these directly and must not be fed through `inherited_value`.
const LIST_FIELDS: &str = "_id,coreOAuth2ClientConfig/clientName,coreOAuth2ClientConfig/clientType,coreOAuth2ClientConfig/status,advancedOAuth2ClientConfig/grantTypes";

/// The fields the token-exchange view is built from.
///
/// Same projection contract as [`LIST_FIELDS`] — measured 2026-09-10 against
/// the sandbox's `bravo` realm, which is the realm that grants the exchange.
/// A field absent from a client is simply absent from its row rather than
/// null, so every reader here has to treat missing as "not set" and not as a
/// shape error.
///
/// Which group each field lives in is **not** guessable, and was read off the
/// live `?_action=schema` on 2026-09-10 after a first attempt put one in the
/// wrong place and got a permanently-absent column for it:
/// `allowedResourceServerAudienceValues` and `tokenExchangeAuthLevel` are
/// `advancedOAuth2ClientConfig`, while
/// `acceptAudienceParametersInTokenExchangeRequests` and both may-act scripts
/// are `overrideOAuth2ClientConfig` — and therefore governed by
/// `providerOverridesEnabled`. There are no `…MayActPluginType` companions;
/// setting the script id is enough.
const EXCHANGE_FIELDS: &str = "_id,advancedOAuth2ClientConfig/grantTypes,advancedOAuth2ClientConfig/tokenExchangeAuthLevel,advancedOAuth2ClientConfig/allowedResourceServerAudienceValues,overrideOAuth2ClientConfig/providerOverridesEnabled,overrideOAuth2ClientConfig/acceptAudienceParametersInTokenExchangeRequests,overrideOAuth2ClientConfig/accessTokenMayActScript,overrideOAuth2ClientConfig/oidcMayActScript";

/// The cookie for the next page, or `None` when this was the last one.
///
/// Errors rather than stopping when the field is present with an unusable
/// type: silently ending a listing early is how an incomplete population
/// becomes a confident answer about "every client".
fn next_page_cookie(body: &Value) -> Result<Option<String>> {
    match body.get("pagedResultsCookie") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cookie)) if cookie.is_empty() => Ok(None),
        Some(Value::String(cookie)) => Ok(Some(cookie.clone())),
        Some(other) => Err(Error::Api {
            status: 0,
            body: format!(
                "oauth client listing returned a `pagedResultsCookie` that is not a string ({other}); refusing rather than silently stopping mid-listing"
            ),
        }),
    }
}

/// List clients as whole rows. See [`list_clients`] for the ids alone.
pub async fn list_client_rows(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    list_projected_rows(tenant, realm, LIST_FIELDS).await
}

/// Every client projected down to its token-exchange configuration.
pub async fn list_exchange_rows(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    list_projected_rows(tenant, realm, EXCHANGE_FIELDS).await
}

/// One paged pass over the client collection, keeping `fields` of each row.
///
/// Shared by the two projections above so paging, the `_id` guard and the
/// ordering are decided once. It keeps whole rows because both callers want
/// several fields; the id-only [`list_clients`] stays separate rather than
/// projecting a `Vec<Value>` it would immediately throw away.
async fn list_projected_rows(tenant: &str, realm: &str, fields: &str) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    let mut cookie: Option<String> = None;

    loop {
        let query = {
            let mut query = Serializer::new(String::new());
            query
                .append_pair("_queryFilter", "true")
                .append_pair("_fields", fields)
                .append_pair("_pageSize", "1000");
            if let Some(cookie) = cookie.as_deref() {
                query.append_pair("_pagedResultsCookie", cookie);
            }
            query.finish()
        };

        let path = format!("{}?{}", clients_path(realm), query);
        let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
        let result = body
            .get("result")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Api {
                status: 0,
                body: format!("unexpected oauth client list shape: {body}"),
            })?;
        // A row with no `_id` names nothing and cannot be fetched, filtered or
        // acted on. It used to be dropped silently, which is the worst of the
        // three options: a listing that quietly loses rows can manufacture
        // "no client holds the grant" out of an incomplete answer, and nothing
        // says so. Refusing is loud, and a CREST collection row without an id
        // is not a thing this API produces.
        if let Some(nameless) = result
            .iter()
            .position(|row| row.get("_id").and_then(Value::as_str).is_none())
        {
            return Err(Error::Api {
                status: 0,
                body: format!(
                    "oauth client listing returned a row with no string `_id` (at index {nameless} of this page); refusing rather than silently dropping it"
                ),
            });
        }
        rows.extend(result.iter().cloned());

        // A present cookie of the wrong type used to read as end-of-results,
        // so a realm past one page silently handed a partial client list to
        // findings that then spoke about "every client". Absent, null and the
        // empty string are the real terminators; anything else is a shape this
        // reader does not understand.
        cookie = next_page_cookie(&body)?;
        if cookie.is_none() {
            break;
        }
    }

    rows.sort_by(|a, b| {
        a.get("_id")
            .and_then(Value::as_str)
            .cmp(&b.get("_id").and_then(Value::as_str))
    });
    Ok(rows)
}

pub async fn list_clients(tenant: &str, realm: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut cookie: Option<String> = None;

    loop {
        let query = {
            let mut query = Serializer::new(String::new());
            query
                .append_pair("_queryFilter", "true")
                .append_pair("_fields", "_id")
                .append_pair("_pageSize", "1000");
            if let Some(cookie) = cookie.as_deref() {
                query.append_pair("_pagedResultsCookie", cookie);
            }
            query.finish()
        };

        let path = format!("{}?{}", clients_path(realm), query);
        let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
        let result = body
            .get("result")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Api {
                status: 0,
                body: format!("unexpected oauth client list shape: {body}"),
            })?;

        ids.extend(
            result
                .iter()
                .filter_map(|client| client.get("_id").and_then(Value::as_str))
                .map(str::to_owned),
        );

        cookie = next_page_cookie(&body)?;
        if cookie.is_none() {
            break;
        }
    }

    ids.sort();
    Ok(ids)
}

pub async fn read_client(tenant: &str, realm: &str, id: &str) -> Result<Value> {
    validate_client_id(id)?;
    let path = format!("{}/{}", clients_path(realm), id);
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

/// Read the realm-wide OAuth2 / OIDC provider service configuration.
pub async fn read_provider(tenant: &str, realm: &str) -> Result<Value> {
    crate::aic::api::get_versioned(tenant, &provider_path(realm), API_VERSION).await
}

/// Fetch the tenant's complete default OAuth2 client body.
///
/// The action uses POST but does not mutate tenant state. `confirmed_prod` is
/// still required by the shared transport for any POST to a production tenant.
pub async fn client_template(tenant: &str, realm: &str, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{}?_action=template", clients_path(realm));
    crate::aic::api::post_versioned(tenant, &path, json!({}), confirmed_prod, API_VERSION).await
}

/// Fetch field metadata and live enum choices for OAuth2 clients.
///
/// Like [`client_template`], this read-like action is POST-shaped and therefore
/// carries the caller's production confirmation through the shared transport.
pub async fn client_schema(tenant: &str, realm: &str, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{}?_action=schema", clients_path(realm));
    crate::aic::api::post_versioned(tenant, &path, json!({}), confirmed_prod, API_VERSION).await
}

/// Upsert a complete OAuth2 client body with the caller's production choice.
pub async fn upsert_client(
    tenant: &str,
    realm: &str,
    id: &str,
    body: Value,
    confirmed_prod: bool,
) -> Result<Value> {
    validate_client_id(id)?;
    let path = format!("{}/{}", clients_path(realm), id);
    let body = sanitize_for_write(&body);
    crate::aic::api::put_versioned(tenant, &path, body, confirmed_prod, API_VERSION).await
}

/// Create or explicitly replace a client after the caller's existence check.
pub async fn create_client(
    tenant: &str,
    realm: &str,
    id: &str,
    body: Value,
    confirmed_prod: bool,
) -> Result<Value> {
    validate_client_id(id)?;
    let path = format!("{}/{}", clients_path(realm), id);
    let body = sanitize_for_write(&body);
    crate::aic::api::put_versioned(tenant, &path, body, confirmed_prod, API_VERSION).await
}

pub async fn delete_client(tenant: &str, realm: &str, id: &str) -> Result<()> {
    validate_client_id(id)?;
    let path = format!("{}/{}", clients_path(realm), id);
    crate::aic::api::delete_versioned(tenant, &path, false, API_VERSION).await?;
    Ok(())
}

fn strip_revs(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_revs).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "_rev")
                .map(|(key, value)| (key.clone(), strip_revs(value)))
                .collect(),
        ),
        // `serde_json::Value` compares `-0.0` and `0.0` equal but serialises
        // them differently, which would be the one input where the drift check
        // and the rendered diff disagreed. Collapse the sign so they cannot.
        Value::Number(number) if number.as_f64() == Some(0.0) && !number.is_i64() => {
            json!(0.0_f64)
        }
        value => value.clone(),
    }
}

/// Replace every secret value with a digest of itself.
///
/// Two kinds reach a rendering: the `*-encrypted` blobs AM returns on a `GET`,
/// which are AES-wrapped, and a plaintext `userpassword` in a **local** file
/// someone authored (AM reads that field back as `null`, so it is only ever
/// on the local side — which is the side `diff` prints).
///
/// `pull` already writes both to the workspace, so this is not about the bytes
/// existing on disk — it is that a rendered diff goes to a terminal, a pager's
/// history, a CI log, or a pasted snippet, which the workspace file does not.
///
/// A digest rather than a constant, so a rotated secret still shows as a
/// changed line. That also keeps `content_text` agreeing with
/// [`content_equal`]: equal values digest equally, and unequal ones do not.
///
/// The digest is **keyed on a random per-process value**, and that matters for
/// the plaintext case. An unsalted SHA-256 of an AES-wrapped blob gives an
/// attacker nothing, but a client secret a human chose is dictionary-testable
/// from a pasted terminal line — so publishing one would have handed out
/// exactly what the redaction exists to withhold. A per-process key keeps both
/// sides of one comparison consistent, which is all the change-detection
/// needs, and makes the printed value useless anywhere else. The cost is that
/// two separate runs print different placeholders for the same secret; a diff
/// only ever compares within one run.
pub(crate) fn redact_secrets(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(redact_secrets).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if crate::oauth::spec::is_secret_key(key) && !value.is_null() {
                        (key.clone(), json!(encrypted_digest(value)))
                    } else {
                        (key.clone(), redact_secrets(value))
                    }
                })
                .collect(),
        ),
        value => value.clone(),
    }
}

/// A random value mixed into every secret digest, minted once per process.
///
/// Not persisted anywhere: persisting it would make the digests comparable
/// across runs again, which is the property being removed.
fn digest_key() -> &'static str {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

fn encrypted_digest(value: &Value) -> String {
    // The whole digest, not a prefix. A truncated one is shorter to read and
    // makes the agreement with `content_equal` merely probable: two unequal
    // blobs sharing a 64-bit prefix would render identically while `push`
    // still saw drift. The line is long; the invariant is exact.
    let keyed = json!({ "key": digest_key(), "value": value });
    format!("<redacted: sha256:{}>", crate::access::spec::digest(&keyed))
}

pub(crate) fn content_equal(a: &Value, b: &Value) -> bool {
    strip_revs(a) == strip_revs(b)
}

/// The text `aic oauth diff` renders — one side of a comparison, normalised
/// exactly the way [`content_equal`] normalises.
///
/// The two must agree or the tool contradicts itself: `push` decides drift with
/// `content_equal`, so a diff that showed a `_rev` line would report a change
/// on a client `push` calls unchanged. Keys sort because `serde_json`'s map is
/// a `BTreeMap` here (no `preserve_order` feature), so the ordering is stable
/// across a pull, a snapshot and a fetch rather than following insertion.
pub(crate) fn content_text(value: &Value) -> String {
    let rendered = redact_secrets(&strip_revs(value));
    // A `Value` always serialises, so the fallible form would only add an
    // unreachable error path to every caller.
    serde_json::to_string_pretty(&rendered).unwrap_or_else(|_| rendered.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sanitize_for_write_strips_top_level_server_fields() {
        let value = json!({
            "_id": "client-a",
            "_rev": "-123",
            "_type": {"_id": "OAuth2Client"},
            "_provider": {"_id": "provider"},
            "coreOAuth2ClientConfig": {
                "clientName": {"inherited": false, "value": ["Client A"]},
                "userpassword": null
            },
            "coreUmaClientConfig": {
                "claimsRedirectionUris": {"inherited": true, "value": []}
            }
        });

        let stripped = sanitize_for_write(&value);

        assert!(stripped.get("_id").is_none());
        assert!(stripped.get("_rev").is_none());
        assert!(stripped.get("_type").is_none());
        assert!(stripped.get("_provider").is_none());
        assert_eq!(
            stripped["coreOAuth2ClientConfig"]["clientName"],
            json!({"inherited": false, "value": ["Client A"]})
        );
        assert_eq!(
            stripped["coreUmaClientConfig"]["claimsRedirectionUris"],
            json!({"inherited": true, "value": []})
        );
    }

    #[test]
    fn sanitize_for_write_removes_encrypted_fields_at_any_depth() {
        let value = json!({
            "top-encrypted": "top",
            "coreOAuth2ClientConfig": {
                "userpassword": null,
                "userpassword-encrypted": "AQIC...",
                "nested": {
                    "clientSecret-encrypted": "nested",
                    "clientSecret": null
                }
            },
            "advancedOAuth2ClientConfig": {
                "array": [
                    {
                        "item-encrypted": "item",
                        "item": "kept"
                    },
                    {
                        "inner": {
                            "another-encrypted": "another",
                            "another": true
                        }
                    }
                ]
            }
        });

        let stripped = sanitize_for_write(&value);

        assert!(stripped.get("top-encrypted").is_none());
        assert!(
            stripped["coreOAuth2ClientConfig"]
                .get("userpassword-encrypted")
                .is_none()
        );
        assert!(
            stripped["coreOAuth2ClientConfig"]["nested"]
                .get("clientSecret-encrypted")
                .is_none()
        );
        assert!(
            stripped["advancedOAuth2ClientConfig"]["array"][0]
                .get("item-encrypted")
                .is_none()
        );
        assert!(
            stripped["advancedOAuth2ClientConfig"]["array"][1]["inner"]
                .get("another-encrypted")
                .is_none()
        );
        assert_eq!(
            stripped["coreOAuth2ClientConfig"]["userpassword"],
            Value::Null
        );
        assert_eq!(
            stripped["coreOAuth2ClientConfig"]["nested"]["clientSecret"],
            Value::Null
        );
        assert_eq!(
            stripped["advancedOAuth2ClientConfig"]["array"][0]["item"],
            "kept"
        );
        assert!(
            stripped["advancedOAuth2ClientConfig"]["array"][1]["inner"]["another"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn content_equal_ignores_rev_fields_recursively() {
        let a = json!({
            "_rev": "one",
            "coreOAuth2ClientConfig": {
                "_rev": "two",
                "clientType": {"inherited": false, "value": "Confidential"}
            },
            "array": [
                {"_rev": "three", "value": 1}
            ]
        });
        let b = json!({
            "_rev": "changed",
            "coreOAuth2ClientConfig": {
                "_rev": "changed",
                "clientType": {"inherited": false, "value": "Confidential"}
            },
            "array": [
                {"_rev": "changed", "value": 1}
            ]
        });

        assert!(content_equal(&a, &b));
    }

    #[test]
    fn content_equal_catches_real_differences() {
        let a = json!({
            "_rev": "one",
            "coreOAuth2ClientConfig": {
                "clientType": {"inherited": false, "value": "Confidential"}
            }
        });
        let b = json!({
            "_rev": "two",
            "coreOAuth2ClientConfig": {
                "clientType": {"inherited": false, "value": "Public"}
            }
        });

        assert!(!content_equal(&a, &b));
    }

    /// The invariant that keeps `diff` and `push` from contradicting each
    /// other. The discriminating pair is the first one: it differs only in
    /// `_rev`, so a `content_text` that pretty-printed the raw value would
    /// render a change on a client `push` reports as already matching.
    #[test]
    fn diff_text_says_equal_exactly_when_the_drift_check_does() {
        let pairs = [
            (
                json!({"_rev": "one", "a": {"_rev": "x", "v": 1}}),
                json!({"_rev": "two", "a": {"_rev": "y", "v": 1}}),
            ),
            (json!({"a": {"v": 1}}), json!({"a": {"v": 2}})),
            (json!({"a": 1, "b": 2}), json!({"b": 2, "a": 1})),
            (json!({"a": [1, 2]}), json!({"a": [2, 1]})),
            // The counterexample the reviewer found: `Value` compares these
            // equal and `serde_json` serialises them differently, so an
            // un-normalised `content_text` would render drift on a client
            // `push` reports as unchanged.
            (json!({"a": -0.0}), json!({"a": 0.0})),
            // The same value under a `*-encrypted` key must survive redaction
            // as equal, and a different one must not.
            (
                json!({"userpassword-encrypted": "AQIC-one"}),
                json!({"userpassword-encrypted": "AQIC-one"}),
            ),
            (
                json!({"userpassword-encrypted": "AQIC-one"}),
                json!({"userpassword-encrypted": "AQIC-two"}),
            ),
        ];
        for (a, b) in pairs {
            assert_eq!(
                content_equal(&a, &b),
                content_text(&a) == content_text(&b),
                "{a} vs {b}"
            );
        }
    }

    /// `pull` writes these blobs to the workspace, but a rendered diff reaches
    /// a terminal, a pager history and a CI log, which the workspace file does
    /// not. The digest keeps a rotated secret visible as a changed line
    /// without printing either value.
    #[test]
    fn an_encrypted_value_never_reaches_the_rendered_text() {
        let text = content_text(&json!({
            "coreOAuth2ClientConfig": {
                "userpassword": null,
                "userpassword-encrypted": "AQICSecretWrappedBytes"
            }
        }));

        assert!(!text.contains("AQICSecretWrappedBytes"), "{text}");
        assert!(text.contains("<redacted: sha256:"), "{text}");
        // The key stays, so the diff still says which field changed.
        assert!(text.contains("userpassword-encrypted"), "{text}");
    }

    /// The redaction must not double as an offline oracle. A plaintext
    /// `userpassword` is a value a human chose, so an unsalted digest of it is
    /// dictionary-testable straight out of a pasted terminal line — which is
    /// the thing the redaction exists to prevent. The discriminating pair:
    /// within one process two equal secrets still render alike, so a diff
    /// still detects a rotation, while the printed digest is not the plain
    /// SHA-256 anyone can precompute.
    #[test]
    fn the_secret_digest_is_not_a_precomputable_hash_of_the_secret() {
        let secret = json!("hunter2");
        let text = content_text(&json!({
            "coreOAuth2ClientConfig": { "userpassword": secret }
        }));

        assert!(!text.contains("hunter2"), "{text}");
        assert!(
            !text.contains(&crate::access::spec::digest(&secret)),
            "the plain digest of the secret is in the output: {text}"
        );
        // ...and it still detects a change, which is why it is a digest at all.
        let same = content_text(&json!({
            "coreOAuth2ClientConfig": { "userpassword": "hunter2" }
        }));
        let rotated = content_text(&json!({
            "coreOAuth2ClientConfig": { "userpassword": "hunter3" }
        }));
        assert_eq!(text, same);
        assert_ne!(text, rotated);
    }

    /// Not one special-cased key: the rule is the suffix, at any depth,
    /// including inside an array — which is where an implementation that only
    /// walked objects at the top level would leak. The negative case is the
    /// point of the suffix rule: `userinfoEncryptedResponseAlg` is an
    /// algorithm name, and masking it would hide real configuration.
    #[test]
    fn redaction_follows_the_suffix_wherever_it_appears() {
        let text = content_text(&json!({
            "signEncOAuth2ClientConfig": {
                "userinfoEncryptedResponseAlg": "RSA-OAEP-256",
                "jwks": [
                    {"kid": "one", "key-encrypted": "AQICNestedInAnArray"}
                ]
            },
            "another-encrypted": "AQICSecondKey",
            "nulls-encrypted": null
        }));

        assert!(!text.contains("AQICNestedInAnArray"), "{text}");
        assert!(!text.contains("AQICSecondKey"), "{text}");
        assert!(text.contains("RSA-OAEP-256"), "{text}");
        // A null is already the absence of a secret; masking it would invent
        // one, and hide that the field is unset.
        assert!(text.contains("\"nulls-encrypted\": null"), "{text}");
    }

    /// Rotating the secret must still show up as a changed line — a constant
    /// mask would hide the one thing a diff of that field is for.
    #[test]
    fn a_rotated_encrypted_value_still_renders_as_a_change() {
        let before = content_text(&json!({"userpassword-encrypted": "AQIC-before"}));
        let after = content_text(&json!({"userpassword-encrypted": "AQIC-after"}));
        assert_ne!(before, after);
    }

    /// Pretty-printed, so a diff is line-oriented rather than one long line
    /// git reports as wholly changed.
    #[test]
    fn diff_text_is_one_key_per_line() {
        let text = content_text(&json!({"a": 1, "b": 2}));
        assert_eq!(text.lines().count(), 4, "{text}");
    }
}
