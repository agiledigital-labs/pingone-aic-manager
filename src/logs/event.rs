//! Event identity shared by `aic logs tail` (in-memory) and `aic logs sync`
//! (DuckDB `ON CONFLICT (id)`). One definition so the two paths cannot drift.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Stable id for one log event.
///
/// Prefers a non-empty payload `_id` when the API supplies one; otherwise a
/// SHA-256 of `source|timestamp|payload_json`. Callers must pass the same
/// `payload_json` they persist or compare, so a pretty-printed payload cannot
/// mint a different id than the compact form.
pub(crate) fn event_id(
    source: Option<&str>,
    timestamp: &str,
    payload: &Value,
    payload_json: &str,
) -> String {
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

/// Identity of a logs-API event object (`timestamp`, `source`, `payload`).
pub(crate) fn event_id_of(event: &Value) -> String {
    let timestamp = event.get("timestamp").and_then(Value::as_str).unwrap_or("");
    let source = event.get("source").and_then(Value::as_str);
    let payload = event.get("payload").unwrap_or(&Value::Null);
    let payload_json = serde_json::to_string(payload).unwrap_or_else(|_| "null".to_string());
    event_id(source, timestamp, payload, &payload_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefers_a_non_empty_payload_id() {
        let payload = json!({"_id": "event-1", "message": "x"});
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            event_id(Some("am-access"), "2026-09-10T12:00:00Z", &payload, &json),
            "event-1"
        );
    }

    #[test]
    fn empty_or_missing_payload_id_is_a_deterministic_hash() {
        let payload = json!({"message": "no id"});
        let json = serde_json::to_string(&payload).unwrap();
        let first = event_id(Some("idm-core"), "2026-09-10T12:00:00Z", &payload, &json);
        let again = event_id(Some("idm-core"), "2026-09-10T12:00:00Z", &payload, &json);
        assert_eq!(first, again);
        assert_eq!(first.len(), 64);
        assert_ne!(
            first,
            event_id(Some("am-access"), "2026-09-10T12:00:00Z", &payload, &json)
        );
        assert_eq!(
            event_id_of(&json!({
                "timestamp": "2026-09-10T12:00:00Z",
                "source": "idm-core",
                "payload": payload,
            })),
            first
        );
    }

    #[test]
    fn empty_payload_id_does_not_count_as_an_identity() {
        let payload = json!({"_id": "", "message": "x"});
        let json = serde_json::to_string(&payload).unwrap();
        let hashed = event_id(Some("am-access"), "2026-09-10T12:00:00Z", &payload, &json);
        assert_ne!(hashed, "");
        assert_eq!(hashed.len(), 64);
    }
}
