//! ESV (Environment-Specific Variable) helpers shared by TUI and CLI.
//! All HTTP goes through `aic::api` → agent.

use crate::{Error, Result};

/// Authoritative tenant startup state from `GET /environment/startup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStatus {
    Ready,
    Restarting,
}

/// `GET /environment/variables` → returns the `result` array of variable
/// objects (see `docs/api/03-esvs.md` for the object shape). Pagination not
/// implemented; AIC's default page size is 1000 which is fine for "show me
/// the list."
pub async fn list_variables(tenant: &str) -> Result<Vec<serde_json::Value>> {
    list_variables_at(tenant, "/environment/variables").await
}

/// `GET /environment/variables?_onlyPending=true` → variables that Ping
/// currently says need a restart/apply. This is authoritative for existing
/// variables; deletes are not returned here.
pub async fn list_pending_variables(tenant: &str) -> Result<Vec<serde_json::Value>> {
    list_variables_at(tenant, "/environment/variables?_onlyPending=true").await
}

async fn list_variables_at(tenant: &str, path: &str) -> Result<Vec<serde_json::Value>> {
    let body = super::api::get(tenant, path).await?;
    match body.get("result") {
        Some(serde_json::Value::Array(arr)) => Ok(arr.clone()),
        _ => Err(Error::Api {
            status: 0,
            body: format!("unexpected {path} response shape: {body}"),
        }),
    }
}

/// `GET /environment/startup` → current runtime restart status.
pub async fn startup_status(tenant: &str) -> Result<StartupStatus> {
    let body = super::api::get(tenant, "/environment/startup").await?;
    parse_startup_status(&body)
}

pub fn parse_startup_status(body: &serde_json::Value) -> Result<StartupStatus> {
    match body.get("restartStatus").and_then(|v| v.as_str()) {
        Some("ready") => Ok(StartupStatus::Ready),
        Some("restarting") => Ok(StartupStatus::Restarting),
        _ => Err(Error::Api {
            status: 0,
            body: format!("unexpected /environment/startup response shape: {body}"),
        }),
    }
}

/// `GET /environment/variables/{id}` → single variable object. Used to
/// refresh a record right before saving so we can do content-based
/// conflict detection (variables have no `_rev`).
pub async fn get_variable(tenant: &str, id: &str) -> Result<serde_json::Value> {
    super::api::get(tenant, &format!("/environment/variables/{id}")).await
}

/// `PUT /environment/variables/{id}` with the editable fields. Returns the
/// server's response (echoes the saved object). `confirmed_prod` is
/// forwarded to the agent so prod-themed tenants can be guarded by the
/// existing confirm flow.
pub async fn update_variable(
    tenant: &str,
    id: &str,
    description: &str,
    expression_type: &str,
    value_base64: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "valueBase64": value_base64,
        "expressionType": expression_type,
        "description": description,
    });
    super::api::put(
        tenant,
        &format!("/environment/variables/{id}"),
        body,
        confirmed_prod,
    )
    .await
}

/// `DELETE /environment/variables/{id}`. Used as the first half of a
/// type-change save — AIC rejects in-place type changes on existing
/// variables, but immediately recreating after a delete with a new type
/// is fine (verified on the sandbox 2026-05-26). No restart needed
/// between delete and recreate.
pub async fn delete_variable(
    tenant: &str,
    id: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::delete(
        tenant,
        &format!("/environment/variables/{id}"),
        confirmed_prod,
    )
    .await
}

/// `POST /environment/startup?_action=restart`. Triggers a tenant-wide
/// restart so freshly-saved ESVs become the loaded values. Per the docs
/// rate limits are tighter than the read endpoints — guard the call
/// behind a user-confirmed action, never poll. Returns the server's
/// response body (typically `{"restartStatus":"restarting"}`).
pub async fn trigger_restart(tenant: &str, confirmed_prod: bool) -> Result<serde_json::Value> {
    super::api::post(
        tenant,
        "/environment/startup?_action=restart",
        serde_json::json!({}),
        confirmed_prod,
    )
    .await
}

/// True iff the editable content of two variable objects matches. Used
/// for conflict detection just before a PUT — we refetch and compare
/// against the snapshot the user started editing from. Server-managed
/// fields (`lastChangeDate`, `lastChangedBy`, `loaded`) are ignored.
pub fn content_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    let pick = |v: &serde_json::Value| {
        (
            v.get("valueBase64")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("expressionType")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        )
    };
    pick(a) == pick(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_startup_status() {
        assert_eq!(
            parse_startup_status(&serde_json::json!({"restartStatus":"ready"})).unwrap(),
            StartupStatus::Ready
        );
        assert_eq!(
            parse_startup_status(&serde_json::json!({"restartStatus":"restarting"})).unwrap(),
            StartupStatus::Restarting
        );
        assert!(parse_startup_status(&serde_json::json!({"restartStatus":"unknown"})).is_err());
    }
}
