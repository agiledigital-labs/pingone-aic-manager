//! Verified HTTP wrappers for AM secret mappings.
//! See `docs/api/15-secret-mappings.md`.

use std::path::is_separator;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, Result};

pub const API_VERSION: &str = "protocol=2.0,resource=1.0";

fn store_path(realm: &str) -> String {
    format!(
        "/am/json/realms/root/realms/{realm}/realm-config/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV"
    )
}

fn mappings_path(realm: &str) -> String {
    format!("{}/mappings", store_path(realm))
}

fn validate_secret_id(secret_id: &str) -> Result<()> {
    if secret_id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "secret label {secret_id:?} contains a path separator"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mapping {
    pub secret_id: String,
    pub alias: Option<String>,
}

pub fn parse_mapping(value: &Value) -> Mapping {
    let secret_id = value
        .get("secretId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let alias = value
        .get("aliases")
        .and_then(Value::as_array)
        .and_then(|aliases| aliases.first())
        .and_then(Value::as_str)
        .map(str::to_owned);

    Mapping { secret_id, alias }
}

pub fn parse_mappings(value: &Value) -> Vec<Mapping> {
    let mut mappings: Vec<Mapping> = value
        .get("result")
        .and_then(Value::as_array)
        .map(|result| result.iter().map(parse_mapping).collect())
        .unwrap_or_default();
    mappings.sort_by(|a, b| a.secret_id.cmp(&b.secret_id));
    mappings
}

pub async fn list_mappings(tenant: &str, realm: &str) -> Result<Vec<Mapping>> {
    let path = format!("{}?_queryFilter=true", mappings_path(realm));
    let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
    if body.get("result").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected secret mapping list shape: {body}"),
        });
    }
    Ok(parse_mappings(&body))
}

pub async fn read_mapping(tenant: &str, realm: &str, secret_id: &str) -> Result<Value> {
    validate_secret_id(secret_id)?;
    let path = format!("{}/{}", mappings_path(realm), secret_id);
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

pub async fn valid_secret_ids(tenant: &str, realm: &str) -> Result<Vec<String>> {
    let path = format!("{}?_action=schema", mappings_path(realm));
    let body =
        crate::aic::api::post_versioned(tenant, &path, json!({}), false, API_VERSION).await?;
    let values = body
        .pointer("/properties/secretId/enum")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("secret mapping schema missing properties.secretId.enum: {body}"),
        })?;

    let mut ids = Vec::with_capacity(values.len());
    for value in values {
        let Some(id) = value.as_str() else {
            return Err(Error::Api {
                status: 0,
                body: format!("secret mapping schema contains a non-string secretId enum: {body}"),
            });
        };
        ids.push(id.to_owned());
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub async fn set_mapping(
    tenant: &str,
    realm: &str,
    secret_id: &str,
    alias: &str,
    confirmed_prod: bool,
) -> Result<Value> {
    validate_secret_id(secret_id)?;
    let path = format!("{}/{}", mappings_path(realm), secret_id);
    // `secretId` MUST be in the body, not just the path: the store rejects a
    // create with `400 "Invalid config: Secret value is missing"` when it's
    // absent. Verified live (2026-06-17) for every label/alias pairing; the
    // console sends it too. Harmless on update.
    let body = json!({ "aliases": [alias], "secretId": secret_id });
    crate::aic::api::put_versioned(tenant, &path, body, confirmed_prod, API_VERSION).await
}

pub async fn delete_mapping(tenant: &str, realm: &str, secret_id: &str) -> Result<()> {
    delete_mapping_confirmed(tenant, realm, secret_id, false).await
}

pub async fn delete_mapping_confirmed(
    tenant: &str,
    realm: &str,
    secret_id: &str,
    confirmed_prod: bool,
) -> Result<()> {
    validate_secret_id(secret_id)?;
    let path = format!("{}/{}", mappings_path(realm), secret_id);
    crate::aic::api::delete_versioned(tenant, &path, confirmed_prod, API_VERSION).await?;
    Ok(())
}

fn mapping_content(value: &Value) -> Value {
    let secret_id = value
        .get("secretId")
        .or_else(|| value.get("_id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let aliases = value
        .get("aliases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    json!({
        "secretId": secret_id,
        "aliases": aliases
    })
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
        value => value.clone(),
    }
}

pub fn content_equal(a: &Value, b: &Value) -> bool {
    strip_revs(&mapping_content(a)) == strip_revs(&mapping_content(b))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_mapping_extracts_secret_id_and_first_alias() {
        let parsed = parse_mapping(&json!({
            "_id": "am.example.secret",
            "_rev": "one",
            "secretId": "am.example.secret",
            "aliases": ["esv-primary", "esv-ignored"]
        }));

        assert_eq!(
            parsed,
            Mapping {
                secret_id: "am.example.secret".into(),
                alias: Some("esv-primary".into())
            }
        );
    }

    #[test]
    fn parse_mapping_uses_none_for_empty_aliases() {
        let parsed = parse_mapping(&json!({
            "secretId": "am.example.unset",
            "aliases": []
        }));

        assert_eq!(parsed.secret_id, "am.example.unset");
        assert_eq!(parsed.alias, None);
    }

    #[test]
    fn parse_mappings_reads_result_array_and_sorts_by_secret_id() {
        let parsed = parse_mappings(&json!({
            "result": [
                {"secretId": "am.z", "aliases": ["esv-z"]},
                {"secretId": "am.a", "aliases": []}
            ]
        }));

        assert_eq!(
            parsed,
            vec![
                Mapping {
                    secret_id: "am.a".into(),
                    alias: None
                },
                Mapping {
                    secret_id: "am.z".into(),
                    alias: Some("esv-z".into())
                }
            ]
        );
    }

    #[test]
    fn validate_secret_id_rejects_path_separators() {
        let error = validate_secret_id("am.example/folder.secret").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn validate_secret_id_accepts_dotted_ids() {
        validate_secret_id("am.applications.oauth2.client.alpha.vktest.secret").unwrap();
    }

    #[test]
    fn content_equal_ignores_rev_fields_recursively() {
        let a = json!({
            "_rev": "one",
            "secretId": "am.example.secret",
            "aliases": ["esv-primary"],
            "_type": {
                "_rev": "two",
                "name": "Mappings"
            }
        });
        let b = json!({
            "_rev": "changed",
            "secretId": "am.example.secret",
            "aliases": ["esv-primary"],
            "_type": {
                "_rev": "changed",
                "name": "Mappings"
            }
        });

        assert!(content_equal(&a, &b));
    }

    #[test]
    fn content_equal_compares_only_mapping_content() {
        let raw = json!({
            "_id": "am.example.secret",
            "_rev": "one",
            "secretId": "am.example.secret",
            "aliases": ["esv-primary"],
            "_type": {"_id": "mappings"}
        });
        let snapshot = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-primary"]
        });

        assert!(content_equal(&raw, &snapshot));
    }

    #[test]
    fn content_equal_catches_alias_changes() {
        let a = json!({
            "_rev": "one",
            "secretId": "am.example.secret",
            "aliases": ["esv-primary"]
        });
        let b = json!({
            "_rev": "two",
            "secretId": "am.example.secret",
            "aliases": ["esv-secondary"]
        });

        assert!(!content_equal(&a, &b));
    }
}
