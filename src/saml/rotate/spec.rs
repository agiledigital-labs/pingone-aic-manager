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
/// looks like it addresses.
///
/// The identifier is spliced into a dotted label **and** into a URL path, so a
/// dot in it silently renames the label family and a slash escapes the
/// collection. Neither fails loudly: a mapping `PUT` at the wrong label is a
/// perfectly good 201 that nothing will ever read.
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
        .find(|character| !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_'))
    {
        return Err(Error::Config(format!(
            "--identifier {identifier:?} contains {bad:?}: the identifier is spliced into the \
             dotted label `{}` and into a URL path, so anything but letters, digits, `-` and \
             `_` addresses a different label than it reads as",
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

/// Whether the entity that came back is the entity that was sent.
///
/// Returns the paths that differ, so a write AM quietly reshaped is reported
/// as *what* it lost rather than as "mismatch". `_rev` is excluded because it
/// is content-derived and our content changed on purpose.
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
        let mut enabled: Vec<&SecretVersion> =
            self.versions.iter().filter(|v| v.enabled()).collect();
        enabled.sort_by_key(|version| version.number());
        enabled
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

/// What `complete` would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePlan {
    pub secret_id: String,
    /// The ESV secret version to disable — the oldest ENABLED one.
    pub disable_version: String,
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
/// `retain` is the fingerprint the operator wants to keep. It comes from this
/// install's own record of the stage when there is one, and otherwise has to
/// be supplied: **ESV secret values are write-only**, so nothing readable ties
/// a published certificate to a version number, and "the newest version is
/// listed first" is an ordering convention rather than an identity.
pub fn plan_complete(state: &RotationState, retain: Option<&str>) -> Result<CompletePlan> {
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
    let retain = match retain.map(str::trim).filter(|value| !value.is_empty()) {
        Some(retain) => retain.to_lowercase(),
        None => {
            let record = state.record.as_ref().ok_or_else(|| {
                Error::Config(format!(
                    "this install has no record of staging a rollover for {} ({}), so it cannot \
                     tell which of the two published certificates is the new one — ESV secret \
                     values are write-only and AM publishes no <ds:KeyName>. Pass \
                     --retain <sha256> naming the certificate to keep; \
                     `aic saml rotate status` lists both.",
                    state.entity_id,
                    role_descriptor(state.role)
                ))
            })?;
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
    let oldest = enabled
        .first()
        .copied()
        .expect("Staged means two ENABLED versions");
    if let Some(latest) = state.latest_version() {
        if latest.version == oldest.version {
            return Err(Error::Config(format!(
                "the oldest ENABLED version of {} is also its newest version ({}), and AIC \
                 refuses to disable the latest version. Add the new key pair first.",
                secret.id, oldest.version
            )));
        }
    }
    if let Some(record) = state.record.as_ref() {
        if record.sha256 == retain && record.version == oldest.version {
            return Err(Error::Config(format!(
                "the certificate to keep ({retain}) is the one this install staged as version \
                 {} of {}, and that is also the oldest ENABLED version — disabling it would \
                 retire the new certificate and leave the old one. Check \
                 `aic saml rotate status` before forcing this by hand.",
                record.version, secret.id
            )));
        }
    }

    let expected_drop = state
        .record
        .as_ref()
        .filter(|record| record.sha256 == retain)
        .and_then(|_| published.iter().find(|cert| **cert != retain).cloned());

    Ok(CompletePlan {
        secret_id: secret.id.clone(),
        disable_version: oldest.version.clone(),
        retain,
        expected_drop,
    })
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

/// The sentence every two-certificate report carries.
pub const PAIRING_CAVEAT: &str = "\
Which published certificate came from which ESV secret version is not readable: secret values are
write-only and AM emits no <ds:KeyName>. A certificate is tied to a version here only when this
install staged it; otherwise `complete` needs --retain <sha256> naming the one to keep.";

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
        lines.push(step(
            self.create_secret,
            &format!(
                "create ESV secret {} — encoding pem, useInPlaceholders false{}",
                self.secret_id,
                self.certificate
                    .as_deref()
                    .map(|sha| format!(", certificate {sha}"))
                    .unwrap_or_default()
            ),
        ));
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
