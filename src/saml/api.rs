//! Verified HTTP wrappers for SAML 2.0 entity providers.
//! See `docs/api/06-saml.md`.
//!
//! Two transports, deliberately, because the endpoints differ in what they
//! authenticate with:
//!
//! - The `realm-config/saml2` JSON collection is an ordinary AM read and goes
//!   through the agent daemon like every other one.
//! - **`exportmetadata.jsp` takes no authentication at all** — verified by
//!   fetching it with and without a bearer and comparing the bodies byte for
//!   byte. It therefore does *not* go through the daemon: it needs no token
//!   cache, no prod gate and no unlock, and routing it through the daemon
//!   would mean teaching the wire protocol to carry a non-JSON body for a call
//!   that has nothing to gain from it. See [`export_metadata`].

use serde_json::Value;

use crate::config::tenant::Tenant;
use crate::saml::spec::{self, EntityStub, Location};
use crate::{Error, Result};

/// What the console sends. Optional on this family — omitting the header
/// entirely still returns 200 — but `resource=2.0` is a 404, so it is pinned
/// rather than left to the default.
const API_VERSION: &str = "protocol=2.1,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn entities_path(realm: &str) -> String {
    format!("{}/realm-config/saml2", realm_path(realm))
}

/// List every entity provider in the realm.
///
/// `?_queryFilter=true` works **only** on this parent collection: the same
/// query against `/hosted` or `/remote` is a `400 Query not supported`, so
/// filtering by location happens client-side ([`spec::select`]).
pub async fn list(tenant: &str, realm: &str) -> Result<Vec<EntityStub>> {
    let body = crate::aic::api::get_versioned(
        tenant,
        &format!("{}?_queryFilter=true", entities_path(realm)),
        API_VERSION,
    )
    .await?;
    let results = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected SAML entity list shape: {body}"),
        })?;
    results
        .iter()
        .map(|stub| {
            serde_json::from_value::<EntityStub>(stub.clone()).map_err(|error| Error::Api {
                status: 0,
                body: format!("unexpected SAML entity stub {stub}: {error}"),
            })
        })
        .collect()
}

/// Read one entity in full.
///
/// `location` must be right: the id is the same in both collections and the
/// wrong one answers 404, which reads as "no such entity". Callers that did
/// not get a location from the operator infer it with [`spec::locate`].
pub async fn read(tenant: &str, realm: &str, location: Location, entity_id: &str) -> Result<Value> {
    let path = format!(
        "{}/{}/{}",
        entities_path(realm),
        location.as_str(),
        spec::entity_id64(entity_id)
    );
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

/// Export an entity's standard metadata XML.
///
/// Takes the tenant **record**, not its name, and never touches the daemon:
/// the JSP is unauthenticated, so this works against a locked agent, which is
/// why `saml::cli::needs_tenant_auth` classifies the verb as needing none. The
/// transport is constructed with a null JWK because nothing on this path can
/// mint or attach a bearer — the same reason `jwtbearer::api::mint_user_token`
/// builds its own.
///
/// The returned body is **not** known to be metadata. A failed export is HTTP
/// 200 with a plain-text `ERROR : …` body; [`spec::classify_export`] is what
/// decides, and every caller must run it.
pub async fn export_metadata(tenant: &Tenant, realm: &str, entity_id: &str) -> Result<String> {
    crate::aic::AicClient::new(tenant.clone(), Value::Null)
        .get_text_unauthenticated(&spec::export_path(entity_id, realm))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_query_targets_the_parent_collection_only() {
        // `?_queryFilter=true` on `/hosted` or `/remote` is a 400, so the path
        // this builds must carry no location segment.
        let path = format!("{}?_queryFilter=true", entities_path("bravo"));
        assert_eq!(
            path,
            "/am/json/realms/root/realms/bravo/realm-config/saml2?_queryFilter=true"
        );
        assert!(!path.contains("/hosted"), "{path}");
        assert!(!path.contains("/remote"), "{path}");
    }

    #[test]
    fn the_read_path_uses_the_realm_convention_and_the_base64url_id() {
        let path = format!(
            "{}/{}/{}",
            entities_path("alpha"),
            Location::Remote.as_str(),
            spec::entity_id64("https://sp-a.example.com")
        );
        assert_eq!(
            path,
            "/am/json/realms/root/realms/alpha/realm-config/saml2/remote/\
             aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t"
        );
    }
}
