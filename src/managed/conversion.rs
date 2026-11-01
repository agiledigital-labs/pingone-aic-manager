//! Durable relationship conversions: capture -> switch -> restore, and explicit
//! rollback. Pure census, schema/storage guards and injected sequencing live
//! here; conversion_cli supplies tenant I/O. The capture is the recovery
//! journal: intent is saved before requests and forgotten only after reads.
//! API contract: docs/api/10-managed-objects.md; Q14 in 99-quirks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{relink, storage, verify};
use crate::{Error, Result, statefile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
    pub object: String,
    pub key: String,
    pub array: bool,
    pub target: String,
    pub reverse_key: Option<String>,
}

impl End {
    fn of(end: &verify::End) -> Result<Self> {
        let [target] = end.targets.as_slice() else {
            return Err(Error::Config(
                "conversion requires one managed target".into(),
            ));
        };
        Ok(Self {
            object: end.object.clone(),
            key: end.key.clone(),
            array: end.array,
            target: target.clone(),
            reverse_key: end.reverse_key.clone(),
        })
    }

    pub fn label(&self) -> String {
        format!("{}.{}", self.object, self.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from_id: String,
    pub to_collection: String,
    pub to_id: String,
    pub properties: Map<String, Value>,
    #[serde(default)]
    pub reverse_properties: Option<Map<String, Value>>,
}

impl Link {
    pub fn of(edge: &relink::Edge) -> Self {
        Self {
            from_id: edge.link.from_id.clone(),
            to_collection: edge.link.to_collection.clone(),
            to_id: edge.link.to_id.clone(),
            properties: edge.properties.clone(),
            reverse_properties: None,
        }
    }
    pub fn matches(&self, edge: &relink::Edge) -> bool {
        self.from_id == edge.link.from_id
            && self.to_collection == edge.link.to_collection
            && self.to_id == edge.link.to_id
            && self.properties == edge.properties
    }
    pub fn label(&self) -> String {
        format!("{} -> {}/{}", self.from_id, self.to_collection, self.to_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub forward: End,
    pub reverse: Option<End>,
    pub mappings: Vec<storage::Entry>,
    pub links: Vec<Link>,
}

impl Pair {
    pub fn of(report: &verify::PairReport, mappings: Vec<storage::Entry>) -> Result<Self> {
        Ok(Self {
            forward: End::of(&report.forward)?,
            reverse: report.reverse.as_ref().map(End::of).transpose()?,
            mappings,
            links: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    InPlace,
    Promotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Captured,
    Switching,
    Restoring,
    Complete,
    RollingBack,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Create,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub pair: usize,
    pub link: usize,
    pub action: Action,
    #[serde(default)]
    pub edge_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub pair: usize,
    pub link: usize,
    pub edge_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    pub version: u8,
    pub id: String,
    pub tenant: String,
    pub base_url: String,
    pub origin: Origin,
    pub created_at: String,
    pub state: State,
    pub pending: Option<Intent>,
    pub pairs: Vec<Pair>,
    #[serde(default)]
    pub restored: Vec<Receipt>,
    #[serde(default)]
    pub new_pairs: Vec<String>,
}

impl Capture {
    pub fn new(tenant: String, base_url: String, origin: Origin, pairs: Vec<Pair>) -> Self {
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            tenant,
            base_url,
            origin,
            created_at: chrono::Utc::now().to_rfc3339(),
            state: State::Captured,
            pending: None,
            pairs,
            restored: Vec::new(),
            new_pairs: Vec::new(),
        }
    }
    pub fn count(&self) -> usize {
        self.pairs.iter().map(|pair| pair.links.len()).sum()
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.id.is_empty() || self.pairs.is_empty() {
            return Err(Error::Config(
                "invalid or unsupported relationship capture".into(),
            ));
        }
        let valid_index = |pair: usize, link: usize| {
            self.pairs
                .get(pair)
                .is_some_and(|pair| link < pair.links.len())
        };
        if self
            .pending
            .as_ref()
            .is_some_and(|intent| !valid_index(intent.pair, intent.link))
            || self.restored.iter().any(|receipt| {
                !valid_index(receipt.pair, receipt.link) || receipt.edge_id.is_empty()
            })
        {
            return Err(Error::Config(
                "capture has an invalid journal index; inspect it before continuing".into(),
            ));
        }
        let mut ends = BTreeSet::new();
        for pair in &self.pairs {
            for end in std::iter::once(&pair.forward).chain(pair.reverse.iter()) {
                if !ends.insert(end.label()) {
                    return Err(Error::Config(
                        "capture repeats a relationship end; inspect it before continuing".into(),
                    ));
                }
            }
            if pair.links.iter().any(|link| {
                std::iter::once(&link.properties)
                    .chain(link.reverse_properties.iter())
                    .any(|properties| {
                        properties.contains_key("_id") || properties.contains_key("_rev")
                    })
            }) {
                return Err(Error::Config(
                    "capture contains reserved edge properties; inspect the file".into(),
                ));
            }
            unique(&pair.links)?;
            validate_pair(pair)?;
        }
        Ok(())
    }
}

fn validate_pair(pair: &Pair) -> Result<()> {
    use serde_json::json;
    let invalid = || {
        Error::Config(format!(
            "invalid capture pair {}; inspect the file before continuing",
            pair.forward.label()
        ))
    };
    for end in std::iter::once(&pair.forward).chain(pair.reverse.iter()) {
        super::state::validate_property_key(&end.object).map_err(Error::Config)?;
        super::state::validate_property_key(&end.key).map_err(Error::Config)?;
        if !end.target.starts_with("managed/") {
            return Err(invalid());
        }
    }
    if pair.links.iter().any(|link| {
        link.from_id.is_empty()
            || link.to_id.is_empty()
            || link.to_collection != pair.forward.target
    }) {
        return Err(invalid());
    }
    if pair
        .links
        .iter()
        .any(|link| match (&pair.reverse, &link.reverse_properties) {
            (None, Some(_)) => true,
            (Some(_), Some(reverse)) => reverse != &link.properties,
            _ => false,
        })
    {
        return Err(invalid());
    }
    let mut expected = Vec::new();
    let forward = pair.mappings.first().ok_or_else(invalid)?;
    let column = forward
        .value
        .get("ldapAttribute")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if !column
        .strip_prefix("fr-idm-reference-")
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=10).contains(&number))
    {
        return Err(invalid());
    }
    expected.push(storage::Entry { section: forward.section.clone(), object: pair.forward.object.clone(), key: pair.forward.key.clone(),
        value: json!({"type":"reference","ldapAttribute":column,"primaryKey":"uid","resourcePath":pair.forward.target,"isMultiValued":pair.forward.array}) });
    if let Some(reverse) = &pair.reverse {
        if pair.forward.reverse_key.as_deref() != Some(&reverse.key)
            || reverse.reverse_key.as_deref() != Some(&pair.forward.key)
            || pair.forward.target != format!("managed/{}", reverse.object)
            || reverse.target != format!("managed/{}", pair.forward.object)
            || pair.forward.object == reverse.object && pair.forward.key == reverse.key
        {
            return Err(invalid());
        }
        expected.push(storage::Entry { section: pair.mappings.get(1).ok_or_else(invalid)?.section.clone(), object: reverse.object.clone(), key: reverse.key.clone(),
            value: json!({"type":"reverseReference","resourcePath":reverse.target,"propertyName":pair.forward.key,"isMultiValued":reverse.array}) });
    } else if pair.forward.reverse_key.is_some() {
        return Err(invalid());
    }
    if pair.mappings != expected {
        return Err(invalid());
    }
    Ok(())
}

/// Full census, including reverse-only links. No one-sided edge is silently
/// repaired by conversion. Target records must exist even on a one-way pair.
#[derive(Debug)]
pub struct Census {
    pub links: Vec<Link>,
    pub edges: Vec<relink::Edge>,
}

pub fn census(pair: &Pair, forward: &[Value], target: &[Value]) -> Result<Census> {
    let edges = decode_edges(forward, &pair.forward.key)?;
    let target_ids: BTreeSet<&str> = target
        .iter()
        .filter_map(|record| record.get("_id").and_then(Value::as_str))
        .collect();
    if target_ids.len() != target.len() {
        return Err(Error::Config(
            "target census has missing or duplicate record IDs".into(),
        ));
    }
    let reverse = match &pair.reverse {
        Some(end) => decode_edges(target, &end.key)?,
        None => Vec::new(),
    };
    let mut problems = Vec::new();
    let mut links = Vec::new();
    let source = format!("managed/{}", pair.forward.object);
    let forward_index = edges
        .iter()
        .filter_map(|edge| {
            edge.link
                .edge_id
                .as_deref()
                .map(|id| (id, edge.link.from_id.as_str(), edge.link.to_id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    let reverse_index = reverse
        .iter()
        .filter(|edge| edge.link.to_collection == source)
        .filter_map(|edge| {
            edge.link
                .edge_id
                .as_deref()
                .map(|id| (id, edge.link.to_id.as_str(), edge.link.from_id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    for edge in &edges {
        let mut link = Link::of(edge);
        if let Some(far) = reverse.iter().find(|far| {
            far.link.to_collection == source
                && far.link.from_id == edge.link.to_id
                && far.link.to_id == edge.link.from_id
                && far.link.edge_id == edge.link.edge_id
        }) {
            link.reverse_properties = Some(far.properties.clone());
            if far.properties != edge.properties {
                problems.push(format!("{} (edge {:?}): asymmetric custom _refProperties: forward {}; reverse {}. Restoring asymmetric values is unmeasured", link.label(), edge.link.edge_id, Value::Object(edge.properties.clone()), Value::Object(far.properties.clone())));
            }
        }
        links.push(link);
        if edge.link.to_collection != pair.forward.target
            || !target_ids.contains(edge.link.to_id.as_str())
        {
            problems.push(format!(
                "{}: target record absent or outside the declared target",
                Link::of(edge).label()
            ));
        } else if pair.reverse.is_some()
            && !edge.link.edge_id.as_deref().is_some_and(|id| {
                reverse_index.contains(&(id, edge.link.from_id.as_str(), edge.link.to_id.as_str()))
            })
        {
            problems.push(format!(
                "{} (edge {:?}): forward-only link",
                Link::of(edge).label(),
                edge.link.edge_id
            ));
        }
    }
    for edge in &reverse {
        if edge.link.to_collection != source
            || !edge.link.edge_id.as_deref().is_some_and(|id| {
                forward_index.contains(&(id, edge.link.to_id.as_str(), edge.link.from_id.as_str()))
            })
        {
            problems.push(format!(
                "{} (edge {:?}): reverse-only link",
                Link::of(edge).label(),
                edge.link.edge_id
            ));
        }
    }
    if !problems.is_empty() {
        return Err(Error::Config(format!(
            "refusing inconsistent relationship census:\n  {}\nResolve these links before converting",
            problems.join("\n  ")
        )));
    }
    unique(&links)?;
    sort(&mut links);
    Ok(Census { links, edges })
}

fn decode_edges(records: &[Value], key: &str) -> Result<Vec<relink::Edge>> {
    let mut edges = Vec::new();
    for record in records {
        let entries = match record.get(key) {
            Some(Value::Array(entries)) => entries.iter().collect::<Vec<_>>(),
            Some(entry @ Value::Object(_)) => vec![entry],
            _ => Vec::new(),
        };
        if entries.iter().any(|entry| {
            entry
                .get("_refProperties")
                .is_some_and(|properties| !properties.is_object())
        }) {
            return Err(Error::Config(format!(
                "{key} has malformed _refProperties; refuse to discard values during capture"
            )));
        }
        edges.extend(relink::edges_of(record, key).map_err(Error::Config)?);
    }
    Ok(edges)
}

fn identity(link: &Link) -> (&str, &str, &str) {
    (&link.from_id, &link.to_collection, &link.to_id)
}
fn sort(links: &mut [Link]) {
    links.sort_by(|a, b| identity(a).cmp(&identity(b)));
}
fn unique(links: &[Link]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for link in links {
        if !seen.insert(identity(link)) {
            return Err(Error::Config(format!(
                "duplicate forward/target links at {}; resolve them before conversion",
                link.label()
            )));
        }
    }
    Ok(())
}

fn expected_links(pair: &Pair) -> Vec<Link> {
    pair.links
        .iter()
        .cloned()
        .map(|mut link| {
            // Older captures recorded only forward values. They may resume only
            // against symmetric reverse values, the sole supported restore shape.
            if pair.reverse.is_some() && link.reverse_properties.is_none() {
                link.reverse_properties = Some(link.properties.clone());
            }
            link
        })
        .collect()
}

pub fn subset(pair: &Pair, current: &Census) -> Result<()> {
    let expected = expected_links(pair);
    let wanted = expected
        .iter()
        .map(|link| (identity(link), link))
        .collect::<BTreeMap<_, _>>();
    for link in &current.links {
        if wanted.get(&identity(link)).copied() != Some(link) {
            return Err(Error::Config(format!(
                "{} has an extra/altered mapped link {}; inspect before restoring",
                pair.forward.label(),
                link.label()
            )));
        }
    }
    Ok(())
}

pub fn exact(pair: &Pair, current: &Census) -> Result<()> {
    let mut wanted = expected_links(pair);
    sort(&mut wanted);
    if wanted != current.links {
        let wanted_index = wanted
            .iter()
            .map(|link| (identity(link), link))
            .collect::<BTreeMap<_, _>>();
        let current_index = current
            .links
            .iter()
            .map(|link| (identity(link), link))
            .collect::<BTreeMap<_, _>>();
        let missing = wanted
            .iter()
            .filter(|link| current_index.get(&identity(link)).copied() != Some(*link))
            .map(Link::label)
            .collect::<Vec<_>>();
        let extra = current
            .links
            .iter()
            .filter(|link| wanted_index.get(&identity(link)).copied() != Some(*link))
            .map(Link::label)
            .collect::<Vec<_>>();
        return Err(Error::Config(format!(
            "{} differs from capture (target or custom _refProperties): missing {missing:?}; extra/altered {extra:?}",
            pair.forward.label()
        )));
    }
    Ok(())
}

/// Q14 may remove a confirmed declaration. Check the live shape at both
/// boundaries and on each record operation; a read is evidence, not a lock.
pub fn guard(
    pair: &Pair,
    managed: &Value,
    repo: &Value,
    expected: storage::Mode,
) -> Result<verify::PairReport> {
    let report = storage::pair(managed, repo, &pair.forward.object, &pair.forward.key)
        .map_err(|reason| Error::Config(format!("Q14/schema guard for {}: {reason}. Both declarations must be present on this fresh read; nothing further written", pair.forward.label())))?;
    if End::of(&report.forward)? != pair.forward
        || report.reverse.as_ref().map(End::of).transpose()? != pair.reverse
    {
        return Err(Error::Config(format!(
            "Q14/schema guard: {} changed shape or lost its reverse; stop and restore the declaration before resuming",
            pair.forward.label()
        )));
    }
    if storage::mode(&report) != expected {
        return Err(Error::Config(format!(
            "{} must be {expected:?} in repo.ds before this operation; read the mapping and resume at the correct stage",
            pair.forward.label()
        )));
    }
    if expected == storage::Mode::Mapped {
        for entry in &pair.mappings {
            if verify::sections_mapping(repo, &entry.object, &entry.key) != [entry.section.as_str()]
                || repo["resourceMapping"][&entry.section][format!("managed/{}", entry.object)]["properties"]
                    [&entry.key]
                    != entry.value
            {
                return Err(Error::Config(format!(
                    "{}.{} mapping differs from the captured conversion; inspect it before restoring",
                    entry.object, entry.key
                )));
            }
        }
    }
    Ok(report)
}

/// A raw binding includes every field and its section/location. Comparison
/// precedes schema parsing: unchanged stock multi-target pairs are supported
/// as exclusions even though conversion cannot safely recreate them.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Binding {
    section: String,
    resource: String,
    key: String,
    value: Value,
}

type Endpoint = (String, String);

fn connected(seed: Endpoint, mappings: &[verify::Located]) -> BTreeSet<Endpoint> {
    let mut ends = BTreeSet::from([seed]);
    loop {
        let before = ends.len();
        for entry in mappings {
            if entry.mapping.kind != "reverseReference" {
                continue;
            }
            let (Some(resource), Some(key)) =
                (&entry.mapping.resource_path, &entry.mapping.property_name)
            else {
                continue;
            };
            let owner = (entry.resource.clone(), entry.key.clone());
            let reference = (resource.clone(), key.clone());
            if ends.contains(&owner) || ends.contains(&reference) {
                ends.insert(owner);
                ends.insert(reference);
            }
        }
        if ends.len() == before {
            return ends;
        }
    }
}

fn binding(repo: &Value, ends: &BTreeSet<Endpoint>) -> Vec<Binding> {
    let mut entries = Vec::new();
    for (section, resources) in repo
        .get("resourceMapping")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for (resource, key) in ends {
            if let Some(value) = resources
                .get(resource)
                .and_then(|entry| entry.get("properties"))
                .and_then(|properties| properties.get(key))
            {
                entries.push(Binding {
                    section: section.clone(),
                    resource: resource.clone(),
                    key: key.clone(),
                    value: value.clone(),
                });
            }
        }
    }
    entries
}

/// Compare connected bindings in both directions. Promotion replaces repo.ds;
/// a removed upper binding is unsafe too (inferred from storage selection).
pub fn promotion_pairs(
    lower_managed: &Value,
    lower_repo: &Value,
    upper_managed: &Value,
    upper_repo: &Value,
    new_pairs: &BTreeSet<String>,
) -> Result<Vec<Pair>> {
    let lower_entries = verify::all_relationship_mappings(lower_repo);
    let mut all = lower_entries.clone();
    all.extend(verify::all_relationship_mappings(upper_repo));
    let mut visited = BTreeSet::new();
    let mut selected = Vec::new();
    let mut acknowledged = BTreeSet::new();
    for located in &all {
        let endpoint = (located.resource.clone(), located.key.clone());
        if visited.contains(&endpoint) {
            continue;
        }
        let ends = connected(endpoint, &all);
        visited.extend(ends.iter().cloned());
        let lower_binding = binding(lower_repo, &ends);
        let upper_binding = binding(upper_repo, &ends);
        if lower_binding == upper_binding {
            continue;
        }
        let reference = lower_binding
            .iter()
            .find(|entry| entry.value["type"] == "reference");
        // A changed lower binding with a missing declaration is an inconsistent
        // read, even when upper also has a mapping. Never label it stale/skip.
        for entry in &lower_binding {
            if let Some(object) = entry.resource.strip_prefix("managed/")
                && !verify::properties(lower_managed, object)
                    .is_some_and(|props| props.contains_key(&entry.key))
            {
                return Err(Error::Config(format!(
                    "Q14: lower {object}.{} has a mapping without a declaration. Restore the declaration and re-read; if genuinely stale, clean it with relationship repair before promotion",
                    entry.key
                )));
            }
        }
        if !upper_binding.is_empty() {
            return Err(Error::Config(format!(
                "promotion would change/remove relationship binding for {}: lower {}; upper {}. Do not promote until the bindings are resolved; capture-links cannot migrate between mapped stores",
                located.label(),
                serde_json::to_string(&lower_binding)?,
                serde_json::to_string(&upper_binding)?
            )));
        }
        let reference = reference.ok_or_else(|| Error::Config(format!("lower {} has an incomplete binding without a reference; repair it before promotion", located.label())))?;
        let object = reference.resource.strip_prefix("managed/").ok_or_else(|| {
            Error::Config(format!(
                "unsupported conversion candidate {}.{} in {}: reference owner is not managed",
                reference.resource, reference.key, reference.section
            ))
        })?;
        let label = format!("{object}.{}", reference.key);
        let lower =
            storage::pair(lower_managed, lower_repo, object, &reference.key).map_err(|reason| {
                Error::Config(format!(
                    "unsupported lower conversion candidate {label}: {reason}"
                ))
            })?;
        if storage::mode(&lower) != storage::Mode::Mapped {
            return Err(Error::Config(format!(
                "lower {label} is mixed; repair before promotion"
            )));
        }
        if new_pairs.contains(&label) {
            for end in std::iter::once(&lower.forward).chain(lower.reverse.iter()) {
                if verify::properties(upper_managed, &end.object)
                    .is_some_and(|props| props.contains_key(&end.key))
                    || !verify::sections_mapping(upper_repo, &end.object, &end.key).is_empty()
                {
                    return Err(Error::Config(format!(
                        "{}.{} already has an upper declaration or mapping; remove --new-pair {label} and capture it normally",
                        end.object, end.key
                    )));
                }
            }
            acknowledged.insert(label);
            continue;
        }
        let mappings = std::iter::once(&lower.forward)
            .chain(lower.reverse.iter())
            .map(|end| {
                let matches = lower_binding
                    .iter()
                    .filter(|entry| {
                        entry.resource == format!("managed/{}", end.object) && entry.key == end.key
                    })
                    .collect::<Vec<_>>();
                let [entry] = matches.as_slice() else {
                    return Err(Error::Config(format!(
                        "{label} has ambiguous storage entries; repair before conversion"
                    )));
                };
                Ok(storage::Entry {
                    section: entry.section.clone(),
                    object: end.object.clone(),
                    key: end.key.clone(),
                    value: entry.value.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let pair = Pair::of(&lower, mappings)?;
        validate_pair(&pair).map_err(|reason| {
            Error::Config(format!(
                "unsupported lower conversion candidate {label}: {reason}"
            ))
        })?;
        guard(&pair, upper_managed, upper_repo, storage::Mode::Generic)
            .map_err(|reason| Error::Config(format!("upper declaration cannot be trusted (possible Q14): {reason}. Only if both ends have never existed in upper, explicitly acknowledge --new-pair {label} after checking their history")))?;
        selected.push(pair);
    }
    if &acknowledged != new_pairs {
        return Err(Error::Config(format!(
            "--new-pair did not name lower mapped reference ends: {:?}",
            new_pairs.difference(&acknowledged).collect::<Vec<_>>()
        )));
    }
    Ok(selected)
}

/// The four live documents used for promotion eligibility. Even an empty
/// selection gets a second read: Q14 can invalidate a seemingly empty plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionRead {
    pub lower_managed: Value,
    pub lower_repo: Value,
    pub upper_managed: Value,
    pub upper_repo: Value,
}

impl PromotionRead {
    pub fn select(&self, new_pairs: &BTreeSet<String>) -> Result<Vec<Pair>> {
        promotion_pairs(
            &self.lower_managed,
            &self.lower_repo,
            &self.upper_managed,
            &self.upper_repo,
            new_pairs,
        )
    }
}

pub struct PromotionPlan {
    pub initial: PromotionRead,
    pub selection: Vec<Pair>,
    pub linked: Vec<Pair>,
}

/// Shared planning sequence; the empty/no-file branch is reached only after
/// the fresh read. Inject reads beneath selection, never a preselected result.
pub async fn prepare_promotion(
    new_pairs: &BTreeSet<String>,
    mut read: impl AsyncFnMut() -> Result<PromotionRead>,
    mut links: impl AsyncFnMut(&Pair) -> Result<Vec<Link>>,
) -> Result<PromotionPlan> {
    let initial = read().await?;
    let selection = initial.select(new_pairs)?;
    let mut linked = selection.clone();
    for pair in &mut linked {
        pair.links = links(pair).await?;
    }
    linked.retain(|pair| !pair.links.is_empty());
    recheck_promotion(&initial, &selection, new_pairs, async || read().await).await?;
    Ok(PromotionPlan {
        initial,
        selection,
        linked,
    })
}

pub async fn recheck_promotion(
    initial: &PromotionRead,
    selection: &[Pair],
    new_pairs: &BTreeSet<String>,
    read: impl AsyncFnOnce() -> Result<PromotionRead>,
) -> Result<()> {
    let fresh = read().await?;
    let current = fresh.select(new_pairs)?;
    if &fresh != initial || current != selection {
        return Err(Error::Config("declarations or repo.ds changed during promotion capture (possible Q14); re-run against the stable locked configuration before promoting".into()));
    }
    Ok(())
}

pub struct Permit(());
pub fn authorize(dry_run: bool, consented: bool) -> Option<Permit> {
    (!dry_run && consented).then_some(Permit(()))
}

/// Persist the original links before any switch request. A failed Q14 read
/// leaves a durable capture and sends nothing.
pub async fn switch_with(
    capture: &mut Capture,
    _permit: &Permit,
    mut save: impl AsyncFnMut(&Capture) -> Result<()>,
    recheck: impl AsyncFnOnce() -> Result<()>,
    send: impl AsyncFnOnce() -> Result<()>,
) -> Result<()> {
    capture.state = State::Switching;
    save(capture).await?;
    recheck().await?;
    send().await
}

pub struct Observation {
    pub edges: Vec<relink::Edge>,
    pub rev: Option<String>,
}

/// Semantic reads determine idempotence, never the journal's progress flags.
/// `check` confirms far resolution and mapped runtime before clearing intent.
pub async fn restore_with(
    capture: &mut Capture,
    _permit: &Permit,
    mut ready: impl AsyncFnMut(&Pair) -> Result<()>,
    mut read: impl AsyncFnMut(&Pair, &Link) -> Result<Observation>,
    mut save: impl AsyncFnMut(&Capture) -> Result<()>,
    mut create: impl AsyncFnMut(&Pair, &Link, Option<&str>) -> Result<Option<String>>,
    mut check: impl AsyncFnMut(&Pair, &Link, &relink::Edge) -> Result<()>,
) -> Result<()> {
    if capture.state == State::RolledBack {
        return Err(Error::Config(
            "capture is rolled back; use a new capture for a new conversion".into(),
        ));
    }
    capture.state = State::Restoring;
    let pending = capture.pending.clone();
    for index in ordered(
        capture.pairs.len(),
        pending.as_ref().map(|intent| intent.pair),
    ) {
        let pair = capture.pairs[index].clone();
        ready(&pair).await?;
        let first = pending
            .as_ref()
            .filter(|intent| intent.pair == index)
            .map(|intent| intent.link);
        for link_index in ordered(pair.links.len(), first) {
            let link = &pair.links[link_index];
            let current = read(&pair, link).await?;
            let matching = current.edges.iter().find(|edge| link.matches(edge));
            if let Some(edge) = matching {
                check(&pair, link, edge).await?;
                receipt(capture, index, link_index, edge)?;
                capture.pending = None;
                save(capture).await?;
                continue;
            }
            if current.edges.iter().any(|edge| {
                edge.link.to_collection == link.to_collection && edge.link.to_id == link.to_id
            }) || !pair.forward.array && !current.edges.is_empty()
            {
                return Err(Error::Config(format!(
                    "{} {} is occupied or has different custom properties; nothing overwritten",
                    pair.forward.label(),
                    link.label()
                )));
            }
            capture.pending = Some(Intent {
                pair: index,
                link: link_index,
                action: Action::Create,
                edge_id: None,
            });
            save(capture).await?;
            let written = create(&pair, link, current.rev.as_deref()).await?;
            if let Some(intent) = &mut capture.pending {
                intent.edge_id = written.clone();
            }
            save(capture).await?;
            let after = read(&pair, link).await?;
            let edge = after
                .edges
                .iter()
                .find(|edge| {
                    link.matches(edge)
                        && written
                            .as_deref()
                            .is_none_or(|id| edge.link.edge_id.as_deref() == Some(id))
                })
                .ok_or_else(|| {
                    Error::Config(format!(
                        "{} creation not confirmed; re-run restore-links with this capture",
                        link.label()
                    ))
                })?;
            check(&pair, link, edge).await?;
            receipt(capture, index, link_index, edge)?;
            capture.pending = None;
            save(capture).await?;
        }
    }
    Ok(())
}

fn ordered(len: usize, first: Option<usize>) -> Vec<usize> {
    first
        .into_iter()
        .chain((0..len).filter(|index| Some(*index) != first))
        .collect()
}

/// A restore may have landed without a response; unknown progress is handled
/// first so no unrelated link can erase that intent.
pub(super) fn receipt(
    capture: &mut Capture,
    pair: usize,
    link: usize,
    edge: &relink::Edge,
) -> Result<()> {
    let edge_id = edge.link.edge_id.clone().ok_or_else(|| {
        Error::Config("restored edge has no ID; keep its intent and inspect before retrying".into())
    })?;
    capture
        .restored
        .retain(|receipt| receipt.pair != pair || receipt.link != link);
    capture.restored.push(Receipt {
        pair,
        link,
        edge_id,
    });
    Ok(())
}

/// Rollback's record removals must be confirmed before the mapping removal.
/// The caller supplies an empty-store verification as part of remove_links.
pub async fn rollback_with(
    capture: &mut Capture,
    _permit: &Permit,
    confirm_config_write: impl AsyncFnOnce() -> Result<()>,
    mut save: impl AsyncFnMut(&Capture) -> Result<()>,
    remove_links: impl AsyncFnOnce(&mut Capture) -> Result<()>,
    remove_mapping: impl AsyncFnOnce() -> Result<()>,
) -> Result<()> {
    if capture.origin != Origin::InPlace {
        return Err(Error::Config("rollback-map is only available for an in-place capture; it cannot write upper-environment config".into()));
    }
    confirm_config_write().await?;
    capture.state = State::RollingBack;
    save(capture).await?;
    remove_links(capture).await?;
    remove_mapping().await?;
    // Generic-store read-back and completeness are checked by the caller
    // before marking RolledBack; config acceptance alone is insufficient.
    Ok(())
}

const DESCRIBE: statefile::Describe = statefile::Describe {
    what: "relationship capture",
    unreadable,
};
fn unreadable(path: &Path, error: &serde_json::Error) -> Error {
    Error::Config(format!(
        "capture {} is unreadable ({error}); preserve it and inspect before any retry",
        path.display()
    ))
}

pub fn load(path: &Path) -> Result<Capture> {
    let mut entries = statefile::load::<Capture>(path, &DESCRIBE)?;
    if entries.len() != 1 {
        return Err(Error::Config(format!(
            "{} must contain exactly one capture bundle; do not overwrite it",
            path.display()
        )));
    }
    let capture = entries.remove(0);
    capture.validate()?;
    Ok(capture)
}

/// A run lock spans the network work. The statefile lock protects individual
/// atomic journal updates. Compare the previous file to catch manual edits.
pub struct FileSession {
    pub path: PathBuf,
    before: Option<Capture>,
    _guard: File,
}
impl FileSession {
    pub async fn open(path: PathBuf, expected: Option<Capture>) -> Result<Self> {
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let mut name = path.as_os_str().to_os_string();
            name.push(".run.lock");
            let guard = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(PathBuf::from(name))?;
            guard.try_lock().map_err(|error| {
                Error::Config(format!(
                    "capture {} is in use by another run ({error})",
                    path.display()
                ))
            })?;
            let actual = if path.exists() {
                Some(load(&path)?)
            } else {
                None
            };
            if actual != expected {
                return Err(Error::Config(
                    "capture changed since planning; nothing written".into(),
                ));
            }
            Ok(Self {
                path,
                before: actual,
                _guard: guard,
            })
        })
        .await
        .map_err(|error| Error::Config(error.to_string()))?
    }
    pub async fn save(&mut self, capture: &Capture) -> Result<()> {
        capture.validate()?;
        let (path, before, capture) = (self.path.clone(), self.before.clone(), capture.clone());
        let saved = capture.clone();
        tokio::task::spawn_blocking(move || {
            statefile::update(
                &path,
                &DESCRIBE,
                |entries: &mut Vec<Capture>| -> Result<()> {
                    if entries.as_slice() != before.as_slice() {
                        return Err(Error::Config(
                            "capture changed while running; preserve both files and inspect".into(),
                        ));
                    }
                    *entries = vec![capture];
                    Ok(())
                },
            )?
        })
        .await
        .map_err(|error| Error::Config(error.to_string()))??;
        self.before = Some(saved);
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    pub(crate) fn fixture(array: bool) -> (Value, Value, Value, Pair) {
        let link = json!({"type":"relationship", "resourceCollection":[{"path":"managed/b"}],
            "reverseRelationship":true,"reversePropertyName":"back"});
        let managed = json!({"objects":[
            {"name":"a","schema":{"properties":{"link":if array {json!({"type":"array","items":link})} else {link}}}},
            {"name":"b","schema":{"properties":{"back":{"type":"array","items":{
                "type":"relationship","resourceCollection":[{"path":"managed/a"}],
                "reverseRelationship":true,"reversePropertyName":"link"}}}}}
        ]});
        let generic = json!({"resourceMapping":{"genericMapping":{}}});
        let addition = storage::plan(&managed, &generic, "a", "link").unwrap();
        let pair = Pair::of(&addition.pair, addition.entries).unwrap();
        (managed, generic, addition.after, pair)
    }

    pub(crate) fn records(array: bool) -> (Vec<Value>, Vec<Value>) {
        let edge = json!({"_ref":"managed/b/b1","_refResourceCollection":"managed/b","_refResourceId":"b1",
            "_refProperties":{"_id":"e1","_rev":"r1","note":"keep","nested":{"enabled":true}}});
        (
            vec![json!({"_id":"a1","link":if array {json!([edge])} else {edge}})],
            vec![
                json!({"_id":"b1","back":[{"_refResourceCollection":"managed/a","_refResourceId":"a1",
             "_refProperties":{"_id":"e1","note":"keep","nested":{"enabled":true}}}]}),
            ],
        )
    }
    pub(crate) fn captured(array: bool) -> (Capture, Census) {
        let (_, _, _, mut pair) = fixture(array);
        let (forward, back) = records(array);
        let census = census(&pair, &forward, &back).unwrap();
        pair.links = census.links.clone();
        (
            Capture::new(
                "dev".into(),
                "https://tenant.example.com".into(),
                Origin::InPlace,
                vec![pair],
            ),
            census,
        )
    }

    #[tokio::test]
    async fn capture_round_trip_preserves_custom_properties_drops_id_rev() {
        let (capture, _) = captured(true);
        assert_eq!(
            capture.pairs[0].links[0].reverse_properties.as_ref(),
            Some(&capture.pairs[0].links[0].properties)
        );
        let dir = std::env::temp_dir().join(format!("aic-capture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("capture.json");
        let mut session = FileSession::open(path.clone(), None).await.unwrap();
        session.save(&capture).await.unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(capture, loaded);
        assert_eq!(
            loaded.pairs[0].links[0].properties,
            json!({"note":"keep","nested":{"enabled":true}})
                .as_object()
                .unwrap()
                .clone()
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(session);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn capture_is_written_before_switch_and_q14_failure_sends_nothing() {
        let (mut capture, _) = captured(true);
        let log = RefCell::new(Vec::new());
        let permit = authorize(false, true).unwrap();
        switch_with(
            &mut capture,
            &permit,
            async |_| {
                log.borrow_mut().push("save");
                Ok(())
            },
            async || {
                log.borrow_mut().push("fresh declarations");
                Ok(())
            },
            async || {
                log.borrow_mut().push("switch");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*log.borrow(), ["save", "fresh declarations", "switch"]);
        log.borrow_mut().clear();
        let (mut managed, repo, _, _) = fixture(true);
        managed["objects"][1]["schema"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("back");
        let pair = capture.pairs[0].clone();
        let error = switch_with(
            &mut capture,
            &permit,
            async |_| {
                log.borrow_mut().push("save");
                Ok(())
            },
            async || guard(&pair, &managed, &repo, storage::Mode::Generic).map(|_| ()),
            async || {
                log.borrow_mut().push("switch");
                Ok(())
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Q14"));
        assert_eq!(*log.borrow(), ["save"]);
    }

    #[tokio::test]
    async fn restore_after_partial_run_recreates_only_missing_for_one_and_many() {
        for array in [false, true] {
            let (mut capture, census) = captured(array);
            let mut originals = census.edges;
            if array {
                let mut second = originals[0].clone();
                second.link.to_id = "b2".into();
                second.link.edge_id = Some("e2".into());
                capture.pairs[0].links.push(Link::of(&second));
                originals.push(second);
            }
            let existing = RefCell::new(Vec::<relink::Edge>::new());
            let writes = RefCell::new(0);
            let saves = RefCell::new(Vec::new());
            let permit = authorize(false, true).unwrap();
            for attempt in 0..3 {
                let result = restore_with(
                    &mut capture,
                    &permit,
                    async |_| Ok(()),
                    async |_, _| {
                        Ok(Observation {
                            edges: existing.borrow().clone(),
                            rev: Some("r".into()),
                        })
                    },
                    async |capture| {
                        saves.borrow_mut().push(capture.pending.clone());
                        Ok(())
                    },
                    async |_, link, _| {
                        assert!(saves.borrow().last().unwrap().is_some());
                        *writes.borrow_mut() += 1;
                        let mut written = originals
                            .iter()
                            .find(|edge| link.matches(edge))
                            .unwrap()
                            .clone();
                        written.link.edge_id = Some(format!("mapped-{}", link.to_id));
                        existing.borrow_mut().push(written);
                        if attempt == 0 {
                            return Err(Error::Config("response lost after request landed".into()));
                        }
                        Ok(None)
                    },
                    async |_, _, _| Ok(()),
                )
                .await;
                if attempt == 0 {
                    assert!(result.is_err());
                    assert!(capture.pending.is_some());
                } else {
                    result.unwrap();
                }
            }
            assert_eq!(*writes.borrow(), if array { 2 } else { 1 });
            assert!(capture.pending.is_none());
            assert_eq!(capture.restored.len(), capture.count());
        }
    }

    #[tokio::test]
    async fn pending_later_link_is_reconciled_before_any_other_intent_is_forgotten() {
        let (mut capture, census) = captured(true);
        let first = census.edges[0].clone();
        let mut second = first.clone();
        second.link.to_id = "b2".into();
        second.link.edge_id = Some("e2".into());
        capture.pairs[0].links.push(Link::of(&second));
        capture.pending = Some(Intent {
            pair: 0,
            link: 1,
            action: Action::Create,
            edge_id: None,
        });
        let reads = RefCell::new(Vec::new());
        restore_with(
            &mut capture,
            &authorize(false, true).unwrap(),
            async |_| Ok(()),
            async |_, link| {
                reads.borrow_mut().push(link.to_id.clone());
                Ok(Observation {
                    edges: vec![first.clone(), second.clone()],
                    rev: None,
                })
            },
            async |_| Ok(()),
            async |_, _, _| panic!("nothing is missing"),
            async |_, _, _| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(*reads.borrow(), ["b2", "b1"]);
        assert!(capture.pending.is_none());
    }

    #[test]
    fn verify_rejects_extra_link_and_different_custom_properties() {
        let (capture, mut census) = captured(true);
        let pair = &capture.pairs[0];
        exact(pair, &census).unwrap();
        let mut extra = census.links[0].clone();
        extra.to_id = "extra".into();
        census.links.push(extra);
        assert!(exact(pair, &census).is_err());
        census.links.pop();
        census.links[0]
            .properties
            .insert("note".into(), json!("different"));
        assert!(
            exact(pair, &census)
                .unwrap_err()
                .to_string()
                .contains("_refProperties")
        );
    }

    #[test]
    fn promotion_selection_exactly_lower_mapped_upper_generic() {
        let (managed, generic, mapped, _) = fixture(true);
        assert_eq!(
            promotion_pairs(&managed, &mapped, &managed, &generic, &BTreeSet::new())
                .unwrap()
                .len(),
            1
        );
        assert!(
            promotion_pairs(&managed, &mapped, &managed, &mapped, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
        assert!(
            promotion_pairs(&managed, &generic, &managed, &generic, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
        assert!(promotion_pairs(&managed, &generic, &managed, &mapped, &BTreeSet::new()).is_err());
    }

    #[test]
    fn census_refuses_and_lists_forward_only_and_reverse_only_links() {
        let (_, _, _, pair) = fixture(true);
        let (forward, mut back) = records(true);
        back[0]["back"][0]["_refProperties"]["_id"] = json!("another-edge");
        let error = census(&pair, &forward, &back).unwrap_err().to_string();
        assert!(error.contains("forward-only"));
        assert!(error.contains("reverse-only"));
    }
    #[tokio::test]
    async fn rollback_writer_removes_both_ends_in_one_put_and_keeps_type_metadata() {
        let (managed, _, mapped, pair) = fixture(true);
        let plan = super::super::repair::plan_rollback(&managed, &mapped, &pair).unwrap();
        let reads = RefCell::new(vec![plan.after().clone(), mapped.clone()]);
        let sends = RefCell::new(0);
        let permit = super::super::repair::authorize(false, true).unwrap();
        let applied = super::super::repair::apply_with(
            &plan,
            &permit,
            async || Ok(managed.clone()),
            async || Ok(reads.borrow_mut().pop().unwrap()),
            |_| Ok(PathBuf::from("backup.json")),
            async |body| {
                *sends.borrow_mut() += 1;
                assert!(verify::mapped(body, "a", "link").is_none());
                assert!(verify::mapped(body, "b", "back").is_none());
                assert_eq!(
                    body["resourceMapping"]["genericMapping"]["managed/a"]["dnTemplate"],
                    mapped["resourceMapping"]["genericMapping"]["managed/a"]["dnTemplate"]
                );
                Ok(())
            },
            &[],
        )
        .await
        .unwrap();
        assert_eq!(*sends.borrow(), 1);
        assert_eq!(applied.readback, super::super::repair::Readback::Confirmed);
    }

    #[tokio::test]
    async fn rollback_failure_in_link_removal_never_reaches_mapping_writer() {
        let (mut capture, _) = captured(true);
        let sent = RefCell::new(false);
        let result = rollback_with(
            &mut capture,
            &authorize(false, true).unwrap(),
            async || Ok(()),
            async |_| Ok(()),
            async |_| Err(Error::Config("removal not confirmed".into())),
            async || {
                *sent.borrow_mut() = true;
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        assert!(!*sent.borrow());
    }

    #[tokio::test]
    async fn capture_run_lock_and_manual_edit_guard_preserve_existing_data() {
        let (capture, _) = captured(true);
        let dir = std::env::temp_dir().join(format!("aic-capture-lock-{}", uuid::Uuid::new_v4()));
        let path = dir.join("capture.json");
        let mut file = FileSession::open(path.clone(), None).await.unwrap();
        file.save(&capture).await.unwrap();
        assert!(
            FileSession::open(path.clone(), Some(capture.clone()))
                .await
                .is_err()
        );
        let mut changed = capture.clone();
        changed.id = "manual-edit".into();
        statefile::update(&path, &DESCRIBE, |entries: &mut Vec<Capture>| {
            *entries = vec![changed.clone()]
        })
        .unwrap();
        assert!(file.save(&capture).await.is_err());
        assert_eq!(load(&path).unwrap().id, "manual-edit");
        drop(file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_attributes_and_capture_mapping_are_refused_without_silent_loss() {
        let (_, _, _, pair) = fixture(true);
        let (mut forward, back) = records(true);
        forward[0]["link"][0]["_refProperties"] = json!("must not discard");
        assert!(
            census(&pair, &forward, &back)
                .unwrap_err()
                .to_string()
                .contains("malformed _refProperties")
        );
        let (mut capture, _) = captured(true);
        capture.pairs[0].mappings.clear();
        assert!(capture.validate().is_err());
        let (mut capture, _) = captured(true);
        capture.pending = Some(Intent {
            pair: 0,
            link: 99,
            action: Action::Create,
            edge_id: None,
        });
        assert!(capture.validate().is_err());
    }

    #[tokio::test]
    async fn dry_run_does_not_mint_conversion_write_permit() {
        assert!(authorize(true, true).is_none());
        assert!(authorize(true, false).is_none());
        // Both remote engines require &Permit; the CLI returns its preview
        // before opening FileSession, so no local journal is written either.
    }
    #[tokio::test]
    async fn resume_uses_captured_values_even_if_a_confirmed_edge_id_changed() {
        let (mut capture, census) = captured(true);
        capture.restored.push(Receipt {
            pair: 0,
            link: 0,
            edge_id: "old-counter".into(),
        });
        capture.pending = Some(Intent {
            pair: 0,
            link: 0,
            action: Action::Create,
            edge_id: Some("old-counter".into()),
        });
        let mut current = census.edges[0].clone();
        current.link.edge_id = Some("current-counter".into());
        restore_with(
            &mut capture,
            &authorize(false, true).unwrap(),
            async |_| Ok(()),
            async |_, _| {
                Ok(Observation {
                    edges: vec![current.clone()],
                    rev: None,
                })
            },
            async |_| Ok(()),
            async |_, _, _| panic!("captured values already present"),
            async |_, _, _| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(capture.restored[0].edge_id, "current-counter");
        assert!(capture.pending.is_none());
    }
    #[test]
    fn promotion_missing_upper_requires_scoped_new_pair_acknowledgment() {
        let (managed, generic, mapped, _) = fixture(true);
        let absent = serde_json::json!({"objects":[]});
        let error = promotion_pairs(&managed, &mapped, &absent, &generic, &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Q14"));
        assert!(error.contains("--new-pair a.link"));
        let acknowledged = BTreeSet::from(["a.link".into()]);
        assert!(
            promotion_pairs(&managed, &mapped, &absent, &generic, &acknowledged)
                .unwrap()
                .is_empty()
        );
        let unknown = BTreeSet::from(["a.unknown".into()]);
        assert!(promotion_pairs(&managed, &mapped, &managed, &generic, &unknown).is_err());

        // A new lower pair must not prevent capture of an existing upper pair.
        let mut lower = managed.clone();
        let mut extra = managed["objects"].as_array().unwrap().clone();
        extra[0]["name"] = json!("c");
        extra[0]["schema"]["properties"]["link"]["items"]["resourceCollection"][0]["path"] =
            json!("managed/d");
        extra[1]["name"] = json!("d");
        extra[1]["schema"]["properties"]["back"]["items"]["resourceCollection"][0]["path"] =
            json!("managed/c");
        lower["objects"].as_array_mut().unwrap().extend(extra);
        let both = storage::plan(&lower, &mapped, "c", "link").unwrap().after;
        let selected = promotion_pairs(
            &lower,
            &both,
            &managed,
            &generic,
            &BTreeSet::from(["c.link".into()]),
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].forward.label(), "a.link");
    }

    #[test]
    fn new_pair_acknowledgment_cannot_skip_an_existing_upper_end() {
        let (managed, generic, mapped, _) = fixture(true);
        let acknowledged = BTreeSet::from(["a.link".into()]);
        assert!(promotion_pairs(&managed, &mapped, &managed, &generic, &acknowledged).is_err());
        let mut partial = managed.clone();
        partial["objects"][0]["schema"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("link");
        assert!(promotion_pairs(&managed, &mapped, &partial, &generic, &acknowledged).is_err());
        let absent = json!({"objects":[]});
        assert!(promotion_pairs(&managed, &mapped, &absent, &mapped, &acknowledged).is_err());
    }
    #[tokio::test]
    async fn rollback_config_denial_happens_before_any_link_removal() {
        let (mut capture, _) = captured(true);
        let removed = RefCell::new(false);
        let error = rollback_with(
            &mut capture,
            &authorize(false, true).unwrap(),
            async || Err(Error::Config("config write denied".into())),
            async |_| Ok(()),
            async |_| {
                *removed.borrow_mut() = true;
                Ok(())
            },
            async || panic!("mapping removal cannot be reached"),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("denied"));
        assert!(!*removed.borrow());
        assert_ne!(capture.state, State::RollingBack);
    }

    #[test]
    fn rollback_authority_plan_keeps_the_entire_mapping_document_unchanged() {
        let (managed, _, mapped, pair) = fixture(true);
        let plan = super::super::repair::plan_confirm(&managed, &mapped, &pair).unwrap();
        assert_eq!(plan.before(), plan.after());
        assert!(verify::mapped(plan.after(), "a", "link").is_some());
        assert!(verify::mapped(plan.after(), "b", "back").is_some());
        super::super::repair::recheck(&plan, &managed, &mapped).unwrap();
    } // Regression: equivalent stock multi-target mappings must be excluded before
    // conversion's deliberately narrower schema validation.
    fn stock_multi_target() -> (Value, Value) {
        let managed = json!({"objects":[
            {"name":"alpha_user","schema":{"properties":{"authzRoles":{"type":"array","items":{
                "type":"relationship","resourceCollection":[{"path":"internal/role"},{"path":"managed/role"}],
                "reverseRelationship":true,"reversePropertyName":"authzMembers"}}}}},
            {"name":"role","schema":{"properties":{"authzMembers":{"type":"array","items":{
                "type":"relationship","resourceCollection":[{"path":"managed/alpha_user"}],
                "reverseRelationship":true,"reversePropertyName":"authzRoles"}}}}}
        ]});
        let repo = json!({"resourceMapping":{
            "genericMapping":{"managed/alpha_user":{"properties":{"authzRoles":{
                "type":"reference","ldapAttribute":"fr-idm-reference-1","primaryKey":"uid",
                "resourcePath":"managed/role","isMultiValued":true}}},
                "managed/role":{"properties":{"authzMembers":{"type":"reverseReference",
                    "resourcePath":"managed/alpha_user","propertyName":"authzRoles","isMultiValued":true}}}},
            "explicitMapping":{"internal/role":{"properties":{"authzMembers":{"type":"reverseReference",
                "resourcePath":"managed/alpha_user","propertyName":"authzRoles","isMultiValued":true}}}}
        }});
        (managed, repo)
    }

    #[test]
    fn promotion_skips_identical_stock_multi_target_bindings_before_schema_validation() {
        let (managed, repo) = stock_multi_target();
        assert!(storage::pair(&managed, &repo, "alpha_user", "authzRoles").is_err());
        assert!(
            promotion_pairs(&managed, &repo, &managed, &repo, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
        // Equivalent bindings remain safe exclusions even during a missing declaration read.
        assert!(
            promotion_pairs(&json!({}), &repo, &json!({}), &repo, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn promotion_refuses_unsupported_multi_target_conversion_candidate_by_name() {
        let (managed, repo) = stock_multi_target();
        let error = promotion_pairs(&managed, &repo, &managed, &json!({}), &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("alpha_user.authzRoles"), "{error}");
        assert!(
            error.contains("unsupported lower conversion candidate"),
            "{error}"
        );
    }

    #[test]
    fn promotion_refuses_different_column_bindings() {
        let (managed, _, lower, _) = fixture(true);
        let mut upper = lower.clone();
        upper["resourceMapping"]["genericMapping"]["managed/a"]["properties"]["link"]["ldapAttribute"] =
            json!("fr-idm-reference-8");
        let error = promotion_pairs(&managed, &lower, &managed, &upper, &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("fr-idm-reference-1") && error.contains("fr-idm-reference-8"),
            "{error}"
        );
        assert!(error.contains("Do not promote"));
    }

    #[test]
    fn promotion_refuses_opposite_reference_owner() {
        let (managed, _, lower, _) = fixture(true);
        let mut upper = lower.clone();
        upper["resourceMapping"]["genericMapping"]["managed/b"]["properties"]["back"] = json!({"type":"reference","resourcePath":"managed/a","ldapAttribute":"fr-idm-reference-1","primaryKey":"uid","isMultiValued":true});
        upper["resourceMapping"]["genericMapping"]["managed/a"]["properties"]["link"] = json!({"type":"reverseReference","resourcePath":"managed/b","propertyName":"back","isMultiValued":true});
        let error = promotion_pairs(&managed, &lower, &managed, &upper, &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("change/remove relationship binding"),
            "{error}"
        );
        assert!(error.contains("managed/a") && error.contains("managed/b"));
    }

    #[test]
    fn promotion_checks_explicit_mapping_bindings_and_captures_generic_upper() {
        let (managed, generic, mut lower, _) = fixture(true);
        let entries = lower["resourceMapping"]
            .as_object_mut()
            .unwrap()
            .remove("genericMapping")
            .unwrap();
        lower["resourceMapping"]["explicitMapping"] = entries;
        let pairs =
            promotion_pairs(&managed, &lower, &managed, &generic, &BTreeSet::new()).unwrap();
        assert_eq!(pairs.len(), 1);
        guard(&pairs[0], &managed, &lower, storage::Mode::Mapped).unwrap();
        let capture = Capture::new(
            "uat".into(),
            "https://tenant.example.com".into(),
            Origin::Promotion,
            pairs,
        );
        capture.validate().unwrap();
        assert!(
            promotion_pairs(&managed, &lower, &managed, &lower, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
        let (_, _, upper_generic_section, _) = fixture(true);
        let error = promotion_pairs(
            &managed,
            &lower,
            &managed,
            &upper_generic_section,
            &BTreeSet::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("explicitMapping") && error.contains("genericMapping"));
    }

    #[test]
    fn promotion_refuses_upper_mapped_lower_generic() {
        let (managed, lower, upper, _) = fixture(true);
        let error = promotion_pairs(&managed, &lower, &managed, &upper, &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("change/remove relationship binding"),
            "{error}"
        );
        assert!(
            error.contains("lower []") && error.contains("upper [{"),
            "{error}"
        );
    }

    #[test]
    fn promotion_lower_mapping_without_declaration_blocks_q14_instead_of_skipping() {
        let (mut managed, generic, mapped, _) = fixture(true);
        managed["objects"][0]["schema"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("link");
        let error = promotion_pairs(&managed, &mapped, &managed, &generic, &BTreeSet::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Q14") && error.contains("a.link") && error.contains("repair"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn promotion_empty_selection_rechecks_missing_then_reappearing_declaration() {
        let (managed, _, mapped, _) = fixture(true);
        let mut missing = managed.clone();
        missing["objects"][0]["schema"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("link");
        let initial = PromotionRead {
            lower_managed: missing,
            lower_repo: mapped.clone(),
            upper_managed: managed.clone(),
            upper_repo: mapped.clone(),
        };
        let reads = RefCell::new(0);
        let error = prepare_promotion(
            &BTreeSet::new(),
            async || {
                let index = *reads.borrow();
                *reads.borrow_mut() += 1;
                Ok(if index == 0 {
                    initial.clone()
                } else {
                    PromotionRead {
                        lower_managed: managed.clone(),
                        ..initial.clone()
                    }
                })
            },
            async |_| panic!("equivalent bindings must never be scanned"),
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert_eq!(*reads.borrow(), 2);
        assert!(error.contains("Q14"), "{error}");
        let stable = prepare_promotion(
            &BTreeSet::new(),
            async || Ok(initial.clone()),
            async |_| panic!("equivalent bindings must never be scanned"),
        )
        .await
        .unwrap();
        assert!(stable.selection.is_empty() && stable.linked.is_empty());
    }

    #[test]
    fn census_refuses_matching_edge_ids_with_distinct_or_missing_reverse_custom_values() {
        let (_, _, _, pair) = fixture(true);
        for note in [Some(json!("different")), None] {
            let (forward, mut reverse) = records(true);
            let properties = reverse[0]["back"][0]["_refProperties"]
                .as_object_mut()
                .unwrap();
            properties.remove("note");
            if let Some(note) = note {
                properties.insert("note".into(), note);
            }
            let error = census(&pair, &forward, &reverse).unwrap_err().to_string();
            assert!(
                error.contains("asymmetric custom _refProperties")
                    && error.contains("a1 -> managed/b/b1"),
                "{error}"
            );
        }
    }

    #[test]
    fn exact_verification_checks_captured_reverse_custom_values() {
        let (mut capture, census) = captured(true);
        capture.pairs[0].links[0]
            .reverse_properties
            .as_mut()
            .unwrap()
            .insert("note".into(), json!("different"));
        assert!(
            exact(&capture.pairs[0], &census)
                .unwrap_err()
                .to_string()
                .contains("_refProperties")
        );
        assert!(capture.validate().is_err());
    }
    // Regression: mode equality used to conceal changes to target, shape and
    // reverse binding as well as the reference's LDAP column.
    #[test]
    fn promotion_comparison_checks_target_cardinality_and_reverse_fields() {
        let (managed, _, lower, _) = fixture(true);
        for (object, key, field, value) in [
            ("managed/a", "link", "resourcePath", json!("managed/other")),
            ("managed/a", "link", "isMultiValued", json!(false)),
            ("managed/a", "link", "primaryKey", json!("other")),
            ("managed/b", "back", "propertyName", json!("other")),
            ("managed/b", "back", "isMultiValued", json!(false)),
            ("managed/b", "back", "type", json!("simple")),
        ] {
            let mut upper = lower.clone();
            upper["resourceMapping"]["genericMapping"][object]["properties"][key][field] = value;
            let error = promotion_pairs(&managed, &lower, &managed, &upper, &BTreeSet::new())
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("change/remove relationship binding"),
                "{field}: {error}"
            );
        }
    }
}
