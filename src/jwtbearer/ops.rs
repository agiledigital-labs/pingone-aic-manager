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

pub(crate) const SETUP_PRODUCTION_REMEDY: &str =
    "pass --id/--username so the issuer is restricted, or run `aic jwt-bearer subjects add` first";
pub(crate) const ROTATE_PRODUCTION_REMEDY: &str =
    "restrict the issuer with `aic jwt-bearer subjects add` before rotating";
pub(crate) const CREATE_PRODUCTION_REMEDY: &str =
    "pass --id/--username so the new issuer is restricted";
pub(crate) const SUBJECTS_RM_PRODUCTION_REMEDY: &str =
    "the last non-empty subject cannot be removed on a production-themed tenant";

/// Production gate for `setup`: the issuer must be restricted *after* merging
/// `incoming` into whatever is already published (or `[]` if it is absent).
pub fn check_setup_on_production(
    theme: crate::config::TenantTheme,
    existing: Option<&Value>,
    incoming: &[String],
) -> Result<()> {
    let planned = spec::subjects_after_add(
        &existing
            .map(spec::issuer_allowed_subjects)
            .unwrap_or_default(),
        incoming,
    )?;
    spec::ensure_production_write_restricted(theme, &planned, SETUP_PRODUCTION_REMEDY)
}

/// Production gate for `rotate`: the existing issuer must already be restricted.
pub fn check_rotate_on_production(
    theme: crate::config::TenantTheme,
    existing: &Value,
) -> Result<()> {
    spec::ensure_production_write_restricted(
        theme,
        &spec::issuer_allowed_subjects(existing),
        ROTATE_PRODUCTION_REMEDY,
    )
}

/// Production gate for `issuer create`: a new issuer's subjects are the flags
/// alone (the template is empty).
pub fn check_create_on_production(
    theme: crate::config::TenantTheme,
    incoming: &[String],
) -> Result<()> {
    let planned = spec::subjects_after_add(&[], incoming)?;
    spec::ensure_production_write_restricted(theme, &planned, CREATE_PRODUCTION_REMEDY)
}

/// Set up the shared default issuer and this install's one per-tenant key.
pub async fn setup(
    tenant: &Tenant,
    realm: &str,
    operator: &ResolvedOperator,
    incoming_subjects: &[String],
    confirmed_prod: bool,
) -> Result<String> {
    if operator.source == NameSource::Placeholder {
        return Err(Error::Config(
            "operator name is required for jwt-bearer setup; run `aic settings set operator.name <name>`".into(),
        ));
    }

    // Production only: one extra issuer read, before the vault write. The
    // publish path reads again and re-checks the body it is about to send, so
    // this pre-check is fail-fast rather than the last word.
    if tenant.theme == crate::config::TenantTheme::Production {
        let existing = match api::read_issuer(&tenant.name, realm, DEFAULT_ISSUER_ID).await {
            Ok(value) => Some(value),
            Err(error) if is_not_found(&error) => None,
            Err(error) => return Err(error),
        };
        check_setup_on_production(tenant.theme, existing.as_ref(), incoming_subjects)?;
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
    let incoming_for_publish = incoming_subjects.to_vec();
    let theme = tenant.theme;
    publish_and_verify(
        &record.kid,
        || {
            let tenant = tenant_name.clone();
            let realm = realm_name.clone();
            let public_jwk = public_jwk_for_publish.clone();
            let incoming_subjects = incoming_for_publish.clone();
            async move {
                let remote = read_or_template(&tenant, &realm, DEFAULT_ISSUER_ID).await?;
                check_setup_on_production(theme, Some(&remote), &incoming_subjects)?;
                let body = if incoming_subjects.is_empty() {
                    body_with_key(remote, DEFAULT_ISSUER, &public_jwk)?
                } else {
                    body_with_key_and_subjects(
                        remote,
                        DEFAULT_ISSUER,
                        &public_jwk,
                        &incoming_subjects,
                    )?
                };
                api::upsert_issuer(&tenant, &realm, DEFAULT_ISSUER_ID, body, confirmed_prod)
                    .await?;
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

/// Remove a key from a previously read default issuer and return its new size.
/// The caller reads and displays the key before invoking this write so a
/// destructive command always shows the attribution it is about to revoke.
pub async fn remove_key_from_issuer(
    tenant: &str,
    realm: &str,
    kid: &str,
    issuer: Value,
    confirmed_prod: bool,
) -> Result<usize> {
    let jwk_set = spec::remove_from_jwk_set(spec::issuer_jwk_set(&issuer), kid)?;
    let remaining = spec::parse_jwk_set(Some(&jwk_set))?
        .get("keys")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let body = spec::issuer_body(issuer, DEFAULT_ISSUER, jwk_set)?;
    api::upsert_issuer(tenant, realm, DEFAULT_ISSUER_ID, body, confirmed_prod).await?;
    Ok(remaining)
}

/// Generate a replacement key and retire the currently stored key.
pub async fn rotate(
    tenant: &Tenant,
    realm: &str,
    operator: &ResolvedOperator,
    confirmed_prod: bool,
) -> Result<(String, String)> {
    let old_record = crate::jwtbearer::get_key(AgentClient::connect_or_spawn().await?, &tenant.name)
        .await?
        .ok_or_else(|| {
            Error::Config(format!(
                "no Trusted JWT private key stored for tenant {}; run `aic jwt-bearer setup --realm {realm}`",
                tenant.name
            ))
        })?;
    if operator.source == NameSource::Placeholder {
        return Err(Error::Config(
            "operator name is required for jwt-bearer rotate; run `aic settings set operator.name <name>`".into(),
        ));
    }
    let old_kid = old_record.kid.clone();
    let new_record = generate_key(operator)?;
    let new_kid = new_record.kid.clone();
    let public = public_jwk(&new_record)?;

    // These three steps are ordered so that every intermediate state leaves a
    // usable install, which is why rotation publishes before it stores — the
    // opposite of `setup` above. A failed publish changes nothing; a failed
    // store leaves the old private key still local and still published; a
    // failed removal leaves the new key working and an orphaned old public key
    // that `key remove` can clean up. Storing first would instead produce a
    // state where the only published key's private half has been discarded.
    // The production restriction check rides the existing publish read, so it
    // fires before the first irreversible step (the upsert).
    publish_public_jwk(&tenant.name, realm, &public, tenant.theme, confirmed_prod).await?;
    crate::jwtbearer::put_key(
        AgentClient::connect_or_spawn().await?,
        &tenant.name,
        &new_record,
    )
    .await?;
    remove_published_key(&tenant.name, realm, &old_kid, confirmed_prod).await?;

    Ok((old_kid, new_kid))
}

async fn publish_public_jwk(
    tenant: &str,
    realm: &str,
    public_jwk: &Value,
    theme: crate::config::TenantTheme,
    confirmed_prod: bool,
) -> Result<()> {
    let source = read_or_template(tenant, realm, DEFAULT_ISSUER_ID).await?;
    check_rotate_on_production(theme, &source)?;
    let body = body_with_key(source, DEFAULT_ISSUER, public_jwk)?;
    api::upsert_issuer(tenant, realm, DEFAULT_ISSUER_ID, body, confirmed_prod).await?;
    Ok(())
}

async fn remove_published_key(
    tenant: &str,
    realm: &str,
    kid: &str,
    confirmed_prod: bool,
) -> Result<()> {
    let issuer = api::read_issuer(tenant, realm, DEFAULT_ISSUER_ID).await?;
    remove_key_from_issuer(tenant, realm, kid, issuer, confirmed_prod).await?;
    Ok(())
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
    incoming_subjects: &[String],
    confirmed_prod: bool,
) -> Result<()> {
    // Planned subjects are the flags alone; a first create has no list to
    // add to. Fail before the existence read / file / template / upsert.
    check_create_on_production(tenant.theme, incoming_subjects)?;
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
    let subjects = spec::subjects_after_add(&[], incoming_subjects)?;
    let body = spec::issuer_body_with_subjects(template, issuer, jwk_set, subjects)?;
    api::upsert_issuer(&tenant.name, realm, id, body, confirmed_prod).await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubjectEdit {
    Add,
    Remove,
}

/// Replace `allowedSubjects` on an existing issuer without touching its
/// published key set. One read: the write body is built from the same
/// document the edit was computed against.
pub async fn edit_issuer_subjects(
    tenant: &Tenant,
    realm: &str,
    issuer_id: &str,
    incoming: &[String],
    edit: SubjectEdit,
    confirmed_prod: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let issuer = api::read_issuer(&tenant.name, realm, issuer_id).await?;
    let before = spec::issuer_allowed_subjects(&issuer);
    let after = match edit {
        SubjectEdit::Add => spec::subjects_after_add(&before, incoming)?,
        SubjectEdit::Remove => spec::subjects_after_remove(&before, incoming)?,
    };
    if after == before {
        return Ok((before, after));
    }
    spec::ensure_production_write_restricted(tenant.theme, &after, SUBJECTS_RM_PRODUCTION_REMEDY)?;
    let jwk_set = match spec::issuer_jwk_set(&issuer) {
        Some(value) => value.to_string(),
        None => serde_json::to_string(&spec::parse_jwk_set(None)?)?,
    };
    // Refuse rather than default. `--issuer` targets an arbitrary id, and
    // `issuer_body` stamps this value into the `issuer` claim the assertion
    // must match — so guessing `aic-agent` here would silently repoint someone
    // else's issuer config at our own assertions while reporting a subject
    // edit. There is no correct guess: the claim is the thing that identifies
    // whose tokens this config accepts.
    let claim = spec::issuer_name(&issuer)
        .ok_or_else(|| {
            Error::Config(format!(
                "Trusted JWT issuer {issuer_id:?} carries no `issuer` claim; refusing to guess one while editing its subjects"
            ))
        })?
        .to_string();
    let body = spec::issuer_body_with_subjects(issuer, &claim, jwk_set, after.clone())?;
    api::upsert_issuer(&tenant.name, realm, issuer_id, body, confirmed_prod).await?;
    Ok((before, after))
}

async fn read_or_template(tenant: &str, realm: &str, id: &str) -> Result<Value> {
    match api::read_issuer(tenant, realm, id).await {
        Ok(value) => Ok(value),
        Err(error) if is_not_found(&error) => api::issuer_template(tenant, realm).await,
        Err(error) => Err(error),
    }
}

fn body_with_key(source: Value, issuer: &str, public_jwk: &Value) -> Result<Value> {
    let existing = spec::issuer_jwk_set(&source);
    let jwk_set = spec::merge_jwk_set(existing, public_jwk.clone())?;
    spec::issuer_body(source, issuer, jwk_set)
}

fn body_with_key_and_subjects(
    source: Value,
    issuer: &str,
    public_jwk: &Value,
    extra_subjects: &[String],
) -> Result<Value> {
    let existing = spec::issuer_jwk_set(&source);
    let jwk_set = spec::merge_jwk_set(existing, public_jwk.clone())?;
    let subjects =
        spec::subjects_after_add(&spec::issuer_allowed_subjects(&source), extra_subjects)?;
    spec::issuer_body_with_subjects(source, issuer, jwk_set, subjects)
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

    use super::{
        add_attribution, check_create_on_production, check_rotate_on_production,
        check_setup_on_production, generate_kid, publish_and_verify,
    };
    use crate::config::TenantTheme;
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

    /// Drive the same functions `setup` / `rotate` / `create_issuer` call.
    /// `[""]` is the case a length-only check would get wrong.
    #[test]
    fn production_setup_rotate_and_create_refuse_an_unrestricted_result() {
        let empty = json!({"allowedSubjects": []});
        let blank = json!({"allowedSubjects": [""]});
        let restricted = json!({"allowedSubjects": ["user-uuid"]});

        assert!(check_setup_on_production(TenantTheme::Production, None, &[]).is_err());
        assert!(check_setup_on_production(TenantTheme::Production, Some(&empty), &[]).is_err());
        assert!(check_setup_on_production(TenantTheme::Production, Some(&blank), &[]).is_err());
        assert!(check_setup_on_production(TenantTheme::Production, Some(&restricted), &[]).is_ok());
        assert!(
            check_setup_on_production(TenantTheme::Production, Some(&blank), &["user-uuid".into()])
                .is_ok()
        );
        assert!(check_setup_on_production(TenantTheme::Sandbox, None, &[]).is_ok());

        assert!(check_rotate_on_production(TenantTheme::Production, &empty).is_err());
        assert!(check_rotate_on_production(TenantTheme::Production, &blank).is_err());
        assert!(check_rotate_on_production(TenantTheme::Production, &restricted).is_ok());
        assert!(check_rotate_on_production(TenantTheme::Sandbox, &blank).is_ok());
        let rotate_err = check_rotate_on_production(TenantTheme::Production, &blank).unwrap_err();
        assert!(
            rotate_err.to_string().contains("subjects add"),
            "{rotate_err}"
        );

        assert!(check_create_on_production(TenantTheme::Production, &[]).is_err());
        assert!(check_create_on_production(TenantTheme::Production, &["".into()]).is_err());
        assert!(check_create_on_production(TenantTheme::Production, &["user-uuid".into()]).is_ok());
        assert!(check_create_on_production(TenantTheme::Sandbox, &[]).is_ok());
        let create_err = check_create_on_production(TenantTheme::Production, &[]).unwrap_err();
        assert!(
            create_err.to_string().contains("--id/--username"),
            "{create_err}"
        );
    }
}
