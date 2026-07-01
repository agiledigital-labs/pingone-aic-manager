//! DuckDB storage for locally synced AIC log events.

use std::path::Path;

use chrono::{DateTime, Timelike, Utc};
use duckdb::types::ToSql;
use duckdb::{Connection, OptionalExt, params, params_from_iter};
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
}
