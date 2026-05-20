use serde_json::json;

use super::AicClient;
use crate::Result;

/// Create a service account and return its UUID.
pub async fn create_service_account(
    client: &AicClient,
    name: &str,
    description: &str,
    scopes: &[String],
    public_jwk: &serde_json::Value,
    confirmed_prod: bool,
) -> Result<String> {
    let jwks_str = serde_json::to_string(&json!({ "keys": [public_jwk] }))?;

    let body = json!({
        "name": name,
        "description": description,
        "accountStatus": "active",
        "scopes": scopes,
        "jwks": jwks_str,
    });

    let resp = client
        .write(
            reqwest::Method::POST,
            "/openidm/managed/svcacct?_action=create",
            body,
            confirmed_prod,
        )
        .await?;

    let sa_id = resp["_id"]
        .as_str()
        .ok_or_else(|| crate::Error::Api {
            status: 0,
            body: format!("no _id in svcacct response: {resp}"),
        })?
        .to_string();

    Ok(sa_id)
}
