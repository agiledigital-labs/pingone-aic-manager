//! Trusted JWT Issuer orchestration: generate, merge, verify, retry, store.

use std::future::Future;
use std::path::Path;

use chrono::Utc;
use rand::RngCore;
use serde_json::Value;
use uuid::Uuid;

use crate::agent::AgentClient;
use crate::config::{
    Tenant,
    operator::{NameSource, ResolvedOperator},
};
use crate::jwtbearer::{KeyRecord, api, spec};
use crate::{Error, Result};

pub const DEFAULT_ISSUER_ID: &str = "aic-agent";
pub const DEFAULT_ISSUER: &str = "aic-agent";

/// Set up the shared default issuer and this install's one per-tenant key.
pub async fn setup(tenant: &Tenant, realm: &str, operator: &ResolvedOperator) -> Result<String> {
    spec::ensure_not_production(tenant.theme)?;
    if operator.source == NameSource::Placeholder {
        return Err(Error::Config(
            "operator name is required for jwt-bearer setup; run `aic settings set operator.name <name>`".into(),
        ));
    }

    let record =
        match crate::jwtbearer::get_key(AgentClient::connect_or_spawn().await?, &tenant.name)
            .await?
        {
            Some(record) => record,
            None => generate_key(operator)?,
        };
    let public_jwk = public_jwk(&record)?;

    // Store first: if publishing fails, the next setup can retry this same
    // key instead of generating an orphaned public key in the shared JWKS.
    crate::jwtbearer::put_key(
        AgentClient::connect_or_spawn().await?,
        &tenant.name,
        &record,
    )
    .await?;

    let tenant_name = tenant.name.clone();
    let realm_name = realm.to_string();
    let public_jwk_for_publish = public_jwk.clone();
    publish_and_verify(
        &record.kid,
        || {
            let tenant = tenant_name.clone();
            let realm = realm_name.clone();
            let public_jwk = public_jwk_for_publish.clone();
            async move {
                let remote = read_or_template(&tenant, &realm, DEFAULT_ISSUER_ID).await?;
                let body = body_with_key(remote, DEFAULT_ISSUER, &public_jwk)?;
                api::upsert_issuer(&tenant, &realm, DEFAULT_ISSUER_ID, body).await?;
                Ok(())
            }
        },
        || {
            let tenant = tenant_name.clone();
            let realm = realm_name.clone();
            let kid = record.kid.clone();
            async move { key_is_present(&tenant, &realm, &kid).await }
        },
    )
    .await?;
    Ok(record.kid)
}

async fn publish_and_verify<Publish, PublishFuture, Verify, VerifyFuture>(
    kid: &str,
    mut publish: Publish,
    mut verify: Verify,
) -> Result<()>
where
    Publish: FnMut() -> PublishFuture,
    PublishFuture: Future<Output = Result<()>>,
    Verify: FnMut() -> VerifyFuture,
    VerifyFuture: Future<Output = Result<bool>>,
{
    publish().await?;
    if verify().await? {
        return Ok(());
    }

    publish().await?;
    if verify().await? {
        return Ok(());
    }

    Err(Error::Config(format!(
        "Trusted JWT issuer write did not retain kid {kid}; concurrent update may have dropped it"
    )))
}

/// Create a named issuer from a public JWKS file.
pub async fn create_issuer(
    tenant: &Tenant,
    realm: &str,
    id: &str,
    issuer: &str,
    jwks_path: &Path,
) -> Result<()> {
    spec::ensure_not_production(tenant.theme)?;
    let existing = match api::read_issuer(&tenant.name, realm, id).await {
        Ok(_) => true,
        Err(error) if is_not_found(&error) => false,
        Err(error) => return Err(error),
    };
    if existing {
        return Err(Error::Config(format!(
            "Trusted JWT issuer {id:?} already exists"
        )));
    }

    let contents = std::fs::read_to_string(jwks_path).map_err(|error| {
        Error::Config(format!("read JWKS file {}: {error}", jwks_path.display()))
    })?;
    let jwks = spec::parse_jwk_set(Some(&contents))?;
    let jwk_set = serde_json::to_string(&jwks)?;
    let template = api::issuer_template(&tenant.name, realm).await?;
    let body = spec::issuer_body(template, issuer, jwk_set)?;
    api::upsert_issuer(&tenant.name, realm, id, body).await?;
    Ok(())
}

async fn read_or_template(tenant: &str, realm: &str, id: &str) -> Result<Value> {
    match api::read_issuer(tenant, realm, id).await {
        Ok(value) => Ok(value),
        Err(error) if is_not_found(&error) => api::issuer_template(tenant, realm).await,
        Err(error) => Err(error),
    }
}

fn body_with_key(source: Value, issuer: &str, public_jwk: &Value) -> Result<Value> {
    let existing = source.get("jwkSet").and_then(Value::as_str).or_else(|| {
        source
            .get("jwkSet")
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
    });
    let jwk_set = spec::merge_jwk_set(existing, public_jwk.clone())?;
    spec::issuer_body(source, issuer, jwk_set)
}

async fn key_is_present(tenant: &str, realm: &str, kid: &str) -> Result<bool> {
    let issuer = api::read_issuer(tenant, realm, DEFAULT_ISSUER_ID).await?;
    spec::jwk_set_contains(&issuer, kid)
}

/// Check the default issuer once after a local key import. A missing issuer or
/// an unreadable/malformed set is deliberately non-fatal: import transfers a
/// local credential and must remain useful when the sender's publication is
/// unavailable.
pub async fn warn_if_key_not_published(tenant: &str, realm: &str, kid: &str) {
    let Ok(issuer) = api::read_issuer(tenant, realm, DEFAULT_ISSUER_ID).await else {
        return;
    };
    let Ok(published) = spec::jwk_set_contains(&issuer, kid) else {
        return;
    };
    if !published {
        eprintln!(
            "warning: imported kid {kid:?} is not published in the default Trusted JWT issuer for tenant {tenant} (realm {realm}); `aic auth` will fail until it is. Run `aic jwt-bearer setup --realm {realm}` to publish the key you just imported, or ask whoever sent it to publish theirs."
        );
    }
}

fn generate_key(operator: &ResolvedOperator) -> Result<KeyRecord> {
    let kid = generate_kid();
    let created = Utc::now().to_rfc3339();
    let mut private_jwk = crate::onboard::bootstrap::generate_rsa_jwk(&kid)?;
    add_attribution(&mut private_jwk, operator, &created)?;
    Ok(KeyRecord { kid, private_jwk })
}

fn generate_kid() -> String {
    let mut random = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    Uuid::from_bytes(random).to_string()
}

fn public_jwk(record: &KeyRecord) -> Result<Value> {
    if record.private_jwk.get("kid").and_then(Value::as_str) != Some(record.kid.as_str()) {
        return Err(Error::Config(
            "stored Trusted JWT private JWK kid does not match its key record".into(),
        ));
    }
    for field in ["kty", "n", "e"] {
        if record
            .private_jwk
            .get(field)
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(Error::Config(format!(
                "stored Trusted JWT private JWK is missing {field}"
            )));
        }
    }
    let mut public_jwk = crate::aic::auth::public_jwk(&record.private_jwk);
    let object = public_jwk
        .as_object_mut()
        .ok_or_else(|| Error::Config("generated public JWK is not a JSON object".into()))?;
    for field in ["aic_owner", "aic_host", "aic_created"] {
        let value = record.private_jwk.get(field).ok_or_else(|| {
            Error::Config(format!("private JWK missing attribution member {field}"))
        })?;
        object.insert(field.to_string(), value.clone());
    }
    Ok(public_jwk)
}

fn add_attribution(
    private_jwk: &mut Value,
    operator: &ResolvedOperator,
    created: &str,
) -> Result<()> {
    let object = private_jwk
        .as_object_mut()
        .ok_or_else(|| Error::Config("generated private JWK is not a JSON object".into()))?;
    object.insert("aic_owner".into(), Value::String(operator.name.clone()));
    object.insert("aic_host".into(), Value::String(operator.host.clone()));
    object.insert("aic_created".into(), Value::String(created.to_string()));
    Ok(())
}

fn is_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use serde_json::json;

    use super::{add_attribution, generate_kid, publish_and_verify};
    use crate::config::operator::{NameSource, ResolvedOperator};

    #[test]
    fn attribution_adds_the_three_aic_members_to_a_jwk_object() {
        let mut jwk = json!({"kty": "RSA", "kid": "opaque"});
        add_attribution(
            &mut jwk,
            &ResolvedOperator {
                name: "owner".into(),
                host: "host".into(),
                source: NameSource::Settings,
            },
            "created",
        )
        .unwrap();
        assert_eq!(jwk["aic_owner"], "owner");
        assert_eq!(jwk["aic_host"], "host");
        assert_eq!(jwk["aic_created"], "created");
    }

    #[test]
    fn generated_kids_are_opaque() {
        let kid = generate_kid();
        assert!(!kid.is_empty());
        assert!(!kid.contains(':'));
    }

    #[tokio::test]
    async fn publish_verification_accepts_a_key_present_first_time() {
        let publishes = Cell::new(0);
        publish_and_verify(
            "kid-first",
            || {
                publishes.set(publishes.get() + 1);
                async { Ok(()) }
            },
            || async { Ok(true) },
        )
        .await
        .unwrap();
        assert_eq!(publishes.get(), 1);
    }

    #[tokio::test]
    async fn publish_verification_retries_when_the_key_is_missing() {
        let publishes = Rc::new(Cell::new(0));
        let verifies = Rc::new(Cell::new(0));
        publish_and_verify(
            "kid-retry",
            {
                let publishes = publishes.clone();
                move || {
                    publishes.set(publishes.get() + 1);
                    async { Ok(()) }
                }
            },
            {
                let verifies = verifies.clone();
                move || {
                    verifies.set(verifies.get() + 1);
                    let present = verifies.get() == 2;
                    async move { Ok(present) }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(publishes.get(), 2);
        assert_eq!(verifies.get(), 2);
    }

    #[tokio::test]
    async fn publish_verification_names_a_key_that_never_persists() {
        let error = publish_and_verify("kid-missing", || async { Ok(()) }, || async { Ok(false) })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("kid-missing"));
    }
}
