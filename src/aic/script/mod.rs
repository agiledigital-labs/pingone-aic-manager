//! Script-sync core: pull/push AIC scripts to a local typed workspace, with
//! content-based conflict detection (CLAUDE.md §5 — scripts have no `_rev`).
//!
//! Architecture: the sync **engine** (`sync`) and **workspace scaffolding**
//! (`workspace`) are entirely kind-agnostic. Everything that differs between
//! AM scripts (realm-scoped, base64 `script`, context→dir routing) and IDM
//! endpoints (`/openidm/config/endpoint`, plaintext `source`) lives behind the
//! [`Kind`] enum, which delegates to the `am` / `idm` modules. The engine
//! never matches on `Kind` — to add journeys later, add a variant + a module
//! and wire the dispatch arms here; nothing in `sync`/`workspace` changes.

pub mod am;
pub mod idm;
pub mod schedule;
pub mod sync;
pub mod workspace;

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The script-like resource families we sync. The single dispatch point for
/// all AM-vs-IDM differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Kind {
    /// AM scripts: `/am/json{realm}/scripts/{uuid}`, base64 `script` body.
    #[serde(rename = "am")]
    #[value(name = "am")]
    Am,
    /// IDM custom endpoints: `/openidm/config/endpoint/{name}`, plaintext `source`.
    #[serde(rename = "idm")]
    #[value(name = "idm", alias = "endpoint")]
    IdmEndpoint,
    /// IDM scheduled jobs: `/openidm/config/schedule/{name}`, script at
    /// `invokeContext.script.source` (script-invoking schedules only).
    #[serde(rename = "schedule")]
    #[value(name = "schedule", alias = "schedules")]
    IdmSchedule,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Am => "am",
            Kind::IdmEndpoint => "idm",
            Kind::IdmSchedule => "schedule",
        }
    }

    pub fn all() -> &'static [Kind] {
        &[Kind::Am, Kind::IdmEndpoint, Kind::IdmSchedule]
    }

    // ----- async I/O (delegates to the per-kind module) ------------------

    pub async fn list(self, tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
        match self {
            Kind::Am => am::list(tenant, realm).await,
            Kind::IdmEndpoint => idm::list(tenant, realm).await,
            Kind::IdmSchedule => schedule::list(tenant, realm).await,
        }
    }

    pub async fn fetch(self, tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
        match self {
            Kind::Am => am::fetch(tenant, realm, id).await,
            Kind::IdmEndpoint => idm::fetch(tenant, realm, id).await,
            Kind::IdmSchedule => schedule::fetch(tenant, realm, id).await,
        }
    }

    pub async fn write(
        self,
        tenant: &str,
        realm: &str,
        script: &RemoteScript,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        match self {
            Kind::Am => am::write(tenant, realm, script, confirmed_prod).await,
            Kind::IdmEndpoint => idm::write(tenant, realm, script, confirmed_prod).await,
            Kind::IdmSchedule => schedule::write(tenant, realm, script, confirmed_prod).await,
        }
    }

    pub async fn delete(
        self,
        tenant: &str,
        realm: &str,
        id: &str,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        match self {
            Kind::Am => am::delete(tenant, realm, id, confirmed_prod).await,
            Kind::IdmEndpoint => idm::delete(tenant, realm, id, confirmed_prod).await,
            Kind::IdmSchedule => schedule::delete(tenant, realm, id, confirmed_prod).await,
        }
    }

    // ----- pure mapping (delegates to the per-kind module) ---------------

    /// Decode the editable source bytes out of a raw config object.
    pub fn decode_source(self, raw: &serde_json::Value) -> Result<Vec<u8>> {
        match self {
            Kind::Am => am::decode_source(raw),
            Kind::IdmEndpoint => idm::decode_source(raw),
            Kind::IdmSchedule => schedule::decode_source(raw),
        }
    }

    /// Merge edited source bytes back into a raw config object in place.
    pub fn encode_source(self, raw: &mut serde_json::Value, source: &[u8]) -> Result<()> {
        match self {
            Kind::Am => am::encode_source(raw, source),
            Kind::IdmEndpoint => idm::encode_source(raw, source),
            Kind::IdmSchedule => schedule::encode_source(raw, source),
        }
    }

    /// Whether this kind is realm-scoped (AM scripts live per realm; IDM
    /// endpoints are tenant-global). Lets the engine key the workspace path,
    /// snapshot, and manifest on realm without matching on the enum itself.
    pub fn realm_scoped(self) -> bool {
        matches!(self, Kind::Am)
    }

    /// Workspace-relative path of the source file. AM: `am/<realm>/<type>/Foo.cjs`;
    /// IDM: `idm/endpoint/foo.cjs` (realm ignored).
    pub fn workspace_subpath(self, r: &RemoteRef, realm: &str) -> PathBuf {
        match self {
            Kind::Am => am::workspace_subpath(r, realm),
            Kind::IdmEndpoint => idm::workspace_subpath(r),
            Kind::IdmSchedule => schedule::workspace_subpath(r),
        }
    }

    /// Path (relative to `.aic-sync/configs/`) for the cached raw config. AM is
    /// realm-keyed so same-named scripts in alpha/bravo don't collide.
    pub fn config_subpath(self, r: &RemoteRef, realm: &str) -> PathBuf {
        match self {
            Kind::Am => am::config_subpath(r, realm),
            Kind::IdmEndpoint => idm::config_subpath(r),
            Kind::IdmSchedule => schedule::config_subpath(r),
        }
    }

    /// Additional generated files (workspace-relative path → contents), e.g.
    /// the ES-module wrapper for AM `LIBRARY` scripts. Empty for most kinds.
    pub fn extra_files(self, r: &RemoteRef, realm: &str) -> Vec<(PathBuf, String)> {
        match self {
            Kind::Am => am::extra_files(r, realm),
            Kind::IdmEndpoint => idm::extra_files(r),
            Kind::IdmSchedule => schedule::extra_files(r),
        }
    }
}

/// A lightweight remote identity — enough to locate a script and place it in
/// the workspace, without holding its (possibly large) body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRef {
    pub kind: Kind,
    /// Wire id used for fetch/write/delete (AM: UUID; IDM: `endpoint/<name>`).
    pub id: String,
    /// Human name used for the workspace filename.
    pub name: String,
    /// AM script `context` (routes lib/oidc/src); `None` for IDM endpoints.
    pub context: Option<String>,
    /// Product-shipped default (AM `default:true`); avoid clobbering.
    pub is_default: bool,
}

/// A fully-fetched script: identity plus the raw config object exactly as it
/// lives on the wire (kept verbatim so a push round-trips every field).
#[derive(Debug, Clone)]
pub struct RemoteScript {
    pub reference: RemoteRef,
    pub raw_config: serde_json::Value,
}

/// The first segment of a full-name addressing a script — `alpha/`, `bravo/`,
/// `endpoint/`, `schedule/`. Each prefix uniquely maps to a (kind, realm)
/// bucket (realm for AM, kind for the realmless IDM kinds), because AIC's realm
/// set is fixed and disjoint from the IDM kind names. So `bravo/Foo`,
/// `endpoint/foo`, `schedule/Job` fully identify a script without `--kind` /
/// `--realm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub kind: Kind,
    /// `Some(realm)` for AM; `None` for tenant-global IDM kinds.
    pub realm: Option<String>,
}

impl Namespace {
    /// Parse a prefix (`alpha`/`bravo`/`endpoint`/`schedule`).
    pub fn parse(prefix: &str) -> Option<Namespace> {
        match prefix {
            "alpha" | "bravo" => Some(Namespace {
                kind: Kind::Am,
                realm: Some(prefix.to_string()),
            }),
            "endpoint" => Some(Namespace { kind: Kind::IdmEndpoint, realm: None }),
            "schedule" => Some(Namespace { kind: Kind::IdmSchedule, realm: None }),
            _ => None,
        }
    }

    /// Every namespace — used to expand "act on everything".
    pub fn all() -> Vec<Namespace> {
        vec![
            Namespace { kind: Kind::Am, realm: Some("alpha".to_string()) },
            Namespace { kind: Kind::Am, realm: Some("bravo".to_string()) },
            Namespace { kind: Kind::IdmEndpoint, realm: None },
            Namespace { kind: Kind::IdmSchedule, realm: None },
        ]
    }

    /// The prefix label (`alpha`/`bravo`/`endpoint`/`schedule`).
    pub fn label(&self) -> &str {
        match self.kind {
            Kind::Am => self.realm.as_deref().unwrap_or("am"),
            Kind::IdmEndpoint => "endpoint",
            Kind::IdmSchedule => "schedule",
        }
    }

    /// Realm string for the engine (empty + ignored for IDM kinds).
    pub fn realm_arg(&self) -> &str {
        self.realm.as_deref().unwrap_or("")
    }
}

/// Render a script's full-name (`<prefix>/<name>`) for display / copy-paste.
pub fn full_name(kind: Kind, realm: Option<&str>, name: &str) -> String {
    let prefix = match kind {
        Kind::Am => realm.unwrap_or("am"),
        Kind::IdmEndpoint => "endpoint",
        Kind::IdmSchedule => "schedule",
    };
    format!("{prefix}/{name}")
}

#[cfg(test)]
mod ns_tests {
    use super::*;

    #[test]
    fn prefixes_map_to_kind_and_realm() {
        assert_eq!(Namespace::parse("bravo").unwrap().kind, Kind::Am);
        assert_eq!(Namespace::parse("bravo").unwrap().realm.as_deref(), Some("bravo"));
        assert_eq!(Namespace::parse("endpoint").unwrap().kind, Kind::IdmEndpoint);
        assert_eq!(Namespace::parse("schedule").unwrap().kind, Kind::IdmSchedule);
        assert!(Namespace::parse("endpoint").unwrap().realm.is_none());
        assert!(Namespace::parse("nope").is_none());
    }

    #[test]
    fn full_names_render() {
        assert_eq!(full_name(Kind::Am, Some("bravo"), "Foo"), "bravo/Foo");
        assert_eq!(full_name(Kind::IdmEndpoint, None, "bar"), "endpoint/bar");
        assert_eq!(full_name(Kind::IdmSchedule, None, "Job"), "schedule/Job");
    }
}
