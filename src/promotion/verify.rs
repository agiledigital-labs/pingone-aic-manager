//! `aic promotion verify`, tenant-free half: which live document a report
//! item names, how two copies are compared, and what the comparison means.
//!
//! The provisional report is the promotion service's claim, not a diff — on
//! 2026-10-26 it listed 150 items on a pair the console called in sync, and
//! every sampled one was byte-identical on both tenants
//! (`docs/api/23-promotion.md`). So every item is fetched from **both**
//! tenants and judged on content, whatever the report says its change kind is:
//! a "modified" item that is identical is in sync, and a "deleted" item absent
//! from both is too.
//!
//! An item this module cannot read is `Unchecked`, never `Same`: a verifier
//! that counted what it could not see as matching would recreate the defect it
//! exists to expose. That covers a response that decodes but is not the shape
//! measured — a tree without `nodes`, a theme document without a `realm`
//! object, a list that says more pages follow — which is an error here, not
//! an empty or absent item.

use serde_json::{Map, Value};

/// One entry of a report, flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub section: String,
    pub item: String,
    pub change: String,
    pub name: String,
    pub realm: Option<String>,
    pub uid: Option<String>,
}

/// Flatten a report. Shape errors are errors, as in `spec::summarise`.
pub fn entries(report: &Value) -> Result<Vec<Entry>, String> {
    let sections = report
        .get("report")
        .and_then(Value::as_object)
        .ok_or_else(|| "the report has no `report` object".to_string())?;
    let mut out = Vec::new();
    for (section, items) in sections {
        let items = items
            .as_array()
            .ok_or_else(|| format!("report section {section} is not an array"))?;
        for item in items {
            let name = str_field(item, "configItem")?;
            let changes = item
                .get("configChange")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{section} / {name} has no `configChange` object"))?;
            for (change, list) in changes {
                let list = list
                    .as_array()
                    .ok_or_else(|| format!("{section} / {name} / {change} is not an array"))?;
                for entry in list {
                    out.push(Entry {
                        section: section.clone(),
                        item: name.clone(),
                        change: change.clone(),
                        name: str_field(entry, "name")?,
                        realm: opt_str(entry, "realm")?,
                        uid: opt_str(entry, "uid")?,
                    });
                }
            }
        }
    }
    Ok(out)
}

fn str_field(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("no string `{key}` in {value}"))
}

fn opt_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!("`{key}` is not a string: {other}")),
    }
}

/// The live document a report entry names. Each variant's path was measured
/// 2026-10-26 (`docs/api/23-promotion.md`, "Verifying a report").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The tree **and every node it reaches**, page-node children included.
    Journey {
        realm: String,
        name: String,
    },
    Script {
        realm: String,
        id: String,
    },
    /// A script the report lists with no `uid` (seen 2026-10-26), found by
    /// an exact `name eq` query.
    ScriptByName {
        realm: String,
        name: String,
    },
    SecretMapping {
        realm: String,
        secret_id: String,
    },
    /// A realm service, addressed by its display name (`AM Services >
    /// <name>`); the id comes from the realm's service list.
    RealmService {
        realm: String,
        service: String,
    },
    AuthenticationSettings {
        realm: String,
    },
    /// A CORS configuration. The report lowercases its id (`client-a`
    /// for `Client-A`, measured), so it is matched without case
    /// against the list.
    Cors {
        name: String,
    },
    /// A realm's **whole** theme list in `ui/themerealm`. The report's theme
    /// name does not match the stored one ("Starter" for "Starter Theme",
    /// measured), so rather than guess which theme it means, the realm's
    /// list is compared entire — equal lists cover every theme in them.
    Themes {
        realm: String,
    },
    /// An IDM custom endpoint's config document, `endpoint/<name>`. The
    /// report names it with every `/` turned to `-` (`endpoint-<name>`,
    /// measured 2026-10-27), which cannot be reversed, so `api` resolves the
    /// name against the config list (`endpoint_id`) rather than guessing.
    IdmEndpoint {
        name: String,
    },
    Unchecked(String),
}

/// AM reports realms as `/alpha`, IDM as `alpha`. Only a plain sub-realm name
/// is addressable here; the root realm and nested paths are not.
fn realm(entry: &Entry) -> Result<String, String> {
    let raw = entry
        .realm
        .as_deref()
        .ok_or_else(|| "the report gives no realm".to_string())?;
    let name = raw.strip_prefix('/').unwrap_or(raw);
    if name.is_empty() || name.contains('/') {
        return Err(format!("realm {raw:?} is not a sub-realm this can address"));
    }
    Ok(name.to_string())
}

/// `ou=<uuid>,ou=scriptConfigurations,…` → `<uuid>`.
fn script_id(uid: &str) -> Option<String> {
    let first = uid.split(',').next()?.strip_prefix("ou=")?;
    let ok = first.len() == 36 && first.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    ok.then(|| first.to_string())
}

pub fn target(entry: &Entry) -> Target {
    let realm_of = || realm(entry);
    let unchecked = |why: String| Target::Unchecked(why);
    match entry.item.as_str() {
        "Journeys" => match realm_of() {
            Ok(realm) => Target::Journey {
                realm,
                name: entry.name.clone(),
            },
            Err(why) => unchecked(why),
        },
        "Scripts" => match (realm_of(), entry.uid.as_deref()) {
            (Err(why), _) => unchecked(why),
            (Ok(realm), None) => Target::ScriptByName {
                realm,
                name: entry.name.clone(),
            },
            (Ok(realm), Some(uid)) => match script_id(uid) {
                Some(id) => Target::Script { realm, id },
                None => unchecked(format!("no script id in the report's uid {uid:?}")),
            },
        },
        "ESV Secrets Store Mappings" => match realm_of() {
            Ok(realm) => Target::SecretMapping {
                realm,
                secret_id: entry.name.clone(),
            },
            Err(why) => unchecked(why),
        },
        "AM Authentication > Settings" => match realm_of() {
            Ok(realm) => Target::AuthenticationSettings { realm },
            Err(why) => unchecked(why),
        },
        "CORS" => Target::Cors {
            name: entry.name.clone(),
        },
        "IDM Themes By Realm" => match realm_of() {
            Ok(realm) => Target::Themes { realm },
            Err(why) => unchecked(why),
        },
        "IDM Custom Endpoints" => Target::IdmEndpoint {
            name: entry.name.clone(),
        },
        item => match item.strip_prefix("AM Services > ") {
            Some(service) => match realm_of() {
                Ok(realm) => Target::RealmService {
                    realm,
                    service: service.to_string(),
                },
                Err(why) => unchecked(why),
            },
            None => unchecked(format!("no reader for {item:?}")),
        },
    }
}

/// Where a target's copy metadata lives. Metadata is stripped **at those
/// roots only**: a nested configuration map may legitimately hold a key
/// spelt `lastModifiedDate`, and a difference there is a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `_rev` at the document root.
    Rev,
    /// A script: `_rev` and the four audit stamps at the root.
    Script,
    /// `{tree, nodes: {id: node}}` as `api::fetch` assembles it: `_rev` at
    /// the tree root and at each node's root.
    Journey,
    /// Nothing is metadata (IDM documents carry no `_rev` here).
    Plain,
}

impl Target {
    pub fn shape(&self) -> Shape {
        match self {
            Target::Script { .. } | Target::ScriptByName { .. } => Shape::Script,
            Target::Journey { .. } => Shape::Journey,
            Target::Themes { .. } | Target::IdmEndpoint { .. } => Shape::Plain,
            _ => Shape::Rev,
        }
    }
}

const SCRIPT_AUDIT: &[&str] = &[
    "createdBy",
    "creationDate",
    "lastModifiedBy",
    "lastModifiedDate",
];

fn strip_root(value: &Value, keys: &[&str]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !keys.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

pub fn normalise(shape: Shape, value: &Value) -> Value {
    match shape {
        Shape::Plain => value.clone(),
        Shape::Rev => strip_root(value, &["_rev"]),
        Shape::Script => {
            let mut keys = vec!["_rev"];
            keys.extend_from_slice(SCRIPT_AUDIT);
            strip_root(value, &keys)
        }
        Shape::Journey => {
            let mut out = value.clone();
            if let Some(tree) = out.get_mut("tree") {
                *tree = strip_root(tree, &["_rev"]);
            }
            if let Some(Value::Object(nodes)) = out.get_mut("nodes") {
                for node in nodes.values_mut() {
                    *node = strip_root(node, &["_rev"]);
                }
            }
            out
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Same,
    /// Up to `MAX_PATHS` JSON paths where the copies differ.
    Differs(Vec<String>),
    OnlyLower,
    OnlyUpper,
    /// Absent from both — in sync, e.g. a "deleted" item already gone.
    Neither,
    Unchecked(String),
}

impl Verdict {
    pub fn in_sync(&self) -> bool {
        matches!(self, Verdict::Same | Verdict::Neither)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Same => "same",
            Verdict::Neither => "absent-both",
            Verdict::Differs(_) => "DIFFERS",
            Verdict::OnlyLower => "ONLY-LOWER",
            Verdict::OnlyUpper => "ONLY-UPPER",
            Verdict::Unchecked(_) => "unchecked",
        }
    }
}

pub const MAX_PATHS: usize = 5;

/// `None` is "the tenant says it does not exist" (a measured 404), never "could
/// not read" — that is an error before this is reached.
pub fn judge(shape: Shape, lower: Option<&Value>, upper: Option<&Value>) -> Verdict {
    match (lower, upper) {
        (None, None) => Verdict::Neither,
        (Some(_), None) => Verdict::OnlyLower,
        (None, Some(_)) => Verdict::OnlyUpper,
        (Some(l), Some(u)) => {
            let mut paths = Vec::new();
            diff("", &normalise(shape, l), &normalise(shape, u), &mut paths);
            if paths.is_empty() {
                Verdict::Same
            } else {
                Verdict::Differs(paths)
            }
        }
    }
}

fn diff(at: &str, l: &Value, u: &Value, out: &mut Vec<String>) {
    if out.len() >= MAX_PATHS || l == u {
        return;
    }
    match (l, u) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let here = format!("{at}/{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => diff(&here, x, y, out),
                    (Some(_), None) => out.push(format!("{here} (lower only)")),
                    (None, Some(_)) => out.push(format!("{here} (upper only)")),
                    (None, None) => {}
                }
                if out.len() >= MAX_PATHS {
                    return;
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(&format!("{at}/{i}"), x, y, out);
            }
        }
        _ => out.push(if at.is_empty() { "/".into() } else { at.into() }),
    }
}

/// Whether two configured base URLs name one tenant, so `verify` cannot pass
/// by comparing a tenant with itself under two context names. Compared as
/// parsed origins — scheme, lowercased host, port with the scheme's default
/// filled in (`:443` is `https`'s) — plus the path without a trailing slash.
/// A URL that does not parse falls back to a case-insensitive string match.
pub fn same_tenant(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(x), Ok(y)) => {
            x.origin() == y.origin()
                && x.path().trim_end_matches('/') == y.path().trim_end_matches('/')
        }
        _ => a
            .trim_end_matches('/')
            .eq_ignore_ascii_case(b.trim_end_matches('/')),
    }
}

/// A CREST list must be complete for absence to mean anything: an item not
/// on a page that says more follow has not been shown to be absent.
pub fn complete(list: &Value) -> Result<&[Value], String> {
    complete_in(list, "result")
}

/// `complete` for a list whose entries are under `key` rather than `result`
/// (`GET /openidm/config` answers `configurations`).
fn complete_in<'a>(list: &'a Value, key: &str) -> Result<&'a [Value], String> {
    let result = list
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("list has no `{key}` array: {list}"))?;
    let cookie = list.get("pagedResultsCookie");
    if cookie.is_some_and(|c| !c.is_null()) {
        return Err("the list is paged (pagedResultsCookie set); absence cannot be shown".into());
    }
    match list.get("remainingPagedResults") {
        None | Some(Value::Null) => {}
        Some(n) if n.as_i64().is_some_and(|n| n <= 0) => {}
        Some(n) if n.is_u64() || n.is_i64() => {
            return Err("the list says more results remain; absence cannot be shown".into());
        }
        Some(other) => {
            return Err(format!(
                "remainingPagedResults is not an integer ({other}); the list cannot be read"
            ));
        }
    }
    Ok(result)
}

/// `complete`, plus every row must be an object carrying a string `field`.
/// A row a selector cannot read is not a row that failed to match: skipping
/// it would let the item it holds read as absent.
pub fn rows<'a>(list: &'a Value, field: &str) -> Result<Vec<(&'a str, &'a Value)>, String> {
    complete(list)?
        .iter()
        .map(|row| {
            row.get(field)
                .and_then(Value::as_str)
                .map(|key| (key, row))
                .ok_or_else(|| format!("list row without a string `{field}`: {row}"))
        })
        .collect()
}

/// What a fetched document must be before it is compared: an object, not a
/// CREST error envelope, and where the read named a resource, the resource
/// it named, carrying the field the comparison is about. Two `null`s, two
/// error envelopes answered with 200, or two scripts with no source would
/// otherwise compare `Same`.
///
/// For the object-only targets (secret mappings, realm services,
/// authentication settings) this is the whole check: their shapes are not
/// measured beyond being objects, and no field is required that was not.
/// The envelope test is a CREST error's own shape — an HTTP-error `code`
/// with a string `reason` or `message` — not a guess at the resource's.
pub fn check_document(target: &Target, doc: &Value) -> Result<(), String> {
    if let Some(code) = crest_error(doc) {
        return Err(format!(
            "read returned a CREST error envelope (code {code}): {doc}"
        ));
    }
    fn identified(doc: &Value, key: &str, want: &str, what: &str) -> Result<(), String> {
        match doc.get(key).and_then(Value::as_str) {
            Some(got) if got == want => Ok(()),
            Some(got) => Err(format!("{what} read returned {key} {got:?}, not {want:?}")),
            None if doc.is_object() => Err(format!("{what} document has no string `{key}`")),
            None => Err(format!("{what} read returned a non-object: {doc}")),
        }
    }
    match target {
        Target::Journey { name, .. } => {
            identified(&doc["tree"], "_id", name, "journey tree")?;
            let nodes = doc
                .get("nodes")
                .and_then(Value::as_object)
                .ok_or("journey composite without a nodes map")?;
            for (id, node) in nodes {
                identified(node, "_id", id, "journey node")?;
            }
            Ok(())
        }
        Target::Script { id, .. } => {
            identified(doc, "_id", id, "script")?;
            has_source(doc)
        }
        Target::ScriptByName { name, .. } => {
            identified(doc, "name", name, "script")?;
            has_source(doc)
        }
        Target::Themes { .. } => match doc {
            Value::Array(list) => match list.iter().find(|theme| !theme.is_object()) {
                None => Ok(()),
                Some(other) => Err(format!("theme list entry is not an object: {other}")),
            },
            other => Err(format!("theme list is not an array: {other}")),
        },
        Target::IdmEndpoint { name } => match doc.get("_id").and_then(Value::as_str) {
            Some(got) if reported_as(got, name) => endpoint_body(doc),
            _ => Err(format!("endpoint read did not return {name:?}: {doc}")),
        },
        Target::Cors { name } => match doc.get("_id").and_then(Value::as_str) {
            Some(got) if got.eq_ignore_ascii_case(name) => Ok(()),
            _ => Err(format!("CORS read did not return {name:?}: {doc}")),
        },
        Target::SecretMapping { .. }
        | Target::RealmService { .. }
        | Target::AuthenticationSettings { .. } => match doc {
            Value::Object(_) => Ok(()),
            other => Err(format!("expected an object, got {other}")),
        },
        Target::Unchecked(why) => Err(why.clone()),
    }
}

/// Whether config id `id` could be what a report calls `reported`: every
/// `/` as `-`, compared without case. Folding only *finds* candidates — see
/// `endpoint_id` for why a match that needed it is not accepted.
fn reported_as(id: &str, reported: &str) -> bool {
    id.replace('/', "-").eq_ignore_ascii_case(reported)
}

/// The one endpoint config id in `GET /openidm/config`'s `configurations`
/// that the report's name stands for, or `None` when the list has none.
///
/// Absence here is what lets a reported delete pass, so everything that
/// could fake it is an error instead: a paged list, a row with no string
/// `_id`, and a row that is an endpoint by one sign (`factoryPid`) but not
/// the other (an `endpoint/` id) — measured, the two always agree. Two
/// candidates are refused rather than picked, and so is a candidate that
/// matches only without case: the report lowercased CORS ids, but whether
/// it folds endpoint names is unmeasured, and treating a case-only match as
/// either the endpoint or "not found" would be a guess.
pub fn endpoint_id(list: &Value, reported: &str) -> Result<Option<String>, String> {
    let mut candidates = Vec::new();
    for row in complete_in(list, "configurations")? {
        let id = row
            .get("_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("config list row without a string `_id`: {row}"))?;
        let by_factory = row.get("factoryPid").and_then(Value::as_str) == Some("endpoint");
        if by_factory != id.starts_with("endpoint/") {
            return Err(format!(
                "config list row is an endpoint by one sign and not the other: {row}"
            ));
        }
        if by_factory && reported_as(id, reported) {
            candidates.push(id);
        }
    }
    match candidates[..] {
        [] => Ok(None),
        [id] if id.replace('/', "-") == reported => Ok(Some(id.to_string())),
        [id] => Err(format!(
            "{reported:?} matches endpoint {id:?} only without case, and whether \
             reports fold endpoint case is unmeasured"
        )),
        [a, b, ..] => Err(format!(
            "{reported:?} could be endpoint {a:?} or {b:?}; refusing to pick one"
        )),
    }
}

/// An endpoint document read as config id `id`: that exact `_id` — encoding
/// it back to the report's name would accept `endpoint/a-b` read for
/// `endpoint/a/b` — and the measured inline shape. An endpoint backed by a
/// `file`, or any other shape, is not one this compares.
pub fn check_endpoint(doc: &Value, id: &str) -> Result<(), String> {
    match doc.get("_id").and_then(Value::as_str) {
        Some(got) if got == id => endpoint_body(doc),
        _ => Err(format!("endpoint read as {id:?} returned another: {doc}")),
    }
}

/// The measured inline endpoint: string `source` and `type`. An `_id` alone
/// would compare `same` with nothing compared.
fn endpoint_body(doc: &Value) -> Result<(), String> {
    for field in ["source", "type"] {
        if !doc.get(field).is_some_and(Value::is_string) {
            return Err(format!(
                "endpoint document has no string `{field}` (only the inline shape is compared): {doc}"
            ));
        }
    }
    Ok(())
}

/// A script's source, the thing a script comparison is about: the
/// base64 `script` string (`docs/api/04-scripts.md`). A script read without
/// it compares its metadata alone.
fn has_source(doc: &Value) -> Result<(), String> {
    match doc.get("script") {
        Some(Value::String(_)) => Ok(()),
        Some(other) => Err(format!("script source is not a string: {other}")),
        None => Err("script document has no `script` source".into()),
    }
}

/// The HTTP status of a CREST error envelope — `{"code": 4xx/5xx, "reason":
/// …, "message": …}` — answered in place of the document.
fn crest_error(doc: &Value) -> Option<i64> {
    let code = doc
        .get("code")?
        .as_i64()
        .filter(|c| (400..600).contains(c))?;
    let worded = ["reason", "message"]
        .iter()
        .any(|key| doc.get(key).is_some_and(Value::is_string));
    worded.then_some(code)
}

/// A realm's theme list out of `ui/themerealm`. The document must carry a
/// `realm` object — anything else is an error, not absence. A realm that is
/// missing, or whose list is empty, holds no theme, so `Ok(None)`: two empty
/// lists show the reported theme on neither side, which `settle` then calls
/// unchecked unless the report said deleted.
pub fn themes<'a>(themerealm: &'a Value, realm: &str) -> Result<Option<&'a Value>, String> {
    let realms = themerealm
        .get("realm")
        .and_then(Value::as_object)
        .ok_or_else(|| "ui/themerealm has no `realm` object".to_string())?;
    match realms.get(realm) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) if list.is_empty() => Ok(None),
        Some(list @ Value::Array(_)) => Ok(Some(list)),
        Some(other) => Err(format!("themerealm.realm.{realm} is not an array: {other}")),
    }
}

/// The one entry of a list whose `field` equals `want` ignoring ASCII case.
/// None → `Ok(None)`; several → an error, never a pick.
pub fn one_by<'a>(list: &'a Value, field: &str, want: &str) -> Result<Option<&'a Value>, String> {
    let hits: Vec<&Value> = rows(list, field)?
        .into_iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(want))
        .map(|(_, row)| row)
        .collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(format!("{} entries match {field} {want:?}", hits.len())),
    }
}

/// A CREST `name eq "…"` filter, with `\\` and `"` escaped.
pub fn name_filter(name: &str) -> String {
    format!(
        "name eq \"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Absent from both tenants is in sync only when the report calls the item
/// deleted. For an item it calls added or modified, absence everywhere says
/// the lookup did not find what the report meant — the failure that once
/// passed three CORS entries and a theme as in sync — so it is unchecked.
pub fn settle(change: &str, verdict: Verdict) -> Verdict {
    match verdict {
        Verdict::Neither if change != "deleted" => Verdict::Unchecked(format!(
            "reported {change} but found on neither tenant; the lookup may not match the report's name"
        )),
        other => other,
    }
}

/// The realm-service id for the report's display name, from the realm's
/// service list (`{result: [{_id, name}]}`). The report says "Session
/// Service" where the list says "Session", and "Validation Service" where it
/// says "Validation Service" (measured 2026-10-26), so the name is tried as
/// given and without a trailing " Service". Two different matches are an
/// error, not a guess. No match → `Ok(None)`: not configured in that realm.
pub fn service_id(list: &Value, service: &str) -> Result<Option<String>, String> {
    let mut names = vec![service];
    if let Some(short) = service.strip_suffix(" Service") {
        names.push(short);
    }
    let mut ids: Vec<&str> = rows(list, "name")?
        .into_iter()
        .filter(|(name, _)| names.contains(name))
        .map(|(_, s)| {
            s.get("_id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("realm service without a string _id: {s}"))
        })
        .collect::<Result<_, _>>()?;
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).to_string())),
        _ => Err(format!("{service:?} names several realm services: {ids:?}")),
    }
}

/// Which documents list child nodes, and must: a tree lists its nodes as an
/// object keyed by id, a `PageNode` as an array of `{_id, nodeType}`. A tree
/// or page node *without* its `nodes` is an incomplete read, not a leaf — two
/// such responses would otherwise compare equal having read nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Tree,
    /// A node of this type; only `PageNode` holds children.
    Node,
}

pub fn node_refs(
    holder: Holder,
    node_type: &str,
    doc: &Value,
) -> Result<Vec<(String, String)>, String> {
    let required = match holder {
        Holder::Tree => true,
        Holder::Node => node_type == "PageNode",
    };
    if !required {
        return Ok(Vec::new());
    }
    match doc.get("nodes") {
        Some(Value::Object(map)) if holder == Holder::Tree => map
            .iter()
            .map(|(id, meta)| {
                let ty = meta
                    .get("nodeType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("node {id} has no string nodeType"))?;
                Ok((ty.to_string(), id.clone()))
            })
            .collect(),
        Some(Value::Array(list)) if holder == Holder::Node => list
            .iter()
            .map(|child| {
                let id = child.get("_id").and_then(Value::as_str);
                let ty = child.get("nodeType").and_then(Value::as_str);
                match (ty, id) {
                    (Some(ty), Some(id)) => Ok((ty.to_string(), id.to_string())),
                    _ => Err(format!("page-node child without _id/nodeType: {child}")),
                }
            })
            .collect(),
        None | Some(Value::Null) => Err(format!(
            "{} has no `nodes`; the read is incomplete",
            if holder == Holder::Tree {
                "the tree"
            } else {
                "a PageNode"
            }
        )),
        Some(other) => Err(format!("`nodes` has the wrong shape here: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(item: &str, name: &str, realm: Option<&str>, uid: Option<&str>) -> Entry {
        Entry {
            section: "AMConfig".into(),
            item: item.into(),
            change: "modified".into(),
            name: name.into(),
            realm: realm.map(str::to_string),
            uid: uid.map(str::to_string),
        }
    }

    #[test]
    fn report_items_map_to_their_measured_targets() {
        let uid = "ou=00000000-0000-4000-8000-000000000001,ou=scriptConfigurations,ou=default";
        assert_eq!(
            target(&entry("Scripts", "S", Some("/alpha"), Some(uid))),
            Target::Script {
                realm: "alpha".into(),
                id: "00000000-0000-4000-8000-000000000001".into()
            }
        );
        assert_eq!(
            target(&entry(
                "AM Services > Session Service",
                "opendj",
                Some("/bravo"),
                None
            )),
            Target::RealmService {
                realm: "bravo".into(),
                service: "Session Service".into()
            }
        );
        assert_eq!(
            target(&entry("CORS", "web", None, None)),
            Target::Cors { name: "web".into() }
        );
        assert_eq!(
            target(&entry("Scripts", "S", Some("/bravo"), None)),
            Target::ScriptByName {
                realm: "bravo".into(),
                name: "S".into()
            }
        );
    }

    /// What cannot be read is never counted as matching.
    #[test]
    fn unknown_items_and_unaddressable_realms_are_unchecked() {
        assert!(matches!(
            target(&entry("Global Scripts", "T", None, None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry("Journeys", "J", Some("/"), None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry("Journeys", "J", None, None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry(
                "Scripts",
                "S",
                Some("/alpha"),
                Some("ou=not-a-uuid,ou=x")
            )),
            Target::Unchecked(_)
        ));
        assert!(!Verdict::Unchecked("x".into()).in_sync());
    }

    #[test]
    fn judge_ignores_copy_metadata_and_names_the_differing_paths() {
        let lower = json!({"_rev": "1", "script": "a", "lastModifiedDate": 1});
        let same = json!({"_rev": "2", "script": "a", "lastModifiedDate": 2});
        assert_eq!(
            judge(Shape::Script, Some(&lower), Some(&same)),
            Verdict::Same
        );
        let other = json!({"script": "b", "extra": true});
        assert_eq!(
            judge(Shape::Script, Some(&lower), Some(&other)),
            Verdict::Differs(vec!["/extra (upper only)".into(), "/script".into()])
        );
        let tree =
            json!({"tree": {"_rev": "1", "entry": "a"}, "nodes": {"n": {"_rev": "9", "x": 1}}});
        let moved =
            json!({"tree": {"_rev": "2", "entry": "a"}, "nodes": {"n": {"_rev": "8", "x": 1}}});
        assert_eq!(
            judge(Shape::Journey, Some(&tree), Some(&moved)),
            Verdict::Same
        );
    }

    /// The discriminating case for root-scoped stripping: a key spelt like
    /// metadata *inside* the configuration is configuration.
    #[test]
    fn metadata_names_below_the_root_are_compared() {
        let a = json!({"_rev": "1", "settings": {"lastModifiedDate": 1, "_rev": "a"}});
        let b = json!({"_rev": "2", "settings": {"lastModifiedDate": 2, "_rev": "b"}});
        assert_eq!(
            judge(Shape::Rev, Some(&a), Some(&b)),
            Verdict::Differs(vec![
                "/settings/_rev".into(),
                "/settings/lastModifiedDate".into()
            ])
        );
        // A service document's audit-looking field is not a script's audit stamp.
        let c = json!({"lastModifiedBy": "x"});
        let d = json!({"lastModifiedBy": "y"});
        assert!(matches!(
            judge(Shape::Rev, Some(&c), Some(&d)),
            Verdict::Differs(_)
        ));
        let journey_a = json!({"tree": {"x": {"_rev": "1"}}, "nodes": {}});
        let journey_b = json!({"tree": {"x": {"_rev": "2"}}, "nodes": {}});
        assert!(matches!(
            judge(Shape::Journey, Some(&journey_a), Some(&journey_b)),
            Verdict::Differs(_)
        ));
    }

    #[test]
    fn presence_is_judged_whatever_the_report_called_the_change() {
        let v = json!({"a": 1});
        assert_eq!(judge(Shape::Rev, None, None), Verdict::Neither);
        assert!(Verdict::Neither.in_sync());
        assert_eq!(judge(Shape::Rev, Some(&v), None), Verdict::OnlyLower);
        assert_eq!(judge(Shape::Rev, None, Some(&v)), Verdict::OnlyUpper);
    }

    /// The report's name is the config id with `/` as `-`, which cannot be
    /// reversed: `endpoint-a-b` is `endpoint/a-b` or `endpoint/a/b`. The
    /// discriminating case is the nested id, which reversing gets wrong.
    #[test]
    fn endpoints_resolve_against_the_config_list() {
        assert_eq!(
            target(&entry("IDM Custom Endpoints", "endpoint-p", None, None)),
            Target::IdmEndpoint {
                name: "endpoint-p".into()
            }
        );
        let ep = |id: &str| json!({"_id": id, "factoryPid": "endpoint"});
        let list = |rows: Vec<Value>| json!({"_id": "", "configurations": rows});
        let base = list(vec![
            ep("endpoint/a/b"),
            ep("endpoint/Probe"),
            json!({"_id": "managed"}),
            ep("endpoint/c-d"),
            ep("endpoint/c/d"),
        ]);
        assert_eq!(
            endpoint_id(&base, "endpoint-a-b"),
            Ok(Some("endpoint/a/b".into()))
        );
        assert_eq!(
            endpoint_id(&base, "endpoint-Probe"),
            Ok(Some("endpoint/Probe".into()))
        );
        assert_eq!(endpoint_id(&base, "endpoint-gone"), Ok(None));
        // Folding finds a candidate but does not accept it.
        assert!(endpoint_id(&base, "endpoint-probe").is_err());
        assert!(endpoint_id(&base, "endpoint-c-d").is_err());
        assert!(endpoint_id(&json!({}), "endpoint-a").is_err());

        // Everything that could fake absence is an error, not `None`.
        let mut paged = list(vec![]);
        paged["pagedResultsCookie"] = json!("next");
        assert!(endpoint_id(&paged, "endpoint-a").is_err());
        let mut more = list(vec![]);
        more["remainingPagedResults"] = json!(1);
        assert!(endpoint_id(&more, "endpoint-a").is_err());
        let no_id = list(vec![json!({"factoryPid": "endpoint"})]);
        assert!(endpoint_id(&no_id, "endpoint-a").is_err());
        let bare = list(vec![json!({"pid": "x"})]);
        assert!(endpoint_id(&bare, "endpoint-a").is_err());
        let unflagged = list(vec![json!({"_id": "endpoint/a", "pid": "x"})]);
        assert!(endpoint_id(&unflagged, "endpoint-a").is_err());
        let misnamed = list(vec![ep("other/a")]);
        assert!(endpoint_id(&misnamed, "endpoint-a").is_err());

        let measured = json!({"_id": "endpoint/a/b", "source": "x", "type": "text/javascript"});
        assert_eq!(check_endpoint(&measured, "endpoint/a/b"), Ok(()));
        // Encodes to the same name, but is not the id that was read.
        let sibling = json!({"_id": "endpoint/a-b", "source": "x", "type": "text/javascript"});
        assert!(check_endpoint(&sibling, "endpoint/a/b").is_err());
        assert!(check_endpoint(&json!({"_id": "endpoint/a/b"}), "endpoint/a/b").is_err());
        let untyped = json!({"_id": "endpoint/a/b", "source": "x"});
        assert!(check_endpoint(&untyped, "endpoint/a/b").is_err());
        let file = json!({"_id": "endpoint/a/b", "file": "a.js", "type": "text/javascript"});
        assert!(check_endpoint(&file, "endpoint/a/b").is_err());

        let probe = Target::IdmEndpoint {
            name: "endpoint-a-b".into(),
        };
        assert_eq!(probe.shape(), Shape::Plain);
        assert!(check_document(&probe, &measured).is_ok());
        assert!(check_document(&probe, &json!({"_id": "endpoint/a/b"})).is_err());
        let other = json!({"_id": "endpoint/other", "source": "x", "type": "t"});
        assert!(check_document(&probe, &other).is_err());
    }

    #[test]
    fn themes_are_compared_by_whole_realm_list() {
        let doc = json!({"realm": {"alpha": [{"name": "Starter Theme"}]}});
        assert_eq!(
            target(&entry(
                "IDM Themes By Realm",
                "Starter",
                Some("alpha"),
                None
            )),
            Target::Themes {
                realm: "alpha".into()
            }
        );
        assert_eq!(
            themes(&doc, "alpha").unwrap(),
            Some(&json!([{"name": "Starter Theme"}]))
        );
        assert_eq!(themes(&doc, "bravo").unwrap(), None);
        assert!(themes(&json!({"realm": {"alpha": {}}}), "alpha").is_err());
        // A document without a `realm` object was not read, so it is not absence.
        assert!(themes(&json!({"realm": []}), "alpha").is_err());
        assert!(themes(&json!({}), "alpha").is_err());
        // An empty list holds no theme: absence, which `settle` refuses for
        // a reported modify.
        assert_eq!(
            themes(&json!({"realm": {"alpha": []}}), "alpha").unwrap(),
            None
        );
    }

    #[test]
    fn one_by_matches_without_case_and_refuses_ambiguity() {
        let list = json!({"result": [{"_id": "Client-A"}, {"_id": "Other"}]});
        assert_eq!(
            one_by(&list, "_id", "client-a").unwrap(),
            Some(&json!({"_id": "Client-A"}))
        );
        assert_eq!(one_by(&list, "_id", "missing").unwrap(), None);
        let twice = json!({"result": [{"_id": "A"}, {"_id": "a"}]});
        assert!(one_by(&twice, "_id", "a").is_err());
        assert!(one_by(&json!({}), "_id", "a").is_err());
        let paged = json!({"result": [], "pagedResultsCookie": "abc"});
        assert!(one_by(&paged, "_id", "a").is_err());
        let remaining =
            json!({"result": [], "pagedResultsCookie": null, "remainingPagedResults": 3});
        assert!(one_by(&remaining, "_id", "a").is_err());
        let typed = json!({"result": [], "remainingPagedResults": "3"});
        assert!(one_by(&typed, "_id", "a").is_err());
        let huge = json!({"result": [], "remainingPagedResults": u64::MAX});
        assert!(one_by(&huge, "_id", "a").is_err());
        // A row the selector cannot read is an error, not a non-match.
        assert!(one_by(&json!({"result": [{}]}), "_id", "a").is_err());
        assert!(service_id(&json!({"result": [{"_id": "x"}]}), "X").is_err());
        let measured = json!({"result": [{"_id": "x"}], "pagedResultsCookie": null, "remainingPagedResults": -1});
        assert_eq!(
            one_by(&measured, "_id", "x").unwrap(),
            Some(&json!({"_id": "x"}))
        );
    }

    #[test]
    fn name_filter_escapes_quotes() {
        assert_eq!(name_filter(r#"a"b\c"#), r#"name eq "a\"b\\c""#);
    }

    /// The discriminating case: absent everywhere for a reported modify is
    /// not in sync.
    #[test]
    fn absent_from_both_is_in_sync_only_for_a_reported_delete() {
        assert_eq!(settle("deleted", Verdict::Neither), Verdict::Neither);
        assert!(!settle("modified", Verdict::Neither).in_sync());
        assert!(!settle("added", Verdict::Neither).in_sync());
        assert_eq!(settle("modified", Verdict::Same), Verdict::Same);
    }

    #[test]
    fn service_ids_resolve_from_the_measured_list() {
        let list = json!({"result": [
            {"_id": "pushNotification", "name": "Push Notification Service"},
            {"_id": "session", "name": "Session"}
        ]});
        assert_eq!(
            service_id(&list, "Push Notification Service").unwrap(),
            Some("pushNotification".into())
        );
        assert_eq!(
            service_id(&list, "Session Service").unwrap(),
            Some("session".into())
        );
        assert_eq!(service_id(&list, "Validation Service").unwrap(), None);
        assert!(service_id(&json!({}), "x").is_err());
        let both =
            json!({"result": [{"_id": "a", "name": "X Service"}, {"_id": "b", "name": "X"}]});
        assert!(service_id(&both, "X Service").is_err());
    }

    #[test]
    fn node_refs_read_trees_and_page_nodes() {
        let tree = json!({"nodes": {"id1": {"nodeType": "PageNode"}}});
        assert_eq!(
            node_refs(Holder::Tree, "", &tree).unwrap(),
            vec![("PageNode".into(), "id1".into())]
        );
        let page = json!({"nodes": [{"_id": "c1", "nodeType": "ScriptedDecisionNode"}]});
        assert_eq!(
            node_refs(Holder::Node, "PageNode", &page).unwrap(),
            vec![("ScriptedDecisionNode".into(), "c1".into())]
        );
        // A leaf node lists nothing, whatever it carries.
        assert_eq!(
            node_refs(Holder::Node, "ScriptedDecisionNode", &json!({"x": 1})).unwrap(),
            vec![]
        );
        assert!(node_refs(Holder::Node, "PageNode", &json!({"nodes": [{"_id": "c1"}]})).is_err());
        // An empty tree is a tree with no nodes.
        assert_eq!(
            node_refs(Holder::Tree, "", &json!({"nodes": {}})).unwrap(),
            vec![]
        );
    }

    /// The discriminating case: a tree or page node read without `nodes` is
    /// incomplete, not childless — two such reads would otherwise match.
    #[test]
    fn a_holder_without_nodes_is_an_error() {
        assert!(node_refs(Holder::Tree, "", &json!({"entryNodeId": "x"})).is_err());
        assert!(node_refs(Holder::Node, "PageNode", &json!({"x": 1})).is_err());
        assert!(node_refs(Holder::Tree, "", &json!({"nodes": []})).is_err());
        assert!(node_refs(Holder::Node, "PageNode", &json!({"nodes": {}})).is_err());
    }

    #[test]
    fn same_tenant_ignores_case_and_a_trailing_slash() {
        assert!(same_tenant(
            "https://tenant.example.com/",
            "https://TENANT.example.com"
        ));
        assert!(same_tenant(
            "https://tenant.example.com:443",
            "https://tenant.example.com/"
        ));
        assert!(!same_tenant(
            "http://tenant.example.com",
            "https://tenant.example.com"
        ));
        // Credentials in the URL do not make it another tenant.
        assert!(same_tenant(
            "https://alice@tenant.example.com/",
            "https://bob@tenant.example.com"
        ));
        assert!(!same_tenant(
            "https://tenant.example.com",
            "https://other.example.com"
        ));
    }

    /// The discriminating cases: two `null`s, or two documents for some
    /// other resource, must not reach `judge` as comparable equals.
    #[test]
    fn a_document_must_be_the_resource_it_was_read_as() {
        let script = Target::Script {
            realm: "alpha".into(),
            id: "a".into(),
        };
        assert!(check_document(&script, &json!({"_id": "a", "script": "eA=="})).is_ok());
        assert!(check_document(&script, &Value::Null).is_err());
        assert!(check_document(&script, &json!({"_id": "b", "script": "eA=="})).is_err());
        // The right script without its source compares metadata alone.
        assert!(check_document(&script, &json!({"_id": "a"})).is_err());
        assert!(check_document(&script, &json!({"_id": "a", "script": null})).is_err());
        let by_name = Target::ScriptByName {
            realm: "alpha".into(),
            name: "S".into(),
        };
        assert!(check_document(&by_name, &json!({"name": "S", "script": "eA=="})).is_ok());
        assert!(check_document(&by_name, &json!({"name": "S"})).is_err());
        let journey = Target::Journey {
            realm: "alpha".into(),
            name: "J".into(),
        };
        let good = json!({"tree": {"_id": "J"}, "nodes": {"n1": {"_id": "n1"}}});
        assert!(check_document(&journey, &good).is_ok());
        let null_node = json!({"tree": {"_id": "J"}, "nodes": {"n1": null}});
        assert!(check_document(&journey, &null_node).is_err());
        let service = Target::RealmService {
            realm: "alpha".into(),
            service: "S".into(),
        };
        assert!(check_document(&service, &json!({})).is_ok());
        assert!(check_document(&service, &Value::Null).is_err());
        // An error answered with 200 is an object too; its CREST shape is
        // what gives it away. Control: a `code` that is not an HTTP error,
        // or one with no wording beside it, is resource data.
        let envelope = json!({"code": 403, "reason": "Forbidden", "message": "denied"});
        assert!(check_document(&service, &envelope).is_err());
        assert!(check_document(&service, &json!({"code": 403, "message": "x"})).is_err());
        assert!(check_document(&service, &json!({"code": 200, "reason": "OK"})).is_ok());
        assert!(check_document(&service, &json!({"code": 403})).is_ok());
        assert!(check_document(&service, &json!({"code": "403", "reason": "x"})).is_ok());
        let themes = Target::Themes {
            realm: "alpha".into(),
        };
        assert!(check_document(&themes, &json!([{"name": "T"}])).is_ok());
        assert!(check_document(&themes, &json!([{"name": "T"}, null])).is_err());
        assert!(check_document(&themes, &json!(["T"])).is_err());
        let cors = Target::Cors {
            name: "client-a".into(),
        };
        assert!(check_document(&cors, &json!({"_id": "Client-A"})).is_ok());
        assert!(check_document(&cors, &json!({"_id": "other"})).is_err());
    }

    #[test]
    fn entries_flatten_and_refuse_a_bad_shape() {
        let report = json!({"report": {"AMConfig": [{"configItem": "Journeys", "configChange": {
            "added": [{"name": "A", "realm": "/bravo"}], "deleted": [{"name": "B", "realm": "/alpha"}]}}]}});
        let got = entries(&report).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].change, "deleted");
        assert!(entries(&json!({"report": {"AMConfig": [{"configItem": "J", "configChange": {"added": [{"realm": 1, "name": "x"}]}}]}})).is_err());
    }
}
