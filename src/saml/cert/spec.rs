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

use crate::saml::metadata::{self, Edit, KeyEntry, KeyLayout, RoleKeys};
use crate::saml::pem::{self, CertDetails};
use crate::saml::spec::{
    EntityStub, Located, Location, Role, SIGNING_USE, locate, role_descriptor,
};
use crate::saml::write::{Decision, WriteFailure};
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

pub(super) fn role_of(role: metadata::Role) -> Role {
    match role {
        metadata::Role::IdentityProvider => Role::Idp,
        metadata::Role::ServiceProvider => Role::Sp,
    }
}

/// A key descriptor's `use`, as `cert list` names it.
pub(super) fn use_label(key_use: Option<&str>) -> &str {
    key_use.unwrap_or("unspecified")
}

pub(super) fn is_signing(key: &KeyEntry) -> bool {
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
    serializer.serialize_str(role.cli_word())
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
            row.role.cli_word(),
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
    pub(super) fn from_der(sha256: &str, der: &[u8]) -> Self {
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
    /// The roles the realm's list showed when this plan was computed, which
    /// [`finish`] checked the document against. Private for the same reason
    /// as `base`: the recheck compares a fresh list with it, because a role
    /// the list stops showing is one the update would add back whole.
    roles: BTreeSet<Role>,
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
pub(super) fn editable(export: &[u8], entity_id: &str) -> Result<KeyLayout> {
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

pub(super) fn role_keys(layout: &KeyLayout, role: Role) -> &RoleKeys {
    layout
        .roles
        .iter()
        .find(|keys| role_of(keys.role) == role)
        .expect("choose_role returned a role the layout holds")
}

/// Signing certificates as `(sha256, der)`, in document order.
pub(super) type SigningCerts = Vec<(String, Vec<u8>)>;

/// The signing certificates a role publishes.
pub(super) fn signing_certs(role: &RoleKeys) -> SigningCerts {
    role.keys
        .iter()
        .filter(|key| is_signing(key))
        .flat_map(|key| &key.certs)
        .map(|(cert, der)| (cert.sha256.clone(), der.clone()))
        .collect()
}

pub(super) fn insert(role: &RoleKeys, der: &[u8], entity_id: &str) -> Result<Edit> {
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
            role.cli_word()
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
pub(super) fn finish(
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
                role.cli_word()
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
        roles: target.roles.iter().copied().collect(),
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
            change.role.cli_word(),
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
/// would change. `--force` is the one escape, for `add`, `remove` and an
/// import alike.
pub fn write_ok(confirmed: bool, plan: &CertPlan) -> Result<()> {
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
         at a terminal, or pass --force.",
        plan.entity_id
    )))
}

/// The pre-write recheck: the entity is still remote, the realm's list shows
/// exactly the roles it showed at planning time, and the export is
/// byte-identical to the one the plan was computed from. Anything else means
/// the plan describes an entity that no longer exists.
///
/// The roles are not implied by the export. They come from the entity list,
/// and `UPDATE_CERTIFICATES` adds a role the entity lacks whole, endpoints and
/// all (measured) — so a role removed between the plan and the write would be
/// recreated by it, from an export that had not changed a byte.
pub fn recheck_ok(plan: &CertPlan, fresh_target: &Target, fresh_export: &[u8]) -> Result<()> {
    if fresh_target.entity_id != plan.entity_id {
        return Err(Error::Config(format!(
            "{} changed identity between the plan and the write; refusing",
            plan.entity_id
        )));
    }
    let fresh_roles = fresh_target.roles.iter().copied().collect::<BTreeSet<_>>();
    if fresh_roles != plan.roles {
        let words = |roles: &BTreeSet<Role>| {
            roles
                .iter()
                .map(|role| role.wire())
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(Error::Config(format!(
            "the realm's entity list shows {} holding roles [{}], not the [{}] this plan was \
             computed against; sending it could add a role back whole. Nothing was sent; run \
             the command again to plan from what is there now.",
            plan.entity_id,
            words(&fresh_roles),
            words(&plan.roles)
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
pub fn failure_message(failure: &WriteFailure, plan: &CertPlan) -> String {
    failure.message(
        &format!("the certificate update of {}", plan.entity_id),
        &format!(
            "Read the tenant before retrying: `aic saml cert list {} --realm {}` shows what it \
             publishes now.",
            plan.entity_id, plan.realm
        ),
    )
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
            change.role.cli_word(),
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
    use crate::saml::cert::testdoc::*;

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

    /// The export is byte-identical in every case, so only the roles can
    /// refuse: a role gone from the list, a role added to it, both swapped.
    #[test]
    fn the_recheck_refuses_roles_that_changed_under_an_identical_export() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let plan =
            change(plan_add(&target(&[Role::Idp]), &export, &der(EC_PEM), EC_SHA, None).unwrap());
        for roles in [vec![], vec![Role::Sp], vec![Role::Idp, Role::Sp]] {
            let error = recheck_ok(&plan, &target(&roles), &export)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("roles") && error.contains("Nothing was sent"),
                "{roles:?}: {error}"
            );
        }
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
        let error = write_ok(false, &plan).unwrap_err().to_string();
        assert!(error.contains("1 added, 0 removed") && error.contains("--force"));
        assert!(write_ok(true, &plan).is_ok());
        assert!(matches!(authorize(true, &plan), Decision::Preview));
        assert!(matches!(authorize(false, &plan), Decision::Send(_)));
    }

    #[test]
    fn a_failure_that_may_have_landed_says_to_list_before_retrying() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let plan =
            change(plan_add(&target(&[Role::Idp]), &export, &der(EC_PEM), EC_SHA, None).unwrap());
        let boom = || Error::Config("boom".into());
        for failure in [
            WriteFailure::from_send(boom(), &[]),
            WriteFailure::unverified(boom()),
        ] {
            let message = failure_message(&failure, &plan);
            assert!(
                message.contains(&format!("aic saml cert list {ENTITY} --realm alpha"))
                    && message.ends_with("boom"),
                "{message}"
            );
        }
        assert_eq!(
            failure_message(&WriteFailure::before_send(boom()), &plan),
            "Config error: boom"
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
