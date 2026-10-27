//! HTTP wrappers for `/environment/promotion/*`. The reads are plain `GET`s;
//! the writes go through [`send`], which requires the [`Permit`] only
//! `lifecycle::authorize` mints — so none can be reached without consent and
//! a state check against a read taken after it.

use serde_json::Value;

use crate::aic::api::{ApiCall, get, get_versioned};
use crate::promotion::lifecycle::{Op, Permit};
use crate::promotion::verify;
use crate::{Error, Result};

pub async fn lock_state(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/lock/state").await
}

/// The promotion status. A `GET` of the same path that `POST` starts a
/// promotion on; `/environment/promotion/status` is `501` (measured).
pub async fn status(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/promote").await
}

const LOCK: &str = "/environment/promotion/lock";
const PROMOTE: &str = "/environment/promotion/promote";

/// The path a permitted write goes to. `DELETE lock/{promotionId}` carries
/// the id segment-encoded.
fn path(permit: &Permit) -> String {
    match permit.op() {
        Op::Lock => LOCK.into(),
        Op::Promote { .. } => PROMOTE.into(),
        Op::Unlock { promotion_id } => format!("{LOCK}/{}", segment(promotion_id)),
    }
}

/// The request a permit authorizes:
///
/// - `POST lock` with no body of its own — measured with none sent (202
///   `result: locking`). The transport sends JSON `null` for a bodyless
///   `POST`, as it does for every `DELETE`; the first run through `aic`
///   measures that.
/// - `POST promote` with `{"dryRun": …}`. Measured with `true` (202,
///   "Promotion process initiated successfully") and with `false`, the real
///   run (accepted with the same message; 2026-10-27).
/// - `DELETE lock/{promotionId}` (202 `result: unlocking`, measured).
fn call<'a>(tenant: &'a str, path: &'a str, confirmed_prod: bool, permit: &Permit) -> ApiCall<'a> {
    let call = match permit.op() {
        Op::Lock => ApiCall::new(tenant, "POST", path),
        Op::Promote { dry_run } => {
            ApiCall::new(tenant, "POST", path).body(serde_json::json!({ "dryRun": dry_run }))
        }
        Op::Unlock { .. } => ApiCall::new(tenant, "DELETE", path),
    };
    call.confirmed_prod(confirmed_prod)
}

/// Send the one write `permit` authorizes. It is taken by value: one permit,
/// one request.
pub async fn send(tenant: &str, confirmed_prod: bool, permit: Permit) -> Result<Value> {
    let path = path(&permit);
    call(tenant, &path, confirmed_prod, &permit).send().await
}

pub async fn reports(tenant: &str) -> Result<Value> {
    get(tenant, "/environment/promotion/reports").await
}

/// The latest report, one by id, or the provisional one — generated on
/// request, with no lock.
pub async fn report(tenant: &str, which: &crate::promotion::spec::Which) -> Result<Value> {
    get(tenant, &which.path()).await
}

/// One report item's live document on one tenant, for `verify`. `Ok(None)`
/// is the tenant answering 404 for the item itself; every other failure —
/// including a journey node the tree names that cannot be read — is an
/// error, so an unread document is never judged equal to anything.
pub async fn fetch(tenant: &str, target: &verify::Target) -> Result<Option<Value>> {
    let doc = read(tenant, target).await?;
    if let Some(doc) = &doc {
        verify::check_document(target, doc).map_err(Error::Config)?;
    }
    Ok(doc)
}

async fn read(tenant: &str, target: &verify::Target) -> Result<Option<Value>> {
    use verify::Target;
    match target {
        Target::Journey { realm, name } => journey(tenant, realm, name).await,
        Target::Script { realm, id } => absent_on_404(
            get_versioned(
                tenant,
                &format!("{}/scripts/{}", realm_path(realm), segment(id)),
                "resource=1.1",
            )
            .await,
        ),
        Target::ScriptByName { realm, name } => {
            let list = get_versioned(
                tenant,
                &format!(
                    "{}/scripts?_queryFilter={}",
                    realm_path(realm),
                    segment(&verify::name_filter(name))
                ),
                "resource=1.1",
            )
            .await?;
            exact_name(&list, name)
        }
        Target::SecretMapping { realm, secret_id } => {
            absent_on_404(crate::secretmap::api::read_mapping(tenant, realm, secret_id).await)
        }
        Target::RealmService { realm, service } => {
            let list = get_versioned(
                tenant,
                &format!(
                    "{}/realm-config/services?_queryFilter=true",
                    realm_path(realm)
                ),
                AM_CONFIG_VERSION,
            )
            .await?;
            let Some(id) = verify::service_id(&list, service).map_err(Error::Config)? else {
                return Ok(None);
            };
            absent_on_404(
                get_versioned(
                    tenant,
                    &format!(
                        "{}/realm-config/services/{}",
                        realm_path(realm),
                        segment(&id)
                    ),
                    AM_CONFIG_VERSION,
                )
                .await,
            )
        }
        Target::AuthenticationSettings { realm } => absent_on_404(
            get_versioned(
                tenant,
                &format!("{}/realm-config/authentication", realm_path(realm)),
                AM_CONFIG_VERSION,
            )
            .await,
        ),
        Target::Cors { name } => {
            let list = get_versioned(
                tenant,
                "/am/json/global-config/services/CorsService/configuration?_queryFilter=true",
                AM_CONFIG_VERSION,
            )
            .await?;
            Ok(verify::one_by(&list, "_id", name)
                .map_err(Error::Config)?
                .cloned())
        }
        Target::Themes { realm } => {
            let doc = get(tenant, "/openidm/config/ui/themerealm").await?;
            Ok(verify::themes(&doc, realm).map_err(Error::Config)?.cloned())
        }
        Target::IdmEndpoint { name } => {
            endpoint(
                name,
                move |path: String| async move { get(tenant, &path).await },
            )
            .await
        }
        Target::Unchecked(why) => Err(Error::Config(why.clone())),
    }
}

/// The endpoint a report names, read through `get` (a path → document
/// reader, injected so a test drives this orchestration).
///
/// The config list resolves a present endpoint to its exact id. When the
/// list names none, absence is **not** taken from it — that the list is
/// complete is unmeasured, and a reported delete passes on two absences —
/// but from a direct read of the one id the name stands for
/// (`verify::endpoint_reading`): a 404 is absent, a 200 means the list
/// omitted it, and anything else is an error.
async fn endpoint<G, F>(name: &str, get: G) -> Result<Option<Value>>
where
    G: Fn(String) -> F,
    F: std::future::Future<Output = Result<Value>>,
{
    let list = get("/openidm/config".to_string()).await?;
    let Some(id) = verify::endpoint_id(&list, name).map_err(Error::Config)? else {
        let id = verify::endpoint_reading(name).map_err(Error::Config)?;
        return match absent_on_404(get(endpoint_path(&id)).await)? {
            None => Ok(None),
            Some(_) => Err(Error::Config(format!(
                "the config list omits {id:?}, which reads directly; \
                 the list cannot show absence"
            ))),
        };
    };
    let doc = absent_on_404(get(endpoint_path(&id)).await)?;
    if let Some(doc) = &doc {
        verify::check_endpoint(doc, &id).map_err(Error::Config)?;
    }
    Ok(doc)
}

/// `endpoint/<name>` → `/openidm/config/endpoint/<name>`, each segment
/// encoded and the `/`s kept as separators.
fn endpoint_path(id: &str) -> String {
    let segments: Vec<String> = id.split('/').map(segment).collect();
    format!("/openidm/config/{}", segments.join("/"))
}

/// The tree and every node it lists, page-node children included, keyed by
/// node id. An inner-tree node's *target* journey is not followed — it is a
/// separate report item. A node that cannot be read fails the whole journey:
/// the journey reader in `journey::api` warns and carries on, which would let
/// an unread node compare equal. Its paths also interpolate names raw, so
/// these are built here with every segment encoded.
async fn journey(tenant: &str, realm: &str, name: &str) -> Result<Option<Value>> {
    let trees = format!(
        "{}/realm-config/authentication/authenticationtrees",
        realm_path(realm)
    );
    let Some(tree) = absent_on_404(
        get_versioned(
            tenant,
            &format!("{trees}/trees/{}", segment(name)),
            JOURNEY_VERSION,
        )
        .await,
    )?
    else {
        return Ok(None);
    };
    let mut nodes = serde_json::Map::new();
    let mut pending = verify::node_refs(verify::Holder::Tree, "", &tree).map_err(Error::Config)?;
    while let Some((node_type, id)) = pending.pop() {
        if nodes.contains_key(&id) {
            continue;
        }
        let node = get_versioned(
            tenant,
            &format!("{trees}/nodes/{}/{}", segment(&node_type), segment(&id)),
            JOURNEY_VERSION,
        )
        .await?;
        pending.extend(
            verify::node_refs(verify::Holder::Node, &node_type, &node).map_err(Error::Config)?,
        );
        nodes.insert(id, node);
    }
    Ok(Some(serde_json::json!({ "tree": tree, "nodes": nodes })))
}

/// The journey family's version (`docs/api/09-journeys.md`).
const JOURNEY_VERSION: &str = "protocol=2.0,resource=1.0";

/// Measured for the session, validation, OAuth2 provider, authentication and
/// CORS documents on 2026-10-26.
const AM_CONFIG_VERSION: &str = "protocol=1.0,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{}", segment(realm))
}

/// One path segment, percent-encoded (a space is `%20`, never `+`).
fn segment(raw: &str) -> String {
    url::form_urlencoded::byte_serialize(raw.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn absent_on_404(result: Result<Value>) -> Result<Option<Value>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::Api { status: 404, .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The query result's one entry named exactly `name` — case **matters** for a
/// script name, so `one_by`'s case-folding is not used, and a query that
/// matched several is an error rather than a pick.
fn exact_name(list: &Value, name: &str) -> Result<Option<Value>> {
    let hits: Vec<&Value> = verify::rows(list, "name")
        .map_err(Error::Config)?
        .into_iter()
        .filter(|(got, _)| *got == name)
        .map(|(_, row)| row)
        .collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        _ => Err(Error::Config(format!(
            "{} scripts are named {name:?}",
            hits.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::{ApiCallRequest, Request};
    use crate::promotion::lifecycle;
    use crate::promotion::spec::assess;

    #[test]
    fn endpoint_paths_keep_separators_and_encode_segments() {
        assert_eq!(
            endpoint_path("endpoint/aic-promotion-probe"),
            "/openidm/config/endpoint/aic-promotion-probe"
        );
        assert_eq!(
            endpoint_path("endpoint/a b/c"),
            "/openidm/config/endpoint/a%20b/c"
        );
    }

    /// Drives `endpoint` with scripted responses, returning its result and
    /// every path it asked for. A path not scripted answers 404.
    fn endpoint_with(
        name: &str,
        script: &[(&str, std::result::Result<Value, u16>)],
    ) -> (Result<Option<Value>>, Vec<String>) {
        let asked = std::cell::RefCell::new(Vec::new());
        let reader = |path: String| {
            asked.borrow_mut().push(path.clone());
            let answer = match script.iter().find(|(p, _)| *p == path) {
                Some((_, Ok(doc))) => Ok(doc.clone()),
                Some((_, Err(status))) => Err(Error::Api {
                    status: *status,
                    body: String::new(),
                }),
                None => Err(Error::Api {
                    status: 404,
                    body: String::new(),
                }),
            };
            async move { answer }
        };
        let result = futures::executor::block_on(endpoint(name, reader));
        (result, asked.into_inner())
    }

    #[test]
    fn an_endpoint_absent_from_the_list_is_read_directly() {
        const LIST: &str = "/openidm/config";
        const READ: &str = "/openidm/config/endpoint/a-b";
        let ep = |id: &str| json!({"_id": id, "factoryPid": "endpoint"});
        let empty = json!({"_id": "", "configurations": []});
        let doc = json!({"_id": "endpoint/a-b", "source": "x", "type": "text/javascript"});

        // The direct read 404s: absent, and it was asked for.
        let (got, asked) = endpoint_with("endpoint-a-b", &[(LIST, Ok(empty.clone()))]);
        assert!(matches!(got, Ok(None)));
        assert_eq!(asked, [LIST, READ]);

        // It reads though the list omits it: the list cannot show absence.
        let (got, _) = endpoint_with(
            "endpoint-a-b",
            &[(LIST, Ok(empty.clone())), (READ, Ok(doc.clone()))],
        );
        assert!(got.is_err());

        // A read that errors is not a 404.
        let (got, _) = endpoint_with(
            "endpoint-a-b",
            &[(LIST, Ok(empty.clone())), (READ, Err(403))],
        );
        assert!(got.is_err());

        // A name of another form is refused before any read.
        let (got, asked) = endpoint_with("probe", &[(LIST, Ok(empty.clone()))]);
        assert!(got.is_err());
        assert_eq!(asked, [LIST]);

        // Listed: read by its exact id, and checked.
        let listed = json!({"_id": "", "configurations": [ep("endpoint/a-b")]});
        let (got, asked) = endpoint_with(
            "endpoint-a-b",
            &[(LIST, Ok(listed.clone())), (READ, Ok(doc.clone()))],
        );
        assert_eq!(got.unwrap(), Some(doc));
        assert_eq!(asked, [LIST, READ]);
        let wrong = json!({"_id": "endpoint/A-B", "source": "x", "type": "t"});
        let (got, _) = endpoint_with("endpoint-a-b", &[(LIST, Ok(listed)), (READ, Ok(wrong))]);
        assert!(got.is_err());
    }

    fn wire(call: ApiCall<'_>) -> ApiCallRequest {
        let Request::ApiCall(req) = call.envelope() else {
            panic!("expected an API call");
        };
        req
    }

    fn state(locked: bool) -> crate::promotion::spec::Assessment {
        let s = if locked { "locked" } else { "unlocked" };
        let side = json!({"state": s, "proxyState": s});
        assess(
            &json!({"result": s, "lowerEnv": side, "upperEnv": side}),
            &if locked {
                json!({"status": "READY", "globalLock": "LOCKED", "blockingError": false,
                       "promotionId": "a b/c"})
            } else {
                json!({"status": "UNLOCKED", "globalLock": "UNLOCKED", "blockingError": false})
            },
        )
        .unwrap()
    }

    fn permit(verb: lifecycle::Verb, locked: bool) -> Permit {
        let a = state(locked);
        let approved = lifecycle::check(&verb, &a, "uat").unwrap();
        lifecycle::authorize(&verb, &approved, &a, "uat", true).unwrap()
    }

    fn wire_of(permit: &Permit, confirmed_prod: bool) -> ApiCallRequest {
        let path = path(permit);
        wire(call("uat", &path, confirmed_prod, permit))
    }

    #[test]
    fn lock_posts_with_no_body_of_its_own() {
        let req = wire_of(&permit(lifecycle::Verb::Lock, false), false);
        assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", LOCK));
        assert_eq!(req.body, None);
        assert!(!req.confirmed_prod);
    }

    #[test]
    fn promote_sends_the_kind_the_permit_was_minted_for() {
        for dry_run in [true, false] {
            let req = wire_of(&permit(lifecycle::Verb::Promote { dry_run }, true), true);
            assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", PROMOTE));
            assert_eq!(req.body, Some(json!({ "dryRun": dry_run })));
            assert!(req.confirmed_prod);
        }
    }

    #[test]
    fn unlock_deletes_the_reported_id_encoded() {
        let req = wire_of(&permit(lifecycle::Verb::Unlock, true), false);
        assert_eq!(
            (req.method.as_str(), req.path.as_str()),
            ("DELETE", "/environment/promotion/lock/a%20b%2Fc")
        );
        assert_eq!(req.body, None);
    }
}
