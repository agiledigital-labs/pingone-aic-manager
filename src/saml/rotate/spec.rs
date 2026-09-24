//! The tenant-free half of `aic saml rotate`: what the four reads mean
//! together, and what each verb is allowed to do about it.
//!
//! Nothing here touches a tenant, a file or a clock. Everything a rotation
//! decides — which role, which label, which ESV secret version to disable,
//! whether the export has settled — is a function of documents that were read
//! elsewhere, so the decisions can be tested against the states that matter
//! rather than against the one state a sandbox happened to be in.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::saml::metadata::CertRef;
use crate::saml::rotate::journal::StagedRecord;
use crate::saml::rotate::pem::KeyPair;
use crate::saml::spec::{Location, Role};
use crate::secretmap::api::Mapping;
use crate::{Error, Result};

/// The dotted label family a `secretIdIdentifier` mints
/// (`docs/api/15-secret-mappings.md`).
const LABEL_PREFIX: &str = "am.applications.federation.entity.providers.saml2.";

/// The `use` attribute a signing key descriptor carries.
///
/// This slice rotates **signing** and nothing else. `encryption` and `mtls`
/// get their own labels off the same identifier and the procedure looks
/// identical, but nobody has measured that an encryption rollover publishes
/// two `KeyDescriptor`s the way a signing one does, and `mtls` may be
/// suppressed from the metadata entirely by
/// `clientAuthentication.excludeClientCertificate` — which would leave the
/// verification step with nothing to read (`docs/api/06-saml.md`).
pub const SIGNING_USE: &str = "signing";

/// The metadata descriptor element a role publishes its keys under.
///
/// The entity JSON says `serviceProvider`; the exported XML says
/// `SPSSODescriptor`. Both names are needed at once here — one addresses the
/// role block being written, the other the certificates being verified — and
/// conflating them is how a dual-role entity's two signing keys become one.
pub fn role_descriptor(role: Role) -> &'static str {
    match role {
        Role::Idp => "IDPSSODescriptor",
        Role::Sp => "SPSSODescriptor",
    }
}

/// Where the role block keeps its secret-label pointer.
///
/// Hosted and remote entities keep the same string in different groups
/// (`docs/api/06-saml.md`). A writer has to pick; only a reader can take
/// either.
pub fn identifier_path(location: Location) -> &'static [&'static str] {
    match location {
        Location::Hosted => &[
            "assertionContent",
            "signingAndEncryption",
            "secretIdAndAlgorithms",
            "secretIdIdentifier",
        ],
        Location::Remote => &["assertionContent", "secrets", "secretIdIdentifier"],
    }
}

/// The signing label a `secretIdIdentifier` resolves to.
pub fn signing_label(identifier: &str) -> String {
    format!("{LABEL_PREFIX}{identifier}.{SIGNING_USE}")
}

/// Refuse an identifier that would address a label other than the one it
/// looks like it addresses, or that AM will refuse anyway.
///
/// The identifier is spliced into a dotted label **and** into a URL path, so a
/// dot in it silently renames the label family and a slash escapes the
/// collection. Neither fails loudly: a mapping `PUT` at the wrong label is a
/// perfectly good 201 that nothing will ever read.
///
/// AM is narrower still, and says so only at the entity `PUT`: **ASCII
/// alphanumerics only**. `-` and `_` come back as
/// `400 Invalid character present in Secret ID Identifier` — measured
/// 2026-09-18 against a live tenant, where `sp-rotate-test` and
/// `sp_rotate_test` were both refused while `sprotatetest` and `SpRotate1`
/// were accepted. Rejecting them here matters because the `PUT` is `init`\'s
/// first step: a caller who gets past this check spends a round trip to be
/// told the same thing in AM\'s words, with no mention of which flag is at
/// fault.
pub fn validate_identifier(identifier: &str) -> Result<&str> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return Err(Error::Config(
            "--identifier is required: it is the namespace the three secret labels are minted \
             under, and an empty one leaves the role on the realm defaults"
                .into(),
        ));
    }
    if trimmed != identifier {
        return Err(Error::Config(format!(
            "--identifier {identifier:?} has surrounding whitespace; the label is built by \
             concatenation and the space would be part of it"
        )));
    }
    if let Some(bad) = trimmed
        .chars()
        .find(|character| !character.is_ascii_alphanumeric())
    {
        return Err(Error::Config(format!(
            "--identifier {identifier:?} contains {bad:?}: AM accepts only letters and digits \
             here and answers the entity PUT with `400 Invalid character present in Secret ID \
             Identifier` for anything else, naming no flag. The identifier is also spliced \
             into the dotted label `{}` and into a URL path, so a `.` or `/` would address a \
             different label than it reads as",
            signing_label("<identifier>")
        )));
    }
    Ok(trimmed)
}

/// Refuse an ESV secret id AIC's own path validation would refuse.
///
/// `^esv-[a-z0-9_-]{1,124}$`, verified live — a `PUT` outside it is a 400
/// naming the regex (`docs/api/03-esvs.md`). Checking it here means the
/// refusal names the rule rather than relaying the regex.
pub fn validate_secret_id(secret_id: &str) -> Result<&str> {
    let body = secret_id.strip_prefix("esv-").filter(|body| {
        (1..=124).contains(&body.len())
            && body.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '_' | '-')
            })
    });
    body.map(|_| secret_id).ok_or_else(|| {
        Error::Config(format!(
            "ESV secret id {secret_id:?} is not one AIC will accept: the path parameter is \
             validated against `^esv-[a-z0-9_-]{{1,124}}$`, so the `esv-` prefix is required \
             and the rest must be lowercase"
        ))
    })
}

/// Which role this rotation is about.
///
/// **An entity may hold both roles**, and each has its own
/// `secretIdIdentifier`, its own labels and its own `KeyDescriptor`s. Guessing
/// on a dual-role entity would rotate one role's key while reporting the
/// other's certificates, so the choice is refused rather than defaulted.
pub fn choose_role(entity: &Value, requested: Option<Role>) -> Result<Role> {
    let held: Vec<Role> = [Role::Idp, Role::Sp]
        .into_iter()
        .filter(|role| {
            entity
                .get(role.wire())
                .is_some_and(|block| block.is_object())
        })
        .collect();

    match (held.as_slice(), requested) {
        ([], _) => Err(Error::Config(
            "this entity has no role blocks, so it publishes no keys and there is nothing to \
             rotate; `aic saml show` prints its (legal, inert) state"
                .into(),
        )),
        ([only], None) => Ok(*only),
        ([only], Some(asked)) if *only == asked => Ok(*only),
        ([only], Some(asked)) => Err(Error::Config(format!(
            "this entity holds no {} block; its only role is {}",
            asked.wire(),
            only.wire()
        ))),
        (_, Some(asked)) => Ok(asked),
        (_, None) => Err(Error::Config(
            "this entity holds both the identityProvider and serviceProvider roles, and each \
             signs with its own secret label and publishes its own KeyDescriptors — pass \
             --role idp or --role sp to say which one to rotate"
                .into(),
        )),
    }
}

/// The `secretIdIdentifier` a role block currently points at, if any.
///
/// Read from either home, because a reader has no reason to care which
/// collection the entity lives in and a mistaken location would otherwise
/// report "unconfigured" for a configured entity.
pub fn current_identifier(entity: &Value, role: Role) -> Option<String> {
    let block = entity.get(role.wire())?;
    [Location::Hosted, Location::Remote]
        .into_iter()
        .find_map(|location| leaf(block, identifier_path(location)))
}

fn leaf(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(key)?;
    }
    let text = cursor.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Set exactly one leaf of a full entity document, leaving every other byte
/// of it alone.
///
/// Entity `PUT` is a **full replace with no `If-Match`**: a body of
/// `{"entityId": "<same>"}` answers 200 and deletes the whole role block
/// (`docs/api/06-saml.md`). So the write body is the document that was just
/// read, with one leaf changed — never a document this code assembled — and
/// the only structure created is the groups on the chosen path, which a live
/// entity carries as `{}` rather than as values.
pub fn set_secret_identifier(
    entity: &Value,
    location: Location,
    role: Role,
    identifier: &str,
) -> Result<Value> {
    let mut document = entity.clone();
    let object = document
        .as_object_mut()
        .ok_or_else(|| Error::Config("the entity read is not a JSON object".into()))?;
    let block = object
        .get_mut(role.wire())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            Error::Config(format!(
                "this entity has no {} block to point at a secret label",
                role.wire()
            ))
        })?;

    let path = identifier_path(location);
    let (leaf, groups) = path
        .split_last()
        .expect("the path has at least one segment");
    let mut cursor: &mut Map<String, Value> = block;
    for group in groups {
        let next = cursor
            .entry((*group).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let found = kind_of(next);
        cursor = next.as_object_mut().ok_or_else(|| {
            Error::Config(format!(
                "the entity's {}.{group} is {found} rather than a group; refusing to rewrite it",
                role.wire()
            ))
        })?;
    }
    cursor.insert((*leaf).to_string(), Value::from(identifier));
    Ok(document)
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Whether the entity is still the one the plan was authorized against.
///
/// `.ai/core.md` §5 with the sides swapped. The rule there is that a local
/// file is re-read immediately before it is overwritten; here it is the
/// *remote* document that moves, and the re-read that makes the write safe is
/// also what hides the move. [`set_secret_identifier`] builds its full-replace
/// body from a fresh read, so every other byte of a concurrent change survives
/// — and the one leaf the plan was about is silently replaced. An operator who
/// set the identifier in that gap gets no conflict, no diff and no message.
///
/// `planned_from` is the identifier this role had when [`plan_init`] decided
/// to set one. Identical is the **only** case that may write, and that
/// includes "someone else already set it to exactly what we wanted": the plan
/// said this was an unconfigured role, it is not one now, and re-running is
/// what should decide from the state that exists. `plan_init` then skips the
/// step and nothing is sent.
///
/// Role-scoped, because the other role's identifier is not this one's: a
/// dual-role entity has two, set independently.
pub fn identifier_write_ok(planned_from: Option<&str>, fresh: &Value, role: Role) -> Result<()> {
    let now = current_identifier(fresh, role);
    if now.as_deref() == planned_from {
        return Ok(());
    }
    Err(Error::Config(format!(
        "the {} block's secretIdIdentifier changed while this run was planning — it was {} when \
         the plan was made and it is {} now, so something else is writing this entity. The \
         entity `PUT` is a full replace with no `If-Match`, so going ahead would overwrite that \
         change with nothing reported. **Nothing has been sent.** Find out what the other change \
         was, then re-run: `aic saml rotate status` reads the state that exists now.",
        role.wire(),
        quoted(planned_from),
        quoted(now.as_deref()),
    )))
}

fn quoted(identifier: Option<&str>) -> String {
    identifier.map_or_else(|| "unset".to_string(), |value| format!("{value:?}"))
}

/// The two tenant documents a `stage` or a `complete` decides from.
///
/// Not the whole picture on purpose — see [`rollover_write_ok`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloverInputs {
    /// Which versions of the ESV secret are ENABLED, in numeric order.
    pub enabled: Vec<String>,
    /// Which signing certificates this role publishes.
    pub published: BTreeSet<String>,
}

pub fn rollover_inputs(state: &RotationState) -> RolloverInputs {
    rollover_inputs_from(&state.versions, state.published_fingerprints())
}

pub fn rollover_inputs_from(
    versions: &[SecretVersion],
    published: BTreeSet<String>,
) -> RolloverInputs {
    RolloverInputs {
        enabled: enabled_in(versions)
            .into_iter()
            .map(|version| version.version.clone())
            .collect(),
        published,
    }
}

/// Whether the rollover is still the rollover the plan was made from.
///
/// [`identifier_write_ok`]'s rule, owed to the other two writes. Round one
/// gave the entity `PUT` a freshness guard and left its siblings, and the gap
/// they carry is longer, not shorter: `complete` plans, then **waits through
/// an interactive confirmation**, then disables a version by number. A version
/// added, enabled or disabled in that interval changes which certificate that
/// number holds, and nothing downstream says so — the number survives the
/// change and the pairing does not, so the tenant disables whatever version N
/// now is and the post-write export check reports it once the certificate has
/// stopped being published.
///
/// Compared as a **set** and a list of identities, never as counts: an ESV
/// secret version disabled and another enabled in the gap leaves both totals
/// where they were, and that is the case where the planned version now holds
/// the certificate being kept.
///
/// Deliberately **not** a whole-document comparison, and that reasoning was
/// reviewed and upheld. The entity, its mapping and the secret's metadata can
/// all move without touching what this decision rests on, and refusing a safe
/// write is its own failure here: the remedy is to re-run, and a rollover that
/// will not complete is one that leaves two certificates published.
pub fn rollover_write_ok(
    verb: &str,
    secret_id: &str,
    planned_from: &RolloverInputs,
    fresh: &RolloverInputs,
) -> Result<()> {
    if planned_from == fresh {
        return Ok(());
    }
    let mut moved = Vec::new();
    if planned_from.enabled != fresh.enabled {
        moved.push(format!(
            "the ENABLED versions of {secret_id} were {} when the plan was made and are {} now",
            named(planned_from.enabled.iter()),
            named(fresh.enabled.iter())
        ));
    }
    if planned_from.published != fresh.published {
        moved.push(format!(
            "this role published {} and publishes {} now",
            named(planned_from.published.iter()),
            named(fresh.published.iter())
        ));
    }
    Err(Error::Config(format!(
        "the rollover changed while this run was planning, so the {verb} was decided from a \
         state that no longer exists: {}. Something else is writing this ESV secret or this \
         entity. A version number outlives the change and the certificate it holds does not, \
         so going ahead would act on the number and not on the certificate the plan named. \
         **Nothing has been sent.** `aic saml rotate status` reads the state that exists now.",
        moved.join("; ")
    )))
}

/// Whether this role still resolves the ESV secret the plan is about.
///
/// The half [`rollover_write_ok`] cannot cover. It compares the secret's
/// ENABLED versions and the role's published certificates, and **neither
/// carries any attribution**: metadata names no secret and no version, so two
/// identical certificate sets say nothing about which secret is publishing
/// them. If the role's `secretIdIdentifier` or that identifier's mapping moved
/// in the gap, the write is about to add a version to — or disable a version
/// of — a secret that no longer backs this role, and every other check around
/// it still passes.
///
/// Deliberately **not** a comparison against the planned identifier or the
/// planned mapping. What has to hold is that the chain still ends at the
/// planned secret: an identifier repointed at a label that maps to the same
/// secret rotates exactly the same key, and refusing that would refuse a safe
/// write, whose cost here is a rollover left with two certificates published.
pub fn rollover_target_ok(
    verb: &str,
    secret_id: &str,
    identifier: Option<&str>,
    alias: Option<&str>,
) -> Result<()> {
    let broken = match (identifier, alias) {
        (Some(_), Some(alias)) if alias == secret_id => return Ok(()),
        (None, _) => "the role now names no secretIdIdentifier at all and signs with the \
                      realm-wide default"
            .to_string(),
        (Some(identifier), None) => format!(
            "the role names secretIdIdentifier {identifier:?}, whose label {} maps to no ESV \
             secret",
            signing_label(identifier)
        ),
        (Some(identifier), Some(alias)) => format!(
            "the role names secretIdIdentifier {identifier:?}, whose label {} now maps to ESV \
             secret {alias}",
            signing_label(identifier)
        ),
    };
    Err(Error::Config(format!(
        "this run planned a {verb} of ESV secret {secret_id}, and {broken}. The {verb} would \
         therefore act on a secret this role no longer signs with, and nothing published says \
         which secret a certificate came from — so neither the ENABLED versions nor the \
         published certificates checked alongside this would have noticed. **Nothing has been \
         sent.** `aic saml rotate status` reads the state that exists now."
    )))
}

/// Everything `ops`' pre-write recheck reads, immediately before a `stage` or
/// a `complete` writes — after the confirmation prompt and the production
/// gate, which is the human interval every earlier read is older than.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloverRecheck {
    /// The role's `secretIdIdentifier`, read fresh.
    pub identifier: Option<String>,
    /// The ESV secret that identifier's signing label maps to now.
    pub alias: Option<String>,
    /// The planned secret's versions.
    pub versions: Vec<SecretVersion>,
    /// This role's published signing fingerprints.
    pub published: BTreeSet<String>,
    /// A fresh consumer survey of every realm in [`SURVEYED_REALMS`].
    pub survey: Vec<RealmSurvey>,
}

/// The whole pre-write decision for `stage` and `complete`, and the only
/// source of the [`ExclusivityProof`] their write holds.
///
/// Three questions, in the order a failure is cheapest to explain:
///
/// 1. does the role still resolve the planned secret ([`rollover_target_ok`]);
/// 2. does that secret still back this role and nothing else — **re-surveyed
///    here**, not carried over from the plan. The plan-time survey ran before
///    the prompt, and a consumer added while the operator read it (or one in a
///    realm the old survey never read) was cut over and never verified. The
///    survey's own `mine` is built from the fresh identifier, because (1)
///    permits an identifier that moved to a label backed by the same secret;
/// 3. is the rollover itself unchanged ([`rollover_write_ok`]).
///
/// Pure, so a test drives the real decision: the discriminating case is a
/// plan whose survey was exclusive and a recheck whose survey is not, which a
/// recheck that never re-surveyed passes.
pub fn rollover_pre_write_ok(
    verb: &str,
    secret_id: &str,
    state: &RotationState,
    fresh: &RolloverRecheck,
) -> Result<ExclusivityProof> {
    rollover_target_ok(
        verb,
        secret_id,
        fresh.identifier.as_deref(),
        fresh.alias.as_deref(),
    )?;
    let mine = Consumer {
        realm: state.realm.clone(),
        entity_id: state.entity_id.clone(),
        location: state.location,
        role: state.role,
        label: signing_label(
            fresh
                .identifier
                .as_deref()
                .expect("rollover_target_ok refuses a role with no identifier"),
        ),
    };
    let proof = exclusive_ok(
        verb,
        &survey_consumers(&mine, Some(secret_id), &fresh.survey),
    )?;
    rollover_write_ok(
        verb,
        secret_id,
        &rollover_inputs(state),
        &rollover_inputs_from(&fresh.versions, fresh.published.clone()),
    )?;
    Ok(proof)
}

/// Everything `init`'s pre-write recheck reads, after the prompt and the
/// production gate and before the first of its writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRecheck {
    /// The entity, read fresh.
    pub entity: Value,
    /// The alias at the planned label now.
    pub mapping: Option<String>,
    /// A fresh consumer survey of every realm in [`SURVEYED_REALMS`].
    pub survey: Vec<RealmSurvey>,
}

/// `init`'s counterpart of [`rollover_target_ok`]: is the setup still the
/// one the plan was made against?
///
/// Both halves are the rules the per-step writers already apply —
/// [`identifier_write_ok`] and [`mapping_write_ok`] — applied once more on
/// the far side of the prompt, before anything is sent, so the refusal is
/// never one that follows a completed step.
pub fn init_target_ok(
    state: &RotationState,
    plan: &InitPlan,
    planned_mapping: Option<&str>,
    entity: &Value,
    mapping: Option<&str>,
) -> Result<()> {
    identifier_write_ok(state.identifier.as_deref(), entity, state.role)?;
    mapping_write_ok(&plan.label, &plan.secret_id, planned_mapping, mapping)
}

/// The whole pre-write decision for `init`, and the only source of the
/// [`ExclusivityProof`] its writes hold — [`rollover_pre_write_ok`]'s
/// sibling, in the same order: the target first, then the re-survey.
///
/// `mine` is this role through the **planned** label, because that is the
/// label `init` is about to make it resolve; the target check has just shown
/// the identifier is unchanged, so there is no fresher one to use.
pub fn init_pre_write_ok(
    state: &RotationState,
    plan: &InitPlan,
    planned_mapping: Option<&str>,
    fresh: &InitRecheck,
) -> Result<ExclusivityProof> {
    init_target_ok(
        state,
        plan,
        planned_mapping,
        &fresh.entity,
        fresh.mapping.as_deref(),
    )?;
    let mine = Consumer {
        realm: state.realm.clone(),
        entity_id: state.entity_id.clone(),
        location: state.location,
        role: state.role,
        label: plan.label.clone(),
    };
    exclusive_ok(
        "init",
        &survey_consumers(&mine, Some(&plan.secret_id), &fresh.survey),
    )
}

/// Whether the signing label is still unmapped, as `plan_init` found it.
///
/// The orphan case from the other side. `plan_init` refuses when the label
/// already maps somewhere, and `map_label` writes when it does not — but
/// `set_mapping` is a `PUT` that updates as happily as it creates, so a
/// mapping made in the gap is overwritten with nothing reported. That is
/// another entity's rotation, or an orphan someone is in the middle of
/// unpicking.
///
/// Identical is the only case that may write, and that includes "someone else
/// already mapped it to exactly what we wanted": the plan said this label was
/// free, it is not now, and re-running is what should decide from the state
/// that exists — `plan_init` then skips the step and nothing is sent. Same
/// rule as [`identifier_write_ok`], for the same reason.
pub fn mapping_write_ok(
    label: &str,
    secret_id: &str,
    planned_from: Option<&str>,
    fresh: Option<&str>,
) -> Result<()> {
    if planned_from == fresh {
        return Ok(());
    }
    Err(Error::Config(format!(
        "the secret label {label} changed while this run was planning — it mapped to {} when \
         the plan was made and maps to {} now, so something else is writing this realm's \
         mappings. Pointing it at {secret_id} would overwrite that with nothing reported, and \
         a mapping outlives the label that named it, so the overwritten one may be the only \
         thing naming an ESV secret. **Nothing has been sent.** Check it with \
         `aic secretmap list` and re-run.",
        quoted(planned_from),
        quoted(fresh),
    )))
}

/// A list of identities for a message, or the word for having none.
fn named<'a>(values: impl Iterator<Item = &'a String>) -> String {
    let joined = values.cloned().collect::<Vec<_>>().join(", ");
    if joined.is_empty() {
        "none".to_string()
    } else {
        joined
    }
}

/// Whether the entity that came back is the entity that was sent.
///
/// Returns the paths that differ, so a write AM quietly reshaped is reported
/// as *what* it lost rather than as "mismatch". `_rev` is excluded because it
/// is content-derived and our content changed on purpose.
///
/// A difference is reported at the **highest path where the two documents
/// stop both being objects**, not at every leaf below it. That is what makes
/// the report readable in the case it exists for: `{"entityId": "<same>"}`
/// answers 200 and deletes the whole role block, and the useful sentence is
/// `/serviceProvider` rather than the forty leaves inside it. A leaf dropped
/// from a block that survived is still named as that leaf, because both sides
/// are objects the whole way down to it.
pub fn entity_write_differences(intended: &Value, after: &Value) -> Vec<String> {
    let mut differences = Vec::new();
    diff_into(
        &strip_rev(intended),
        &strip_rev(after),
        "",
        &mut differences,
    );
    differences.sort();
    differences
}

fn strip_rev(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_rev).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "_rev")
                .map(|(key, value)| (key.clone(), strip_rev(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn diff_into(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
            for key in keys {
                diff_into(
                    left.get(key).unwrap_or(&Value::Null),
                    right.get(key).unwrap_or(&Value::Null),
                    &format!("{path}/{key}"),
                    out,
                );
            }
        }
        _ => out.push(if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        }),
    }
}

/// What `GET /environment/secrets/{id}` says about the secret behind the
/// label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFacts {
    pub id: String,
    pub encoding: String,
    pub use_in_placeholders: bool,
    pub loaded: bool,
    pub active_version: String,
    pub loaded_version: String,
}

pub fn parse_secret_facts(id: &str, secret: &Value) -> SecretFacts {
    let text = |key: &str| {
        secret
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    SecretFacts {
        id: id.to_string(),
        encoding: text("encoding"),
        use_in_placeholders: secret
            .get("useInPlaceholders")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        loaded: secret
            .get("loaded")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        active_version: text("activeVersion"),
        loaded_version: text("loadedVersion"),
    }
}

/// One entry of the bare `GET …/versions` array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretVersion {
    pub version: String,
    pub status: String,
    pub create_date: String,
}

impl SecretVersion {
    pub fn enabled(&self) -> bool {
        self.status == "ENABLED"
    }

    /// Versions are numbered, and AIC's "latest" is the numerically highest —
    /// so `"10"` is later than `"9"`, which a string comparison gets wrong.
    pub fn number(&self) -> Option<u64> {
        self.version.parse().ok()
    }
}

pub fn parse_versions(versions: &[Value]) -> Vec<SecretVersion> {
    versions
        .iter()
        .map(|version| {
            let text = |key: &str| {
                version
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            SecretVersion {
                version: text("version"),
                status: text("status"),
                create_date: text("createDate"),
            }
        })
        .collect()
}

/// Everything the four reads found, before anything is decided about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationState {
    pub tenant: String,
    pub realm: String,
    pub entity_id: String,
    pub location: Location,
    pub role: Role,
    /// The role's `secretIdIdentifier`, if it has one.
    pub identifier: Option<String>,
    /// The ESV secret the signing label maps to, if the label is mapped.
    pub mapped_alias: Option<String>,
    /// The mapped secret's metadata, when that secret exists.
    pub secret: Option<SecretFacts>,
    pub versions: Vec<SecretVersion>,
    /// Every certificate the exported metadata publishes, all roles.
    pub certs: Vec<CertRef>,
    /// What this install recorded when it staged, if it was this install.
    pub record: Option<StagedRecord>,
}

impl RotationState {
    /// The signing certificates **this role** publishes.
    ///
    /// Role-scoped, not document-scoped. A dual-role entity publishes
    /// `use="signing"` from both its IdP and its SP role, AM emits no
    /// `<ds:KeyName>` at all, and a whole-document count therefore reports two
    /// certificates for a role that has one (`docs/api/06-saml.md`).
    pub fn published(&self) -> Vec<&CertRef> {
        let descriptor = role_descriptor(self.role);
        self.certs
            .iter()
            .filter(|cert| {
                cert.descriptor == descriptor && cert.key_use.as_deref() == Some(SIGNING_USE)
            })
            .collect()
    }

    pub fn published_fingerprints(&self) -> BTreeSet<String> {
        self.published()
            .into_iter()
            .map(|cert| cert.sha256.clone())
            .collect()
    }

    pub fn enabled_versions(&self) -> Vec<&SecretVersion> {
        enabled_in(&self.versions)
    }

    /// The numerically highest version, whatever its status — the one AIC
    /// refuses to disable (`docs/api/03-esvs.md`).
    pub fn latest_version(&self) -> Option<&SecretVersion> {
        self.versions.iter().max_by_key(|version| version.number())
    }

    pub fn label(&self) -> Option<String> {
        self.identifier.as_deref().map(signing_label)
    }

    /// This rotation's own entry in [`survey_consumers`]'s answer.
    ///
    /// `None` before an identifier exists: a role signing with the realm
    /// default resolves no label of its own, so there is nothing whose
    /// exclusivity could be surveyed.
    pub fn consumer(&self) -> Option<Consumer> {
        Some(Consumer {
            realm: self.realm.clone(),
            entity_id: self.entity_id.clone(),
            location: self.location,
            role: self.role,
            label: self.label()?,
        })
    }
}

/// The ENABLED versions of one ESV secret, in numeric order.
///
/// Free rather than a method because the freshness recheck reads versions
/// without building a whole [`RotationState`] around them.
pub fn enabled_in(versions: &[SecretVersion]) -> Vec<&SecretVersion> {
    let mut enabled: Vec<&SecretVersion> = versions.iter().filter(|v| v.enabled()).collect();
    enabled.sort_by_key(|version| version.number());
    enabled
}

/// Where a rotation currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// The role has no `secretIdIdentifier`, so it signs with the realm-wide
    /// default certificate. Nothing here is rotatable yet.
    Unconfigured,
    /// The identifier exists but its signing label is unmapped, so resolution
    /// still falls through to the realm default. Measured: setting the
    /// identifier alone changes the exported metadata not at all.
    Unmapped,
    /// The label maps to an ESV secret that does not exist. `aic secretmap`
    /// will happily point a label at a name, and AM resolves nothing.
    Dangling { alias: String },
    /// The secret exists but cannot back a restart-free rotation, and neither
    /// property can be changed after creation.
    Unusable { reason: String },
    /// One ENABLED version, one published certificate. The steady state, and
    /// the only one `stage` will act from.
    Settled,
    /// Two ENABLED versions and two published certificates. **Treat signing
    /// as having already moved to the newer version**: on the measured SP
    /// path the newest ENABLED version was the signer by the first
    /// observation (2026-09-22, `docs/api/06-saml.md`; the IdP path is
    /// unmeasured and assumed the same), so this is the state that *follows* a
    /// cutover, not the one that precedes it. What is still open is the old
    /// certificate's publication, which `complete` closes.
    Staged,
    /// The reads do not agree, so no verb may act on them.
    Inconsistent { detail: String },
}

pub fn phase(state: &RotationState) -> Phase {
    if state.identifier.is_none() {
        return Phase::Unconfigured;
    }
    let Some(alias) = state.mapped_alias.as_deref() else {
        return Phase::Unmapped;
    };
    let Some(secret) = state.secret.as_ref() else {
        return Phase::Dangling {
            alias: alias.to_string(),
        };
    };
    if let Some(reason) = unusable_reason(secret) {
        return Phase::Unusable { reason };
    }

    let enabled = state.enabled_versions().len();
    let published = state.published().len();
    match (enabled, published) {
        (1, 1) => Phase::Settled,
        (2, 2) => Phase::Staged,
        (enabled, published) if enabled == published => Phase::Inconsistent {
            detail: format!(
                "the secret has {enabled} ENABLED versions and the export publishes {published} \
                 signing certificates for this role — a rollover is two, a steady state is one"
            ),
        },
        (enabled, published) => Phase::Inconsistent {
            detail: format!(
                "the secret has {enabled} ENABLED version(s) but the export publishes \
                 {published} signing certificate(s) for this role; either a change has not \
                 propagated yet (it takes single-digit seconds) or something else resolves \
                 this label"
            ),
        },
    }
}

/// Why a secret cannot back a rotation. Both properties are set at create and
/// **immutable** — `PUT` is create-only and `setDescription` is the only
/// action — so the remedy is always delete-and-recreate, never an update.
fn unusable_reason(secret: &SecretFacts) -> Option<String> {
    if secret.encoding != "pem" {
        return Some(format!(
            "ESV secret {} has encoding {:?}, not `pem`, so it cannot hold a key pair; encoding \
             is fixed at create, so this needs a new secret",
            secret.id, secret.encoding
        ));
    }
    if secret.use_in_placeholders {
        return Some(format!(
            "ESV secret {} was created with `useInPlaceholders: true`, so a new version stays \
             `loaded: false` until a tenant restart — a rollover through it is not the \
             restart-free operation this command reports. `useInPlaceholders` cannot be \
             changed after create; recreate the secret with --no-placeholders",
            secret.id
        ));
    }
    None
}

/// One (entity, role) that resolves a key through a given ESV secret.
///
/// Not an entity: a dual-role entity can point both role blocks at the same
/// identifier, and each role publishes its own `KeyDescriptor` from the one
/// secret. Nor a role: the same role resolves `signing`, `encryption` and
/// `mtls` through three labels off one identifier, and nothing stops two of
/// them being mapped to the same ESV secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    /// The realm the entity lives in. Part of the identity for the same reason
    /// location is: an ESV secret is tenant-global, so the same entity ID in
    /// `alpha` and in `bravo` are two consumers of it, not one.
    pub realm: String,
    pub entity_id: String,
    pub location: Location,
    pub role: Role,
    /// The secret label this role resolves the key through. It names the
    /// **use** as well as the identifier, because a secret shared between one
    /// entity's signing and encryption labels is shared just as dangerously
    /// as one shared between two entities.
    pub label: String,
}

impl Consumer {
    /// One line naming it in a refusal. Location and realm are part of the
    /// identity: the same entity ID can exist in both collections, and in
    /// both realms.
    pub fn describe(&self) -> String {
        format!(
            "{} ({} {} in realm {}) through {}",
            self.entity_id,
            self.location,
            role_descriptor(self.role),
            self.realm,
            self.label
        )
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        (
            &self.realm,
            &self.entity_id,
            self.location.as_str(),
            &self.label,
        )
    }
}

/// One entity provider in the realm, reduced to the only thing a rotation has
/// to know about it: which label family each of its roles names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityIdentifiers {
    pub entity_id: String,
    pub location: Location,
    /// `(role, secretIdIdentifier)` for each role block that names one. A
    /// role without one signs with the realm default and resolves no label,
    /// so it is absent rather than empty.
    pub identifiers: Vec<(Role, String)>,
}

/// Reduce one entity document to its per-role identifiers.
pub fn entity_identifiers(
    entity_id: &str,
    location: Location,
    document: &Value,
) -> EntityIdentifiers {
    EntityIdentifiers {
        entity_id: entity_id.to_string(),
        location,
        identifiers: [Role::Idp, Role::Sp]
            .into_iter()
            .filter_map(|role| current_identifier(document, role).map(|id| (role, id)))
            .collect(),
    }
}

/// Split a secret label back into the identifier it was minted from and the
/// use it addresses.
///
/// `None` for anything outside the SAML family: the mapping table is the
/// realm's whole secret-label collection, and most of it has nothing to do
/// with federation.
///
/// The split is at the **last** dot, which is exact for every label this tool
/// or AM's console can mint — [`validate_identifier`] and AM both refuse a dot
/// in an identifier. A hand-made label with a dotted identifier splits into an
/// identifier no entity can be carrying, so it is reported as claimed by
/// nobody rather than silently attributed to the wrong one.
pub fn label_parts(label: &str) -> Option<(&str, &str)> {
    let (identifier, key_use) = label.strip_prefix(LABEL_PREFIX)?.rsplit_once('.')?;
    (!identifier.is_empty() && !key_use.is_empty()).then_some((identifier, key_use))
}

/// Every realm a consumer of an ESV secret can be found in, and so every realm
/// [`survey_consumers`] must be handed before it will call a secret exclusive.
///
/// `alpha` and `bravo`, because those are the realms AIC gives a tenant and
/// the only two this tool addresses (`docs/api/01-realms-and-paths.md`). The
/// **root** realm is not here, and that is a measured gap rather than a
/// decision — see [`SHARING_CAVEAT`].
pub const SURVEYED_REALMS: &[&str] = &["alpha", "bravo"];

/// What one realm contributed to a survey: its secret-label mapping table and
/// every entity provider in it.
///
/// Per realm because both halves are: the mapping table lives in the realm's
/// ESV secret store, and the entity collection is realm-scoped. The ESV secret
/// they point at is not, which is the whole reason a survey has several of
/// these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealmSurvey {
    pub realm: String,
    pub mappings: Vec<Mapping>,
    pub entities: Vec<EntityIdentifiers>,
}

/// Who, anywhere on the tenant, resolves a key through the ESV secret a
/// rotation is about.
///
/// **This is the release-blocking question, not a nicety.** AIC explicitly
/// permits one secret label to back several providers, and `stage` and
/// `complete` mutate the ESV *secret* — globally — while every export check
/// and every line of the report covers only the entity the operator named. A
/// completion that retires the certificate a second entity's peer is still
/// verifying reports success, because the second entity was never looked at.
///
/// Two channels reach the same secret, and missing either is how that
/// happens:
///
/// 1. **the same identifier.** `secretIdIdentifier` is free text and AM
///    enforces no uniqueness across entities, so several role blocks can name
///    one label family.
/// 2. **a second label on the same secret.** The mapping table is
///    many-to-one, so `…spb.signing` and `…spa.encryption` can both point at
///    the ESV secret this rollover is about.
///
/// Both are found through each realm's mapping table, which is the choke
/// point: a label resolves only where it is mapped, so every consumer of this
/// secret in a realm holds a label that realm's table names. `mine`'s own
/// label joins that set **in `mine`'s realm** whether or not it is mapped yet,
/// because `init` is the verb that maps it — and an `init` that adopts an
/// identifier a second entity is already carrying is how the sharing gets
/// created in the first place. In any other realm the same identifier is only
/// a consumer if that realm maps its label onto this secret, which the table
/// already says; an identifier is a namespace per realm, not per tenant.
///
/// **Tenant-wide, and checked rather than assumed.** A realm in
/// [`SURVEYED_REALMS`] that is absent from `realms` is reported in
/// [`Consumers::unsurveyed`], which makes the survey not exclusive: a caller
/// that surveyed one realm gets a refusal naming the other, rather than a
/// proof that covers less than it says.
pub fn survey_consumers(
    mine: &Consumer,
    secret_id: Option<&str>,
    realms: &[RealmSurvey],
) -> Consumers {
    let mut others = Vec::new();
    let mut unclaimed_labels = Vec::new();
    for surveyed in realms {
        let here = surveyed.realm == mine.realm;
        // `Mapping::secret_id` is AM's name for the **label**; the ESV secret
        // is its `alias`. Getting those two the wrong way round would survey
        // the right table for the wrong thing.
        let mut labels: BTreeSet<String> = surveyed
            .mappings
            .iter()
            .filter(|mapping| secret_id.is_some() && mapping.alias.as_deref() == secret_id)
            .map(|mapping| mapping.secret_id.clone())
            .collect();
        if here {
            labels.insert(mine.label.clone());
        }

        for label in &labels {
            let claimants: Vec<Consumer> = label_parts(label)
                .map(|(identifier, _)| {
                    surveyed
                        .entities
                        .iter()
                        .flat_map(|entity| {
                            entity
                                .identifiers
                                .iter()
                                .filter(|(_, named)| named == identifier)
                                .map(|(role, _)| Consumer {
                                    realm: surveyed.realm.clone(),
                                    entity_id: entity.entity_id.clone(),
                                    location: entity.location,
                                    role: *role,
                                    label: label.clone(),
                                })
                        })
                        .collect()
                })
                .unwrap_or_default();
            // `mine`'s own label is never unclaimed: on a fresh `init` the
            // entity has not been written yet, so nothing names it, and that
            // is the state `init` exists to leave behind rather than a finding.
            if claimants.is_empty() && !(here && label == &mine.label) {
                unclaimed_labels.push(UnclaimedLabel {
                    realm: surveyed.realm.clone(),
                    label: label.clone(),
                });
            }
            others.extend(claimants.into_iter().filter(|consumer| consumer != mine));
        }
    }
    others.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    let covered: BTreeSet<&str> = realms.iter().map(|realm| realm.realm.as_str()).collect();
    let unsurveyed = SURVEYED_REALMS
        .iter()
        .copied()
        .chain(std::iter::once(mine.realm.as_str()))
        .filter(|realm| !covered.contains(realm))
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Consumers {
        realms: covered.into_iter().map(str::to_string).collect(),
        unsurveyed,
        secret_id: secret_id.map(str::to_string),
        mine: mine.clone(),
        others,
        unclaimed_labels,
    }
}

/// A label mapped onto the surveyed secret that no entity provider names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnclaimedLabel {
    pub realm: String,
    pub label: String,
}

/// What [`survey_consumers`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumers {
    /// The realms whose tables and entities were read.
    pub realms: Vec<String>,
    /// Realms that should have been and were not. Never non-empty on a survey
    /// `ops` made; the field exists so a survey that covered less than the
    /// tenant cannot be mistaken for one that covered all of it.
    pub unsurveyed: Vec<String>,
    /// The ESV secret surveyed, when the label is mapped to one. `None` is
    /// the pre-`init` state, where only the identifier can be shared.
    pub secret_id: Option<String>,
    pub mine: Consumer,
    /// Every other (entity, role) resolving a key through it.
    pub others: Vec<Consumer>,
    /// Labels on it that no entity names: an orphan mapping, or a label
    /// belonging to something that is not SAML at all. Neither is resolving a
    /// key **today**, and both say the secret is not this rollover's alone — a
    /// non-SAML label is another AM subsystem holding the same key, and an
    /// orphan is one entity delete away from being claimed again.
    pub unclaimed_labels: Vec<UnclaimedLabel>,
}

impl Consumers {
    pub fn exclusive(&self) -> bool {
        self.others.is_empty() && self.unclaimed_labels.is_empty() && self.unsurveyed.is_empty()
    }

    /// What the secret is called in a report — its ESV id, or the label that
    /// would resolve to one, before `init` has mapped it.
    fn subject(&self) -> &str {
        self.secret_id.as_deref().unwrap_or(&self.mine.label)
    }

    fn realm_list(&self) -> String {
        self.realms.join(", ")
    }
}

/// Proof that a rotation's ESV secret backs the role being rotated and
/// nothing else, anywhere on the tenant.
///
/// The field is private and [`exclusive_ok`] is the only thing that fills it
/// in. It is minted **twice** per write, and the two proofs do different jobs:
///
/// - the plan-time one is **consumed** by [`authorize_init`],
///   [`authorize_stage`] or [`authorize_complete`], so a verb that forgot to
///   survey cannot mint a write permit — the permit construction, one step
///   earlier;
/// - the write-time one is minted inside `ops`' pre-write recheck, after the
///   confirmation prompt and the production gate, from a survey that recheck
///   made itself. The plan-time proof is gone by then (moved into the
///   authorizer), so the proof in hand at the write is by construction no
///   older than the recheck — which is what closes "a consumer added while
///   the operator was reading the prompt".
///
/// What it proves is the **coverage** of the survey it was minted from —
/// [`survey_consumers`] refuses exclusivity to a survey missing a realm — and
/// that the survey said "nobody else". It cannot prove the reads behind it
/// were honest, which is the same limit every permit here has.
#[derive(Debug)]
pub struct ExclusivityProof {
    _minted_by_exclusive_ok: (),
}

/// Refuse a rotation of a secret that is not this role's alone.
///
/// The refusal is the inventory, because the operator cannot act on "shared"
/// — they need the names, and the remedy differs per consumer. There is no
/// `--force`: a shared rollover is not one operation with a risk attached but
/// several operations this command cannot sequence, since adding a version
/// may cut **every** consumer's signing over at the same instant, and each
/// consumer's peer has to have been given the new certificate before that.
pub fn exclusive_ok(verb: &str, consumers: &Consumers) -> Result<ExclusivityProof> {
    if consumers.exclusive() {
        return Ok(ExclusivityProof {
            _minted_by_exclusive_ok: (),
        });
    }
    let mut lines = vec![format!(
        "cannot {verb} a rollover of {}: it does not back {} alone, and a rollover of it is not \
         a change to one entity. Adding a version is treated as cutting **every** consumer over \
         to the new certificate at once, so every one of their peers has to trust it \
         beforehand; and disabling one retires a certificate every consumer's peer must already \
         have stopped verifying. This command checks the export, the phase and the settlement \
         of {} only. AIC permits a secret label to be shared between providers, so this is a \
         supported tenant state and not a corrupted one. **Nothing has been sent.**",
        consumers.subject(),
        consumers.mine.describe(),
        consumers.mine.entity_id,
    )];
    if !consumers.others.is_empty() {
        lines.push("  also resolving it:".to_string());
        lines.extend(
            consumers
                .others
                .iter()
                .map(|consumer| format!("    {}", consumer.describe())),
        );
    }
    if !consumers.unclaimed_labels.is_empty() {
        lines.push("  mapped onto it, but named by no entity provider in its realm:".to_string());
        lines.extend(
            consumers
                .unclaimed_labels
                .iter()
                .map(|unclaimed| format!("    {} (realm {})", unclaimed.label, unclaimed.realm)),
        );
    }
    if !consumers.unsurveyed.is_empty() {
        lines.push(format!(
            "  not surveyed, so not known to be free of it: realm {}",
            consumers.unsurveyed.join(", realm ")
        ));
    }
    lines.push(format!(
        "  give this role a label and an ESV secret of its own if it should roll on its own \
         schedule; otherwise roll the shared key deliberately: once **every** peer above has \
         been given the new certificate and trusts it, `aic esv secret add-version {0}` (which \
         is where signing changes for all of them), then `aic esv secret disable {0} <n>`. \
         `aic secretmap list --realm <realm>` shows what maps where.",
        consumers.subject(),
    ));
    lines.push(SHARING_CAVEAT.to_string());
    Err(Error::Config(lines.join("\n")))
}

/// The sentence every sharing report carries: what the survey still cannot
/// see now that it covers every realm this tool addresses.
pub const SHARING_CAVEAT: &str = "\
Who resolves an ESV secret is surveyed across realms alpha and bravo. The root realm is not read:
AIC answers 403 for every root realm-config family measured so far (scripts, OAuth2 clients,
Trusted JWT issuers), but whether it can hold a SAML entity provider or a secret mapping has not
been measured.";

/// The sharing rows of `aic saml rotate status`.
fn sharing_lines(consumers: &Consumers) -> Vec<String> {
    if consumers.exclusive() {
        return vec![format!(
            "sharing     nothing else in realms {} resolves {}",
            consumers.realm_list(),
            consumers.subject()
        )];
    }
    let mut lines = vec![format!(
        "sharing     SHARED — {} also backs:",
        consumers.subject()
    )];
    lines.extend(
        consumers
            .others
            .iter()
            .map(|consumer| format!("              {}", consumer.describe())),
    );
    lines.extend(consumers.unclaimed_labels.iter().map(|unclaimed| {
        format!(
            "              {} in realm {} (mapped, named by no entity provider)",
            unclaimed.label, unclaimed.realm
        )
    }));
    lines.extend(
        consumers
            .unsurveyed
            .iter()
            .map(|realm| format!("              realm {realm} (not surveyed)")),
    );
    lines
}

/// Permission to add one ESV secret version.
///
/// The field is private to this module and [`authorize_stage`] is the only
/// thing that fills it in, so the tenant write is **unreachable** from a
/// `--dry-run`: the preview arm holds no permit, and a preview that fell
/// through to the call would not compile. Same shape as
/// [`crate::saml::spec::ImportPermit`] and `scripts::gate`'s `WritePermit`.
#[derive(Debug)]
pub struct StagePermit {
    _minted_by_authorize_stage: (),
}

/// Permission to disable one ESV secret version.
#[derive(Debug)]
pub struct CompletePermit {
    _minted_by_authorize_complete: (),
}

/// Permission to write the entity, create the secret and set the mapping.
#[derive(Debug)]
pub struct InitPermit {
    _minted_by_authorize_init: (),
}

#[derive(Debug)]
pub enum Decision<P> {
    /// Print the plan and stop. Carries no permit.
    Preview,
    Send(P),
}

/// What `stage` would do, worked out before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePlan {
    pub secret_id: String,
    /// The certificate currently published, which must survive the stage.
    pub retained: String,
    /// The one ENABLED version before the stage — the version holding
    /// `retained`, since `Settled` means exactly one of each. Named because
    /// the way back needs it and nothing can work it out once this run is
    /// gone.
    pub retained_version: String,
    /// The certificate in the supplied key pair.
    pub incoming: String,
    /// What the export must settle on once the version exists.
    pub expected: BTreeSet<String>,
}

pub fn plan_stage(state: &RotationState, incoming: &KeyPair) -> Result<StagePlan> {
    let current = phase(state);
    let secret = match (&current, state.secret.as_ref()) {
        (Phase::Settled, Some(secret)) => secret,
        (Phase::Staged, _) => {
            return Err(Error::Config(format!(
                "a rollover is already staged for {} ({}): two certificates are published and \
                 the newer one should be treated as the one signing. Finish it with \
                 `aic saml rotate complete`, or re-enable the state you want with \
                 `aic esv secret enable`.",
                state.entity_id,
                role_descriptor(state.role)
            )));
        }
        (other, _) => return Err(not_ready("stage", state, other)),
    };

    let published = state.published_fingerprints();
    let retained = published
        .iter()
        .next()
        .cloned()
        .expect("Settled means exactly one published certificate");
    if published.contains(&incoming.sha256) {
        return Err(Error::Config(format!(
            "the certificate in that key pair is already the one {} publishes ({}); staging it \
             would add a second ESV secret version holding the same certificate, which is a \
             rollover that rotates nothing",
            state.entity_id, incoming.sha256
        )));
    }

    let retained_version = state
        .enabled_versions()
        .first()
        .map(|version| version.version.clone())
        .expect("Settled means exactly one ENABLED version");

    let mut expected = published;
    expected.insert(incoming.sha256.clone());
    Ok(StagePlan {
        secret_id: secret.id.clone(),
        retained,
        retained_version,
        incoming: incoming.sha256.clone(),
        expected,
    })
}

/// What is known about **when** signing moves to a newly added version, for
/// one role — stated as a measurement where there is one and as an assumption
/// where there is not.
///
/// The 2026-09-22 measurement (`docs/api/06-saml.md`) covered the **SP
/// AuthnRequest-signing** path only: the signer had moved to the new version by
/// the first observation, about 12 s after the version was added, and was
/// unchanged minutes later. Nothing measured the IdP's assertion signing. The
/// resolution is the same secret label through the same store, so the same
/// behaviour is plausible — but plausible is a reason to prepare as though it
/// were true, not a reason to report it as observed. So the *warning* is the
/// same for both roles (the conservative direction) and the *evidence* line is
/// not.
pub fn cutover_evidence(role: Role) -> &'static str {
    match role {
        Role::Sp => {
            "measured on this path (SP AuthnRequest signing, 2026-09-22): signing had moved to \
             the new version by the first observation, no later than 12 s after it was added"
        }
        Role::Idp => {
            "IdP assertion signing is unmeasured — only the SP AuthnRequest path was measured — \
             so assume signing moves to the new version immediately"
        }
    }
}

/// What `stage` says before it sends anything — and the reason it is here
/// rather than inline in `cli`.
///
/// **Treat `stage` as the cutover.** On the measured SP path the signer moved
/// to the newest ENABLED version by the first observation, ≤12 s after it was
/// added ([`cutover_evidence`]), so a peer that does not already trust the
/// incoming certificate starts rejecting this role's signatures the moment the
/// version exists — before it could possibly have loaded a certificate that
/// did not exist until then. The IdP path is unmeasured and is warned about
/// the same way, as an assumption. The two-certificate export that follows is
/// how a peer which *refreshes* metadata catches up; it is not a window in
/// which to prepare, and the module used to describe it as one.
///
/// That makes this the one message an operator must read before confirming, so
/// it is built by a function a test can drive rather than assembled from
/// `eprintln!`s no test can see. It prints on `--dry-run` too, which is where
/// it is most useful.
///
/// It names **emergency signer restoration** because the obvious rollback does
/// not exist: AIC refuses to disable the latest version (`400 Cannot disable
/// latest secret version`). Restoration is another `add-version` holding the
/// old key pair, and it is only half of the way back — see
/// [`restoration_lines`].
pub fn stage_plan_lines(state: &RotationState, plan: &StagePlan) -> Vec<String> {
    let role = role_descriptor(state.role);
    let mut lines = vec![
        format!(
            "will add a version to ESV secret {} holding certificate {}",
            plan.secret_id, plan.incoming
        ),
        format!(
            "  **treat this as the signing cutover**: {} ({role}) is expected to start signing \
             with {} as soon as the version exists — not when the peer next loads metadata, and \
             not when `complete` runs. {}",
            state.entity_id,
            plan.incoming,
            cutover_evidence(state.role)
        ),
        format!(
            "  stop here unless the peer already holds and trusts {}: from that moment it is \
             what verifies this role's signatures, and a peer that does not have it starts \
             rejecting them",
            plan.incoming
        ),
        format!(
            "  {} ({role}) will then publish both {} and {}. The already-published {} stays \
             published — nothing is retired here — but it is expected to stop being the \
             certificate in use; that export is how a peer which refreshes metadata catches up \
             afterwards",
            state.entity_id, plan.retained, plan.incoming, plan.retained
        ),
    ];
    lines.extend(restoration_lines(plan, "<the version this adds>"));
    lines
}

/// What `stage` says once the tenant has confirmed the new certificate is
/// published.
///
/// The plan lines above warn; this one reports, and an operator who confirmed
/// past the warning and then found out reads it here. What it reports as
/// **read** is publication, because that is what this run read; which
/// certificate signs is stated as the expectation [`cutover_evidence`] supports
/// for this role, never as an observation this run did not make. It names
/// restoration a second time with both version numbers in hand, because this
/// is the moment it is needed and [`stage_plan_lines`] has scrolled away.
pub fn stage_outcome_lines(state: &RotationState, plan: &StagePlan, version: &str) -> Vec<String> {
    let mut lines = vec![format!(
        "{} ({}) publishes {} as version {version}; treat it as the certificate signing now, \
         not from whenever the peer loads the metadata — {}",
        state.entity_id,
        role_descriptor(state.role),
        plan.incoming,
        cutover_evidence(state.role)
    )];
    lines.extend(restoration_lines(plan, version));
    lines
}

/// Emergency signer restoration, stated completely.
///
/// "Add the old key pair again" restores the signer and leaves three ENABLED
/// versions — the one that was in service, the one `stage` added, and the
/// restored copy — which is a state `rotate` treats like its other off-path
/// states (`Inconsistent`: nothing automatic), not one it finishes. So the
/// lines name the finish as well: disable the two superseded versions, which
/// is possible because neither is the latest any more. And they say what the
/// restoration needs, because nothing here keeps it: the **old private key**,
/// not only its certificate — an ESV secret value is write-only, so a key
/// pair that was not kept cannot be read back out of the tenant.
fn restoration_lines(plan: &StagePlan, added: &str) -> Vec<String> {
    vec![
        format!(
            "  there is no un-stage: AIC refuses to disable the version just added (400 Cannot \
             disable latest secret version). **Emergency signer restoration** puts {} back in \
             front by adding its key pair again as a *newer* version — `aic esv secret \
             add-version {} --value-file <old key pair>` — and it needs the **old private key** \
             as well as its certificate: ESV secret values are write-only, so a key pair that \
             was not kept cannot be recovered from the tenant",
            plan.retained, plan.secret_id
        ),
        format!(
            "  restoration alone leaves versions {}, {added} and the restored one all ENABLED, a \
             state `rotate` will not finish; settle it by disabling the two superseded versions \
             — `aic esv secret disable {} {added}` and `aic esv secret disable {} {}` — which \
             leaves the restored version, and {}, alone. Nothing is destroyed",
            plan.retained_version,
            plan.secret_id,
            plan.secret_id,
            plan.retained_version,
            plan.retained
        ),
    ]
}

/// Permission to cut signing over: `--force` was supplied, or the operator
/// confirmed at a terminal.
///
/// [`complete_ok`]'s sibling, and the same shape on purpose. `stage` is the
/// verb that breaks a peer which was not prepared — more so than `complete`,
/// which on its usual path is not expected to change the signer — so it asks the same question
/// in the same way: `--dry-run`, the permits and the production `--yes` gate
/// each answer something else, and none of them is "have you pre-trusted the
/// certificate this names".
pub fn stage_ok(forced: bool, plan: &StagePlan, state: &RotationState) -> Result<()> {
    if forced {
        return Ok(());
    }
    Err(Error::Config(format!(
        "would add certificate {} to ESV secret {} as a new version, which is treated as \
         cutting {} ({}) signing over to it at once. A peer that does not already hold and trust \
         {} starts rejecting this role's signatures from then, and there is no un-stage — only \
         emergency signer restoration, which needs the old private key. Confirm at a terminal, \
         or pass --force.",
        plan.incoming,
        plan.secret_id,
        state.entity_id,
        role_descriptor(state.role),
        plan.incoming,
    )))
}

pub fn authorize_stage(dry_run: bool, _exclusive: ExclusivityProof) -> Decision<StagePermit> {
    if dry_run {
        Decision::Preview
    } else {
        Decision::Send(StagePermit {
            _minted_by_authorize_stage: (),
        })
    }
}

/// Where the version to disable came from.
///
/// Carried on the plan because it is the whole safety argument. Disabling a
/// version retires whichever certificate that version holds, and the only two
/// honest sources for that pairing are this install's own record and the
/// operator's own knowledge. Ordering is not a third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionChoice {
    /// Derived from [`StagedRecord`], the one place a fingerprint is tied to a
    /// version number. Also what a `--disable-version` that **agrees** with
    /// the record is: the derivation is still the command's, and reporting it
    /// as the operator's claim would understate what is known.
    Recorded,
    /// Named with `--disable-version`, where no record could decide. Nothing
    /// readable corroborates it, so the report says whose claim it is.
    Named,
}

/// What `complete` would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePlan {
    pub secret_id: String,
    /// The ESV secret version to disable.
    pub disable_version: String,
    /// How that version was chosen.
    pub chosen_by: VersionChoice,
    /// The certificate that must still be published afterwards.
    pub retain: String,
    /// The certificate expected to stop being published, when this install
    /// knows which one that is. `None` means it does not, and the report has
    /// to say so rather than name the other fingerprint by elimination —
    /// which would be right only if the version/certificate pairing were
    /// known, and it is exactly what is not known.
    pub expected_drop: Option<String>,
    /// Whether the version being disabled is the **newest ENABLED** one —
    /// the one [`SIGNER_RULE`] says to treat as signing. `complete` usually
    /// retires the other version and is not expected to change the signer; keeping the older
    /// certificate (`--retain` naming it, reachable when a DISABLED spare
    /// sits above the staged version) disables the signing one, and that is a
    /// cutover back to the retained certificate, with the same peer
    /// precondition as `stage`.
    pub moves_signer: bool,
}

/// Decide which version to disable, and what must remain afterwards.
///
/// `retain` is the fingerprint the operator wants to keep; `disable_version`
/// is `--disable-version`, the escape hatch for when nothing here can work the
/// version out — and **only** for then. Where a usable record exists it
/// decides, and the flag may agree with it or be refused; see
/// [`contradicted_pairing`].
///
/// **Retaining a fingerprint does not select a version, and this used to
/// assume it did.** `--retain` only proved the certificate was currently
/// published; the version disabled was then always the oldest ENABLED one, so
/// asking to keep the *older* certificate disabled the very version holding
/// it. ESV secret values are write-only and AM publishes no `<ds:KeyName>`, so
/// the export cannot supply the missing half — "the newest ENABLED version is
/// listed first" is an ordering convention, not an identity. The mistake
/// surfaced only in the post-write export check, after a live certificate had
/// already stopped being published.
///
/// So the version is **derived from the pairing** or not derived at all:
///
/// - with a [`usable_pairing`], the version follows in both directions — keep
///   what was staged and the other ENABLED version goes; keep what was there
///   before and the staged version goes;
/// - with no usable record there is no pairing, and `--disable-version` is
///   required. That refuses the case that used to work by luck as well as the
///   case that used to fail: "it happened to agree" is not knowledge, and the
///   two are indistinguishable from in here.
pub fn plan_complete(
    state: &RotationState,
    retain: Option<&str>,
    disable_version: Option<&str>,
) -> Result<CompletePlan> {
    let current = phase(state);
    let secret = match (&current, state.secret.as_ref()) {
        (Phase::Staged, Some(secret)) => secret,
        (Phase::Settled, _) => {
            return Err(Error::Config(format!(
                "nothing is staged for {} ({}): one certificate is published, which is the \
                 finished state. `aic saml rotate stage` starts a rollover.",
                state.entity_id,
                role_descriptor(state.role)
            )));
        }
        (other, _) => return Err(not_ready("complete", state, other)),
    };

    let published = state.published_fingerprints();
    let pairing = usable_pairing(state);

    let retain = match retain.map(str::trim).filter(|value| !value.is_empty()) {
        Some(retain) => retain.to_lowercase(),
        None => {
            let record = pairing.ok_or_else(|| no_pairing(state))?;
            record.sha256.clone()
        }
    };

    if !published.contains(&retain) {
        return Err(Error::Config(format!(
            "certificate {retain} is not one of the {} this role publishes, so keeping it is \
             not something this rollover can do; `aic saml rotate status` lists what is \
             published",
            published.len()
        )));
    }

    let enabled = state.enabled_versions();
    let named = disable_version
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let derived = pairing.map(|record| version_from_record(record, &enabled, &retain));

    let (disable_version, chosen_by) = match derived {
        // The record is the only thing that holds the pairing, so it decides.
        // `--disable-version` may **corroborate** it and may not overrule it:
        // a named version that disagrees is a claim about the same two
        // certificates made from nothing, and the plan it would produce
        // disables the certificate `--retain` just named. The choice also
        // stays `Recorded`, because it still is — reporting a corroborated
        // derivation as the operator's claim would understate what is known.
        Some(Ok(derived)) => {
            if let Some(named) = named.filter(|named| *named != derived) {
                return Err(contradicted_pairing(
                    pairing.expect("a derived version comes from a pairing"),
                    &derived,
                    named,
                    &retain,
                    &secret.id,
                ));
            }
            (derived, VersionChoice::Recorded)
        }
        // No usable pairing, so `--disable-version` is the only remaining
        // source and it is the operator's own knowledge. Without it there is
        // nothing left but ordering, which is the guess this verb exists to
        // refuse.
        other => {
            let named = named.ok_or_else(|| match other {
                Some(Err(error)) => error,
                _ => no_pairing(state),
            })?;
            if !enabled.iter().any(|version| version.version == named) {
                return Err(Error::Config(format!(
                    "--disable-version {named} is not an ENABLED version of {}: the ENABLED \
                     versions are {}. Only an ENABLED version publishes a certificate, so \
                     disabling anything else would change nothing.",
                    secret.id,
                    enabled
                        .iter()
                        .map(|version| version.version.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            (named.to_string(), VersionChoice::Named)
        }
    };

    // AIC's own rule, and now reachable rather than defensive: keeping the
    // older certificate means retiring the newer one, whose version is
    // normally the latest. There is no way to disable it, so the remedy is a
    // fresh rollover rather than a flag.
    if state
        .latest_version()
        .is_some_and(|latest| latest.version == disable_version)
    {
        return Err(Error::Config(format!(
            "version {disable_version} is the newest version of {}, and AIC refuses to disable \
             the latest version (`400 Cannot disable latest secret version`). Keeping {retain} \
             means retiring the certificate that version publishes, which this command cannot \
             do. Stage the key pair you want to keep as a new version instead \
             (`aic saml rotate stage --key-file …`) and complete that rollover.",
            secret.id
        )));
    }

    let expected_drop = pairing.and_then(|record| {
        if record.version == disable_version {
            // Direct: the record names the certificate this very version holds.
            Some(record.sha256.clone())
        } else if record.sha256 == retain {
            // The record ties `retain` to the version that survives, and
            // `Staged` means exactly two published — so the other one is what
            // goes. Still an expectation, and the post-write export check is
            // what settles it.
            published.iter().find(|cert| **cert != retain).cloned()
        } else {
            None
        }
    });

    let moves_signer = enabled
        .last()
        .is_some_and(|newest| newest.version == disable_version);

    Ok(CompletePlan {
        secret_id: secret.id.clone(),
        disable_version,
        chosen_by,
        retain,
        expected_drop,
        moves_signer,
    })
}

/// This install's record of the stage, but only while it still describes the
/// tenant in front of it.
///
/// A record is a claim about **four** things at once, and all four have to
/// still hold before a version may be read out of it: the role still points at
/// the identifier that was staged, its signing label still resolves to that
/// ESV secret, that secret still holds the version the record names **as an
/// ENABLED one**, and the certificate the record names is still published.
///
/// ENABLED rather than merely present, and the difference is not academic. A
/// DISABLED version publishes nothing, so a record naming one cannot be what
/// ties a published certificate to a version. The case that misleads is
/// identical certificate material in two versions, one disabled and one
/// enabled: the fingerprint matches throughout, and the version named is the
/// one publishing none of it. The dangerous *write* is caught downstream —
/// [`version_from_record`] derives only from ENABLED versions — but `status`
/// reads this same function, and the number it prints is the number an
/// operator then types into `--disable-version`.
///
/// A published fingerprint alone used to be the whole test, and the case it
/// misses is a realistic one: an entity deleted and recreated under the same
/// id, from the same key pair, with a different identifier or a different
/// secret — or simply with version numbering that restarted at 1. The
/// fingerprint matches throughout, and “version 2” now names something the
/// record has never seen. Disabling on that basis retires whichever
/// certificate the new version 2 happens to hold, which is the guess the
/// journal exists to replace.
///
/// Deliberately **not** a check that the entity is the same entity — nothing
/// readable answers that. Each of the four is a fact the rest of
/// [`plan_complete`] depends on, so a record disagreeing with any of them is
/// not a pairing rather than a suspicious one.
pub fn usable_pairing(state: &RotationState) -> Option<&StagedRecord> {
    let record = state.record.as_ref()?;
    let secret = state.secret.as_ref()?;
    (state.published_fingerprints().contains(&record.sha256)
        && state.identifier.as_deref() == Some(record.identifier.as_str())
        && secret.id == record.secret_id
        && state
            .versions
            .iter()
            .any(|version| version.version == record.version && version.enabled()))
    .then_some(record)
}

/// A hand-named `--disable-version` the record contradicts.
///
/// The pairing is the only evidence tying a certificate to a version, so a
/// named version that disagrees with it is a claim about the same two
/// certificates made from nothing. Refusing is what catches the symmetric
/// mistake: the record names the version to disable, `--disable-version` names
/// the **other** one, and that other one is — by elimination across exactly
/// two ENABLED versions and two published certificates — the one holding the
/// certificate `--retain` just asked to keep. Nothing before the write said
/// so; the post-write export check reported it after the certificate had
/// stopped being published.
fn contradicted_pairing(
    record: &StagedRecord,
    derived: &str,
    named: &str,
    retain: &str,
    secret_id: &str,
) -> Error {
    Error::Config(format!(
        "--disable-version {named} contradicts this install's own record of the stage, which \
         says version {} of {secret_id} holds certificate {}. Two ENABLED versions publish the \
         two certificates this role has, so keeping {retain} means disabling version {derived} \
         — and version {named} is the one holding the certificate you asked to keep, so \
         disabling it would retire {retain} and leave the other in service. The record is the \
         only thing that pairs a version with a certificate, so if it is the record that is \
         wrong, move `{}` aside and re-run with both flags rather than overruling it here. \
         `aic saml rotate status` lists the versions and the fingerprints.",
        record.version,
        record.sha256,
        crate::saml::rotate::journal::JOURNAL_FILE,
    ))
}

/// The version to disable, derived from the one pairing this install knows.
///
/// Never from ordering. "The oldest ENABLED version" is right only when the
/// rollover is being finished in the obvious direction, and it is exactly
/// wrong when the operator is keeping the older certificate — which is what a
/// stage that went in with the wrong key pair leaves you wanting.
fn version_from_record(
    record: &StagedRecord,
    enabled: &[&SecretVersion],
    retain: &str,
) -> Result<String> {
    let stale = || {
        Error::Config(format!(
            "this install's record of the rollover names ESV secret version {}, which is not one \
             of the ENABLED versions ({}) — so it does not describe the state on the tenant and \
             nothing here ties a certificate to a version. Pass --disable-version <n> naming the \
             version to disable; `aic saml rotate status` lists the versions and the certificates.",
            record.version,
            enabled
                .iter()
                .map(|version| version.version.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    };

    if record.sha256 == retain {
        // Keeping what was staged: the version that goes is the other one.
        let mut others = enabled
            .iter()
            .filter(|version| version.version != record.version);
        match (others.next(), others.next()) {
            (Some(only), None) => Ok(only.version.clone()),
            _ => Err(stale()),
        }
    } else if enabled
        .iter()
        .any(|version| version.version == record.version)
    {
        // Keeping what was published before the stage: the certificate to
        // stop publishing is the staged one, and the record names its version.
        Ok(record.version.clone())
    } else {
        Err(stale())
    }
}

/// No fingerprint here is tied to a version, so no version may be chosen.
fn no_pairing(state: &RotationState) -> Error {
    Error::Config(format!(
        "this install has no usable record of staging a rollover for {} ({}), so nothing ties \
         either published certificate to an ESV secret version — values are write-only and AM \
         publishes no <ds:KeyName>. Disabling a version retires whichever certificate it holds, \
         and picking by age would be a guess, so say both: --retain <sha256> for the \
         certificate to keep and --disable-version <n> for the version to disable. \
         `aic saml rotate status` lists the versions and the fingerprints.",
        state.entity_id,
        role_descriptor(state.role)
    ))
}

pub fn authorize_complete(dry_run: bool, _exclusive: ExclusivityProof) -> Decision<CompletePermit> {
    if dry_run {
        Decision::Preview
    } else {
        Decision::Send(CompletePermit {
            _minted_by_authorize_complete: (),
        })
    }
}

pub fn authorize_init(dry_run: bool, _exclusive: ExclusivityProof) -> Decision<InitPermit> {
    if dry_run {
        Decision::Preview
    } else {
        Decision::Send(InitPermit {
            _minted_by_authorize_init: (),
        })
    }
}

fn not_ready(verb: &str, state: &RotationState, phase: &Phase) -> Error {
    let detail = match phase {
        Phase::Unconfigured => format!(
            "{} ({}) has no secretIdIdentifier, so it signs with the realm-wide default \
             certificate and there is no per-entity key to roll. `aic saml rotate init` sets \
             the identifier, creates the ESV secret and maps the label.",
            state.entity_id,
            role_descriptor(state.role)
        ),
        Phase::Unmapped => format!(
            "the label {} is not mapped to any ESV secret, so this role still resolves the \
             realm default. `aic saml rotate init` finishes the setup.",
            state.label().unwrap_or_default()
        ),
        Phase::Dangling { alias } => format!(
            "the label {} maps to ESV secret {alias}, which does not exist — AM resolves \
             nothing through it. Recreate the secret or re-point the mapping.",
            state.label().unwrap_or_default()
        ),
        Phase::Unusable { reason } => reason.clone(),
        Phase::Inconsistent { detail } => detail.clone(),
        Phase::Settled | Phase::Staged => "unreachable".to_string(),
    };
    Error::Config(format!("cannot {verb} a rollover: {detail}"))
}

/// Has the export settled on exactly these fingerprints?
///
/// A **set** comparison, deliberately. The count is right for the wrong set
/// in the case that matters most — a completion that disabled the new version
/// instead of the old one leaves exactly one certificate published, and it is
/// the one the rollover existed to replace.
pub fn settled_on(published: &BTreeSet<String>, expected: &BTreeSet<String>) -> bool {
    published == expected
}

/// What an export that has not settled is missing or still carrying.
pub fn settlement_gap(
    published: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    (
        expected.difference(published).cloned().collect(),
        published.difference(expected).cloned().collect(),
    )
}

/// Permission to close a rollover: `--force` was supplied, or the operator
/// confirmed at a terminal.
///
/// Split out the way [`crate::saml::spec::delete_ok`] and
/// [`crate::cli::prod_write_ok`] are: a rule left inline can only be tested by
/// a test that restates it, and "a caller stopped applying it" is exactly the
/// defect that shape catches.
pub fn complete_ok(forced: bool, plan: &CompletePlan, state: &RotationState) -> Result<()> {
    if forced {
        return Ok(());
    }
    let signer = if plan.moves_signer {
        format!(
            " And version {} is the newest ENABLED version — the one to treat as signing — so \
             this also moves signing back to {}: a peer that does not trust {} starts rejecting \
             this role's signatures.",
            plan.disable_version, plan.retain, plan.retain
        )
    } else {
        String::new()
    };
    Err(Error::Config(format!(
        "would disable version {} of {} and stop publishing a certificate {} ({}) currently \
         advertises. That copy is what a peer which refreshes metadata is catching up from, and \
         once it is gone a peer still pinned to it has nothing left to move to — it will go on \
         rejecting signatures with no published certificate that would fix it.{signer} Confirm \
         at a terminal, or pass --force.",
        plan.disable_version,
        plan.secret_id,
        state.entity_id,
        role_descriptor(state.role)
    )))
}

/// Permission to point a role at its own ESV secret: `--force` was supplied,
/// or the operator confirmed at a terminal.
///
/// The third of [`stage_ok`] and [`complete_ok`], because `init` is a
/// certificate change too. Whichever of its steps runs last completes the
/// chain identifier → label → mapping → secret, and from then the role
/// resolves the ESV secret instead of the realm default. **Mapping a label
/// replaces the default certificate; it does not add to it** (measured
/// 2026-09-16, `docs/api/06-saml.md`), so unlike `stage` there is not even a
/// two-certificate export to catch up from: the old certificate stops being
/// published at the same moment. Which certificate then *signs* was not
/// measured for this transition; the same conservative assumption as
/// `stage` applies. Only a plan with nothing to do (`is_noop`) skips this,
/// and that plan never reaches here.
pub fn init_ok(forced: bool, plan: &InitPlan, state: &RotationState) -> Result<()> {
    if forced {
        return Ok(());
    }
    Err(Error::Config(format!(
        "would point {} ({}) at ESV secret {}, which replaces the certificate it publishes — \
         {} now — with {}, and is treated as cutting signing over to it at once. There is no \
         two-certificate export afterwards: mapping a label replaces the default certificate \
         rather than adding to it, so a peer that does not already trust the new one starts \
         rejecting this role's signatures. Confirm at a terminal, or pass --force.",
        state.entity_id,
        role_descriptor(state.role),
        plan.secret_id,
        init_current(state),
        init_incoming(plan),
    )))
}

/// What a role publishes before `init`, for the confirmation.
pub fn init_current(state: &RotationState) -> String {
    let published = state.published_fingerprints();
    if published.is_empty() {
        "nothing".to_string()
    } else {
        published.into_iter().collect::<Vec<_>>().join(", ")
    }
}

/// The certificate `init` brings in, or why it cannot be named.
///
/// Only a secret this run **creates** has a known certificate. An adopted one
/// was never read — values are write-only — so naming the supplied key pair's
/// fingerprint there would name a certificate nothing wrote.
pub fn init_incoming(plan: &InitPlan) -> String {
    match (plan.create_secret, plan.certificate.as_deref()) {
        (true, Some(sha)) => format!("certificate {sha}"),
        _ => format!(
            "whatever certificate {} holds (it already exists, so its value was never seen here)",
            plan.secret_id
        ),
    }
}

/// The one-line reading of a phase, for a human.
pub fn phase_summary(phase: &Phase) -> String {
    match phase {
        Phase::Unconfigured => {
            "unconfigured — this role has no secret label of its own, so it signs with the \
             realm-wide default certificate"
                .to_string()
        }
        Phase::Unmapped => {
            "unmapped — the label exists but nothing backs it, so the role still resolves the \
             realm default"
                .to_string()
        }
        Phase::Dangling { alias } => {
            format!("dangling — the label maps to ESV secret {alias}, which does not exist")
        }
        Phase::Unusable { reason } => format!("unusable — {reason}"),
        Phase::Settled => {
            "settled — one certificate published, backed by one ENABLED ESV secret version"
                .to_string()
        }
        Phase::Staged => {
            "staged — two certificates published; treat the newest ENABLED version as the one \
             already signing, and `complete` as what stops publishing the other"
                .to_string()
        }
        Phase::Inconsistent { detail } => format!("inconsistent — {detail}"),
    }
}

/// What the operator would do next, named as the command that does it.
pub fn next_step(state: &RotationState, phase: &Phase) -> String {
    let scope = format!("{} --realm {}", state.entity_id, state.realm);
    match phase {
        Phase::Unconfigured | Phase::Unmapped => {
            format!(
                "aic saml rotate init {scope} --identifier <name> --secret-id esv-<name> --key-file pair.pem"
            )
        }
        Phase::Settled => format!("aic saml rotate stage {scope} --key-file new-pair.pem"),
        Phase::Staged => format!(
            "`aic saml rotate complete {scope}` — the peer needed the new certificate before \
             the stage, not now; `aic saml metadata export {scope}` only lets one that \
             refreshes metadata catch up"
        ),
        Phase::Dangling { .. } | Phase::Unusable { .. } | Phase::Inconsistent { .. } => {
            "nothing automatic — the state above has to be resolved first".to_string()
        }
    }
}

/// What the `init` making an adoption claim actually did.
///
/// One sentence apart, and it is the sentence an operator acts on. After a run
/// that wrote the entity, "nothing has been undone" is what stops them
/// re-running to be safe; after a run that sent nothing at all, the same
/// sentence is a claim about a `PUT` that never happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adoption {
    /// At least one of the three steps was performed this run.
    Applied,
    /// The tenant already showed every step done, so nothing was sent.
    AlreadyDone,
}

impl Adoption {
    fn aftermath(self) -> &'static str {
        match self {
            Self::Applied => "The entity has been written; nothing has been undone.",
            Self::AlreadyDone => {
                "Nothing was sent: the tenant already showed every step of the setup done, which \
                 is exactly why this is a problem rather than a half-finished run."
            }
        }
    }
}

/// What a two-certificate report must say about which one is *in use*.
///
/// Sibling of [`PAIRING_CAVEAT`], and deliberately a different claim: that one
/// says which **version** holds which certificate cannot be read, and this one
/// says what is known about which **certificate** signs — which is less than
/// this constant used to claim. Measured 2026-09-22 (`docs/api/06-saml.md`),
/// five rounds on the SP AuthnRequest-signing path only: the signer was the
/// newest ENABLED version every time, by the first observation, and a rollback
/// round that re-added the *older* certificate as the newest version saw it
/// take over again — which rules out any property of the certificate. What the
/// rounds did **not** separate is recency from export order: the newest
/// ENABLED version was also listed first every time, so "AM picks the newest"
/// and "AM picks whatever is first" fit the evidence equally. The operational
/// consequence is the same either way, which is why the rule is safe to act
/// on and not safe to state as a mechanism.
///
/// It is stated as a rule rather than pinned to one of the fingerprints above
/// it, because naming one would mean reading it off the export's order — the
/// inference this module refuses everywhere else.
pub const SIGNER_RULE: &str = "\
Observed on the SP AuthnRequest-signing path only (2026-09-22, docs/api/06-saml.md): after a version
was added, the signer was the newest ENABLED version by the first observation, no later than 12 s —
and that version was also always listed first in the export, so which of the two AM selects by was
not separated. IdP assertion signing is unmeasured; assume the same. So treat two published
certificates as meaning the cutover has already happened: the newer one is in use, and the older
one is published only so a peer that refreshes metadata can catch up. Retiring that older one — what
`complete` normally does — is not expected to change what signs; `--retain` naming the older
certificate is the exception, and moves signing back to it.";

/// The sentence every two-certificate report carries.
pub const PAIRING_CAVEAT: &str = "\
Which published certificate came from which ESV secret version is not readable: secret values are
write-only and AM emits no <ds:KeyName>. A certificate is tied to a version here only when this
install staged it; otherwise `complete` needs --retain <sha256> naming the one to keep, and
--disable-version <n> naming the version to disable.";

/// The sentence an adopted-secret report carries.
///
/// Sibling of [`PAIRING_CAVEAT`] — the same write-only-values limit, one step
/// earlier in the story. An `init` that reused an existing ESV secret can
/// confirm the role publishes a certificate; it cannot confirm it is the
/// intended one, because it has never seen the value behind the label.
pub const ADOPTION_CAVEAT: &str = "\
The ESV secret already existed, so `init` adopted it rather than writing a key pair — and secret
values are write-only. What is published above was read back from the tenant, but nothing here can
say it is the certificate you meant. Check the fingerprint against the key pair you expect.";

/// What an `init` that adopted an existing ESV secret may claim, given what
/// the export showed afterwards.
///
/// Reporting success here without reading anything was the defect: `expected`
/// was `None` whenever no key pair was written, and the run returned `Ok`
/// having repointed the entity at a label it never checked resolved. A label
/// backed by a secret with no ENABLED version publishes nothing, and that read
/// as done.
///
/// So the claim is narrowed to what a read supports — this role publishes
/// *something* now — and the rest is named as unknown rather than implied. The
/// rule lives here, out of `cli`, so a test drives the real function: a test
/// restating "empty is bad" would have passed against exactly the code that
/// never looked.
///
/// The **idempotent** `init` reports through here too, for the same reason it
/// exists: "the tenant already shows every step done" is a statement about
/// three documents, and a label backed by a secret with no ENABLED version
/// satisfies all three while publishing nothing. That path used to return
/// success in front of the read, so the one configuration that needs telling
/// got "already set up" and exit zero.
pub fn adoption_outcome(
    state: &RotationState,
    secret_id: &str,
    published: &BTreeSet<String>,
    supplied: Option<&str>,
    did: Adoption,
) -> Result<Vec<String>> {
    if published.is_empty() {
        return Err(Error::Config(format!(
            "{} ({}) publishes no signing certificate. ESV secret {secret_id} already existed, \
             so `init` adopted it rather than writing a key pair, and nothing checked it holds \
             one — a secret with no ENABLED version resolves to nothing. \
             `aic esv secret versions {secret_id}` lists what is in it, and \
             `aic saml rotate status {} --realm {}` reads the whole picture back. {}",
            state.entity_id,
            role_descriptor(state.role),
            state.entity_id,
            state.realm,
            did.aftermath()
        )));
    }

    let mut lines = vec![format!(
        "{} ({}) publishes {}",
        state.entity_id,
        role_descriptor(state.role),
        published.iter().cloned().collect::<Vec<_>>().join(", ")
    )];
    // A key pair handed to an `init` that adopted is not written anywhere.
    // Saying so is the difference between an operator who knows their new key
    // is not in service and one who finds out from a peer.
    if let Some(sha) = supplied {
        lines.push(if published.contains(sha) {
            format!(
                "the certificate in the supplied key pair ({sha}) is among them — but it was not \
                 written here: ESV secret {secret_id} already existed"
            )
        } else {
            format!(
                "the certificate in the supplied key pair ({sha}) is NOT among them: ESV secret \
                 {secret_id} already existed, so that pair was not written. \
                 `aic saml rotate stage --key-file …` is what adds a key pair to a secret that \
                 exists."
            )
        });
    }
    lines.push(ADOPTION_CAVEAT.to_string());
    Ok(lines)
}

/// What `aic saml rotate status` prints.
///
/// `consumers` is `None` only where there is nothing to survey — a role with
/// no identifier of its own. Everywhere else the sharing rows are part of the
/// report rather than an extra: the write verbs refuse a shared secret, and
/// `status` is where the operator is sent to find out what they are sharing
/// it with.
pub fn status_lines(
    state: &RotationState,
    phase: &Phase,
    consumers: Option<&Consumers>,
) -> Vec<String> {
    let mut lines = vec![
        format!("entity      {}", state.entity_id),
        format!("location    {}", state.location),
        format!(
            "role        {} ({})",
            state.role.wire(),
            role_descriptor(state.role)
        ),
        format!(
            "identifier  {}",
            state
                .identifier
                .as_deref()
                .unwrap_or("(none — realm defaults)")
        ),
    ];
    if let Some(label) = state.label() {
        lines.push(format!("label       {label}"));
        lines.push(format!(
            "mapping     {}",
            state
                .mapped_alias
                .as_deref()
                .unwrap_or("(unmapped — the label resolves nothing)")
        ));
    }
    if let Some(secret) = state.secret.as_ref() {
        lines.push(format!(
            "secret      {} — encoding {}, placeholders {}, loaded {}, active version {}, loaded version {}",
            secret.id,
            secret.encoding,
            if secret.use_in_placeholders { "on" } else { "off" },
            secret.loaded,
            blank_as_dash(&secret.active_version),
            blank_as_dash(&secret.loaded_version),
        ));
        let mut versions = state.versions.clone();
        versions.sort_by_key(|version| std::cmp::Reverse(version.number()));
        lines.push(format!(
            "versions    {}",
            if versions.is_empty() {
                "(none)".to_string()
            } else {
                versions
                    .iter()
                    .map(|version| format!("{} {}", version.version, version.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
    }

    let published = state.published();
    lines.push(format!(
        "published   {} signing certificate(s) in {}",
        published.len(),
        role_descriptor(state.role)
    ));
    for cert in &published {
        lines.push(format!(
            "              {}  {}",
            cert.sha256,
            attribution(state, cert)
        ));
    }
    if let Some(consumers) = consumers {
        lines.extend(sharing_lines(consumers));
    }
    lines.push(format!("phase       {}", phase_summary(phase)));
    lines.push(format!("next        {}", next_step(state, phase)));
    if published.len() > 1 {
        lines.push(String::new());
        lines.push(SIGNER_RULE.to_string());
        lines.push(String::new());
        lines.push(PAIRING_CAVEAT.to_string());
    }
    if consumers.is_some_and(|consumers| !consumers.exclusive()) {
        lines.push(String::new());
        lines.push(SHARING_CAVEAT.to_string());
    }
    lines
}

/// What this install can say about where one published certificate came from.
///
/// Deliberately says nothing by elimination. Knowing that certificate A is
/// version 2 does **not** make certificate B version 1: a third ENABLED
/// version, or a certificate arriving from a label mapped elsewhere, would
/// both produce exactly this picture.
fn attribution(state: &RotationState, cert: &CertRef) -> String {
    // Through [`usable_pairing`], not through `state.record` directly: a
    // version number this report prints is the number an operator then types
    // into `--disable-version`, so it must clear the same bar `complete` sets
    // for acting on one. A record left by a setup this tenant no longer has
    // names a version whose meaning has moved.
    match usable_pairing(state) {
        Some(record) if record.sha256 == cert.sha256 => format!(
            "ESV secret version {}, staged here {}",
            record.version, record.staged_at
        ),
        _ => "(no local record of which version holds it)".to_string(),
    }
}

fn blank_as_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

/// The same report, for `--json`.
pub fn status_json(state: &RotationState, phase: &Phase, consumers: Option<&Consumers>) -> Value {
    let published: Vec<Value> = state
        .published()
        .into_iter()
        .map(|cert| {
            serde_json::json!({
                "sha256": cert.sha256,
                "keyName": cert.key_name,
                "stagedVersion": usable_pairing(state)
                    .filter(|record| record.sha256 == cert.sha256)
                    .map(|record| record.version.clone()),
            })
        })
        .collect();
    serde_json::json!({
        "entityId": state.entity_id,
        "tenant": state.tenant,
        "realm": state.realm,
        "location": state.location.as_str(),
        "role": state.role.wire(),
        "descriptor": role_descriptor(state.role),
        "secretIdIdentifier": state.identifier,
        "label": state.label(),
        "mappedAlias": state.mapped_alias,
        "secret": state.secret.as_ref().map(|secret| serde_json::json!({
            "id": secret.id,
            "encoding": secret.encoding,
            "useInPlaceholders": secret.use_in_placeholders,
            "loaded": secret.loaded,
            "activeVersion": secret.active_version,
            "loadedVersion": secret.loaded_version,
        })),
        "versions": state.versions.iter().map(|version| serde_json::json!({
            "version": version.version,
            "status": version.status,
            "createDate": version.create_date,
        })).collect::<Vec<_>>(),
        "published": published,
        "sharing": consumers.map(|consumers| serde_json::json!({
            "exclusive": consumers.exclusive(),
            "secretId": consumers.secret_id,
            "others": consumers.others.iter().map(|consumer| serde_json::json!({
                "realm": consumer.realm,
                "entityId": consumer.entity_id,
                "location": consumer.location.as_str(),
                "role": consumer.role.wire(),
                "label": consumer.label,
            })).collect::<Vec<_>>(),
            "unclaimedLabels": consumers.unclaimed_labels.iter().map(|unclaimed| serde_json::json!({
                "realm": unclaimed.realm,
                "label": unclaimed.label,
            })).collect::<Vec<_>>(),
            "realms": consumers.realms,
            "unsurveyedRealms": consumers.unsurveyed,
            "caveat": SHARING_CAVEAT,
        })),
        "phase": phase_key(phase),
        "phaseDetail": phase_summary(phase),
        "caveat": PAIRING_CAVEAT,
    })
}

fn phase_key(phase: &Phase) -> &'static str {
    match phase {
        Phase::Unconfigured => "unconfigured",
        Phase::Unmapped => "unmapped",
        Phase::Dangling { .. } => "dangling",
        Phase::Unusable { .. } => "unusable",
        Phase::Settled => "settled",
        Phase::Staged => "staged",
        Phase::Inconsistent { .. } => "inconsistent",
    }
}

/// What `init` still has to do. Every step is skipped when the tenant already
/// shows it done, which is what makes an interrupted `init` resumable: the
/// three stores *are* the record, and each step is idempotent by inspection
/// rather than by a retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    pub identifier: String,
    pub label: String,
    pub secret_id: String,
    /// `PUT` the entity to point at the identifier.
    pub set_identifier: bool,
    /// Create the `pem`, placeholders-off ESV secret.
    pub create_secret: bool,
    /// Point the signing label at it.
    pub map_label: bool,
    /// The certificate in the supplied key pair, when one was supplied.
    pub certificate: Option<String>,
}

impl InitPlan {
    pub fn is_noop(&self) -> bool {
        !self.set_identifier && !self.create_secret && !self.map_label
    }

    pub fn lines(&self, state: &RotationState) -> Vec<String> {
        let mut lines = vec![format!(
            "plan for {} ({}) in {}/{}",
            state.entity_id,
            role_descriptor(state.role),
            state.tenant,
            state.realm
        )];
        lines.push(step(
            self.set_identifier,
            &format!(
                "PUT the whole entity with secretIdIdentifier {:?} (full replace, no If-Match)",
                self.identifier
            ),
        ));
        // The certificate is named only on a step that writes it. Naming it on
        // an adopted secret reads as "that secret holds this certificate",
        // which is the one thing nothing here has checked.
        lines.push(step(
            self.create_secret,
            &format!(
                "create ESV secret {} — encoding pem, useInPlaceholders false{}",
                self.secret_id,
                match (self.create_secret, self.certificate.as_deref()) {
                    (true, Some(sha)) => format!(", certificate {sha}"),
                    _ => String::new(),
                }
            ),
        ));
        if !self.create_secret && self.certificate.is_some() {
            lines.push(
                "         the supplied key pair is not written: that secret already exists, and \
                 a version is added by `aic saml rotate stage`"
                    .to_string(),
            );
        }
        lines.push(step(
            self.map_label,
            &format!("map {} at {}", self.label, self.secret_id),
        ));
        lines
    }
}

fn step(todo: bool, what: &str) -> String {
    if todo {
        format!("  will   {what}")
    } else {
        format!("  done   {what}")
    }
}

/// Work out the one-time setup, refusing the states that would orphan
/// something or write over someone else's.
///
/// `label_mapping` is the alias currently sitting at the **requested**
/// identifier's signing label — which is not the same read as
/// [`RotationState::mapped_alias`], because that one follows the identifier
/// the entity has now. The difference is the orphan case: a mapping outlives
/// the label that named it, so `…<identifier>.signing` can already exist,
/// pointing at a secret from a rotation someone abandoned
/// (`docs/api/15-secret-mappings.md`).
pub fn plan_init(
    state: &RotationState,
    identifier: &str,
    secret_id: &str,
    label_mapping: Option<&str>,
    existing_secret: Option<&SecretFacts>,
    key: Option<&KeyPair>,
) -> Result<InitPlan> {
    let identifier = validate_identifier(identifier)?.to_string();
    let secret_id = validate_secret_id(secret_id)?.to_string();
    let label = signing_label(&identifier);

    if let Some(current) = state.identifier.as_deref()
        && current != identifier
    {
        return Err(Error::Config(format!(
            "{} ({}) already points at secretIdIdentifier {current:?}. Repointing it would \
                 leave `{}` mapped to whatever it maps to now — changing the identifier does \
                 **not** remove the old mapping, and the labels vanish from the schema enum the \
                 moment the entity stops naming them. Rotate the certificate instead \
                 (`aic saml rotate stage`), or unpick the old label deliberately with \
                 `aic secretmap remove` before repointing.",
            state.entity_id,
            role_descriptor(state.role),
            signing_label(current),
        )));
    }

    match label_mapping {
        Some(alias) if alias == secret_id => {}
        Some(alias) => {
            return Err(Error::Config(format!(
                "the label {label} already maps to ESV secret {alias}, not {secret_id}. That is \
                 either another entity's rotation or an orphan left by one — a mapping survives \
                 the entity that minted its label. Check it with \
                 `aic secretmap list --realm {}` before overwriting it.",
                state.realm
            )));
        }
        None => {}
    }

    let create_secret = match existing_secret {
        Some(secret) => {
            if let Some(reason) = unusable_reason(secret) {
                return Err(Error::Config(reason));
            }
            false
        }
        None => true,
    };
    if create_secret && key.is_none() {
        return Err(Error::Config(format!(
            "ESV secret {secret_id} does not exist yet, so --key-file (or --key-stdin) is \
             required: it is the private key and certificate this role will sign with, \
             concatenated as `cat key.pem cert.pem`"
        )));
    }

    Ok(InitPlan {
        identifier,
        label,
        secret_id,
        set_identifier: state.identifier.is_none(),
        create_secret,
        map_label: label_mapping.is_none(),
        certificate: key.map(|key| key.sha256.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------
    // Fixtures. Every state here is built from the shape the tenant
    // actually returns, because the defects this module exists to catch
    // are all *agreement between documents* — a count that is right for
    // the wrong set, a role attribution that is right for the wrong role.
    // A fixture that flattened them into one struct would test nothing.
    // -----------------------------------------------------------------

    const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const STRANGER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn cert(descriptor: &str, key_use: Option<&str>, sha: &str) -> CertRef {
        CertRef {
            descriptor: descriptor.to_string(),
            key_use: key_use.map(str::to_string),
            key_name: None,
            sha256: sha.to_string(),
        }
    }

    fn version(number: &str, status: &str) -> SecretVersion {
        SecretVersion {
            version: number.to_string(),
            status: status.to_string(),
            create_date: "2026-09-17T00:00:00Z".to_string(),
        }
    }

    fn facts() -> SecretFacts {
        SecretFacts {
            id: "esv-sp-a-signing".to_string(),
            encoding: "pem".to_string(),
            use_in_placeholders: false,
            loaded: true,
            active_version: "1".to_string(),
            loaded_version: "1".to_string(),
        }
    }

    fn record(version: &str, sha: &str) -> StagedRecord {
        StagedRecord {
            tenant: "sandbox".into(),
            realm: "alpha".into(),
            entity_id: "https://sp-a.example.com".into(),
            role: Role::Sp.wire().into(),
            identifier: "spa".into(),
            secret_id: "esv-sp-a-signing".into(),
            version: version.into(),
            sha256: sha.into(),
            staged_at: "2026-09-17T00:00:00Z".into(),
        }
    }

    fn state() -> RotationState {
        RotationState {
            tenant: "sandbox".into(),
            realm: "alpha".into(),
            entity_id: "https://sp-a.example.com".into(),
            location: Location::Remote,
            role: Role::Sp,
            identifier: Some("spa".into()),
            mapped_alias: Some("esv-sp-a-signing".into()),
            secret: Some(facts()),
            versions: vec![version("1", "ENABLED")],
            certs: vec![cert("SPSSODescriptor", Some("signing"), OLD)],
            record: None,
        }
    }

    /// The steady state, one step further on: two ENABLED versions and the
    /// two certificates they publish.
    fn staged() -> RotationState {
        RotationState {
            versions: vec![version("1", "ENABLED"), version("2", "ENABLED")],
            certs: vec![
                cert("SPSSODescriptor", Some("signing"), OLD),
                cert("SPSSODescriptor", Some("signing"), NEW),
            ],
            record: Some(record("2", NEW)),
            ..state()
        }
    }

    /// Nothing in this module reads `value` or `certificate_der` — the
    /// validation that gives them meaning is `pem`'s, and is tested there —
    /// so they are inert stand-ins here. Deliberately **not** PEM-shaped
    /// armour: a literal BEGIN/END pair in a committed file is what the
    /// secret scanners are looking for, and a fixture nothing parses has no
    /// business tripping one.
    fn pair(sha: &str) -> KeyPair {
        KeyPair {
            value: format!("key-pair-for-{sha}"),
            certificate_der: vec![0x30, 0x00],
            sha256: sha.to_string(),
        }
    }

    fn shas<'a>(values: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
        values.into_iter().map(str::to_string).collect()
    }

    /// One row of the realm's secret-label mapping table. AM calls the label
    /// `secretId` and the ESV secret it resolves to the `alias`, which is the
    /// pair the survey is most easily got backwards.
    fn mapping(label: &str, alias: &str) -> Mapping {
        Mapping {
            secret_id: label.to_string(),
            alias: Some(alias.to_string()),
        }
    }

    fn entity(entity_id: &str, roles: &[(Role, &str)]) -> EntityIdentifiers {
        EntityIdentifiers {
            entity_id: entity_id.to_string(),
            location: Location::Remote,
            identifiers: roles
                .iter()
                .map(|(role, identifier)| (*role, (*identifier).to_string()))
                .collect(),
        }
    }

    /// The rotation under test: sp-a's SP role, through `spa`'s signing label.
    fn mine() -> Consumer {
        state().consumer().expect("the fixture names an identifier")
    }

    /// Ours and nobody else's — the state every other survey here is a
    /// departure from.
    fn exclusive_survey() -> Consumers {
        alpha_survey(
            &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
            &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
        )
    }

    fn realm_survey(
        realm: &str,
        mappings: &[Mapping],
        entities: &[EntityIdentifiers],
    ) -> RealmSurvey {
        RealmSurvey {
            realm: realm.to_string(),
            mappings: mappings.to_vec(),
            entities: entities.to_vec(),
        }
    }

    /// A tenant-wide survey whose `bravo` is empty: every single-realm
    /// departure below is made in `alpha`, the rotation's own realm, and the
    /// cross-realm ones are built explicitly.
    fn alpha_survey(mappings: &[Mapping], entities: &[EntityIdentifiers]) -> Consumers {
        survey_consumers(
            &mine(),
            Some("esv-sp-a-signing"),
            &[
                realm_survey("alpha", mappings, entities),
                realm_survey("bravo", &[], &[]),
            ],
        )
    }

    fn proof() -> ExclusivityProof {
        exclusive_ok("stage", &exclusive_survey()).expect("the fixture is exclusive")
    }

    fn message(error: Error) -> String {
        match error {
            Error::Config(text) => text,
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Role scoping. The case the whole `descriptor` field exists for.
    // -----------------------------------------------------------------

    /// Red when `published()` drops `cert.descriptor == descriptor` and
    /// filters on `use` alone.
    #[test]
    fn a_dual_role_entity_publishes_signing_from_both_roles_and_they_are_not_interchangeable() {
        // Both roles sign, neither carries a <ds:KeyName> — AM emits none —
        // so `use` plus fingerprint names two different keys identically.
        // Whole-document counting reports two certificates for a role that
        // has one, which reads as a staged rollover that was never staged.
        let both = RotationState {
            certs: vec![
                cert("IDPSSODescriptor", Some("signing"), OLD),
                cert("SPSSODescriptor", Some("signing"), NEW),
                cert("SPSSODescriptor", Some("encryption"), STRANGER),
            ],
            ..state()
        };
        assert!(both.certs.iter().all(|cert| cert.key_name.is_none()));

        let sp = RotationState {
            role: Role::Sp,
            ..both.clone()
        };
        assert_eq!(sp.published_fingerprints(), shas([NEW]));
        assert_eq!(phase(&sp), Phase::Settled);

        let idp = RotationState {
            role: Role::Idp,
            ..both
        };
        assert_eq!(idp.published_fingerprints(), shas([OLD]));
        assert_eq!(phase(&idp), Phase::Settled);
    }

    /// Red when `choose_role`'s `(_, None)` arm defaults instead of refusing.
    #[test]
    fn choosing_a_role_is_refused_on_a_dual_role_entity_and_settled_on_a_single_role_one() {
        let dual = json!({
            "identityProvider": { "assertionContent": {} },
            "serviceProvider": { "assertionContent": {} },
        });
        let sp_only = json!({ "serviceProvider": { "assertionContent": {} } });
        let roleless = json!({ "entityId": "https://sp-a.example.com" });

        assert!(message(choose_role(&dual, None).unwrap_err()).contains("--role idp or --role sp"));
        assert_eq!(choose_role(&dual, Some(Role::Idp)).unwrap(), Role::Idp);
        assert_eq!(choose_role(&dual, Some(Role::Sp)).unwrap(), Role::Sp);

        assert_eq!(choose_role(&sp_only, None).unwrap(), Role::Sp);
        assert_eq!(choose_role(&sp_only, Some(Role::Sp)).unwrap(), Role::Sp);
        assert!(
            message(choose_role(&sp_only, Some(Role::Idp)).unwrap_err())
                .contains("holds no identityProvider block")
        );

        assert!(message(choose_role(&roleless, None).unwrap_err()).contains("no role blocks"));
    }

    // -----------------------------------------------------------------
    // Sets, not counts.
    // -----------------------------------------------------------------

    /// Red when `settled_on` compares `published.len() == expected.len()`.
    #[test]
    fn a_wrong_fingerprint_set_of_the_right_size_is_not_settled() {
        // This is the shape a `complete` that disabled the *new* version
        // leaves behind: exactly one certificate published, and it is the
        // one the rollover existed to replace. A count check calls it done.
        assert!(!settled_on(&shas([OLD]), &shas([NEW])));
        assert_eq!(
            settlement_gap(&shas([OLD]), &shas([NEW])),
            (vec![NEW.to_string()], vec![OLD.to_string()])
        );

        // And the same at two: the peer's certificate arrived, ours did not.
        assert!(!settled_on(&shas([OLD, STRANGER]), &shas([OLD, NEW])));
        assert_eq!(
            settlement_gap(&shas([OLD, STRANGER]), &shas([OLD, NEW])),
            (vec![NEW.to_string()], vec![STRANGER.to_string()])
        );

        assert!(settled_on(&shas([OLD, NEW]), &shas([NEW, OLD])));
        assert_eq!(
            settlement_gap(&shas([OLD, NEW]), &shas([OLD, NEW])),
            (Vec::new(), Vec::new())
        );
    }

    // -----------------------------------------------------------------
    // Phases.
    // -----------------------------------------------------------------

    /// Red when `unusable_reason` stops checking `use_in_placeholders`, or
    /// stops checking `encoding`.
    #[test]
    fn a_secret_that_cannot_back_a_restart_free_rotation_is_unusable_not_settled() {
        // `useInPlaceholders: true` reads `loaded: false` on every new
        // version until a tenant restart, so the rollover this command
        // reports — add a version, see two certificates — does not happen.
        // Both properties are fixed at create, so the remedy is a new
        // secret, and saying "unusable" beats staging into a silence.
        let placeholders = RotationState {
            secret: Some(SecretFacts {
                use_in_placeholders: true,
                loaded: false,
                loaded_version: String::new(),
                ..facts()
            }),
            ..state()
        };
        let Phase::Unusable { reason } = phase(&placeholders) else {
            panic!(
                "placeholders-on must be unusable, got {:?}",
                phase(&placeholders)
            );
        };
        assert!(reason.contains("useInPlaceholders"), "{reason}");
        assert!(reason.contains("--no-placeholders"), "{reason}");
        assert!(
            message(plan_stage(&placeholders, &pair(NEW)).unwrap_err()).contains("cannot stage"),
        );

        let generic = RotationState {
            secret: Some(SecretFacts {
                encoding: "generic".into(),
                ..facts()
            }),
            ..state()
        };
        let Phase::Unusable { reason } = phase(&generic) else {
            panic!("a non-pem secret must be unusable");
        };
        assert!(reason.contains("not `pem`"), "{reason}");
    }

    /// Red when any arm of `phase` is reordered past another — the four
    /// setup phases are a chain, and each has a different remedy.
    #[test]
    fn the_phase_of_a_state_is_the_first_thing_missing_from_it() {
        let cases: Vec<(&str, RotationState, Phase)> = vec![
            (
                "no identifier at all",
                RotationState {
                    identifier: None,
                    mapped_alias: None,
                    secret: None,
                    versions: Vec::new(),
                    ..state()
                },
                Phase::Unconfigured,
            ),
            (
                "an identifier whose label nothing maps",
                RotationState {
                    mapped_alias: None,
                    secret: None,
                    versions: Vec::new(),
                    ..state()
                },
                Phase::Unmapped,
            ),
            (
                "a mapping pointing at a secret that is not there",
                RotationState {
                    secret: None,
                    versions: Vec::new(),
                    ..state()
                },
                Phase::Dangling {
                    alias: "esv-sp-a-signing".into(),
                },
            ),
            ("one version, one certificate", state(), Phase::Settled),
            ("two versions, two certificates", staged(), Phase::Staged),
        ];
        for (what, state, expected) in cases {
            assert_eq!(phase(&state), expected, "{what}");
        }
    }

    /// Red when `phase`'s `(enabled, published)` match gains a `_ => Settled`
    /// arm, or when `enabled_versions` counts DISABLED ones.
    #[test]
    fn versions_and_certificates_that_disagree_are_inconsistent_rather_than_guessed_at() {
        // A DISABLED version publishes nothing, so two versions and one
        // certificate is the *normal* post-complete state, not a fault.
        let completed = RotationState {
            versions: vec![version("1", "DISABLED"), version("2", "ENABLED")],
            ..state()
        };
        assert_eq!(phase(&completed), Phase::Settled);

        // Two ENABLED versions but one published certificate: the export has
        // not caught up, or something else resolves this label. Either way
        // `stage` and `complete` both refuse.
        let lagging = RotationState {
            versions: vec![version("1", "ENABLED"), version("2", "ENABLED")],
            ..state()
        };
        let Phase::Inconsistent { detail } = phase(&lagging) else {
            panic!("2 enabled / 1 published must be inconsistent");
        };
        assert!(detail.contains("propagated"), "{detail}");
        assert!(message(plan_stage(&lagging, &pair(NEW)).unwrap_err()).contains("cannot stage"));
        assert!(
            message(plan_complete(&lagging, Some(OLD), None).unwrap_err())
                .contains("cannot complete")
        );

        // Equal but wrong: three of each is not a rollover.
        let three = RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "ENABLED"),
                version("3", "ENABLED"),
            ],
            certs: vec![
                cert("SPSSODescriptor", Some("signing"), OLD),
                cert("SPSSODescriptor", Some("signing"), NEW),
                cert("SPSSODescriptor", Some("signing"), STRANGER),
            ],
            ..state()
        };
        let Phase::Inconsistent { detail } = phase(&three) else {
            panic!("3 and 3 is not a rollover");
        };
        assert!(detail.contains("a rollover is two"), "{detail}");
    }

    // -----------------------------------------------------------------
    // stage
    // -----------------------------------------------------------------

    /// Red when `plan_stage` stops checking `published.contains(&incoming)`,
    /// or when `expected` is built from the incoming certificate alone.
    #[test]
    fn staging_adds_a_certificate_and_refuses_to_add_the_one_already_there() {
        let plan = plan_stage(&state(), &pair(NEW)).unwrap();
        assert_eq!(plan.secret_id, "esv-sp-a-signing");
        assert_eq!(plan.retained, OLD);
        assert_eq!(plan.incoming, NEW);
        // Both, not just the new one: the certificate that was in service
        // stays *published* — that is what lets a peer which refreshes
        // metadata catch up, and what makes the state readable afterwards.
        // It does not stay in service; see the cutover tests below.
        assert_eq!(plan.expected, shas([OLD, NEW]));

        let same = message(plan_stage(&state(), &pair(OLD)).unwrap_err());
        assert!(same.contains("rotates nothing"), "{same}");

        let again = message(plan_stage(&staged(), &pair(STRANGER)).unwrap_err());
        assert!(again.contains("already staged"), "{again}");
        assert!(again.contains("rotate complete"), "{again}");
    }

    /// The same single-certificate rotation, on an IdP role. The claims about
    /// *when* signing moves differ by role — the SP path was measured and the
    /// IdP path was not — so every cutover message is checked on both.
    fn idp_state() -> RotationState {
        RotationState {
            role: Role::Idp,
            certs: vec![cert("IDPSSODescriptor", Some("signing"), OLD)],
            ..state()
        }
    }

    /// Red when the plan `stage` prints stops saying that it is the cutover,
    /// stops naming the complete way back, or states the IdP cutover as a
    /// measurement.
    ///
    /// The load-bearing facts: the change should be treated as *now*, the peer
    /// needed the certificate *beforehand*, disabling the version just added
    /// is refused, and restoration needs the old private key and then two
    /// disables — an operator who has only the first two still has no way back,
    /// and one given "add it again" alone is left with three ENABLED versions
    /// `rotate` will not finish.
    #[test]
    fn the_stage_plan_says_it_cuts_signing_over_and_names_the_way_back() {
        let state = state();
        let plan = plan_stage(&state, &pair(NEW)).unwrap();
        assert_eq!(plan.retained_version, "1");
        let text = stage_plan_lines(&state, &plan).join("\n");

        assert!(text.contains("treat this as the signing cutover"), "{text}");
        assert!(text.contains(NEW), "{text}");
        assert!(
            text.contains("stop here unless the peer already holds"),
            "the trust step belongs before this command, not after it: {text}"
        );
        // The old certificate's fate, stated as it is: still published, no
        // longer expected to be in use.
        assert!(text.contains("stays published"), "{text}");
        assert!(
            text.contains("expected to stop being the certificate in use"),
            "{text}"
        );
        // The measured bound, not the unmeasured "next request".
        assert!(text.contains("no later than 12 s"), "{text}");
        assert!(!text.contains("next request"), "{text}");

        // Emergency signer restoration, stated completely.
        assert!(text.contains("Emergency signer restoration"), "{text}");
        assert!(text.contains("old private key"), "{text}");
        assert!(
            text.contains("aic esv secret add-version esv-sp-a-signing"),
            "{text}"
        );
        assert!(
            text.contains("Cannot disable latest secret version"),
            "{text}"
        );
        assert!(
            text.contains(OLD),
            "restoration needs the certificate it puts back: {text}"
        );
        assert!(text.contains("will not finish"), "{text}");
        assert!(
            text.contains("aic esv secret disable esv-sp-a-signing 1"),
            "the version that was in service is one of the two to disable: {text}"
        );
        assert!(
            text.contains("aic esv secret disable esv-sp-a-signing <the version this adds>"),
            "{text}"
        );

        // The IdP role gets the same warning and a different evidence line:
        // its signing path was never measured.
        let idp = idp_state();
        let idp_plan = plan_stage(&idp, &pair(NEW)).unwrap();
        let idp_text = stage_plan_lines(&idp, &idp_plan).join("\n");
        assert!(
            idp_text.contains("treat this as the signing cutover"),
            "{idp_text}"
        );
        assert!(
            idp_text.contains("IdP assertion signing is unmeasured"),
            "{idp_text}"
        );
        assert!(!idp_text.contains("measured on this path"), "{idp_text}");
        assert!(!text.contains("IdP assertion signing"), "{text}");
    }

    /// Red when `stage` stops reporting the cutover after the write, reports
    /// signing as something this run observed, or names restoration without
    /// the version numbers it needs.
    ///
    /// Separate from the plan lines because they are read at different
    /// moments: the plan is read before a decision, this after one, and an
    /// operator who confirmed past the warning finds out here.
    #[test]
    fn the_stage_outcome_repeats_the_cutover_with_the_version_in_hand() {
        let state = state();
        let plan = plan_stage(&state, &pair(NEW)).unwrap();
        let text = stage_outcome_lines(&state, &plan, "2").join("\n");

        assert!(
            text.contains(&format!("publishes {NEW} as version 2")),
            "{text}"
        );
        assert!(
            text.contains("treat it as the certificate signing now"),
            "{text}"
        );
        // This run read publication, not signing; it must not say "signs".
        assert!(!text.contains("now signs with"), "{text}");
        assert!(
            text.contains("aic esv secret add-version esv-sp-a-signing"),
            "{text}"
        );
        assert!(
            text.contains("aic esv secret disable esv-sp-a-signing 2")
                && text.contains("aic esv secret disable esv-sp-a-signing 1"),
            "both superseded versions, by number: {text}"
        );

        let idp = idp_state();
        let idp_plan = plan_stage(&idp, &pair(NEW)).unwrap();
        let idp_text = stage_outcome_lines(&idp, &idp_plan, "2").join("\n");
        assert!(
            idp_text.contains("IdP assertion signing is unmeasured"),
            "{idp_text}"
        );
    }

    /// Red when `stage` can reach its write unconfirmed — the sibling of
    /// `closing_the_window_is_refused_until_it_is_confirmed`, and driven
    /// through the same kind of function.
    #[test]
    fn cutting_signing_over_is_refused_until_it_is_confirmed() {
        let state = state();
        let plan = plan_stage(&state, &pair(NEW)).unwrap();
        assert!(stage_ok(true, &plan, &state).is_ok());
        let refusal = message(stage_ok(false, &plan, &state).unwrap_err());
        assert!(
            refusal.contains(&format!("would add certificate {NEW}")),
            "the prompt's subject is the incoming certificate: {refusal}"
        );
        assert!(refusal.contains("no un-stage"), "{refusal}");
        assert!(refusal.contains("old private key"), "{refusal}");
        assert!(refusal.contains("--force"), "{refusal}");
    }

    /// Red when `init` can complete the label chain unconfirmed, or when its
    /// confirmation names a certificate nothing wrote.
    ///
    /// `init` replaces the published certificate outright — mapping a label
    /// replaces the default rather than adding to it — so it is a cutover
    /// with no catch-up export at all. The discriminating case is adoption:
    /// a key pair supplied to an `init` that adopts an existing secret is not
    /// written, so naming its fingerprint as the incoming certificate would be
    /// a claim about a value this run never saw.
    #[test]
    fn pointing_a_role_at_its_own_secret_is_refused_until_it_is_confirmed() {
        let unconfigured = RotationState {
            identifier: None,
            mapped_alias: None,
            secret: None,
            versions: vec![],
            certs: vec![cert("SPSSODescriptor", Some("signing"), STRANGER)],
            ..state()
        };
        let creating = plan_init(
            &unconfigured,
            "spa",
            "esv-sp-a-signing",
            None,
            None,
            Some(&pair(NEW)),
        )
        .unwrap();
        assert!(init_ok(true, &creating, &unconfigured).is_ok());
        let refusal = message(init_ok(false, &creating, &unconfigured).unwrap_err());
        assert!(refusal.contains(&format!("certificate {NEW}")), "{refusal}");
        assert!(refusal.contains(STRANGER), "what it replaces: {refusal}");
        assert!(refusal.contains("no two-certificate export"), "{refusal}");
        assert!(refusal.contains("--force"), "{refusal}");

        let adopting = plan_init(
            &unconfigured,
            "spa",
            "esv-sp-a-signing",
            None,
            Some(&facts()),
            Some(&pair(NEW)),
        )
        .unwrap();
        let adopted = message(init_ok(false, &adopting, &unconfigured).unwrap_err());
        assert!(!adopted.contains(NEW), "{adopted}");
        assert!(adopted.contains("never seen here"), "{adopted}");
    }

    /// Red when a two-certificate `status` stops saying which certificate to
    /// treat as in use — and red if it starts saying so when there is only
    /// one, or if it claims more than the evidence separated.
    ///
    /// The control is the second half: [`SIGNER_RULE`] is a statement about a
    /// rollover in progress, and a settled role that carried it would be
    /// telling an operator about a cutover that is not happening.
    #[test]
    fn a_two_certificate_status_says_which_certificate_is_in_use() {
        let staged = staged();
        let staged_text = status_lines(&staged, &phase(&staged), None).join("\n");
        assert!(staged_text.contains(SIGNER_RULE), "{staged_text}");
        assert!(
            staged_text.contains("treat the newest ENABLED version as the one already signing"),
            "the phase line carries it too, because that is the line read first: {staged_text}"
        );
        // And it is not the same claim as the pairing caveat: one says which
        // certificate signs, the other says which version holds it.
        assert!(staged_text.contains(PAIRING_CAVEAT), "{staged_text}");
        // What the measurement did not separate, and what it did not cover.
        assert!(SIGNER_RULE.contains("not separated"), "{SIGNER_RULE}");
        assert!(SIGNER_RULE.contains("IdP assertion signing is unmeasured"));
        assert!(!SIGNER_RULE.contains("next request"), "{SIGNER_RULE}");

        let settled = state();
        let settled_text = status_lines(&settled, &phase(&settled), None).join("\n");
        assert!(!settled_text.contains(SIGNER_RULE), "{settled_text}");
    }

    // -----------------------------------------------------------------
    // complete
    // -----------------------------------------------------------------

    /// Red when `plan_complete` picks the version by ordering rather than
    /// from the record, and red when `VersionChoice` stops being reported.
    #[test]
    fn completing_takes_the_version_from_the_record_and_not_from_the_numbering() {
        // Keeping what was staged: the version that goes is the *other*
        // ENABLED one, found by version identity. Numbers here are wide
        // enough that a string comparison would order them backwards, which
        // is what an ordering-derived implementation would trip on.
        let wide = RotationState {
            versions: vec![version("9", "ENABLED"), version("10", "ENABLED")],
            record: Some(record("10", NEW)),
            ..staged()
        };
        let plan = plan_complete(&wide, None, None).unwrap();
        assert_eq!(plan.disable_version, "9");
        assert_eq!(plan.chosen_by, VersionChoice::Recorded);
        assert_eq!(plan.retain, NEW);
        assert_eq!(plan.expected_drop.as_deref(), Some(OLD));
    }

    /// Red when `--retain` selects a version again — the P1 this was written
    /// for.
    ///
    /// `--retain` proves only that a fingerprint is currently published. It
    /// says nothing about which ESV secret version holds it, and "disable the
    /// oldest ENABLED version" supplies that half by guessing.
    #[test]
    fn retaining_a_fingerprint_does_not_choose_a_version_and_will_not_pretend_to() {
        let no_record = RotationState {
            record: None,
            ..staged()
        };

        // The discriminating input: keep the OLDER certificate, with nothing
        // tying a fingerprint to a version. "Disable the oldest ENABLED
        // version" disables version 1 — the one holding the very certificate
        // it was told to keep — and the operator finds out from the
        // post-write export check, after it has stopped being published.
        let refusal = message(plan_complete(&no_record, Some(OLD), None).unwrap_err());
        assert!(refusal.contains("--disable-version"), "{refusal}");
        assert!(refusal.contains("write-only"), "{refusal}");

        // And the agreeing case is refused for exactly the same reason. This
        // one used to plan, which is the trap: from in here it is
        // indistinguishable from the case above.
        let agreeing = message(plan_complete(&no_record, Some(NEW), None).unwrap_err());
        assert!(agreeing.contains("--disable-version"), "{agreeing}");

        // Named, it plans — and reports the pairing as the operator's claim,
        // with no certificate named by elimination.
        let plan = plan_complete(&no_record, Some(NEW), Some("1")).unwrap();
        assert_eq!(plan.disable_version, "1");
        assert_eq!(plan.chosen_by, VersionChoice::Named);
        assert_eq!(plan.retain, NEW);
        assert_eq!(plan.expected_drop, None);

        // Keeping OLD means retiring what version 2 publishes, and version 2
        // is the latest — which AIC will not disable. Named honestly, the
        // refusal is AIC's rule rather than a silent substitution.
        let latest = message(plan_complete(&no_record, Some(OLD), Some("2")).unwrap_err());
        assert!(latest.contains("newest version"), "{latest}");

        // A version that publishes nothing is not a version to disable.
        let absent = message(plan_complete(&no_record, Some(NEW), Some("7")).unwrap_err());
        assert!(absent.contains("not an ENABLED version"), "{absent}");

        // Case-insensitively, since a fingerprint gets pasted from openssl.
        assert_eq!(
            plan_complete(&no_record, Some(&NEW.to_uppercase()), Some("1"))
                .unwrap()
                .retain,
            NEW
        );

        // A fingerprint this role does not publish is refused, not disabled
        // hopefully: the two certificates are the only ones in play.
        let wrong = message(plan_complete(&no_record, Some(STRANGER), Some("1")).unwrap_err());
        assert!(wrong.contains("is not one of the 2"), "{wrong}");
    }

    /// Red when the version is derived from the ordering even though the
    /// record says which version holds which certificate.
    #[test]
    fn keeping_the_older_certificate_disables_the_version_holding_the_newer_one() {
        // A stage that went in with the wrong key pair: the operator wants
        // the certificate that was already in service, not the one just
        // staged as version 2. "Disable the oldest ENABLED version" would
        // disable version 1 — the one holding what is being kept — and the
        // record was sitting right there saying so.
        let with_spare = RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "ENABLED"),
                version("3", "DISABLED"),
            ],
            ..staged() // records version 2 as NEW
        };
        let plan = plan_complete(&with_spare, Some(OLD), None).unwrap();
        assert_eq!(plan.disable_version, "2");
        assert_eq!(plan.chosen_by, VersionChoice::Recorded);
        assert_eq!(plan.retain, OLD);
        // Direct from the record, not by elimination: version 2 is the one
        // being disabled and the record names the certificate it holds.
        assert_eq!(plan.expected_drop.as_deref(), Some(NEW));
        // And disabling version 2 — the newest ENABLED one — moves the
        // signer, which the usual completion does not. Both are said before
        // the confirmation.
        assert!(plan.moves_signer);
        let refusal = message(complete_ok(false, &plan, &with_spare).unwrap_err());
        assert!(
            refusal.contains(&format!("moves signing back to {OLD}")),
            "{refusal}"
        );
    }

    /// Red when a stale record is read as a pairing.
    #[test]
    fn a_record_naming_a_certificate_nobody_publishes_is_not_a_pairing() {
        // Left over from an earlier rollover: it names a version and a
        // fingerprint, and neither describes what is on the tenant now.
        // Deriving a version from it is ordering wearing a record's clothes.
        let stale = RotationState {
            record: Some(record("1", STRANGER)),
            ..staged()
        };
        let refusal = message(plan_complete(&stale, Some(NEW), None).unwrap_err());
        assert!(refusal.contains("--disable-version"), "{refusal}");
        assert!(message(plan_complete(&stale, None, None).unwrap_err()).contains("--retain"));
    }

    /// Red when `--disable-version` is allowed to name a version the record
    /// contradicts — in **either** direction.
    #[test]
    fn a_named_version_may_corroborate_the_record_and_may_not_overrule_it() {
        // A spare DISABLED version, so that AIC's latest-version rule is not
        // what does the refusing here: both ENABLED versions are disable-able
        // as far as the tenant is concerned, and the record is the only thing
        // left saying which one may go.
        let with_spare = RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "ENABLED"),
                version("3", "DISABLED"),
            ],
            ..staged() // records version 2 as NEW
        };

        // The discriminating input, and the half the earlier test missed:
        // keep the certificate the record does NOT name. The record proves
        // version 2 holds NEW, so keeping OLD means disabling 2 — and
        // `--disable-version 1` names the version holding OLD itself. The
        // "contradiction" guard this replaced only fired when the record
        // named the certificate being kept, so it let this through, the plan
        // reported the pairing as the operator's claim, and the post-write
        // export check found out afterwards.
        let overruled = message(plan_complete(&with_spare, Some(OLD), Some("1")).unwrap_err());
        assert!(overruled.contains("contradicts"), "{overruled}");
        assert!(
            overruled.contains("holding the certificate you asked to keep"),
            "{overruled}"
        );

        // Corroborating it plans, and stays `Recorded`: the version is still
        // derived from the record, and calling it the operator's claim would
        // understate what is known.
        let agreed = plan_complete(&with_spare, Some(OLD), Some("2")).unwrap();
        assert_eq!(agreed.disable_version, "2");
        assert_eq!(agreed.chosen_by, VersionChoice::Recorded);
        assert_eq!(agreed.expected_drop.as_deref(), Some(NEW));

        // The other direction, which the old guard did catch: the inverted
        // rollover, where this install staged NEW as version 1 because
        // version 2 was already there. Keeping NEW and naming version 1
        // retires the certificate being kept.
        let inverted = RotationState {
            record: Some(record("1", NEW)),
            ..with_spare.clone()
        };
        let forced = message(plan_complete(&inverted, Some(NEW), Some("1")).unwrap_err());
        assert!(forced.contains("contradicts"), "{forced}");
        assert!(forced.contains("would retire"), "{forced}");

        // And derived, the same inverted state picks the other ENABLED
        // version rather than the one the record ties to what is kept.
        let derived = plan_complete(&inverted, Some(NEW), None).unwrap();
        assert_eq!(derived.disable_version, "2");
        assert_eq!(derived.chosen_by, VersionChoice::Recorded);

        // With no spare, AIC's own rule is what stops the inverted rollover:
        // the other ENABLED version is 2, which is also the newest.
        let newest = RotationState {
            record: Some(record("1", NEW)),
            ..staged()
        };
        let refusal = message(plan_complete(&newest, None, None).unwrap_err());
        assert!(refusal.contains("newest version"), "{refusal}");
    }

    /// Red when a still-published fingerprint is the whole test for a record.
    #[test]
    fn a_record_that_describes_a_different_setup_is_not_a_pairing_however_familiar() {
        // The recreation case: same entity id, same certificate, re-created
        // setup. Each of these three states publishes the fingerprint the
        // record names, so the fingerprint-only test called every one of them
        // a pairing and read "version 2" out of a secret that has never held
        // what the record says it holds.
        let recreated_under_another_identifier = RotationState {
            identifier: Some("spa2".into()),
            ..staged()
        };
        let backed_by_another_secret = RotationState {
            secret: Some(SecretFacts {
                id: "esv-sp-a-signing-v2".into(),
                ..facts()
            }),
            mapped_alias: Some("esv-sp-a-signing-v2".into()),
            ..staged()
        };
        let numbering_restarted = RotationState {
            // The record names version 2; this secret only goes up to 1 and 4.
            versions: vec![version("1", "ENABLED"), version("4", "ENABLED")],
            ..staged()
        };

        for state in [
            &recreated_under_another_identifier,
            &backed_by_another_secret,
            &numbering_restarted,
        ] {
            assert!(usable_pairing(state).is_none());
            // With no pairing, `complete` needs both halves — and says so
            // rather than deriving a version from a record about something
            // else.
            let refusal = message(plan_complete(state, Some(NEW), None).unwrap_err());
            assert!(refusal.contains("--disable-version"), "{refusal}");
            assert!(message(plan_complete(state, None, None).unwrap_err()).contains("--retain"));
        }

        // The positive control: the untouched state differs from all three in
        // exactly the field under test, and is a pairing.
        assert_eq!(
            usable_pairing(&staged())
                .expect("the record still describes this")
                .version,
            "2"
        );
        assert_eq!(
            plan_complete(&staged(), Some(NEW), None)
                .unwrap()
                .disable_version,
            "1"
        );
    }

    /// Red when `usable_pairing` tests that the recorded version *exists*
    /// rather than that it is ENABLED.
    ///
    /// A DISABLED version publishes nothing, so a record naming one ties no
    /// published certificate to anything. `plan_complete` survives the weaker
    /// test because it derives only from ENABLED versions; `status` does not,
    /// and the version it prints is the one an operator types into
    /// `--disable-version`.
    #[test]
    fn a_disabled_version_is_not_a_pairing_even_when_its_certificate_is_published() {
        // The discriminating input: the same certificate material in two
        // versions, one DISABLED and one ENABLED. Every weaker test passes —
        // the record's fingerprint is published, its identifier and secret
        // still match, and its version number is right there in the list —
        // and the version it names is the one publishing none of it.
        let duplicated = RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "DISABLED"),
                version("3", "ENABLED"),
            ],
            record: Some(record("2", NEW)),
            ..staged() // publishes OLD and NEW
        };
        assert_eq!(phase(&duplicated), Phase::Staged);
        assert!(usable_pairing(&duplicated).is_none());

        let lines = status_lines(&duplicated, &phase(&duplicated), None).join("\n");
        assert_eq!(lines.matches("no local record").count(), 2, "{lines}");
        assert!(!lines.contains("ESV secret version 2"), "{lines}");

        // And `complete` asks for both halves rather than deriving from a
        // record that describes a version publishing nothing.
        let refusal = message(plan_complete(&duplicated, Some(NEW), None).unwrap_err());
        assert!(refusal.contains("--disable-version"), "{refusal}");

        // The positive control, differing in exactly the field under test:
        // the record names version 3, which is ENABLED and holds the same
        // certificate. Now there is a pairing, and it is reported.
        let enabled = RotationState {
            record: Some(record("3", NEW)),
            ..duplicated
        };
        assert_eq!(
            usable_pairing(&enabled)
                .expect("version 3 is ENABLED")
                .version,
            "3"
        );
        assert!(
            status_lines(&enabled, &phase(&enabled), None)
                .join("\n")
                .contains("ESV secret version 3"),
            "the ENABLED pairing is reported"
        );
    }

    /// Red when `complete_ok` returns `Ok(())` regardless of `forced`.
    #[test]
    fn closing_the_window_is_refused_until_it_is_confirmed() {
        let state = staged();
        let plan = plan_complete(&state, None, None).unwrap();
        assert!(complete_ok(true, &plan, &state).is_ok());
        let refusal = message(complete_ok(false, &plan, &state).unwrap_err());
        assert!(refusal.contains("would disable version 1"), "{refusal}");
        assert!(refusal.contains("rejecting signatures"), "{refusal}");
        assert!(refusal.contains("--force"), "{refusal}");
        // The usual completion retires the version that is not signing.
        assert!(!plan.moves_signer);
        assert!(!refusal.contains("moves signing back"), "{refusal}");
    }

    // -----------------------------------------------------------------
    // Whose secret is this, and does this role still resolve it.
    // -----------------------------------------------------------------

    /// Red when the survey finds a second consumer through only one of the
    /// two channels — the release-blocking defect this was written for.
    ///
    /// `stage` and `complete` mutate the ESV secret globally while every
    /// check around them covers one entity, so a secret backing two providers
    /// makes a completion an outage for the one nobody looked at. Each
    /// departure below is a state a plausible narrower implementation calls
    /// exclusive.
    #[test]
    fn a_secret_backing_a_second_provider_is_found_through_either_channel() {
        // The positive control: one label, one entity, one role.
        assert!(exclusive_survey().exclusive());
        assert!(exclusive_survey().others.is_empty());

        // Channel 1 — a second entity carrying the *same* identifier. It
        // needs no mapping of its own: the label is the same label.
        // Discriminating against a survey that only reads the mapping table.
        let shared_identifier = alpha_survey(
            &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
            &[
                entity("https://sp-a.example.com", &[(Role::Sp, "spa")]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spa")]),
            ],
        );
        assert_eq!(shared_identifier.others.len(), 1);
        assert_eq!(
            shared_identifier.others[0].entity_id,
            "https://sp-b.example.com"
        );

        // Channel 2 — a second label mapped onto the same ESV secret. The
        // identifiers differ, so an identifier-only survey calls this one
        // exclusive.
        let shared_secret = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping(&signing_label("spb"), "esv-sp-a-signing"),
            ],
            &[
                entity("https://sp-a.example.com", &[(Role::Sp, "spa")]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spb")]),
            ],
        );
        assert_eq!(shared_secret.others.len(), 1);
        assert_eq!(shared_secret.others[0].label, signing_label("spb"));

        // ...and its control, which is the same tenant one mapping different:
        // the second label points somewhere else, so it is not this secret's
        // business at all.
        let neighbour = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping(&signing_label("spb"), "esv-sp-b-signing"),
            ],
            &[
                entity("https://sp-a.example.com", &[(Role::Sp, "spa")]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spb")]),
            ],
        );
        assert!(neighbour.exclusive(), "{neighbour:?}");

        // One entity, both roles, one identifier: an entity-keyed survey
        // calls this exclusive, and disabling a version retires the IdP
        // role's certificate as surely as the SP role's.
        let both_roles = alpha_survey(
            &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
            &[entity(
                "https://sp-a.example.com",
                &[(Role::Idp, "spa"), (Role::Sp, "spa")],
            )],
        );
        assert_eq!(both_roles.others.len(), 1);
        assert_eq!(both_roles.others[0].role, Role::Idp);

        // Same entity, same role, the encryption label off the same
        // identifier mapped onto the same secret: an (entity, role) survey
        // calls this exclusive, and the rollover would roll a key no
        // `<KeyDescriptor use="signing">` ever showed.
        let also_encryption = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping(
                    "am.applications.federation.entity.providers.saml2.spa.encryption",
                    "esv-sp-a-signing",
                ),
            ],
            &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
        );
        assert_eq!(also_encryption.others.len(), 1);
        assert!(also_encryption.others[0].label.ends_with(".encryption"));

        // A label mapped onto the secret that nobody names resolves nothing
        // today and still says the secret is not this rollover's alone.
        let orphan = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping(&signing_label("spb"), "esv-sp-a-signing"),
            ],
            &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
        );
        assert!(orphan.others.is_empty());
        assert_eq!(
            orphan.unclaimed_labels,
            vec![UnclaimedLabel {
                realm: "alpha".into(),
                label: signing_label("spb")
            }]
        );

        // Including one that is not SAML's at all — another AM subsystem
        // holding the same key. It cannot be named as a provider, so it is
        // reported as a label rather than dropped for not parsing.
        let foreign = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping("am.services.oauth2.stateless.signing", "esv-sp-a-signing"),
            ],
            &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
        );
        assert_eq!(
            foreign.unclaimed_labels,
            vec![UnclaimedLabel {
                realm: "alpha".into(),
                label: "am.services.oauth2.stateless.signing".into()
            }]
        );

        // The state a fresh `init` surveys from: nothing is mapped and the
        // entity does not name the identifier yet, because writing it is what
        // `init` is about. Counting our own label as an orphan here would
        // refuse every first-time setup.
        let fresh = alpha_survey(&[], &[entity("https://sp-a.example.com", &[])]);
        assert!(fresh.exclusive(), "{fresh:?}");

        // But an `init` adopting an identifier a second entity already
        // carries is refused, with nothing mapped anywhere — the sharing does
        // not exist in the mapping table yet, which is exactly why `init` is
        // the cheapest place to catch it.
        let adopting_a_busy_identifier = alpha_survey(
            &[],
            &[
                entity("https://sp-a.example.com", &[]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spa")]),
            ],
        );
        assert_eq!(adopting_a_busy_identifier.others.len(), 1);
    }

    /// Red when a shared secret is refused without saying what it is shared
    /// with, or when `status` reports a rollover it will not perform as
    /// though nothing were in the way.
    #[test]
    fn a_shared_secret_is_refused_by_name_and_named_again_by_status() {
        let shared = alpha_survey(
            &[
                mapping(&signing_label("spa"), "esv-sp-a-signing"),
                mapping(&signing_label("spc"), "esv-sp-a-signing"),
            ],
            &[
                entity("https://sp-a.example.com", &[(Role::Sp, "spa")]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spa")]),
            ],
        );

        // The refusal *is* the inventory: "shared" is not something an
        // operator can act on, and the remedy differs per consumer.
        let refusal = message(exclusive_ok("complete", &shared).unwrap_err());
        assert!(refusal.contains("cannot complete a rollover"), "{refusal}");
        assert!(refusal.contains("https://sp-b.example.com"), "{refusal}");
        assert!(refusal.contains("SPSSODescriptor"), "{refusal}");
        assert!(refusal.contains(&signing_label("spc")), "{refusal}");
        assert!(refusal.contains("Nothing has been sent"), "{refusal}");
        assert!(refusal.contains("esv secret add-version"), "{refusal}");
        assert!(refusal.contains(SHARING_CAVEAT), "{refusal}");

        // And the write cannot be reached without the answer: every
        // `authorize_*` takes the proof, which only `exclusive_ok` mints. The
        // compile-time half is the type checker's; what a test can pin is
        // that an exclusive survey is what yields one.
        assert!(exclusive_ok("stage", &exclusive_survey()).is_ok());

        // `status` is where every one of those refusals sends the operator,
        // so it names the same set and carries the same caveat.
        let lines = status_lines(&state(), &phase(&state()), Some(&shared)).join("\n");
        assert!(lines.contains("SHARED"), "{lines}");
        assert!(lines.contains("https://sp-b.example.com"), "{lines}");
        assert!(lines.contains(&signing_label("spc")), "{lines}");
        assert!(lines.contains(SHARING_CAVEAT), "{lines}");

        let alone = status_lines(&state(), &phase(&state()), Some(&exclusive_survey())).join("\n");
        assert!(
            alone.contains("nothing else in realms alpha, bravo resolves esv-sp-a-signing"),
            "{alone}"
        );
        assert!(!alone.contains(SHARING_CAVEAT), "{alone}");

        let json = status_json(&state(), &phase(&state()), Some(&shared));
        assert_eq!(json["sharing"]["exclusive"], false);
        assert_eq!(
            json["sharing"]["others"][0]["entityId"],
            "https://sp-b.example.com"
        );
        assert_eq!(
            json["sharing"]["unclaimedLabels"][0]["label"],
            signing_label("spc")
        );
        assert_eq!(json["sharing"]["unclaimedLabels"][0]["realm"], "alpha");
        // A role with no identifier of its own has nothing to survey, and
        // says so rather than claiming exclusivity it never checked.
        assert!(status_json(&state(), &phase(&state()), None)["sharing"].is_null());
    }

    /// Red when the survey reads only the rotation's own realm, or when it
    /// treats an identifier as tenant-wide rather than per realm.
    ///
    /// The ESV secret is tenant-global and the mapping table is not, so a
    /// consumer in `bravo` is cut over by an `alpha` rotation's write. The two
    /// controls are what a plausible over-reach gets wrong: the same
    /// identifier in `bravo` is **not** a consumer unless `bravo` maps its
    /// label onto this secret, because identifiers are a per-realm namespace.
    #[test]
    fn a_consumer_in_the_other_realm_is_found_and_a_namesake_there_is_not() {
        let alpha = realm_survey(
            "alpha",
            &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
            &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
        );

        // A bravo entity whose label bravo maps onto the same secret.
        let across = survey_consumers(
            &mine(),
            Some("esv-sp-a-signing"),
            &[
                alpha.clone(),
                realm_survey(
                    "bravo",
                    &[mapping(&signing_label("spb"), "esv-sp-a-signing")],
                    &[entity("https://sp-b.example.com", &[(Role::Sp, "spb")])],
                ),
            ],
        );
        assert_eq!(across.others.len(), 1, "{across:?}");
        assert_eq!(across.others[0].realm, "bravo");
        let refusal = message(exclusive_ok("stage", &across).unwrap_err());
        assert!(refusal.contains("in realm bravo"), "{refusal}");

        // The same entity id in bravo is a different consumer from mine.
        let same_id_elsewhere = survey_consumers(
            &mine(),
            Some("esv-sp-a-signing"),
            &[
                alpha.clone(),
                realm_survey(
                    "bravo",
                    &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
                    &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
                ),
            ],
        );
        assert_eq!(same_id_elsewhere.others.len(), 1);
        assert_eq!(same_id_elsewhere.others[0].realm, "bravo");

        // Control: bravo carries the same identifier, and maps its label
        // somewhere else — or nowhere. Not this secret's business.
        for bravo_mappings in [
            vec![mapping(&signing_label("spa"), "esv-sp-b-signing")],
            vec![],
        ] {
            let namesake = survey_consumers(
                &mine(),
                Some("esv-sp-a-signing"),
                &[
                    alpha.clone(),
                    realm_survey(
                        "bravo",
                        &bravo_mappings,
                        &[entity("https://sp-z.example.com", &[(Role::Sp, "spa")])],
                    ),
                ],
            );
            assert!(namesake.exclusive(), "{namesake:?}");
        }
    }

    /// Red when a survey that skipped a realm can still mint a proof.
    ///
    /// Coverage is checked in the pure function rather than trusted of the
    /// caller, so "surveyed one realm" cannot be mistaken for "surveyed the
    /// tenant" — which is exactly the proof the old per-realm survey minted.
    #[test]
    fn a_survey_missing_a_realm_is_not_exclusive() {
        let partial = survey_consumers(
            &mine(),
            Some("esv-sp-a-signing"),
            &[realm_survey(
                "alpha",
                &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
                &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
            )],
        );
        assert!(partial.others.is_empty() && partial.unclaimed_labels.is_empty());
        assert_eq!(partial.unsurveyed, vec!["bravo".to_string()]);
        assert!(!partial.exclusive());
        let refusal = message(exclusive_ok("complete", &partial).unwrap_err());
        assert!(refusal.contains("not surveyed"), "{refusal}");
        assert!(refusal.contains("realm bravo"), "{refusal}");

        // The positive control is the full survey the fixtures use.
        assert!(exclusive_survey().unsurveyed.is_empty());
        assert!(exclusive_survey().exclusive());
    }

    /// What the pre-write recheck reads when nothing has moved since
    /// `staged()` was planned.
    fn unmoved_recheck() -> RolloverRecheck {
        let staged = staged();
        RolloverRecheck {
            identifier: staged.identifier.clone(),
            alias: staged.mapped_alias.clone(),
            versions: staged.versions.clone(),
            published: staged.published_fingerprints(),
            survey: vec![
                realm_survey(
                    "alpha",
                    &[mapping(&signing_label("spa"), "esv-sp-a-signing")],
                    &[entity("https://sp-a.example.com", &[(Role::Sp, "spa")])],
                ),
                realm_survey("bravo", &[], &[]),
            ],
        }
    }

    /// Red when the pre-write recheck stops re-surveying — the defect the
    /// round-4 review found.
    ///
    /// The discriminating case: the plan's survey was exclusive (the plan
    /// could not have been authorized otherwise), and a consumer appeared
    /// while the operator was reading the prompt. Every other input is
    /// unchanged, so a recheck of only the target, the versions and the
    /// published set — the previous implementation — passes it and writes.
    #[test]
    fn a_consumer_added_after_the_plan_stops_the_write() {
        let staged = staged();
        assert!(exclusive_ok("complete", &exclusive_survey()).is_ok());
        assert!(
            rollover_pre_write_ok("complete", "esv-sp-a-signing", &staged, &unmoved_recheck())
                .is_ok()
        );

        // The same inputs the old recheck compared, all unchanged…
        let mut arrived = unmoved_recheck();
        assert!(
            rollover_target_ok(
                "complete",
                "esv-sp-a-signing",
                arrived.identifier.as_deref(),
                arrived.alias.as_deref()
            )
            .is_ok()
                && rollover_write_ok(
                    "complete",
                    "esv-sp-a-signing",
                    &rollover_inputs(&staged),
                    &rollover_inputs_from(&arrived.versions, arrived.published.clone()),
                )
                .is_ok(),
            "the old recheck's inputs have not moved"
        );
        // …and a second consumer in the other realm, mapped in the gap.
        arrived.survey[1] = realm_survey(
            "bravo",
            &[mapping(&signing_label("spb"), "esv-sp-a-signing")],
            &[entity("https://sp-b.example.com", &[(Role::Sp, "spb")])],
        );
        let refusal = message(
            rollover_pre_write_ok("complete", "esv-sp-a-signing", &staged, &arrived).unwrap_err(),
        );
        assert!(refusal.contains("https://sp-b.example.com"), "{refusal}");
        assert!(refusal.contains("Nothing has been sent"), "{refusal}");

        // `mine` comes from the *fresh* identifier: an identifier moved to a
        // label backed by the same secret is allowed by the target check, and
        // a survey keyed on the planned label would count this role itself as
        // a second consumer and refuse a safe write.
        let moved_same_secret = RolloverRecheck {
            identifier: Some("spa2".into()),
            survey: vec![
                realm_survey(
                    "alpha",
                    &[mapping(&signing_label("spa2"), "esv-sp-a-signing")],
                    &[entity("https://sp-a.example.com", &[(Role::Sp, "spa2")])],
                ),
                realm_survey("bravo", &[], &[]),
            ],
            ..unmoved_recheck()
        };
        assert!(
            rollover_pre_write_ok("stage", "esv-sp-a-signing", &staged, &moved_same_secret).is_ok()
        );

        // The target check still comes first and still refuses on its own.
        let repointed = RolloverRecheck {
            alias: Some("esv-sp-b-signing".into()),
            ..unmoved_recheck()
        };
        let refusal = message(
            rollover_pre_write_ok("complete", "esv-sp-a-signing", &staged, &repointed).unwrap_err(),
        );
        assert!(refusal.contains("esv-sp-b-signing"), "{refusal}");
    }

    /// Red when `init`'s pre-write recheck stops re-surveying — the same
    /// discriminating case as `a_consumer_added_after_the_plan_stops_the_write`,
    /// for the third writing verb.
    ///
    /// The plan's survey was exclusive (it could not have been authorized
    /// otherwise), and a second entity started carrying the same identifier
    /// while the operator read the prompt. The identifier and mapping checks
    /// the per-step writers make both still pass, so a recheck without the
    /// survey writes. The refusal comes from the one recheck `apply_init` runs
    /// before its first step, so nothing has been written when it fires.
    #[test]
    fn a_consumer_added_after_the_init_plan_stops_the_setup() {
        let unconfigured = RotationState {
            identifier: None,
            mapped_alias: None,
            secret: None,
            versions: vec![],
            ..state()
        };
        let plan = plan_init(
            &unconfigured,
            "spa",
            "esv-sp-a-signing",
            None,
            None,
            Some(&pair(NEW)),
        )
        .unwrap();
        let fresh_entity = json!({"serviceProvider": {}});
        let unmoved = || InitRecheck {
            entity: fresh_entity.clone(),
            mapping: None,
            survey: vec![
                realm_survey("alpha", &[], &[entity("https://sp-a.example.com", &[])]),
                realm_survey("bravo", &[], &[]),
            ],
        };
        // Plan time: exclusive, and the recheck agrees when nothing moved.
        assert!(
            exclusive_ok(
                "init",
                &alpha_survey(&[], &[entity("https://sp-a.example.com", &[])])
            )
            .is_ok()
        );
        assert!(init_pre_write_ok(&unconfigured, &plan, None, &unmoved()).is_ok());

        // A second entity now names the identifier. The per-step checks are
        // unmoved…
        let mut arrived = unmoved();
        assert!(
            identifier_write_ok(None, &arrived.entity, Role::Sp).is_ok()
                && mapping_write_ok(&plan.label, &plan.secret_id, None, None).is_ok(),
            "the per-step writers' own checks have not moved"
        );
        arrived.survey[0] = realm_survey(
            "alpha",
            &[],
            &[
                entity("https://sp-a.example.com", &[]),
                entity("https://sp-b.example.com", &[(Role::Sp, "spa")]),
            ],
        );
        let refusal = message(init_pre_write_ok(&unconfigured, &plan, None, &arrived).unwrap_err());
        assert!(refusal.contains("cannot init a rollover"), "{refusal}");
        assert!(refusal.contains("https://sp-b.example.com"), "{refusal}");
        assert!(refusal.contains("Nothing has been sent"), "{refusal}");

        // The target half still refuses first, on its own: the label was
        // mapped in the gap.
        let mapped = InitRecheck {
            mapping: Some("esv-other".into()),
            ..unmoved()
        };
        let refusal = message(init_pre_write_ok(&unconfigured, &plan, None, &mapped).unwrap_err());
        assert!(refusal.contains("esv-other"), "{refusal}");
    }

    /// Red when the pre-write recheck lets the identifier or the mapping move
    /// while comparing only versions and fingerprints.
    ///
    /// Certificate equality cannot establish the relationship: metadata
    /// carries no secret and no version, so a role repointed in the gap
    /// publishes a set that still matches the plan while the secret about to
    /// be written is somebody else's.
    #[test]
    fn a_role_that_stopped_resolving_the_planned_secret_is_not_written_to() {
        assert!(
            rollover_target_ok(
                "complete",
                "esv-sp-a-signing",
                Some("spa"),
                Some("esv-sp-a-signing")
            )
            .is_ok()
        );

        // The discriminating case, and the reason this is a check on the
        // *resolution* rather than on the identifier: the identifier moved,
        // and its new label maps to the same secret. The key this rollover is
        // about is still this role's key, and refusing would leave a rollover
        // that cannot be completed — two certificates published.
        assert!(
            rollover_target_ok(
                "complete",
                "esv-sp-a-signing",
                Some("spa2"),
                Some("esv-sp-a-signing")
            )
            .is_ok()
        );

        // Repointed at a label backed by a different secret: the versions and
        // the published fingerprints can be exactly as planned and the write
        // would still land on a secret this role no longer signs with.
        let moved = message(
            rollover_target_ok(
                "stage",
                "esv-sp-a-signing",
                Some("spa2"),
                Some("esv-sp-b-signing"),
            )
            .unwrap_err(),
        );
        assert!(moved.contains(&signing_label("spa2")), "{moved}");
        assert!(moved.contains("esv-sp-b-signing"), "{moved}");
        assert!(moved.contains("Nothing has been sent"), "{moved}");

        // The mapping removed under it — the label resolves the realm default
        // now, and the secret is nobody's.
        let unmapped = message(
            rollover_target_ok("complete", "esv-sp-a-signing", Some("spa"), None).unwrap_err(),
        );
        assert!(unmapped.contains("maps to no ESV secret"), "{unmapped}");

        // The identifier removed under it: the role is back on the realm
        // default certificate, and nothing it publishes comes from here.
        let unconfigured =
            message(rollover_target_ok("complete", "esv-sp-a-signing", None, None).unwrap_err());
        assert!(
            unconfigured.contains("realm-wide default"),
            "{unconfigured}"
        );
    }

    // -----------------------------------------------------------------
    // dry run
    // -----------------------------------------------------------------

    /// Red when any `authorize_*` mints its permit for `dry_run == true`.
    ///
    /// The compile-time half of this rule cannot be asserted from a test —
    /// that a preview holds no permit is what the type checker enforces, and
    /// `ops`' writers take a `&Permit` they cannot be called without. What a
    /// test can pin is the other half: that the authorizing function is the
    /// one thing deciding, and that it says no.
    #[test]
    fn a_dry_run_is_handed_no_permit_by_any_of_the_three_authorizers() {
        assert!(matches!(authorize_init(true, proof()), Decision::Preview));
        assert!(matches!(authorize_stage(true, proof()), Decision::Preview));
        assert!(matches!(
            authorize_complete(true, proof()),
            Decision::Preview
        ));
        assert!(matches!(authorize_init(false, proof()), Decision::Send(_)));
        assert!(matches!(authorize_stage(false, proof()), Decision::Send(_)));
        assert!(matches!(
            authorize_complete(false, proof()),
            Decision::Send(_)
        ));
    }

    // -----------------------------------------------------------------
    // The entity write.
    // -----------------------------------------------------------------

    /// Red when `set_secret_identifier` builds a document instead of cloning
    /// the one it was given, and red when it writes the hosted path into a
    /// remote entity.
    #[test]
    fn setting_the_identifier_changes_one_leaf_of_the_document_it_was_given() {
        // An entity `PUT` is a full replace with no `If-Match`, so every
        // byte not being changed has to survive verbatim — including the
        // keys this code has never heard of.
        let before = json!({
            "entityId": "https://sp-a.example.com",
            "_rev": "17",
            "serviceProvider": {
                "assertionContent": {
                    "secrets": {},
                    // A live entity carries the groups as `{}` rather than as
                    // values, which is why the writer creates only what is
                    // missing.
                    "signingAndEncryption": { "secretIdAndAlgorithms": {}, "other": 1 },
                },
                "services": { "metaAlias": "/alpha/sp-a" },
                "somethingNobodyHereKnowsAbout": [1, 2, 3],
            },
            "identityProvider": { "assertionContent": { "secrets": {} } },
        });

        let remote = set_secret_identifier(&before, Location::Remote, Role::Sp, "spa").unwrap();
        assert_eq!(
            remote.pointer("/serviceProvider/assertionContent/secrets/secretIdIdentifier"),
            Some(&json!("spa"))
        );
        assert_eq!(
            entity_write_differences(&before, &remote),
            vec!["/serviceProvider/assertionContent/secrets/secretIdIdentifier"]
        );
        // The other role is untouched: a dual-role entity rotates one at a time.
        assert_eq!(remote["identityProvider"], before["identityProvider"]);

        // Hosted keeps the same string in a different group, and a writer
        // has to pick — a reader may take either.
        let hosted = set_secret_identifier(&before, Location::Hosted, Role::Sp, "spa").unwrap();
        assert_eq!(
            entity_write_differences(&before, &hosted),
            vec![
                "/serviceProvider/assertionContent/signingAndEncryption/secretIdAndAlgorithms/secretIdIdentifier"
            ]
        );
        assert_eq!(
            current_identifier(&hosted, Role::Sp).as_deref(),
            Some("spa")
        );
        assert_eq!(
            current_identifier(&remote, Role::Sp).as_deref(),
            Some("spa")
        );
        assert_eq!(current_identifier(&before, Role::Sp), None);
        assert_eq!(current_identifier(&remote, Role::Idp), None);

        // Missing groups are created; a non-group in the way is refused
        // rather than overwritten.
        let bare = json!({ "serviceProvider": {} });
        let filled = set_secret_identifier(&bare, Location::Remote, Role::Sp, "spa").unwrap();
        assert_eq!(
            current_identifier(&filled, Role::Sp).as_deref(),
            Some("spa")
        );
        // A group that had to be created is reported at the group, because
        // that is the highest path where the two documents stop agreeing.
        assert_eq!(
            entity_write_differences(&bare, &filled),
            vec!["/serviceProvider/assertionContent"]
        );
        let hostile = json!({ "serviceProvider": { "assertionContent": "not a group" } });
        assert!(
            message(set_secret_identifier(&hostile, Location::Remote, Role::Sp, "x").unwrap_err())
                .contains("refusing to rewrite it")
        );
        assert!(
            message(set_secret_identifier(&bare, Location::Remote, Role::Idp, "x").unwrap_err())
                .contains("no identityProvider block")
        );
    }

    /// Red when `set_identifier` writes whatever its fresh read returned
    /// instead of checking the leaf the plan was authorized against.
    #[test]
    fn a_concurrent_identifier_change_is_reported_rather_than_overwritten() {
        let unconfigured = json!({
            "entityId": "https://sp-a.example.com",
            "serviceProvider": { "assertionContent": { "secrets": {} } },
        });
        // Planned from an unconfigured role and still unconfigured: the only
        // case that may write.
        assert!(identifier_write_ok(None, &unconfigured, Role::Sp).is_ok());

        // The discriminating input. `plan_init` decided to set the identifier
        // *because* the role had none; by the time the write re-reads the
        // entity, someone else has set one. The wrong implementation — re-read,
        // change the leaf, `PUT` — cannot tell this document from the one
        // above, and replaces "theirs" with no diff and no message.
        let taken = json!({
            "entityId": "https://sp-a.example.com",
            "serviceProvider": {
                "assertionContent": { "secrets": { "secretIdIdentifier": "theirs" } },
            },
        });
        let refusal = message(identifier_write_ok(None, &taken, Role::Sp).unwrap_err());
        assert!(refusal.contains("\"theirs\""), "{refusal}");
        assert!(refusal.contains("unset"), "{refusal}");
        assert!(refusal.contains("Nothing has been sent"), "{refusal}");

        // Including when the concurrent change is the value this run wanted:
        // the plan said unconfigured, and it is not unconfigured now. Nothing
        // is lost by refusing — a re-run skips the step.
        assert!(
            message(identifier_write_ok(None, &taken, Role::Sp).unwrap_err()).contains("theirs")
        );
        let same = json!({
            "serviceProvider": { "assertionContent": { "secrets": { "secretIdIdentifier": "spa" } } },
        });
        assert!(identifier_write_ok(None, &same, Role::Sp).is_err());
        assert!(identifier_write_ok(Some("spa"), &same, Role::Sp).is_ok());

        // Role-scoped: the other role's identifier is not this one's, and a
        // check that read either would refuse a legitimate write on every
        // dual-role entity that had already configured its IdP.
        let other_role = json!({
            "identityProvider": {
                "assertionContent": { "secrets": { "secretIdIdentifier": "theirs" } },
            },
            "serviceProvider": { "assertionContent": { "secrets": {} } },
        });
        assert!(identifier_write_ok(None, &other_role, Role::Sp).is_ok());
        assert!(identifier_write_ok(None, &other_role, Role::Idp).is_err());
    }

    /// Red when the sibling writers act on a plan whose inputs have moved.
    ///
    /// Round one gave the entity `PUT` a freshness guard and left the other
    /// three writes, whose gaps are longer: `complete` plans, waits through an
    /// interactive confirmation, and then disables a version **by number**.
    #[test]
    fn a_rollover_that_moved_while_the_plan_waited_is_refused_rather_than_acted_on() {
        let planned = rollover_inputs(&staged());
        assert_eq!(planned.enabled, ["1", "2"]);
        assert!(rollover_write_ok("complete", "esv-sp-a-signing", &planned, &planned).is_ok());

        // The discriminating input, and the reason this is a set-and-identity
        // comparison rather than a count: one version disabled and another
        // enabled in the gap, and one certificate replaced by another. Every
        // total is where it was. The planned version number still resolves,
        // and it no longer holds the certificate the plan named.
        let swapped = rollover_inputs(&RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "DISABLED"),
                version("3", "ENABLED"),
            ],
            certs: vec![
                cert("SPSSODescriptor", Some("signing"), OLD),
                cert("SPSSODescriptor", Some("signing"), STRANGER),
            ],
            ..staged()
        });
        assert_eq!(swapped.enabled.len(), planned.enabled.len());
        assert_eq!(swapped.published.len(), planned.published.len());
        let refusal = message(
            rollover_write_ok("complete", "esv-sp-a-signing", &planned, &swapped).unwrap_err(),
        );
        assert!(refusal.contains("Nothing has been sent"), "{refusal}");
        assert!(
            refusal.contains("ENABLED versions of esv-sp-a-signing"),
            "{refusal}"
        );
        assert!(refusal.contains("1, 2"), "{refusal}");
        assert!(refusal.contains("1, 3"), "{refusal}");
        assert!(refusal.contains(STRANGER), "{refusal}");

        // Each half fires on its own, so a change to either input is enough.
        let another_version = rollover_inputs(&RotationState {
            versions: vec![
                version("1", "ENABLED"),
                version("2", "ENABLED"),
                version("3", "ENABLED"),
            ],
            ..staged()
        });
        assert!(
            rollover_write_ok("stage", "esv-sp-a-signing", &planned, &another_version).is_err()
        );
        let another_cert = rollover_inputs(&RotationState {
            certs: vec![
                cert("SPSSODescriptor", Some("signing"), OLD),
                cert("SPSSODescriptor", Some("signing"), NEW),
                cert("SPSSODescriptor", Some("signing"), STRANGER),
            ],
            ..staged()
        });
        assert!(rollover_write_ok("stage", "esv-sp-a-signing", &planned, &another_cert).is_err());

        // And the inputs are the rollover's, not the whole picture: this
        // function is about whether the rollover moved, so a document it does
        // not decide from moving is not its refusal to make.
        //
        // That is **not** the same as the identifier and the mapping being
        // unchecked, which is what this comment used to imply and what the
        // code used to do. A role repointed at a label backed by a different
        // secret is refused — by `rollover_target_ok`, which runs first in
        // `ops::recheck` and asks the question certificate equality cannot:
        // does this role still resolve the secret about to be written.
        let elsewhere = rollover_inputs(&RotationState {
            mapped_alias: Some("esv-something-else".into()),
            identifier: Some("spa2".into()),
            record: None,
            ..staged()
        });
        assert!(rollover_write_ok("complete", "esv-sp-a-signing", &planned, &elsewhere).is_ok());
        assert!(
            rollover_target_ok(
                "complete",
                "esv-sp-a-signing",
                Some("spa2"),
                Some("esv-something-else")
            )
            .is_err()
        );
    }

    /// Red when `map_label` writes over a mapping made while it was planning.
    #[test]
    fn a_label_mapped_while_init_was_planning_is_not_overwritten() {
        let label = signing_label("spa");
        // `plan_init` only sets `map_label` when the label is free, so free is
        // the only state a write may proceed from.
        assert!(mapping_write_ok(&label, "esv-sp-a-signing", None, None).is_ok());

        // Another entity's rotation, or an orphan somebody is unpicking. A
        // `PUT` updates as happily as it creates, so this is silent.
        let taken = message(
            mapping_write_ok(&label, "esv-sp-a-signing", None, Some("esv-theirs")).unwrap_err(),
        );
        assert!(taken.contains("esv-theirs"), "{taken}");
        assert!(taken.contains("Nothing has been sent"), "{taken}");

        // Including the agreeing case, as for the identifier: the plan said
        // this label was free, it is not now, and re-running is what should
        // decide from the state that exists — `plan_init` then skips the step.
        assert!(
            mapping_write_ok(&label, "esv-sp-a-signing", None, Some("esv-sp-a-signing")).is_err()
        );
    }

    /// Red when `entity_write_differences` compares with `==` and reports a
    /// verdict, and red when it stops stripping `_rev`.
    #[test]
    fn a_put_that_lost_a_role_block_is_reported_as_the_paths_it_lost() {
        // `{"entityId": "<same>"}` answers 200 and deletes the whole role
        // block. A 200 therefore proves nothing, and the post-read has to
        // name what went missing — "mismatch" sends nobody anywhere.
        let intended = json!({
            "entityId": "https://sp-a.example.com",
            "_rev": "17",
            "serviceProvider": {
                "assertionContent": { "secrets": { "secretIdIdentifier": "spa" } },
                "services": { "metaAlias": "/alpha/sp-a" },
            },
        });
        let collapsed = json!({ "entityId": "https://sp-a.example.com", "_rev": "18" });
        // Named at the block, not at every leaf inside it: the operator has
        // to read this and act on it, and "the role block is gone" is the
        // sentence, not forty paths.
        assert_eq!(
            entity_write_differences(&intended, &collapsed),
            vec!["/serviceProvider"]
        );

        // But a leaf dropped from a block that survived is named as that
        // leaf — both sides stay objects the whole way down to it, so the
        // recursion reaches it. This is the discriminating pair: a diff that
        // only ever reported the top-level key would pass the case above and
        // fail this one.
        let mut lost_one = intended.clone();
        lost_one["serviceProvider"]["services"]
            .as_object_mut()
            .unwrap()
            .remove("metaAlias");
        assert_eq!(
            entity_write_differences(&intended, &lost_one),
            vec!["/serviceProvider/services/metaAlias"]
        );

        // `_rev` changes on every write, by design, and is not a difference.
        let mut same = intended.clone();
        same["_rev"] = json!("99");
        assert!(entity_write_differences(&intended, &same).is_empty());

        // A scalar replaced by a group is one difference at the group, not
        // a silent equality.
        let reshaped = json!({ "entityId": { "nested": true } });
        assert_eq!(
            entity_write_differences(&json!({ "entityId": "x" }), &reshaped),
            vec!["/entityId"]
        );
        assert_eq!(
            entity_write_differences(&json!("a"), &json!("b")),
            vec!["/"]
        );
    }

    // -----------------------------------------------------------------
    // Identifiers and secret ids.
    // -----------------------------------------------------------------

    /// Red when `validate_identifier` accepts `.`, `/`, `-` or `_`.
    #[test]
    fn an_identifier_that_would_address_a_different_label_is_refused() {
        // The identifier is spliced into a dotted label *and* into a URL
        // path. A dot renames the label family and a slash escapes the
        // collection, and neither fails loudly: the mapping `PUT` at the
        // wrong label is a perfectly good 201 nothing will ever read.
        //
        // `-` and `_` read as harmless and are the discriminating cases:
        // they cannot address a different label, so the local rationale
        // above does not reach them, and a check written from that
        // rationale alone accepts both. AM refuses them at the entity
        // `PUT` with `400 Invalid character present in Secret ID
        // Identifier` (measured 2026-09-18), so accepting them here just
        // relocates the failure to a message that names no flag.
        assert_eq!(validate_identifier("spa1").unwrap(), "spa1");
        assert_eq!(validate_identifier("SpRotate1").unwrap(), "SpRotate1");
        for bad in [
            "sp.a", "sp/a", "sp a", "sp:a", "", "  ", " spa", "spa ", "sp-a", "sp_a",
        ] {
            assert!(
                validate_identifier(bad).is_err(),
                "identifier {bad:?} should be refused"
            );
        }
        assert_eq!(
            signing_label("spa"),
            "am.applications.federation.entity.providers.saml2.spa.signing"
        );
    }

    /// Red when `validate_secret_id` drops the `esv-` requirement or the
    /// lowercase rule.
    #[test]
    fn a_secret_id_aic_would_reject_is_refused_by_name_rather_than_by_relayed_regex() {
        assert_eq!(validate_secret_id("esv-sp-a_1").unwrap(), "esv-sp-a_1");
        for bad in ["spa", "sp-a", "esv-", "esv-SP", "esv-sp.a", "ESV-sp"] {
            assert!(
                validate_secret_id(bad).is_err(),
                "secret id {bad:?} should be refused"
            );
        }
        assert!(validate_secret_id(&format!("esv-{}", "a".repeat(124))).is_ok());
        assert!(validate_secret_id(&format!("esv-{}", "a".repeat(125))).is_err());
    }

    // -----------------------------------------------------------------
    // init
    // -----------------------------------------------------------------

    /// Red when `plan_init` stops comparing `label_mapping` against the
    /// requested `secret_id`.
    #[test]
    fn init_refuses_a_label_someone_else_already_mapped_and_adopts_its_own() {
        let fresh = RotationState {
            identifier: None,
            mapped_alias: None,
            secret: None,
            versions: Vec::new(),
            ..state()
        };

        // Nothing there: all three steps.
        let plan = plan_init(
            &fresh,
            "spa",
            "esv-sp-a-signing",
            None,
            None,
            Some(&pair(NEW)),
        )
        .unwrap();
        assert!(plan.set_identifier && plan.create_secret && plan.map_label);
        assert!(!plan.is_noop());
        assert_eq!(plan.certificate.as_deref(), Some(NEW));

        // A mapping at this label pointing somewhere else is an orphan or
        // another entity's rotation — a mapping outlives the label that
        // named it, so overwriting it silently is how one gets stolen.
        let orphan = message(
            plan_init(
                &fresh,
                "spa",
                "esv-sp-a-signing",
                Some("esv-someone-elses"),
                None,
                Some(&pair(NEW)),
            )
            .unwrap_err(),
        );
        assert!(orphan.contains("esv-someone-elses"), "{orphan}");
        assert!(orphan.contains("secretmap list"), "{orphan}");

        // Pointing at the secret we were going to create is a resumed run,
        // and that step is simply already done.
        let resumed = plan_init(
            &fresh,
            "spa",
            "esv-sp-a-signing",
            Some("esv-sp-a-signing"),
            Some(&facts()),
            None,
        )
        .unwrap();
        assert!(resumed.set_identifier);
        assert!(!resumed.create_secret && !resumed.map_label);
    }

    /// Red when an adopted secret is reported as set up without reading what
    /// the entity publishes — the P1 this was written for.
    #[test]
    fn adopting_an_existing_secret_claims_only_what_the_export_showed() {
        // The discriminating input: the adopted secret publishes nothing,
        // which is exactly what a secret with no ENABLED version does. The old
        // code never reached an export at all — `expected` was `None` whenever
        // no key pair was written and `finish` returned success — so this state
        // and the one below were reported identically.
        let nothing = message(
            adoption_outcome(
                &state(),
                "esv-sp-a-signing",
                &shas([]),
                None,
                Adoption::Applied,
            )
            .unwrap_err(),
        );
        assert!(
            nothing.contains("publishes no signing certificate"),
            "{nothing}"
        );
        assert!(nothing.contains("no ENABLED version"), "{nothing}");
        assert!(
            nothing.contains("esv secret versions esv-sp-a-signing"),
            "{nothing}"
        );
        // The entity was already written; saying so is the difference between
        // an operator who reads their tenant and one who re-runs blind.
        assert!(nothing.contains("nothing has been undone"), "{nothing}");

        // Publishing something is reported as the fingerprint that was read,
        // with the limit named rather than implied past.
        let found = adoption_outcome(
            &state(),
            "esv-sp-a-signing",
            &shas([OLD]),
            None,
            Adoption::Applied,
        )
        .unwrap()
        .join("\n");
        assert!(found.contains(OLD), "{found}");
        assert!(found.contains(ADOPTION_CAVEAT), "{found}");
        assert!(ADOPTION_CAVEAT.contains("write-only"));

        // A key pair handed to an `init` that adopted is not written
        // anywhere, and the report says which of the two it is rather than
        // leaving the operator to assume the new key is in service.
        let ignored = adoption_outcome(
            &state(),
            "esv-sp-a-signing",
            &shas([OLD]),
            Some(NEW),
            Adoption::Applied,
        )
        .unwrap()
        .join("\n");
        assert!(ignored.contains("is NOT among them"), "{ignored}");
        assert!(ignored.contains("rotate stage"), "{ignored}");
        let present = adoption_outcome(
            &state(),
            "esv-sp-a-signing",
            &shas([OLD]),
            Some(OLD),
            Adoption::Applied,
        )
        .unwrap()
        .join("\n");
        assert!(present.contains("is among them"), "{present}");
        assert!(!present.contains("NOT among them"), "{present}");

        // The idempotent `init` reaches the same rule, and must not inherit
        // the sentence about a write it never made. This is the arm that used
        // to be skipped entirely: `plan.is_noop()` returned success in front
        // of the export read, so a fully configured role publishing nothing
        // printed "already set up" and exited zero.
        let already = message(
            adoption_outcome(
                &state(),
                "esv-sp-a-signing",
                &shas([]),
                None,
                Adoption::AlreadyDone,
            )
            .unwrap_err(),
        );
        assert!(
            already.contains("publishes no signing certificate"),
            "{already}"
        );
        assert!(already.contains("Nothing was sent"), "{already}");
        assert!(!already.contains("nothing has been undone"), "{already}");
    }

    /// Red when `InitPlan::lines` credits an adopted secret with the
    /// certificate in the supplied key pair.
    #[test]
    fn a_plan_that_adopts_a_secret_does_not_name_a_certificate_it_never_wrote() {
        let fresh = RotationState {
            identifier: None,
            mapped_alias: None,
            secret: None,
            versions: Vec::new(),
            ..state()
        };
        let creating = plan_init(
            &fresh,
            "spa",
            "esv-sp-a-signing",
            None,
            None,
            Some(&pair(NEW)),
        )
        .unwrap()
        .lines(&fresh)
        .join("\n");
        assert!(
            creating.contains(&format!("certificate {NEW}")),
            "{creating}"
        );

        // Same key pair, but the secret already exists. Naming the
        // certificate on the "done" step reads as "that secret holds this
        // certificate", which is the one thing nothing here has checked.
        let adopting = plan_init(
            &fresh,
            "spa",
            "esv-sp-a-signing",
            None,
            Some(&facts()),
            Some(&pair(NEW)),
        )
        .unwrap()
        .lines(&fresh)
        .join("\n");
        assert!(
            !adopting.contains(&format!("certificate {NEW}")),
            "{adopting}"
        );
        assert!(adopting.contains("is not written"), "{adopting}");
    }

    /// Red when `plan_init` stops refusing a different existing identifier,
    /// stops calling `unusable_reason`, or stops requiring a key to create.
    #[test]
    fn init_refuses_to_repoint_an_entity_and_to_adopt_a_secret_that_cannot_work() {
        // Repointing is not rotation. It leaves the old label mapped to the
        // old secret with nothing naming it, and the labels vanish from the
        // schema enum the moment the entity stops naming them — which is the
        // orphaned-mapping defect this sprint just fixed.
        let repoint = message(
            plan_init(
                &state(),
                "spb",
                "esv-sp-b-signing",
                None,
                None,
                Some(&pair(NEW)),
            )
            .unwrap_err(),
        );
        assert!(repoint.contains("rotate stage"), "{repoint}");
        assert!(repoint.contains("secretmap remove"), "{repoint}");
        assert!(repoint.contains(".spa.signing"), "{repoint}");

        // Same identifier is a resumed run, not a repoint.
        assert!(
            plan_init(
                &state(),
                "spa",
                "esv-sp-a-signing",
                Some("esv-sp-a-signing"),
                Some(&facts()),
                None
            )
            .unwrap()
            .is_noop()
        );

        let fresh = RotationState {
            identifier: None,
            mapped_alias: None,
            secret: None,
            versions: Vec::new(),
            ..state()
        };

        // An existing secret that cannot back a restart-free rotation is
        // refused here, not discovered after the entity has been written.
        let placeholders = SecretFacts {
            use_in_placeholders: true,
            ..facts()
        };
        assert!(
            message(
                plan_init(
                    &fresh,
                    "spa",
                    "esv-sp-a-signing",
                    None,
                    Some(&placeholders),
                    None
                )
                .unwrap_err()
            )
            .contains("useInPlaceholders")
        );

        // Creating a secret needs the key pair that goes in it.
        let keyless =
            message(plan_init(&fresh, "spa", "esv-sp-a-signing", None, None, None).unwrap_err());
        assert!(keyless.contains("--key-file"), "{keyless}");
    }

    // -----------------------------------------------------------------
    // What status admits it cannot know.
    // -----------------------------------------------------------------

    /// Red when `status_lines` drops the `published.len() > 1` caveat, or
    /// when `attribution` names a version for a certificate no record
    /// covers.
    #[test]
    fn status_says_which_pairing_it_cannot_know_and_never_infers_one_by_elimination() {
        let lines = status_lines(&staged(), &phase(&staged()), None).join("\n");
        assert!(lines.contains(PAIRING_CAVEAT), "{lines}");
        // The staged certificate is attributed because *this install* wrote
        // the record...
        assert!(
            lines.contains(&format!("{NEW}  ESV secret version 2")),
            "{lines}"
        );
        // ...and the other one is not, because knowing A is version 2 does
        // not make B version 1: a third ENABLED version, or a certificate
        // arriving from a label mapped elsewhere, produces this same picture.
        assert!(
            lines.contains(&format!(
                "{OLD}  (no local record of which version holds it)"
            )),
            "{lines}"
        );

        // A settled single-certificate report carries no caveat, because
        // there is no pairing question to answer.
        assert!(
            !status_lines(&state(), &phase(&state()), None)
                .join("\n")
                .contains(PAIRING_CAVEAT)
        );

        // Someone else's rollover: two certificates, no record, nothing
        // attributed to anything.
        let stranger = RotationState {
            record: None,
            ..staged()
        };
        let lines = status_lines(&stranger, &phase(&stranger), None).join("\n");
        assert_eq!(lines.matches("no local record").count(), 2, "{lines}");
        assert!(lines.contains(PAIRING_CAVEAT));

        // A record about a setup this tenant no longer has attributes
        // nothing either, and that is not cosmetic: the version number this
        // report prints is the number an operator types into
        // `--disable-version`, so it has to clear the same bar `complete`
        // sets before acting on one. Reading `state.record` directly would
        // print "version 2" for a secret that has never held what the record
        // says it holds.
        let recreated = RotationState {
            identifier: Some("spa2".into()),
            ..staged()
        };
        let lines = status_lines(&recreated, &phase(&recreated), None).join("\n");
        assert_eq!(lines.matches("no local record").count(), 2, "{lines}");
        assert_eq!(
            status_json(&recreated, &phase(&recreated), None)["published"]
                .as_array()
                .expect("published")
                .iter()
                .filter(|entry| !entry["stagedVersion"].is_null())
                .count(),
            0
        );
        // The positive control: the same state with the identifier the record
        // names does attribute, in both reports.
        assert!(
            status_lines(&staged(), &phase(&staged()), None)
                .join("\n")
                .contains("ESV secret version 2")
        );
        assert_eq!(
            status_json(&staged(), &phase(&staged()), None)["published"]
                .as_array()
                .expect("published")
                .iter()
                .filter(|entry| entry["stagedVersion"] == "2")
                .count(),
            1
        );
    }

    /// Red when `status_json` reports a `stagedVersion` for an unrecorded
    /// certificate, or drops the caveat.
    #[test]
    fn the_json_report_carries_the_same_admission_as_the_text_one() {
        let staged = staged();
        let json = status_json(&staged, &phase(&staged), None);
        assert_eq!(json["phase"], "staged");
        assert_eq!(json["caveat"], PAIRING_CAVEAT);
        assert_eq!(json["descriptor"], "SPSSODescriptor");
        let published = json["published"].as_array().unwrap();
        assert_eq!(published.len(), 2);
        let staged_versions: Vec<&Value> = published
            .iter()
            .map(|entry| &entry["stagedVersion"])
            .collect();
        assert!(staged_versions.contains(&&json!("2")));
        assert!(staged_versions.contains(&&Value::Null));
    }

    /// Red when `next_step` names a verb the phase cannot run.
    #[test]
    fn every_phase_names_the_command_that_moves_it_on_or_says_there_is_none() {
        let state = state();
        assert!(next_step(&state, &Phase::Unconfigured).contains("rotate init"));
        assert!(next_step(&state, &Phase::Unmapped).contains("rotate init"));
        assert!(next_step(&state, &Phase::Settled).contains("rotate stage"));
        assert!(next_step(&state, &Phase::Staged).contains("rotate complete"));
        for stuck in [
            Phase::Dangling {
                alias: "esv-x".into(),
            },
            Phase::Unusable { reason: "x".into() },
            Phase::Inconsistent { detail: "x".into() },
        ] {
            assert!(next_step(&state, &stuck).contains("nothing automatic"));
            assert!(phase_summary(&stuck).contains("x"));
        }
    }
}
