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

// ---------------------------------------------------------------------------
// Secrets
//
// Secrets differ from variables in important ways (verified 2026-05-30, see
// docs/api/03-esvs.md):
//   * `PUT` is CREATE-ONLY — re-PUT on an existing id 400s. Value changes go
//     through versions, not PUT.
//   * Values are write-only — the API never returns plaintext.
//   * `encoding` and `useInPlaceholders` are required at create and immutable.
//   * Versions carry status (ENABLED/DISABLED/DESTROYED); the latest can't be
//     disabled; DESTROYED is one-way via DELETE on the version.
//   * `useInPlaceholders:false` secrets load immediately and never gate a
//     restart.
// ---------------------------------------------------------------------------

/// `GET /environment/secrets` → the `result` array of secret metadata.
pub async fn list_secrets(tenant: &str) -> Result<Vec<serde_json::Value>> {
    list_result_at(tenant, "/environment/secrets").await
}

/// `GET /environment/secrets?_onlyPending=true` → secrets whose active version
/// hasn't loaded yet (only `useInPlaceholders:true` secrets ever appear here).
pub async fn list_pending_secrets(tenant: &str) -> Result<Vec<serde_json::Value>> {
    list_result_at(tenant, "/environment/secrets?_onlyPending=true").await
}

async fn list_result_at(tenant: &str, path: &str) -> Result<Vec<serde_json::Value>> {
    let body = super::api::get(tenant, path).await?;
    match body.get("result") {
        Some(serde_json::Value::Array(arr)) => Ok(arr.clone()),
        _ => Err(Error::Api {
            status: 0,
            body: format!("unexpected {path} response shape: {body}"),
        }),
    }
}

/// `GET /environment/secrets/{id}` → single secret metadata (no value).
pub async fn get_secret(tenant: &str, id: &str) -> Result<serde_json::Value> {
    super::api::get(tenant, &format!("/environment/secrets/{id}")).await
}

/// `PUT /environment/secrets/{id}` — **create only**. All three of `encoding`,
/// `useInPlaceholders`, `valueBase64` are required by the API. Returns the
/// created secret object.
pub async fn create_secret(
    tenant: &str,
    id: &str,
    encoding: &str,
    use_in_placeholders: bool,
    value_base64: &str,
    description: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "encoding": encoding,
        "useInPlaceholders": use_in_placeholders,
        "valueBase64": value_base64,
        "description": description,
    });
    super::api::put(
        tenant,
        &format!("/environment/secrets/{id}"),
        body,
        confirmed_prod,
    )
    .await
}

/// `POST /environment/secrets/{id}?_action=setDescription`. The only mutating
/// action available on an existing secret's metadata.
pub async fn set_secret_description(
    tenant: &str,
    id: &str,
    description: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::post(
        tenant,
        &format!("/environment/secrets/{id}?_action=setDescription"),
        serde_json::json!({ "description": description }),
        confirmed_prod,
    )
    .await
}

/// `DELETE /environment/secrets/{id}` — removes the secret and all versions.
pub async fn delete_secret(
    tenant: &str,
    id: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::delete(tenant, &format!("/environment/secrets/{id}"), confirmed_prod).await
}

/// `GET /environment/secrets/{id}/versions` → a **bare array** (no `result`
/// wrapper), newest-first. Each entry is `{version, createDate, loaded, status}`.
pub async fn list_secret_versions(tenant: &str, id: &str) -> Result<Vec<serde_json::Value>> {
    let body = super::api::get(tenant, &format!("/environment/secrets/{id}/versions")).await?;
    match body {
        serde_json::Value::Array(arr) => Ok(arr),
        other => Err(Error::Api {
            status: 0,
            body: format!("unexpected secret-versions response shape: {other}"),
        }),
    }
}

/// `POST /environment/secrets/{id}/versions?_action=create`. The new version
/// is auto-ENABLED and becomes the active version. Returns the version object.
pub async fn create_secret_version(
    tenant: &str,
    id: &str,
    value_base64: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::post(
        tenant,
        &format!("/environment/secrets/{id}/versions?_action=create"),
        serde_json::json!({ "valueBase64": value_base64 }),
        confirmed_prod,
    )
    .await
}

/// `POST /environment/secrets/{id}/versions/{version}?_action=changestatus`.
/// `status` must be `ENABLED` or `DISABLED` (DESTROYED is rejected here — use
/// `destroy_secret_version`). The latest version cannot be disabled.
pub async fn change_version_status(
    tenant: &str,
    id: &str,
    version: &str,
    status: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::post(
        tenant,
        &format!("/environment/secrets/{id}/versions/{version}?_action=changestatus"),
        serde_json::json!({ "status": status }),
        confirmed_prod,
    )
    .await
}

/// `DELETE /environment/secrets/{id}/versions/{version}` — sets the version's
/// status to DESTROYED (irreversible). The version stays listed as DESTROYED.
pub async fn destroy_secret_version(
    tenant: &str,
    id: &str,
    version: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    super::api::delete(
        tenant,
        &format!("/environment/secrets/{id}/versions/{version}"),
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
