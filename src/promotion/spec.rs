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

/// An optional string field: absent or `null` is `None`, a present value of
/// another type is an error — never quietly read as absent.
fn opt_text(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!("`{key}` is not a string: {other}")),
    }
}

fn opt_bool(value: &Value, key: &str) -> Result<Option<bool>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(format!("`{key}` is not a boolean: {other}")),
    }
}

fn env(value: &Value, key: &str) -> Result<Option<EnvLock>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(side) => Ok(Some(EnvLock {
            state: text(side, "state")
                .ok_or_else(|| format!("`{key}` has no string `state`: {side}"))?,
            proxy_state: opt_text(side, "proxyState")?,
        })),
    }
}

/// Decode `lock/state`; a body without a string `result` is an error, never
/// "unlocked".
pub fn lock_state(value: &Value) -> Result<LockState, String> {
    Ok(LockState {
        result: text(value, "result")
            .ok_or_else(|| format!("lock state has no string `result`: {value}"))?,
        description: opt_text(value, "description")?,
        lower: env(value, "lowerEnv")?,
        upper: env(value, "upperEnv")?,
    })
}

pub fn status(value: &Value) -> Result<Status, String> {
    Ok(Status {
        status: text(value, "status")
            .ok_or_else(|| format!("promotion status has no string `status`: {value}"))?,
        message: opt_text(value, "message")?,
        global_lock: opt_text(value, "globalLock")?,
        blocking_error: opt_bool(value, "blockingError")?,
        timestamp: opt_text(value, "timeStamp")?,
    })
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Where the two endpoints that each describe the lock describe it
/// differently. Measured on an upper environment: `lock/state` said
/// `unlocked` (both sides) while the promotion status said `UNLOCKING`. What
/// is measured is that they *can* differ, not which fields must always be
/// equal — `UNLOCKING` may be a transition, and the two reads are sequential —
/// so a difference is reported as an indeterminate observation, never as the
/// answer. Case is never a difference.
pub fn disagreements(lock: &LockState, status: &Status) -> Vec<String> {
    let mut out = Vec::new();
    if !same(&lock.result, &status.status) {
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
        && !same(global, &lock.result)
    {
        out.push(format!(
            "the promotion status's globalLock is `{global}`, lock/state's result is `{}`",
            lock.result
        ));
    }
    for (name, side) in [("lower", &lock.lower), ("upper", &lock.upper)] {
        if let Some(side) = side {
            if !same(&side.state, &lock.result) {
                out.push(format!("the {name} environment is `{}`", side.state));
            }
            if let Some(proxy) = &side.proxy_state
                && !same(proxy, &side.state)
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

/// One `status` run's conclusion, decided before anything is rendered so
/// that `--json` and the table exit the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub lock: LockState,
    pub status: Status,
    pub disagreements: Vec<String>,
}

impl Assessment {
    /// Reported separately from the disagreements: both endpoints can agree
    /// and the status still carry a blocking error.
    pub fn blocking(&self) -> bool {
        self.status.blocking_error == Some(true)
    }

    /// Non-zero exit: the sources differ, or a blocking error is reported.
    /// Agreeing `LOCKED` values pass — this is not a readiness check.
    pub fn ok(&self) -> bool {
        self.disagreements.is_empty() && !self.blocking()
    }
}

pub fn assess(lock_raw: &Value, status_raw: &Value) -> Result<Assessment, String> {
    let lock = lock_state(lock_raw)?;
    let status = status(status_raw)?;
    let disagreements = disagreements(&lock, &status);
    Ok(Assessment {
        lock,
        status,
        disagreements,
    })
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
    fn case_alone_is_not_a_disagreement() {
        let lock = lock_state(&json!({
            "result": "unlocked",
            "lowerEnv": {"state": "UNLOCKED", "proxyState": "Unlocked"},
            "upperEnv": {"state": "unlocked", "proxyState": "UNLOCKED"}
        }))
        .unwrap();
        let status = status(&json!({"status": "Unlocked", "globalLock": "UNLOCKED"})).unwrap();
        assert_eq!(disagreements(&lock, &status), Vec::<String>::new());
    }

    /// The discriminating case: everything agrees, and a blocking error is
    /// reported anyway. It must fail the run on its own.
    #[test]
    fn a_blocking_error_fails_the_assessment_when_the_sources_agree() {
        let lock = json!({"result": "unlocked"});
        let clean = assess(
            &lock,
            &json!({"status": "UNLOCKED", "blockingError": false}),
        )
        .unwrap();
        assert!(clean.ok());
        let blocked = assess(&lock, &json!({"status": "UNLOCKED", "blockingError": true})).unwrap();
        assert!(blocked.disagreements.is_empty());
        assert!(!blocked.ok());
    }

    #[test]
    fn a_present_optional_field_of_the_wrong_type_is_an_error_not_absent() {
        assert!(status(&json!({"status": "UNLOCKED", "blockingError": "true"})).is_err());
        assert!(status(&json!({"status": "UNLOCKED", "globalLock": 1})).is_err());
        assert!(lock_state(&json!({"result": "unlocked", "lowerEnv": {"state": "unlocked", "proxyState": false}})).is_err());
        assert!(status(&json!({"status": "UNLOCKED", "globalLock": null})).is_ok());
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
