//! ESV (Environment-Specific Variable) helpers shared by TUI and CLI.
//! All HTTP goes through `aic::api` → agent.

use crate::{Error, Result};

/// `GET /environment/variables` → returns the `result` array of variable
/// objects (see `docs/api/03-esvs.md` for the object shape). Pagination not
/// implemented; AIC's default page size is 1000 which is fine for "show me
/// the list."
pub async fn list_variables(tenant: &str) -> Result<Vec<serde_json::Value>> {
    let body = super::api::get(tenant, "/environment/variables").await?;
    match body.get("result") {
        Some(serde_json::Value::Array(arr)) => Ok(arr.clone()),
        _ => Err(Error::Api {
            status: 0,
            body: format!("unexpected /environment/variables response shape: {body}"),
        }),
    }
}
