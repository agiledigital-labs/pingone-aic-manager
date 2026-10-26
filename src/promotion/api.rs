//! HTTP wrappers for `/environment/promotion/*`, all `GET`.

use serde_json::Value;

use crate::Result;
use crate::aic::api::get;

pub async fn lock_state(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/lock/state").await
}

/// The promotion status. A `GET` of the same path that `POST` starts a
/// promotion on; `/environment/promotion/status` is `501` (measured).
pub async fn status(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/promote").await
}

pub async fn reports(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/reports").await
}

/// The latest report, one by id, or the provisional one — generated on
/// request, with no lock.
pub async fn report(tenant: &str, which: &crate::promotion::spec::Which) -> Result<Value> {
    get(tenant, &which.path()).await
}
