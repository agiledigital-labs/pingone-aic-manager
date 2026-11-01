//! Durable relationship conversions: capture -> switch -> restore, and explicit
//! rollback. Pure census, schema/storage guards and injected sequencing live
//! here; conversion_cli supplies tenant I/O. The capture is the recovery
//! journal: intent is saved before requests and forgotten only after reads.
//! API contract: docs/api/10-managed-objects.md; Q14 in 99-quirks.

use std::collections::BTreeSet;
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
}

impl Link {
    pub fn of(edge: &relink::Edge) -> Self {
        Self {
            from_id: edge.link.from_id.clone(),
            to_collection: edge.link.to_collection.clone(),
            to_id: edge.link.to_id.clone(),
            properties: edge.properties.clone(),
        }
    }
    pub fn matches(&self, edge: &relink::Edge) -> bool {
        self == &Self::of(edge)
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
        for pair in &self.pairs {
            if pair.links.iter().any(|link| {
                link.properties.contains_key("_id") || link.properties.contains_key("_rev")
            }) {
                return Err(Error::Config(
                    "capture contains reserved edge properties; inspect the file".into(),
                ));
            }
            unique(&pair.links)?;
        }
        Ok(())
    }
}

/// Full census, including reverse-only links. No one-sided edge is silently
/// repaired by conversion. Target records must exist even on a one-way pair.
#[derive(Debug)]
pub struct Census {
    pub links: Vec<Link>,
    pub edges: Vec<relink::Edge>,
}

pub fn census(pair: &Pair, forward: &[Value], target: &[Value]) -> Result<Census> {
    let edges = forward
        .iter()
        .map(|record| relink::edges_of(record, &pair.forward.key))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Config)?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
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
        Some(end) => target
            .iter()
            .map(|record| relink::edges_of(record, &end.key))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Config)?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let mut problems = Vec::new();
    let reciprocal = |a: &relink::Edge, b: &relink::Edge| {
        a.link.edge_id.is_some()
            && a.link.edge_id == b.link.edge_id
            && a.link.from_id == b.link.to_id
            && a.link.to_id == b.link.from_id
            && b.link.to_collection == format!("managed/{}", pair.forward.object)
    };
    for edge in &edges {
        if edge.link.to_collection != pair.forward.target
            || !target_ids.contains(edge.link.to_id.as_str())
        {
            problems.push(format!(
                "{}: target record absent or outside the declared target",
                Link::of(edge).label()
            ));
        } else if pair.reverse.is_some() && !reverse.iter().any(|back| reciprocal(edge, back)) {
            problems.push(format!(
                "{} (edge {:?}): forward-only link",
                Link::of(edge).label(),
                edge.link.edge_id
            ));
        }
    }
    for edge in &reverse {
        if !edges.iter().any(|forward| reciprocal(forward, edge)) {
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
    let mut links = edges.iter().map(Link::of).collect::<Vec<_>>();
    unique(&links)?;
    sort(&mut links);
    Ok(Census { links, edges })
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

pub fn exact(pair: &Pair, current: &Census) -> Result<()> {
    let mut wanted = pair.links.clone();
    sort(&mut wanted);
    if wanted != current.links {
        let missing = wanted
            .iter()
            .filter(|link| !current.links.contains(link))
            .map(Link::label)
            .collect::<Vec<_>>();
        let extra = current
            .links
            .iter()
            .filter(|link| !wanted.contains(link))
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
            if verify::sections_mapping(repo, &entry.object, &entry.key) != ["genericMapping"]
                || repo["resourceMapping"]["genericMapping"][format!("managed/{}", entry.object)]["properties"]
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

/// Select reference ends only, avoiding a second visit from the reverse.
/// No report is consulted: selection compares live stored mappings.
pub fn promotion_pairs(
    lower_managed: &Value,
    lower_repo: &Value,
    upper_managed: &Value,
    upper_repo: &Value,
) -> Result<Vec<Pair>> {
    let mut selected = Vec::new();
    for located in verify::all_relationship_mappings(lower_repo) {
        let Some(object) = located.generic_object() else {
            continue;
        };
        if located.mapping.kind != "reference" {
            continue;
        }
        // Stale lower mappings are not relationship pairs to migrate.
        if !verify::properties(lower_managed, object)
            .is_some_and(|props| props.contains_key(&located.key))
        {
            continue;
        }
        let lower = storage::pair(lower_managed, lower_repo, object, &located.key)
            .map_err(Error::Config)?;
        if storage::mode(&lower) != storage::Mode::Mapped {
            return Err(Error::Config(format!("lower {} is mixed", located.label())));
        }
        let upper = storage::pair(upper_managed, upper_repo, object, &located.key)
            .map_err(Error::Config)?;
        if storage::mode(&upper) == storage::Mode::Mapped {
            continue;
        }
        let mut mappings = Vec::new();
        for end in std::iter::once(&lower.forward).chain(lower.reverse.iter()) {
            mappings.push(storage::Entry {
                object: end.object.clone(),
                key: end.key.clone(),
                value: lower_repo["resourceMapping"]["genericMapping"]
                    [format!("managed/{}", end.object)]["properties"][&end.key]
                    .clone(),
            });
        }
        let pair = Pair::of(&lower, mappings)?;
        guard(&pair, upper_managed, upper_repo, storage::Mode::Generic)?;
        selected.push(pair);
    }
    Ok(selected)
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
    mut create: impl AsyncFnMut(&Pair, &Link, Option<&str>) -> Result<()>,
    mut check: impl AsyncFnMut(&Pair, &Link, &relink::Edge) -> Result<()>,
) -> Result<()> {
    if matches!(capture.state, State::RollingBack | State::RolledBack) {
        return Err(Error::Config(
            "capture is rolling back or rolled back; finish rollback before a new conversion"
                .into(),
        ));
    }
    capture.state = State::Restoring;
    for index in 0..capture.pairs.len() {
        let pair = capture.pairs[index].clone();
        ready(&pair).await?;
        for (link_index, link) in pair.links.iter().enumerate() {
            let current = read(&pair, link).await?;
            let matching = current.edges.iter().find(|edge| link.matches(edge));
            if let Some(edge) = matching {
                check(&pair, link, edge).await?;
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
            });
            save(capture).await?;
            create(&pair, link, current.rev.as_deref()).await?;
            let after = read(&pair, link).await?;
            let edge = after
                .edges
                .iter()
                .find(|edge| link.matches(edge))
                .ok_or_else(|| {
                    Error::Config(format!(
                        "{} creation not confirmed; re-run restore-links with this capture",
                        link.label()
                    ))
                })?;
            check(&pair, link, edge).await?;
            capture.pending = None;
            save(capture).await?;
        }
    }
    Ok(())
}

/// Rollback's record removals must be confirmed before the mapping removal.
/// The caller supplies an empty-store verification as part of remove_links.
pub async fn rollback_with(
    capture: &mut Capture,
    _permit: &Permit,
    mut save: impl AsyncFnMut(&Capture) -> Result<()>,
    remove_links: impl AsyncFnOnce(&mut Capture) -> Result<()>,
    remove_mapping: impl AsyncFnOnce() -> Result<()>,
) -> Result<()> {
    if capture.origin != Origin::InPlace {
        return Err(Error::Config("rollback-map is only available for an in-place capture; it cannot write upper-environment config".into()));
    }
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    pub(super) fn fixture(array: bool) -> (Value, Value, Value, Pair) {
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

    fn records(array: bool) -> (Vec<Value>, Vec<Value>) {
        let edge = json!({"_ref":"managed/b/b1","_refResourceCollection":"managed/b","_refResourceId":"b1",
            "_refProperties":{"_id":"e1","_rev":"r1","note":"keep","nested":{"enabled":true}}});
        (
            vec![json!({"_id":"a1","link":if array {json!([edge])} else {edge}})],
            vec![
                json!({"_id":"b1","back":[{"_refResourceCollection":"managed/a","_refResourceId":"a1",
             "_refProperties":{"_id":"e1"}}]}),
            ],
        )
    }
    fn captured(array: bool) -> (Capture, Census) {
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
            let original = census.edges[0].clone();
            let existing = RefCell::new(Vec::<relink::Edge>::new());
            let writes = RefCell::new(0);
            let saves = RefCell::new(Vec::new());
            let permit = authorize(false, true).unwrap();
            for _ in 0..2 {
                restore_with(
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
                    async |_, _, _| {
                        assert!(saves.borrow().last().unwrap().is_some());
                        *writes.borrow_mut() += 1;
                        existing.borrow_mut().push(original.clone());
                        Ok(())
                    },
                    async |_, _, _| Ok(()),
                )
                .await
                .unwrap();
            }
            assert_eq!(*writes.borrow(), 1);
            assert!(capture.pending.is_none());
        }
    }

    #[tokio::test]
    async fn restore_refuses_unmapped_upper_before_any_record_write() {
        let (mut capture, _) = captured(true);
        let (managed, repo, _, _) = fixture(true);
        let writes = RefCell::new(0);
        let error = restore_with(
            &mut capture,
            &authorize(false, true).unwrap(),
            async |pair| guard(pair, &managed, &repo, storage::Mode::Mapped).map(|_| ()),
            async |_, _| {
                Ok(Observation {
                    edges: Vec::new(),
                    rev: None,
                })
            },
            async |_| Ok(()),
            async |_, _, _| {
                *writes.borrow_mut() += 1;
                Ok(())
            },
            async |_, _, _| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Mapped"));
        assert_eq!(*writes.borrow(), 0);
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
            promotion_pairs(&managed, &mapped, &managed, &generic)
                .unwrap()
                .len(),
            1
        );
        assert!(
            promotion_pairs(&managed, &mapped, &managed, &mapped)
                .unwrap()
                .is_empty()
        );
        assert!(
            promotion_pairs(&managed, &generic, &managed, &generic)
                .unwrap()
                .is_empty()
        );
        assert!(
            promotion_pairs(&managed, &generic, &managed, &mapped)
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rollback_removes_restored_links_before_mapping_and_rejects_promotion() {
        let (mut capture, _) = captured(true);
        let log = RefCell::new(Vec::new());
        let permit = authorize(false, true).unwrap();
        rollback_with(
            &mut capture,
            &permit,
            async |_| {
                log.borrow_mut().push("intent");
                Ok(())
            },
            async |_| {
                log.borrow_mut().push("remove links and confirm empty");
                Ok(())
            },
            async || {
                log.borrow_mut().push("remove mapping");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(
            *log.borrow(),
            ["intent", "remove links and confirm empty", "remove mapping"]
        );
        capture.origin = Origin::Promotion;
        log.borrow_mut().clear();
        assert!(
            rollback_with(
                &mut capture,
                &permit,
                async |_| Ok(()),
                async |_| Ok(()),
                async || Ok(())
            )
            .await
            .is_err()
        );
        assert!(authorize(true, true).is_none());
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
}
