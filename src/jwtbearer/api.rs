//! Verified HTTP wrappers for realm-scoped Trusted JWT Issuer agents.
//! See `docs/api/17-jwt-bearer-user-tokens.md`.

use std::path::is_separator;

use serde_json::{Value, json};

use crate::config::tenant::Tenant;
use crate::jwtbearer::spec::TokenRequest;
use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.1,resource=1.0";

/// The bare realm segment — what `docs/api/17-jwt-bearer-user-tokens.md` writes
/// as `{realm-path}`. It carries no `/am/json` prefix, because the two families
/// that use it sit under different roots: `realm-config` under `/am/json`, and
/// the OAuth2 endpoints under `/am/oauth2`.
fn realm_segment(realm: &str) -> String {
    format!("/realms/root/realms/{realm}")
}

fn realm_path(realm: &str) -> String {
    format!("/am/json{}", realm_segment(realm))
}

fn issuers_path(realm: &str) -> String {
    format!("{}/realm-config/agents/TrustedJwtIssuer", realm_path(realm))
}

pub fn discovery_path(realm: &str) -> String {
    format!(
        "/am/oauth2{}/.well-known/openid-configuration",
        realm_segment(realm)
    )
}

pub fn token_path(realm: &str) -> String {
    format!("/am/oauth2{}/access_token", realm_segment(realm))
}

pub async fn discovery(tenant: &str, realm: &str) -> Result<Value> {
    crate::aic::api::get(tenant, &discovery_path(realm)).await
}

pub async fn lookup_username(tenant: &str, realm: &str, username: &str) -> Result<Value> {
    let path = crate::jwtbearer::spec::username_lookup_path(realm, username);
    crate::aic::api::get(tenant, &path).await
}

pub async fn mint_user_token(
    tenant: &Tenant,
    realm: &str,
    request: &TokenRequest,
) -> Result<Value> {
    // This token exchange deliberately bypasses the agent API proxy: its wire
    // shape cannot carry an OAuth client's Authorization header, and the
    // exchange neither needs nor may receive the service-account bearer. The
    // private JWK is null because this form-only transport never calls bearer().
    crate::aic::AicClient::new(tenant.clone(), Value::Null)
        .write_form_with_authorization(
            reqwest::Method::POST,
            &token_path(realm),
            &request.body,
            false,
            request.authorization.as_deref(),
        )
        .await
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

#[cfg(test)]
mod tests {
    use super::*;

    /// These four paths sit under two different roots and are easy to compose
    /// wrongly: `realm-config` hangs off `/am/json`, the OAuth2 endpoints off
    /// `/am/oauth2`. A discovery path built from the `/am/json` helper 404s,
    /// which reads as "wrong realm" rather than "wrong root".
    #[test]
    fn realm_scoped_paths_use_the_right_root() {
        assert_eq!(
            issuers_path("alpha"),
            "/am/json/realms/root/realms/alpha/realm-config/agents/TrustedJwtIssuer"
        );
        assert_eq!(
            discovery_path("alpha"),
            "/am/oauth2/realms/root/realms/alpha/.well-known/openid-configuration"
        );
        assert_eq!(
            token_path("bravo"),
            "/am/oauth2/realms/root/realms/bravo/access_token"
        );
        for path in [discovery_path("alpha"), token_path("alpha")] {
            assert!(!path.contains("/am/json"), "{path} must not carry /am/json");
        }
    }
}
