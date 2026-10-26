//! Tenant-free half of `aic promotion`: decoding, classification and the
//! report summary. Shapes measured 2026-10-26 (`docs/api/23-promotion.md`).

use serde_json::Value;

use crate::Error;

/// Which report to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Which {
    Latest,
    Id(String),
    Provisional,
}

impl Which {
    pub fn path(&self) -> String {
        match self {
            Which::Latest => "/environment/promotion/report".into(),
            Which::Provisional => "/environment/promotion/report/provisional".into(),
            Which::Id(id) => format!(
                "/environment/promotion/report/{}",
                url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>()
            ),
        }
    }
}

/// One side's lock, as the upper environment reports both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvLock {
    pub state: String,
    pub proxy_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockState {
    pub result: String,
    pub description: Option<String>,
    /// Present only on the **upper** environment of a pair (measured): the
    /// lower one answers with `result` alone.
    pub lower: Option<EnvLock>,
    pub upper: Option<EnvLock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub status: String,
    pub message: Option<String>,
    pub global_lock: Option<String>,
    pub blocking_error: Option<bool>,
    pub timestamp: Option<String>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn env(value: &Value, key: &str) -> Result<Option<EnvLock>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(side) => Ok(Some(EnvLock {
            state: text(side, "state")
                .ok_or_else(|| format!("`{key}` has no string `state`: {side}"))?,
            proxy_state: text(side, "proxyState"),
        })),
    }
}

/// Decode `lock/state`; a body without a string `result` is an error, never
/// "unlocked".
pub fn lock_state(value: &Value) -> Result<LockState, String> {
    Ok(LockState {
        result: text(value, "result")
            .ok_or_else(|| format!("lock state has no string `result`: {value}"))?,
        description: text(value, "description"),
        lower: env(value, "lowerEnv")?,
        upper: env(value, "upperEnv")?,
    })
}

pub fn status(value: &Value) -> Result<Status, String> {
    Ok(Status {
        status: text(value, "status")
            .ok_or_else(|| format!("promotion status has no string `status`: {value}"))?,
        message: text(value, "message"),
        global_lock: text(value, "globalLock"),
        blocking_error: value.get("blockingError").and_then(Value::as_bool),
        timestamp: text(value, "timeStamp"),
    })
}

/// Where the two endpoints that each describe the lock disagree. Measured on
/// an upper environment: `lock/state` said `unlocked` (both sides) while the
/// promotion status said `UNLOCKING` — so neither is reported as the answer.
pub fn disagreements(lock: &LockState, status: &Status) -> Vec<String> {
    let mut out = Vec::new();
    let lock_word = lock.result.to_ascii_lowercase();
    let status_word = status.status.to_ascii_lowercase();
    if lock_word != status_word {
        out.push(format!(
            "lock/state says `{}` but the promotion status says `{}`{}",
            lock.result,
            status.status,
            status
                .message
                .as_ref()
                .map(|m| format!(" ({m})"))
                .unwrap_or_default()
        ));
    }
    if let Some(global) = &status.global_lock
        && global.to_ascii_lowercase() != lock_word
    {
        out.push(format!(
            "the promotion status's globalLock is `{global}`, lock/state's result is `{}`",
            lock.result
        ));
    }
    for (name, side) in [("lower", &lock.lower), ("upper", &lock.upper)] {
        if let Some(side) = side {
            if side.state != lock.result {
                out.push(format!("the {name} environment is `{}`", side.state));
            }
            if let Some(proxy) = &side.proxy_state
                && *proxy != side.state
            {
                out.push(format!(
                    "the {name} environment's proxy is `{proxy}` while its state is `{}`",
                    side.state
                ));
            }
        }
    }
    out
}

/// What a failed promotion call means, from its status and body. `None` is
/// "no special meaning" — show the error as it is.
pub fn explain(error: &Error, tenant: &str) -> Option<String> {
    let Error::Api { status, body } = error else {
        return None;
    };
    match status {
        403 if body.contains("sufficient scope") => Some(format!(
            "{tenant}'s token does not carry fr:idc:promotion:* — this is a permissions answer, \
             not a lock state. `aic ctx scopes --tenant {tenant}` shows whether the tenant grants \
             it and whether aic requests it"
        )),
        503 => Some(format!(
            "no promotion service answers on {tenant} (a sandbox has none) — nothing is known \
             about its lock"
        )),
        404 if body.contains("No latest report") => Some(format!(
            "{tenant} has no promotion report — on a pair, reports live on the upper environment"
        )),
        _ => None,
    }
}

/// One `(section, configItem, change)` group of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub section: String,
    pub item: String,
    pub change: String,
    pub names: Vec<String>,
}

/// The `report` of a promotion report as groups, in the tenant's order.
/// Anything that does not have the measured shape is an error: a summary that
/// dropped what it could not read would undercount the difference.
pub fn summarise(report: &Value) -> Result<Vec<Group>, String> {
    let sections = report
        .get("report")
        .and_then(Value::as_object)
        .ok_or_else(|| "the report has no `report` object".to_string())?;
    let mut groups = Vec::new();
    for (section, items) in sections {
        let items = items
            .as_array()
            .ok_or_else(|| format!("report section {section} is not an array"))?;
        for item in items {
            let name = text(item, "configItem")
                .ok_or_else(|| format!("an item in {section} has no `configItem`: {item}"))?;
            let changes = item
                .get("configChange")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{section} / {name} has no `configChange` object"))?;
            for (change, entries) in changes {
                let entries = entries
                    .as_array()
                    .ok_or_else(|| format!("{section} / {name} / {change} is not an array"))?;
                let names = entries
                    .iter()
                    .map(|entry| {
                        let label = text(entry, "name").ok_or_else(|| {
                            format!("an entry in {section} / {name} has no `name`: {entry}")
                        })?;
                        Ok(match text(entry, "realm") {
                            Some(realm) => format!("{label} ({realm})"),
                            None => label,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                groups.push(Group {
                    section: section.clone(),
                    item: name.clone(),
                    change: change.clone(),
                    names,
                });
            }
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_upper_environments_measured_disagreement_is_reported() {
        let lock = lock_state(&json!({
            "description": "Environment unlocked",
            "lowerEnv": {"proxyState": "unlocked", "state": "unlocked"},
            "result": "unlocked",
            "upperEnv": {"proxyState": "unlocked", "state": "unlocked"}
        }))
        .unwrap();
        let status = status(&json!({
            "blockingError": false, "globalLock": "UNLOCKED",
            "message": "Environment unlock in progress", "status": "UNLOCKING",
            "timeStamp": "2026-10-26T04:52:42Z", "type": "promotion"
        }))
        .unwrap();
        let found = disagreements(&lock, &status);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("UNLOCKING"), "{found:?}");
    }

    /// The control: the lower environment as measured, all agreeing.
    #[test]
    fn an_agreeing_lower_environment_reports_nothing() {
        let lock =
            lock_state(&json!({"description": "Environment unlocked", "result": "unlocked"}))
                .unwrap();
        assert_eq!(lock.lower, None);
        let status = status(&json!({
            "blockingError": false, "globalLock": "UNLOCKED",
            "message": "Environment unlocked", "status": "UNLOCKED"
        }))
        .unwrap();
        assert!(disagreements(&lock, &status).is_empty());
    }

    #[test]
    fn a_locked_side_and_a_lagging_proxy_are_each_named() {
        let lock = lock_state(&json!({
            "result": "unlocked",
            "lowerEnv": {"state": "locked", "proxyState": "locked"},
            "upperEnv": {"state": "unlocked", "proxyState": "locked"}
        }))
        .unwrap();
        let status = status(&json!({"status": "UNLOCKED", "globalLock": "UNLOCKED"})).unwrap();
        let found = disagreements(&lock, &status).join("\n");
        assert!(found.contains("lower environment is `locked`"), "{found}");
        assert!(
            found.contains("upper environment's proxy is `locked`"),
            "{found}"
        );
    }

    #[test]
    fn a_body_without_result_or_status_is_an_error_not_unlocked() {
        assert!(lock_state(&json!({"description": "x"})).is_err());
        assert!(status(&json!({"message": "x"})).is_err());
        assert!(
            lock_state(&json!({"result": "unlocked", "upperEnv": {"proxyState": "x"}})).is_err()
        );
    }

    #[test]
    fn scope_and_missing_service_are_told_apart_from_lock_state() {
        let scope = Error::Api {
            status: 403,
            body: r#"{"code":403,"message":"The access token does not have sufficient scope"}"#
                .into(),
        };
        assert!(
            explain(&scope, "dev")
                .unwrap()
                .contains("aic ctx scopes --tenant dev")
        );
        let none = Error::Api {
            status: 503,
            body: "<html>No server is available</html>".into(),
        };
        assert!(
            explain(&none, "sandbox")
                .unwrap()
                .contains("no promotion service")
        );
        let other = Error::Api {
            status: 403,
            body: "Access denied".into(),
        };
        assert_eq!(explain(&other, "dev"), None);
    }

    #[test]
    fn summarise_groups_every_entry_and_keeps_realms() {
        let groups = summarise(&json!({"report": {
            "AMConfig": [
                {"configItem": "Journeys", "configChange": {
                    "added": [{"name": "A", "realm": "/bravo"}],
                    "modified": [{"name": "B", "realm": "/alpha"}, {"name": "C", "realm": "/alpha"}]
                }},
                {"configItem": "Global Scripts", "configChange": {"added": [{"name": "Toggle"}]}}
            ],
            "IDMConfig": [{"configItem": "Themes", "configChange": {"modified": [{"name": "Starter", "realm": "alpha"}]}}]
        }}))
        .unwrap();
        let total: usize = groups.iter().map(|g| g.names.len()).sum();
        assert_eq!(total, 5);
        assert_eq!(groups[0].names, vec!["A (/bravo)"]);
        assert_eq!(groups[2].names, vec!["Toggle"]);
        assert_eq!(groups[3].section, "IDMConfig");
    }

    #[test]
    fn summarise_refuses_a_shape_it_cannot_count() {
        assert!(summarise(&json!({})).is_err());
        assert!(summarise(&json!({"report": {"AMConfig": {}}})).is_err());
        assert!(summarise(&json!({"report": {"AMConfig": [{"configItem": "J", "configChange": {"added": [{"realm": "/a"}]}}]}})).is_err());
    }

    #[test]
    fn report_paths_are_the_measured_ones() {
        assert_eq!(Which::Latest.path(), "/environment/promotion/report");
        assert_eq!(
            Which::Provisional.path(),
            "/environment/promotion/report/provisional"
        );
        assert_eq!(
            Which::Id("a/b".into()).path(),
            "/environment/promotion/report/a%2Fb"
        );
    }
}
