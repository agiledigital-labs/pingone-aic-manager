//! Tenant-free input and projection types for `aic saml`.
//!
//! Everything here is pure: given a list of entity stubs, or the bytes of a
//! metadata-export response, it decides what the CLI should do. Nothing in
//! this module reads a file, opens a socket, or needs a bearer, so the rules
//! that matter — the base64url id encoding, which collection an entity lives
//! in, and above all *whether an export actually succeeded* — are testable
//! without a tenant.
//!
//! See `docs/api/06-saml.md` for the measurements these encode.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::saml::metadata;

/// Which collection an entity lives in. AM exposes `hosted` and `remote` as
/// separate sub-collections and the read path needs the right one; a guess
/// answers 404, which reads as "no such entity".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Location {
    Hosted,
    Remote,
}

impl Location {
    /// The URL path segment, which is also the wire value of `location`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hosted => "hosted",
            Self::Remote => "remote",
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A SAML role, as `aic saml list --role` spells it.
///
/// Distinct from [`metadata::Role`], which names the XML descriptor elements.
/// This one is the short CLI word and the wire string AM puts in `roles`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum Role {
    Idp,
    Sp,
}

impl Role {
    /// The value AM uses inside a stub's `roles` array.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Idp => "identityProvider",
            Self::Sp => "serviceProvider",
        }
    }
}

/// One row of `…/realm-config/saml2?_queryFilter=true`.
///
/// `roles` is **absent**, not `[]`, on an entity with no role blocks
/// (`docs/api/06-saml.md`), so it defaults rather than being required — a
/// roleless entity is legal and must list, not abort the whole command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityStub {
    #[serde(rename = "_id")]
    pub id64: String,
    #[serde(rename = "entityId")]
    pub entity_id: String,
    pub location: Location,
    #[serde(default)]
    pub roles: Vec<String>,
}

impl EntityStub {
    /// Whether this entity holds `role`. An entity can hold **both**, so this
    /// is membership, never equality against a single value.
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.iter().any(|held| held == role.wire())
    }

    /// `roles` rendered for a table cell. Empty is a real state, not a gap.
    pub fn roles_cell(&self) -> String {
        if self.roles.is_empty() {
            "-".to_string()
        } else {
            self.roles.join(",")
        }
    }
}

/// The entity's `_id`: its entity ID base64url-encoded **without** padding.
///
/// Verified against live ids (`docs/api/06-saml.md`). Only surrounding
/// whitespace is trimmed — a trailing `/` is part of the entity ID (Entra's
/// `https://sts.windows.net/<guid>/` has one) and encoding it away addresses a
/// different entity.
pub fn entity_id64(entity_id: &str) -> String {
    URL_SAFE_NO_PAD.encode(entity_id.trim().as_bytes())
}

/// What a list lookup by entity ID found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// Exactly one stub matched; use this collection.
    In(Location),
    /// No stub matched. The caller knows the realm it searched and says so.
    NotFound,
    /// The same entity ID appears in more than one collection. Not observed
    /// live, but a guess here would silently read the wrong document, so it is
    /// surfaced rather than resolved by precedence.
    Ambiguous(Vec<Location>),
}

/// Infer which collection holds `entity_id`, from one cheap list call.
///
/// This exists because `GET …/saml2/{location}/{id64}` answers **404** for a
/// right id in the wrong collection, which reads as "that entity does not
/// exist" — the footgun `--location` would otherwise leave to the operator.
///
/// Entity IDs are compared exactly after `trim()`: AM stores them verbatim and
/// the `_id` is derived from the exact bytes, so case-folding or trimming a
/// trailing `/` here would find an entity whose document lives elsewhere.
pub fn locate(entity_id: &str, stubs: &[EntityStub]) -> Located {
    let wanted = entity_id.trim();
    let mut found: Vec<Location> = Vec::new();
    for stub in stubs {
        if stub.entity_id.trim() == wanted && !found.contains(&stub.location) {
            found.push(stub.location);
        }
    }
    match found.len() {
        0 => Located::NotFound,
        1 => Located::In(found[0]),
        _ => Located::Ambiguous(found),
    }
}

/// Apply `aic saml list`'s two filters and sort by entity ID.
pub fn select(
    stubs: Vec<EntityStub>,
    location: Option<Location>,
    role: Option<Role>,
) -> Vec<EntityStub> {
    let mut selected = stubs
        .into_iter()
        .filter(|stub| location.is_none_or(|wanted| stub.location == wanted))
        .filter(|stub| role.is_none_or(|wanted| stub.has_role(wanted)))
        .collect::<Vec<_>>();
    selected.sort_by(|a, b| {
        a.entity_id
            .to_lowercase()
            .cmp(&b.entity_id.to_lowercase())
            .then_with(|| a.entity_id.cmp(&b.entity_id))
    });
    selected
}

/// The table `aic saml list` prints.
pub const LIST_HEADERS: [&str; 3] = ["ENTITY ID", "LOCATION", "ROLES"];

pub fn list_rows(stubs: &[EntityStub]) -> Vec<Vec<String>> {
    stubs
        .iter()
        .map(|stub| {
            vec![
                stub.entity_id.clone(),
                stub.location.as_str().to_string(),
                stub.roles_cell(),
            ]
        })
        .collect()
}

/// What `aic saml show` prints when the operator did not ask for raw JSON.
///
/// A full entity document is ~100 lines of nested groups whose keys are
/// present-but-empty when nothing in them is set (`docs/api/06-saml.md`), so
/// "the key exists" says nothing. This projection reads only the leaves that
/// do mean something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitySummary {
    pub entity_id: String,
    pub location: Location,
    pub roles: Vec<RoleDetail>,
}

/// One configured role block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDetail {
    /// The wire key the block appeared under.
    pub role: &'static str,
    /// `services.metaAlias` — the SP/IdP routing key every published endpoint
    /// URL embeds. Absent on a remote entity, which has no local endpoints.
    pub meta_alias: Option<String>,
    /// The secret-label namespace this role signs with. `None` means the
    /// role-wide tenant defaults, which is what every live sandbox entity uses.
    pub secret_id_identifier: Option<String>,
}

/// The two role keys, in the order AM lists them in a stub.
const ROLE_KEYS: [&str; 2] = ["identityProvider", "serviceProvider"];

/// Project a full entity read.
///
/// `roles` is **derived** from which role blocks are present — there is no
/// `roles` field on a full read, only on the list stub — so an entity with no
/// role blocks summarises to no roles, which is its real (legal, inert) state.
pub fn summarise(entity: &serde_json::Value, location: Location) -> EntitySummary {
    EntitySummary {
        entity_id: entity
            .get("entityId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        location,
        roles: ROLE_KEYS
            .into_iter()
            .filter_map(|role| {
                let block = entity.get(role)?;
                Some(RoleDetail {
                    role,
                    meta_alias: leaf(block, &["services", "metaAlias"]),
                    // Hosted entities keep it under `signingAndEncryption`;
                    // remote ones under `secrets`. Same string, two homes.
                    secret_id_identifier: leaf(
                        block,
                        &[
                            "assertionContent",
                            "signingAndEncryption",
                            "secretIdAndAlgorithms",
                            "secretIdIdentifier",
                        ],
                    )
                    .or_else(|| {
                        leaf(
                            block,
                            &["assertionContent", "secrets", "secretIdIdentifier"],
                        )
                    }),
                })
            })
            .collect(),
    }
}

/// A string leaf, treating empty as absent. An unset slot is the empty string
/// or a missing key depending on the field, and neither is a value.
fn leaf(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(key)?;
    }
    let text = cursor.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

impl EntitySummary {
    /// The block `aic saml show` prints, one line per element.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("entity id  {}", self.entity_id),
            format!("location   {}", self.location),
        ];
        if self.roles.is_empty() {
            lines.push("roles      none — this entity has no role blocks and is inert".to_string());
            return lines;
        }
        lines.push(format!(
            "roles      {}",
            self.roles
                .iter()
                .map(|role| role.role)
                .collect::<Vec<_>>()
                .join(",")
        ));
        for role in &self.roles {
            lines.push(format!("  {}", role.role));
            lines.push(format!(
                "    metaAlias          {}",
                role.meta_alias.as_deref().unwrap_or("-")
            ));
            lines.push(format!(
                "    secret identifier  {}",
                role.secret_id_identifier
                    .as_deref()
                    .unwrap_or("(realm defaults)")
            ));
        }
        lines
    }
}

/// How the metadata-export JSP answered.
///
/// **The status code carries none of this.** A failed export is HTTP 200 with
/// no `Content-Type` and a plain-text body, so branching on the status reports
/// success for every failure (`docs/api/06-saml.md`). This enum is the only
/// thing between an error message and a file called `entity.xml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportOutcome {
    /// The body is a SAML `<EntityDescriptor>` document.
    Metadata,
    /// The JSP's own `ERROR : …` message, unescaped for reading.
    TenantError(String),
    /// Neither. Never treated as success; the caller shows the prefix.
    Unrecognised(String),
}

/// The JSP's failure prefix. Observed exactly as `ERROR : `, but the space and
/// colon are cosmetic, so the token alone is the test.
const ERROR_PREFIX: &str = "ERROR";

/// Classify an export response body.
///
/// Two signals, in order:
///
/// 1. the body begins with `ERROR` — AM's plain-text failure, which arrives
///    with a 200 and no `Content-Type`;
/// 2. otherwise the body must be one well-formed SAML `<EntityDescriptor>`
///    document, which is what a successful export always is (singular — never
///    an `EntitiesDescriptor`). Well-formed to the end of the file, not just
///    at the root: a start tag with the document truncated after it used to
///    classify as metadata and be written to `--out`.
///
/// Anything else is [`ExportOutcome::Unrecognised`]. Failing closed matters
/// more than being clever here: writing an AM login page or a proxy error to
/// `--out` under an `.xml` name is exactly the outcome the 200 invites.
pub fn classify_export(body: &[u8]) -> ExportOutcome {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim_start_matches('\u{feff}').trim();

    if let Some(rest) = trimmed.strip_prefix(ERROR_PREFIX) {
        return ExportOutcome::TenantError(unescape_message(
            rest.trim_start().trim_start_matches(':').trim(),
        ));
    }
    if metadata::validate_export_document(body).is_ok() {
        return ExportOutcome::Metadata;
    }
    ExportOutcome::Unrecognised(excerpt(trimmed))
}

/// How much of an unrecognised body to quote back. Enough to recognise a login
/// page or a proxy error; not enough to paste a tenant's whole response into a
/// terminal.
const EXCERPT_LIMIT: usize = 200;

fn excerpt(body: &str) -> String {
    if body.is_empty() {
        return "<empty body>".to_string();
    }
    let single_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match single_line.char_indices().nth(EXCERPT_LIMIT) {
        Some((cut, _)) => format!("{}…", &single_line[..cut]),
        None => single_line,
    }
}

/// AM escapes the message, and the entity ID inside it was escaped once
/// already, so the observed body is doubly escaped: `&amp;#x3a;` decodes to
/// `&#x3a;` and only then to `:`. Two passes is what the measured body needs.
///
/// Bounded deliberately at two: this is a human-facing error message, and an
/// unbounded loop would eventually decode a literal `&amp;amp;` a user wrote
/// into their entity ID. Two passes reproduce the encoder; more would invent.
fn unescape_message(message: &str) -> String {
    let once = unescape_once(message);
    unescape_once(&once)
}

fn unescape_once(text: &str) -> String {
    quick_xml::escape::unescape(text)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| text.to_string())
}

/// Path and query for the metadata-export JSP.
///
/// `realm` is always sent: omitting it defaults to the **root** realm, not the
/// current one, so a forgotten realm returns `ERROR :` for an entity that is
/// perfectly present in `bravo`.
pub fn export_path(entity_id: &str, realm: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("entityid", entity_id.trim())
        .append_pair("realm", &format!("/{realm}"))
        .finish();
    format!("/am/saml2/jsp/exportmetadata.jsp?{query}")
}

// ---------------------------------------------------------------------------
// Circles of trust
// ---------------------------------------------------------------------------

/// The sentence every circle-of-trust rendering must carry.
///
/// AM records membership in **two** places: the CoT document's
/// `trustedProviders`, which REST returns, and each entity's `cotlist`
/// extended-metadata attribute, which REST never exposes — and the runtime
/// trust check reads the second one (`docs/api/06-saml.md`). A CLI that prints
/// `trustedProviders` without saying so is confidently wrong in both
/// directions, so this is not decoration: it is the only thing stopping the
/// output being read as the membership that governs authentication.
pub const COT_MEMBERSHIP_CAVEAT: &str = "\
note: this is the circle-of-trust document's own `trustedProviders` list, and it
      is only half of AM's membership record. The other half is the `cotlist`
      attribute in each entity's extended metadata, which REST does not expose.
      The runtime trust check reads `cotlist`, not this list — so a provider
      shown here can still have its assertions rejected, and one missing here
      can still authenticate.";

/// One circle-of-trust document.
///
/// The CoT list endpoint returns **full documents**, not stubs, so this same
/// type serves `cot list` and `cot show`. Three shape notes, all measured
/// (`docs/api/06-saml.md`):
///
/// - `_id` is the **plain name** — only entity providers are base64url-encoded.
/// - `description` is **absent** when unset, not `null` and not `""`. AM has no
///   way to store an empty one, so `Some("")` is not a state that exists.
/// - `_rev` is deliberately not captured. It is useless for drift detection on
///   this family, per `.ai/core.md` §5, and a field nothing may read is a
///   field a later slice will read by accident.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Cot {
    #[serde(rename = "_id")]
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "trustedProviders", default)]
    pub trusted_providers: Vec<String>,
}

/// One parsed `trustedProviders` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedProvider {
    /// The entity ID half, trimmed.
    pub entity_id: String,
    /// The protocol suffix (`saml2`; AM also understands `wsfed`).
    ///
    /// `None` for an entry with no `|` at all. Those are real: AM stores and
    /// removes `garbage-no-pipe` with a cheerful 200 precisely because nothing
    /// can be resolved from it, so an entry without a protocol is inert rather
    /// than invalid, and the renderer says so instead of hiding it.
    pub protocol: Option<String>,
    /// The entry exactly as the tenant stores it.
    pub raw: String,
}

/// Split a `trustedProviders` entry into entity ID and protocol.
///
/// The split is at the **last** `|`, because the protocol is a suffix AM
/// appends to an entity ID it does not escape. Splitting at the first one
/// would mis-read an entity ID that contained a pipe as a bare protocol.
pub fn parse_trusted_provider(entry: &str) -> TrustedProvider {
    let trimmed = entry.trim();
    match trimmed.rsplit_once('|') {
        Some((entity_id, protocol)) => TrustedProvider {
            entity_id: entity_id.trim().to_string(),
            protocol: Some(protocol.trim().to_string()),
            raw: trimmed.to_string(),
        },
        None => TrustedProvider {
            entity_id: trimmed.to_string(),
            protocol: None,
            raw: trimmed.to_string(),
        },
    }
}

impl Cot {
    /// Every `trustedProviders` entry, parsed.
    pub fn members(&self) -> Vec<TrustedProvider> {
        self.trusted_providers
            .iter()
            .map(|entry| parse_trusted_provider(entry))
            .collect()
    }

    /// The entries naming `entity_id`, exactly as stored.
    ///
    /// The comparison is on the parsed **entity-ID half**, not the whole entry
    /// and not a substring of it: `https://sp-a.example.com` is a prefix of
    /// `https://sp-a.example.com.au`, and a substring test would report a
    /// cascade onto a circle of trust that will not change — or, on delete,
    /// promise one that does not happen.
    pub fn entries_naming(&self, entity_id: &str) -> Vec<String> {
        let wanted = entity_id.trim();
        self.members()
            .into_iter()
            .filter(|member| member.entity_id == wanted)
            .map(|member| member.raw)
            .collect()
    }
}

/// One circle of trust that names an entity, and the entries that do it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CotMembership {
    pub cot: String,
    /// A CoT may list the same entity under more than one protocol, so this is
    /// a list — and it names *which* entries, because "how many CoTs" is not
    /// what an operator about to delete an entity needs to read.
    pub entries: Vec<String>,
}

/// Which circles of trust name `entity_id`.
///
/// This is the cascade an entity `DELETE` performs silently: AM edits every
/// CoT document that listed the entity, with no write of our own in between
/// (verified 2026-09-16, `docs/api/06-saml.md`). An operator cannot see it
/// coming from anything the delete command itself prints, so the command has
/// to go and look.
pub fn cots_naming(entity_id: &str, cots: &[Cot]) -> Vec<CotMembership> {
    cots.iter()
        .filter_map(|cot| {
            let entries = cot.entries_naming(entity_id);
            (!entries.is_empty()).then(|| CotMembership {
                cot: cot.name.clone(),
                entries,
            })
        })
        .collect()
}

/// What `aic saml delete` prints before it writes, and what it prints instead
/// of writing when `--force` is absent.
pub fn cascade_lines(entity_id: &str, realm: &str, affected: &[CotMembership]) -> Vec<String> {
    if affected.is_empty() {
        return vec![format!(
            "no circle of trust in realm {realm} lists {entity_id}, \
             so no CoT document changes"
        )];
    }
    let mut lines = vec![format!(
        "deleting {entity_id} also rewrites {} circle(s) of trust in realm {realm}, \
         removing these entries:",
        affected.len()
    )];
    for membership in affected {
        for entry in &membership.entries {
            lines.push(format!("  {}  {entry}", membership.cot));
        }
    }
    lines
}

/// What the cascade actually did, read back rather than assumed.
///
/// The delete response echoes the deleted entity and says nothing about the
/// circles of trust AM rewrote, so a command that printed `before` as the
/// outcome would be reporting a claim, not an observation — the same mistake
/// as snapshotting the bytes you submitted (`.ai/core.md` §5). Every line here
/// comes from a second read.
///
/// A circle that **still** lists the entity is the interesting case: the
/// cascade is documented but it is AM's, not ours, and a federation left
/// naming a provider that no longer exists is worth a warning rather than
/// silence.
pub fn cascade_outcome_lines(
    entity_id: &str,
    before: &[CotMembership],
    after: &[CotMembership],
) -> Vec<String> {
    before
        .iter()
        .map(
            |was| match after.iter().find(|still| still.cot == was.cot) {
                None => format!(
                    "  removed from circle of trust {}: {}",
                    was.cot,
                    was.entries.join(", ")
                ),
                Some(still) => format!(
                    "  warning: circle of trust {} still lists {entity_id} as {} \
                     after the delete",
                    was.cot,
                    still.entries.join(", ")
                ),
            },
        )
        .collect()
}

/// Permission to delete one entity provider: `--force` was supplied.
///
/// Split out of the command the way `cli::prod_write_ok` is, and for the same
/// reason — a rule left inline can only be tested by a test that restates it,
/// and "the caller stopped applying it" is the defect this shape catches.
///
/// The refusal is also the preview: the command has already read the circle-of
/// -trust collection and printed the cascade by the time it asks, so running
/// without `--force` is how an operator finds out what a delete would take
/// with it. That is why there is no `--dry-run` — there is no permission token
/// for a preview to carry by accident.
pub fn delete_ok(
    forced: bool,
    entity_id: &str,
    location: Location,
    tenant: &str,
    realm: &str,
) -> crate::Result<()> {
    if forced {
        return Ok(());
    }
    Err(crate::Error::Config(format!(
        "would delete SAML entity provider {entity_id} ({location}) from {tenant}/{realm}; \
         pass --force to delete it"
    )))
}

/// Parse a CoT list response.
pub fn cots(documents: &[serde_json::Value]) -> crate::Result<Vec<Cot>> {
    documents.iter().map(cot).collect()
}

/// Parse one CoT document.
pub fn cot(document: &serde_json::Value) -> crate::Result<Cot> {
    serde_json::from_value::<Cot>(document.clone()).map_err(|error| crate::Error::Api {
        status: 0,
        body: format!("unexpected circle-of-trust document {document}: {error}"),
    })
}

/// Sort circles of trust by name, the way `select` sorts entities.
pub fn sort_cots(mut cots: Vec<Cot>) -> Vec<Cot> {
    cots.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    cots
}

/// The table `aic saml cot list` prints.
///
/// `PROVIDERS` is a count, which is all a list column can carry — and is
/// exactly why the caveat is printed alongside the table rather than only by
/// `cot show`.
pub const COT_LIST_HEADERS: [&str; 4] = ["NAME", "STATUS", "PROVIDERS", "DESCRIPTION"];

pub fn cot_rows(cots: &[Cot]) -> Vec<Vec<String>> {
    cots.iter()
        .map(|cot| {
            vec![
                cot.name.clone(),
                cot.status.clone().unwrap_or_else(|| "-".to_string()),
                cot.trusted_providers.len().to_string(),
                cot.description.clone().unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect()
}

/// The block `aic saml cot show` prints.
///
/// The last lines are [`COT_MEMBERSHIP_CAVEAT`], unconditionally — including
/// for an empty circle of trust, where "no members" is the reading most likely
/// to be taken as proof that nothing trusts anything.
pub fn cot_show_lines(cot: &Cot) -> Vec<String> {
    let mut lines = vec![
        format!("name         {}", cot.name),
        format!("status       {}", cot.status.as_deref().unwrap_or("-")),
        format!("description  {}", cot.description.as_deref().unwrap_or("-")),
    ];
    let members = cot.members();
    if members.is_empty() {
        lines.push("trusted providers  none".to_string());
    } else {
        lines.push(format!("trusted providers  {}", members.len()));
        for member in members {
            match member.protocol {
                Some(_) => lines.push(format!("  {}", member.raw)),
                None => lines.push(format!(
                    "  {}  (no |protocol suffix — AM never resolves this entry)",
                    member.raw
                )),
            }
        }
    }
    lines.push(String::new());
    lines.push(COT_MEMBERSHIP_CAVEAT.to_string());
    lines
}

/// A circle-of-trust name that is safe to splice into a URL path.
///
/// The CoT resource id is the plain name, so it reaches the path unencoded.
/// Rejecting the separators is the same guard `aic role` applies to its
/// caller-chosen ids, and for the same reason: a name containing `/` would
/// address a different resource entirely.
pub fn validate_cot_name(name: &str) -> crate::Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '?' | '#'))
    {
        return Err(crate::Error::Config(format!(
            "circle-of-trust name {name:?} is empty or contains a URL path separator"
        )));
    }
    Ok(trimmed)
}

// ---------------------------------------------------------------------------
// Creating a hosted entity
// ---------------------------------------------------------------------------

/// What `aic saml create-hosted` was asked for, before any tenant contact.
///
/// TUI-free and tenant-free on purpose: every rule below is one AM does not
/// enforce, so the only place they can be tested is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedCreate {
    pub entity_id: String,
    pub role: Role,
    pub meta_alias: String,
}

/// Build the create body, refusing what AM would accept or mangle.
///
/// Three rules, each a measured AM behaviour the CLI has to cover for
/// (`docs/api/06-saml.md`):
///
/// 1. **`entityId` must be supplied.** `POST …?_action=create` with `{}` is a
///    **201** and AM mints a UUID, leaving an entity in the realm that nothing
///    will ever reference. Only the CLI can prevent that.
/// 2. **`services.metaAlias` must be supplied.** A role block without it fails
///    with `500 Exception from invocation expected to be handled by promise`,
///    which names no field — so there is nothing in the response to translate
///    and the check has to happen before the call.
/// 3. **The alias lives under `/<realm>/`.** Every endpoint AM publishes for
///    the entity embeds it (`/am/AuthConsumer/metaAlias/<realm>/<leaf>`), so an
///    alias in another realm's namespace is wrong in a way nothing reports.
pub fn build_hosted_create(
    realm: &str,
    request: &HostedCreate,
) -> crate::Result<serde_json::Value> {
    let entity_id = request.entity_id.trim();
    if entity_id.is_empty() {
        return Err(crate::Error::Config(
            "an entity id is required: `_action=create` with no `entityId` answers 201 and \
             mints a UUID-named entity, which nothing will reference"
                .into(),
        ));
    }

    let meta_alias = request.meta_alias.trim();
    if meta_alias.is_empty() {
        return Err(crate::Error::Config(format!(
            "--meta-alias is required and must not be empty: AM rejects a `{}` block without \
             `services.metaAlias` with a 500 that names no field",
            request.role.wire()
        )));
    }

    let prefix = format!("/{realm}/");
    let leaf = meta_alias
        .strip_prefix(&prefix)
        .ok_or_else(|| meta_alias_error(realm, meta_alias))?;
    if leaf.is_empty() || leaf.contains('/') {
        return Err(meta_alias_error(realm, meta_alias));
    }

    // Built key by key rather than with one `json!` literal: the role key is
    // chosen at runtime, and a macro that can only take a literal there would
    // push the choice back out to two near-identical call sites.
    let mut body = serde_json::Map::new();
    body.insert("entityId".into(), serde_json::Value::from(entity_id));
    body.insert(
        request.role.wire().to_string(),
        serde_json::json!({ "services": { "metaAlias": meta_alias } }),
    );
    Ok(serde_json::Value::Object(body))
}

fn meta_alias_error(realm: &str, meta_alias: &str) -> crate::Error {
    crate::Error::Config(format!(
        "--meta-alias {meta_alias:?} must be `/{realm}/<name>`: the alias is the entity's \
         routing key and every endpoint AM publishes for it embeds the realm \
         (`/am/AuthConsumer/metaAlias/{realm}/<name>`)"
    ))
}

/// What `aic saml create-hosted` prints, split by who said it.
///
/// The 201 body is a **stub** — `_id`, `_rev`, `entityId` — not the created
/// document, so the only thing here AM confirmed is the id. The role and the
/// alias are what was *sent*, and saying so is the same discipline as
/// re-reading the circles of trust after a delete: a report must not state a
/// post-state nothing measured (`.ai/core.md` §5).
///
/// `assigned` is the `entityId` the 201 carried, if any. Two of its three
/// states are failures the operator has to be told about, because AM's answer
/// to a create it did not like is still a 201:
///
/// - a **different** id means the request's `entityId` did not take, which is
///   precisely the UUID-minting behaviour the CLI exists to prevent;
/// - **no** id means nothing confirmed what was created at all.
pub fn created_lines(
    assigned: Option<&str>,
    request: &HostedCreate,
    tenant: &str,
    realm: &str,
) -> Vec<String> {
    let requested = request.entity_id.trim();
    let named = assigned.map(str::trim).filter(|id| !id.is_empty());
    let mut lines = vec![format!(
        "created hosted SAML entity provider {} in {tenant}/{realm}",
        named.unwrap_or(requested)
    )];
    match named {
        Some(id) if id != requested => lines.push(format!(
            "warning: AM named it {id}, not the requested {requested} — the entityId \
             in the request did not take"
        )),
        None => lines.push(format!(
            "warning: the 201 carried no entityId, so the id of what was created is \
             unconfirmed; the request asked for {requested}"
        )),
        Some(_) => {}
    }
    lines.push(format!(
        "role {} and metaAlias {} are what was sent; the 201 body is a stub, so nothing \
         was read back — `aic saml show {} --realm {realm}` shows what AM stored",
        request.role.wire(),
        request.meta_alias.trim(),
        named.unwrap_or(requested)
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub(entity_id: &str, location: Location, roles: &[&str]) -> EntityStub {
        EntityStub {
            id64: entity_id64(entity_id),
            entity_id: entity_id.to_string(),
            location,
            roles: roles.iter().map(|role| role.to_string()).collect(),
        }
    }

    /// The first row is the live oracle from `docs/api/06-saml.md`: AM minted
    /// that `entityId` and returned that `_id` in the same 201 body. The rest
    /// pin the two properties a reimplementation gets wrong — the URL-safe
    /// alphabet, and that padding is dropped.
    #[test]
    fn entity_id64_matches_the_live_oracle_and_the_alphabet_rules() {
        let cases: [(&str, &str, &str); 5] = [
            (
                "the id AM minted from an empty create body",
                "a8830144-e650-492c-8aa1-b1b9a7ad7ac1",
                "YTg4MzAxNDQtZTY1MC00OTJjLThhYTEtYjFiOWE3YWQ3YWMx",
            ),
            (
                "a plain https entity id",
                "https://sp-a.example.com",
                "aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t",
            ),
            (
                // Entra entity ids end in `/`. It is part of the id: the
                // encoding must change when it is present.
                "a trailing slash is part of the id",
                "https://sp-a.example.com/",
                "aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29tLw",
            ),
            (
                // `?` and `~` both encode to bytes the standard alphabet
                // spells `+` and `/`; the URL-safe one spells them `-` and `_`.
                "the url-safe alphabet, not the standard one",
                "??~~",
                "Pz9-fg",
            ),
            (
                "surrounding whitespace is trimmed, inner text is not",
                "  https://sp-a.example.com  ",
                "aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t",
            ),
        ];

        for (what, entity_id, expected) in cases {
            assert_eq!(entity_id64(entity_id), expected, "{what}");
            assert!(
                !expected.contains('=') && !expected.contains('+') && !expected.contains('/'),
                "{what}: {expected} is not unpadded base64url"
            );
        }
    }

    #[test]
    fn entity_id64_distinguishes_a_trailing_slash() {
        assert_ne!(
            entity_id64("https://sp-a.example.com"),
            entity_id64("https://sp-a.example.com/")
        );
    }

    #[test]
    fn locate_finds_the_collection_and_refuses_to_guess() {
        let stubs = vec![
            stub(
                "https://sp-a.example.com",
                Location::Hosted,
                &["serviceProvider"],
            ),
            stub(
                "https://sts.windows.net/00000000-0000-0000-0000-000000000000/",
                Location::Remote,
                &["identityProvider"],
            ),
        ];

        let cases: [(&str, &str, Located); 5] = [
            (
                "a hosted entity",
                "https://sp-a.example.com",
                Located::In(Location::Hosted),
            ),
            (
                "a remote entity, trailing slash and all",
                "https://sts.windows.net/00000000-0000-0000-0000-000000000000/",
                Located::In(Location::Remote),
            ),
            (
                "surrounding whitespace does not change the answer",
                "  https://sp-a.example.com ",
                Located::In(Location::Hosted),
            ),
            (
                // The discriminating case: dropping the trailing `/` names a
                // different entity, and a lenient match would read the wrong
                // document rather than say so.
                "the same id without its trailing slash is a different entity",
                "https://sts.windows.net/00000000-0000-0000-0000-000000000000",
                Located::NotFound,
            ),
            (
                "an id nobody holds",
                "https://sp-z.example.com",
                Located::NotFound,
            ),
        ];

        for (what, entity_id, expected) in cases {
            assert_eq!(locate(entity_id, &stubs), expected, "{what}");
        }
    }

    #[test]
    fn locate_reports_an_id_held_in_both_collections() {
        let both = vec![
            stub("https://dual.example.com", Location::Hosted, &[]),
            stub("https://dual.example.com", Location::Remote, &[]),
        ];
        assert_eq!(
            locate("https://dual.example.com", &both),
            Located::Ambiguous(vec![Location::Hosted, Location::Remote])
        );
    }

    fn filter_fixture() -> Vec<EntityStub> {
        vec![
            stub(
                "https://sp-a.example.com",
                Location::Hosted,
                &["serviceProvider"],
            ),
            stub(
                "https://both.example.com",
                Location::Hosted,
                &["identityProvider", "serviceProvider"],
            ),
            stub(
                "https://idp-a.example.com",
                Location::Remote,
                &["identityProvider"],
            ),
            stub("https://roleless.example.com", Location::Hosted, &[]),
        ]
    }

    /// The dual-role entity is the control: a filter that compared `roles` to
    /// a single value, or took `roles[0]`, drops it from one of these two rows
    /// and stays green on every other case.
    #[test]
    fn select_filters_on_membership_not_equality() {
        type Case = (
            &'static str,
            Option<Location>,
            Option<Role>,
            &'static [&'static str],
        );
        let cases: [Case; 6] = [
            (
                "no filter lists everything, sorted by entity id",
                None,
                None,
                &[
                    "https://both.example.com",
                    "https://idp-a.example.com",
                    "https://roleless.example.com",
                    "https://sp-a.example.com",
                ],
            ),
            (
                "sp matches the dual-role entity too",
                None,
                Some(Role::Sp),
                &["https://both.example.com", "https://sp-a.example.com"],
            ),
            (
                "idp matches the dual-role entity too",
                None,
                Some(Role::Idp),
                &["https://both.example.com", "https://idp-a.example.com"],
            ),
            (
                "location alone",
                Some(Location::Remote),
                None,
                &["https://idp-a.example.com"],
            ),
            (
                "both filters intersect",
                Some(Location::Hosted),
                Some(Role::Idp),
                &["https://both.example.com"],
            ),
            (
                "a roleless entity matches no role filter",
                Some(Location::Hosted),
                Some(Role::Sp),
                &["https://both.example.com", "https://sp-a.example.com"],
            ),
        ];

        for (what, location, role, expected) in cases {
            let got = select(filter_fixture(), location, role)
                .into_iter()
                .map(|stub| stub.entity_id)
                .collect::<Vec<_>>();
            assert_eq!(got, expected, "{what}");
        }
    }

    #[test]
    fn list_rows_shows_a_roleless_entity_as_a_row_not_a_gap() {
        let rows = list_rows(&select(filter_fixture(), Some(Location::Hosted), None));
        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter().any(|row| row
                == &vec![
                    "https://roleless.example.com".to_string(),
                    "hosted".to_string(),
                    "-".to_string(),
                ]),
            "roleless row missing from {rows:?}"
        );
        assert_eq!(
            rows[0],
            vec![
                "https://both.example.com".to_string(),
                "hosted".to_string(),
                "identityProvider,serviceProvider".to_string(),
            ]
        );
    }

    #[test]
    fn stub_deserialises_with_roles_absent() {
        let stub: EntityStub = serde_json::from_str(
            r#"{"_id":"aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t","_rev":"1725473215",
                "entityId":"https://sp-a.example.com","location":"hosted"}"#,
        )
        .expect("roles is absent, not []");
        assert!(stub.roles.is_empty());
        assert_eq!(stub.location, Location::Hosted);
    }

    /// A full read has **no** `roles` field — roles are derived from which
    /// blocks are present — and every group key is present-but-empty when
    /// nothing in it is set, so a projection that tested key presence would
    /// report configuration that does not exist.
    #[test]
    fn summarise_reads_leaves_not_key_presence() {
        let hosted_sp = serde_json::json!({
            "_id": "aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t",
            "entityId": "https://sp-a.example.com",
            "serviceProvider": {
                "assertionContent": {
                    "signingAndEncryption": {
                        "secretIdAndAlgorithms": { "secretIdIdentifier": "probe3key" },
                        "encryption": {}
                    },
                    "nameIdFormat": {}
                },
                "services": { "metaAlias": "/bravo/client-b-sp" },
                "advanced": { "idpProxy": {} }
            }
        });
        let summary = summarise(&hosted_sp, Location::Hosted);
        assert_eq!(summary.entity_id, "https://sp-a.example.com");
        assert_eq!(
            summary.roles,
            vec![RoleDetail {
                role: "serviceProvider",
                meta_alias: Some("/bravo/client-b-sp".to_string()),
                secret_id_identifier: Some("probe3key".to_string()),
            }]
        );
        // The pure SP has no `identityProvider` key at all — absent, not null.
        assert!(!summary.lines().join("\n").contains("identityProvider"));
    }

    #[test]
    fn summarise_handles_the_shapes_a_sandbox_actually_holds() {
        // Every live sandbox entity carries `secretIdAndAlgorithms: {}`, so
        // the common case is "no identifier" and must not read as one.
        let unset = serde_json::json!({
            "entityId": "https://sp-b.example.com",
            "serviceProvider": {
                "assertionContent": { "signingAndEncryption": { "secretIdAndAlgorithms": {} } },
                "services": { "metaAlias": "/bravo/sp-b" }
            }
        });
        assert_eq!(
            summarise(&unset, Location::Hosted).roles[0].secret_id_identifier,
            None
        );

        // A remote entity keeps the same string somewhere else, and has no
        // metaAlias because it publishes no local endpoints.
        let remote = serde_json::json!({
            "entityId": "https://sts.windows.net/00000000-0000-0000-0000-000000000000/",
            "identityProvider": {
                "assertionContent": { "secrets": { "secretIdIdentifier": "peerkey" } }
            }
        });
        assert_eq!(
            summarise(&remote, Location::Remote).roles,
            vec![RoleDetail {
                role: "identityProvider",
                meta_alias: None,
                secret_id_identifier: Some("peerkey".to_string()),
            }]
        );

        // Both roles, in stub order.
        let dual = serde_json::json!({
            "entityId": "https://both.example.com",
            "serviceProvider": { "services": { "metaAlias": "/bravo/both-sp" } },
            "identityProvider": { "services": { "metaAlias": "/bravo/both-idp" } }
        });
        let roles = summarise(&dual, Location::Hosted).roles;
        assert_eq!(
            roles.iter().map(|role| role.role).collect::<Vec<_>>(),
            vec!["identityProvider", "serviceProvider"]
        );

        // A roleless entity is legal and says so rather than printing nothing.
        let roleless = serde_json::json!({ "entityId": "https://roleless.example.com" });
        let summary = summarise(&roleless, Location::Hosted);
        assert!(summary.roles.is_empty());
        assert!(summary.lines().join("\n").contains("inert"));
    }

    /// The whole point of the classifier. Every body here arrives as HTTP 200,
    /// so a status check calls all of them success.
    #[test]
    fn classify_export_separates_metadata_from_a_200_that_failed() {
        const DESCRIPTOR: &str = concat!(
            r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata" "#,
            r#"entityID="https://sp-a.example.com"/>"#
        );

        let cases: [(&str, String, ExportOutcome); 10] = [
            (
                "a real export",
                DESCRIPTOR.to_string(),
                ExportOutcome::Metadata,
            ),
            (
                "an export with an XML declaration in front of it",
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{DESCRIPTOR}"),
                ExportOutcome::Metadata,
            ),
            (
                // A roleless entity exports a bare self-closing descriptor
                // with no entityID. Legal, inert, and still a successful
                // export — a stricter check would report it as a failure.
                "a roleless entity's bare descriptor",
                r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata"/>"#.to_string(),
                ExportOutcome::Metadata,
            ),
            (
                "AM's failure body, doubly escaped as it arrives",
                "ERROR : No metadata for entity &quot;https&amp;#x3a;&amp;#x2f;&amp;#x2f;sp-z.example.com&quot; \
                 under realm &quot;&amp;#x2f;bravo&quot; found."
                    .to_string(),
                ExportOutcome::TenantError(
                    "No metadata for entity \"https://sp-z.example.com\" under realm \"/bravo\" found."
                        .to_string(),
                ),
            ),
            (
                "a leading BOM does not hide the failure",
                "\u{feff}ERROR : nope".to_string(),
                ExportOutcome::TenantError("nope".to_string()),
            ),
            (
                // Not a descriptor: an `EntitiesDescriptor` wrapper is what a
                // federation aggregate looks like, and the JSP never emits one.
                "an EntitiesDescriptor is not what this endpoint returns",
                r#"<EntitiesDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata"/>"#.to_string(),
                ExportOutcome::Unrecognised(
                    r#"<EntitiesDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata"/>"#
                        .to_string(),
                ),
            ),
            (
                "an HTML login page written to --out would be the real damage",
                "<html><head><title>Sign in</title></head></html>".to_string(),
                ExportOutcome::Unrecognised(
                    "<html><head><title>Sign in</title></head></html>".to_string(),
                ),
            ),
            (
                "an empty body",
                String::new(),
                ExportOutcome::Unrecognised("<empty body>".to_string()),
            ),
            (
                // The row this table was green without. A correctly
                // namespaced start tag and then nothing — a truncated
                // response, a proxy that cut the body — used to classify as
                // metadata, and the CLI wrote it to --out under an .xml name.
                "a descriptor the response was truncated after",
                r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata">"#.to_string(),
                ExportOutcome::Unrecognised(
                    r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata">"#.to_string(),
                ),
            ),
            (
                // Same shape, one document further on: two descriptors
                // concatenated are not one entity's metadata either.
                "a second descriptor appended to the first",
                format!("{DESCRIPTOR}{DESCRIPTOR}"),
                ExportOutcome::Unrecognised(format!("{DESCRIPTOR}{DESCRIPTOR}")),
            ),
        ];

        for (what, body, expected) in cases {
            assert_eq!(classify_export(body.as_bytes()), expected, "{what}");
        }
    }

    #[test]
    fn classify_export_truncates_a_long_unrecognised_body() {
        let body = "x".repeat(EXCERPT_LIMIT * 2);
        let ExportOutcome::Unrecognised(excerpt) = classify_export(body.as_bytes()) else {
            panic!("a wall of x is not metadata");
        };
        assert!(excerpt.ends_with('…'));
        assert_eq!(excerpt.chars().count(), EXCERPT_LIMIT + 1);
    }

    #[test]
    fn export_path_always_sends_the_realm_and_encodes_the_entity_id() {
        assert_eq!(
            export_path("https://sp-a.example.com", "bravo"),
            "/am/saml2/jsp/exportmetadata.jsp\
             ?entityid=https%3A%2F%2Fsp-a.example.com&realm=%2Fbravo"
        );
        // An `&` in an entity id must not become a second query parameter.
        assert_eq!(
            export_path("https://sp.example.com/?a=1&realm=/alpha", "bravo"),
            "/am/saml2/jsp/exportmetadata.jsp\
             ?entityid=https%3A%2F%2Fsp.example.com%2F%3Fa%3D1%26realm%3D%2Falpha&realm=%2Fbravo"
        );
    }
    fn cot_doc(name: &str, providers: &[&str]) -> Cot {
        Cot {
            name: name.to_string(),
            status: Some("active".to_string()),
            description: None,
            trusted_providers: providers.iter().map(|entry| entry.to_string()).collect(),
        }
    }

    /// The split is at the **last** `|`, not the first. The fourth row is the
    /// discriminator: an entity ID that itself contains a pipe comes out whole
    /// only if the protocol is taken as a suffix.
    #[test]
    fn a_trusted_provider_entry_splits_at_its_protocol_suffix() {
        let cases: [(&str, &str, &str, Option<&str>); 5] = [
            (
                "the live shape",
                "https://sp-b.example.com|saml2",
                "https://sp-b.example.com",
                Some("saml2"),
            ),
            (
                "AM also understands wsfed",
                "https://idp-a.example.com|wsfed",
                "https://idp-a.example.com",
                Some("wsfed"),
            ),
            (
                // Stored and removed with a 200 because nothing can be
                // resolved from it. Inert, not invalid.
                "an entry with no protocol suffix",
                "garbage-no-pipe",
                "garbage-no-pipe",
                None,
            ),
            (
                // The discriminator between splitting at the first `|` and the
                // last: the protocol is a suffix AM appends to an entity ID it
                // does not escape.
                "a pipe inside the entity id belongs to the entity id",
                "urn:example:sp|a|saml2",
                "urn:example:sp|a",
                Some("saml2"),
            ),
            (
                "surrounding whitespace is not part of either half",
                "  https://sp-b.example.com | saml2  ",
                "https://sp-b.example.com",
                Some("saml2"),
            ),
        ];

        for (what, entry, entity_id, protocol) in cases {
            let parsed = parse_trusted_provider(entry);
            assert_eq!(parsed.entity_id, entity_id, "{what}");
            assert_eq!(parsed.protocol.as_deref(), protocol, "{what}");
        }
    }

    /// Which circles of trust a delete would rewrite — by name, because the
    /// operator has to recognise them, and the count is what a cascade report
    /// must not reduce to.
    #[test]
    fn cots_naming_matches_the_whole_entity_id_and_reports_every_circle() {
        let cots = [
            cot_doc("client-a", &["https://sp-a.example.com|saml2"]),
            // The trap: `https://sp-a.example.com` is a prefix of this one —
            // and a trailing slash really is how Entra spells an entity ID —
            // so a `starts_with` or `contains` test reports a cascade onto a
            // circle of trust that will not change.
            cot_doc("client-a-slash", &["https://sp-a.example.com/|saml2"]),
            cot_doc(
                "shared",
                &[
                    "https://idp-a.example.com|saml2",
                    "https://sp-a.example.com|saml2",
                ],
            ),
            cot_doc("empty", &[]),
        ];

        let hit = cots_naming("https://sp-a.example.com", &cots);
        assert_eq!(
            hit.iter().map(|m| m.cot.as_str()).collect::<Vec<_>>(),
            ["client-a", "shared"],
            "an entity listed in two circles of trust names both, and only those"
        );
        assert_eq!(hit[1].entries, ["https://sp-a.example.com|saml2"]);

        assert_eq!(
            cots_naming("https://sp-a.example.com/", &cots)
                .iter()
                .map(|m| m.cot.as_str())
                .collect::<Vec<_>>(),
            ["client-a-slash"],
            "the longer id is its own entity, not a member of the shorter one's circles"
        );

        assert!(cots_naming("https://sp-z.example.com", &cots).is_empty());
    }

    /// One circle can list the same entity twice, under two protocols. Both
    /// entries go, so both are reported.
    #[test]
    fn a_circle_listing_an_entity_under_two_protocols_reports_both_entries() {
        let cots = [cot_doc(
            "dual",
            &[
                "https://sp-a.example.com|saml2",
                "https://sp-a.example.com|wsfed",
            ],
        )];
        let hit = cots_naming("https://sp-a.example.com", &cots);
        assert_eq!(hit.len(), 1, "one circle of trust");
        assert_eq!(
            hit[0].entries,
            [
                "https://sp-a.example.com|saml2",
                "https://sp-a.example.com|wsfed"
            ]
        );
    }

    #[test]
    fn the_cascade_report_names_the_circles_and_the_entries() {
        let cots = [
            cot_doc("client-a", &["https://sp-a.example.com|saml2"]),
            cot_doc("shared", &["https://sp-a.example.com|saml2"]),
        ];
        let affected = cots_naming("https://sp-a.example.com", &cots);
        let report = cascade_lines("https://sp-a.example.com", "bravo", &affected).join("\n");
        assert!(
            report.contains("client-a  https://sp-a.example.com|saml2"),
            "{report}"
        );
        assert!(
            report.contains("shared  https://sp-a.example.com|saml2"),
            "{report}"
        );

        let nothing = cascade_lines("https://sp-z.example.com", "bravo", &[]).join("\n");
        assert!(
            nothing.contains("no circle of trust in realm bravo"),
            "{nothing}"
        );
        assert!(!nothing.contains("also rewrites"), "{nothing}");
    }

    /// The caveat is the whole reason this command is allowed to exist, so it
    /// is asserted on the rendered output — including for an **empty** circle
    /// of trust, where "no members" is the reading most likely to be taken as
    /// proof that nothing trusts anything.
    #[test]
    fn cot_show_always_says_membership_is_stored_in_two_places() {
        for cot in [
            cot_doc("client-b", &["https://sp-b.example.com|saml2"]),
            cot_doc("empty", &[]),
        ] {
            let rendered = cot_show_lines(&cot).join("\n");
            for phrase in [
                "cotlist",
                "trustedProviders",
                "REST does not expose",
                "runtime trust check",
            ] {
                assert!(
                    rendered.contains(phrase),
                    "`cot show {}` dropped {phrase:?}:\n{rendered}",
                    cot.name
                );
            }
        }
    }

    #[test]
    fn cot_show_renders_the_entries_and_flags_one_with_no_protocol() {
        let rendered = cot_show_lines(&cot_doc(
            "client-b",
            &["https://sp-b.example.com|saml2", "garbage-no-pipe"],
        ))
        .join("\n");
        assert!(
            rendered.contains("https://sp-b.example.com|saml2"),
            "{rendered}"
        );
        assert!(
            rendered.contains("garbage-no-pipe  (no |protocol suffix"),
            "an entry AM will never resolve must not read like a member: {rendered}"
        );
    }

    /// `description` is absent when unset, and `status` is what AM calls the
    /// field. A gap renders as `-`, not as an empty cell that reads like a
    /// value we failed to fetch.
    #[test]
    fn cot_rows_distinguish_an_absent_description_from_a_present_one() {
        let described = Cot {
            description: Some("client B federation".to_string()),
            ..cot_doc("client-b", &["https://sp-b.example.com|saml2"])
        };
        let bare = cot_doc("bare", &[]);
        let rows = cot_rows(&[described, bare]);
        assert_eq!(
            rows[0],
            ["client-b", "active", "1", "client B federation"],
            "a described circle of trust"
        );
        assert_eq!(rows[1], ["bare", "active", "0", "-"], "an undescribed one");
    }

    /// The committed shape from `docs/api/06-saml.md`, deserialised. The
    /// second document is the one that matters: two of UAT `bravo`'s three
    /// CoTs omit `description` entirely, and a required field would abort the
    /// whole listing on them.
    #[test]
    fn a_cot_document_parses_with_its_optional_keys_absent() {
        let full = serde_json::json!({
            "_id": "client-b",
            "_rev": "-1000217909",
            "status": "active",
            "trustedProviders": [
                "https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2",
                "https://sp-b.example.com|saml2"
            ],
            "_type": { "_id": "circlesoftrust", "name": "Circle of Trust", "collection": true }
        });
        let parsed = cot(&full).expect("the documented shape parses");
        assert_eq!(parsed.name, "client-b");
        assert_eq!(parsed.status.as_deref(), Some("active"));
        assert_eq!(parsed.description, None, "absent, not empty");
        assert_eq!(parsed.trusted_providers.len(), 2);

        let bare = serde_json::json!({ "_id": "servicedesk", "status": "active" });
        let parsed = cot(&bare).expect("a CoT with no members and no description parses");
        assert_eq!(parsed.description, None);
        assert!(parsed.trusted_providers.is_empty());

        assert!(
            cot(&serde_json::json!({ "status": "active" })).is_err(),
            "a document with no `_id` names nothing and must not parse"
        );
    }

    #[test]
    fn a_cot_name_that_would_address_another_resource_is_refused() {
        for name in ["", "   ", "../saml2", "a/b", "a?b", "a#b", "a\\b"] {
            assert!(
                validate_cot_name(name).is_err(),
                "{name:?} reaches the URL path unencoded"
            );
        }
        assert_eq!(
            validate_cot_name("  client-b ").expect("a plain name"),
            "client-b"
        );
    }

    /// Everything `create-hosted` refuses, AM would have accepted or failed
    /// opaquely on. Each row is one of those.
    #[test]
    fn build_hosted_create_imposes_what_am_does_not() {
        let ok = |entity_id: &str, role: Role, meta_alias: &str| HostedCreate {
            entity_id: entity_id.to_string(),
            role,
            meta_alias: meta_alias.to_string(),
        };

        assert_eq!(
            build_hosted_create(
                "bravo",
                &ok("https://sp-b.example.com", Role::Sp, "/bravo/client-b-sp")
            )
            .expect("a complete SP request"),
            serde_json::json!({
                "entityId": "https://sp-b.example.com",
                "serviceProvider": { "services": { "metaAlias": "/bravo/client-b-sp" } }
            })
        );
        assert_eq!(
            build_hosted_create(
                "bravo",
                &ok("https://idp-b.example.com", Role::Idp, "/bravo/b-idp")
            )
            .expect("a complete IdP request"),
            serde_json::json!({
                "entityId": "https://idp-b.example.com",
                "identityProvider": { "services": { "metaAlias": "/bravo/b-idp" } }
            }),
            "the role block key follows --role, not the SP default"
        );

        let refused: [(&str, HostedCreate, &str); 8] = [
            (
                // `{}` is a 201 and AM mints a UUID; only this stops it.
                "an empty entity id",
                ok("", Role::Sp, "/bravo/x"),
                "entityId",
            ),
            (
                "an entity id that is only whitespace",
                ok("   ", Role::Sp, "/bravo/x"),
                "entityId",
            ),
            (
                // A role block without it is a 500 naming no field.
                "an empty meta alias",
                ok("https://sp-b.example.com", Role::Sp, ""),
                "--meta-alias",
            ),
            (
                "a meta alias that is only whitespace",
                ok("https://sp-b.example.com", Role::Sp, "  "),
                "--meta-alias",
            ),
            (
                "a meta alias with no leading slash",
                ok("https://sp-b.example.com", Role::Sp, "bravo/x"),
                "must be",
            ),
            (
                // The discriminator for the realm half: a check that only
                // asked for a leading `/` would accept this and publish
                // endpoints under a realm the entity does not live in.
                "a meta alias under a different realm",
                ok("https://sp-b.example.com", Role::Sp, "/alpha/x"),
                "must be",
            ),
            (
                "a meta alias with no leaf",
                ok("https://sp-b.example.com", Role::Sp, "/bravo/"),
                "must be",
            ),
            (
                "a meta alias with a second path segment",
                ok("https://sp-b.example.com", Role::Sp, "/bravo/x/y"),
                "must be",
            ),
        ];

        for (what, request, expected) in refused {
            let error = build_hosted_create("bravo", &request)
                .expect_err(what)
                .to_string();
            assert!(error.contains(expected), "{what}: {error}");
        }
    }

    /// `--force` is the whole permission, so both directions are asserted —
    /// and the refusal has to name the flag that lifts it.
    #[test]
    fn delete_is_refused_without_force_and_allowed_with_it() {
        let refusal = delete_ok(
            false,
            "https://sp-b.example.com",
            Location::Hosted,
            "sandbox",
            "bravo",
        )
        .expect_err("an unforced delete")
        .to_string();
        assert!(refusal.contains("--force"), "{refusal}");
        assert!(refusal.contains("https://sp-b.example.com"), "{refusal}");
        assert!(refusal.contains("sandbox/bravo"), "{refusal}");
        assert!(refusal.contains("hosted"), "{refusal}");

        delete_ok(
            true,
            "https://sp-b.example.com",
            Location::Hosted,
            "sandbox",
            "bravo",
        )
        .expect("--force authorizes the delete");
    }
    /// The cascade report is a diff of two reads, not a replay of the first.
    /// A circle that still lists the entity must not be reported as cleaned.
    #[test]
    fn the_cascade_outcome_distinguishes_a_removal_from_a_survivor() {
        let before = vec![
            CotMembership {
                cot: "client-a".to_string(),
                entries: vec!["https://sp-a.example.com|saml2".to_string()],
            },
            CotMembership {
                cot: "shared".to_string(),
                entries: vec!["https://sp-a.example.com|saml2".to_string()],
            },
        ];
        // `shared` did not change; `client-a` did.
        let after = vec![CotMembership {
            cot: "shared".to_string(),
            entries: vec!["https://sp-a.example.com|saml2".to_string()],
        }];

        let lines = cascade_outcome_lines("https://sp-a.example.com", &before, &after);
        assert_eq!(lines.len(), 2, "one line per circle that named the entity");
        assert!(
            lines[0].contains("removed from circle of trust client-a"),
            "{:?}",
            lines[0]
        );
        assert!(
            lines[1].contains("warning") && lines[1].contains("shared"),
            "a circle that still lists the entity is not a removal: {:?}",
            lines[1]
        );
        assert!(!lines[1].contains("removed from"), "{:?}", lines[1]);
    }

    /// The create report must separate what AM confirmed (the id in the 201)
    /// from what was merely sent (the role and the alias), and must not read a
    /// 201 as proof that the request took: AM answers 201 to a create it
    /// renamed, and to one whose body it ignored entirely.
    #[test]
    fn the_create_report_separates_what_am_confirmed_from_what_was_sent() {
        let request = HostedCreate {
            entity_id: "https://sp-b.example.com".to_string(),
            role: Role::Sp,
            meta_alias: "/bravo/client-b-sp".to_string(),
        };

        let echoed = created_lines(
            Some("https://sp-b.example.com"),
            &request,
            "sandbox",
            "bravo",
        )
        .join("\n");
        assert!(
            echoed.contains(
                "created hosted SAML entity provider https://sp-b.example.com \
                 in sandbox/bravo"
            ),
            "{echoed}"
        );
        assert!(
            !echoed.contains("warning"),
            "an echoed id is the expected case: {echoed}"
        );
        assert!(
            echoed.contains("are what was sent") && echoed.contains("nothing was read back"),
            "the role and alias were never read back and the report must say so: {echoed}"
        );

        // The UUID case. A count of lines would not tell these apart; the id
        // AM named is the identity that matters.
        let renamed = created_lines(
            Some("9f2c0f38-0000-0000-0000-000000000000"),
            &request,
            "sandbox",
            "bravo",
        )
        .join("\n");
        assert!(
            renamed.contains("warning: AM named it 9f2c0f38-0000-0000-0000-000000000000"),
            "{renamed}"
        );
        assert!(
            renamed.contains("not the requested https://sp-b.example.com"),
            "{renamed}"
        );

        for missing in [None, Some(""), Some("  ")] {
            let unconfirmed = created_lines(missing, &request, "sandbox", "bravo").join("\n");
            assert!(
                unconfirmed.contains("the 201 carried no entityId"),
                "{missing:?} is not a confirmation: {unconfirmed}"
            );
            assert!(
                unconfirmed.contains("https://sp-b.example.com"),
                "the requested id is still the operator's only handle: {unconfirmed}"
            );
        }
    }
}
