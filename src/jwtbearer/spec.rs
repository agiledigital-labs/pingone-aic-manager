//! TUI-free input types and pure Trusted JWT Issuer transforms.
//!
//! In particular, [`merge_jwk_set`] is deliberately independent of the HTTP
//! layer so future key removal can share its parser and validation rules.

use serde_json::{Map, Value, json};

use crate::config::TenantTheme;
use crate::{Error, Result};

const SERVER_FIELDS: [&str; 4] = ["_id", "_rev", "_type", "_provider"];

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

/// Convert AM's inherited read wrappers and server fields into a plain PUT
/// body, then apply the fields that are load-bearing for this feature.
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
    object.insert("allowedSubjects".into(), json!([]));
    // This names the assertion claim that narrows a requested grant: an
    // assertion claiming `scope: "openid"` while requesting `openid profile`
    // receives only `openid`; it never grants scopes by itself. `sub` must
    // remain the user's UUID so IDM accepts the resulting token.
    object.insert("consentedScopesClaim".into(), Value::String("scope".into()));
    object.insert(
        "resourceOwnerIdentityClaim".into(),
        Value::String("sub".into()),
    );
    Ok(source)
}

/// The Trusted JWT capability is never allowed on a production-themed tenant.
pub fn ensure_not_production(theme: TenantTheme) -> Result<()> {
    if theme == TenantTheme::Production {
        return Err(Error::Config(
            "jwt-bearer setup is refused on production-themed tenants".into(),
        ));
    }
    Ok(())
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

    use super::*;

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
    fn production_is_refused_without_a_confirmable_path() {
        assert!(ensure_not_production(TenantTheme::Production).is_err());
        assert!(ensure_not_production(TenantTheme::Sandbox).is_ok());
        assert!(ensure_not_production(TenantTheme::Development).is_ok());
        assert!(ensure_not_production(TenantTheme::Staging).is_ok());
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
    }
}
