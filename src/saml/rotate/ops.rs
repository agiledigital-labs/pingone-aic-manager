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
use crate::saml::rotate::pem::KeyPair;
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

/// Who else, anywhere on the tenant, resolves a key through the ESV secret a
/// rotation is about ([`spec::survey_consumers`]).
///
/// **Every realm in [`spec::SURVEYED_REALMS`]**, because the ESV secret is
/// tenant-global while the mapping table and the entity collection are per
/// realm: a consumer in the realm the operator did not name is cut over by
/// the same write. Per realm that is the mapping table, the entity list, and
/// **one read per entity provider** — the list is stubs only
/// (`docs/api/06-saml.md`) and carries no `secretIdIdentifier`, and there is
/// no query filter for one, so the only way to learn which providers name a
/// label family is to read them. So a survey costs `2 × realms + N` calls for
/// a tenant with N entity providers across those realms, and every write verb
/// pays it twice: once to plan, once in its pre-write recheck ([`recheck`],
/// [`init_recheck`]).
///
/// A 404 mid-survey is skipped rather than fatal: an entity deleted between
/// the list and its read resolves nothing and consumes nothing. Any other
/// error propagates — a consumer that could not be read is not a consumer that
/// is absent.
pub async fn read_consumers(
    tenant: &str,
    mine: &spec::Consumer,
    secret_id: Option<&str>,
) -> Result<spec::Consumers> {
    let mut realms = Vec::new();
    for realm in spec::SURVEYED_REALMS {
        realms.push(read_realm_survey(tenant, realm).await?);
    }
    Ok(spec::survey_consumers(mine, secret_id, &realms))
}

async fn read_realm_survey(tenant: &str, realm: &str) -> Result<spec::RealmSurvey> {
    let mappings = crate::secretmap::api::list_mappings(tenant, realm).await?;
    let mut entities = Vec::new();
    for stub in api::list(tenant, realm).await? {
        match api::read(tenant, realm, stub.location, &stub.entity_id).await {
            Ok(document) => entities.push(spec::entity_identifiers(
                &stub.entity_id,
                stub.location,
                &document,
            )),
            Err(error) if is_not_found(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(spec::RealmSurvey {
        realm: realm.to_string(),
        mappings,
        entities,
    })
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
    let published = poll_signing_certs(tenant, realm, entity_id, role, |seen| {
        spec::settled_on(seen, expected)
    })
    .await?;
    if spec::settled_on(&published, expected) {
        return Ok(Settlement::Settled);
    }
    let (missing, unexpected) = spec::settlement_gap(&published, expected);
    Ok(Settlement::TimedOut {
        missing,
        unexpected,
    })
}

/// Poll the export until this role publishes **anything**, and report what.
///
/// The adoption case. An `init` that reused an ESV secret it did not create has
/// never seen that secret's value — values are write-only — so it has no
/// fingerprint to wait for and must not invent one. What it can still check is
/// the thing that actually goes wrong: the entity has just been repointed at a
/// label, and a label backed by a secret with no ENABLED version resolves to
/// nothing at all.
///
/// Returns the set as last read, empty or not; [`spec::adoption_outcome`]
/// decides what that entitles the caller to claim.
pub async fn wait_for_any_cert(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
    role: Role,
) -> Result<BTreeSet<String>> {
    poll_signing_certs(tenant, realm, entity_id, role, |seen| !seen.is_empty()).await
}

/// Re-export until `settled` accepts this role's signing fingerprints, and
/// return the last set seen — which on a timeout is what the caller has to
/// report, not a failure of the write that preceded it.
async fn poll_signing_certs(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
    role: Role,
    settled: impl Fn(&BTreeSet<String>) -> bool,
) -> Result<BTreeSet<String>> {
    let deadline = std::time::Instant::now() + SETTLE_TIMEOUT;
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
        if settled(&seen) || std::time::Instant::now() >= deadline {
            return Ok(seen);
        }
        tokio::time::sleep(SETTLE_INTERVAL).await;
    }
}

/// What `init`'s steps need beyond the plan and the state.
pub struct InitInputs<'a> {
    /// This role as a consumer of the planned label — the survey's `mine`,
    /// and the realm and label `map_label` writes.
    pub mine: &'a spec::Consumer,
    /// The key pair, when the plan creates the secret.
    pub key: Option<&'a KeyPair>,
    pub description: &'a str,
    /// The alias at the label when the plan was made (none, if it maps it).
    pub planned_mapping: Option<&'a str>,
}

/// Perform `init`'s planned steps — the entity `PUT`, the secret, the mapping
/// — each holding an exclusivity proof no older than it needs to be.
///
/// The sibling of [`add_version`] and [`disable_version`]: the first act is
/// [`init_recheck`], after the prompt and the production gate, which
/// re-reads what the plan was decided from, re-surveys every realm and mints
/// the proof. `init` differs from its siblings in having up to **three**
/// writes, and the survey in front of the first is older than the third. So
/// the step that **activates** the chain ([`spec::InitPlan::activating_step`]
/// — the write from which the role resolves the secret) re-surveys again
/// immediately before it is sent, and holds the proof minted there
/// ([`spec::InitPlan::resurveys_before`]). A second entity that took the
/// identifier after step one is refused rather than cut over with this one.
/// That narrows the race to one round trip; it cannot make it atomic — no
/// REST read followed by a write can.
///
/// The loop itself is [`spec::run_init_steps`], handed the two tenant calls
/// as callbacks so a test can drive the ordering; this function is only the
/// up-front recheck and the wiring. Every writer a step calls takes the
/// [`spec::ExclusivityProof`] it is holding, as `stage`'s and `complete`'s
/// do — which guards against a writer being reached with the check left
/// out, and says nothing about the survey being atomic or still current.
///
/// A stop partway is an [`spec::InitApplyError`] naming the steps that
/// completed, `.ai/core.md` §5's batch rule, plus what is known about the
/// step that stopped — refused, accepted but unverified, or unknown
/// ([`spec::WriteStatus`]). `report` still receives one line per completed
/// step as it completes; this module prints nothing itself.
pub async fn apply_init(
    tenant: &str,
    state: &RotationState,
    plan: &spec::InitPlan,
    inputs: &InitInputs<'_>,
    confirmed_prod: bool,
    permit: &spec::InitPermit,
    report: &mut dyn FnMut(String),
) -> std::result::Result<(), spec::InitApplyError> {
    let upfront = init_recheck(tenant, state, plan, inputs)
        .await
        .map_err(|error| {
            spec::InitApplyError::new(
                state,
                plan,
                &[],
                spec::InitStop::Recheck,
                spec::WriteFailure::before_send(error),
            )
        })?;
    spec::run_init_steps(
        state,
        plan,
        &upfront,
        async |_step| activation_recheck(tenant, state, plan).await,
        async |step, proof| {
            run_step(
                tenant,
                state,
                plan,
                inputs,
                step,
                confirmed_prod,
                permit,
                proof,
            )
            .await
        },
        report,
    )
    .await
}

/// Send one of `init`'s steps and say what it did.
#[allow(clippy::too_many_arguments)]
async fn run_step(
    tenant: &str,
    state: &RotationState,
    plan: &spec::InitPlan,
    inputs: &InitInputs<'_>,
    step: spec::InitStep,
    confirmed_prod: bool,
    permit: &spec::InitPermit,
    proof: &spec::ExclusivityProof,
) -> std::result::Result<String, spec::WriteFailure> {
    match step {
        spec::InitStep::SetIdentifier => {
            set_identifier(state, &plan.identifier, confirmed_prod, permit, proof).await?;
            Ok(format!(
                "entity {} now points at secretIdIdentifier {:?} (read back and compared whole)",
                state.entity_id, plan.identifier
            ))
        }
        spec::InitStep::CreateSecret => {
            let key = inputs.key.expect("plan_init requires a key to create");
            create_key_secret(
                tenant,
                &plan.secret_id,
                &key.value,
                inputs.description,
                confirmed_prod,
                permit,
                proof,
            )
            .await?;
            Ok(format!(
                "ESV secret {} created — encoding pem, useInPlaceholders false, certificate {}",
                plan.secret_id, key.sha256
            ))
        }
        spec::InitStep::MapLabel => {
            map_label(
                tenant,
                inputs.mine,
                &plan.secret_id,
                inputs.planned_mapping,
                confirmed_prod,
                permit,
                proof,
            )
            .await?;
            Ok(format!(
                "label {} now maps to {}",
                plan.label, plan.secret_id
            ))
        }
    }
}

/// The re-survey in front of `init`'s activating step
/// ([`spec::init_exclusive`]).
async fn activation_recheck(
    tenant: &str,
    state: &RotationState,
    plan: &spec::InitPlan,
) -> Result<spec::ExclusivityProof> {
    let mut survey = Vec::new();
    for realm in spec::SURVEYED_REALMS {
        survey.push(read_realm_survey(tenant, realm).await?);
    }
    spec::init_exclusive(state, plan, &survey)
}

/// `init`'s pre-write recheck: [`recheck`]'s sibling, in the same order.
///
/// The cheap reads first — the entity's identifier and the label's mapping,
/// which must still be what the plan saw ([`spec::init_pre_write_ok`] applies
/// [`spec::identifier_write_ok`] and [`spec::mapping_write_ok`]) — and a
/// refusal there comes before the survey, as `recheck`'s target check does.
/// Then the tenant-wide survey, from which the proof is minted.
///
/// The per-step checks inside [`set_identifier`] and [`map_label`] stay: they
/// guard the seconds between steps, this guards the prompt.
async fn init_recheck(
    tenant: &str,
    state: &RotationState,
    plan: &spec::InitPlan,
    inputs: &InitInputs<'_>,
) -> Result<spec::ExclusivityProof> {
    let entity = api::read(tenant, &state.realm, state.location, &state.entity_id).await?;
    let mapping = mapping_alias(tenant, &state.realm, &plan.label).await?;
    spec::init_target_ok(
        state,
        plan,
        inputs.planned_mapping,
        &entity,
        mapping.as_deref(),
    )?;

    let mut survey = Vec::new();
    for realm in spec::SURVEYED_REALMS {
        survey.push(read_realm_survey(tenant, realm).await?);
    }
    spec::init_pre_write_ok(
        state,
        plan,
        inputs.planned_mapping,
        &spec::InitRecheck {
            entity,
            mapping,
            survey,
        },
    )
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
async fn set_identifier(
    state: &RotationState,
    identifier: &str,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
    _proof: &spec::ExclusivityProof,
) -> std::result::Result<Value, spec::WriteFailure> {
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
    let before = api::read(tenant, realm, location, entity_id)
        .await
        .map_err(spec::WriteFailure::before_send)?;
    spec::identifier_write_ok(state.identifier.as_deref(), &before, role)
        .map_err(spec::WriteFailure::before_send)?;
    let intended = spec::set_secret_identifier(&before, location, role, identifier)
        .map_err(spec::WriteFailure::before_send)?;
    api::update_entity(
        tenant,
        realm,
        location,
        entity_id,
        intended.clone(),
        confirmed_prod,
        _permit,
    )
    .await
    .map_err(|error| spec::WriteFailure::from_send(error, &[]))?;
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
        })
        .map_err(spec::WriteFailure::unverified)?;

    let differences = spec::entity_write_differences(&intended, &after);
    if differences.is_empty() {
        return Ok(after);
    }
    Err(spec::WriteFailure::unverified(Error::Config(format!(
        "the entity `PUT` returned success but {}/{realm}/{entity_id} does not match what was \
         sent — {} differ(s). An entity `PUT` is a full replace with no `If-Match`, so a body \
         AM reshaped leaves the entity in whatever state it chose; read it with \
         `aic saml show {entity_id} --realm {realm} --json` before doing anything else, and do \
         not create the ESV secret until it is right.",
        tenant,
        differences.join(", ")
    ))))
}

/// Create the `pem` ESV secret that will hold the key pairs.
///
/// The one writer on this path with **no** freshness recheck in front of it,
/// because the tenant already fails closed: a secret `PUT` is create-only and
/// answers `400 "Failed to create secret, the secret already exists"`
/// (`docs/api/03-esvs.md`), naming the id. A secret created in the gap is
/// therefore refused by AIC rather than overwritten, and a local pre-read
/// would buy a nicer sentence for one more round trip on every `init`.
///
/// `useInPlaceholders: false` is not a preference. A signing key is resolved
/// through the secret store rather than substituted into config, so it needs
/// none of the placeholder machinery — and a `false` secret reads
/// `loaded: true` the moment it is created, which is what makes the whole
/// rotation restart-free. With placeholders on it reads `loaded: false`,
/// `loadedVersion: ""`, and every later version waits for a tenant restart
/// (`docs/api/03-esvs.md`). Neither property can be changed afterwards.
async fn create_key_secret(
    tenant: &str,
    secret_id: &str,
    value: &str,
    description: &str,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
    _proof: &spec::ExclusivityProof,
) -> std::result::Result<Value, spec::WriteFailure> {
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
    .map_err(|error| spec::WriteFailure::from_send(error, &[spec::SECRET_ALREADY_EXISTS]))
}

/// Point the signing label at the ESV secret, having checked it is still free.
///
/// `planned_from` is the alias sitting at this label when [`spec::plan_init`]
/// decided to map it, which was none. The recheck is here rather than at the
/// call site for the reason [`set_identifier`]'s is: `set_mapping` is a `PUT`
/// that updates as happily as it creates, so the dangerous write and the
/// evidence that it is safe have to be in the same function.
async fn map_label(
    tenant: &str,
    mine: &spec::Consumer,
    secret_id: &str,
    planned_from: Option<&str>,
    confirmed_prod: bool,
    _permit: &spec::InitPermit,
    _proof: &spec::ExclusivityProof,
) -> std::result::Result<Value, spec::WriteFailure> {
    // The realm and label are the consumer's own, so the label written is
    // the one the exclusivity proof was minted about.
    let (realm, label) = (mine.realm.as_str(), mine.label.as_str());
    let fresh = mapping_alias(tenant, realm, label)
        .await
        .map_err(spec::WriteFailure::before_send)?;
    spec::mapping_write_ok(label, secret_id, planned_from, fresh.as_deref())
        .map_err(spec::WriteFailure::before_send)?;
    crate::secretmap::api::set_mapping(tenant, realm, label, secret_id, confirmed_prod)
        .await
        .map_err(|error| spec::WriteFailure::from_send(error, &[]))
}

/// Add the second key pair as a new ESV secret version.
///
/// The new version is auto-ENABLED and becomes `activeVersion`, and **every
/// ENABLED version is published at once** — which is the whole mechanism: two
/// ENABLED versions, two `<KeyDescriptor use="signing">`.
///
/// Rechecks the plan's inputs first ([`recheck`]); a second version added in
/// the gap makes this the third certificate rather than the second, which is
/// the `Inconsistent` phase every later verb refuses from.
///
/// The proof the recheck mints is passed to the writer, which requires one.
/// A failure says which side of the send it is on ([`spec::WriteFailure`]).
pub async fn add_version(
    tenant: &Tenant,
    state: &RotationState,
    plan: &spec::StagePlan,
    value: &str,
    confirmed_prod: bool,
    permit: &spec::StagePermit,
) -> std::result::Result<Value, spec::WriteFailure> {
    let fresh = recheck(tenant, state, "stage", &plan.secret_id)
        .await
        .map_err(spec::WriteFailure::before_send)?;
    send_version(tenant, plan, value, confirmed_prod, permit, &fresh).await
}

/// `stage`'s write: holds the permit and the proof the recheck minted.
async fn send_version(
    tenant: &Tenant,
    plan: &spec::StagePlan,
    value: &str,
    confirmed_prod: bool,
    _permit: &spec::StagePermit,
    _proof: &spec::ExclusivityProof,
) -> std::result::Result<Value, spec::WriteFailure> {
    crate::esv::api::create_secret_version(
        &tenant.name,
        &plan.secret_id,
        &encode_pem(value),
        confirmed_prod,
    )
    .await
    .map_err(|error| spec::WriteFailure::from_send(error, &[]))
}

/// Disable the old version, closing the window.
///
/// Disabling is **reversible** — `aic esv secret enable` puts the certificate
/// back — which is why this is where the rotation stops. Destroying the
/// version is not reversible and is not done here.
///
/// The recheck matters most here, and it is the longest gap on this path: the
/// plan is made, the operator is asked to confirm at a terminal — a human
/// interval by design — and only then is a version disabled **by number**.
/// Whatever moved in between, the number still resolves.
///
/// The proof the recheck mints is passed to the writer, which requires one.
/// A failure says which side of the send it is on ([`spec::WriteFailure`]).
pub async fn disable_version(
    tenant: &Tenant,
    state: &RotationState,
    plan: &spec::CompletePlan,
    confirmed_prod: bool,
    permit: &spec::CompletePermit,
) -> std::result::Result<Value, spec::WriteFailure> {
    let fresh = recheck(tenant, state, "complete", &plan.secret_id)
        .await
        .map_err(spec::WriteFailure::before_send)?;
    send_disable(tenant, plan, confirmed_prod, permit, &fresh).await
}

/// `complete`'s write: holds the permit and the proof the recheck minted.
async fn send_disable(
    tenant: &Tenant,
    plan: &spec::CompletePlan,
    confirmed_prod: bool,
    _permit: &spec::CompletePermit,
    _proof: &spec::ExclusivityProof,
) -> std::result::Result<Value, spec::WriteFailure> {
    crate::esv::api::change_version_status(
        &tenant.name,
        &plan.secret_id,
        &plan.disable_version,
        "DISABLED",
        confirmed_prod,
    )
    .await
    .map_err(|error| spec::WriteFailure::from_send(error, &[spec::CANNOT_DISABLE_LATEST]))
}

/// Read the rollover's inputs again, re-survey its consumers, and compare
/// them with the plan's — returning the only [`spec::ExclusivityProof`] the
/// write holds.
///
/// The reads are the entity, the mapping, the secret's versions and the
/// export, plus a full tenant-wide consumer survey ([`read_consumers`]); the
/// decision is [`spec::rollover_pre_write_ok`], pure so a test drives it. It
/// answers three questions: whether this role still resolves the secret about
/// to be mutated (which certificate equality cannot answer — published
/// metadata names no secret and no version), whether that secret still backs
/// this role **alone**, and whether the rollover itself is still the one that
/// was planned.
///
/// The survey is here, and not only at plan time, because this is the far
/// side of the confirmation prompt: a consumer added while the operator read
/// it was invisible to the plan's survey and is cut over by this write. The
/// plan-time proof was consumed by `authorize_*`, so returning a proof at all
/// means this function minted one — a recheck that stopped surveying would not
/// compile.
///
/// The secret's own metadata is still not re-read: `encoding` and
/// `useInPlaceholders` are immutable after create (`docs/api/03-esvs.md`), so
/// there is nothing about it that can move.
async fn recheck(
    tenant: &Tenant,
    state: &RotationState,
    verb: &str,
    secret_id: &str,
) -> Result<spec::ExclusivityProof> {
    let entity = api::read(&tenant.name, &state.realm, state.location, &state.entity_id).await?;
    let identifier = spec::current_identifier(&entity, state.role);
    let alias = match identifier.as_deref() {
        Some(identifier) => {
            mapping_alias(&tenant.name, &state.realm, &signing_label(identifier)).await?
        }
        None => None,
    };
    // Refused before the survey when the role no longer resolves the secret:
    // there is no consumer set to survey for a role that is not one, and the
    // survey is the expensive half of this recheck.
    spec::rollover_target_ok(verb, secret_id, identifier.as_deref(), alias.as_deref())?;

    let versions = spec::parse_versions(
        &crate::esv::api::list_secret_versions(&tenant.name, secret_id).await?,
    );
    let certs = export_certs(tenant, &state.realm, &state.entity_id).await?;
    let published = certs
        .iter()
        .filter(|cert| {
            cert.descriptor == role_descriptor(state.role)
                && cert.key_use.as_deref() == Some(spec::SIGNING_USE)
        })
        .map(|cert| cert.sha256.clone())
        .collect();
    let mut survey = Vec::new();
    for realm in spec::SURVEYED_REALMS {
        survey.push(read_realm_survey(&tenant.name, realm).await?);
    }
    spec::rollover_pre_write_ok(
        verb,
        secret_id,
        state,
        &spec::RolloverRecheck {
            identifier,
            alias,
            versions,
            published,
            survey,
        },
    )
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
