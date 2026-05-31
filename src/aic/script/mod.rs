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
pub mod sync;
pub mod workspace;

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The script-like resource families we sync. The single dispatch point for
/// all AM-vs-IDM differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// AM scripts: `/am/json{realm}/scripts/{uuid}`, base64 `script` body.
    #[serde(rename = "am")]
    Am,
    /// IDM custom endpoints: `/openidm/config/endpoint/{name}`, plaintext `source`.
    #[serde(rename = "idm")]
    IdmEndpoint,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Am => "am",
            Kind::IdmEndpoint => "idm",
        }
    }

    /// Parse a `--kind` CLI value. Accepts a few friendly aliases.
    pub fn parse(s: &str) -> Option<Kind> {
        match s.to_ascii_lowercase().as_str() {
            "am" | "script" | "scripts" => Some(Kind::Am),
            "idm" | "endpoint" | "endpoints" | "idm-endpoint" => Some(Kind::IdmEndpoint),
            _ => None,
        }
    }

    pub fn all() -> &'static [Kind] {
        &[Kind::Am, Kind::IdmEndpoint]
    }

    // ----- async I/O (delegates to the per-kind module) ------------------

    pub async fn list(self, tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
        match self {
            Kind::Am => am::list(tenant, realm).await,
            Kind::IdmEndpoint => idm::list(tenant, realm).await,
        }
    }

    pub async fn fetch(self, tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
        match self {
            Kind::Am => am::fetch(tenant, realm, id).await,
            Kind::IdmEndpoint => idm::fetch(tenant, realm, id).await,
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
        }
    }

    // ----- pure mapping (delegates to the per-kind module) ---------------

    /// Decode the editable source bytes out of a raw config object.
    pub fn decode_source(self, raw: &serde_json::Value) -> Result<Vec<u8>> {
        match self {
            Kind::Am => am::decode_source(raw),
            Kind::IdmEndpoint => idm::decode_source(raw),
        }
    }

    /// Merge edited source bytes back into a raw config object in place.
    pub fn encode_source(self, raw: &mut serde_json::Value, source: &[u8]) -> Result<()> {
        match self {
            Kind::Am => am::encode_source(raw, source),
            Kind::IdmEndpoint => idm::encode_source(raw, source),
        }
    }

    /// Workspace-relative path of the source file (e.g. `am/src/Foo.cjs`).
    pub fn workspace_subpath(self, r: &RemoteRef) -> PathBuf {
        match self {
            Kind::Am => am::workspace_subpath(r),
            Kind::IdmEndpoint => idm::workspace_subpath(r),
        }
    }

    /// Filename for the cached raw config under `.aic-sync/snapshots/`.
    pub fn config_filename(self, r: &RemoteRef) -> String {
        match self {
            Kind::Am => am::config_filename(r),
            Kind::IdmEndpoint => idm::config_filename(r),
        }
    }

    /// Additional generated files (workspace-relative path → contents), e.g.
    /// the ES-module wrapper for AM `LIBRARY` scripts. Empty for most kinds.
    pub fn extra_files(self, r: &RemoteRef) -> Vec<(PathBuf, String)> {
        match self {
            Kind::Am => am::extra_files(r),
            Kind::IdmEndpoint => idm::extra_files(r),
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
