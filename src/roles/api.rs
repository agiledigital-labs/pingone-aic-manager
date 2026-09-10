//! Verified HTTP wrappers for IDM internal roles.
//! See `docs/api/18-internal-roles.md`.

use serde_json::Value;

use crate::aic::api::ApiCall;
use crate::{Error, Result};

const API_VERSION: &str = "resource=1.0";
const ROLES_PATH: &str = "/openidm/internal/role";

fn role_write_call<'a>(
    tenant: &'a str,
    method: &'a str,
    path: &'a str,
    body: Option<Value>,
    confirmed_prod: bool,
    revision: Option<&'a str>,
) -> ApiCall<'a> {
    let mut call = ApiCall::new(tenant, method, path)
        .confirmed_prod(confirmed_prod)
        .api_version(API_VERSION);
    if let Some(body) = body {
        call = call.body(body);
    }
    if let Some(revision) = revision {
        call = call.if_match(revision);
    }
    call
}

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
pub async fn put_role(tenant: &str, id: &str, body: Value, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    role_write_call(tenant, "PUT", &path, Some(body), confirmed_prod, None)
        .send()
        .await
}

/// Fully replace a role only when its current revision still matches.
pub async fn put_role_if_match(
    tenant: &str,
    id: &str,
    body: Value,
    revision: &str,
    confirmed_prod: bool,
) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    role_write_call(
        tenant,
        "PUT",
        &path,
        Some(body),
        confirmed_prod,
        Some(revision),
    )
    .send()
    .await
}

pub async fn delete_role(tenant: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{ROLES_PATH}/{id}");
    role_write_call(tenant, "DELETE", &path, None, confirmed_prod, None)
        .send()
        .await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::Request;

    fn confirmed(call: ApiCall<'_>) -> bool {
        let Request::ApiCall(request) = call.envelope() else {
            panic!("expected API call");
        };
        request.confirmed_prod
    }

    #[test]
    fn every_role_write_forwards_the_callers_production_consent() {
        for (method, body, revision) in [
            ("PUT", Some(json!({"name": "support"})), None),
            ("PUT", Some(json!({"name": "support"})), Some("rev-1")),
            ("DELETE", None, None),
        ] {
            let path = format!("{ROLES_PATH}/support");
            assert!(!confirmed(role_write_call(
                "prod",
                method,
                &path,
                body.clone(),
                false,
                revision,
            )));
            assert!(confirmed(role_write_call(
                "prod", method, &path, body, true, revision,
            )));
        }
    }
}
