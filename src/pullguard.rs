//! Shared protected-pull decision (`.ai/core.md` §5).
//!
//! Compare authored content, not `_rev`. A local file that matches the last
//! synced snapshot may be overwritten; a local edit that does not match is
//! protected until the caller consents. Parse failure is protection: bytes we
//! cannot normalise are never treated as "unchanged" or as a snapshot match.
//!
//! Callers supply the normaliser and the content-equality predicate. Policy
//! identity checks, journey export shaping and `_rev` stripping stay at the
//! call site so a fourth resource kind does not special-case this module.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullDecision {
    Install,
    Unchanged,
    Protected,
}

pub(crate) fn decide(
    local: Option<&[u8]>,
    remote: &Value,
    snapshot: Option<&[u8]>,
    normalize: impl Fn(&[u8]) -> Option<Value>,
    content_equal: impl Fn(&Value, &Value) -> bool,
) -> PullDecision {
    let Some(local_bytes) = local else {
        return PullDecision::Install;
    };
    let Some(local) = normalize(local_bytes) else {
        return PullDecision::Protected;
    };
    if content_equal(&local, remote) {
        return PullDecision::Unchanged;
    }
    match snapshot.and_then(normalize) {
        Some(snapshot) if content_equal(&local, &snapshot) => PullDecision::Install,
        _ => PullDecision::Protected,
    }
}

pub(crate) fn needs_consent(decision: PullDecision, operation_force: bool) -> bool {
    decision == PullDecision::Protected && !operation_force
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn json_object(bytes: &[u8]) -> Option<Value> {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        value.is_object().then_some(value)
    }

    fn equal(a: &Value, b: &Value) -> bool {
        a == b
    }

    fn assert_decision(
        name: &str,
        local: Option<&[u8]>,
        remote: &Value,
        snapshot: Option<&[u8]>,
        expected: PullDecision,
    ) {
        assert_eq!(
            decide(local, remote, snapshot, json_object, equal),
            expected,
            "{name}"
        );
    }

    #[test]
    fn protected_pull_matrix() {
        let remote = json!({"value": 2});
        let same = br#"{"value":2}"#;
        let old = br#"{"value":1}"#;
        let edited = br#"{"value":"edited"}"#;

        assert_decision(
            "missing local installs",
            None,
            &remote,
            None,
            PullDecision::Install,
        );
        assert_decision(
            "local matching remote is unchanged",
            Some(same),
            &remote,
            None,
            PullDecision::Unchanged,
        );
        assert_decision(
            "local matching snapshot installs remote",
            Some(old),
            &remote,
            Some(old),
            PullDecision::Install,
        );
        assert_decision(
            "local edit against snapshot is protected",
            Some(edited),
            &remote,
            Some(old),
            PullDecision::Protected,
        );
        assert_decision(
            "drift with no snapshot is protected",
            Some(old),
            &remote,
            None,
            PullDecision::Protected,
        );
        assert_decision(
            "unreadable snapshot cannot authorize overwrite",
            Some(old),
            &remote,
            Some(b"not json"),
            PullDecision::Protected,
        );
        assert_decision(
            "unreadable local is protected",
            Some(b"not json"),
            &remote,
            Some(old),
            PullDecision::Protected,
        );
    }

    #[test]
    fn consent_is_only_required_for_unforced_protected_pulls() {
        assert!(needs_consent(PullDecision::Protected, false));
        assert!(!needs_consent(PullDecision::Protected, true));
        assert!(!needs_consent(PullDecision::Install, false));
        assert!(!needs_consent(PullDecision::Unchanged, false));
    }
}
