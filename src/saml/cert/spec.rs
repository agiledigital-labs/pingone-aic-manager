//! The tenant-free half of `aic saml cert`: what a remote entity publishes,
//! what a certificate change would send, and every guard AM does not apply.
//!
//! The measured facts behind each guard are in `docs/api/06-saml.md`,
//! "Updating a remote entity's certificates". The short version:
//! `UPDATE_CERTIFICATES` replaces **every** `<KeyDescriptor>` of each role the
//! document names, encryption included; adds a whole role the entity lacks;
//! accepts a role with no signing certificate; is not refused on a hosted
//! entity; and targets whatever entity the document's `entityID` names. So
//! the document sent is the entity's own export with signing key descriptors
//! spliced in or out ([`metadata::key_layout`]), and [`finish`] re-reads that
//! document before anything can be authorized.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::saml::metadata::{self, Edit, Fact, KeyEntry, KeyLayout, RoleKeys};
use crate::saml::rotate::pem::{self, CertDetails};
use crate::saml::rotate::spec::{Decision, SIGNING_USE, WriteStatus, role_descriptor};
use crate::saml::spec::{EntityStub, Located, Location, Role, locate};
use crate::{Error, Result};

/// A certificate expiring within this many days is flagged in `cert list`.
pub const EXPIRING_SOON_DAYS: i64 = 30;

// ── the target ─────────────────────────────────────────────────────────────

/// A remote entity, confirmed from the realm's entity list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub entity_id: String,
    pub realm: String,
    /// The roles the list says it holds.
    pub roles: Vec<Role>,
}

/// Confirm `entity_id` is a **remote** entity in `stubs`.
///
/// The endpoint's collection name is not a guard: an `UPDATE_CERTIFICATES`
/// sent to `…/saml2/remote` naming a hosted entity is a 200 that can add a
/// whole role to it (measured 2026-09-28). So hosted is refused here, before
/// anything is planned, and pointed at the verb that does own a hosted
/// entity's signing key.
pub fn target_ok(entity_id: &str, stubs: &[EntityStub], realm: &str) -> Result<Target> {
    match locate(entity_id, stubs) {
        Located::NotFound => Err(Error::Config(format!(
            "no SAML entity {entity_id:?} in realm {realm}; `aic saml list --realm {realm}` \
             shows what exists. The certificate update targets an existing remote entity and \
             never creates one — `aic saml import` does that."
        ))),
        Located::Ambiguous(_) => Err(Error::Config(format!(
            "{entity_id:?} exists as both a hosted and a remote entity in realm {realm}, and a \
             certificate update names its target only by entity ID; refusing rather than \
             guessing which one AM would write"
        ))),
        Located::In(Location::Hosted) => Err(Error::Config(format!(
            "{entity_id:?} is a hosted entity in realm {realm}. A hosted entity's certificates \
             are not in its metadata — AM generates them from the secret store — so this verb \
             would not change them, and AM does not refuse the attempt: it can add a whole role \
             to the hosted entity instead. Rotate a hosted entity's signing key with \
             `aic saml rotate`."
        ))),
        Located::In(Location::Remote) => {
            let stub = stubs
                .iter()
                .find(|stub| {
                    stub.location == Location::Remote && stub.entity_id.trim() == entity_id.trim()
                })
                .expect("locate found it in remote");
            Ok(Target {
                entity_id: stub.entity_id.clone(),
                realm: realm.to_string(),
                roles: [Role::Idp, Role::Sp]
                    .into_iter()
                    .filter(|role| stub.has_role(*role))
                    .collect(),
            })
        }
    }
}

// ── reading what is published ───────────────────────────────────────────────

fn short(role: Role) -> &'static str {
    match role {
        Role::Idp => "idp",
        Role::Sp => "sp",
    }
}

fn role_of(role: metadata::Role) -> Role {
    match role {
        metadata::Role::IdentityProvider => Role::Idp,
        metadata::Role::ServiceProvider => Role::Sp,
    }
}

/// A key descriptor's `use`, as `cert list` names it.
fn use_label(key_use: Option<&str>) -> &str {
    key_use.unwrap_or("unspecified")
}

fn is_signing(key: &KeyEntry) -> bool {
    key.key_use.as_deref() == Some(SIGNING_USE)
}

/// One certificate a role publishes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertRow {
    #[serde(serialize_with = "serialize_role")]
    pub role: Role,
    /// `signing`, `encryption`, or `unspecified` for a key descriptor with no
    /// `use` — which the metadata schema reads as both.
    #[serde(rename = "use")]
    pub key_use: String,
    pub sha256: String,
    /// `None` when the DER is not a certificate this tool can read; the
    /// fingerprint is still real.
    #[serde(flatten)]
    pub details: Option<CertDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
}

fn serialize_role<S: serde::Serializer>(
    role: &Role,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(short(*role))
}

/// Every certificate in the export, in document order, optionally for one
/// role. Reads the export only, which is why `cert list` works locked.
pub fn cert_rows(export: &[u8], role: Option<Role>) -> Result<Vec<CertRow>> {
    let layout = metadata::key_layout(export)?;
    let mut rows = Vec::new();
    for role_keys in &layout.roles {
        let this = role_of(role_keys.role);
        if role.is_some_and(|wanted| wanted != this) {
            continue;
        }
        for key in &role_keys.keys {
            for (cert, der) in &key.certs {
                let (details, unreadable) = match pem::certificate_details(der) {
                    Ok(details) => (Some(details), None),
                    Err(error) => (None, Some(error)),
                };
                rows.push(CertRow {
                    role: this,
                    key_use: use_label(key.key_use.as_deref()).to_string(),
                    sha256: cert.sha256.clone(),
                    details,
                    unreadable,
                });
            }
        }
    }
    Ok(rows)
}

/// How close a certificate is to its `notAfter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expiry {
    Valid,
    ExpiringSoon,
    Expired,
}

pub fn expiry(not_after: DateTime<Utc>, now: DateTime<Utc>) -> (i64, Expiry) {
    let days = (not_after - now).num_days();
    let state = if not_after <= now {
        Expiry::Expired
    } else if days < EXPIRING_SOON_DAYS {
        Expiry::ExpiringSoon
    } else {
        Expiry::Valid
    };
    (days, state)
}

/// `cert list --json`: each row with the days left and the flag, computed
/// against `now` so a test can pin them.
pub fn list_json(rows: &[CertRow], now: DateTime<Utc>) -> serde_json::Value {
    let values = rows
        .iter()
        .map(|row| {
            let mut value = serde_json::to_value(row).unwrap_or_default();
            if let (Some(details), Some(object)) = (&row.details, value.as_object_mut()) {
                let (days, state) = expiry(details.not_after, now);
                object.insert("daysUntilExpiry".into(), days.into());
                object.insert(
                    "expiry".into(),
                    serde_json::to_value(state).unwrap_or_default(),
                );
            }
            value
        })
        .collect();
    serde_json::Value::Array(values)
}

/// `cert list`, as a block per certificate. Seven fields do not fit a table
/// row at any terminal width a subject DN leaves, so this follows
/// `saml show`'s block layout rather than `saml list`'s table.
pub fn list_lines(entity_id: &str, rows: &[CertRow], now: DateTime<Utc>) -> Vec<String> {
    if rows.is_empty() {
        return vec![format!("{entity_id} publishes no certificates")];
    }
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.push(format!(
            "{} {}  sha256 {}",
            short(row.role),
            row.key_use,
            row.sha256
        ));
        match (&row.details, &row.unreadable) {
            (Some(details), _) => {
                let (days, state) = expiry(details.not_after, now);
                let flag = match state {
                    Expiry::Valid => format!("{days} days left"),
                    Expiry::ExpiringSoon => format!("EXPIRING SOON — {days} days left"),
                    Expiry::Expired => format!("EXPIRED {} days ago", -days),
                };
                lines.push(format!("  subject    {}", details.subject));
                lines.push(format!("  issuer     {}", details.issuer));
                lines.push(format!("  notBefore  {}", details.not_before.to_rfc3339()));
                lines.push(format!(
                    "  notAfter   {}  ({flag})",
                    details.not_after.to_rfc3339()
                ));
                lines.push(format!("  key        {}", details.key()));
            }
            (None, Some(error)) => lines.push(format!("  unreadable certificate: {error}")),
            (None, None) => {}
        }
    }
    lines
}

// ── planning a change ───────────────────────────────────────────────────────

/// One published certificate's identity: `(role, use, sha256)`. The
/// post-write check compares a **set** of these, never a count — a change
/// that dropped the wrong certificate can leave the same number published.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CertKey {
    pub descriptor: String,
    pub key_use: Option<String>,
    pub sha256: String,
}

impl CertKey {
    fn describe(&self) -> String {
        format!(
            "{} {} {}",
            self.descriptor,
            use_label(self.key_use.as_deref()),
            self.sha256
        )
    }
}

/// Every certificate the document publishes, as a set.
pub fn published(xml: &[u8]) -> Result<BTreeSet<CertKey>> {
    Ok(metadata::cert_refs(xml)?
        .into_iter()
        .map(|cert| CertKey {
            descriptor: cert.descriptor,
            key_use: cert.key_use,
            sha256: cert.sha256,
        })
        .collect())
}

/// A signing certificate as a summary names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertInfo {
    pub sha256: String,
    pub subject: Option<String>,
    pub not_after: Option<DateTime<Utc>>,
}

impl CertInfo {
    fn from_der(sha256: &str, der: &[u8]) -> Self {
        let details = pem::certificate_details(der).ok();
        Self {
            sha256: sha256.to_string(),
            subject: details.as_ref().map(|details| details.subject.clone()),
            not_after: details.map(|details| details.not_after),
        }
    }

    fn line(&self) -> String {
        format!(
            "{}  sha256 {}  notAfter {}",
            self.subject
                .as_deref()
                .unwrap_or("(unreadable certificate)"),
            self.sha256,
            self.not_after
                .map_or_else(|| "?".to_string(), |at| at.format("%Y-%m-%d").to_string())
        )
    }
}

/// What a plan does to one role's **signing** certificates. Encryption and
/// `use`-less key descriptors are carried byte for byte and never listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChange {
    pub role: Role,
    pub added: Vec<CertInfo>,
    pub removed: Vec<CertInfo>,
    pub kept: Vec<CertInfo>,
}

/// A certificate change, worked out and checked before anything is sent.
///
#[derive(Debug, Clone)]
pub struct CertPlan {
    pub entity_id: String,
    pub realm: String,
    /// The export this plan was computed from. The pre-write recheck refuses
    /// the write if a fresh export differs from it by a single byte. Private,
    /// with [`finish`] the only constructor, so holding a plan means every
    /// structural guard passed over the exact bytes it carries.
    base: Vec<u8>,
    /// The document that will be sent: `base` with signing key descriptors
    /// spliced in or out.
    pub document: Vec<u8>,
    pub changes: Vec<RoleChange>,
    /// What the export must publish afterwards — every role, every use.
    pub expected: BTreeSet<CertKey>,
}

/// A plan, or the reason there is nothing to send.
#[derive(Debug, Clone)]
pub enum Planned {
    Change(CertPlan),
    /// Already the requested state; the sentence says so and exits 0.
    NoChange(String),
}

/// Read the export as something this verb may splice.
fn editable(export: &[u8], entity_id: &str) -> Result<KeyLayout> {
    let layout = metadata::key_layout(export)?;
    if layout.signed {
        return Err(Error::Config(format!(
            "the tenant's export of {entity_id} carries a <ds:Signature>, and splicing a key \
             descriptor into it would send a signature this tool had broken; refusing rather \
             than stripping it"
        )));
    }
    let mut seen = Vec::new();
    for role in &layout.roles {
        if seen.contains(&role.role) {
            return Err(Error::Config(format!(
                "the tenant's export of {entity_id} declares {} twice, and which one AM would \
                 update is not known; refusing",
                role.descriptor
            )));
        }
        seen.push(role.role);
    }
    Ok(layout)
}

/// The role a single-role change applies to.
///
/// An entity may hold both roles, each with its own signing key descriptors,
/// so on a dual-role entity the choice is refused rather than defaulted —
/// `rotate::spec::choose_role`'s rule, applied to the roles the export
/// publishes.
fn choose_role(layout: &KeyLayout, requested: Option<Role>, entity_id: &str) -> Result<Role> {
    let held = layout
        .roles
        .iter()
        .map(|role| role_of(role.role))
        .collect::<Vec<_>>();
    match (held.as_slice(), requested) {
        ([], _) => Err(Error::Config(format!(
            "{entity_id} publishes no IdP or SP role, so there is nowhere to put a certificate"
        ))),
        ([only], None) => Ok(*only),
        (_, Some(asked)) if held.contains(&asked) => Ok(asked),
        (_, Some(asked)) => Err(Error::Config(format!(
            "{entity_id} holds no {} role; it holds {}",
            asked.wire(),
            held.iter()
                .map(|role| role.wire())
                .collect::<Vec<_>>()
                .join(" and ")
        ))),
        (_, None) => Err(Error::Config(format!(
            "{entity_id} holds both the identityProvider and serviceProvider roles, and each \
             publishes its own signing certificates — pass --role idp or --role sp to say which"
        ))),
    }
}

fn role_keys(layout: &KeyLayout, role: Role) -> &RoleKeys {
    layout
        .roles
        .iter()
        .find(|keys| role_of(keys.role) == role)
        .expect("choose_role returned a role the layout holds")
}

/// Signing certificates as `(sha256, der)`, in document order.
type SigningCerts = Vec<(String, Vec<u8>)>;

/// The signing certificates a role publishes.
fn signing_certs(role: &RoleKeys) -> SigningCerts {
    role.keys
        .iter()
        .filter(|key| is_signing(key))
        .flat_map(|key| &key.certs)
        .map(|(cert, der)| (cert.sha256.clone(), der.clone()))
        .collect()
}

fn insert(role: &RoleKeys, der: &[u8], entity_id: &str) -> Result<Edit> {
    role.insert_signing_key(der).ok_or_else(|| {
        Error::Config(format!(
            "{entity_id}'s {} is an empty element with nowhere to put a key descriptor without \
             rewriting it; refusing",
            role.descriptor
        ))
    })
}

/// Add one signing certificate to one role.
pub fn plan_add(
    target: &Target,
    export: &[u8],
    der: &[u8],
    sha256: &str,
    requested: Option<Role>,
) -> Result<Planned> {
    let layout = editable(export, &target.entity_id)?;
    let role = choose_role(&layout, requested, &target.entity_id)?;
    let keys = role_keys(&layout, role);
    let current = signing_certs(keys);
    if current.iter().any(|(held, _)| held == sha256) {
        return Ok(Planned::NoChange(format!(
            "{} already publishes {sha256} as a signing certificate of its {} role; nothing to \
             send",
            target.entity_id,
            short(role)
        )));
    }
    let edit = insert(keys, der, &target.entity_id)?;
    let change = RoleChange {
        role,
        added: vec![CertInfo::from_der(sha256, der)],
        removed: Vec::new(),
        kept: infos(&current),
    };
    finish(target, export, &[edit], vec![change])
}

fn infos(certs: &[(String, Vec<u8>)]) -> Vec<CertInfo> {
    certs
        .iter()
        .map(|(sha, der)| CertInfo::from_der(sha, der))
        .collect()
}

/// Normalise a `--cert` selector: lowercase hex, colons allowed (the way
/// `openssl x509 -fingerprint` prints one).
fn selector(raw: &str) -> Result<String> {
    let cleaned = raw.trim().replace(':', "").to_ascii_lowercase();
    if cleaned.is_empty()
        || !cleaned
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(Error::Config(format!(
            "{raw:?} is not a SHA-256 fingerprint or a prefix of one; `aic saml cert list` \
             prints them"
        )));
    }
    Ok(cleaned)
}

/// Remove one signing certificate, named by fingerprint or an unambiguous
/// prefix of one.
///
/// Only `use="signing"` key descriptors are candidates: an encryption or
/// `use`-less certificate is outside this verb's scope and is carried byte
/// for byte. A role's **last** signing certificate is never removable —
/// AM accepts a role with none (measured), and then the entity can verify
/// nothing it signs. There is no `--force` for that; deleting the entity is
/// the deliberate way to stop trusting it.
pub fn plan_remove(
    target: &Target,
    export: &[u8],
    raw_selector: &str,
    requested: Option<Role>,
) -> Result<Planned> {
    let wanted = selector(raw_selector)?;
    let layout = editable(export, &target.entity_id)?;
    if let Some(asked) = requested {
        choose_role(&layout, Some(asked), &target.entity_id)?;
    }

    // Every (role, sha) the selector matches among signing certificates.
    let mut matches: Vec<(Role, String)> = Vec::new();
    for keys in &layout.roles {
        let role = role_of(keys.role);
        if requested.is_some_and(|asked| asked != role) {
            continue;
        }
        for (sha, _) in signing_certs(keys) {
            if sha.starts_with(&wanted) && !matches.contains(&(role, sha.clone())) {
                matches.push((role, sha));
            }
        }
    }
    let distinct = matches
        .iter()
        .map(|(_, sha)| sha.as_str())
        .collect::<BTreeSet<_>>();
    let (role, sha) = match (matches.as_slice(), distinct.len()) {
        ([], _) => {
            return Err(Error::Config(format!(
                "no signing certificate of {} matches {raw_selector:?}; `aic saml cert list {} \
                 --realm {}` prints what it publishes. Only signing certificates can be removed.",
                target.entity_id, target.entity_id, target.realm
            )));
        }
        ([only], _) => only.clone(),
        (_, 1) => {
            return Err(Error::Config(format!(
                "{} is a signing certificate of both roles of {}; pass --role idp or --role sp \
                 to say which to remove it from",
                matches[0].1, target.entity_id
            )));
        }
        (_, _) => {
            return Err(Error::Config(format!(
                "{raw_selector:?} matches {} signing certificates of {} ({}); give more of the \
                 fingerprint",
                distinct.len(),
                target.entity_id,
                distinct.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    };

    let keys = role_keys(&layout, role);
    let mut edits = Vec::new();
    for key in keys.keys.iter().filter(|key| is_signing(key)) {
        if !key.certs.iter().any(|(cert, _)| cert.sha256 == sha) {
            continue;
        }
        if key.certs.len() > 1 {
            return Err(Error::Config(format!(
                "{sha} shares one <KeyDescriptor> with {} other certificate(s) in {}'s {}, and \
                 removing it alone would mean rewriting that key descriptor; refusing",
                key.certs.len() - 1,
                target.entity_id,
                keys.descriptor
            )));
        }
        edits.push(key.remove());
    }
    let current = signing_certs(keys);
    let (removed, kept): (Vec<_>, Vec<_>) = current.into_iter().partition(|(held, _)| *held == sha);
    let change = RoleChange {
        role,
        added: Vec::new(),
        removed: infos(&removed),
        kept: infos(&kept),
    };
    finish(target, export, &edits, vec![change])
}

/// How an import over an existing remote entity treats its certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum ImportMode {
    /// Merge the file's signing certificates in, deduplicated by fingerprint.
    Add,
    /// Make each role's signing certificates exactly the file's.
    Replace,
}

impl ImportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
        }
    }
}

/// What a file says that an update will not apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Differences {
    /// Non-certificate content in the file and not on the tenant.
    pub only_in_file: Vec<Fact>,
    /// Non-certificate content on the tenant and not in the file.
    pub only_on_tenant: Vec<Fact>,
    /// Encryption or `use`-less certificates the file has and the tenant's
    /// role does not — out of this verb's scope, so ignored.
    pub ignored_keys: Vec<String>,
}

/// The sentence every summary with a non-certificate difference carries.
/// Pinned by a test: it is the reason an operator does not expect an
/// endpoint change to have happened.
pub const NOT_APPLIED: &str = "not applied — AIC ignores non-certificate changes on an update; \
     changing them means delete and re-import, which drops the entity's circle-of-trust \
     membership";

/// The one line for a file whose only differences are certificates.
pub const ONLY_CERTS_DIFFER: &str =
    "the file and the tenant differ only in certificates; nothing else would be left behind";

impl Differences {
    pub fn lines(&self) -> Vec<String> {
        if self.only_in_file.is_empty()
            && self.only_on_tenant.is_empty()
            && self.ignored_keys.is_empty()
        {
            return vec![ONLY_CERTS_DIFFER.to_string()];
        }
        let mut lines = Vec::new();
        if !self.only_in_file.is_empty() || !self.only_on_tenant.is_empty() {
            lines.push(format!("other differences: {NOT_APPLIED}"));
            for fact in &self.only_in_file {
                lines.push(format!("  file only    {}: {}", fact.owner, fact.text));
            }
            for fact in &self.only_on_tenant {
                lines.push(format!("  tenant only  {}: {}", fact.owner, fact.text));
            }
        }
        if !self.ignored_keys.is_empty() {
            lines.push(
                "non-signing certificates in the file: not applied — `aic saml cert` changes \
                 signing certificates only, and the tenant's encryption keys are kept as they are"
                    .to_string(),
            );
            for key in &self.ignored_keys {
                lines.push(format!("  {key}"));
            }
        }
        lines
    }
}

/// Multiset difference, `a` minus `b`.
fn minus(a: &[Fact], b: &[Fact]) -> Vec<Fact> {
    let mut remaining = b.to_vec();
    let mut out = Vec::new();
    for fact in a {
        if let Some(index) = remaining.iter().position(|held| held == fact) {
            remaining.swap_remove(index);
        } else {
            out.push(fact.clone());
        }
    }
    out.sort();
    out
}

/// An import over an existing remote entity, read against its export.
#[derive(Debug, Clone)]
pub struct ImportComparison {
    pub differences: Differences,
    target: Target,
    export: Vec<u8>,
    export_layout: KeyLayout,
    /// Per file role: its signing certificates, deduplicated, in order.
    file_signing: Vec<(Role, SigningCerts)>,
}

/// Compare an import file with the entity's export. `file` is the document
/// the import would otherwise have sent (sanitised, unless the operator
/// said not to), and **none of its bytes are sent**: the plan is built from
/// the export, and a certificate from the file reaches the document as a key
/// descriptor written fresh from its DER.
pub fn compare_import(target: &Target, export: &[u8], file: &[u8]) -> Result<ImportComparison> {
    let export_layout = editable(export, &target.entity_id)?;
    let file_layout = metadata::key_layout(file)?;
    if file_layout.entity_id != target.entity_id {
        return Err(Error::Config(format!(
            "the file names {:?}, not {:?}; refusing",
            file_layout.entity_id, target.entity_id
        )));
    }
    let held = export_layout
        .roles
        .iter()
        .map(|role| role_of(role.role))
        .collect::<Vec<_>>();
    let mut file_signing = Vec::new();
    let mut ignored_keys = Vec::new();
    for file_role in &file_layout.roles {
        let role = role_of(file_role.role);
        if !held.contains(&role) {
            return Err(Error::Config(format!(
                "the file declares a {} role that {} does not hold. A certificate update would \
                 add that whole role, endpoints and all — AM does not limit it to certificates \
                 for a role the entity lacks — so this is refused; adding a role means delete \
                 and re-import, which drops the entity's circle-of-trust membership",
                role.wire(),
                target.entity_id
            )));
        }
        if file_signing.iter().any(|(seen, _)| *seen == role) {
            return Err(Error::Config(format!(
                "the file declares {} twice; refusing",
                file_role.descriptor
            )));
        }
        let tenant_role = role_keys(&export_layout, role);
        let tenant_all = tenant_role
            .keys
            .iter()
            .flat_map(|key| key.certs.iter().map(|(cert, _)| cert.sha256.clone()))
            .collect::<BTreeSet<_>>();
        let mut signing: SigningCerts = Vec::new();
        for key in &file_role.keys {
            for (cert, der) in &key.certs {
                if is_signing(key) {
                    if !signing.iter().any(|(sha, _)| *sha == cert.sha256) {
                        signing.push((cert.sha256.clone(), der.clone()));
                    }
                } else if !tenant_all.contains(&cert.sha256) {
                    ignored_keys.push(format!(
                        "{} {} {}",
                        short(role),
                        use_label(key.key_use.as_deref()),
                        cert.sha256
                    ));
                }
            }
        }
        file_signing.push((role, signing));
    }
    let differences = Differences {
        only_in_file: minus(&file_layout.facts, &export_layout.facts),
        only_on_tenant: minus(&export_layout.facts, &file_layout.facts),
        ignored_keys,
    };
    Ok(ImportComparison {
        differences,
        target: target.clone(),
        export: export.to_vec(),
        export_layout,
        file_signing,
    })
}

/// Plan an import in `mode`. Roles the file does not declare are left as
/// they are, which is also what AM does with a role a document omits.
pub fn plan_import(comparison: &ImportComparison, mode: ImportMode) -> Result<Planned> {
    let entity_id = &comparison.target.entity_id;
    let mut edits = Vec::new();
    let mut changes = Vec::new();
    for (role, wanted) in &comparison.file_signing {
        let keys = role_keys(&comparison.export_layout, *role);
        let current = signing_certs(keys);
        let mut added = Vec::new();
        for (sha, der) in wanted {
            if !current.iter().any(|(held, _)| held == sha) {
                edits.push(insert(keys, der, entity_id)?);
                added.push(CertInfo::from_der(sha, der));
            }
        }
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for (sha, der) in &current {
            let goes = mode == ImportMode::Replace && !wanted.iter().any(|(want, _)| want == sha);
            if goes {
                removed.push(CertInfo::from_der(sha, der));
            } else {
                kept.push(CertInfo::from_der(sha, der));
            }
        }
        if mode == ImportMode::Replace {
            for key in keys.keys.iter().filter(|key| is_signing(key)) {
                let dropping = key
                    .certs
                    .iter()
                    .filter(|(cert, _)| !wanted.iter().any(|(want, _)| *want == cert.sha256))
                    .count();
                if dropping == 0 {
                    continue;
                }
                if dropping < key.certs.len() {
                    return Err(Error::Config(format!(
                        "a <KeyDescriptor> in {entity_id}'s {} holds certificates the file keeps \
                         and certificates it drops, and replacing only some of them would mean \
                         rewriting that key descriptor; refusing",
                        keys.descriptor
                    )));
                }
                edits.push(key.remove());
            }
        }
        changes.push(RoleChange {
            role: *role,
            added,
            removed,
            kept,
        });
    }
    finish(&comparison.target, &comparison.export, &edits, changes)
}

/// Apply the edits and check the document that would be sent.
///
/// The guards read the **result**, not the edits that produced it, so a
/// planner bug that spliced the wrong bytes is caught here rather than by
/// the tenant:
///
/// - the `entityID` is exactly the target's — it is the only thing AM
///   addresses the update by;
/// - every role is one the entity holds, because a role it lacks is added
///   whole, endpoints and all;
/// - every role keeps at least one explicit `use="signing"` certificate,
///   because AM accepts none (a `use`-less one does not count: what AM makes
///   of it for signing is not measured).
fn finish(
    target: &Target,
    export: &[u8],
    edits: &[Edit],
    changes: Vec<RoleChange>,
) -> Result<Planned> {
    let document = metadata::apply_edits(export, edits);
    let layout = metadata::key_layout(&document)?;
    if layout.entity_id != target.entity_id {
        return Err(Error::Config(format!(
            "the document to send names {:?}, not {:?}; refusing",
            layout.entity_id, target.entity_id
        )));
    }
    let mut seen = Vec::new();
    for keys in &layout.roles {
        let role = role_of(keys.role);
        if !target.roles.contains(&role) || seen.contains(&role) {
            return Err(Error::Config(format!(
                "the document to send declares {} for {}, which the realm's entity list does \
                 not show it holding (once); AM would add that role whole, so this is refused",
                keys.descriptor, target.entity_id
            )));
        }
        seen.push(role);
        if signing_certs(keys).is_empty() {
            return Err(Error::Config(format!(
                "this would leave {}'s {} role with no signing certificate. AM accepts that, and \
                 then nothing the peer signs can be verified. There is no --force for it: add \
                 the replacement first (`aic saml cert add`), or delete the entity deliberately \
                 with `aic saml delete` if trust is meant to end.",
                target.entity_id,
                short(role)
            )));
        }
    }
    let expected = published(&document)?;
    if expected == published(export)? {
        return Ok(Planned::NoChange(format!(
            "{} already publishes exactly these certificates; nothing to send",
            target.entity_id
        )));
    }
    Ok(Planned::Change(CertPlan {
        entity_id: target.entity_id.clone(),
        realm: target.realm.clone(),
        base: export.to_vec(),
        document,
        changes,
        expected,
    }))
}

/// The plan, per role: signing certificates added, removed and kept.
pub fn plan_lines(plan: &CertPlan) -> Vec<String> {
    let mut lines = vec![format!(
        "entity      {} (remote, realm {})",
        plan.entity_id, plan.realm
    )];
    for change in &plan.changes {
        lines.push(format!(
            "{} signing certificates ({}):",
            short(change.role),
            role_descriptor(change.role)
        ));
        let groups: [(&str, &Vec<CertInfo>); 3] = [
            ("add   ", &change.added),
            ("remove", &change.removed),
            ("keep  ", &change.kept),
        ];
        for (verb, certs) in groups {
            for cert in certs {
                lines.push(format!("  {verb}  {}", cert.line()));
            }
        }
    }
    lines.push(
        "encryption and use-less key descriptors are sent back exactly as the tenant exported \
         them"
            .to_string(),
    );
    lines
}

// ── authorizing and sending ─────────────────────────────────────────────────

/// Permission to send one certificate update.
///
/// The field is private and [`authorize`] is the only minting site, so
/// `api::update_certificates` — which requires one — is unreachable from a
/// preview: the same shape as `saml::spec::ImportPermit` and `rotate`'s
/// permits.
#[derive(Debug)]
pub struct CertPermit {
    _minted_by_authorize: (),
}

/// `--dry-run` stops here by holding no permit. Takes a plan, which only
/// [`finish`] builds, so a permit implies the structural guards passed.
pub fn authorize(dry_run: bool, _plan: &CertPlan) -> Decision<CertPermit> {
    if dry_run {
        Decision::Preview
    } else {
        Decision::Send(CertPermit {
            _minted_by_authorize: (),
        })
    }
}

/// The consent gate, after the prompt: refuse unless confirmed, naming what
/// would change. `flag` is the escape the verb offers — `--force` for `add`
/// and `remove`, `--certs <mode>` for an import.
pub fn write_ok(confirmed: bool, plan: &CertPlan, flag: &str) -> Result<()> {
    if confirmed {
        return Ok(());
    }
    let (added, removed) = plan
        .changes
        .iter()
        .fold((0, 0), |(added, removed), change| {
            (added + change.added.len(), removed + change.removed.len())
        });
    Err(Error::Config(format!(
        "would change {}'s published signing certificates ({added} added, {removed} removed), \
         which changes which signatures its peers' assertions are verified against. Confirm \
         at a terminal, or pass {flag}.",
        plan.entity_id
    )))
}

/// The pre-write recheck: the entity is still remote with the same roles,
/// and its export is byte-identical to the one the plan was computed from.
/// Anything else means the plan describes a document that no longer exists.
pub fn recheck_ok(plan: &CertPlan, fresh_target: &Target, fresh_export: &[u8]) -> Result<()> {
    if fresh_target.entity_id != plan.entity_id {
        return Err(Error::Config(format!(
            "{} changed identity between the plan and the write; refusing",
            plan.entity_id
        )));
    }
    if fresh_export != plan.base.as_slice() {
        return Err(Error::Config(format!(
            "{}'s exported metadata changed after this plan was computed — someone else wrote \
             it in the meantime. Nothing was sent; run the command again to plan from what is \
             there now.",
            plan.entity_id
        )));
    }
    Ok(())
}

/// Whether the export publishes exactly `expected`.
pub fn settled(published: &BTreeSet<CertKey>, expected: &BTreeSet<CertKey>) -> bool {
    published == expected
}

/// What differs between the export and the plan, for the failure message.
pub fn settlement_gap(published: &BTreeSet<CertKey>, expected: &BTreeSet<CertKey>) -> String {
    let missing = expected
        .difference(published)
        .map(CertKey::describe)
        .collect::<Vec<_>>();
    let unexpected = published
        .difference(expected)
        .map(CertKey::describe)
        .collect::<Vec<_>>();
    format!(
        "missing: {}; unexpected: {}",
        if missing.is_empty() {
            "none".to_string()
        } else {
            missing.join(", ")
        },
        if unexpected.is_empty() {
            "none".to_string()
        } else {
            unexpected.join(", ")
        }
    )
}

/// The message for a failed or unproven update: what is known, then the
/// read to do before retrying.
pub fn failure_message(status: WriteStatus, source: &Error, plan: &CertPlan) -> String {
    let what = format!("the certificate update of {}", plan.entity_id);
    match status {
        WriteStatus::Refused => source.to_string(),
        WriteStatus::AcceptedUnverified | WriteStatus::Unknown => format!(
            "{} Read the tenant before retrying: `aic saml cert list {} --realm {}` shows what it \
             publishes now.\n{source}",
            status.sentence(&what),
            plan.entity_id,
            plan.realm
        ),
    }
}

/// What landed, once the export confirms it.
pub fn outcome_lines(plan: &CertPlan) -> Vec<String> {
    let mut lines = vec![format!(
        "updated {}: the export now publishes exactly the planned certificates",
        plan.entity_id
    )];
    for change in &plan.changes {
        lines.push(format!(
            "  {}  {} added, {} removed, {} kept",
            short(change.role),
            change.added.len(),
            change.removed.len(),
            change.kept.len()
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use chrono::TimeZone;

    use super::*;

    const RSA_DER: &[u8] = include_bytes!("../fixtures/cert-rsa.der");
    const RSA_SHA: &str = "353f92d3903677e8efd96420323c35de7f7961c088e9b6f2c1828fc3c3ba8447";
    const PREFIX_PEM: &[u8] = include_bytes!("../fixtures/cert-prefix.crt");
    const PREFIX_SHA: &str = "35897af3b7fba63b3d5fab862b9447522979469562133aced02038fc4d2447c6";
    const EC_PEM: &[u8] = include_bytes!("../fixtures/cert-ec.crt");
    const EC_SHA: &str = "7f3ce2e437673b639c20bc54bc9a152a248b36280a3a32f29a02a6d2066600d9";
    const ENTITY: &str = "https://sts.windows.net/00000000-0000-0000-0000-000000000000/";

    fn der(pem_bytes: &[u8]) -> Vec<u8> {
        pem::read_certificate(pem_bytes).unwrap().der
    }

    /// A key descriptor the way AM's export writes one: unprefixed, `ds:`
    /// declared on the root, base64 wrapped at 76 columns, 4-space indent.
    fn am_key(key_use: Option<&str>, der: &[u8]) -> String {
        let body = STANDARD.encode(der);
        let wrapped = body
            .as_bytes()
            .chunks(76)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let use_attr = key_use.map_or(String::new(), |key_use| format!(" use=\"{key_use}\""));
        format!(
            "        <KeyDescriptor{use_attr}>\n            <ds:KeyInfo>\n                \
             <ds:X509Data>\n                    <ds:X509Certificate>\n{wrapped}\n\
             </ds:X509Certificate>\n                </ds:X509Data>\n            </ds:KeyInfo>\n\
             \x20       </KeyDescriptor>\n"
        )
    }

    fn idp(keys: &[(Option<&str>, &[u8])]) -> String {
        format!(
            "    <IDPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\n\
             {}        <SingleSignOnService Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect\" \
             Location=\"https://login.example.com/saml2\"/>\n    </IDPSSODescriptor>\n",
            keys.iter()
                .map(|(key_use, der)| am_key(*key_use, der))
                .collect::<String>()
        )
    }

    fn sp(keys: &[(Option<&str>, &[u8])]) -> String {
        format!(
            "    <SPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\n\
             {}        <AssertionConsumerService index=\"0\" Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST\" \
             Location=\"https://login.example.com/acs\"/>\n    </SPSSODescriptor>\n",
            keys.iter()
                .map(|(key_use, der)| am_key(*key_use, der))
                .collect::<String>()
        )
    }

    fn document(roles: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<EntityDescriptor \
             xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" \
             xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\"\n  \
             entityID=\"{ENTITY}\">\n{roles}\
             </EntityDescriptor>\n"
        )
        .into_bytes()
    }

    fn target(roles: &[Role]) -> Target {
        Target {
            entity_id: ENTITY.into(),
            realm: "alpha".into(),
            roles: roles.to_vec(),
        }
    }

    fn change(planned: Planned) -> CertPlan {
        match planned {
            Planned::Change(plan) => plan,
            Planned::NoChange(sentence) => panic!("expected a change, got: {sentence}"),
        }
    }

    fn signing_of(xml: &[u8], descriptor: &str) -> Vec<String> {
        metadata::cert_refs(xml)
            .unwrap()
            .into_iter()
            .filter(|cert| {
                cert.descriptor == descriptor && cert.key_use.as_deref() == Some(SIGNING_USE)
            })
            .map(|cert| cert.sha256)
            .collect()
    }

    fn stub(location: Location, roles: &[&str]) -> EntityStub {
        EntityStub {
            id64: crate::saml::spec::entity_id64(ENTITY),
            entity_id: ENTITY.into(),
            location,
            roles: roles.iter().map(|role| (*role).to_string()).collect(),
        }
    }

    #[test]
    fn only_a_remote_entity_is_a_target_and_hosted_points_at_rotate() {
        let remote = target_ok(
            ENTITY,
            &[stub(Location::Remote, &["identityProvider"])],
            "alpha",
        )
        .unwrap();
        assert_eq!(remote.roles, [Role::Idp]);

        let hosted = target_ok(
            ENTITY,
            &[stub(Location::Hosted, &["serviceProvider"])],
            "alpha",
        )
        .unwrap_err()
        .to_string();
        assert!(
            hosted.contains("hosted") && hosted.contains("aic saml rotate"),
            "{hosted}"
        );

        let missing = target_ok(ENTITY, &[], "alpha").unwrap_err().to_string();
        assert!(missing.contains("no SAML entity"), "{missing}");

        let both = target_ok(
            ENTITY,
            &[
                stub(Location::Hosted, &["serviceProvider"]),
                stub(Location::Remote, &["identityProvider"]),
            ],
            "alpha",
        );
        assert!(both.is_err());
    }

    #[test]
    fn adding_then_removing_a_certificate_sends_back_the_original_export() {
        let export = document(&idp(&[(Some("signing"), &der(EC_PEM))]));
        let added =
            change(plan_add(&target(&[Role::Idp]), &export, RSA_DER, RSA_SHA, None).unwrap());
        assert_eq!(
            signing_of(&added.document, "IDPSSODescriptor"),
            [EC_SHA, RSA_SHA]
        );
        assert_eq!(added.expected, published(&added.document).unwrap());
        assert_eq!(added.changes[0].added[0].sha256, RSA_SHA);
        assert_eq!(added.changes[0].kept[0].sha256, EC_SHA);

        let removed =
            change(plan_remove(&target(&[Role::Idp]), &added.document, "353f", None).unwrap());
        assert_eq!(removed.document, export);
    }

    #[test]
    fn adding_a_certificate_the_role_already_signs_with_is_a_no_op() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let planned = plan_add(&target(&[Role::Idp]), &export, RSA_DER, RSA_SHA, None).unwrap();
        let Planned::NoChange(sentence) = planned else {
            panic!("expected no change");
        };
        assert!(sentence.contains("already publishes"), "{sentence}");
    }

    /// Discriminating: an encryption certificate is still a certificate, and a
    /// guard that counted certificates rather than signing ones would let this
    /// through and leave the IdP with nothing to verify signatures against.
    #[test]
    fn removing_the_last_signing_certificate_refuses_even_with_an_encryption_one_left() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let error = plan_remove(&target(&[Role::Idp]), &export, RSA_SHA, None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no signing certificate")
                && error.contains("no --force")
                && error.contains("aic saml delete"),
            "{error}"
        );
    }

    /// A `use`-less key serves both purposes by the schema, but what AM does
    /// with one for signing is not measured, so it is not counted as the
    /// signing certificate a role must keep.
    #[test]
    fn a_key_with_no_use_does_not_count_as_the_remaining_signing_certificate() {
        let export = document(&idp(&[(Some("signing"), RSA_DER), (None, &der(EC_PEM))]));
        assert!(plan_remove(&target(&[Role::Idp]), &export, RSA_SHA, None).is_err());
    }

    #[test]
    fn an_ambiguous_prefix_refuses_and_a_longer_one_resolves() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(PREFIX_PEM)),
        ]));
        let error = plan_remove(&target(&[Role::Idp]), &export, "35", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("matches 2 signing certificates"), "{error}");
        let plan = change(plan_remove(&target(&[Role::Idp]), &export, "35:3F", None).unwrap());
        assert_eq!(signing_of(&plan.document, "IDPSSODescriptor"), [PREFIX_SHA]);
        assert!(plan_remove(&target(&[Role::Idp]), &export, "zz", None).is_err());
        assert!(plan_remove(&target(&[Role::Idp]), &export, "ffff", None).is_err());
    }

    #[test]
    fn only_signing_certificates_are_removable() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(PREFIX_PEM)),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let error = plan_remove(&target(&[Role::Idp]), &export, EC_SHA, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Only signing certificates"), "{error}");
    }

    #[test]
    fn a_dual_role_entity_needs_a_role_and_changes_only_that_one() {
        let export = document(&format!(
            "{}{}",
            idp(&[(Some("signing"), RSA_DER)]),
            sp(&[
                (Some("signing"), RSA_DER),
                (Some("encryption"), &der(EC_PEM))
            ])
        ));
        let both = target(&[Role::Idp, Role::Sp]);
        let error = plan_add(&both, &export, &der(PREFIX_PEM), PREFIX_SHA, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--role idp or --role sp"), "{error}");
        let plan =
            change(plan_add(&both, &export, &der(PREFIX_PEM), PREFIX_SHA, Some(Role::Sp)).unwrap());
        assert_eq!(signing_of(&plan.document, "IDPSSODescriptor"), [RSA_SHA]);
        assert_eq!(
            signing_of(&plan.document, "SPSSODescriptor"),
            [RSA_SHA, PREFIX_SHA]
        );

        // The same certificate signs for both roles: removing it needs a role.
        let error = plan_remove(&both, &plan.document, RSA_SHA, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("both roles"), "{error}");
        let removed = change(plan_remove(&both, &plan.document, RSA_SHA, Some(Role::Sp)).unwrap());
        assert_eq!(signing_of(&removed.document, "IDPSSODescriptor"), [RSA_SHA]);
        assert_eq!(
            signing_of(&removed.document, "SPSSODescriptor"),
            [PREFIX_SHA]
        );
    }

    #[test]
    fn a_document_naming_a_role_the_list_does_not_show_is_refused() {
        // The export shows an SP role the realm list does not: sending it
        // could add a role, so the guard reads the list, not the export.
        let export = document(&format!(
            "{}{}",
            idp(&[(Some("signing"), RSA_DER)]),
            sp(&[(Some("signing"), RSA_DER)])
        ));
        let error = plan_add(
            &target(&[Role::Idp]),
            &export,
            &der(EC_PEM),
            EC_SHA,
            Some(Role::Idp),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("SPSSODescriptor"), "{error}");
    }

    #[test]
    fn a_signed_export_is_not_spliced() {
        let entra = include_bytes!("../fixtures/entra-federationmetadata.xml");
        let entity = metadata::key_layout(entra).unwrap().entity_id;
        let error = plan_add(
            &Target {
                entity_id: entity,
                realm: "alpha".into(),
                roles: vec![Role::Idp],
            },
            entra,
            RSA_DER,
            RSA_SHA,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("ds:Signature"), "{error}");
    }

    #[test]
    fn a_key_descriptor_holding_two_certificates_is_not_split() {
        let export = String::from_utf8(document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(EC_PEM)),
        ])))
        .unwrap();
        // Move the EC certificate into the RSA key descriptor's X509Data.
        let ec = STANDARD.encode(der(EC_PEM));
        let merged = export.replacen(
            "</ds:X509Data>",
            &format!("<ds:X509Certificate>{ec}</ds:X509Certificate></ds:X509Data>"),
            1,
        );
        let error = plan_remove(&target(&[Role::Idp]), merged.as_bytes(), RSA_SHA, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("shares one <KeyDescriptor>"), "{error}");
    }

    fn file(roles: &str) -> Vec<u8> {
        document(roles)
    }

    #[test]
    fn replace_keeps_encryption_keys_byte_for_byte_and_drops_other_signing_ones() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let upload = file(&idp(&[(Some("signing"), &der(PREFIX_PEM))]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        let plan = change(plan_import(&comparison, ImportMode::Replace).unwrap());
        assert_eq!(signing_of(&plan.document, "IDPSSODescriptor"), [PREFIX_SHA]);
        let encryption = am_key(Some("encryption"), &der(EC_PEM));
        assert!(
            String::from_utf8(plan.document.clone())
                .unwrap()
                .contains(&encryption),
            "the encryption key descriptor must survive byte for byte"
        );
        assert_eq!(plan.changes[0].removed[0].sha256, RSA_SHA);
        assert_eq!(plan.changes[0].added[0].sha256, PREFIX_SHA);
        assert!(plan.changes[0].kept.is_empty());

        // Add, over the same file, keeps the RSA certificate as well.
        let add = change(plan_import(&comparison, ImportMode::Add).unwrap());
        assert_eq!(
            signing_of(&add.document, "IDPSSODescriptor"),
            [RSA_SHA, PREFIX_SHA]
        );
        assert!(add.changes[0].removed.is_empty());
    }

    #[test]
    fn add_deduplicates_by_fingerprint_and_a_file_already_in_place_is_no_change() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(EC_PEM)),
        ]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        let plan = change(plan_import(&comparison, ImportMode::Add).unwrap());
        assert_eq!(
            signing_of(&plan.document, "IDPSSODescriptor"),
            [RSA_SHA, EC_SHA]
        );

        let same = file(&idp(&[(Some("signing"), RSA_DER)]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &same).unwrap();
        for mode in [ImportMode::Add, ImportMode::Replace] {
            assert!(matches!(
                plan_import(&comparison, mode).unwrap(),
                Planned::NoChange(_)
            ));
        }
        assert_eq!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    #[test]
    fn replace_with_a_file_that_has_no_signing_certificate_refuses() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&idp(&[(Some("encryption"), &der(EC_PEM))]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        assert!(plan_import(&comparison, ImportMode::Replace).is_err());
        // The encryption certificate is reported as not applied.
        assert_eq!(
            comparison.differences.ignored_keys,
            [format!("idp encryption {EC_SHA}")]
        );
    }

    #[test]
    fn a_file_role_the_entity_lacks_is_refused() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&format!(
            "{}{}",
            idp(&[(Some("signing"), RSA_DER)]),
            sp(&[(Some("signing"), RSA_DER)])
        ));
        let error = compare_import(&target(&[Role::Idp]), &export, &upload)
            .unwrap_err()
            .to_string();
        assert!(error.contains("serviceProvider role"), "{error}");
    }

    /// The wording is pinned: it is what stops an operator believing an
    /// endpoint change in the file was applied.
    #[test]
    fn non_certificate_differences_are_reported_as_not_applied() {
        assert_eq!(
            NOT_APPLIED,
            "not applied — AIC ignores non-certificate changes on an update; changing them \
             means delete and re-import, which drops the entity's circle-of-trust membership"
        );
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = String::from_utf8(file(&idp(&[(Some("signing"), &der(EC_PEM))])))
            .unwrap()
            .replace(
                "https://login.example.com/saml2",
                "https://login.example.com/moved",
            );
        let comparison = compare_import(&target(&[Role::Idp]), &export, upload.as_bytes()).unwrap();
        let lines = comparison.differences.lines();
        assert_eq!(lines[0], format!("other differences: {NOT_APPLIED}"));
        assert!(lines[1].starts_with("  file only    IDPSSODescriptor: SingleSignOnService"));
        assert!(lines[1].contains("/moved"));
        assert!(lines[2].starts_with("  tenant only  IDPSSODescriptor: SingleSignOnService"));
        // And the plan still contains only the certificate change.
        let plan = change(plan_import(&comparison, ImportMode::Replace).unwrap());
        assert!(!String::from_utf8(plan.document).unwrap().contains("/moved"));
    }

    #[test]
    fn the_file_must_name_the_entity() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = String::from_utf8(file(&idp(&[(Some("signing"), RSA_DER)])))
            .unwrap()
            .replace(ENTITY, "https://sp-b.example.com");
        assert!(compare_import(&target(&[Role::Idp]), &export, upload.as_bytes()).is_err());
    }

    #[test]
    fn the_recheck_refuses_an_export_that_moved() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let plan =
            change(plan_add(&target(&[Role::Idp]), &export, &der(EC_PEM), EC_SHA, None).unwrap());
        assert!(recheck_ok(&plan, &target(&[Role::Idp]), &export).is_ok());
        let mut moved = export.clone();
        moved.push(b'\n');
        let error = recheck_ok(&plan, &target(&[Role::Idp]), &moved)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Nothing was sent"), "{error}");
    }

    /// A set, not a count: two published and two expected is not settled when
    /// they are different two.
    #[test]
    fn settlement_is_the_exact_role_use_fingerprint_set() {
        let key = |descriptor: &str, key_use: &str, sha: &str| CertKey {
            descriptor: descriptor.into(),
            key_use: Some(key_use.into()),
            sha256: sha.into(),
        };
        let expected = BTreeSet::from([
            key("IDPSSODescriptor", "signing", RSA_SHA),
            key("IDPSSODescriptor", "signing", EC_SHA),
        ]);
        let wrong = BTreeSet::from([
            key("IDPSSODescriptor", "signing", RSA_SHA),
            key("IDPSSODescriptor", "signing", PREFIX_SHA),
        ]);
        assert!(!settled(&wrong, &expected));
        let wrong_use = BTreeSet::from([
            key("IDPSSODescriptor", "signing", RSA_SHA),
            key("IDPSSODescriptor", "encryption", EC_SHA),
        ]);
        assert!(!settled(&wrong_use, &expected));
        assert!(settled(&expected, &expected));
        let gap = settlement_gap(&wrong, &expected);
        assert!(gap.contains(EC_SHA) && gap.contains(PREFIX_SHA), "{gap}");
    }

    #[test]
    fn the_consent_gate_names_the_flag_and_a_dry_run_holds_no_permit() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let plan =
            change(plan_add(&target(&[Role::Idp]), &export, &der(EC_PEM), EC_SHA, None).unwrap());
        let error = write_ok(false, &plan, "--certs replace")
            .unwrap_err()
            .to_string();
        assert!(error.contains("1 added, 0 removed") && error.contains("--certs replace"));
        assert!(write_ok(true, &plan, "--force").is_ok());
        assert!(matches!(authorize(true, &plan), Decision::Preview));
        assert!(matches!(authorize(false, &plan), Decision::Send(_)));
    }

    #[test]
    fn a_failure_that_may_have_landed_says_to_list_before_retrying() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let plan =
            change(plan_add(&target(&[Role::Idp]), &export, &der(EC_PEM), EC_SHA, None).unwrap());
        let source = Error::Config("boom".into());
        for status in [WriteStatus::Unknown, WriteStatus::AcceptedUnverified] {
            let message = failure_message(status, &source, &plan);
            assert!(
                message.contains(&format!("aic saml cert list {ENTITY} --realm alpha"))
                    && message.ends_with("boom"),
                "{message}"
            );
        }
        assert_eq!(
            failure_message(WriteStatus::Refused, &source, &plan),
            source.to_string()
        );
    }

    #[test]
    fn the_list_flags_expiry_against_the_clock_it_is_given() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let rows = cert_rows(&export, None).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 0, 0, 0).unwrap();
        let lines = list_lines(ENTITY, &rows, now);
        assert_eq!(lines[0], format!("idp signing  sha256 {RSA_SHA}"));
        assert!(
            lines.iter().any(|line| line.contains("(364 days left)")),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("EXPIRING SOON — 29 days left"))
        );
        assert!(lines.iter().any(|line| line == "  key        RSA 2048"));
        assert!(lines.iter().any(|line| line == "  key        EC 256"));

        let later = Utc.with_ymd_and_hms(2027, 10, 1, 0, 0, 0).unwrap();
        assert!(
            list_lines(ENTITY, &rows, later)
                .iter()
                .any(|line| line.contains("EXPIRED"))
        );

        let json = list_json(&rows, now);
        assert_eq!(json[0]["role"], "idp");
        assert_eq!(json[0]["use"], "signing");
        assert_eq!(json[0]["sha256"], RSA_SHA);
        assert_eq!(json[0]["keyAlgorithm"], "RSA");
        assert_eq!(json[0]["keySize"], 2048);
        assert_eq!(json[0]["daysUntilExpiry"], 364);
        assert_eq!(json[0]["expiry"], "valid");
        assert_eq!(json[1]["use"], "encryption");
        assert_eq!(json[1]["expiry"], "expiring-soon");

        assert!(cert_rows(&export, Some(Role::Sp)).unwrap().is_empty());
    }
}
