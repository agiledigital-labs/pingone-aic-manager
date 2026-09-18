//! The only module here that talks to a tenant.
//!
//! Three stores, four reads, and a poll. Every decision it acts on comes from
//! [`super::spec`]; what this module owns is the *order* the calls happen in,
//! and the rule that a claim is only made after the tenant confirmed it.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;

use crate::config::tenant::Tenant;
use crate::saml::metadata;
use crate::saml::rotate::journal::{self, Key};
use crate::saml::rotate::spec::{
    self, Phase, RotationState, SecretVersion, role_descriptor, signing_label,
};
use crate::saml::spec::{ExportOutcome, Location, Role};
use crate::saml::{api, spec as saml_spec};
use crate::{Error, Result};

/// How long to wait for the export to show a change, and how often to look.
///
/// Every change measured on the sandbox appeared within **single-digit
/// seconds** (`docs/api/06-saml.md`), so this is generous rather than
/// hopeful. A timeout is reported, never treated as a failure of the write:
/// the version exists either way, and telling an operator the stage failed
/// when it landed is how a rollover gets staged twice.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(45);
const SETTLE_INTERVAL: Duration = Duration::from_secs(3);

/// Read the whole picture: entity, mapping, secret, versions, export.
///
/// The export is fetched **unauthenticated**, like `aic saml metadata export`,
/// because the JSP takes no credential at all.
pub async fn read_state(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
    location: Option<Location>,
    requested_role: Option<Role>,
) -> Result<RotationState> {
    let location = match location {
        Some(location) => location,
        None => resolve_location(&tenant.name, realm, entity_id).await?,
    };
    let entity = api::read(&tenant.name, realm, location, entity_id).await?;
    let role = spec::choose_role(&entity, requested_role)?;
    let identifier = spec::current_identifier(&entity, role);

    let mut mapped_alias = None;
    let mut secret = None;
    let mut versions = Vec::new();
    if let Some(identifier) = identifier.as_deref() {
        mapped_alias = mapping_alias(&tenant.name, realm, &signing_label(identifier)).await?;
        if let Some(alias) = mapped_alias.as_deref() {
            secret = secret_facts(&tenant.name, alias).await?;
            if secret.is_some() {
                versions = spec::parse_versions(
                    &crate::esv::api::list_secret_versions(&tenant.name, alias).await?,
                );
            }
        }
    }

    let certs = export_certs(tenant, realm, entity_id).await?;
    let record = journal::find(&Key {
        tenant: tenant.name.clone(),
        realm: realm.to_string(),
        entity_id: entity_id.to_string(),
        role: role.wire().to_string(),
    })?;

    Ok(RotationState {
        tenant: tenant.name.clone(),
        realm: realm.to_string(),
        entity_id: entity_id.to_string(),
        location,
        role,
        identifier,
        mapped_alias,
        secret,
        versions,
        certs,
        record,
    })
}

fn is_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

/// Which ESV secret one label maps to, if the mapping exists at all.
///
/// An absent mapping is `None`, not an error: "nothing is mapped here" is a
/// legal state both `status` and `init` have to reason about, and the state
/// `init` needs most is the **orphan** — a mapping that outlived the entity
/// that minted its label (`docs/api/15-secret-mappings.md`). Collapsing that
/// into an error would turn the one case worth refusing into a stack trace.
pub async fn mapping_alias(tenant: &str, realm: &str, label: &str) -> Result<Option<String>> {
    match crate::secretmap::api::read_mapping(tenant, realm, label).await {
        Ok(document) => Ok(crate::secretmap::api::parse_mapping(&document).alias),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What the tenant says about one ESV secret, or `None` if it does not exist.
///
/// The `None` is load-bearing twice over: a mapped-but-absent secret is the
/// `Dangling` phase, and an absent secret is what makes `init`'s create step
/// necessary rather than a collision.
pub async fn secret_facts(tenant: &str, secret_id: &str) -> Result<Option<spec::SecretFacts>> {
    match crate::esv::api::get_secret(tenant, secret_id).await {
        Ok(document) => Ok(Some(spec::parse_secret_facts(secret_id, &document))),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Fetch and fingerprint the entity's published certificates.
///
/// **The export endpoint answers 200 for failure too**, so the body is
/// classified before it is parsed — otherwise `ERROR : No metadata for
/// entity …` would fingerprint to nothing and read as "this entity publishes
/// no certificates", which is the same shape as a successful roleless export.
async fn export_certs(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
) -> Result<Vec<metadata::CertRef>> {
    let body = api::export_metadata(tenant, realm, entity_id).await?;
    match saml_spec::classify_export(body.as_bytes()) {
        ExportOutcome::Metadata => Ok(metadata::cert_refs(body.as_bytes())?),
        ExportOutcome::TenantError(message) => Err(Error::Config(format!(
            "tenant {} refused to export {entity_id:?} from realm {realm}, so what it publishes \
             is unknown: {message}",
            tenant.name
        ))),
        ExportOutcome::Unrecognised(excerpt) => Err(Error::Config(format!(
            "tenant {} answered the metadata export for {entity_id:?} in realm {realm} with \
             something that is not SAML metadata: {excerpt}",
            tenant.name
        ))),
    }
}

async fn resolve_location(tenant: &str, realm: &str, entity_id: &str) -> Result<Location> {
    let stubs = api::list(tenant, realm).await?;
    match saml_spec::locate(entity_id, &stubs) {
        saml_spec::Located::In(location) => Ok(location),
        saml_spec::Located::NotFound => Err(Error::Config(format!(
            "no SAML entity provider {entity_id:?} in realm {realm}; \
             `aic saml list --realm {realm}` shows what is there"
        ))),
        saml_spec::Located::Ambiguous(locations) => Err(Error::Config(format!(
            "SAML entity provider {entity_id:?} exists in realm {realm} as {}; \
             pass --location to choose",
            locations
                .iter()
                .map(|location| location.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        ))),
    }
}

/// What a poll of the export concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    /// The export publishes exactly the expected set.
    Settled,
    /// It did not, within the timeout. Carries what is missing and what is
    /// still there, because "it did not settle" is not actionable and
    /// "certificate X never appeared" is.
    TimedOut {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
}

/// Poll the export until it publishes exactly `expected`.
///
/// The comparison is a **set**, not a count: a completion that disabled the
/// new version instead of the old one leaves exactly one certificate
/// published, and a count check would call that a success.
pub async fn wait_for_export(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
    role: Role,
    expected: &BTreeSet<String>,
) -> Result<Settlement> {
    let deadline = std::time::Instant::now() + SETTLE_TIMEOUT;
    let published;
    loop {
        let certs = export_certs(tenant, realm, entity_id).await?;
        let seen: BTreeSet<String> = certs
            .iter()
            .filter(|cert| {
                cert.descriptor == role_descriptor(role)
                    && cert.key_use.as_deref() == Some(spec::SIGNING_USE)
            })
            .map(|cert| cert.sha256.clone())
            .collect();
        if spec::settled_on(&seen, expected) {
            return Ok(Settlement::Settled);
        }
        if std::time::Instant::now() >= deadline {
            published = seen;
            break;
        }
        tokio::time::sleep(SETTLE_INTERVAL).await;
    }
    let (missing, unexpected) = spec::settlement_gap(&published, expected);
    Ok(Settlement::TimedOut {
        missing,
        unexpected,
    })
}

/// `PUT` the entity with a `secretIdIdentifier`, and prove it survived.
///
/// Five steps, and the shape is `.ai/core.md` §5 applied to a family with no
/// `If-Match` and a `PUT` that replaces the whole document:
///
/// 1. read the entity **fresh** — the snapshot is taken here, not from a read
///    the caller did minutes ago;
/// 2. check the fresh read against the state the plan was authorized from, and
///    refuse if the decision's own input moved ([`spec::identifier_write_ok`]);
/// 3. change exactly one leaf of it;
/// 4. `PUT` the result;
/// 5. read it back and compare the **whole document**, and report the paths
///    that differ rather than a verdict.
///
/// Steps 2 and 5 are the ones that matter, and they guard opposite ends. Step 5
/// is about AM: `{"entityId": "<same>"}` answers 200 and leaves a roleless
/// shell, so a write that succeeded by status code proves nothing about what is
/// now on the tenant. Step 2 is about the other operator: the fresh read in
/// step 1 carries a concurrent change through into the body verbatim, which is
/// right for every leaf except the one this write is about — that one gets
/// replaced, and without step 2 nothing says so.
pub async fn set_identifier(
    state: &RotationState,
    identifier: &str,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
) -> Result<Value> {
    // Taken from the state rather than passed alongside it: tenant, realm,
    // entity id, location and role are five positional arguments of which
    // three are `&str`, and a transposed pair would `PUT` a full replace at
    // the wrong document.
    let (tenant, realm, entity_id) = (
        state.tenant.as_str(),
        state.realm.as_str(),
        state.entity_id.as_str(),
    );
    let (location, role) = (state.location, state.role);
    let before = api::read(tenant, realm, location, entity_id).await?;
    spec::identifier_write_ok(state.identifier.as_deref(), &before, role)?;
    let intended = spec::set_secret_identifier(&before, location, role, identifier)?;
    api::update_entity(
        tenant,
        realm,
        location,
        entity_id,
        intended.clone(),
        confirmed_prod,
        _permit,
    )
    .await?;
    // The write is already gone. An error from here on is an error about the
    // *proof*, and it has to say which: an operator who reads "failed" and
    // re-runs to be sure is acting on the belief that nothing changed, and on
    // a full-replace `PUT` that belief is the expensive one.
    let after = api::read(tenant, realm, location, entity_id)
        .await
        .map_err(|error| {
            Error::Config(format!(
                "the entity `PUT` was accepted — {tenant}/{realm}/{entity_id} has been written \
                 with secretIdIdentifier {identifier:?} — but reading it back to prove that \
                 failed: {error}. **The write landed; this is not a no-op.** Nothing here has \
                 seen what is on the tenant now, so read it with `aic saml show {entity_id} \
                 --realm {realm} --json` before doing anything else, and do not create the ESV \
                 secret until the identifier is right. Re-running `aic saml rotate init` is \
                 safe — it skips a step the tenant already shows done."
            ))
        })?;

    let differences = spec::entity_write_differences(&intended, &after);
    if differences.is_empty() {
        return Ok(after);
    }
    Err(Error::Config(format!(
        "the entity `PUT` returned success but {}/{realm}/{entity_id} does not match what was \
         sent — {} differ(s). An entity `PUT` is a full replace with no `If-Match`, so a body \
         AM reshaped leaves the entity in whatever state it chose; read it with \
         `aic saml show {entity_id} --realm {realm} --json` before doing anything else, and do \
         not create the ESV secret until it is right.",
        tenant,
        differences.join(", ")
    )))
}

/// Create the `pem` ESV secret that will hold the key pairs.
///
/// `useInPlaceholders: false` is not a preference. A signing key is resolved
/// through the secret store rather than substituted into config, so it needs
/// none of the placeholder machinery — and a `false` secret reads
/// `loaded: true` the moment it is created, which is what makes the whole
/// rotation restart-free. With placeholders on it reads `loaded: false`,
/// `loadedVersion: ""`, and every later version waits for a tenant restart
/// (`docs/api/03-esvs.md`). Neither property can be changed afterwards.
pub async fn create_key_secret(
    tenant: &str,
    secret_id: &str,
    value: &str,
    description: &str,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
) -> Result<Value> {
    let value_base64 = encode_pem(value);
    crate::esv::api::create_secret(
        tenant,
        secret_id,
        "pem",
        false,
        &value_base64,
        description,
        confirmed_prod,
    )
    .await
}

/// Point the signing label at the ESV secret.
pub async fn map_label(
    tenant: &str,
    realm: &str,
    label: &str,
    secret_id: &str,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
) -> Result<Value> {
    crate::secretmap::api::set_mapping(tenant, realm, label, secret_id, confirmed_prod).await
}

/// Add the second key pair as a new ESV secret version.
///
/// The new version is auto-ENABLED and becomes `activeVersion`, and **every
/// ENABLED version is published at once** — which is the whole mechanism: two
/// ENABLED versions, two `<KeyDescriptor use="signing">`.
pub async fn add_version(
    tenant: &str,
    secret_id: &str,
    value: &str,
    confirmed_prod: bool,
    _permit: &spec::StagePermit,
) -> Result<Value> {
    let value_base64 = encode_pem(value);
    crate::esv::api::create_secret_version(tenant, secret_id, &value_base64, confirmed_prod).await
}

/// Disable the old version, closing the window.
///
/// Disabling is **reversible** — `aic esv secret enable` puts the certificate
/// back — which is why this is where the rotation stops. Destroying the
/// version is not reversible and is not done here.
pub async fn disable_version(
    tenant: &str,
    secret_id: &str,
    version: &str,
    confirmed_prod: bool,
    _permit: &spec::CompletePermit,
) -> Result<Value> {
    crate::esv::api::change_version_status(tenant, secret_id, version, "DISABLED", confirmed_prod)
        .await
}

/// The wire encoding of a `pem` secret value: base64 of the PEM text itself.
///
/// [`crate::esv::api::encode_secret_value`] would do this, and deliberately is
/// not used: its `pem` arm accepts anything containing `-----BEGIN`, and the
/// value reaching here has already been through
/// [`super::pem::validate_key_pair`], which is a far stronger check that a
/// weaker one cannot add to.
fn encode_pem(value: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

/// The three rows `status` needs about the ESV secret's versions, ordered
/// newest first the way the API returns them.
pub fn version_rows(versions: &[SecretVersion]) -> Vec<Vec<String>> {
    let mut sorted = versions.to_vec();
    sorted.sort_by_key(|version| std::cmp::Reverse(version.number()));
    sorted
        .into_iter()
        .map(|version| {
            vec![
                version.version.clone(),
                version.status.clone(),
                version.create_date.clone(),
            ]
        })
        .collect()
}

/// Whether a phase is one an operator can act on now.
pub fn actionable(phase: &Phase) -> bool {
    matches!(phase, Phase::Settled | Phase::Staged)
}
