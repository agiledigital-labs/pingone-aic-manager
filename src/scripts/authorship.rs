//! "Who last edited this script, and when?" — the four authorship fields AM
//! stamps on every script, plus the resolver that turns their principal DNs
//! into names. Ground truth: `docs/api/04-scripts.md` ("Authorship and change
//! history"), verified live 2026-08-10.
//!
//! Four traps this module exists to encapsulate, each of which produced a
//! plausible-looking wrong answer during that verification:
//!
//! 1. **`"null"` is the four-character string**, not JSON `null`, and the key
//!    is never omitted. An `Option<String>` deserialises it to `Some("null")`,
//!    so an absence check renders `null` as if it were a principal.
//! 2. **Unknown author and unknown date are independent.** The
//!    `ForgeRock Internal:` scripts carry `createdBy: "null"` with a real 2015
//!    `creationDate`, so neither test may be derived from the other.
//! 3. **Two resolution failures are normal, not errors.** `dsameuser` answers
//!    403 (exists, unreadable) and deleted admins answer 404. Both are facts
//!    about the principal and must not fail a command.
//! 4. **A service account is a credential, never a person.** Every write `aic`
//!    makes is stamped with the shared SA DN — it cannot even distinguish two
//!    concurrent processes using the same credential — so nothing here may
//!    render it as an operator.
//!
//! Only AM scripts have any of this: every IDM config object carries no
//! authorship and no `_rev` (see [`super::Kind::has_authorship`]).

use std::collections::HashMap;

use chrono::TimeZone;
use serde_json::Value;

use crate::Error;
use crate::aic::api::ApiCall;

/// AM's stand-in for "no author": the literal string, not JSON `null`.
const NULL_SENTINEL: &str = "null";

/// Fields the resolver needs. `cn` is fetched for service accounts and AM's
/// built-in admin only — for a human it is the email concatenated with itself,
/// which reads as a bug on screen, so [`classify`] never displays it for one.
const USER_FIELDS: &str = "_fields=username,cn,givenName,sn,mail";

/// Local/UTC rendering of an epoch-ms stamp. The numeric offset (`%:z`) rather
/// than an abbreviation, so a pasted line stays unambiguous.
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S %:z";

// ---------------------------------------------------------------------------
// The fields, as they come off the wire
// ---------------------------------------------------------------------------

/// Who wrote a script and when, as AM reports it. Created and last-modified are
/// stamped independently, and either half may be unknown on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Authorship {
    pub created: Change,
    pub modified: Change,
}

/// One write event: a principal and a timestamp, each independently unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub by: Author,
    /// Epoch **milliseconds**; `None` when AM reported `0` or omitted it.
    pub at: Option<i64>,
}

/// The principal named by `createdBy` / `lastModifiedBy`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Author {
    /// AM sent the `"null"` sentinel (or, defensively, nothing at all).
    #[default]
    Unknown,
    /// A DN whose `id=` component we can hand to the resolver.
    Principal { dn: String, id: String },
    /// A non-empty value that is not a DN shape we recognise. Shown verbatim
    /// rather than guessed at — no such value occurs on the sandbox today.
    Other(String),
}

impl Author {
    /// Parse one `createdBy`/`lastModifiedBy` value.
    pub fn parse(field: Option<&Value>) -> Author {
        let raw = match field {
            Some(Value::String(s)) => s.trim(),
            // JSON `null` does not occur on these two fields (0 of 405 records
            // in either realm) — handled so it can never read as a principal.
            _ => return Author::Unknown,
        };
        if raw.is_empty() || raw == NULL_SENTINEL {
            return Author::Unknown;
        }
        match dn_user_id(raw) {
            Some(id) => Author::Principal {
                dn: raw.to_string(),
                id: id.to_string(),
            },
            None => Author::Other(raw.to_string()),
        }
    }

    /// The id to resolve, for the one variant that has one.
    pub fn principal_id(&self) -> Option<&str> {
        match self {
            Author::Principal { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Extract the `id=` component of a principal DN.
///
/// Both shapes that occur are covered — `id=<x>,ou=user,ou=am-config` and
/// `id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org` — and every observed
/// value is a DN of that form, so "take the first `id=` RDN" is the whole rule.
/// A value that is not a DN yields `None` rather than a fabricated id.
pub fn dn_user_id(value: &str) -> Option<&str> {
    value.split(',').find_map(|rdn| {
        let (attribute, id) = rdn.trim().split_once('=')?;
        let id = id.trim();
        (attribute.trim().eq_ignore_ascii_case("id") && !id.is_empty()).then_some(id)
    })
}

impl Authorship {
    /// Read the four fields out of a raw AM script config. Callers hand this
    /// whole script objects from the existing list/fetch, so it costs no
    /// request of its own.
    pub fn from_config(raw: &Value) -> Authorship {
        Authorship {
            created: Change {
                by: Author::parse(raw.get("createdBy")),
                at: epoch_ms(raw.get("creationDate")),
            },
            modified: Change {
                by: Author::parse(raw.get("lastModifiedBy")),
                at: epoch_ms(raw.get("lastModifiedDate")),
            },
        }
    }
}

/// An epoch-millisecond field, with AM's `0` ("unknown") mapped to `None`.
/// Deliberately number-only: AM scripts use epoch-ms ints while other AIC
/// families use ISO-8601 strings (CLAUDE.md §8), and silently accepting both
/// here would hide a shape change instead of surfacing it.
fn epoch_ms(field: Option<&Value>) -> Option<i64> {
    field.and_then(Value::as_i64).filter(|ms| *ms > 0)
}

// ---------------------------------------------------------------------------
// Resolving a principal to a name
// ---------------------------------------------------------------------------

/// What `GET /am/json/realms/root/users/{id}` said about a principal. Every
/// variant renders to something honest — including the two failures that are
/// normal on a real tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A human tenant admin: `givenName`/`sn` populated, `username` an email.
    Human(String),
    /// A service account: `username` equals the id and there is no personal
    /// name. The payload is the credential's `cn`.
    ServiceAccount(String),
    /// AM's own built-in administrator (`amadmin`): `username` equals the id,
    /// but it does carry a name. Neither a person nor a tenant credential.
    BuiltIn(String),
    /// 403 — the principal exists but AM refuses to read it (`dsameuser`).
    AmInternal,
    /// 404 — the principal was deleted.
    Deleted,
    /// Any other failure. The id still shows; the name doesn't.
    Unresolved,
}

/// Classify a resolved user record. `id` is the DN's `id=` component, which is
/// what discriminates a credential from a person: a service account's
/// `username` **is** its id.
pub fn classify(id: &str, user: &Value) -> Identity {
    let username = first_str(user, "username");
    if username.as_deref() != Some(id) {
        // A human admin. `cn` is deliberately unused: for these records it is
        // the email address concatenated with itself.
        return Identity::Human(
            personal_name(user)
                .or(username)
                .unwrap_or_else(|| id.to_string()),
        );
    }
    let display = first_str(user, "cn").unwrap_or_else(|| id.to_string());
    match personal_name(user) {
        None => Identity::ServiceAccount(display),
        Some(_) => Identity::BuiltIn(display),
    }
}

/// A person's name from the fields service accounts don't have: `givenName` +
/// `sn`, falling back to `mail`.
fn personal_name(user: &Value) -> Option<String> {
    let parts: Vec<String> = ["givenName", "sn"]
        .iter()
        .filter_map(|key| first_str(user, key))
        .collect();
    if parts.is_empty() {
        return first_str(user, "mail");
    }
    Some(parts.join(" "))
}

/// First non-empty value of an AM field, which may be a string or an array.
fn first_str(user: &Value, key: &str) -> Option<String> {
    let value = user.get(key)?;
    let candidate = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .find(|s| !s.is_empty())?
            .to_string(),
        _ => return None,
    };
    (!candidate.is_empty()).then_some(candidate)
}

/// Resolve one principal id to an [`Identity`].
///
/// Never returns an error: 403 and 404 are properties of the principal (see the
/// module header), and any other transport failure degrades to
/// [`Identity::Unresolved`] so a "who changed this" answer still shows the id
/// and both timestamps.
pub async fn resolve_principal(tenant: &str, id: &str) -> Identity {
    let path = format!("/am/json/realms/root/users/{id}?{USER_FIELDS}");
    match ApiCall::new(tenant, "GET", &path).send().await {
        Ok(user) => classify(id, &user),
        Err(Error::Api { status: 403, .. }) => Identity::AmInternal,
        Err(Error::Api { status: 404, .. }) => Identity::Deleted,
        Err(e) => {
            tracing::debug!("resolving principal {id} on {tenant}: {e}");
            Identity::Unresolved
        }
    }
}

/// Per-tenant principal cache.
///
/// Root-user enumeration is 403, so resolution must be lookup-by-id; a whole
/// tenant has only ~15 distinct principals, so one map per tenant is enough.
/// Failures are cached too: they are answers, not transient misses.
#[derive(Debug, Default)]
pub struct PrincipalCache {
    known: HashMap<String, Identity>,
}

impl PrincipalCache {
    pub fn get(&self, id: &str) -> Option<&Identity> {
        self.known.get(id)
    }

    pub fn insert(&mut self, id: impl Into<String>, identity: Identity) {
        self.known.insert(id.into(), identity);
    }

    /// The identity behind `author`, fetching it at most once per cache.
    /// `None` when the author names no resolvable principal.
    pub async fn resolve(&mut self, tenant: &str, author: &Author) -> Option<Identity> {
        let id = author.principal_id()?;
        if let Some(known) = self.known.get(id) {
            return Some(known.clone());
        }
        let identity = resolve_principal(tenant, id).await;
        self.known.insert(id.to_string(), identity.clone());
        Some(identity)
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// One principal, rendered for a human. Never blank, never a bare DN, and never
/// wording that lets a shared credential read as a person. `resolved` is `None`
/// before a background lookup lands — the id shows in the meantime.
pub fn describe_author(author: &Author, resolved: Option<&Identity>) -> String {
    let (dn_id, resolved) = match author {
        Author::Unknown => return "unknown (AM recorded no author)".to_string(),
        Author::Other(raw) => return raw.clone(),
        Author::Principal { id, .. } => (id.as_str(), resolved),
    };
    match resolved {
        None => dn_id.to_string(),
        Some(Identity::Human(name)) => name.clone(),
        // Names the credential and stops there: `aic`'s own pushes all land
        // here, and the audit trail cannot say which operator drove them.
        Some(Identity::ServiceAccount(name)) => format!("service account \"{name}\""),
        Some(Identity::BuiltIn(name)) => format!("{name} (AM built-in administrator)"),
        Some(Identity::AmInternal) => format!("{dn_id} (AM-internal account — not readable)"),
        Some(Identity::Deleted) => format!("{dn_id} (deleted principal)"),
        Some(Identity::Unresolved) => format!("{dn_id} (name lookup failed)"),
    }
}

/// Whether a rendered principal is a shared credential, so a caller can add the
/// "names the credential, not the operator" caveat exactly once.
pub fn is_service_account(resolved: Option<&Identity>) -> bool {
    matches!(resolved, Some(Identity::ServiceAccount(_)))
}

/// Render an epoch-ms stamp in `tz`. Tested against fixed zones; the display
/// path passes [`chrono::Local`] via [`format_local`].
pub fn format_time<Tz: TimeZone>(at: Option<i64>, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(ms) = at else {
        return "unknown".to_string();
    };
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(utc) => utc.with_timezone(tz).format(TIME_FORMAT).to_string(),
        None => format!("{ms} (not a valid epoch-ms)"),
    }
}

/// Same, in the operator's local zone.
pub fn format_local(at: Option<i64>) -> String {
    format_time(at, &chrono::Local)
}

/// ISO-8601 UTC, for `--json` output. `None` stays JSON `null` — the one place
/// a real null is the right answer, since it is ours and not AM's.
pub fn format_iso(at: Option<i64>) -> Option<String> {
    let ms = at?;
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|utc| utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Stable machine-readable tag for an author, for `--json`.
pub fn author_kind(author: &Author, resolved: Option<&Identity>) -> &'static str {
    match author {
        Author::Unknown => "unknown",
        Author::Other(_) => "unrecognised",
        Author::Principal { .. } => match resolved {
            None => "unresolved",
            Some(Identity::Human(_)) => "human",
            Some(Identity::ServiceAccount(_)) => "service-account",
            Some(Identity::BuiltIn(_)) => "am-built-in",
            Some(Identity::AmInternal) => "am-internal",
            Some(Identity::Deleted) => "deleted",
            Some(Identity::Unresolved) => "unresolved",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};
    use serde_json::json;

    // ----- DN parsing -----------------------------------------------------

    #[test]
    fn dn_id_is_extracted_from_both_shapes_that_occur() {
        assert_eq!(
            dn_user_id("id=ad604d54-ef8e-454c-b3f3-c2f8197b56f5,ou=user,ou=am-config"),
            Some("ad604d54-ef8e-454c-b3f3-c2f8197b56f5")
        );
        assert_eq!(
            dn_user_id("id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org"),
            Some("dsameuser")
        );
        assert_eq!(
            dn_user_id("id=amadmin,ou=user,ou=am-config"),
            Some("amadmin")
        );
        // Whitespace and attribute case are both DN-legal.
        assert_eq!(dn_user_id("ID=abc , ou=user, ou=am-config"), Some("abc"));
    }

    #[test]
    fn values_that_are_not_dns_yield_no_id() {
        // Not a DN at all.
        assert_eq!(dn_user_id("Dave Balmain"), None);
        // DN-shaped, but with no `id=` RDN to take.
        assert_eq!(dn_user_id("uid=abc,ou=user,ou=am-config"), None);
        // An `id=` with nothing in it is not an id.
        assert_eq!(dn_user_id("id=,ou=user,ou=am-config"), None);
        assert_eq!(dn_user_id(""), None);
    }

    #[test]
    fn a_non_dn_author_is_shown_verbatim_not_resolved() {
        let author = Author::parse(Some(&json!("some-future-principal-form")));
        assert_eq!(author, Author::Other("some-future-principal-form".into()));
        assert_eq!(author.principal_id(), None);
        assert_eq!(describe_author(&author, None), "some-future-principal-form");
    }

    // ----- the "null" sentinel and the zero date, independently -----------
    //
    // These two vary one field at a time on purpose: the sentinel and the zero
    // date usually travel together, and a test that moved both at once would
    // pass even if one were derived from the other.

    #[test]
    fn the_literal_null_string_is_an_unknown_author_not_a_principal() {
        // Only the author varies; the date stays real.
        let a = Authorship::from_config(&json!({
            "createdBy": "null",
            "creationDate": 1433147666269i64,
            "lastModifiedBy": "null",
            "lastModifiedDate": 1433147666269i64,
        }));
        assert_eq!(a.created.by, Author::Unknown);
        assert_eq!(a.modified.by, Author::Unknown);
        // The date is untouched by the author being unknown.
        assert_eq!(a.created.at, Some(1433147666269));
        assert_eq!(a.modified.at, Some(1433147666269));
        // And "null" never reaches the screen as a principal.
        let rendered = describe_author(&a.created.by, None);
        assert!(!rendered.contains("null"), "rendered as {rendered:?}");
        assert_eq!(rendered, "unknown (AM recorded no author)");
    }

    #[test]
    fn a_zero_date_is_unknown_even_when_the_author_is_known() {
        // Only the date varies; the author stays a real principal.
        let a = Authorship::from_config(&json!({
            "createdBy": "id=ca8daa5d,ou=user,ou=am-config",
            "creationDate": 0,
            "lastModifiedBy": "id=ca8daa5d,ou=user,ou=am-config",
            "lastModifiedDate": 0,
        }));
        assert_eq!(a.created.at, None);
        assert_eq!(a.modified.at, None);
        assert_eq!(format_time(a.created.at, &Utc), "unknown");
        assert_eq!(
            a.created.by,
            Author::Principal {
                dn: "id=ca8daa5d,ou=user,ou=am-config".into(),
                id: "ca8daa5d".into()
            }
        );
    }

    #[test]
    fn the_forgerock_internal_pairing_keeps_a_real_date_under_an_unknown_author() {
        // The live counter-example to "sentinel implies zero date": a `"null"`
        // author with ForgeRock's 2015 build stamp and no modification date.
        let a = Authorship::from_config(&json!({
            "createdBy": "null",
            "creationDate": 1433147666269i64,
            "lastModifiedBy": "null",
            "lastModifiedDate": 0,
        }));
        assert_eq!(a.created.by, Author::Unknown);
        assert_eq!(
            format_time(a.created.at, &Utc),
            "2015-06-01 08:34:26 +00:00"
        );
        assert_eq!(a.modified.at, None);
    }

    #[test]
    fn absent_and_json_null_fields_are_unknown_too() {
        let a = Authorship::from_config(&json!({}));
        assert_eq!(a.created.by, Author::Unknown);
        assert_eq!(a.created.at, None);
        let explicit = Authorship::from_config(&json!({
            "createdBy": null, "creationDate": null,
        }));
        assert_eq!(explicit.created.by, Author::Unknown);
        assert_eq!(explicit.created.at, None);
    }

    // ----- service account vs human ---------------------------------------

    #[test]
    fn a_username_equal_to_the_id_is_a_service_account_displayed_by_cn() {
        let id = "ad604d54-ef8e-454c-b3f3-c2f8197b56f5";
        let identity = classify(
            id,
            &json!({ "username": id, "cn": ["DaveBalmain-fr-config-manager"] }),
        );
        assert_eq!(
            identity,
            Identity::ServiceAccount("DaveBalmain-fr-config-manager".into())
        );
        let author = Author::parse(Some(&json!(format!("id={id},ou=user,ou=am-config"))));
        assert_eq!(
            describe_author(&author, Some(&identity)),
            "service account \"DaveBalmain-fr-config-manager\""
        );
        assert!(is_service_account(Some(&identity)));
    }

    #[test]
    fn a_human_is_given_name_plus_sn_and_never_the_doubled_cn() {
        let id = "ca8daa5d-fe08-428c-aed9-3811db0f7b4f";
        let identity = classify(
            id,
            &json!({
                "username": "dsbalmain@agiledigital.com.au",
                // Live shape: the email, concatenated with itself.
                "cn": ["dsbalmain@agiledigital.com.au dsbalmain@agiledigital.com.au"],
                "givenName": ["David"],
                "sn": ["Balmain"],
                "mail": ["dsbalmain@agiledigital.com.au"],
            }),
        );
        assert_eq!(identity, Identity::Human("David Balmain".into()));
        let author = Author::parse(Some(&json!(format!("id={id},ou=user,ou=am-config"))));
        let rendered = describe_author(&author, Some(&identity));
        assert_eq!(rendered, "David Balmain");
        assert!(
            !rendered.contains("@"),
            "leaked the doubled cn: {rendered:?}"
        );
        assert!(!is_service_account(Some(&identity)));
    }

    #[test]
    fn a_human_without_a_personal_name_falls_back_to_mail_then_username() {
        assert_eq!(
            classify(
                "uuid",
                &json!({ "username": "a@b.com", "mail": ["a@b.com"] })
            ),
            Identity::Human("a@b.com".into())
        );
        assert_eq!(
            classify("uuid", &json!({ "username": "a@b.com", "mail": [] })),
            Identity::Human("a@b.com".into())
        );
        // Nothing usable at all still names the principal.
        assert_eq!(classify("uuid", &json!({})), Identity::Human("uuid".into()));
    }

    #[test]
    fn ams_built_in_admin_is_neither_a_person_nor_a_credential() {
        // `amadmin` shares the service-account tell (`username == id`) but does
        // carry a name, so the SA wording would misdescribe it.
        let identity = classify(
            "amadmin",
            &json!({
                "username": "amadmin", "cn": ["amAdmin"],
                "givenName": ["amAdmin"], "sn": ["amAdmin"], "mail": [],
            }),
        );
        assert_eq!(identity, Identity::BuiltIn("amAdmin".into()));
        assert!(!is_service_account(Some(&identity)));
        let author = Author::parse(Some(&json!("id=amadmin,ou=user,ou=am-config")));
        assert_eq!(
            describe_author(&author, Some(&identity)),
            "amAdmin (AM built-in administrator)"
        );
    }

    #[test]
    fn string_and_array_field_shapes_both_resolve() {
        let plain = classify(
            "uuid",
            &json!({ "username": "uuid", "cn": "Plain String SA" }),
        );
        assert_eq!(plain, Identity::ServiceAccount("Plain String SA".into()));
    }

    // ----- the failure labels ---------------------------------------------

    #[test]
    fn each_resolution_failure_renders_a_distinct_honest_label() {
        let author = Author::parse(Some(&json!(
            "id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org"
        )));
        assert_eq!(
            describe_author(&author, Some(&Identity::AmInternal)),
            "dsameuser (AM-internal account — not readable)"
        );

        let deleted = Author::parse(Some(&json!(
            "id=779e5401-b21b-4092-9da1-c28ee7a96b63,ou=user,ou=am-config"
        )));
        assert_eq!(
            describe_author(&deleted, Some(&Identity::Deleted)),
            "779e5401-b21b-4092-9da1-c28ee7a96b63 (deleted principal)"
        );
        assert_eq!(
            describe_author(&deleted, Some(&Identity::Unresolved)),
            "779e5401-b21b-4092-9da1-c28ee7a96b63 (name lookup failed)"
        );
        // Not yet looked up: the id shows, so the line is never blank.
        assert_eq!(
            describe_author(&deleted, None),
            "779e5401-b21b-4092-9da1-c28ee7a96b63"
        );
    }

    #[test]
    fn json_author_kinds_are_stable() {
        let principal = Author::parse(Some(&json!("id=x,ou=user,ou=am-config")));
        assert_eq!(author_kind(&Author::Unknown, None), "unknown");
        assert_eq!(
            author_kind(&Author::Other("x".into()), None),
            "unrecognised"
        );
        assert_eq!(author_kind(&principal, None), "unresolved");
        assert_eq!(
            author_kind(&principal, Some(&Identity::Human("N".into()))),
            "human"
        );
        assert_eq!(
            author_kind(&principal, Some(&Identity::ServiceAccount("N".into()))),
            "service-account"
        );
        assert_eq!(
            author_kind(&principal, Some(&Identity::BuiltIn("N".into()))),
            "am-built-in"
        );
        assert_eq!(
            author_kind(&principal, Some(&Identity::AmInternal)),
            "am-internal"
        );
        assert_eq!(author_kind(&principal, Some(&Identity::Deleted)), "deleted");
    }

    // ----- epoch-ms formatting -------------------------------------------

    #[test]
    fn epoch_ms_renders_in_the_requested_zone() {
        // The `aic` push observed during verification.
        let at = Some(1786339765012);
        assert_eq!(format_time(at, &Utc), "2026-08-10 05:29:25 +00:00");
        let melbourne = FixedOffset::east_opt(10 * 3600).unwrap();
        assert_eq!(format_time(at, &melbourne), "2026-08-10 15:29:25 +10:00");
        assert_eq!(format_iso(at).unwrap(), "2026-08-10T05:29:25.012Z");
    }

    #[test]
    fn unknown_and_nonsense_dates_never_render_as_an_epoch() {
        assert_eq!(format_time(None, &Utc), "unknown");
        assert_eq!(format_iso(None), None);
        // Seconds mistaken for millis would land in 1970 rather than erroring —
        // that is a real value, so only genuinely unrepresentable input is
        // labelled.
        assert_eq!(format_time(Some(1), &Utc), "1970-01-01 00:00:00 +00:00");
        assert_eq!(
            format_time(Some(i64::MAX), &Utc),
            format!("{} (not a valid epoch-ms)", i64::MAX)
        );
    }

    // ----- cache ----------------------------------------------------------

    #[test]
    fn the_cache_answers_from_memory_and_ignores_unresolvable_authors() {
        let mut cache = PrincipalCache::default();
        cache.insert("uuid-1", Identity::Human("Someone".into()));
        assert_eq!(
            cache.get("uuid-1"),
            Some(&Identity::Human("Someone".into()))
        );
        assert_eq!(cache.get("uuid-2"), None);
        // Nothing to look up for these, so no request could ever be spawned.
        assert_eq!(Author::Unknown.principal_id(), None);
        assert_eq!(Author::Other("x".into()).principal_id(), None);
    }
}
