//! The per-tenant TypeScript endpoint project (`workspace/<tenant>/typescript/`).
//!
//! IDM has no module system — `require()` resolves three bundled libraries and
//! nothing you can push (`docs/api/11-idm-endpoints.md`) — so shared code is
//! shared at **build time** instead. The project bundles each endpoint into one
//! self-contained ES5 file written to `idm/endpoint/<name>.cjs`, where the
//! ordinary sync engine takes over.
//!
//! Rust owns two seams into it: scaffolding + type generation (`workspace.rs`,
//! `managed_types.rs`) and the **ownership manifest** read here. The manifest
//! is what lets `aic script watch` create an endpoint that exists only as a
//! generated file: without it, a generated `.cjs` is indistinguishable from a
//! stray untracked file and is skipped.
//!
//! Design notes: `docs/typescript-endpoints.md`.

use crate::config::ProjectConfig;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Project-relative directory holding the TypeScript project.
pub const PROJECT_DIR: &str = "typescript";

/// The build writes this at the project root after a successful build.
pub const MANIFEST_FILE: &str = ".aic-ts-manifest.json";

/// The manifest as the build writes it. Unknown fields are ignored so the
/// build can add detail without a lockstep `aic` release.
#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    endpoints: Vec<ManifestEndpoint>,
}

#[derive(Debug, Deserialize)]
struct ManifestEndpoint {
    name: String,
}

/// `workspace/<tenant>/typescript/`.
pub fn project_dir(tenant: &str) -> PathBuf {
    ProjectConfig::workspace_tree(tenant).join(PROJECT_DIR)
}

/// `workspace/<tenant>/typescript/.aic-ts-manifest.json`.
pub fn manifest_path(tenant: &str) -> PathBuf {
    project_dir(tenant).join(MANIFEST_FILE)
}

/// Endpoint names the TypeScript project declares it owns.
///
/// Best-effort by design: a missing or malformed manifest yields an empty set,
/// which restores the previous behaviour (untracked files are skipped) rather
/// than failing a watch session over a build artefact.
pub fn declared_endpoints(tenant: &str) -> BTreeSet<String> {
    read_manifest(&manifest_path(tenant))
}

fn read_manifest(path: &Path) -> BTreeSet<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    match serde_json::from_str::<Manifest>(&contents) {
        Ok(manifest) => manifest
            .endpoints
            .into_iter()
            .map(|endpoint| endpoint.name)
            .collect(),
        Err(_) => BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(contents: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("aic-ts-manifest-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_the_declared_endpoint_names() {
        let path = temp_file(
            r#"{"version":1,"endpoints":[
                 {"name":"a","file":"idm/endpoint/a.cjs","routes":[]},
                 {"name":"b","file":"idm/endpoint/b.cjs"}
               ]}"#,
        );
        let names = read_manifest(&path);
        assert_eq!(
            names,
            BTreeSet::from(["a".to_string(), "b".to_string()]),
            "both endpoints should be declared"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tolerates_extra_fields_the_build_may_add() {
        let path = temp_file(r#"{"version":9,"generator":"x","endpoints":[{"name":"a","x":1}]}"#);
        assert!(read_manifest(&path).contains("a"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_or_broken_manifest_declares_nothing() {
        // Missing: the project was never built, or does not exist.
        assert!(read_manifest(Path::new("/definitely/not/here.json")).is_empty());
        // Malformed: never fail a watch session over a build artefact.
        let path = temp_file("{ not json");
        assert!(read_manifest(&path).is_empty());
        std::fs::remove_file(&path).ok();

        let wrong_shape = temp_file(r#"{"endpoints":"nope"}"#);
        assert!(read_manifest(&wrong_shape).is_empty());
        std::fs::remove_file(&wrong_shape).ok();
    }

    #[test]
    fn manifest_path_sits_at_the_project_root() {
        let path = manifest_path("sandbox");
        assert!(
            path.ends_with("typescript/.aic-ts-manifest.json"),
            "{path:?}"
        );
    }
}
