//! Verified HTTP wrappers for realm-scoped Trusted JWT Issuer agents.
//! See `docs/api/17-jwt-bearer-user-tokens.md`.

use std::path::is_separator;

use serde_json::{Value, json};

use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.1,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn issuers_path(realm: &str) -> String {
    format!("{}/realm-config/agents/TrustedJwtIssuer", realm_path(realm))
}

pub fn discovery_path(realm: &str) -> String {
    format!(
        "/am/oauth2{}/.well-known/openid-configuration",
        realm_path(realm)
    )
}

pub fn token_path(realm: &str) -> String {
    format!("/am/oauth2/realms/root/realms/{realm}/access_token")
}

pub async fn discovery(tenant: &str, realm: &str) -> Result<Value> {
    crate::aic::api::get(tenant, &discovery_path(realm)).await
}

pub async fn lookup_username(tenant: &str, realm: &str, username: &str) -> Result<Value> {
    let path = crate::jwtbearer::spec::username_lookup_path(realm, username);
    crate::aic::api::get(tenant, &path).await
}

pub async fn mint_user_token(tenant: &str, realm: &str, body: &str) -> Result<Value> {
    crate::aic::api::post_form(tenant, &token_path(realm), body).await
}

fn validate_issuer_id(id: &str) -> Result<()> {
    if id.is_empty() || id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "Trusted JWT issuer id {id:?} is empty or contains a path separator"
        )));
    }
    Ok(())
}

/// List issuer objects exactly as returned by AM.
pub async fn list_issuers(tenant: &str, realm: &str) -> Result<Value> {
    let path = format!("{}?_queryFilter=true", issuers_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

/// Read one issuer object exactly as returned by AM.
pub async fn read_issuer(tenant: &str, realm: &str, id: &str) -> Result<Value> {
    validate_issuer_id(id)?;
    let path = format!("{}/{}", issuers_path(realm), id);
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

/// Fetch AM's default issuer object before creating one.
pub async fn issuer_template(tenant: &str, realm: &str) -> Result<Value> {
    let path = format!("{}?_action=template", issuers_path(realm));
    crate::aic::api::post_versioned(tenant, &path, json!({}), false, API_VERSION).await
}

/// Create or update an issuer. AM agents use plain PUT without `If-Match`.
pub async fn upsert_issuer(tenant: &str, realm: &str, id: &str, body: Value) -> Result<Value> {
    validate_issuer_id(id)?;
    let path = format!("{}/{}", issuers_path(realm), id);
    crate::aic::api::put_versioned(tenant, &path, body, false, API_VERSION).await
}
