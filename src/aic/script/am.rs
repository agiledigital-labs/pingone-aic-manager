//! AM-script specifics for the script-sync core. The **only** place AM-vs-IDM
//! differences for AM live: realm-scoped `/am/json…/scripts`, the
//! `protocol=2.0,resource=1.0` header, base64 `script` body, and context→dir
//! routing. See `docs/api/04-scripts.md`.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::Value;
use std::path::PathBuf;

/// AM scripts require the protocol-versioned header (the client default of
/// `resource=1.0` 400s on the scripts endpoint).
const API_VERSION: &str = "protocol=2.0,resource=1.0";

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
        is_default: raw.get("default").and_then(|v| v.as_bool()).unwrap_or(false),
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
    crate::aic::api::put_versioned(tenant, &path, script.raw_config.clone(), confirmed_prod, API_VERSION)
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
    if matches!(ctx, "AUTHENTICATION_TREE_DECISION_NODE" | "SCRIPTED_DECISION_NODE") {
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
/// folder needs its own leaf so the editor scopes the right `.d.ts` (loading
/// every type's defs together would conflict) — there's no root tsconfig; the
/// TS server picks the nearest one per file. Folders sit two levels under
/// `am/` (`am/<realm>/<slug>/`), so `../../` reaches the shared defs.
///
/// Only the three types we have bindings for today get a type-specific `.d.ts`
/// (next-gen decision node, library, OIDC claims); every other folder gets the
/// shared globals only, until per-type definitions land.
pub fn leaf_tsconfig(slug: &str) -> String {
    let (extra_dts, lib_path): (Option<&str>, &str) = match slug {
        "decision-node" => (Some("../../src.d.ts"), "../lib/*"),
        "oidc-claims" => (Some("../../oidc.d.ts"), "../lib/*"),
        "lib" => (Some("../../lib.d.ts"), "./*"),
        _ => (None, "../lib/*"),
    };
    let mut includes = vec!["./**/*", "../../amCommon.d.ts"];
    if let Some(d) = extra_dts {
        includes.push(d);
    }
    let include_json = includes
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\n  \"extends\": \"../../tsconfig.json\",\n  \"include\": [{include_json}],\n  \"compilerOptions\": {{ \"paths\": {{ \"*\": [\"{lib_path}\"] }} }}\n}}\n"
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
    fn legacy_array_script_decodes_as_lines() {
        let raw = json!({"script": ["line1", "line2"]});
        assert_eq!(decode_source(&raw).unwrap(), b"line1\nline2\n");
    }

    #[test]
    fn context_routes_to_per_type_folder() {
        // Next-gen vs legacy scripted decision node split by evaluatorVersion.
        assert_eq!(
            workspace_subpath(&rref_v(Some("AUTHENTICATION_TREE_DECISION_NODE"), Some("2.0")), "bravo"),
            PathBuf::from("am/bravo/decision-node/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(&rref_v(Some("AUTHENTICATION_TREE_DECISION_NODE"), Some("1.0")), "alpha"),
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
        assert!(is_syncable(&json!({"name": "My Node", "language": "JAVASCRIPT"})));
        // Groovy is no longer supported by AIC.
        assert!(!is_syncable(&json!({"name": "Old Mapper", "language": "GROOVY"})));
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
        assert_eq!(oidc[0].0, PathBuf::from("am/alpha/oidc-claims/tsconfig.json"));
        assert!(oidc[0].1.contains("../../oidc.d.ts"));

        let lib = extra_files(&rref(Some("LIBRARY")), "bravo");
        assert_eq!(lib.len(), 2);
        assert_eq!(lib[0].0, PathBuf::from("am/bravo/lib/tsconfig.json"));
        assert!(lib[0].1.contains("../../lib.d.ts"));
        assert_eq!(lib[1].0, PathBuf::from("am/bravo/lib/MyScript.js"));
        assert!(lib[1].1.contains("export * from \"./MyScript.cjs\""));
    }

    #[test]
    fn leaf_tsconfig_scopes_dts_by_type() {
        assert!(leaf_tsconfig("decision-node").contains("../../src.d.ts"));
        // A type without bindings yet gets only the shared globals.
        let legacy = leaf_tsconfig("decision-node-legacy");
        assert!(legacy.contains("../../amCommon.d.ts"));
        assert!(!legacy.contains("src.d.ts"));
        assert!(leaf_tsconfig("lib").contains("\"*\": [\"./*\"]"));
    }
}
