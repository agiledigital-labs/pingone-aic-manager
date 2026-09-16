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
/// 2. otherwise the root element must be a SAML `<EntityDescriptor>`, which is
///    what a successful export always is (singular — never an
///    `EntitiesDescriptor`).
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
    if metadata::is_entity_descriptor(body) {
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

        let cases: [(&str, String, ExportOutcome); 8] = [
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
}
