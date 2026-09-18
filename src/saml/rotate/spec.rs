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
    /// Two ENABLED versions and two published certificates: the pre-trust
    /// window. `complete` closes it.
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
                 the peer has to load them before either can be retired. Finish it with \
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

    let mut expected = published;
    expected.insert(incoming.sha256.clone());
    Ok(StagePlan {
        secret_id: secret.id.clone(),
        retained,
        incoming: incoming.sha256.clone(),
        expected,
    })
}

pub fn authorize_stage(dry_run: bool) -> Decision<StagePermit> {
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

    Ok(CompletePlan {
        secret_id: secret.id.clone(),
        disable_version,
        chosen_by,
        retain,
        expected_drop,
    })
}

/// This install's record of the stage, but only while it still describes the
/// tenant in front of it.
///
/// A record is a claim about **four** things at once, and all four have to
/// still hold before a version may be read out of it: the role still points at
/// the identifier that was staged, its signing label still resolves to that
/// ESV secret, that secret still has the version the record names, and the
/// certificate the record names is still published.
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
            .any(|version| version.version == record.version))
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

pub fn authorize_complete(dry_run: bool) -> Decision<CompletePermit> {
    if dry_run {
        Decision::Preview
    } else {
        Decision::Send(CompletePermit {
            _minted_by_authorize_complete: (),
        })
    }
}

pub fn authorize_init(dry_run: bool) -> Decision<InitPermit> {
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
    Err(Error::Config(format!(
        "would disable version {} of {} and stop publishing a certificate {} ({}) currently \
         advertises. Any peer still pinned to it will start rejecting signatures. Confirm at a \
         terminal, or pass --force.",
        plan.disable_version,
        plan.secret_id,
        state.entity_id,
        role_descriptor(state.role)
    )))
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
            "staged — two certificates published; the peer can load both before either is retired"
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
            "hand `aic saml metadata export {scope}` to the peer, then \
             `aic saml rotate complete {scope}`"
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
pub fn status_lines(state: &RotationState, phase: &Phase) -> Vec<String> {
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
    lines.push(format!("phase       {}", phase_summary(phase)));
    lines.push(format!("next        {}", next_step(state, phase)));
    if published.len() > 1 {
        lines.push(String::new());
        lines.push(PAIRING_CAVEAT.to_string());
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
    match state.record.as_ref() {
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
pub fn status_json(state: &RotationState, phase: &Phase) -> Value {
    let published: Vec<Value> = state
        .published()
        .into_iter()
        .map(|cert| {
            serde_json::json!({
                "sha256": cert.sha256,
                "keyName": cert.key_name,
                "stagedVersion": state
                    .record
                    .as_ref()
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

    if let Some(current) = state.identifier.as_deref() {
        if current != identifier {
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
        // Both, not just the new one: the whole point of the window is that
        // the certificate already in service keeps working.
        assert_eq!(plan.expected, shas([OLD, NEW]));

        let same = message(plan_stage(&state(), &pair(OLD)).unwrap_err());
        assert!(same.contains("rotates nothing"), "{same}");

        let again = message(plan_stage(&staged(), &pair(STRANGER)).unwrap_err());
        assert!(again.contains("already staged"), "{again}");
        assert!(again.contains("rotate complete"), "{again}");
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

    /// Red when `complete_ok` returns `Ok(())` regardless of `forced`.
    #[test]
    fn closing_the_window_is_refused_until_it_is_confirmed() {
        let state = staged();
        let plan = plan_complete(&state, None, None).unwrap();
        assert!(complete_ok(true, &plan, &state).is_ok());
        let refusal = message(complete_ok(false, &plan, &state).unwrap_err());
        assert!(refusal.contains("would disable version 1"), "{refusal}");
        assert!(refusal.contains("start rejecting signatures"), "{refusal}");
        assert!(refusal.contains("--force"), "{refusal}");
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
        assert!(matches!(authorize_init(true), Decision::Preview));
        assert!(matches!(authorize_stage(true), Decision::Preview));
        assert!(matches!(authorize_complete(true), Decision::Preview));
        assert!(matches!(authorize_init(false), Decision::Send(_)));
        assert!(matches!(authorize_stage(false), Decision::Send(_)));
        assert!(matches!(authorize_complete(false), Decision::Send(_)));
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

        // And the inputs are the rollover's, not the whole picture: an
        // unrelated document moving must not refuse a safe write, because the
        // remedy is to re-run and a completion that will not run leaves two
        // certificates published.
        let elsewhere = rollover_inputs(&RotationState {
            mapped_alias: Some("esv-something-else".into()),
            identifier: Some("spa2".into()),
            record: None,
            ..staged()
        });
        assert!(rollover_write_ok("complete", "esv-sp-a-signing", &planned, &elsewhere).is_ok());
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
        let lines = status_lines(&staged(), &phase(&staged())).join("\n");
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
            !status_lines(&state(), &phase(&state()))
                .join("\n")
                .contains(PAIRING_CAVEAT)
        );

        // Someone else's rollover: two certificates, no record, nothing
        // attributed to anything.
        let stranger = RotationState {
            record: None,
            ..staged()
        };
        let lines = status_lines(&stranger, &phase(&stranger)).join("\n");
        assert_eq!(lines.matches("no local record").count(), 2, "{lines}");
        assert!(lines.contains(PAIRING_CAVEAT));
    }

    /// Red when `status_json` reports a `stagedVersion` for an unrecorded
    /// certificate, or drops the caveat.
    #[test]
    fn the_json_report_carries_the_same_admission_as_the_text_one() {
        let staged = staged();
        let json = status_json(&staged, &phase(&staged));
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
