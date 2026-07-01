//! DuckDB storage for locally synced AIC log events.

use std::path::Path;

use chrono::{DateTime, Timelike, Utc};
pub use duckdb::Connection;
use duckdb::types::ToSql;
use duckdb::{OptionalExt, params, params_from_iter};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid log event: {0}")]
    InvalidEvent(String),
}

impl From<DbError> for crate::Error {
    fn from(error: DbError) -> Self {
        crate::Error::Config(format!("log store error: {error}"))
    }
}

pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    init(&conn)?;
    Ok(conn)
}

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "LOAD json;
         SET preserve_insertion_order = false;
         SET memory_limit = '2GB';
         SET threads = 4;
         CREATE TABLE IF NOT EXISTS log_events (
             id TEXT PRIMARY KEY,
             ts TIMESTAMP,
             source TEXT,
             transaction_id TEXT,
             event_name TEXT,
             level TEXT,
             topic TEXT,
             user_id TEXT,
             component TEXT,
             payload JSON
         );
         CREATE INDEX IF NOT EXISTS log_events_ts_idx ON log_events (ts);
         CREATE INDEX IF NOT EXISTS log_events_transaction_id_idx
             ON log_events (transaction_id);
         CREATE INDEX IF NOT EXISTS log_events_event_name_idx
             ON log_events (event_name);
         CREATE INDEX IF NOT EXISTS log_events_user_id_idx
             ON log_events (user_id);
         CREATE TABLE IF NOT EXISTS sync_state (
             source TEXT PRIMARY KEY,
             last_end_time TIMESTAMP,
             updated_at TIMESTAMP
         );
         CREATE SEQUENCE IF NOT EXISTS journey_id_seq START 1;
         CREATE SEQUENCE IF NOT EXISTS node_id_seq START 1;
         CREATE SEQUENCE IF NOT EXISTS outcome_id_seq START 1;
         CREATE TABLE IF NOT EXISTS journey (
             id INTEGER PRIMARY KEY DEFAULT nextval('journey_id_seq'),
             name TEXT UNIQUE
         );
         CREATE TABLE IF NOT EXISTS node (
             id INTEGER PRIMARY KEY DEFAULT nextval('node_id_seq'),
             journey_id INTEGER,
             node_uuid TEXT,
             node_type TEXT,
             display_name TEXT,
             UNIQUE (journey_id, node_uuid)
         );
         CREATE TABLE IF NOT EXISTS outcome (
             id INTEGER PRIMARY KEY DEFAULT nextval('outcome_id_seq'),
             name TEXT UNIQUE
         );
         CREATE TABLE IF NOT EXISTS journey_attempt (
             tracking_id      TEXT PRIMARY KEY,
             journey_id       INTEGER,
             user_id          TEXT,
             result           TEXT,
             furthest_node_id INTEGER,
             node_count       INTEGER,
             started_at       TIMESTAMP,
             ended_at         TIMESTAMP,
             path             STRUCT(node_id INTEGER, outcome_id INTEGER)[]
         );
         CREATE TABLE IF NOT EXISTS compact_state (
             id INTEGER PRIMARY KEY,
             last_compacted TIMESTAMP
         );",
    )?;
    Ok(())
}

pub fn insert_events(conn: &mut Connection, events: &[Value]) -> Result<usize> {
    let tx = conn.transaction()?;

    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS staging_events (
             id VARCHAR,
             ts TIMESTAMP,
             source VARCHAR,
             transaction_id VARCHAR,
             event_name VARCHAR,
             level VARCHAR,
             topic VARCHAR,
             user_id VARCHAR,
             component VARCHAR,
             payload VARCHAR
         );
         DELETE FROM staging_events;",
    )?;

    {
        let mut appender = tx.appender("staging_events")?;
        for event in events {
            let timestamp = event
                .get("timestamp")
                .and_then(Value::as_str)
                .ok_or_else(|| DbError::InvalidEvent("missing string timestamp".into()))?;
            let ts = parse_timestamp(timestamp)?;
            let source = event.get("source").and_then(Value::as_str);
            let payload = event.get("payload").unwrap_or(&Value::Null);
            let payload_json = serde_json::to_string(payload)?;
            let id = event_id(source, timestamp, payload, &payload_json);

            appender.append_row(params![
                id,
                ts,
                source,
                payload_text(payload, "transactionId"),
                payload_text(payload, "eventName"),
                payload_text(payload, "level"),
                payload_text(payload, "topic"),
                payload_text(payload, "userId"),
                payload_text(payload, "component"),
                payload_json,
            ])?;
        }
        appender.flush()?;
    }

    let inserted = tx.execute(
        "INSERT INTO log_events (
             id, ts, source, transaction_id, event_name, level, topic,
             user_id, component, payload
         )
         SELECT id, ts, source, transaction_id, event_name, level, topic,
                user_id, component, CAST(payload AS JSON)
         FROM (
             SELECT id, ts, source, transaction_id, event_name, level, topic,
                    user_id, component, payload,
                    row_number() OVER (PARTITION BY id ORDER BY ts) AS rn
             FROM staging_events
         ) AS deduped
         WHERE rn = 1
         ON CONFLICT (id) DO NOTHING",
        [],
    )?;

    tx.commit()?;
    Ok(inserted)
}

pub fn get_sync_state(conn: &Connection, source: &str) -> Result<Option<DateTime<Utc>>> {
    conn.query_row(
        "SELECT last_end_time FROM sync_state WHERE source = ?",
        [source],
        |row| row.get(0),
    )
    .optional()
    .map_err(DbError::from)
}

pub fn set_sync_state(conn: &Connection, source: &str, last_end_time: DateTime<Utc>) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (source, last_end_time, updated_at)
         VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT (source) DO UPDATE SET
             last_end_time = excluded.last_end_time,
             updated_at = excluded.updated_at",
        params![source, last_end_time],
    )?;
    Ok(())
}

pub fn count_events(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM log_events", [], |row| row.get(0))
        .map_err(DbError::from)
}

/// One rolled-up journey execution, keyed by its journey tracking UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyAttempt {
    pub tracking_id: String,
    pub journey_id: i32,
    pub user_id: Option<String>,
    pub result: String,
    pub furthest_node_id: Option<i32>,
    pub node_count: i32,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    /// Ordered `(node_id, outcome_id)` steps.
    pub path: Vec<(i32, i32)>,
}

/// Gets or creates the `journey` row for `name`, returning its surrogate id.
pub fn intern_journey(conn: &Connection, name: &str) -> Result<i32> {
    conn.query_row(
        "INSERT INTO journey (name) VALUES (?)
         ON CONFLICT (name) DO UPDATE SET name = excluded.name
         RETURNING id",
        params![name],
        |row| row.get(0),
    )
    .map_err(DbError::from)
}

/// Gets or creates the `node` row, refreshing its descriptive columns on repeat
/// sight, and returns its surrogate id.
pub fn intern_node(
    conn: &Connection,
    journey_id: i32,
    node_uuid: &str,
    node_type: Option<&str>,
    display_name: Option<&str>,
) -> Result<i32> {
    conn.query_row(
        "INSERT INTO node (journey_id, node_uuid, node_type, display_name)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (journey_id, node_uuid) DO UPDATE SET
             node_type = excluded.node_type,
             display_name = excluded.display_name
         RETURNING id",
        params![journey_id, node_uuid, node_type, display_name],
        |row| row.get(0),
    )
    .map_err(DbError::from)
}

/// Gets or creates the `outcome` row for `name`, returning its surrogate id.
pub fn intern_outcome(conn: &Connection, name: &str) -> Result<i32> {
    conn.query_row(
        "INSERT INTO outcome (name) VALUES (?)
         ON CONFLICT (name) DO UPDATE SET name = excluded.name
         RETURNING id",
        params![name],
        |row| row.get(0),
    )
    .map_err(DbError::from)
}

/// Inserts or refreshes a journey attempt; idempotent across overlapping compact
/// windows via `ON CONFLICT (tracking_id)`.
pub fn upsert_attempt(conn: &Connection, attempt: &JourneyAttempt) -> Result<()> {
    let path = path_literal(&attempt.path);
    let sql = format!(
        "INSERT INTO journey_attempt (
             tracking_id, journey_id, user_id, result, furthest_node_id,
             node_count, started_at, ended_at, path
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, {path})
         ON CONFLICT (tracking_id) DO UPDATE SET
             journey_id = excluded.journey_id,
             user_id = excluded.user_id,
             result = excluded.result,
             furthest_node_id = excluded.furthest_node_id,
             node_count = excluded.node_count,
             started_at = excluded.started_at,
             ended_at = excluded.ended_at,
             path = excluded.path"
    );
    conn.execute(
        &sql,
        params![
            attempt.tracking_id,
            attempt.journey_id,
            attempt.user_id,
            attempt.result,
            attempt.furthest_node_id,
            attempt.node_count,
            attempt.started_at,
            attempt.ended_at,
        ],
    )?;
    Ok(())
}

/// Upserts many attempts in one set-based statement. Mirrors `insert_events`:
/// each attempt is appended to a staging temp table (with `path` carried as a
/// JSON string), then a single `INSERT … ON CONFLICT` folds them into
/// `journey_attempt`. This avoids the per-statement DuckDB planning cost that
/// makes a `upsert_attempt`-per-attempt loop crawl on real captures.
pub fn upsert_attempts(conn: &Connection, attempts: &[JourneyAttempt]) -> Result<()> {
    if attempts.is_empty() {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS staging_attempts (
             tracking_id      VARCHAR,
             journey_id       INTEGER,
             user_id          VARCHAR,
             result           VARCHAR,
             furthest_node_id INTEGER,
             node_count       INTEGER,
             started_at       TIMESTAMP,
             ended_at         TIMESTAMP,
             path             VARCHAR
         );
         DELETE FROM staging_attempts;",
    )?;

    {
        let mut appender = conn.appender("staging_attempts")?;
        for attempt in attempts {
            appender.append_row(params![
                attempt.tracking_id,
                attempt.journey_id,
                attempt.user_id,
                attempt.result,
                attempt.furthest_node_id,
                attempt.node_count,
                attempt.started_at,
                attempt.ended_at,
                path_json(&attempt.path),
            ])?;
        }
        appender.flush()?;
    }

    conn.execute(
        "INSERT INTO journey_attempt (
             tracking_id, journey_id, user_id, result, furthest_node_id,
             node_count, started_at, ended_at, path
         )
         SELECT tracking_id, journey_id, user_id, result, furthest_node_id,
                node_count, started_at, ended_at,
                CAST(CAST(path AS JSON) AS STRUCT(node_id INTEGER, outcome_id INTEGER)[])
         FROM (
             SELECT *, row_number() OVER (PARTITION BY tracking_id ORDER BY ended_at DESC) AS rn
             FROM staging_attempts
         ) AS deduped
         WHERE rn = 1
         ON CONFLICT (tracking_id) DO UPDATE SET
             journey_id = excluded.journey_id,
             user_id = excluded.user_id,
             result = excluded.result,
             furthest_node_id = excluded.furthest_node_id,
             node_count = excluded.node_count,
             started_at = excluded.started_at,
             ended_at = excluded.ended_at,
             path = excluded.path",
        [],
    )?;
    Ok(())
}

/// Serializes a path as a JSON array of `{node_id, outcome_id}` objects for
/// staging; the set-based upsert casts it back to the STRUCT-array column.
fn path_json(path: &[(i32, i32)]) -> String {
    let items: Vec<String> = path
        .iter()
        .map(|(node_id, outcome_id)| {
            format!("{{\"node_id\":{node_id},\"outcome_id\":{outcome_id}}}")
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Builds the `STRUCT(node_id, outcome_id)[]` list literal for a path. The ids
/// are DB-internal integers, so inlining them is injection-safe.
fn path_literal(path: &[(i32, i32)]) -> String {
    if path.is_empty() {
        return "CAST([] AS STRUCT(node_id INTEGER, outcome_id INTEGER)[])".to_string();
    }
    let items: Vec<String> = path
        .iter()
        .map(|(node_id, outcome_id)| {
            format!("{{'node_id': {node_id}, 'outcome_id': {outcome_id}}}")
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// Reads back an attempt's `path` list as ordered `(node_id, outcome_id)` pairs.
#[cfg(test)]
pub fn attempt_path(conn: &Connection, tracking_id: &str) -> Result<Vec<(i32, i32)>> {
    let json: String = conn.query_row(
        "SELECT to_json(path) FROM journey_attempt WHERE tracking_id = ?",
        params![tracking_id],
        |row| row.get(0),
    )?;
    let parsed: Vec<serde_json::Map<String, Value>> = serde_json::from_str(&json)?;
    let mut steps = Vec::with_capacity(parsed.len());
    for step in parsed {
        let node_id = step.get("node_id").and_then(Value::as_i64).unwrap_or(0) as i32;
        let outcome_id = step.get("outcome_id").and_then(Value::as_i64).unwrap_or(0) as i32;
        steps.push((node_id, outcome_id));
    }
    Ok(steps)
}

/// Loads the `am-authentication` payloads at or after `since`, ordered by time,
/// parsed as JSON — the input to the journey rollup.
pub fn load_auth_payloads(conn: &Connection, since: DateTime<Utc>) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM log_events
         WHERE source = 'am-authentication' AND ts >= ?
         ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![since], |row| row.get::<_, String>(0))?;
    let mut payloads = Vec::new();
    for row in rows {
        payloads.push(serde_json::from_str::<Value>(&row?)?);
    }
    Ok(payloads)
}

pub fn get_compact_state(conn: &Connection) -> Result<Option<DateTime<Utc>>> {
    conn.query_row(
        "SELECT last_compacted FROM compact_state WHERE id = 0",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(DbError::from)
}

pub fn set_compact_state(conn: &Connection, last_compacted: DateTime<Utc>) -> Result<()> {
    conn.execute(
        "INSERT INTO compact_state (id, last_compacted)
         VALUES (0, ?)
         ON CONFLICT (id) DO UPDATE SET last_compacted = excluded.last_compacted",
        params![last_compacted],
    )?;
    Ok(())
}

/// Deletes raw `log_events` older than `cutoff`, returning the deleted count.
pub fn prune_events_before(conn: &Connection, cutoff: DateTime<Utc>) -> Result<usize> {
    let deleted = conn.execute("DELETE FROM log_events WHERE ts < ?", params![cutoff])?;
    Ok(deleted)
}

/// Filters for an offline query against the local log store.
#[derive(Debug, Default, Clone)]
pub struct SearchParams {
    pub transaction_id: Option<String>,
    pub source: Option<String>,
    pub event_name: Option<String>,
    pub user_id: Option<String>,
    pub level: Option<String>,
    pub begin: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub contains: Option<String>,
    pub limit: usize,
}

/// Builds the shared `WHERE` clause and its bound parameters. Every filter is a
/// `?` placeholder — user input is never interpolated into the SQL text.
fn where_clause(params: &SearchParams) -> (String, Vec<Box<dyn ToSql>>) {
    let mut clauses: Vec<&str> = Vec::new();
    let mut binds: Vec<Box<dyn ToSql>> = Vec::new();

    let mut eq = |column: &'static str, value: &Option<String>| {
        if let Some(value) = value {
            clauses.push(column);
            binds.push(Box::new(value.clone()));
        }
    };
    eq("transaction_id = ?", &params.transaction_id);
    eq("source = ?", &params.source);
    eq("event_name = ?", &params.event_name);
    eq("user_id = ?", &params.user_id);
    eq("level = ?", &params.level);

    if let Some(begin) = params.begin {
        clauses.push("ts >= ?");
        binds.push(Box::new(begin));
    }
    if let Some(end) = params.end {
        clauses.push("ts < ?");
        binds.push(Box::new(end));
    }
    if let Some(contains) = &params.contains {
        clauses.push("CAST(payload AS VARCHAR) LIKE ?");
        binds.push(Box::new(format!("%{contains}%")));
    }

    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, binds)
}

/// Runs an offline, parameterized search and reconstructs each row into the
/// API-shaped event used by `aic logs tx/range/query`.
pub fn search(conn: &Connection, params: &SearchParams) -> Result<Vec<Value>> {
    let (where_sql, mut binds) = where_clause(params);
    let sql = format!("SELECT ts, source, payload FROM log_events{where_sql} ORDER BY ts LIMIT ?");
    binds.push(Box::new(params.limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(binds.iter()), |row| {
        let ts: DateTime<Utc> = row.get(0)?;
        let source: Option<String> = row.get(1)?;
        let payload: String = row.get(2)?;
        Ok((ts, source, payload))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (ts, source, payload) = row?;
        let payload: Value = serde_json::from_str(&payload)?;
        events.push(serde_json::json!({
            "timestamp": ts.to_rfc3339(),
            "source": source,
            "payload": payload,
        }));
    }
    Ok(events)
}

/// Counts the rows that `search` would return ignoring `limit`.
pub fn count_matching(conn: &Connection, params: &SearchParams) -> Result<i64> {
    let (where_sql, binds) = where_clause(params);
    let sql = format!("SELECT COUNT(*) FROM log_events{where_sql}");
    conn.query_row(&sql, params_from_iter(binds.iter()), |row| row.get(0))
        .map_err(DbError::from)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|error| DbError::InvalidEvent(format!("invalid timestamp {value:?}: {error}")))?
        .with_timezone(&Utc);
    let micros = parsed.timestamp_subsec_micros();
    parsed
        .with_nanosecond(micros * 1_000)
        .ok_or_else(|| DbError::InvalidEvent(format!("invalid timestamp {value:?}")))
}

fn event_id(source: Option<&str>, timestamp: &str, payload: &Value, payload_json: &str) -> String {
    if let Some(id) = payload
        .get("_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        return id.to_string();
    }

    let input = format!("{}|{timestamp}|{payload_json}", source.unwrap_or_default());
    let digest = Sha256::digest(input.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    hex
}

fn payload_text<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload.get(field).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn memory_store() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        init(&conn)?;
        Ok(conn)
    }

    fn events() -> Vec<Value> {
        vec![
            json!({
                "timestamp": "2026-06-24T12:34:56.123456789Z",
                "source": "idm-activity",
                "payload": {
                    "_id": "event-1",
                    "transactionId": "tx-1/0",
                    "eventName": "activity",
                    "level": "INFO",
                    "topic": "managed/user",
                    "userId": "user-1",
                    "component": "repo"
                }
            }),
            json!({
                "timestamp": "2026-06-24T12:35:00Z",
                "source": "am-authentication",
                "payload": {
                    "_id": "event-2",
                    "transactionId": "tx-2/0",
                    "eventName": "authentication"
                }
            }),
        ]
    }

    #[test]
    fn schema_initializes_and_inserts_events() -> Result<()> {
        let mut conn = memory_store()?;

        assert_eq!(insert_events(&mut conn, &events())?, 2);
        assert_eq!(count_events(&conn)?, 2);
        Ok(())
    }

    #[test]
    fn duplicate_ids_are_ignored() -> Result<()> {
        let mut conn = memory_store()?;
        let events = events();

        assert_eq!(insert_events(&mut conn, &events)?, 2);
        assert_eq!(insert_events(&mut conn, &events)?, 0);
        assert_eq!(count_events(&conn)?, 2);
        Ok(())
    }

    #[test]
    fn bulk_insert_dedupes_within_and_across_batches() -> Result<()> {
        let mut conn = memory_store()?;
        let events: Vec<Value> = (0..10_000)
            .map(|index| {
                let id = format!("bulk-event-{}", index % 9_750);
                json!({
                    "timestamp": "2026-06-24T12:34:56.123456Z",
                    "source": "idm-activity",
                    "payload": {
                        "_id": id,
                        "transactionId": format!("tx-{index}/0"),
                        "eventName": "activity",
                        "level": "INFO",
                        "topic": "managed/user",
                        "userId": format!("user-{index}"),
                        "component": "repo"
                    }
                })
            })
            .collect();

        let started = Instant::now();
        assert_eq!(insert_events(&mut conn, &events)?, 9_750);
        assert_eq!(count_events(&conn)?, 9_750);
        assert_eq!(insert_events(&mut conn, &events)?, 0);
        assert_eq!(count_events(&conn)?, 9_750);
        assert!(
            started.elapsed().as_secs() < 10,
            "bulk insert took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    #[test]
    fn extracted_columns_and_timestamp_are_populated() -> Result<()> {
        let mut conn = memory_store()?;
        insert_events(&mut conn, &events())?;

        let row: (DateTime<Utc>, String, String, String) = conn.query_row(
            "SELECT ts, source, transaction_id, event_name
             FROM log_events WHERE id = 'event-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            row,
            (
                Utc.with_ymd_and_hms(2026, 6, 24, 12, 34, 56)
                    .unwrap()
                    .with_nanosecond(123_456_000)
                    .unwrap(),
                "idm-activity".to_string(),
                "tx-1/0".to_string(),
                "activity".to_string(),
            )
        );
        Ok(())
    }

    #[test]
    fn missing_id_uses_a_deterministic_fallback() -> Result<()> {
        let mut conn = memory_store()?;
        let event = json!({
            "timestamp": "2026-06-24T12:36:00Z",
            "source": "idm-core",
            "payload": {"level": "WARN", "message": "no id"}
        });

        assert_eq!(insert_events(&mut conn, std::slice::from_ref(&event))?, 1);
        assert_eq!(insert_events(&mut conn, &[event])?, 0);
        assert_eq!(count_events(&conn)?, 1);
        Ok(())
    }

    #[test]
    fn raw_payload_is_stored_without_extracted_columns() -> Result<()> {
        let mut conn = memory_store()?;
        let event = json!({
            "timestamp": "2026-06-24T12:37:00Z",
            "source": "ctsstore",
            "payload": "plain text payload"
        });

        insert_events(&mut conn, &[event])?;
        let row: (String, Option<String>) = conn.query_row(
            "SELECT CAST(payload AS VARCHAR), transaction_id FROM log_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(row, ("\"plain text payload\"".into(), None));
        Ok(())
    }

    #[test]
    fn sync_state_round_trips() -> Result<()> {
        let conn = memory_store()?;
        let expected = Utc
            .with_ymd_and_hms(2026, 6, 24, 13, 0, 0)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();

        assert_eq!(get_sync_state(&conn, "idm-everything")?, None);
        set_sync_state(&conn, "idm-everything", expected)?;
        assert_eq!(get_sync_state(&conn, "idm-everything")?, Some(expected));
        Ok(())
    }

    fn seeded_store() -> Result<Connection> {
        let mut conn = memory_store()?;
        insert_events(&mut conn, &events())?;
        Ok(conn)
    }

    fn sources_of(events: &[Value]) -> Vec<&str> {
        events
            .iter()
            .map(|event| event["source"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn search_with_empty_params_returns_everything_ordered_by_ts() -> Result<()> {
        let conn = seeded_store()?;
        let params = SearchParams {
            limit: 100,
            ..Default::default()
        };
        let hits = search(&conn, &params)?;
        assert_eq!(sources_of(&hits), vec!["idm-activity", "am-authentication"]);
        Ok(())
    }

    #[test]
    fn search_reconstructs_the_api_shaped_event() -> Result<()> {
        let conn = seeded_store()?;
        let params = SearchParams {
            transaction_id: Some("tx-1/0".into()),
            limit: 100,
            ..Default::default()
        };
        let hits = search(&conn, &params)?;
        assert_eq!(hits.len(), 1);
        let event = &hits[0];
        assert_eq!(event["source"], "idm-activity");
        assert_eq!(event["timestamp"], "2026-06-24T12:34:56.123456+00:00");
        assert_eq!(event["payload"]["eventName"], "activity");
        Ok(())
    }

    #[test]
    fn search_filters_by_source_and_user_id() -> Result<()> {
        let conn = seeded_store()?;
        let by_source = search(
            &conn,
            &SearchParams {
                source: Some("am-authentication".into()),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&by_source), vec!["am-authentication"]);

        let by_user = search(
            &conn,
            &SearchParams {
                user_id: Some("user-1".into()),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&by_user), vec!["idm-activity"]);
        Ok(())
    }

    #[test]
    fn search_matches_a_payload_substring() -> Result<()> {
        let conn = seeded_store()?;
        let hits = search(
            &conn,
            &SearchParams {
                contains: Some("managed/user".into()),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&hits), vec!["idm-activity"]);

        let none = search(
            &conn,
            &SearchParams {
                contains: Some("no-such-text".into()),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert!(none.is_empty());
        Ok(())
    }

    #[test]
    fn search_window_includes_begin_and_excludes_end() -> Result<()> {
        let conn = seeded_store()?;
        let begin = Utc.with_ymd_and_hms(2026, 6, 24, 12, 35, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 6, 24, 12, 35, 0).unwrap();

        // begin is inclusive: the 12:35:00 event matches.
        let from_begin = search(
            &conn,
            &SearchParams {
                begin: Some(begin),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&from_begin), vec!["am-authentication"]);

        // end is exclusive: the 12:35:00 event is dropped.
        let before_end = search(
            &conn,
            &SearchParams {
                end: Some(end),
                limit: 100,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&before_end), vec!["idm-activity"]);
        Ok(())
    }

    #[test]
    fn search_limit_caps_the_row_count() -> Result<()> {
        let conn = seeded_store()?;
        let hits = search(
            &conn,
            &SearchParams {
                limit: 1,
                ..Default::default()
            },
        )?;
        assert_eq!(sources_of(&hits), vec!["idm-activity"]);
        Ok(())
    }

    #[test]
    fn count_matching_shares_the_where_clause_with_search() -> Result<()> {
        let conn = seeded_store()?;
        let params = SearchParams {
            limit: 1,
            ..Default::default()
        };
        // count ignores the limit; search honours it.
        assert_eq!(count_matching(&conn, &params)?, 2);
        assert_eq!(search(&conn, &params)?.len(), 1);

        let filtered = SearchParams {
            event_name: Some("authentication".into()),
            limit: 100,
            ..Default::default()
        };
        assert_eq!(count_matching(&conn, &filtered)?, 1);
        Ok(())
    }

    #[test]
    fn intern_is_get_or_create() -> Result<()> {
        let conn = memory_store()?;
        let a = intern_journey(&conn, "Test-Login")?;
        let b = intern_journey(&conn, "Test-Login")?;
        let c = intern_journey(&conn, "Other-Login")?;
        assert_eq!(a, b);
        assert_ne!(a, c);

        let n1 = intern_node(
            &conn,
            a,
            "node-uuid",
            Some("ScriptedDecisionNode"),
            Some("First"),
        )?;
        let n2 = intern_node(
            &conn,
            a,
            "node-uuid",
            Some("ScriptedDecisionNode"),
            Some("Renamed"),
        )?;
        assert_eq!(n1, n2);
        let display: String = conn.query_row(
            "SELECT display_name FROM node WHERE id = ?",
            params![n1],
            |row| row.get(0),
        )?;
        assert_eq!(display, "Renamed");

        assert_eq!(intern_outcome(&conn, "ok")?, intern_outcome(&conn, "ok")?);
        assert_ne!(
            intern_outcome(&conn, "ok")?,
            intern_outcome(&conn, "false")?
        );
        Ok(())
    }

    fn sample_attempt(path: Vec<(i32, i32)>) -> JourneyAttempt {
        let started = Utc.with_ymd_and_hms(2026, 3, 4, 4, 48, 0).unwrap();
        let ended = Utc.with_ymd_and_hms(2026, 3, 4, 4, 48, 4).unwrap();
        JourneyAttempt {
            tracking_id: "track-1".into(),
            journey_id: 1,
            user_id: Some("alice".into()),
            result: "COMPLETED".into(),
            furthest_node_id: path.last().map(|(node, _)| *node),
            node_count: path.len() as i32,
            started_at: started,
            ended_at: ended,
            path,
        }
    }

    #[test]
    fn upsert_attempt_round_trips_the_path_list() -> Result<()> {
        let conn = memory_store()?;
        upsert_attempt(&conn, &sample_attempt(vec![(10, 1), (11, 2)]))?;
        assert_eq!(attempt_path(&conn, "track-1")?, vec![(10, 1), (11, 2)]);

        // An empty path also round-trips.
        let mut empty = sample_attempt(vec![]);
        empty.tracking_id = "track-empty".into();
        empty.furthest_node_id = None;
        upsert_attempt(&conn, &empty)?;
        assert_eq!(attempt_path(&conn, "track-empty")?, vec![]);
        Ok(())
    }

    #[test]
    fn upsert_attempt_is_idempotent() -> Result<()> {
        let conn = memory_store()?;
        upsert_attempt(&conn, &sample_attempt(vec![(10, 1)]))?;
        upsert_attempt(&conn, &sample_attempt(vec![(10, 1), (11, 2)]))?;

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM journey_attempt", [], |row| row.get(0))?;
        assert_eq!(count, 1);
        assert_eq!(attempt_path(&conn, "track-1")?, vec![(10, 1), (11, 2)]);
        Ok(())
    }

    #[test]
    fn compact_state_round_trips() -> Result<()> {
        let conn = memory_store()?;
        let ts = Utc.with_ymd_and_hms(2026, 6, 24, 13, 0, 0).unwrap();
        assert_eq!(get_compact_state(&conn)?, None);
        set_compact_state(&conn, ts)?;
        assert_eq!(get_compact_state(&conn)?, Some(ts));
        Ok(())
    }

    #[test]
    fn prune_deletes_only_older_rows() -> Result<()> {
        let mut conn = memory_store()?;
        insert_events(&mut conn, &events())?;
        // events() are at 12:34:56 and 12:35:00 on 2026-06-24.
        let cutoff = Utc.with_ymd_and_hms(2026, 6, 24, 12, 35, 0).unwrap();
        assert_eq!(prune_events_before(&conn, cutoff)?, 1);
        assert_eq!(count_events(&conn)?, 1);
        Ok(())
    }
}
