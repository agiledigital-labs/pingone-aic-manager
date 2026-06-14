//! Verified HTTP wrappers for AM OAuth2 clients.
//! See `docs/api/05-oauth2-oidc.md`.

use std::path::is_separator;

use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.1,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn clients_path(realm: &str) -> String {
    format!("{}/realm-config/agents/OAuth2Client", realm_path(realm))
}

fn validate_client_id(id: &str) -> Result<()> {
    if id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "oauth client id {id:?} contains a path separator"
        )));
    }
    Ok(())
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

        cookie = body
            .get("pagedResultsCookie")
            .and_then(Value::as_str)
            .filter(|cookie| !cookie.is_empty())
            .map(str::to_owned);
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

fn strip_encrypted_fields(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_encrypted_fields).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !key.ends_with("-encrypted"))
                .map(|(key, value)| (key.clone(), strip_encrypted_fields(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn sanitize_for_write(client: &Value) -> Value {
    let Value::Object(map) = client else {
        return strip_encrypted_fields(client);
    };

    Value::Object(
        map.iter()
            .filter(|(key, _)| !matches!(key.as_str(), "_id" | "_rev" | "_type" | "_provider"))
            .filter(|(key, _)| !key.ends_with("-encrypted"))
            .map(|(key, value)| (key.clone(), strip_encrypted_fields(value)))
            .collect(),
    )
}

pub async fn upsert_client(tenant: &str, realm: &str, id: &str, body: Value) -> Result<Value> {
    validate_client_id(id)?;
    let path = format!("{}/{}", clients_path(realm), id);
    let body = sanitize_for_write(&body);
    crate::aic::api::put_versioned(tenant, &path, body, false, API_VERSION).await
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
        value => value.clone(),
    }
}

pub(crate) fn content_equal(a: &Value, b: &Value) -> bool {
    strip_revs(a) == strip_revs(b)
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
}
