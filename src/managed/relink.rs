//! Relationship activation and re-linking: what `relationship set` waits for
//! after it declares a new end, and how `relationship relink` turns links
//! written while a pair had no reverse into links both ends list.
//!
//! Measured 2026-10-31 on a generic pair (`docs/api/10-managed-objects.md`,
//! "Adding a reverse to a relationship that has links"):
//!
//! - A newly declared end is not live when `PUT config/managed` returns. A
//!   write through it in that window answered 200 and was lost.
//!   `GET managed/<obj>/<id>/<key>?_queryFilter=true` answered 404 until the
//!   end was live and 200 after; writes made after the flip stuck.
//! - Links made before the reverse existed are never migrated: the far end
//!   does not list them, and no no-op write (replacing a link with itself,
//!   re-adding it, removing by value) changes that.
//! - Removing the edge and creating it again does: a has-many end by
//!   `DELETE <rel>/<edgeId>` then `POST <rel>?_action=create`, a has-one end
//!   by a JSON-patch `remove` then `replace`. The new edge is listed by both
//!   ends and keeps its custom `_refProperties`.
//!
//! Tenant-free: the decisions live here so tests drive the real ones, and
//! `cli.rs` does the reads and writes.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::managed::verify::{Link, LinkOutcome};

/// A relationship end `after` declares that `before` did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEnd {
    pub object: String,
    pub key: String,
}

fn relationship_ends(doc: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for object in crate::managed::api::objects(doc).into_iter().flatten() {
        let Some(name) = object.get("name").and_then(Value::as_str) else {
            continue;
        };
        for (key, property) in crate::managed::state::properties(object)
            .into_iter()
            .flatten()
        {
            if crate::managed::state::is_relationship_property(property) {
                out.push((name.to_string(), key.clone()));
            }
        }
    }
    out
}

/// The relationship ends `after` adds. Only these can be probed: an end that
/// already existed answers the probe before and after a reconfiguration.
pub fn new_ends(before: &Value, after: &Value) -> Vec<NewEnd> {
    let before = relationship_ends(before);
    relationship_ends(after)
        .into_iter()
        .filter(|end| !before.contains(end))
        .map(|(object, key)| NewEnd { object, key })
        .collect()
}

/// When `object.key` existed as a relationship with no reverse in `before`
/// and declares one in `after`, the `(target, reverse key)` it gained — the
/// change that leaves every existing link one-sided.
pub fn reverse_added(
    before: &Value,
    after: &Value,
    object: &str,
    key: &str,
) -> Option<(String, String)> {
    let property = |doc: &Value| -> Option<Value> {
        crate::managed::api::object_named(doc, object)
            .ok()
            .and_then(crate::managed::state::properties)
            .and_then(|properties| properties.get(key))
            .cloned()
    };
    let old = property(before)?;
    if !crate::managed::state::is_relationship_property(&old)
        || crate::managed::ops::declared_reverse_key(&old).is_some()
    {
        return None;
    }
    let new = property(after)?;
    let reverse = crate::managed::ops::declared_reverse_key(&new)?.to_string();
    let target = crate::managed::ops::relationship_targets(&new)
        .first()?
        .strip_prefix("managed/")?
        .to_string();
    Some((target, reverse))
}

/// What one activation probe said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// 404: the end is not live yet.
    Pending,
    /// The collection answered. 200 was measured on a has-many end; a
    /// has-one end's collection read is 501 once live (measured on existing
    /// ends), so 501 counts too.
    Live,
}

/// Judges one probe; any other failure is not an answer about activation.
pub fn activation(result: &crate::Result<Value>) -> Result<Activation, String> {
    match result {
        Ok(_) | Err(crate::Error::Api { status: 501, .. }) => Ok(Activation::Live),
        Err(crate::Error::Api { status: 404, .. }) => Ok(Activation::Pending),
        Err(error) => Err(error.to_string()),
    }
}

/// One link read from the near end, with what relink needs to re-create it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edge {
    #[serde(flatten)]
    pub link: Link,
    /// `_refProperties` without the edge's own `_id` / `_rev`.
    pub properties: Map<String, Value>,
}

/// Pairs each link `links_of` decoded with the custom `_refProperties` of the
/// raw entry it came from.
pub fn edges_of(record: &Value, key: &str) -> Result<Vec<Edge>, String> {
    let links = crate::managed::verify::links_of(record, key)?;
    let entries: Vec<&Value> = match record.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item @ Value::Object(_)) => vec![item],
        _ => Vec::new(),
    };
    Ok(links
        .into_iter()
        .zip(entries)
        .map(|(link, entry)| Edge {
            link,
            properties: custom_properties(entry),
        })
        .collect())
}

fn custom_properties(entry: &Value) -> Map<String, Value> {
    entry
        .get("_refProperties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, _)| !matches!(name.as_str(), "_id" | "_rev"))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Why a link is left alone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Skipped {
    #[serde(flatten)]
    pub edge: Edge,
    pub outcome: LinkOutcome,
    pub reason: &'static str,
}

/// The links a relink would re-create, and the ones it will not touch.
#[derive(Debug, Default, Serialize)]
pub struct Plan {
    /// Links the far end already lists.
    pub resolved: usize,
    /// Links the far end does not list: these are re-created.
    pub relink: Vec<Edge>,
    pub skipped: Vec<Skipped>,
}

/// What the far end's reverse property held when a link was judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FarSlot {
    /// Has-many, or a has-one holding nothing or the link back.
    Free,
    /// A has-one holding a link to some other record. Re-creating this link
    /// does not displace that one — IDM refuses it with `400 … not available
    /// for assignment`, or on a generic has-one to has-one pair accepts it
    /// one-sided (both measured 2026-10-31) — but the refusal would come after
    /// the removal, leaving this link removed.
    Occupied,
}

impl FarSlot {
    /// `far_links` is the far end's reverse read; `None` when the record is
    /// gone.
    pub fn of(far_array: bool, outcome: &LinkOutcome, far_links: Option<&[Link]>) -> Self {
        let holds_other = far_links.is_some_and(|links| !links.is_empty());
        if !far_array && *outcome == LinkOutcome::Missing && holds_other {
            Self::Occupied
        } else {
            Self::Free
        }
    }
}

/// Sorts judged links. Only `Missing` is the one-sided state relink exists
/// for; an edge-id disagreement and a missing far record are other defects,
/// and re-creating either would write a link nothing measured is safe.
pub fn plan(judged: Vec<(Edge, LinkOutcome, FarSlot)>) -> Plan {
    let mut out = Plan::default();
    for (edge, outcome, slot) in judged {
        match outcome {
            LinkOutcome::Resolved => out.resolved += 1,
            LinkOutcome::Missing if slot == FarSlot::Occupied => out.skipped.push(Skipped {
                edge,
                outcome,
                reason: "the far end is a has-one that holds another link; IDM refuses or \
                         one-sides a re-creation into it, so this link is not removed",
            }),
            LinkOutcome::Missing if edge.link.edge_id.is_none() => out.skipped.push(Skipped {
                edge,
                outcome,
                reason: "the link has no edge id to remove",
            }),
            LinkOutcome::Missing => out.relink.push(edge),
            LinkOutcome::EdgeDiffers { .. } => out.skipped.push(Skipped {
                edge,
                outcome,
                reason: "the far end lists it under another edge; `relationship verify` explains",
            }),
            LinkOutcome::FarRecordMissing => out.skipped.push(Skipped {
                edge,
                outcome,
                reason: "the far record does not exist",
            }),
        }
    }
    out
}

/// Proof that a relink was consented to. The field is private and
/// [`authorize`] is the only minting site, so a `--dry-run` that holds none
/// cannot reach [`writes`].
pub struct RelinkPermit(());

/// `consented` is the operator's answer (or `--force`); `dry_run` wins.
pub fn authorize(dry_run: bool, consented: bool) -> Option<RelinkPermit> {
    (!dry_run && consented).then_some(RelinkPermit(()))
}

/// One request a relink sends.
#[derive(Debug, Clone, PartialEq)]
pub struct Write {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
    /// The record `_rev` a has-one `PATCH` is conditional on. Measured
    /// 2026-10-31: on a **mapped** has-one end a link change moves the
    /// record's `_rev` and a stale `If-Match` is a 412 that writes nothing; on
    /// a **generic** end a link change leaves `_rev` alone and a stale
    /// `If-Match` is a 200 that writes. So it closes the race on mapped ends
    /// only, and the callers recheck by reading on both.
    pub if_match: Option<String>,
}

/// A record's `_rev`, which every read returns whatever `_fields` asked for.
pub fn rev_of(record: &Value) -> Option<String> {
    record
        .get("_rev")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// A has-one write asks for its property back with `_fields`, so the
/// response names the edge it created ([`written_edge`]).
fn fields_query(key: &str) -> String {
    url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
}

/// The edge id a link-creating write's response names: the top-level `_id`
/// of a has-many `?_action=create`, and `<key>._refProperties._id` of a
/// has-one `PATCH` sent with `_fields=<key>` (both measured 2026-10-31; each
/// equals the id a following read lists). This, not a before/after
/// comparison, is what says which edge **this** write made: another writer's
/// link to the same record is equally new.
pub fn written_edge(response: &Value, key: &str, array: bool) -> Option<String> {
    let id = if array {
        response.get("_id")
    } else {
        response
            .get(key)
            .and_then(|link| link.get("_refProperties"))
            .and_then(|properties| properties.get("_id"))
    };
    id.and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn record_path(object: &str, id: &str) -> String {
    format!(
        "/openidm/managed/{}/{}",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(id)
    )
}

/// The two writes that re-create `edge` on `near_object.near_key`, in order.
/// The first removes the link, so a failure between them leaves it gone:
/// the caller must stop and say so. A has-one removal is conditional on
/// `rev`; the caller sets the re-creation's `if_match` from a read taken
/// after the removal.
pub fn writes(
    _permit: &RelinkPermit,
    near_object: &str,
    near_key: &str,
    near_array: bool,
    edge: &Edge,
    rev: Option<&str>,
) -> Result<[Write; 2], String> {
    let record = record_path(near_object, &edge.link.from_id);
    let reference = json!({
        "_ref": format!("{}/{}", edge.link.to_collection, edge.link.to_id),
        "_refProperties": Value::Object(edge.properties.clone()),
    });
    if near_array {
        let edge_id = edge
            .link
            .edge_id
            .as_deref()
            .ok_or_else(|| format!("{}.{near_key} link has no edge id", edge.link.from_id))?;
        let collection = format!("{record}/{}", crate::idmstore::api::path_segment(near_key));
        Ok([
            Write {
                method: "DELETE",
                path: format!(
                    "{collection}/{}",
                    crate::idmstore::api::path_segment(edge_id)
                ),
                body: None,
                if_match: None,
            },
            Write {
                method: "POST",
                path: format!("{collection}?_action=create"),
                body: Some(reference),
                if_match: None,
            },
        ])
    } else {
        // `replace` with null is a 400 ("not a valid JSON patch"), so the
        // link is cleared with `remove`.
        let pointer = format!("/{near_key}");
        Ok([
            Write {
                method: "PATCH",
                path: record.clone(),
                body: Some(json!([{ "operation": "remove", "field": pointer }])),
                if_match: rev.map(str::to_string),
            },
            Write {
                method: "PATCH",
                path: format!("{record}?_fields={}", fields_query(near_key)),
                body: Some(
                    json!([{ "operation": "replace", "field": pointer, "value": reference }]),
                ),
                if_match: None,
            },
        ])
    }
}

/// Whether the near record still holds `edge` exactly as planned —
/// rechecked immediately before its writes, since a plan can be minutes old
/// by the time a long batch reaches it.
pub fn still_planned(current: &[Edge], edge: &Edge) -> bool {
    current.iter().any(|candidate| candidate == edge)
}

/// Proof that a `verify --probe` was consented to; minted only by
/// [`authorize_probe`], required by [`probe_link`] and [`probe_unlink`].
pub struct ProbePermit(());

pub fn authorize_probe(consented: bool) -> Option<ProbePermit> {
    consented.then_some(ProbePermit(()))
}

/// A record of a page read with `_fields=_id,<key>` that a probe can link
/// through without disturbing a link: any record for a has-many end, one whose
/// `key` is empty for a has-one end (setting it would replace the link it
/// holds). `exclude` keeps a self-relationship from linking a record to itself.
pub fn probe_candidate(
    records: &[Value],
    key: &str,
    array: bool,
    exclude: Option<&str>,
) -> Option<String> {
    records
        .iter()
        .filter(|record| array || matches!(record.get(key), None | Some(Value::Null)))
        .filter_map(|record| record.get("_id").and_then(Value::as_str))
        .find(|id| Some(*id) != exclude)
        .map(str::to_string)
}

/// The write that adds the probe link from `near_id` to `far_id`.
pub fn probe_link(
    _permit: &ProbePermit,
    (near_object, near_key, near_array): (&str, &str, bool),
    near_id: &str,
    far_object: &str,
    far_id: &str,
    rev: Option<&str>,
) -> Write {
    let record = record_path(near_object, near_id);
    let reference = json!({ "_ref": format!("managed/{far_object}/{far_id}") });
    if near_array {
        Write {
            method: "POST",
            path: format!(
                "{record}/{}?_action=create",
                crate::idmstore::api::path_segment(near_key)
            ),
            body: Some(reference),
            if_match: None,
        }
    } else {
        Write {
            method: "PATCH",
            path: format!("{record}?_fields={}", fields_query(near_key)),
            body: Some(
                json!([{ "operation": "replace", "field": format!("/{near_key}"), "value": reference }]),
            ),
            if_match: rev.map(str::to_string),
        }
    }
}

/// The write that removes the probe link again.
pub fn probe_unlink(
    _permit: &ProbePermit,
    (near_object, near_key, near_array): (&str, &str, bool),
    near_id: &str,
    edge_id: &str,
    rev: Option<&str>,
) -> Write {
    let record = record_path(near_object, near_id);
    if near_array {
        Write {
            method: "DELETE",
            path: format!(
                "{record}/{}/{}",
                crate::idmstore::api::path_segment(near_key),
                crate::idmstore::api::path_segment(edge_id)
            ),
            body: None,
            if_match: None,
        }
    } else {
        Write {
            method: "PATCH",
            path: record,
            body: Some(json!([{ "operation": "remove", "field": format!("/{near_key}") }])),
            if_match: rev.map(str::to_string),
        }
    }
}

/// The probe's link among `after`: the edge `written` — the id its write's
/// response named — provided it links `far_id` in `far_collection` (on a
/// multi-target field two collections can hold the same id).
///
/// There is deliberately no fallback without that id. Whatever "new since the
/// probe's read" test replaced it would also accept another writer's link
/// that took the probe's place between the write and the re-read — and the
/// probe deletes what this returns.
pub fn new_link<'a>(
    after: &'a [Edge],
    far_collection: &str,
    far_id: &str,
    written: &str,
) -> Option<&'a Edge> {
    after.iter().find(|edge| {
        edge.link.to_collection == far_collection
            && edge.link.to_id == far_id
            && edge.link.edge_id.as_deref() == Some(written)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(reverse: bool) -> Value {
        let mut owner = json!({"type": "relationship",
            "resourceCollection": [{"path": "managed/b"}]});
        if reverse {
            owner["reverseRelationship"] = json!(true);
            owner["reversePropertyName"] = json!("owned");
        }
        let mut b = json!({"name": "b", "schema": {"properties": {"n": {"type": "string"}}}});
        if reverse {
            b["schema"]["properties"]["owned"] = json!({"type": "array", "items": {
                "type": "relationship", "reverseRelationship": true,
                "reversePropertyName": "owner",
                "resourceCollection": [{"path": "managed/a"}]}});
        }
        json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {"owner": owner}}}, b]})
    }

    #[test]
    fn adding_a_reverse_names_the_new_end_and_the_one_sided_change() {
        let (before, after) = (doc(false), doc(true));
        assert_eq!(
            new_ends(&before, &after),
            vec![NewEnd {
                object: "b".into(),
                key: "owned".into()
            }]
        );
        assert_eq!(
            reverse_added(&before, &after, "a", "owner"),
            Some(("b".into(), "owned".into()))
        );
        // Discriminating: a pair that already had its reverse gained nothing,
        // and a brand-new relationship has no links to leave one-sided.
        assert_eq!(reverse_added(&after, &after, "a", "owner"), None);
        assert!(new_ends(&after, &after).is_empty());
        let empty = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {}}},
            {"name": "b", "schema": {"properties": {}}}]});
        assert_eq!(reverse_added(&empty, &after, "a", "owner"), None);
        assert_eq!(new_ends(&empty, &after).len(), 2);
    }

    #[test]
    fn activation_reads_404_as_pending_and_501_as_live() {
        let api = |status| {
            Err(crate::Error::Api {
                status,
                body: String::new(),
            })
        };
        assert_eq!(activation(&Ok(json!({}))), Ok(Activation::Live));
        assert_eq!(activation(&api(501)), Ok(Activation::Live));
        assert_eq!(activation(&api(404)), Ok(Activation::Pending));
        assert!(activation(&api(500)).is_err());
        assert!(activation(&api(403)).is_err());
    }

    fn record() -> Value {
        json!({"_id": "a1", "owner": [
            {"_ref": "managed/b/b1", "_refResourceCollection": "managed/b",
             "_refResourceId": "b1",
             "_refProperties": {"_id": "e1", "_rev": "r", "note": "keep"}},
            {"_ref": "managed/b/b2", "_refResourceCollection": "managed/b",
             "_refResourceId": "b2", "_refProperties": {"_id": "e2", "_rev": "r"}}]})
    }

    #[test]
    fn edges_keep_custom_ref_properties_only() {
        let edges = edges_of(&record(), "owner").unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(
            edges[0].properties,
            json!({"note": "keep"}).as_object().unwrap().clone()
        );
        assert_eq!(edges[0].link.edge_id.as_deref(), Some("e1"));
        assert!(edges[1].properties.is_empty());
        let single = json!({"_id": "a1", "owner": record()["owner"][0].clone()});
        assert_eq!(edges_of(&single, "owner").unwrap()[0].link.to_id, "b1");
    }

    #[test]
    fn plan_relinks_only_one_sided_links() {
        let edges = edges_of(&record(), "owner").unwrap();
        let mut no_id = edges[1].clone();
        no_id.link.edge_id = None;
        let free = FarSlot::Free;
        let plan = plan(vec![
            (edges[0].clone(), LinkOutcome::Missing, free),
            (edges[1].clone(), LinkOutcome::Resolved, free),
            (edges[1].clone(), LinkOutcome::FarRecordMissing, free),
            (
                edges[1].clone(),
                LinkOutcome::EdgeDiffers {
                    far_edges: vec!["x".into()],
                },
                free,
            ),
            (no_id, LinkOutcome::Missing, free),
            (edges[1].clone(), LinkOutcome::Missing, FarSlot::Occupied),
        ]);
        assert_eq!(plan.resolved, 1);
        assert_eq!(plan.relink, vec![edges[0].clone()]);
        assert_eq!(plan.skipped.len(), 4);
    }

    /// A has-one far end holding a link to someone else is occupied; the same
    /// read from a has-many end, or holding nothing, is not.
    #[test]
    fn a_far_has_one_holding_another_link_is_occupied() {
        let other = [Link {
            from_id: "b1".into(),
            to_collection: "managed/a".into(),
            to_id: "a-other".into(),
            edge_id: Some("e7".into()),
        }];
        let missing = LinkOutcome::Missing;
        assert_eq!(
            FarSlot::of(false, &missing, Some(&other)),
            FarSlot::Occupied
        );
        assert_eq!(FarSlot::of(true, &missing, Some(&other)), FarSlot::Free);
        assert_eq!(FarSlot::of(false, &missing, Some(&[])), FarSlot::Free);
        assert_eq!(FarSlot::of(false, &missing, None), FarSlot::Free);
    }

    #[test]
    fn dry_run_mints_no_permit() {
        assert!(authorize(true, true).is_none());
        assert!(authorize(false, false).is_none());
        assert!(authorize(false, true).is_some());
    }

    #[test]
    fn has_many_deletes_the_edge_then_creates_it_with_its_properties() {
        let permit = authorize(false, true).unwrap();
        let edge = &edges_of(&record(), "owner").unwrap()[0];
        let [first, second] = writes(&permit, "a", "owner", true, edge, Some("r1")).unwrap();
        assert_eq!(first.method, "DELETE");
        assert_eq!(first.path, "/openidm/managed/a/a1/owner/e1");
        assert_eq!(
            (first.if_match.as_deref(), second.if_match.as_deref()),
            (None, None)
        );
        assert_eq!(second.method, "POST");
        assert_eq!(second.path, "/openidm/managed/a/a1/owner?_action=create");
        assert_eq!(
            second.body,
            Some(json!({"_ref": "managed/b/b1", "_refProperties": {"note": "keep"}}))
        );
    }

    #[test]
    fn has_one_removes_then_replaces() {
        let permit = authorize(false, true).unwrap();
        let edge = &edges_of(&record(), "owner").unwrap()[0];
        let [first, second] = writes(&permit, "a", "owner", false, edge, Some("r1")).unwrap();
        assert_eq!((first.method, second.method), ("PATCH", "PATCH"));
        // The removal is conditional on the read it was planned from; the
        // re-creation's condition comes from a read after the removal.
        assert_eq!(first.if_match.as_deref(), Some("r1"));
        assert_eq!(second.if_match, None);
        assert_eq!(first.path, "/openidm/managed/a/a1");
        assert_eq!(
            first.body,
            Some(json!([{"operation": "remove", "field": "/owner"}]))
        );
        assert_eq!(
            second.body.unwrap()[0]["value"],
            json!({"_ref": "managed/b/b1", "_refProperties": {"note": "keep"}})
        );
    }

    #[test]
    fn a_changed_link_is_no_longer_planned() {
        let edges = edges_of(&record(), "owner").unwrap();
        assert!(still_planned(&edges, &edges[0]));
        let mut edited = edges.clone();
        edited[0].properties.insert("note".into(), json!("changed"));
        assert!(!still_planned(&edited, &edges[0]));
        assert!(!still_planned(&edges[1..], &edges[0]));
    }

    #[test]
    fn probe_candidates_never_displace_a_has_one_link() {
        let records = vec![
            json!({"_id": "x1", "one": {"_ref": "managed/b/b1"}}),
            json!({"_id": "x2", "one": null}),
            json!({"_id": "x3"}),
        ];
        assert_eq!(
            probe_candidate(&records, "one", false, None).as_deref(),
            Some("x2")
        );
        assert_eq!(
            probe_candidate(&records, "one", false, Some("x2")).as_deref(),
            Some("x3")
        );
        assert_eq!(
            probe_candidate(&records, "one", true, None).as_deref(),
            Some("x1")
        );
        assert_eq!(probe_candidate(&records[..1], "one", false, None), None);
    }

    #[test]
    fn probe_writes_add_then_remove_by_shape() {
        let permit = authorize_probe(true).unwrap();
        assert!(authorize_probe(false).is_none());
        let many = probe_link(&permit, ("a", "owner", true), "a1", "b", "b9", Some("r1"));
        assert_eq!(
            (many.method, many.path.as_str()),
            ("POST", "/openidm/managed/a/a1/owner?_action=create")
        );
        assert_eq!(many.body, Some(json!({"_ref": "managed/b/b9"})));
        assert_eq!(many.if_match, None);
        let one = probe_link(&permit, ("a", "owner", false), "a1", "b", "b9", Some("r1"));
        assert_eq!(
            (one.method, one.path.as_str()),
            ("PATCH", "/openidm/managed/a/a1?_fields=owner")
        );
        assert_eq!(one.if_match.as_deref(), Some("r1"));
        assert_eq!(one.body.unwrap()[0]["operation"], "replace");
        let gone = probe_unlink(&permit, ("a", "owner", true), "a1", "e9", Some("r2"));
        assert_eq!(
            (gone.method, gone.path.as_str()),
            ("DELETE", "/openidm/managed/a/a1/owner/e9")
        );
        assert_eq!(gone.if_match, None);
        let cleared = probe_unlink(&permit, ("a", "owner", false), "a1", "e9", Some("r2"));
        assert_eq!(cleared.if_match.as_deref(), Some("r2"));
        assert_eq!(
            cleared.body,
            Some(json!([{"operation": "remove", "field": "/owner"}]))
        );
    }

    #[test]
    fn the_probe_link_is_only_the_edge_its_write_named() {
        let before = edges_of(&record(), "owner").unwrap();
        let target = before[0].link.to_collection.clone();
        let id = before[0].link.to_id.clone();
        let mut after = before.clone();
        // Listed first: an edge with the written id to the same record id in
        // another collection, as on a multi-target field.
        let mut elsewhere = before[0].clone();
        elsewhere.link.to_collection = "managed/other".into();
        elsewhere.link.edge_id = Some("e-new".into());
        after.insert(0, elsewhere);
        // Another writer's link to the same record, new since the read.
        let mut theirs = before[0].clone();
        theirs.link.edge_id = Some("e-theirs".into());
        after.push(theirs);
        assert!(new_link(&after, &target, &id, "e-new").is_none());
        let mut added = before[0].clone();
        added.link.edge_id = Some("e-new".into());
        after.push(added);
        let found = new_link(&after, &target, &id, "e-new").unwrap();
        assert_eq!(
            (
                found.link.to_collection.as_str(),
                found.link.edge_id.as_deref()
            ),
            (target.as_str(), Some("e-new"))
        );
    }

    #[test]
    fn the_written_edge_is_read_from_the_write_response() {
        // Shapes as measured 2026-10-31.
        let many = json!({"_id": "e1", "_ref": "managed/b/b1",
            "_refProperties": {"_id": "e1"}});
        assert_eq!(written_edge(&many, "owner", true).as_deref(), Some("e1"));
        // Each slot alone, so reading the wrong one fails.
        assert_eq!(
            written_edge(&json!({"_id": "e1"}), "owner", true).as_deref(),
            Some("e1")
        );
        assert_eq!(
            written_edge(&json!({"_refProperties": {"_id": "e1"}}), "owner", true),
            None
        );
        let one = json!({"_id": "a1", "_rev": "r",
            "owner": {"_ref": "managed/b/b1", "_refProperties": {"_id": "e2"}}});
        assert_eq!(written_edge(&one, "owner", false).as_deref(), Some("e2"));
        // The record's own `_id` is not the edge's.
        assert_eq!(written_edge(&json!({"_id": "a1"}), "owner", false), None);
        assert_eq!(written_edge(&json!({}), "owner", true), None);
    }
}
