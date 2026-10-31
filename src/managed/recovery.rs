//! What a link write that is two requests long leaves behind if it stops
//! between them.
//!
//! `relationship relink` removes a link and re-creates it; `verify --probe`
//! adds a test link and removes it. Both are two requests, and a process killed
//! between them used to leave the only description of the link in memory: a
//! relink's removed edge, with its `_refProperties`, could not be discovered by
//! a second run, because a re-run scans the forward links that survive. So
//! each write pair records its intent here **before** its first request and
//! forgets it only once a read shows the pair finished.
//!
//! Like the SAML rotation journal this is not a transaction log: what is true
//! is read from the tenant ([`judge`]); the file only says what to look for.
//! It lives under `.aic/` as `managed-links.json`, with the two sidecars every
//! [`crate::statefile`] file has. It names records and edges, not values.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::ProjectConfig;
use crate::managed::relink::Edge;
use crate::statefile;
use crate::{Error, Result};

pub const JOURNAL_FILE: &str = "managed-links.json";

const DESCRIBE: statefile::Describe = statefile::Describe {
    what: "the managed link recovery journal",
    unreadable,
};

fn unreadable(file: &Path, error: &serde_json::Error) -> Error {
    Error::Config(format!(
        "the managed link recovery journal {} is not readable ({error}); it names links an \
         interrupted `relationship relink` or `verify --probe` may have left removed or added. \
         Read it by eye, check those records with `aic idm get`, then move it aside",
        file.display()
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A link removed in order to be re-created.
    Relink,
    /// A test link added in order to be removed.
    Probe,
}

/// One write pair in flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// Addresses this entry; a fresh UUID per write pair.
    pub id: String,
    pub tenant: String,
    pub kind: Kind,
    /// The record the writes go to: `managed/<object>/<from_id>`, property
    /// `key`.
    pub object: String,
    pub key: String,
    pub array: bool,
    pub from_id: String,
    pub to_collection: String,
    pub to_id: String,
    /// What a re-creation restores (relink only).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub properties: Map<String, Value>,
    /// Relink: the edge being removed. Probe: the test edge, once a read has
    /// shown it; absent before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    /// RFC 3339.
    pub started_at: String,
}

impl Pending {
    pub fn relink(tenant: &str, object: &str, key: &str, array: bool, edge: &Edge) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tenant: tenant.into(),
            kind: Kind::Relink,
            object: object.into(),
            key: key.into(),
            array,
            from_id: edge.link.from_id.clone(),
            to_collection: edge.link.to_collection.clone(),
            to_id: edge.link.to_id.clone(),
            properties: edge.properties.clone(),
            edge_id: edge.link.edge_id.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn probe(
        tenant: &str,
        (object, key, array): (&str, &str, bool),
        from_id: &str,
        far_object: &str,
        far_id: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tenant: tenant.into(),
            kind: Kind::Probe,
            object: object.into(),
            key: key.into(),
            array,
            from_id: from_id.into(),
            to_collection: format!("managed/{far_object}"),
            to_id: far_id.into(),
            properties: Map::new(),
            edge_id: None,
            started_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn field(&self) -> String {
        format!("{}.{}", self.object, self.key)
    }

    pub fn label(&self) -> String {
        format!(
            "managed/{}/{}.{} -> {}/{}",
            self.object, self.from_id, self.key, self.to_collection, self.to_id
        )
    }
}

/// What the near record says about a pending write pair now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The pair finished, or its first write never landed: forget it.
    Settled,
    /// Relink: the link is gone and nothing replaced it — re-create it.
    Lost,
    /// Relink: the link is gone and the has-one property now holds another,
    /// so re-creating it would displace that one.
    LostAndOccupied,
    /// Probe: the test edge is still there — remove it.
    Leftover { edge_id: String },
    /// Probe whose edge was never read back, and the record links the far
    /// record: it may be the probe's or one that was there before.
    Ambiguous,
}

/// Judges `pending` against the near record's current links of its property.
pub fn judge(pending: &Pending, current: &[Edge]) -> Recovery {
    let to_target = |edge: &&Edge| {
        edge.link.to_id == pending.to_id && edge.link.to_collection == pending.to_collection
    };
    match pending.kind {
        Kind::Relink => {
            // Either the old edge is still there (the remove never landed) or
            // a link to the same record is (re-created, under a new edge id).
            if current.iter().any(|edge| to_target(&edge)) {
                Recovery::Settled
            } else if !pending.array && !current.is_empty() {
                Recovery::LostAndOccupied
            } else {
                Recovery::Lost
            }
        }
        Kind::Probe => match &pending.edge_id {
            Some(edge_id) => {
                if current
                    .iter()
                    .any(|edge| edge.link.edge_id.as_deref() == Some(edge_id.as_str()))
                {
                    Recovery::Leftover {
                        edge_id: edge_id.clone(),
                    }
                } else {
                    Recovery::Settled
                }
            }
            None if current.iter().any(|edge| to_target(&edge)) => Recovery::Ambiguous,
            None => Recovery::Settled,
        },
    }
}

fn path() -> PathBuf {
    ProjectConfig::dir().join(JOURNAL_FILE)
}

/// Every pending pair for `tenant`.
pub fn pending(tenant: &str) -> Result<Vec<Pending>> {
    pending_at(&path(), tenant)
}

pub fn record(entry: &Pending) -> Result<()> {
    record_at(&path(), entry)
}

pub fn set_edge(id: &str, edge_id: &str) -> Result<()> {
    set_edge_at(&path(), id, edge_id)
}

pub fn clear(id: &str) -> Result<()> {
    clear_at(&path(), id)
}

pub fn file() -> PathBuf {
    path()
}

pub(crate) fn pending_at(file: &Path, tenant: &str) -> Result<Vec<Pending>> {
    Ok(statefile::load::<Pending>(file, &DESCRIBE)?
        .into_iter()
        .filter(|entry| entry.tenant == tenant)
        .collect())
}

pub(crate) fn record_at(file: &Path, entry: &Pending) -> Result<()> {
    statefile::update(file, &DESCRIBE, |entries: &mut Vec<Pending>| {
        entries.retain(|existing| existing.id != entry.id);
        entries.push(entry.clone());
    })
}

pub(crate) fn set_edge_at(file: &Path, id: &str, edge_id: &str) -> Result<()> {
    statefile::update(file, &DESCRIBE, |entries: &mut Vec<Pending>| {
        for entry in entries.iter_mut().filter(|entry| entry.id == id) {
            entry.edge_id = Some(edge_id.to_string());
        }
    })
}

pub(crate) fn clear_at(file: &Path, id: &str) -> Result<()> {
    statefile::update(file, &DESCRIBE, |entries: &mut Vec<Pending>| {
        entries.retain(|existing| existing.id != id);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::verify::Link;
    use serde_json::json;

    fn edge(to: &str, edge_id: &str) -> Edge {
        Edge {
            link: Link {
                from_id: "a1".into(),
                to_collection: "managed/b".into(),
                to_id: to.into(),
                edge_id: Some(edge_id.into()),
            },
            properties: Map::new(),
        }
    }

    fn relink(array: bool) -> Pending {
        let mut old = edge("b1", "e-old");
        old.properties.insert("role".into(), json!("owner"));
        Pending::relink("sandbox", "a", "k", array, &old)
    }

    #[test]
    fn a_relink_is_settled_by_a_link_to_the_target_under_any_edge_id() {
        assert_eq!(
            judge(&relink(true), &[edge("b1", "e-old")]),
            Recovery::Settled
        );
        assert_eq!(
            judge(&relink(true), &[edge("b1", "e-new")]),
            Recovery::Settled
        );
        // The discriminating case: the edge is gone and the record links
        // something else. Has-many: re-create alongside it. Has-one: that
        // would displace it.
        assert_eq!(judge(&relink(true), &[edge("b2", "e9")]), Recovery::Lost);
        assert_eq!(
            judge(&relink(false), &[edge("b2", "e9")]),
            Recovery::LostAndOccupied
        );
        assert_eq!(judge(&relink(false), &[]), Recovery::Lost);
    }

    #[test]
    fn a_probe_is_only_a_leftover_under_its_own_edge_id() {
        let mut probe = Pending::probe("sandbox", ("a", "k", true), "a1", "b", "b1");
        // Never read back: a link to the far record may predate the probe.
        assert_eq!(judge(&probe, &[edge("b1", "e-real")]), Recovery::Ambiguous);
        assert_eq!(judge(&probe, &[]), Recovery::Settled);
        probe.edge_id = Some("e-probe".into());
        assert_eq!(
            judge(&probe, &[edge("b1", "e-real")]),
            Recovery::Settled,
            "a link to the same record under another edge is not the probe's"
        );
        assert_eq!(
            judge(&probe, &[edge("b1", "e-probe")]),
            Recovery::Leftover {
                edge_id: "e-probe".into()
            }
        );
    }

    #[test]
    fn entries_round_trip_through_the_file_and_are_scoped_by_tenant() {
        let dir = std::env::temp_dir().join(format!("aic-links-{}", uuid::Uuid::new_v4()));
        let file = dir.join(JOURNAL_FILE);
        let first = relink(true);
        let mut other = relink(false);
        other.tenant = "other".into();
        record_at(&file, &first).unwrap();
        record_at(&file, &other).unwrap();
        assert_eq!(pending_at(&file, "sandbox").unwrap(), vec![first.clone()]);
        set_edge_at(&file, &first.id, "e-x").unwrap();
        assert_eq!(
            pending_at(&file, "sandbox").unwrap()[0].edge_id.as_deref(),
            Some("e-x")
        );
        clear_at(&file, &first.id).unwrap();
        assert!(pending_at(&file, "sandbox").unwrap().is_empty());
        assert_eq!(pending_at(&file, "other").unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
