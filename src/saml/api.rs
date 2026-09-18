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
//!
//! Every daemon-routed call in this module is assembled by a [`SamlRequest`]
//! builder and sent by [`SamlRequest::send`]. The split is what makes the
//! envelope testable: a test that rebuilt an expected path out of the same
//! helpers the production function uses would stay green with the production
//! function broken, which is exactly what the tests here used to do.

use serde_json::Value;

use crate::aic::api::ApiCall;
use crate::config::tenant::Tenant;
use crate::saml::spec::{self, EntityStub, Location};
use crate::{Error, Result};

/// What the console sends. Optional on this family — omitting the header
/// entirely still returns 200 — but `resource=2.0` is a 404, so it is pinned
/// rather than left to the default.
const API_VERSION: &str = "protocol=2.1,resource=1.0";

/// One assembled SAML request, path and all.
///
/// Owning the path is the point: a builder that took one would push the
/// interesting half — which collection, which id encoding — back out to the
/// call site, where only a test that restates it can reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SamlRequest {
    method: &'static str,
    path: String,
    body: Option<Value>,
    /// Only ever true for the two write verbs, and only when the operator
    /// passed `--yes` for a production-themed tenant.
    confirmed_prod: bool,
}

impl SamlRequest {
    fn get(path: String) -> Self {
        Self {
            method: "GET",
            path,
            body: None,
            confirmed_prod: false,
        }
    }

    fn call<'a>(&'a self, tenant: &'a str) -> ApiCall<'a> {
        let mut call = ApiCall::new(tenant, self.method, &self.path)
            .api_version(API_VERSION)
            .confirmed_prod(self.confirmed_prod);
        if let Some(body) = &self.body {
            call = call.body(body.clone());
        }
        call
    }

    async fn send(&self, tenant: &str) -> Result<Value> {
        self.call(tenant).send().await
    }
}

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn entities_path(realm: &str) -> String {
    format!("{}/realm-config/saml2", realm_path(realm))
}

/// The circle-of-trust collection.
///
/// A sibling of the entity collection under `realm-config`, and the two do not
/// behave alike — the CoT id is the **plain name**, the list returns full
/// documents rather than stubs, and a CoT `PUT` merges and creates where an
/// entity `PUT` replaces and 404s. One helper cannot serve both
/// (`docs/api/06-saml.md`).
fn cots_path(realm: &str) -> String {
    format!(
        "{}/realm-config/federation/circlesoftrust",
        realm_path(realm)
    )
}

/// `?_queryFilter=true` works **only** on the parent collection: the same
/// query against `/hosted` or `/remote` is a `400 Query not supported`, so
/// filtering by location happens client-side ([`spec::select`]).
fn list_request(realm: &str) -> SamlRequest {
    SamlRequest::get(format!("{}?_queryFilter=true", entities_path(realm)))
}

/// `location` must be right: the id is the same in both collections and the
/// wrong one answers 404.
fn read_request(realm: &str, location: Location, entity_id: &str) -> SamlRequest {
    SamlRequest::get(entity_path(realm, location, entity_id))
}

fn entity_path(realm: &str, location: Location, entity_id: &str) -> String {
    format!(
        "{}/{}/{}",
        entities_path(realm),
        location.as_str(),
        spec::entity_id64(entity_id)
    )
}

fn list_cots_request(realm: &str) -> SamlRequest {
    SamlRequest::get(format!("{}?_queryFilter=true", cots_path(realm)))
}

fn read_cot_request(realm: &str, name: &str) -> Result<SamlRequest> {
    Ok(SamlRequest::get(format!(
        "{}/{}",
        cots_path(realm),
        spec::validate_cot_name(name)?
    )))
}

/// `_action=create` exists only on `/hosted`; on `/remote` it is a 400, so the
/// path is not parameterised by location.
///
/// The trailing slash before `?_action=create` is what AM's own console sends
/// and what every measurement in `docs/api/06-saml.md` used.
fn create_hosted_request(realm: &str, body: Value, confirmed_prod: bool) -> SamlRequest {
    SamlRequest {
        method: "POST",
        path: format!("{}/hosted/?_action=create", entities_path(realm)),
        body: Some(body),
        confirmed_prod,
    }
}

/// `?_action=importEntity` exists only on `/remote`; on `/hosted` it is a
/// **501 `importEntity not supported`**, so the path is not parameterised by
/// location either. Same trailing slash as the create path, for the same
/// reason: it is what every measurement in `docs/api/06-saml.md` used.
fn import_request(realm: &str, body: Value, confirmed_prod: bool) -> SamlRequest {
    SamlRequest {
        method: "POST",
        path: format!("{}/remote/?_action=importEntity", entities_path(realm)),
        body: Some(body),
        confirmed_prod,
    }
}

/// `PUT …/saml2/{location}/{entityId64}` — a **full replace**.
///
/// There is no create-by-`PUT` (an unknown id is a 404) and no `If-Match`: a
/// stale header and a bogus `_rev` in the body were both accepted with 200.
/// So the body has to be a whole document that was just read, and the caller
/// has to read it back (`docs/api/06-saml.md`).
fn update_entity_request(
    realm: &str,
    location: Location,
    entity_id: &str,
    body: Value,
    confirmed_prod: bool,
) -> SamlRequest {
    SamlRequest {
        method: "PUT",
        path: entity_path(realm, location, entity_id),
        body: Some(body),
        confirmed_prod,
    }
}

fn delete_entity_request(
    realm: &str,
    location: Location,
    entity_id: &str,
    confirmed_prod: bool,
) -> SamlRequest {
    SamlRequest {
        method: "DELETE",
        path: entity_path(realm, location, entity_id),
        body: None,
        confirmed_prod,
    }
}

/// List every entity provider in the realm.
pub async fn list(tenant: &str, realm: &str) -> Result<Vec<EntityStub>> {
    let body = list_request(realm).send(tenant).await?;
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
/// Callers that did not get a location from the operator infer it with
/// [`spec::locate`], because the wrong collection answers 404 and that reads
/// as "no such entity".
pub async fn read(tenant: &str, realm: &str, location: Location, entity_id: &str) -> Result<Value> {
    read_request(realm, location, entity_id).send(tenant).await
}

/// List the realm's circles of trust, as raw documents.
///
/// Raw on purpose: the list endpoint returns the whole document, and the CLI's
/// `--json` output should be what the tenant said rather than what our struct
/// can hold. [`spec::cots`] parses them for the table.
pub async fn list_cots(tenant: &str, realm: &str) -> Result<Vec<Value>> {
    let body = list_cots_request(realm).send(tenant).await?;
    body.get("result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected circle-of-trust list shape: {body}"),
        })
}

/// Read one circle of trust. `name` is the plain id, **not** base64url.
pub async fn read_cot(tenant: &str, realm: &str, name: &str) -> Result<Value> {
    read_cot_request(realm, name)?.send(tenant).await
}

/// Create a hosted entity provider.
///
/// **201**, and the body that comes back is a stub (`_id`, `_rev`,
/// `entityId`) rather than the created document — so it must not be snapshotted
/// or summarised. Callers that want the entity read it afterwards.
pub async fn create_hosted(
    tenant: &str,
    realm: &str,
    body: Value,
    confirmed_prod: bool,
) -> Result<Value> {
    create_hosted_request(realm, body, confirmed_prod)
        .send(tenant)
        .await
}

/// Delete an entity provider.
///
/// **This silently rewrites every circle of trust that listed the entity.**
/// Verified 2026-09-16: a CoT holding the deleted entity came back with
/// `trustedProviders: []` with no CoT write of ours in between. The cascade is
/// invisible from here, which is why `cli::delete` reads the CoT collection
/// first, prints what will change, and reads it back afterwards.
///
/// The `permit` carries the same compile-time routing proof as
/// [`import_entity`]'s: [`crate::saml::spec::DeletePermit`] can only be minted
/// by `spec::delete_ok`, so the unforced preview — which is this command's
/// whole dry-run story — cannot reach the write.
pub async fn delete_entity(
    tenant: &str,
    realm: &str,
    location: Location,
    entity_id: &str,
    confirmed_prod: bool,
    _permit: &crate::saml::spec::DeletePermit,
) -> Result<Value> {
    delete_entity_request(realm, location, entity_id, confirmed_prod)
        .send(tenant)
        .await
}

/// Replace an entity provider's whole document.
///
/// **This is a full replace with no optimistic concurrency.** `{"entityId":
/// "<same id>"}` answers 200 and silently deletes the entire role block,
/// `metaAlias` and all, leaving a roleless shell whose metadata export
/// collapses to `<EntityDescriptor/>`. So the body must be a document this
/// caller read and then changed, never one it assembled, and the caller must
/// read the entity back afterwards: a 200 here is not evidence of anything.
///
/// The `permit` carries the same compile-time routing proof as
/// [`import_entity`]'s: [`crate::saml::rotate::spec::InitPermit`] can only be
/// minted by `rotate::spec::authorize_init`, so a `--dry-run` cannot reach
/// this call. `rotate::ops::set_identifier` is the only caller, and it owns
/// the read-modify-write-verify sequence.
pub async fn update_entity(
    tenant: &str,
    realm: &str,
    location: Location,
    entity_id: &str,
    body: Value,
    confirmed_prod: bool,
    _permit: &crate::saml::rotate::spec::InitPermit,
) -> Result<Value> {
    update_entity_request(realm, location, entity_id, body, confirmed_prod)
        .send(tenant)
        .await
}

/// Import remote entity metadata.
///
/// **200, not 201**, and the body is `{"importedEntities": [...]}` — which
/// may name more than one entity, because an `EntitiesDescriptor` aggregate
/// imports every entity it contains in this one call. The caller must compare
/// that list against the ids it parsed; see [`spec::compare_imported`].
///
/// The `permit` is not decoration. [`spec::ImportPermit`] can only be minted
/// by [`spec::authorize_import`], so this function is unreachable from a
/// `--dry-run` or from a path that skipped the preflight — the same
/// compile-time routing proof `scripts::gate`'s `WritePermit` gives script
/// writes.
pub async fn import_entity(
    tenant: &str,
    realm: &str,
    body: Value,
    confirmed_prod: bool,
    _permit: &spec::ImportPermit,
) -> Result<Value> {
    import_request(realm, body, confirmed_prod)
        .send(tenant)
        .await
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
    use serde_json::json;

    use super::*;
    use crate::agent::{ApiCallRequest, Request};

    /// The envelope a request actually sends, so every assertion below is on
    /// the wire shape rather than on a path a test reassembled.
    fn envelope(request: &SamlRequest) -> ApiCallRequest {
        let Request::ApiCall(call) = request.call("sandbox").envelope() else {
            panic!("expected an API call");
        };
        call
    }

    #[test]
    fn the_list_query_targets_the_parent_collection_only() {
        // `?_queryFilter=true` on `/hosted` or `/remote` is a 400, so the path
        // the production builder produces must carry no location segment.
        let call = envelope(&list_request("bravo"));
        assert_eq!(call.method, "GET");
        assert_eq!(
            call.path,
            "/am/json/realms/root/realms/bravo/realm-config/saml2?_queryFilter=true"
        );
        assert!(!call.path.contains("/hosted"), "{}", call.path);
        assert!(!call.path.contains("/remote"), "{}", call.path);
        assert_eq!(call.api_version.as_deref(), Some(API_VERSION));
        assert!(call.body.is_none());
    }

    #[test]
    fn the_read_path_uses_the_realm_convention_and_the_base64url_id() {
        let call = envelope(&read_request(
            "alpha",
            Location::Remote,
            "https://sp-a.example.com",
        ));
        assert_eq!(call.method, "GET");
        assert_eq!(
            call.path,
            "/am/json/realms/root/realms/alpha/realm-config/saml2/remote/\
             aHR0cHM6Ly9zcC1hLmV4YW1wbGUuY29t"
        );
    }

    /// The two families sit side by side under `realm-config` and encode their
    /// ids differently. Asserting them together is the point: a shared "encode
    /// the id" helper would make one of these wrong.
    #[test]
    fn the_cot_path_uses_the_plain_name_and_the_entity_path_does_not() {
        let cot = envelope(&read_cot_request("bravo", "client-b").expect("a plain name"));
        assert_eq!(
            cot.path,
            "/am/json/realms/root/realms/bravo/realm-config/federation/circlesoftrust/client-b"
        );

        let entity = envelope(&read_request("bravo", Location::Hosted, "client-b"));
        assert_eq!(
            entity.path,
            "/am/json/realms/root/realms/bravo/realm-config/saml2/hosted/Y2xpZW50LWI"
        );

        assert_eq!(
            envelope(&list_cots_request("bravo")).path,
            "/am/json/realms/root/realms/bravo/realm-config/federation/circlesoftrust\
             ?_queryFilter=true"
        );
    }

    /// A CoT name reaches the path unencoded, so the read builder — not only
    /// `spec::validate_cot_name` — has to refuse one that would address
    /// something else.
    #[test]
    fn a_cot_read_refuses_a_name_that_would_escape_the_collection() {
        assert!(read_cot_request("bravo", "../../saml2/hosted").is_err());
        assert!(read_cot_request("bravo", "  ").is_err());
    }

    /// `_action=create` on `/remote` is a 400 `Create not supported`, so the
    /// create path must never be parameterised by location — and the body the
    /// caller built has to arrive unaltered.
    #[test]
    fn the_create_request_targets_hosted_only_and_carries_the_body() {
        let body = json!({
            "entityId": "https://sp-b.example.com",
            "serviceProvider": { "services": { "metaAlias": "/bravo/client-b-sp" } }
        });
        let call = envelope(&create_hosted_request("bravo", body.clone(), false));
        assert_eq!(call.method, "POST");
        assert_eq!(
            call.path,
            "/am/json/realms/root/realms/bravo/realm-config/saml2/hosted/?_action=create"
        );
        assert!(!call.path.contains("/remote"), "{}", call.path);
        assert_eq!(call.body.as_ref(), Some(&body));
    }

    /// `?_action=importEntity` is a 501 on `/hosted`, and the one field in
    /// the body is `standardMetadata` — `{}` and a wrong base64 alphabet both
    /// answer with the same 400, so nothing in a response would tell us this
    /// path was wrong.
    #[test]
    fn the_import_request_targets_remote_only_and_carries_the_body() {
        let body = json!({ "standardMetadata": "PD94bWw" });
        let call = envelope(&import_request("bravo", body.clone(), false));
        assert_eq!(call.method, "POST");
        assert_eq!(
            call.path,
            "/am/json/realms/root/realms/bravo/realm-config/saml2/remote/?_action=importEntity"
        );
        assert!(!call.path.contains("/hosted"), "{}", call.path);
        assert_eq!(call.body.as_ref(), Some(&body));
        assert_eq!(call.api_version.as_deref(), Some(API_VERSION));
    }

    /// Both writes carry the operator's production consent, and neither
    /// invents it. `confirmed_prod` defaulting to `true` would silently lift
    /// the daemon's prod gate for every SAML write.
    #[test]
    fn both_writes_forward_the_callers_production_consent_and_nothing_else_does() {
        for confirmed in [false, true] {
            assert_eq!(
                envelope(&create_hosted_request("bravo", json!({}), confirmed)).confirmed_prod,
                confirmed
            );
            assert_eq!(
                envelope(&import_request("bravo", json!({}), confirmed)).confirmed_prod,
                confirmed
            );
            assert_eq!(
                envelope(&delete_entity_request(
                    "bravo",
                    Location::Hosted,
                    "https://sp-b.example.com",
                    confirmed,
                ))
                .confirmed_prod,
                confirmed
            );
        }

        for read in [
            list_request("bravo"),
            read_request("bravo", Location::Hosted, "https://sp-b.example.com"),
            list_cots_request("bravo"),
            read_cot_request("bravo", "client-b").expect("a plain name"),
        ] {
            assert!(
                !envelope(&read).confirmed_prod,
                "a read must not claim production consent: {}",
                read.path
            );
        }
    }

    /// The delete targets the same document `read` would have shown, in the
    /// collection the caller resolved — the wrong one is a 404 that reads as
    /// "already gone".
    #[test]
    fn the_delete_request_addresses_the_resolved_collection() {
        let hosted = envelope(&delete_entity_request(
            "bravo",
            Location::Hosted,
            "https://sp-b.example.com",
            true,
        ));
        assert_eq!(hosted.method, "DELETE");
        assert_eq!(
            hosted.path,
            envelope(&read_request(
                "bravo",
                Location::Hosted,
                "https://sp-b.example.com"
            ))
            .path,
            "delete and show must address the same resource"
        );
        assert!(hosted.body.is_none(), "a DELETE sends no body");

        let remote = envelope(&delete_entity_request(
            "bravo",
            Location::Remote,
            "https://sp-b.example.com",
            true,
        ));
        assert_ne!(
            hosted.path, remote.path,
            "--location chooses the collection, so it must reach the path"
        );
        assert!(remote.path.contains("/remote/"), "{}", remote.path);
    }
}
