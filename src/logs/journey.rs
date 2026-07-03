//! Journey rollup for `aic logs compact`.
//!
//! This module groups `am-authentication` payloads into journey executions,
//! interns the distinct journey/node/outcome dimensions, and builds the
//! compacted attempts written to DuckDB.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::Result;
use crate::logs::db::{self, JourneyAttempt};

/// Counts produced by an `aic logs compact` run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CompactReport {
    pub attempts_upserted: usize,
    pub journeys: usize,
    /// Count of DISTINCT `(journey, node_uuid)` nodes interned this run, not the
    /// number of node steps observed (each distinct node is interned once).
    pub nodes_interned: usize,
    pub events_pruned: usize,
}

/// Rolls up `am-authentication` events into the journey model, then prunes raw
/// `log_events` older than `retain_months`. Offline: reads the existing store
/// only, never touches the API or vault.
pub async fn compact_tenant(tenant: Option<String>, retain_months: i64) -> Result<CompactReport> {
    let tenant = crate::cli::tenant_for(tenant)?;
    let mut conn = db::open_store(&tenant)?;
    let now = chrono::Utc::now();

    let window_start = db::get_compact_state(&conn)?
        .map(|last| last - crate::logs::ops::SYNC_OVERLAP)
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap_or(now));

    let payloads = db::load_auth_payloads(&conn, window_start)?;
    let groups = group_attempts(&payloads);
    let mut nodes_interned = 0;
    let mut journeys = std::collections::HashSet::new();
    let attempts_upserted = {
        let tx = conn.transaction().map_err(db::DbError::from)?;
        let attempts = intern_groups(&tx, &groups, &mut nodes_interned)?;
        for attempt in &attempts {
            journeys.insert(attempt.journey_id);
        }
        db::upsert_attempts(&tx, &attempts)?;
        tx.commit().map_err(db::DbError::from)?;
        attempts.len()
    };

    db::set_compact_state(&conn, now)?;
    let cutoff = now
        .checked_sub_months(chrono::Months::new(retain_months as u32))
        .unwrap_or(now);
    let events_pruned = db::prune_events_before(&conn, cutoff)?;

    Ok(CompactReport {
        attempts_upserted,
        journeys: journeys.len(),
        nodes_interned,
        events_pruned,
    })
}

/// One node step observed within a journey execution, before interning.
struct RawStep {
    ts: DateTime<Utc>,
    node_uuid: String,
    node_type: Option<String>,
    display_name: Option<String>,
    outcome: String,
    tree_name: Option<String>,
}

#[derive(Default)]
struct RawAttempt {
    tracking_id: String,
    steps: Vec<RawStep>,
    tree_name: Option<String>,
    user_id: Option<String>,
    result: Option<String>,
    tree_ts: Option<DateTime<Utc>>,
}

impl RawAttempt {
    fn journey_name(&self) -> &str {
        self.tree_name
            .as_deref()
            .or_else(|| self.steps.first().and_then(|s| s.tree_name.as_deref()))
            .unwrap_or_default()
    }
    fn build(&self, journey_id: i32, nodes: &NodeMap, outcomes: &OutcomeMap) -> JourneyAttempt {
        let path: Vec<(i32, i32)> = self
            .steps
            .iter()
            .map(|step| {
                (
                    nodes[&(journey_id, step.node_uuid.as_str())],
                    outcomes[step.outcome.as_str()],
                )
            })
            .collect();
        let result = match self.result.as_deref() {
            None => "ABANDONED",
            Some("SUCCESSFUL") => "COMPLETED",
            Some(_) => "FAILED",
        }
        .to_string();
        let node_min_ts = self.steps.iter().map(|step| step.ts).min();
        let node_max_ts = self.steps.iter().map(|step| step.ts).max();
        let started_at = node_min_ts.or(self.tree_ts).unwrap_or_else(Utc::now);
        let ended_at = self.tree_ts.or(node_max_ts).unwrap_or(started_at);
        JourneyAttempt {
            tracking_id: self.tracking_id.clone(),
            journey_id,
            user_id: self.user_id.clone(),
            result,
            furthest_node_id: path.last().map(|(node_id, _)| *node_id),
            node_count: path.len() as i32,
            started_at,
            ended_at,
            path,
        }
    }
}

type NodeMap<'a> = BTreeMap<(i32, &'a str), i32>;
type OutcomeMap<'a> = BTreeMap<&'a str, i32>;
type NodeCols<'a> = BTreeMap<(i32, &'a str), (Option<&'a str>, Option<&'a str>)>;

fn intern_groups(
    conn: &db::Connection,
    groups: &BTreeMap<String, RawAttempt>,
    nodes_interned: &mut usize,
) -> Result<Vec<JourneyAttempt>> {
    let mut journeys: BTreeMap<&str, i32> = BTreeMap::new();
    for group in groups.values() {
        journeys.entry(group.journey_name()).or_default();
    }
    for (name, id) in journeys.iter_mut() {
        *id = db::intern_journey(conn, name)?;
    }
    let mut outcomes: OutcomeMap = BTreeMap::new();
    for group in groups.values() {
        for step in &group.steps {
            outcomes.entry(step.outcome.as_str()).or_default();
        }
    }
    for (name, id) in outcomes.iter_mut() {
        *id = db::intern_outcome(conn, name)?;
    }
    let mut node_cols: NodeCols = BTreeMap::new();
    for group in groups.values() {
        let journey_id = journeys[group.journey_name()];
        for step in &group.steps {
            node_cols.insert(
                (journey_id, step.node_uuid.as_str()),
                (step.node_type.as_deref(), step.display_name.as_deref()),
            );
        }
    }
    let mut nodes: NodeMap = BTreeMap::new();
    for (&(journey_id, node_uuid), &(node_type, display_name)) in &node_cols {
        let id = db::intern_node(conn, journey_id, node_uuid, node_type, display_name)?;
        nodes.insert((journey_id, node_uuid), id);
    }
    *nodes_interned += nodes.len();
    Ok(groups
        .values()
        .map(|group| group.build(journeys[group.journey_name()], &nodes, &outcomes))
        .collect())
}

fn group_attempts(payloads: &[Value]) -> BTreeMap<String, RawAttempt> {
    let mut groups: BTreeMap<String, RawAttempt> = BTreeMap::new();
    for payload in payloads {
        let event_name = payload.get("eventName").and_then(Value::as_str);
        let ts = payload
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|v| v.with_timezone(&Utc));
        let info = payload
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("info"));
        let tree_name = info
            .and_then(|i| i.get("treeName"))
            .and_then(Value::as_str)
            .map(str::to_string);
        match event_name {
            Some("AM-NODE-LOGIN-COMPLETED") => {
                let Some(track) = payload
                    .get("trackingIds")
                    .and_then(Value::as_array)
                    .and_then(|ids| ids.first())
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(info) = info else { continue };
                let Some(node_uuid) = info.get("nodeId").and_then(Value::as_str) else {
                    continue;
                };
                let entry = groups.entry(track.to_string()).or_default();
                entry.tracking_id = track.to_string();
                entry.steps.push(RawStep {
                    ts: ts.unwrap_or_else(Utc::now),
                    node_uuid: node_uuid.to_string(),
                    node_type: info
                        .get("nodeType")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    display_name: info
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    outcome: info
                        .get("nodeOutcome")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    tree_name,
                });
            }
            Some("AM-TREE-LOGIN-COMPLETED") => {
                let Some(track) = payload
                    .get("trackingIds")
                    .and_then(Value::as_array)
                    .and_then(|ids| ids.first())
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let entry = groups.entry(track.to_string()).or_default();
                entry.tracking_id = track.to_string();
                entry.tree_name = tree_name;
                entry.result = payload
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                entry.user_id = payload
                    .get("principal")
                    .and_then(Value::as_array)
                    .and_then(|principals| principals.first())
                    .and_then(Value::as_str)
                    .map(str::to_string);
                entry.tree_ts = ts;
            }
            _ => {}
        }
    }
    for group in groups.values_mut() {
        group.steps.sort_by_key(|step| step.ts);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory_store() -> db::Connection {
        let conn = db::Connection::open_in_memory().unwrap();
        db::init(&conn).unwrap();
        conn
    }

    fn node_event(track: &str, node_id: &str, outcome: &str, ts: &str) -> Value {
        json!({
            "eventName": "AM-NODE-LOGIN-COMPLETED",
            "timestamp": ts,
            "trackingIds": [track],
            "entries": [{ "info": {
                "treeName": "Test-Login",
                "nodeId": node_id,
                "displayName": format!("Node-{node_id}"),
                "nodeType": "ScriptedDecisionNode",
                "nodeOutcome": outcome,
            }}]
        })
    }

    fn tree_event(track: &str, result: &str, ts: &str) -> Value {
        json!({
            "eventName": "AM-TREE-LOGIN-COMPLETED",
            "timestamp": ts,
            "_id": format!("tree-{track}"),
            "trackingIds": [track],
            "result": result,
            "principal": ["alice"],
            "userId": "id=alice,ou=user,o=alpha",
            "entries": [{ "info": { "treeName": "Test-Login" } }]
        })
    }

    fn intern_all(conn: &db::Connection, payloads: &[Value]) -> Vec<JourneyAttempt> {
        let mut nodes = 0;
        intern_groups(conn, &group_attempts(payloads), &mut nodes).unwrap()
    }

    fn table_count(conn: &db::Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    #[test]
    fn two_nodes_and_a_tree_event_make_one_completed_attempt() {
        let conn = memory_store();
        // One execution: its nodes AND its tree event share ONE full trackingIds[0].
        let track = "a3c45e03-1244-4a1e-98c8-3cde967c4de1-19612069";
        let payloads = vec![
            node_event(track, "node-a", "ok", "2026-03-04T04:48:00.262Z"),
            node_event(track, "node-b", "true", "2026-03-04T04:48:02.000Z"),
            tree_event(track, "SUCCESSFUL", "2026-03-04T04:48:04.714Z"),
        ];

        let attempts = intern_all(&conn, &payloads);
        assert_eq!(attempts.len(), 1);
        let attempt = &attempts[0];
        assert_eq!(attempt.tracking_id, track);
        assert_eq!(attempt.result, "COMPLETED");
        assert_eq!(attempt.user_id.as_deref(), Some("alice"));
        assert_eq!(attempt.node_count, 2);
        assert_eq!(attempt.path.len(), 2);
        // Ordered by timestamp: node-a's outcome "ok" then node-b's "true".
        assert_eq!(attempt.furthest_node_id, Some(attempt.path[1].0));
        assert_ne!(attempt.path[0].0, attempt.path[1].0);
        assert_ne!(attempt.path[0].1, attempt.path[1].1);
    }

    #[test]
    fn a_node_group_with_no_tree_event_is_abandoned() {
        let conn = memory_store();
        let payloads = vec![node_event(
            "bbbbbbbb-1244-4a1e-98c8-3cde967c4de1-42",
            "node-a",
            "ok",
            "2026-03-04T04:48:00.262Z",
        )];

        let attempts = intern_all(&conn, &payloads);
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].result, "ABANDONED");
        assert_eq!(attempts[0].user_id, None);
        assert_eq!(attempts[0].node_count, 1);
    }

    #[test]
    fn different_full_tracking_ids_are_distinct_attempts() {
        // Guards P3-1: two node events sharing a base UUID but differing in the
        // full trackingIds[0] (…-1 vs …-2) are TWO executions, not one. The base
        // UUID is a cluster instance id, not an execution id, so it must NOT be
        // stripped to a join key.
        let conn = memory_store();
        let base = "a3c45e03-1244-4a1e-98c8-3cde967c4de1";
        let payloads = vec![
            node_event(
                &format!("{base}-1"),
                "node-a",
                "ok",
                "2026-03-04T04:48:00.000Z",
            ),
            node_event(
                &format!("{base}-2"),
                "node-a",
                "ok",
                "2026-03-04T04:48:01.000Z",
            ),
        ];
        assert_eq!(intern_all(&conn, &payloads).len(), 2);
    }

    #[test]
    fn module_login_events_are_ignored() {
        let conn = memory_store();
        let payloads = vec![
            json!({
                "eventName": "AM-LOGIN-MODULE-COMPLETED",
                "timestamp": "2026-03-04T04:48:00.262Z",
                "_id": "svc-account-1",
                "authIndex": "module_instance"
            }),
            json!({
                "eventName": "AM-LOGIN-COMPLETED",
                "timestamp": "2026-03-04T04:48:01.000Z",
                "_id": "svc-account-2"
            }),
        ];
        assert!(intern_all(&conn, &payloads).is_empty());
    }

    #[test]
    fn recurring_nodes_and_outcomes_are_interned_once() {
        let conn = memory_store();
        // Three attempts of one journey, each walking the SAME two nodes with the
        // SAME two outcomes. Each attempt has its OWN distinct full track string,
        // shared by that attempt's nodes AND tree event. Distinct dims: 1 journey,
        // 2 nodes, 2 outcomes.
        let tracks = [
            "aaaaaaaa-1244-4a1e-98c8-3cde967c4de1-1",
            "bbbbbbbb-1244-4a1e-98c8-3cde967c4de1-1",
            "cccccccc-1244-4a1e-98c8-3cde967c4de1-1",
        ];
        let mut payloads = Vec::new();
        for track in tracks {
            payloads.push(node_event(
                track,
                "node-a",
                "ok",
                "2026-03-04T04:48:00.000Z",
            ));
            payloads.push(node_event(
                track,
                "node-b",
                "true",
                "2026-03-04T04:48:01.000Z",
            ));
            payloads.push(tree_event(track, "SUCCESSFUL", "2026-03-04T04:48:02.000Z"));
        }

        let mut nodes_interned = 0;
        let attempts =
            intern_groups(&conn, &group_attempts(&payloads), &mut nodes_interned).unwrap();

        assert_eq!(attempts.len(), 3);
        assert_eq!(nodes_interned, 2);
        assert_eq!(table_count(&conn, "journey"), 1);
        assert_eq!(table_count(&conn, "node"), 2);
        assert_eq!(table_count(&conn, "outcome"), 2);

        // Attempts sharing a node resolve to the SAME node_id (and outcome_id).
        assert_eq!(attempts[0].path[0].0, attempts[1].path[0].0);
        assert_eq!(attempts[0].path[1].0, attempts[2].path[1].0);
        assert_eq!(attempts[0].path[0].1, attempts[2].path[0].1);
    }

    #[test]
    fn compacting_many_attempts_is_fast() {
        use std::time::Instant;

        // 120 attempts × 30 node steps = 3,600 node events across one journey.
        let attempt_count = 120;
        let steps_per_attempt = 30;
        let mut payloads = Vec::new();
        for attempt in 0..attempt_count {
            // One unique full track per attempt, shared by its steps AND tree event.
            let track = format!("{attempt:08x}-1244-4a1e-98c8-3cde967c4de1-0");
            for step in 0..steps_per_attempt {
                payloads.push(node_event(
                    &track,
                    &format!("node-{}", step % 15),
                    if step % 2 == 0 { "true" } else { "false" },
                    "2026-03-04T04:48:00.000Z",
                ));
            }
            payloads.push(tree_event(&track, "SUCCESSFUL", "2026-03-04T04:48:05.000Z"));
        }
        assert!(payloads.len() >= 3_000);

        let mut conn = memory_store();
        let groups = group_attempts(&payloads);
        assert_eq!(groups.len(), attempt_count);

        let started = Instant::now();
        let mut nodes_interned = 0;
        let attempts = {
            let tx = conn.transaction().unwrap();
            let attempts = intern_groups(&tx, &groups, &mut nodes_interned).unwrap();
            db::upsert_attempts(&tx, &attempts).unwrap();
            tx.commit().unwrap();
            attempts
        };
        let elapsed = started.elapsed();

        assert_eq!(attempts.len(), attempt_count);
        assert_eq!(nodes_interned, 15);
        assert!(elapsed.as_secs() < 5, "compact took {elapsed:?}");
    }
}
