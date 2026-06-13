//! Scripts feature vertical: pull/push AIC scripts to a local typed workspace,
//! with content-based conflict detection (CLAUDE.md §5 — scripts have no `_rev`).
//!
//! Architecture: the sync **engine** (`sync`) and **workspace scaffolding**
//! (`workspace`) are entirely kind-agnostic. Everything that differs between
//! AM scripts (realm-scoped, base64 `script`, context→dir routing) and IDM
//! endpoints (`/openidm/config/endpoint`, plaintext `source`) lives behind the
//! [`Kind`] enum, which delegates to the `am` / `idm` modules. The engine
//! never matches on `Kind` — to add journeys later, add a variant + a module
//! and wire the dispatch arms here; nothing in `sync`/`workspace` changes.
//!
//! Feature seams: `mod.rs` + engine modules, `screen`, `view`, `cli`, and
//! `workspace`/templates. API ground truth: `docs/api/04-scripts.md`,
//! `docs/api/11-idm-endpoints.md`, `docs/api/12-script-bindings-matrix.md`,
//! and `docs/api/13-script-contexts.md`.

pub mod am;
pub mod cli;
pub mod idm;
pub mod managed_hooks;
pub mod managed_types;
pub mod schedule;
pub mod screen;
pub mod sync;
pub mod view;
pub mod workspace;

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The script-like resource families we sync. The single dispatch point for
/// all AM-vs-IDM differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
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
    /// IDM managed-object hooks: scripts embedded in the single
    /// `/openidm/config/managed` document at `objects[i].<hookKey>.source`.
    /// Addressed as `managed/<object>.<hookKey>`; push is a fresh
    /// read-modify-write of the shared document. See `managed_hooks`.
    #[serde(rename = "managed-hook")]
    #[value(name = "managed-hook", alias = "hooks")]
    IdmManagedHook,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Am => "am",
            Kind::IdmEndpoint => "idm",
            Kind::IdmSchedule => "schedule",
            Kind::IdmManagedHook => "managed-hook",
        }
    }

    pub fn all() -> &'static [Kind] {
        &[
            Kind::Am,
            Kind::IdmEndpoint,
            Kind::IdmSchedule,
            Kind::IdmManagedHook,
        ]
    }

    // ----- async I/O (delegates to the per-kind module) ------------------

    pub async fn list(self, tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
        match self {
            Kind::Am => am::list(tenant, realm).await,
            Kind::IdmEndpoint => idm::list(tenant, realm).await,
            Kind::IdmSchedule => schedule::list(tenant, realm).await,
            Kind::IdmManagedHook => managed_hooks::list(tenant, realm).await,
        }
    }

    pub async fn fetch(self, tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
        match self {
            Kind::Am => am::fetch(tenant, realm, id).await,
            Kind::IdmEndpoint => idm::fetch(tenant, realm, id).await,
            Kind::IdmSchedule => schedule::fetch(tenant, realm, id).await,
            Kind::IdmManagedHook => managed_hooks::fetch(tenant, realm, id).await,
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
            Kind::IdmManagedHook => {
                managed_hooks::write(tenant, realm, script, confirmed_prod).await
            }
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
            Kind::IdmManagedHook => managed_hooks::delete(tenant, realm, id, confirmed_prod).await,
        }
    }

    // ----- pure mapping (delegates to the per-kind module) ---------------

    /// Decode the editable source bytes out of a raw config object.
    pub fn decode_source(self, raw: &serde_json::Value) -> Result<Vec<u8>> {
        match self {
            Kind::Am => am::decode_source(raw),
            Kind::IdmEndpoint => idm::decode_source(raw),
            Kind::IdmSchedule => schedule::decode_source(raw),
            Kind::IdmManagedHook => managed_hooks::decode_source(raw),
        }
    }

    /// Merge edited source bytes back into a raw config object in place.
    pub fn encode_source(self, raw: &mut serde_json::Value, source: &[u8]) -> Result<()> {
        match self {
            Kind::Am => am::encode_source(raw, source),
            Kind::IdmEndpoint => idm::encode_source(raw, source),
            Kind::IdmSchedule => schedule::encode_source(raw, source),
            Kind::IdmManagedHook => managed_hooks::encode_source(raw, source),
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
            Kind::IdmManagedHook => managed_hooks::workspace_subpath(r),
        }
    }

    /// Path (relative to `.aic-sync/configs/`) for the cached raw config. AM is
    /// realm-keyed so same-named scripts in alpha/bravo don't collide.
    pub fn config_subpath(self, r: &RemoteRef, realm: &str) -> PathBuf {
        match self {
            Kind::Am => am::config_subpath(r, realm),
            Kind::IdmEndpoint => idm::config_subpath(r),
            Kind::IdmSchedule => schedule::config_subpath(r),
            Kind::IdmManagedHook => managed_hooks::config_subpath(r),
        }
    }

    /// Additional generated files (workspace-relative path → contents), e.g.
    /// the ES-module wrapper for AM `LIBRARY` scripts. Empty for most kinds.
    pub fn extra_files(self, r: &RemoteRef, realm: &str) -> Vec<(PathBuf, String)> {
        match self {
            Kind::Am => am::extra_files(r, realm),
            Kind::IdmEndpoint => idm::extra_files(r),
            Kind::IdmSchedule => schedule::extra_files(r),
            Kind::IdmManagedHook => managed_hooks::extra_files(r),
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
    /// AM script `context` — routes the workspace folder; `None` for IDM.
    pub context: Option<String>,
    /// Product-shipped default (AM `default:true`); avoid clobbering.
    pub is_default: bool,
    /// AM script engine version (`"1.0"` legacy / `"2.0"` next-gen). The two
    /// scripted-decision-node generations share one `context`, so this is what
    /// splits them into separate folders. `None` for IDM (and pre-`evaluatorVersion`
    /// snapshots).
    #[serde(default)]
    pub evaluator_version: Option<String>,
}

/// A fully-fetched script: identity plus the raw config object exactly as it
/// lives on the wire (kept verbatim so a push round-trips every field).
#[derive(Debug, Clone)]
pub struct RemoteScript {
    pub reference: RemoteRef,
    pub raw_config: serde_json::Value,
}

/// The first segment of a full-name addressing a script — `alpha/`, `bravo/`,
/// `endpoint/`, `schedule/`, `managed/`. Each prefix uniquely maps to a (kind, realm)
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
            "endpoint" => Some(Namespace {
                kind: Kind::IdmEndpoint,
                realm: None,
            }),
            "schedule" => Some(Namespace {
                kind: Kind::IdmSchedule,
                realm: None,
            }),
            "managed" => Some(Namespace {
                kind: Kind::IdmManagedHook,
                realm: None,
            }),
            _ => None,
        }
    }

    /// Every namespace — used to expand "act on everything".
    pub fn all() -> Vec<Namespace> {
        vec![
            Namespace {
                kind: Kind::Am,
                realm: Some("alpha".to_string()),
            },
            Namespace {
                kind: Kind::Am,
                realm: Some("bravo".to_string()),
            },
            Namespace {
                kind: Kind::IdmEndpoint,
                realm: None,
            },
            Namespace {
                kind: Kind::IdmSchedule,
                realm: None,
            },
            Namespace {
                kind: Kind::IdmManagedHook,
                realm: None,
            },
        ]
    }

    /// The prefix label (`alpha`/`bravo`/`endpoint`/`schedule`).
    pub fn label(&self) -> &str {
        match self.kind {
            Kind::Am => self.realm.as_deref().unwrap_or("am"),
            Kind::IdmEndpoint => "endpoint",
            Kind::IdmSchedule => "schedule",
            Kind::IdmManagedHook => "managed",
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
        Kind::IdmManagedHook => "managed",
    };
    format!("{prefix}/{name}")
}

/// The group a script belongs to: `am` (alpha/bravo) or `idm` (endpoint,
/// schedule, managed). Scripts already cluster this way in the workspace tree
/// (`am/…` vs `idm/…`), so these are the natural coarse filter terms.
pub fn group_token(kind: Kind) -> &'static str {
    match kind {
        Kind::Am => "am",
        Kind::IdmEndpoint | Kind::IdmSchedule | Kind::IdmManagedHook => "idm",
    }
}

/// Whether a script matches a free-text filter `term` (used by `aic script
/// status`). Case-insensitive. The term may be:
///   - a **group** alias — exactly `am` or `idm` — matching the whole group;
///   - a **namespace** prefix (`alpha`, `endpoint`, …), a **full-name**
///     (`alpha/Email OTP`), or any **fragment** of the full-name (`Email`),
///     all matched as a substring of `<prefix>/<name>`.
/// An empty term matches everything.
pub fn matches_term(term: &str, kind: Kind, realm: Option<&str>, name: &str) -> bool {
    let term = term.trim().to_lowercase();
    if term.is_empty() {
        return true;
    }
    // `am`/`idm` are group selectors, not substrings — typing `am` means "all
    // AM scripts", never "scripts with 'am' in the name" (which would also
    // catch `saml`, `name`, …). Since the group alias already returns the
    // whole group, nothing is lost.
    if term == "am" || term == "idm" {
        return group_token(kind) == term;
    }
    full_name(kind, realm, name).to_lowercase().contains(&term)
}

#[cfg(test)]
mod ns_tests {
    use super::*;

    #[test]
    fn prefixes_map_to_kind_and_realm() {
        assert_eq!(Namespace::parse("bravo").unwrap().kind, Kind::Am);
        assert_eq!(
            Namespace::parse("bravo").unwrap().realm.as_deref(),
            Some("bravo")
        );
        assert_eq!(
            Namespace::parse("endpoint").unwrap().kind,
            Kind::IdmEndpoint
        );
        assert_eq!(
            Namespace::parse("schedule").unwrap().kind,
            Kind::IdmSchedule
        );
        assert!(Namespace::parse("endpoint").unwrap().realm.is_none());
        assert_eq!(
            Namespace::parse("managed").unwrap().kind,
            Kind::IdmManagedHook
        );
        assert!(Namespace::parse("managed").unwrap().realm.is_none());
        assert!(Namespace::parse("nope").is_none());
    }

    #[test]
    fn full_names_render() {
        assert_eq!(full_name(Kind::Am, Some("bravo"), "Foo"), "bravo/Foo");
        assert_eq!(full_name(Kind::IdmEndpoint, None, "bar"), "endpoint/bar");
        assert_eq!(full_name(Kind::IdmSchedule, None, "Job"), "schedule/Job");
        assert_eq!(
            full_name(Kind::IdmManagedHook, None, "alpha_user.onCreate"),
            "managed/alpha_user.onCreate"
        );
    }

    #[test]
    fn filter_term_matches_group_namespace_fullname_and_fragment() {
        let am = |t: &str| matches_term(t, Kind::Am, Some("alpha"), "Email OTP");
        let ep = |t: &str| matches_term(t, Kind::IdmEndpoint, None, "validateQueryFilter");
        let hook = |t: &str| matches_term(t, Kind::IdmManagedHook, None, "alpha_user.onCreate");

        // group aliases
        assert!(am("am"));
        assert!(!am("idm"));
        assert!(ep("idm"));
        assert!(hook("idm"));
        assert!(!ep("am"));

        // namespace prefixes (substring of the full-name)
        assert!(am("alpha"));
        assert!(!am("bravo"));
        assert!(ep("endpoint"));
        assert!(hook("managed"));

        // full-names and fragments, case-insensitively
        assert!(am("alpha/Email OTP"));
        assert!(am("email"));
        assert!(ep("validate"));
        assert!(hook("alpha_user.oncreate"));

        // `am` is a group, not a substring: it must NOT leak via "Email"/"name"
        assert!(!matches_term(
            "am",
            Kind::IdmEndpoint,
            None,
            "sendSamlAssertion"
        ));

        // empty term matches everything
        assert!(am(""));
        assert!(am("   "));
    }
}
