//! Verified HTTP wrappers for IDM internal roles.
//! See `docs/api/18-internal-roles.md`.

use serde_json::Value;

use crate::{Error, Result};

const API_VERSION: &str = "resource=1.0";
const ROLES_PATH: &str = "/openidm/internal/role";

/// List roles with the non-default `privileges` field included.
pub async fn list_roles(tenant: &str) -> Result<Vec<Value>> {
    let path = format!(
        "{ROLES_PATH}?_queryFilter=true&_fields=name,description,privileges&_pageSize=1000"
    );
    let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected internal-role list shape: {body}"),
        })?;
    let mut roles = result.clone();
    roles.sort_by_cached_key(|role| {
        role.get("_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase()
    });
    Ok(roles)
}

/// Read one whole role. A field projection is unsafe for amend-and-write.
pub async fn read_role(tenant: &str, id: &str) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

/// Create or fully replace a role. Callers own create-only and merge safety.
pub async fn put_role(tenant: &str, id: &str, body: Value) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    crate::aic::api::put_versioned(tenant, &path, body, false, API_VERSION).await
}

/// Fully replace a role only when its current revision still matches.
pub async fn put_role_if_match(
    tenant: &str,
    id: &str,
    body: Value,
    revision: &str,
) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    crate::aic::api::put_versioned_if_match(tenant, &path, body, false, API_VERSION, revision).await
}

pub async fn delete_role(tenant: &str, id: &str) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    crate::aic::api::delete_versioned(tenant, &path, false, API_VERSION).await
}
