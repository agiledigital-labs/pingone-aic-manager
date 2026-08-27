//! AM-script specifics for the script-sync core. The **only** place AM-vs-IDM
//! differences for AM live: realm-scoped `/am/json…/scripts`, the
//! `protocol=2.0,resource=1.0` header, base64 `script` body, and context→dir
//! routing. See `docs/api/04-scripts.md`.

use super::{Kind, NewScriptOpts, RemoteRef, RemoteScript};
use crate::{Error, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::Value;
use std::path::PathBuf;

/// AM scripts require the protocol-versioned header (the client default of
/// `resource=1.0` 400s on the scripts endpoint).
const API_VERSION: &str = "protocol=2.0,resource=1.0";

/// List the live AM scripting contexts so create accepts new contexts without
/// a client-side allow-list. Each result element is a full context object
/// (`{_id, isHidden, languages, defaultScript, …}`) — the name is its `_id`, not
/// the element itself. `isHidden` contexts (`NODE_DESIGNER`) are internal and
/// aren't offered. Verified 2026-07-30: 40 contexts, one hidden.
pub async fn list_contexts(tenant: &str) -> Result<Vec<String>> {
    let body = crate::aic::api::get_versioned(
        tenant,
        "/am/json/global-config/services/scripting/contexts?_queryFilter=true",
        API_VERSION,
    )
    .await?;
    parse_contexts(&body)
}

fn parse_contexts(body: &Value) -> Result<Vec<String>> {
    let contexts = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected scripting contexts shape: {body}"),
        })?
        .iter()
        .filter(|context| context.get("isHidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|context| context.get("_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if contexts.is_empty() {
        return Err(Error::Api {
            status: 0,
            body: format!("no usable scripting contexts in response: {body}"),
        });
    }
    Ok(contexts)
}

/// Resolve a context constant or workspace slug using the tenant's live list.
/// Returns the AM context plus an `evaluatorVersion` the slug forces, if any.
pub fn resolve_context(input: &str, contexts: &[String]) -> Result<(String, Option<String>)> {
    if let Some(context) = contexts
        .iter()
        .find(|context| context.eq_ignore_ascii_case(input))
    {
        return Ok((context.clone(), None));
    }
    // The two scripted-decision contexts are aliases of one another (AM stores
    // either as `AUTHENTICATION_TREE_DECISION_NODE`) and `slug_for` maps both to
    // `decision-node`, so the generic slug match below would call it ambiguous.
    // Resolve it here instead, pinning the next-gen engine.
    if input.eq_ignore_ascii_case("decision-node") {
        return contexts
            .iter()
            .find(|context| {
                matches!(
                    context.as_str(),
                    "SCRIPTED_DECISION_NODE" | "AUTHENTICATION_TREE_DECISION_NODE"
                )
            })
            .cloned()
            .map(|context| (context, Some("2.0".into())))
            .ok_or_else(|| unknown_context(input, contexts));
    }
    if input.eq_ignore_ascii_case("decision-node-legacy") {
        return Err(Error::Config(
            "the legacy scripted-decision engine (evaluatorVersion 1.0) is deprecated — new scripts must be next-gen; use --context decision-node".into(),
        ));
    }
    let matches: Vec<_> = contexts
        .iter()
        .filter(|context| slug_for(Some(context), None).eq_ignore_ascii_case(input))
        .collect();
    match matches.as_slice() {
        [context] => Ok(((*context).clone(), None)),
        [] => Err(unknown_context(input, contexts)),
        _ => Err(Error::Config(format!(
            "context slug {input:?} is ambiguous: {}",
            matches
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn unknown_context(input: &str, contexts: &[String]) -> Error {
    let mut slugs: Vec<_> = contexts.iter().map(|c| slug_for(Some(c), None)).collect();
    slugs.sort();
    slugs.dedup();
    Error::Config(format!(
        "unknown AM context {input:?}; available slugs: {}",
        slugs.join(", ")
    ))
}

pub fn id_for_new(_name: &str) -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn new_script(name: &str, source: &[u8], opts: &NewScriptOpts) -> Result<RemoteScript> {
    let context = opts
        .context
        .clone()
        .ok_or_else(|| Error::Config("AM script create requires --context".into()))?;
    let id = id_for_new(name);
    let language = opts.language.clone().unwrap_or_else(|| "JAVASCRIPT".into());
    // Always sent, never omitted: AM's own create default is "1.0" (verified
    // 2026-07-31), so leaving the field off would quietly produce legacy-engine
    // scripts — the exact thing the check below refuses.
    let evaluator_version = opts
        .evaluator_version
        .clone()
        .unwrap_or_else(|| "2.0".into());
    // Legacy-engine scripts get no new instances: the v1 bindings are deprecated
    // and the workspace's type definitions target next-gen. Existing legacy
    // scripts stay pullable, pushable, and copyable — this only blocks creation.
    if evaluator_version == "1.0" {
        return Err(Error::Config(
            "refusing to create an evaluatorVersion 1.0 (legacy engine) script — it is deprecated; omit --evaluator-version to get next-gen".into(),
        ));
    }
    let raw_config = serde_json::json!({
        "_id": id,
        "name": name,
        "context": context,
        "language": language,
        "script": B64.encode(source),
        "description": opts.description,
        "default": false,
        "evaluatorVersion": evaluator_version,
    });
    Ok(RemoteScript {
        reference: ref_from_config(&raw_config),
        raw_config,
    })
}

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn ref_from_config(raw: &Value) -> RemoteRef {
    RemoteRef {
        kind: Kind::Am,
        id: str_field(raw, "_id"),
        name: str_field(raw, "name"),
        context: raw
            .get("context")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        is_default: raw
            .get("default")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        evaluator_version: raw
            .get("evaluatorVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

/// AIC dropped Groovy support; this old tenant still has many Groovy scripts.
/// We draw the line here: the tool only handles JavaScript. This is the single
/// chokepoint — filtering in `list` keeps Groovy out of list/pull/push/status
/// and the TUI alike. Lift this filter if Groovy is ever supported again.
fn is_javascript(raw: &Value) -> bool {
    raw.get("language").and_then(|v| v.as_str()) != Some("GROOVY")
}

/// Product-internal scripts AIC ships and protects: `fetch`-by-id 403s
/// ("not available in PingOne Advanced Identity Cloud"), so they can't be
/// pulled anyway. No API field marks them as internal (checked every field —
/// `default`, `createdBy`/`lastModifiedBy`, `creationDate` all overlap normal
/// scripts); the only reliable signal is the `"ForgeRock Internal:"` name
/// prefix. Hide them so they don't clutter the list with un-pullable rows.
fn is_internal(raw: &Value) -> bool {
    raw.get("name")
        .and_then(|v| v.as_str())
        .is_some_and(|n| n.starts_with("ForgeRock Internal:"))
}

/// Whether a listed AM script is one the tool syncs (JavaScript, not internal).
fn is_syncable(raw: &Value) -> bool {
    is_javascript(raw) && !is_internal(raw)
}

fn str_field(raw: &Value, key: &str) -> String {
    raw.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

pub async fn list(tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
    // The scripts endpoint paginates but returns a *null* `pagedResultsCookie`
    // (verified 2026-06-01), so cookie paging silently caps at `_pageSize`.
    // Page by offset instead and stop when the server reports none remaining.
    // A large page keeps it to a single request for typical realms.
    const PAGE: usize = 1000;
    let mut refs = Vec::new();
    let mut offset = 0usize;
    loop {
        let path = format!(
            "{}/scripts?_queryFilter=true&_pageSize={PAGE}&_pagedResultsOffset={offset}",
            realm_path(realm)
        );
        let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
        let arr = body
            .get("result")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Api {
                status: 0,
                body: format!("unexpected scripts list shape: {body}"),
            })?;
        let n = arr.len();
        // `n` (the server's page size) drives paging; only syncable scripts
        // make it into `refs` — Groovy and product-internal ones are dropped.
        refs.extend(arr.iter().filter(|el| is_syncable(el)).map(ref_from_config));
        // `remainingPagedResults` is authoritative here; `-1` (unknown) falls
        // back to "stop once a page comes back empty".
        let remaining = body
            .get("remainingPagedResults")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if n == 0 || remaining == 0 {
            break;
        }
        offset += n;
    }
    Ok(refs)
}

pub async fn fetch(tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
    let path = format!("{}/scripts/{id}", realm_path(realm));
    let raw = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
    Ok(RemoteScript {
        reference: ref_from_config(&raw),
        raw_config: raw,
    })
}

pub async fn write(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
) -> Result<Value> {
    let path = format!("{}/scripts/{}", realm_path(realm), script.reference.id);
    crate::aic::api::put_versioned(
        tenant,
        &path,
        script.raw_config.clone(),
        confirmed_prod,
        API_VERSION,
    )
    .await
}

pub async fn delete(tenant: &str, realm: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{}/scripts/{id}", realm_path(realm));
    crate::aic::api::delete_versioned(tenant, &path, confirmed_prod, API_VERSION).await
}

/// AM `script` is base64 on the wire. Some legacy scripts store it as a JSON
/// array of lines; handle both, preferring the string form the live API uses.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    match raw.get("script") {
        Some(Value::String(s)) => B64
            .decode(s.trim())
            .map_err(|e| Error::Config(format!("decode AM script base64: {e}"))),
        Some(Value::Array(lines)) => {
            let joined: Vec<String> = lines
                .iter()
                .map(|l| l.as_str().unwrap_or_default().to_string())
                .collect();
            Ok(format!("{}\n", joined.join("\n")).into_bytes())
        }
        _ => Err(Error::Config("AM script has no `script` field".into())),
    }
}

pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let obj = raw
        .as_object_mut()
        .ok_or_else(|| Error::Config("AM raw config is not an object".into()))?;
    obj.insert("script".into(), Value::String(B64.encode(source)));
    Ok(())
}

/// Folder slug for an AM script: a short, readable name derived from its
/// `context`. Each script type gets its own folder so per-type TypeScript
/// definitions can apply. Two special rules:
///
/// - the scripted-decision-node context carries both engine generations, so it
///   splits by `evaluatorVersion` (`1.0` → `-legacy`); next-gen is the bare
///   `decision-node` since the current types target it.
/// - any `…_NEXT_GEN` / `…_NEXTGEN` context → its base slug + `-ng`.
///
/// Unknown contexts fall back to a lowercased, hyphenated form so a new Ping
/// context still lands somewhere sensible.
fn slug_for(context: Option<&str>, evaluator_version: Option<&str>) -> String {
    let Some(ctx) = context else {
        return "unknown".to_string();
    };
    if matches!(
        ctx,
        "AUTHENTICATION_TREE_DECISION_NODE" | "SCRIPTED_DECISION_NODE"
    ) {
        return if evaluator_version == Some("1.0") {
            "decision-node-legacy".to_string()
        } else {
            "decision-node".to_string()
        };
    }
    let (base, ng) = match ctx
        .strip_suffix("_NEXT_GEN")
        .or_else(|| ctx.strip_suffix("_NEXTGEN"))
    {
        Some(base) => (base, true),
        None => (ctx, false),
    };
    let slug = base_slug(base);
    if ng { format!("{slug}-ng") } else { slug }
}

/// Curated short slug per context base (next-gen suffix already stripped).
fn base_slug(base: &str) -> String {
    let mapped = match base {
        "AUTHENTICATION_SERVER_SIDE" => "auth-server",
        "AUTHENTICATION_CLIENT_SIDE" => "auth-client",
        "CONFIG_PROVIDER_NODE" => "config-provider",
        "DEVICE_MATCH_NODE" => "device-match",
        "LIBRARY" => "lib",
        "OIDC_CLAIMS" => "oidc-claims",
        "OIDC_NODE" => "oidc-node",
        "OAUTH2_ACCESS_TOKEN_MODIFICATION" => "oauth2-access-token",
        "OAUTH2_MAY_ACT" => "oauth2-may-act",
        "OAUTH2_SCRIPTED_JWT_ISSUER" => "oauth2-jwt-issuer",
        "OAUTH2_VALIDATE_SCOPE" => "oauth2-validate-scope",
        "OAUTH2_EVALUATE_SCOPE" => "oauth2-evaluate-scope",
        "OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER" => "oauth2-authz-data",
        "OAUTH2_DYNAMIC_CLIENT_REGISTRATION" => "oauth2-dcr",
        "POLICY_CONDITION" => "policy-condition",
        "CACHE_LOADER" => "cache-loader",
        "SOCIAL_IDP_PROFILE_TRANSFORMATION" => "social-normalize",
        "SOCIAL_PROVIDER_HANDLER_NODE" => "social-handler",
        "PINGONE_VERIFY_COMPLETION_DECISION_NODE" => "pingone-verify",
        "SAML2_IDP_ADAPTER" => "saml-idp-adapter",
        "SAML2_SP_ADAPTER" => "saml-sp-adapter",
        "SAML2_IDP_ATTRIBUTE_MAPPER" => "saml-idp-attr-mapper",
        "SAML2_NAMEID_MAPPER" => "saml-nameid-mapper",
        "SAML2_SP_ACCOUNT_MAPPER" => "saml-sp-account-mapper",
        other => return other.to_ascii_lowercase().replace('_', "-"),
    };
    mapped.to_string()
}

fn am_slug(r: &RemoteRef) -> String {
    slug_for(r.context.as_deref(), r.evaluator_version.as_deref())
}

pub fn workspace_subpath(r: &RemoteRef, realm: &str) -> PathBuf {
    PathBuf::from("am")
        .join(realm)
        .join(am_slug(r))
        .join(format!("{}.cjs", r.name))
}

/// Snapshot config path under `.aic-sync/configs/`. Keyed by realm + folder
/// slug + name so same-named scripts in different realms or contexts can't
/// clobber each other's snapshot.
pub fn config_subpath(r: &RemoteRef, realm: &str) -> PathBuf {
    PathBuf::from("am")
        .join(realm)
        .join(am_slug(r))
        .join(format!("{}.script.json", r.name))
}

/// Per-folder `tsconfig.json` for a script type's folder. Each AM script
/// folder needs its own leaf so the editor scopes the right `.d.ts` set
/// (loading every type's defs together would conflict) — there's no root
/// tsconfig; the TS server picks the nearest one per file. Folders sit two
/// levels under `am/` (`am/<realm>/<slug>/`), so `../../` reaches the shared
/// base tsconfig and `../../types/` the declarations.
///
/// The declaration set per slug follows the verified binding matrix
/// (`docs/api/12-script-bindings-matrix.md`): a shared Rhino interop layer, the
/// next-gen common bindings, and per-family overlays. Scripted decision shares
/// one base across both engine generations, with a thin next-gen / legacy
/// overlay on top. The library path alias is emitted **only** where libraries
/// are actually supported — next-gen scripted decision (`require` from
/// `../lib/*`) and library scripts themselves (sibling `./*`).
pub fn leaf_tsconfig(slug: &str) -> String {
    // Type files (under `../../types/`) and an optional `require()` path alias.
    let (types, lib_alias): (&[&str], Option<&str>) = match slug {
        "decision-node" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "decision-node-base.d.ts",
                "decision-node-next.d.ts",
            ],
            Some("../lib/*"),
        ),
        "decision-node-legacy" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "legacy-common.d.ts",
                "decision-node-base.d.ts",
                "decision-node-legacy.d.ts",
            ],
            None,
        ),
        "lib" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                // Argument types only — a library sees no per-context globals,
                // but has to be able to name what a caller hands it.
                "library-args.d.ts",
                "library.d.ts",
            ],
            Some("./*"),
        ),
        // Legacy OIDC claims is self-contained (its legacy logger/binding shapes
        // clash with the next-gen common set), so it pulls rhino + its own defs.
        "oidc-claims" => (&["rhino-1.7.14.d.ts", "oidc-claims.d.ts"], None),
        // Next-gen contexts (typed from the editor binding metadata via
        // scripts/gen-binding-types.mjs): shared next-gen common + a generated
        // per-context overlay.
        "oidc-claims-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oidc-claims-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "device-match" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "device-match.d.ts",
            ],
            Some("../lib/*"),
        ),
        "social-handler" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "social-handler.d.ts",
            ],
            Some("../lib/*"),
        ),
        "saml-nameid-mapper" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "saml-nameid-mapper.d.ts",
            ],
            Some("../lib/*"),
        ),
        "saml-sp-account-mapper" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "saml-sp-account-mapper.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-dcr" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-dcr.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-access-token-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-access-token-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-may-act-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-may-act-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-jwt-issuer-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-jwt-issuer-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-validate-scope-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-validate-scope-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-evaluate-scope-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-evaluate-scope-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "oauth2-authz-data-ng" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "oauth2-authz-data-ng.d.ts",
            ],
            Some("../lib/*"),
        ),
        "pingone-verify" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "nextgen-common.d.ts",
                "pingone-verify.d.ts",
            ],
            Some("../lib/*"),
        ),
        // Legacy token modification. Fully typed as of 2026-08-27 — every member
        // in `oauth2-access-token.d.ts` was CALLED against the live context, not
        // enumerated with `typeof`, which reports "function" for Java methods
        // that do not exist.
        //
        // This is the one leaf that does NOT get `secrets.d.ts`: the binding is
        // `undefined` here (measured in the same run), so declaring it would
        // hand the leaf a global its scripts can only `TypeError` on.
        "oauth2-access-token" => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "legacy-common.d.ts",
                "oauth2-access-token.d.ts",
            ],
            None,
        ),
        // Any other context (legacy OAuth2, SAML adapters, policy condition, …):
        // shared Rhino + common globals, plus the classic Debug `logger` (these
        // are mostly unmigrated/legacy-style scripts), until they go next-gen.
        _ => (
            &[
                "rhino-1.7.14.d.ts",
                "common.d.ts",
                "secrets.d.ts",
                "legacy-common.d.ts",
            ],
            None,
        ),
    };

    let mut includes = vec!["./**/*".to_string()];
    includes.extend(types.iter().map(|t| format!("../../types/{t}")));
    if types.contains(&"nextgen-common.d.ts") {
        includes.push("../../types/managed/*.d.ts".to_string());
    }
    let include_json = includes
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let compiler_opts = match lib_alias {
        Some(path) => {
            format!(",\n  \"compilerOptions\": {{ \"paths\": {{ \"*\": [\"{path}\"] }} }}")
        }
        None => String::new(),
    };
    format!(
        "{{\n  \"extends\": \"../../tsconfig.json\",\n  \"include\": [{include_json}]{compiler_opts}\n}}\n"
    )
}

/// Files written into the script's folder on pull (overwritten each time —
/// they're managed): the folder's leaf `tsconfig.json` (always), plus, for a
/// `LIBRARY` script, an ES-module wrapper so other scripts can `require` it
/// with types (matches p1-sync).
pub fn extra_files(r: &RemoteRef, realm: &str) -> Vec<(PathBuf, String)> {
    let slug = am_slug(r);
    let folder = PathBuf::from("am").join(realm).join(&slug);
    let mut out = vec![(folder.join("tsconfig.json"), leaf_tsconfig(&slug))];
    if r.context.as_deref() == Some("LIBRARY") {
        out.push((
            folder.join(format!("{}.js", r.name)),
            format!("export * from \"./{}.cjs\";\n", r.name),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rref(context: Option<&str>) -> RemoteRef {
        rref_v(context, None)
    }

    fn rref_v(context: Option<&str>, evaluator_version: Option<&str>) -> RemoteRef {
        RemoteRef {
            kind: Kind::Am,
            id: "uuid-1".into(),
            name: "MyScript".into(),
            context: context.map(|s| s.to_string()),
            is_default: false,
            evaluator_version: evaluator_version.map(|s| s.to_string()),
        }
    }

    #[test]
    fn source_round_trips_through_base64() {
        let body = b"function foo() { return 1; }\n";
        let mut raw = json!({"name": "MyScript", "script": ""});
        encode_source(&mut raw, body).unwrap();
        // wire value is base64
        assert_eq!(raw["script"], json!(B64.encode(body)));
        // and decodes back to the same bytes
        assert_eq!(decode_source(&raw).unwrap(), body);
    }

    #[test]
    fn context_resolution_accepts_constants_slugs_and_reports_ambiguity() {
        let contexts = vec![
            "LIBRARY".into(),
            "SCRIPTED_DECISION_NODE".into(),
            "OTHER_CONTEXT".into(),
        ];
        assert_eq!(
            resolve_context("library", &contexts).unwrap(),
            ("LIBRARY".into(), None)
        );
        assert_eq!(
            resolve_context("lib", &contexts).unwrap(),
            ("LIBRARY".into(), None)
        );
        assert_eq!(
            resolve_context("decision-node", &contexts).unwrap(),
            ("SCRIPTED_DECISION_NODE".into(), Some("2.0".into()))
        );
        // Legacy-engine creation is refused outright, by slug...
        assert!(
            resolve_context("decision-node-legacy", &contexts)
                .unwrap_err()
                .to_string()
                .contains("deprecated")
        );
        assert!(
            resolve_context("missing", &contexts)
                .unwrap_err()
                .to_string()
                .contains("available slugs")
        );
        let ambiguous = vec!["ONE_CONTEXT".into(), "one_context".into()];
        let error = resolve_context("one-context", &ambiguous)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ONE_CONTEXT") && error.contains("one_context"));
    }

    #[test]
    fn new_script_has_required_am_defaults_and_fresh_ids() {
        assert!(
            new_script("x", b"", &NewScriptOpts::default())
                .unwrap_err()
                .to_string()
                .contains("--context")
        );
        let opts = NewScriptOpts {
            context: Some("LIBRARY".into()),
            ..Default::default()
        };
        let first = new_script("x", b"body", &opts).unwrap();
        let second = new_script("x", b"body", &opts).unwrap();
        assert_ne!(first.reference.id, second.reference.id);
        assert!(uuid::Uuid::parse_str(&first.reference.id).is_ok());
        assert_eq!(first.raw_config["language"], "JAVASCRIPT");
        assert_eq!(first.raw_config["evaluatorVersion"], "2.0");
        assert_eq!(first.raw_config["default"], false);
        assert_eq!(first.raw_config["_id"], first.reference.id);
    }

    #[test]
    fn legacy_engine_creation_is_refused_however_it_is_requested() {
        // ...and by an explicit flag, whatever the context.
        let legacy = NewScriptOpts {
            context: Some("LIBRARY".into()),
            evaluator_version: Some("1.0".into()),
            ..Default::default()
        };
        assert!(
            new_script("x", b"body", &legacy)
                .unwrap_err()
                .to_string()
                .contains("legacy engine")
        );
    }

    #[test]
    fn contexts_are_read_from_the_id_field_and_skip_hidden_ones() {
        // The endpoint returns context *objects*, not strings — treating an
        // element as its own name yields an empty list, which would make every
        // `--context` fail with "unknown AM context".
        let body = json!({"result": [
            {"_id": "LIBRARY", "isHidden": false, "languages": ["JAVASCRIPT"]},
            {"_id": "NODE_DESIGNER", "isHidden": true, "languages": ["JAVASCRIPT"]},
            {"_id": "OIDC_CLAIMS"},
        ]});
        assert_eq!(parse_contexts(&body).unwrap(), ["LIBRARY", "OIDC_CLAIMS"]);

        // A shape we don't recognise must fail loudly rather than resolve to
        // "no contexts exist".
        assert!(parse_contexts(&json!({"result": ["LIBRARY"]})).is_err());
        assert!(parse_contexts(&json!({"nope": []})).is_err());
    }

    #[test]
    fn legacy_array_script_decodes_as_lines() {
        let raw = json!({"script": ["line1", "line2"]});
        assert_eq!(decode_source(&raw).unwrap(), b"line1\nline2\n");
    }

    #[test]
    fn context_routes_to_per_type_folder() {
        // Next-gen vs legacy scripted decision node split by evaluatorVersion.
        assert_eq!(
            workspace_subpath(
                &rref_v(Some("AUTHENTICATION_TREE_DECISION_NODE"), Some("2.0")),
                "bravo"
            ),
            PathBuf::from("am/bravo/decision-node/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(
                &rref_v(Some("AUTHENTICATION_TREE_DECISION_NODE"), Some("1.0")),
                "alpha"
            ),
            PathBuf::from("am/alpha/decision-node-legacy/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(&rref(Some("LIBRARY")), "alpha"),
            PathBuf::from("am/alpha/lib/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(&rref(Some("OIDC_CLAIMS")), "bravo"),
            PathBuf::from("am/bravo/oidc-claims/MyScript.cjs")
        );
        // `…_NEXT_GEN` / `…_NEXTGEN` → base slug + `-ng`.
        assert_eq!(
            workspace_subpath(&rref(Some("OIDC_CLAIMS_NEXT_GEN")), "alpha"),
            PathBuf::from("am/alpha/oidc-claims-ng/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(
                &rref(Some("OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN")),
                "alpha"
            ),
            PathBuf::from("am/alpha/oauth2-access-token-ng/MyScript.cjs")
        );
        for (context, slug) in [
            ("OAUTH2_MAY_ACT_NEXT_GEN", "oauth2-may-act-ng"),
            (
                "OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN",
                "oauth2-jwt-issuer-ng",
            ),
            ("OAUTH2_VALIDATE_SCOPE_NEXT_GEN", "oauth2-validate-scope-ng"),
            ("OAUTH2_EVALUATE_SCOPE_NEXT_GEN", "oauth2-evaluate-scope-ng"),
            (
                "OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER_NEXT_GEN",
                "oauth2-authz-data-ng",
            ),
        ] {
            assert_eq!(
                workspace_subpath(&rref(Some(context)), "alpha"),
                PathBuf::from(format!("am/alpha/{slug}/MyScript.cjs"))
            );
        }
        assert_eq!(
            workspace_subpath(&rref(Some("SAML2_IDP_ADAPTER_NEXTGEN")), "alpha"),
            PathBuf::from("am/alpha/saml-idp-adapter-ng/MyScript.cjs")
        );
        // Unknown context → lowercased, hyphenated fallback.
        assert_eq!(
            workspace_subpath(&rref(Some("BRAND_NEW_CONTEXT")), "alpha"),
            PathBuf::from("am/alpha/brand-new-context/MyScript.cjs")
        );
        // Config snapshot path is now folder-scoped too.
        assert_eq!(
            config_subpath(&rref(Some("OIDC_CLAIMS")), "bravo"),
            PathBuf::from("am/bravo/oidc-claims/MyScript.script.json")
        );
    }

    #[test]
    fn skips_groovy_and_internal_scripts() {
        assert!(is_syncable(
            &json!({"name": "My Node", "language": "JAVASCRIPT"})
        ));
        // Groovy is no longer supported by AIC.
        assert!(!is_syncable(
            &json!({"name": "Old Mapper", "language": "GROOVY"})
        ));
        // Product-internal scripts (only the name prefix marks them).
        assert!(!is_syncable(
            &json!({"name": "ForgeRock Internal: OIDC Claims Script", "language": "JAVASCRIPT"})
        ));
    }

    #[test]
    fn extra_files_emit_folder_tsconfig_and_library_wrapper() {
        // Every folder gets a leaf tsconfig; only LIBRARY also gets the wrapper.
        let oidc = extra_files(&rref(Some("OIDC_CLAIMS")), "alpha");
        assert_eq!(oidc.len(), 1);
        assert_eq!(
            oidc[0].0,
            PathBuf::from("am/alpha/oidc-claims/tsconfig.json")
        );
        assert!(oidc[0].1.contains("../../types/oidc-claims.d.ts"));

        let lib = extra_files(&rref(Some("LIBRARY")), "bravo");
        assert_eq!(lib.len(), 2);
        assert_eq!(lib[0].0, PathBuf::from("am/bravo/lib/tsconfig.json"));
        assert!(lib[0].1.contains("../../types/library.d.ts"));
        assert_eq!(lib[1].0, PathBuf::from("am/bravo/lib/MyScript.js"));
        assert!(lib[1].1.contains("export * from \"./MyScript.cjs\""));
    }

    #[test]
    fn leaf_tsconfig_scopes_dts_by_type() {
        // Next-gen decision: base + next overlay + (next-gen) common, lib alias.
        let next = leaf_tsconfig("decision-node");
        assert!(next.contains("../../types/decision-node-base.d.ts"));
        assert!(next.contains("../../types/decision-node-next.d.ts"));
        assert!(next.contains("../../types/common.d.ts"));
        assert!(next.contains("../../types/nextgen-common.d.ts"));
        assert!(next.contains("../../types/managed/*.d.ts"));
        assert!(next.rfind("nextgen-common.d.ts").unwrap() < next.rfind("managed/*.d.ts").unwrap());
        assert!(!next.contains("legacy-common.d.ts"));
        assert!(next.contains("\"*\": [\"../lib/*\"]"));

        // Legacy shares the base but gets the legacy overlay (not next-gen), the
        // classic logger via legacy-common (not nextgen-common's slf4j one), and
        // no library alias — legacy scripts can't require libraries.
        let legacy = leaf_tsconfig("decision-node-legacy");
        assert!(legacy.contains("../../types/decision-node-base.d.ts"));
        assert!(legacy.contains("../../types/decision-node-legacy.d.ts"));
        assert!(legacy.contains("../../types/legacy-common.d.ts"));
        assert!(!legacy.contains("decision-node-next.d.ts"));
        assert!(!legacy.contains("nextgen-common.d.ts"));
        assert!(!legacy.contains("managed/*.d.ts"));
        assert!(!legacy.contains("paths"));

        // Library scripts: next-gen common + the caller-argument types + library
        // overlay + sibling alias.
        let lib = leaf_tsconfig("lib");
        assert!(lib.contains("../../types/library.d.ts"));
        assert!(lib.contains("../../types/library-args.d.ts"));
        assert!(lib.contains("../../types/nextgen-common.d.ts"));
        assert!(lib.contains("../../types/managed/*.d.ts"));
        assert!(lib.contains("\"*\": [\"./*\"]"));
        // Only libraries get them. Not because the names collide — measured
        // 2026-08-26, TypeScript merges the duplicate declarations and the leaf
        // still compiles — but because merging turns a hand-written overlay's
        // `IdRepository.getIdentity(): Identity` and the generated
        // `getIdentity(): object` into one overload set whose resolution order
        // follows file order, so a caller's inferred return type moves silently.
        for slug in ["decision-node", "decision-node-legacy", "device-match"] {
            assert!(
                !leaf_tsconfig(slug).contains("library-args.d.ts"),
                "{slug} must not pull the library argument types"
            );
        }

        // OIDC claims is self-contained (no next-gen common set).
        let oidc = leaf_tsconfig("oidc-claims");
        assert!(oidc.contains("../../types/oidc-claims.d.ts"));
        assert!(!oidc.contains("common.d.ts"));
        assert!(!oidc.contains("managed/*.d.ts"));
        assert!(!oidc.contains("paths"));

        // Next-gen context overlays: shared next-gen common + their generated
        // per-context defs, with the library alias (all next-gen can require).
        for (slug, overlay) in [
            ("oidc-claims-ng", "oidc-claims-ng.d.ts"),
            ("device-match", "device-match.d.ts"),
            ("social-handler", "social-handler.d.ts"),
            ("saml-nameid-mapper", "saml-nameid-mapper.d.ts"),
            ("saml-sp-account-mapper", "saml-sp-account-mapper.d.ts"),
            ("oauth2-dcr", "oauth2-dcr.d.ts"),
            ("oauth2-access-token-ng", "oauth2-access-token-ng.d.ts"),
            ("oauth2-may-act-ng", "oauth2-may-act-ng.d.ts"),
            ("oauth2-jwt-issuer-ng", "oauth2-jwt-issuer-ng.d.ts"),
            ("oauth2-validate-scope-ng", "oauth2-validate-scope-ng.d.ts"),
            ("oauth2-evaluate-scope-ng", "oauth2-evaluate-scope-ng.d.ts"),
            ("oauth2-authz-data-ng", "oauth2-authz-data-ng.d.ts"),
            ("pingone-verify", "pingone-verify.d.ts"),
        ] {
            let cfg = leaf_tsconfig(slug);
            assert!(
                cfg.contains("../../types/nextgen-common.d.ts"),
                "{slug} nextgen-common"
            );
            assert!(
                cfg.contains(&format!("../../types/{overlay}")),
                "{slug} overlay"
            );
            assert!(cfg.contains("../../types/managed/*.d.ts"), "{slug} managed");
            assert!(
                cfg.rfind(&format!("../../types/{overlay}")).unwrap()
                    < cfg.rfind("../../types/managed/*.d.ts").unwrap(),
                "{slug} managed types last"
            );
            assert!(!cfg.contains("legacy-common"), "{slug} no legacy-common");
            assert!(cfg.contains("\"*\": [\"../lib/*\"]"), "{slug} lib alias");
        }

        // Any other (legacy/unmigrated) context: shared rhino + common + classic
        // Debug logger, no next-gen overlay, no library alias.
        let other = leaf_tsconfig("config-provider");
        assert!(other.contains("../../types/rhino-1.7.14.d.ts"));
        assert!(other.contains("../../types/common.d.ts"));
        assert!(other.contains("../../types/legacy-common.d.ts"));
        assert!(!other.contains("decision-node"));
        assert!(!other.contains("nextgen-common"));
        assert!(!other.contains("managed/*.d.ts"));
        assert!(!other.contains("paths"));
    }

    #[test]
    fn library_template_exposes_factory_types_without_decision_globals() {
        // Libraries receive these values as `.load(...)` arguments, never as
        // ambient scripted-decision bindings.
        let library = include_str!("templates/am/types/library.d.ts");
        assert!(library.contains("interface NodeState"));
        assert!(library.contains("type RequestHeaders = RequestMap;"));
        assert!(library.contains("type RequestParameters = RequestMap;"));
        assert!(!library.contains("_nodeStateGet"));
        assert!(!library.contains("declare const nodeState"));
        assert!(!library.contains("declare const requestHeaders"));
        assert!(!library.contains("declare const requestParameters"));

        // Everything else a caller can pass is generated into library-args.d.ts,
        // which carries the types WITHOUT their bindings.
        let args = include_str!("templates/am/types/library-args.d.ts");
        assert!(args.contains("interface CallbacksBuilder"));
        assert!(args.contains("interface Action"));
        assert!(args.contains("interface AccessToken"));
        assert!(
            !args.contains("declare "),
            "library-args.d.ts must declare types only, never a binding"
        );
        // NodeState is hand-written in library.d.ts; two declarations of it in
        // one scope would merge into a contradictory `get`.
        assert!(!args.contains("interface NodeState"));

        // A binding is only the members EVERY caller has. Unioning them was
        // unsound: `createUser` exists on the JWT-issuer `idRepository` alone,
        // and a merged `IdRepository` let a library call it on the
        // scripted-decision binding — type-checked, "not a function" at runtime.
        let id_repository = args
            .split("interface ")
            .find(|block| block.starts_with("IdRepository {"))
            .expect("IdRepository");
        assert!(id_repository.contains("getIdentity"));
        assert!(
            !id_repository.contains("createUser"),
            "createUser is not on every context's idRepository"
        );
        // Omitting it silently would read as "this binding has nothing else", so
        // the generator names what it dropped and which context has it. A
        // context-qualified type carrying the extras was the other option and is
        // worse: a caller resolves `require()` through its own leaf, which does
        // not include this file, so the name fails to compile in every caller.
        assert!(args.contains("createUser") && args.contains("(Oauth2JwtIssuer only)"));
        assert!(!args.contains("interface Oauth2JwtIssuerIdRepository"));
    }

    /// `requestProperties`/`clientProperties` are an `object` with no enumerated
    /// members in the editor metadata, so generated verbatim they reach a script
    /// as `object` — which under the workspace's `strict` tsconfig cannot be
    /// read, indexed or completed at all. The generator names them instead.
    #[test]
    fn the_oauth2_request_context_is_named_not_a_bare_object() {
        for (overlay, contents) in [
            (
                "oauth2-access-token-ng.d.ts",
                include_str!("templates/am/types/oauth2-access-token-ng.d.ts"),
            ),
            (
                "oauth2-validate-scope-ng.d.ts",
                include_str!("templates/am/types/oauth2-validate-scope-ng.d.ts"),
            ),
            (
                "oauth2-evaluate-scope-ng.d.ts",
                include_str!("templates/am/types/oauth2-evaluate-scope-ng.d.ts"),
            ),
            (
                "oauth2-may-act-ng.d.ts",
                include_str!("templates/am/types/oauth2-may-act-ng.d.ts"),
            ),
            (
                "oauth2-authz-data-ng.d.ts",
                include_str!("templates/am/types/oauth2-authz-data-ng.d.ts"),
            ),
            (
                "oauth2-dcr.d.ts",
                include_str!("templates/am/types/oauth2-dcr.d.ts"),
            ),
            (
                "oidc-claims-ng.d.ts",
                include_str!("templates/am/types/oidc-claims-ng.d.ts"),
            ),
        ] {
            for binding in ["requestProperties", "clientProperties"] {
                assert!(
                    !contents.contains(&format!("declare const {binding}: object;")),
                    "{overlay}: `{binding}` regenerated as a bare object — the \
                     generator's NAMED_OPAQUE table is what stops that"
                );
            }
        }
        // Both shapes are shared, so they live with the next-gen common set —
        // which is also what puts them in library scope.
        let shared = include_str!("templates/am/types/nextgen-common.d.ts");
        assert!(shared.contains("interface RequestProperties"));
        assert!(shared.contains("interface ClientProperties"));
        // The legacy OIDC claims leaf keeps its own Java-shaped pair; its leaf
        // pulls neither common file, so the two never meet.
        let legacy = include_str!("templates/am/types/oidc-claims.d.ts");
        assert!(legacy.contains("interface RequestProperties"));
        assert!(!leaf_tsconfig("oidc-claims").contains("nextgen-common.d.ts"));
    }

    /// The legacy access-token-modification leaf, which went from a wall of
    /// `any` to a measured shape on 2026-08-27.
    ///
    /// Every member in `oauth2-access-token.d.ts` was CALLED against the live
    /// context. That mattered: an earlier pass enumerated the surface with
    /// `typeof`, which reports `"function"` for a Rhino-wrapped Java method that
    /// does not exist — `identity.getMemberships` reads as a function and throws
    /// `Can't find method` when called. So this test holds the two properties a
    /// future edit could plausibly break, and leaves the member list to
    /// `docs/api/12-script-bindings-matrix.md`.
    #[test]
    fn the_legacy_token_modification_leaf_is_typed_from_calls_not_from_the_nextgen_overlay() {
        let cfg = leaf_tsconfig("oauth2-access-token");
        assert!(cfg.contains("../../types/oauth2-access-token.d.ts"));
        assert!(cfg.contains("legacy-common.d.ts"));
        // Legacy cannot require() a library (verified — ReferenceError).
        assert!(!cfg.contains("paths"));

        // `secrets` is `undefined` in this context, so this is the one leaf that
        // must not be handed the binding. Declaring it would compile a script
        // that can only TypeError.
        assert!(
            !cfg.contains("secrets.d.ts"),
            "the legacy token-mod leaf must not include secrets.d.ts — the binding is absent there"
        );
        for slug in ["decision-node", "decision-node-legacy", "oidc-claims-ng"] {
            assert!(
                leaf_tsconfig(slug).contains("secrets.d.ts"),
                "{slug} lost secrets.d.ts"
            );
        }

        let legacy = include_str!("templates/am/types/oauth2-access-token.d.ts");
        let lint = include_str!("templates/am/eslint.config.js");
        let block = lint
            .split("files: [\"*/oauth2-access-token/**/*.cjs\"]")
            .nth(1)
            .and_then(|rest| rest.split("},").next())
            .expect("the legacy ESLint globals block");

        // The linter and the type layer must name the same bindings: `no-undef`
        // is off here *because* the type layer is meant to be the authority, so
        // a binding in only one of them is invisible in exactly one direction.
        for binding in [
            "accessToken",
            "identity",
            "session",
            "scopes",
            "requestProperties",
            "clientProperties",
        ] {
            assert!(block.contains(binding), "eslint lost {binding}");
            assert!(
                legacy.contains(&format!("declare const {binding}")),
                "the type layer lost {binding}"
            );
        }
        assert!(
            !block.contains("secrets"),
            "eslint must not offer `secrets` here either"
        );

        // The context is NOT the next-gen one with the suffix stripped, and the
        // measured differences are the ones a well-meaning edit would "fix" by
        // copying `oauth2-access-token-ng.d.ts` over. Each name below throws or
        // is undefined here.
        for absent in [
            "setAct(",
            "setMayAct(",
            "setPermissions(",
            "setConfirmationKey(",
            "getExtraData(",
            "getResourceOwner(",
            "getAttributeValues(",
            "addAttribute(",
        ] {
            assert!(
                !legacy.contains(absent),
                "{absent} is not callable in the legacy token-mod context — do not copy it from the next-gen overlay"
            );
        }
        // `isExists`/`getAttribute` are the legacy AMIdentity spellings; the
        // next-gen `exists`/`getAttributeValues` throw here.
        assert!(legacy.contains("isExists(): boolean;"));
        assert!(legacy.contains("getAttribute(attributeName: StringLike)"));
        // A JS array throws `Cannot convert NativeArray to java.util.Set`.
        assert!(legacy.contains("setScope(scopes: JavaSet<JavaString>): void;"));
    }

    /// A Java collection is reached with a JS string literal in every family —
    /// `scopes.contains("openid")`, `requestedClaims.get("email")` — and typing
    /// the lookup parameter as the collection's own element type rejected all of
    /// them. The widening has to stay conditional: `any` would take anything.
    #[test]
    fn java_lookups_take_a_js_string_without_taking_anything() {
        let rhino = include_str!("templates/am/types/rhino-1.7.14.d.ts");
        assert!(rhino.contains("type Lookup<T> = T extends JavaString ? StringLike : T;"));
        for signature in [
            "get(key: Lookup<Key>)",
            "containsKey?(key: Lookup<Key>)",
            "includes(value: Lookup<T>)",
            "contains(value: Lookup<T>)",
            "contains(key: Lookup<T>)",
        ] {
            assert!(rhino.contains(signature), "rhino: {signature}");
        }
        // The legacy OIDC leaf is the one that reaches Java collections for
        // everything, and its own parameters have to widen the same way.
        let oidc = include_str!("templates/am/types/oidc-claims.d.ts");
        assert!(oidc.contains("getAttribute(attributeName: StringLike)"));
        assert!(oidc.contains("getProperty(name: StringLike)"));
        assert!(
            !oidc.contains(": string)"),
            "a `string` parameter rejects the JavaString a legacy script has in hand"
        );
    }

    /// Anything a caller can hand a library has to be nameable in library scope
    /// — the globals stay out, their types cannot. Driven off the context
    /// metadata rather than a list, so a newly captured context fails here until
    /// library-args.d.ts is regenerated (the command is in its footer).
    #[test]
    fn every_next_gen_binding_type_is_nameable_in_library_scope() {
        use std::collections::HashSet;
        use std::path::Path;

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let types = root.join("src/scripts/templates/am/types");
        // Take the file set from the leaf itself, so the two cannot drift.
        let mut scope = String::new();
        for name in leaf_tsconfig("lib")
            .split('"')
            .filter_map(|part| part.strip_prefix("../../types/"))
            .filter(|name| name.ends_with(".d.ts") && !name.contains('*'))
        {
            scope.push_str(&std::fs::read_to_string(types.join(name)).unwrap());
        }
        let ident = |line: &str, keyword: &str| {
            line.strip_prefix(keyword).map(|rest| {
                rest.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or_default()
                    .to_lowercase()
            })
        };
        // Case-insensitive: `openidm` pascal-cases to `Openidm`, and the shared
        // set spells it `OpenIdm`.
        let declared: HashSet<String> = scope
            .lines()
            .filter_map(|line| ident(line, "interface ").or_else(|| ident(line, "type ")))
            .collect();

        let mut checked = 0;
        for entry in std::fs::read_dir(root.join("docs/api/bindings")).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            // Only next-gen contexts can require() a library, and the library's
            // own bindings are already the shared set.
            if !file.ends_with("-next.json") || file == "library-next.json" {
                continue;
            }
            let ctx: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for binding in ctx["bindings"].as_array().unwrap() {
                let name = binding["name"].as_str().unwrap();
                // Only object bindings with enumerated members get an interface;
                // the rest are scalars, or `RequestMap` (aliased in library.d.ts).
                if binding["javaScriptType"] != "object"
                    || binding["elements"]
                        .as_array()
                        .is_none_or(|elements| elements.is_empty())
                {
                    continue;
                }
                let wanted = name.to_lowercase();
                assert!(
                    declared.contains(&wanted),
                    "{file}: `{name}` can be passed into a library, but library \
                     scope declares no type named it — regenerate library-args.d.ts"
                );
                checked += 1;
            }
        }
        // A glob that stops matching would make this vacuous.
        assert!(checked > 20, "only {checked} bindings checked");
    }
}
