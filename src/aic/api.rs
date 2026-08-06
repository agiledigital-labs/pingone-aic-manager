//! Surface-agnostic AIC HTTP helpers. **The TUI and CLI both call into here**
//! — neither one builds an `AicClient` directly. The bearer-mint, token
//! cache, prod-confirm guard, and HTTP connection pool all live in the
//! agent process; this module is the thin client glue that wraps a single
//! request/response in the `agent::Request::ApiCall` envelope.
//!
//! Add a new resource (scripts, OAuth2, journeys) by adding a module
//! alongside this one (e.g. `esv::api`) that uses these primitives — do
//! NOT thread a parallel HTTP path through `AicClient::*` in either
//! frontend.
//!
//! [`crate::config::operator`] also uses [`get_versioned`] for its optional,
//! best-effort service-account-name lookup; keeping that read here means
//! operator resolution never creates a second bearer or transport path.

use crate::agent::{AgentClient, Request, Response};
use crate::{Error, Result};

pub async fn get(tenant: &str, path: &str) -> Result<serde_json::Value> {
    call(tenant, "GET", path, None, false, None).await
}

pub async fn put(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    call(tenant, "PUT", path, Some(body), confirmed_prod, None).await
}

pub async fn post(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    call(tenant, "POST", path, Some(body), confirmed_prod, None).await
}

/// `POST` a form-encoded body through the daemon-owned HTTP connection pool.
/// The form transport does not attach the service-account bearer because OAuth2
/// token endpoints authenticate from the form body.
pub async fn post_form(tenant: &str, path: &str, body: &str) -> Result<serde_json::Value> {
    call_form(tenant, "POST", path, body, false).await
}

/// `POST` with an explicit `Accept-API-Version`.
pub async fn post_versioned(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    call(
        tenant,
        "POST",
        path,
        Some(body),
        confirmed_prod,
        Some(api_version),
    )
    .await
}

pub async fn patch(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    call(tenant, "PATCH", path, Some(body), confirmed_prod, None).await
}

pub async fn delete(tenant: &str, path: &str, confirmed_prod: bool) -> Result<serde_json::Value> {
    call(tenant, "DELETE", path, None, confirmed_prod, None).await
}

/// `GET` with an explicit `Accept-API-Version` (AM scripts need
/// `protocol=2.0,resource=1.0`; IDM config endpoints set their own).
pub async fn get_versioned(
    tenant: &str,
    path: &str,
    api_version: &str,
) -> Result<serde_json::Value> {
    call(tenant, "GET", path, None, false, Some(api_version)).await
}

/// `PUT` with an explicit `Accept-API-Version`.
pub async fn put_versioned(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    call(
        tenant,
        "PUT",
        path,
        Some(body),
        confirmed_prod,
        Some(api_version),
    )
    .await
}

/// `DELETE` with an explicit `Accept-API-Version`.
pub async fn delete_versioned(
    tenant: &str,
    path: &str,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    call(
        tenant,
        "DELETE",
        path,
        None,
        confirmed_prod,
        Some(api_version),
    )
    .await
}

async fn call(
    tenant: &str,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    confirmed_prod: bool,
    api_version: Option<&str>,
) -> Result<serde_json::Value> {
    let agent = AgentClient::connect_or_spawn().await?;
    let resp = agent
        .send(&Request::ApiCall {
            tenant: tenant.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            body,
            confirmed_prod,
            content_type: None,
            api_version: api_version.map(|s| s.to_string()),
        })
        .await?;
    match resp {
        Response::Json { value } => Ok(value),
        Response::Locked => Err(Error::Auth(
            "agent is locked — run `aic session login` (CLI) or unlock the TUI".into(),
        )),
        Response::ProdConfirmRequired => Err(Error::ProdConfirmRequired),
        Response::ApiError { status, body } => Err(Error::Api { status, body }),
        Response::Error { message } => Err(Error::Config(message)),
        other => Err(Error::Config(format!("unexpected agent reply: {other:?}"))),
    }
}

async fn call_form(
    tenant: &str,
    method: &str,
    path: &str,
    body: &str,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    let agent = AgentClient::connect_or_spawn().await?;
    let resp = agent
        .send(&Request::ApiCall {
            tenant: tenant.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            body: Some(serde_json::Value::String(body.to_string())),
            confirmed_prod,
            content_type: Some("application/x-www-form-urlencoded".into()),
            api_version: None,
        })
        .await?;
    match resp {
        Response::Json { value } => Ok(value),
        Response::Locked => Err(Error::Auth(
            "agent is locked — run `aic session login` (CLI) or unlock the TUI".into(),
        )),
        Response::ProdConfirmRequired => Err(Error::ProdConfirmRequired),
        Response::ApiError { status, body } => Err(Error::Api { status, body }),
        Response::Error { message } => Err(Error::Config(message)),
        other => Err(Error::Config(format!("unexpected agent reply: {other:?}"))),
    }
}
