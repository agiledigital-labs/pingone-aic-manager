//! HTTP wrappers for `/environment/promotion/*`, all `GET`.

use serde_json::Value;

use crate::aic::api::{get, get_versioned};
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
        Target::Unchecked(why) => Err(Error::Config(why.clone())),
    }
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
    let result = verify::complete(list).map_err(Error::Config)?;
    let hits: Vec<&Value> = result
        .iter()
        .filter(|v| v.get("name").and_then(Value::as_str) == Some(name))
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
