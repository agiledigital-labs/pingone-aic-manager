//! Verified HTTP wrappers for the AM entitlement API.
//! See `docs/api/21-am-policies.md`.

use serde_json::{Value, json};

use crate::{Error, Result};

/// Policies and policy sets. Resource types and the type catalogs answer on
/// the default `resource=1.0` and must not send this.
const ENTITLEMENT_VERSION: &str = "protocol=1.0,resource=2.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn policies_path(realm: &str) -> String {
    format!("{}/policies", realm_path(realm))
}

fn sets_path(realm: &str) -> String {
    format!("{}/applications", realm_path(realm))
}

fn resource_types_path(realm: &str) -> String {
    format!("{}/resourcetypes", realm_path(realm))
}

/// AM names go straight into a URL path. A separator would silently address a
/// different collection, so refuse it here rather than send it.
fn validate_name(kind: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Config(format!("{kind} name is empty")));
    }
    if name.contains('/') || name.contains('?') || name.contains('#') {
        return Err(Error::Config(format!(
            "{kind} name {name:?} contains a URL separator"
        )));
    }
    Ok(())
}

fn results(body: &Value, what: &str) -> Result<Vec<Value>> {
    body.get("result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected {what} list shape: {body}"),
        })
}

fn by_name(mut items: Vec<Value>) -> Vec<Value> {
    items.sort_by_cached_key(|item| {
        item.get("name")
            .or_else(|| item.get("_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase()
    });
    items
}

pub fn is_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

// ---------------------------------------------------------------- policies

pub async fn list_policies(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let path = format!("{}?_queryFilter=true&_pageSize=1000", policies_path(realm));
    let body = crate::aic::api::get_versioned(tenant, &path, ENTITLEMENT_VERSION).await?;
    Ok(by_name(results(&body, "policy")?))
}

pub async fn read_policy(tenant: &str, realm: &str, name: &str) -> Result<Value> {
    validate_name("policy", name)?;
    let path = format!("{}/{name}", policies_path(realm));
    crate::aic::api::get_versioned(tenant, &path, ENTITLEMENT_VERSION).await
}

pub async fn create_policy(tenant: &str, realm: &str, body: Value, prod: bool) -> Result<Value> {
    let path = format!("{}?_action=create", policies_path(realm));
    crate::aic::api::post_versioned(tenant, &path, body, prod, ENTITLEMENT_VERSION).await
}

pub async fn update_policy(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    prod: bool,
) -> Result<Value> {
    validate_name("policy", name)?;
    let path = format!("{}/{name}", policies_path(realm));
    crate::aic::api::put_versioned(tenant, &path, body, prod, ENTITLEMENT_VERSION).await
}

pub async fn delete_policy(tenant: &str, realm: &str, name: &str, prod: bool) -> Result<Value> {
    validate_name("policy", name)?;
    let path = format!("{}/{name}", policies_path(realm));
    crate::aic::api::delete_versioned(tenant, &path, prod, ENTITLEMENT_VERSION).await
}

// ------------------------------------------------------------- policy sets

pub async fn list_sets(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let path = format!("{}?_queryFilter=true&_pageSize=1000", sets_path(realm));
    let body = crate::aic::api::get_versioned(tenant, &path, ENTITLEMENT_VERSION).await?;
    Ok(by_name(results(&body, "policy set")?))
}

pub async fn read_set(tenant: &str, realm: &str, name: &str) -> Result<Value> {
    validate_name("policy set", name)?;
    let path = format!("{}/{name}", sets_path(realm));
    crate::aic::api::get_versioned(tenant, &path, ENTITLEMENT_VERSION).await
}

pub async fn create_set(tenant: &str, realm: &str, body: Value, prod: bool) -> Result<Value> {
    let path = format!("{}?_action=create", sets_path(realm));
    crate::aic::api::post_versioned(tenant, &path, body, prod, ENTITLEMENT_VERSION).await
}

pub async fn update_set(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    prod: bool,
) -> Result<Value> {
    validate_name("policy set", name)?;
    let path = format!("{}/{name}", sets_path(realm));
    crate::aic::api::put_versioned(tenant, &path, body, prod, ENTITLEMENT_VERSION).await
}

pub async fn delete_set(tenant: &str, realm: &str, name: &str, prod: bool) -> Result<Value> {
    validate_name("policy set", name)?;
    let path = format!("{}/{name}", sets_path(realm));
    crate::aic::api::delete_versioned(tenant, &path, prod, ENTITLEMENT_VERSION).await
}

// ---------------------------------------------------------- resource types

pub async fn list_resource_types(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let path = format!(
        "{}?_queryFilter=true&_pageSize=1000",
        resource_types_path(realm)
    );
    let body = crate::aic::api::get(tenant, &path).await?;
    Ok(by_name(results(&body, "resource type")?))
}

pub async fn read_resource_type(tenant: &str, realm: &str, id: &str) -> Result<Value> {
    validate_name("resource type", id)?;
    let path = format!("{}/{id}", resource_types_path(realm));
    crate::aic::api::get(tenant, &path).await
}

/// The one collection where `PUT /{id}` both creates (201) and updates.
pub async fn put_resource_type(
    tenant: &str,
    realm: &str,
    id: &str,
    body: Value,
    prod: bool,
) -> Result<Value> {
    validate_name("resource type", id)?;
    let path = format!("{}/{id}", resource_types_path(realm));
    crate::aic::api::put(tenant, &path, body, prod).await
}

pub async fn delete_resource_type(
    tenant: &str,
    realm: &str,
    id: &str,
    prod: bool,
) -> Result<Value> {
    validate_name("resource type", id)?;
    let path = format!("{}/{id}", resource_types_path(realm));
    crate::aic::api::delete(tenant, &path, prod).await
}

// -------------------------------------------------------------- the create
//                                                                 asymmetry

/// Create-or-update a policy, choosing the verb the collection actually
/// accepts. A `PUT` to a name that does not exist is a 404, not a create, so
/// the existence probe is not an optimisation — it is the contract.
pub async fn upsert_policy(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    prod: bool,
) -> Result<(Value, Written)> {
    match read_policy(tenant, realm, name).await {
        Ok(_) => Ok((
            update_policy(tenant, realm, name, body, prod).await?,
            Written::Updated,
        )),
        Err(error) if is_not_found(&error) => Ok((
            create_policy(tenant, realm, body, prod).await?,
            Written::Created,
        )),
        Err(error) => Err(error),
    }
}

/// Create-or-update a policy set. Same asymmetry as a policy.
pub async fn upsert_set(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    prod: bool,
) -> Result<(Value, Written)> {
    match read_set(tenant, realm, name).await {
        Ok(_) => Ok((
            update_set(tenant, realm, name, body, prod).await?,
            Written::Updated,
        )),
        Err(error) if is_not_found(&error) => Ok((
            create_set(tenant, realm, body, prod).await?,
            Written::Created,
        )),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    Created,
    Updated,
}

impl Written {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

// ---------------------------------------------------------------- catalogs

pub async fn subject_types(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let path = format!("{}/subjecttypes?_queryFilter=true", realm_path(realm));
    let body = crate::aic::api::get(tenant, &path).await?;
    Ok(by_name(results(&body, "subject type")?))
}

pub async fn condition_types(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let path = format!("{}/conditiontypes?_queryFilter=true", realm_path(realm));
    let body = crate::aic::api::get(tenant, &path).await?;
    Ok(by_name(results(&body, "condition type")?))
}

// ------------------------------------------------------------------- PDP

/// `POST …/policies?_action=evaluate`. Returns one row per requested resource.
///
/// **AM does not authenticate `subject.jwt`** — not the signature, not the
/// expiry. A resource server must verify the token locally first; see the
/// warning in `docs/api/21-am-policies.md`.
pub async fn evaluate(tenant: &str, realm: &str, body: Value) -> Result<Vec<Value>> {
    let path = format!("{}?_action=evaluate", policies_path(realm));
    let response =
        crate::aic::api::post_versioned(tenant, &path, body, false, ENTITLEMENT_VERSION).await?;
    response.as_array().cloned().ok_or_else(|| Error::Api {
        status: 0,
        body: format!("unexpected evaluate response shape: {response}"),
    })
}

/// `?_action=evaluateTree` takes `resource`, singular. Kept because the shape
/// difference is exactly the sort of thing a caller gets wrong once.
pub async fn evaluate_tree(tenant: &str, realm: &str, body: Value) -> Result<Value> {
    let path = format!("{}?_action=evaluateTree", policies_path(realm));
    crate::aic::api::post_versioned(tenant, &path, body, false, ENTITLEMENT_VERSION).await
}

/// Everything the PDP needs, assembled. `subject` is already the wire form.
pub fn evaluate_body(
    application: &str,
    resources: &[String],
    subject: Option<Value>,
    environment: Option<Value>,
) -> Value {
    let mut body = json!({
        "application": application,
        "resources": resources,
    });
    let map = body.as_object_mut().expect("literal object");
    if let Some(subject) = subject {
        map.insert("subject".into(), subject);
    }
    if let Some(environment) = environment {
        map.insert("environment".into(), environment);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_paths_use_the_project_long_form() {
        assert_eq!(
            policies_path("bravo"),
            "/am/json/realms/root/realms/bravo/policies"
        );
        assert_eq!(
            sets_path("alpha"),
            "/am/json/realms/root/realms/alpha/applications"
        );
    }

    #[test]
    fn a_name_with_a_separator_is_refused_before_it_reaches_a_url() {
        for bad in ["a/b", "a?b", "a#b", ""] {
            assert!(validate_name("policy", bad).is_err(), "accepted {bad:?}");
        }
        assert!(validate_name("policy", "CapTokenDemo_OrdersRead").is_ok());
        // Dots are legal and the demo relies on them.
        assert!(validate_name("policy", "CapTokenDemoScope_orders.read").is_ok());
    }

    #[test]
    fn evaluate_body_omits_absent_subject_and_environment() {
        let body = evaluate_body("Set", &["https://x:443/a".into()], None, None);
        assert_eq!(
            body,
            json!({"application": "Set", "resources": ["https://x:443/a"]})
        );
    }

    #[test]
    fn evaluate_body_carries_subject_and_environment_when_given() {
        let body = evaluate_body(
            "Set",
            &["https://x:443/a".into()],
            Some(json!({"jwt": "abc"})),
            Some(json!({"scope": ["read"]})),
        );
        assert_eq!(body["subject"], json!({"jwt": "abc"}));
        assert_eq!(body["environment"], json!({"scope": ["read"]}));
    }
}
