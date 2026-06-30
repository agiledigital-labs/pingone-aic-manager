//! DuckDB storage for locally synced AIC log events.

use std::path::Path;

use chrono::{DateTime, Timelike, Utc};
use duckdb::{Connection, OptionalExt, params};
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
}
