//! TUI-free delete-plan types and the sharing guard.
//!
//! Slice B (CLI) and slice C (TUI) populate [`Inventory`] from disk and call
//! [`plan`]; they must not re-decide what is safe. The planner never touches
//! the filesystem.

use crate::config::{CredentialSource, Tenant, tenant_file_name};

/// Local artifact that offboarding can remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// The private JWK in the vault. The service *account* itself cannot be
    /// deleted by `aic` at all — an SA bearer gets 403 on
    /// `DELETE /openidm/managed/svcacct/{id}` (`docs/api/00-auth.md`) — so this
    /// purges our local handle and [`ManualCleanup`] reports the `sa_id`.
    ServiceAccountJwk,
    /// The stored api-key pair. Same shape as the SA: `DELETE /keys/{id}` needs
    /// an admin-user bearer (`docs/api/08-logs.md`), so the remote key survives
    /// and is reported for console cleanup.
    LogApiKey,
    /// **The only target with a remote side.** Purging it also removes this
    /// install's kid from the realm's default Trusted JWT Issuer, which is
    /// shared: one issuer, one key per install. Never delete the issuer itself.
    IssuerSigningKey,
    LogsDatabase,
    IdmStore,
    /// `workspace/<tenant>/` — **contains** [`TargetKind::SyncState`]. Purging
    /// the workspace takes `.aic-sync/` with it whatever that target says, so a
    /// caller must not present the two as independent choices.
    Workspace,
    /// `workspace/<tenant>/.aic-sync/`. Separable only in the keep-my-scripts
    /// case; implied whenever [`TargetKind::Workspace`] is purged.
    SyncState,
    UndoLog,
}

impl TargetKind {
    /// Plan order. [`plan`] emits every kind exactly once in this sequence.
    pub const ALL: [TargetKind; 8] = [
        TargetKind::ServiceAccountJwk,
        TargetKind::LogApiKey,
        TargetKind::IssuerSigningKey,
        TargetKind::LogsDatabase,
        TargetKind::IdmStore,
        TargetKind::Workspace,
        TargetKind::SyncState,
        TargetKind::UndoLog,
    ];
}

/// What the planner decided for one target.
///
/// These three arms stay distinct at the call site: `Absent` is not an
/// unticked checkbox, `Refused` is not a default-off offer, and
/// `Offered { default_on: false }` is a real choice the user can turn on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetDecision {
    /// Nothing is stored. Do not offer a choice.
    Absent,
    /// A surviving tenant still depends on this resource.
    Refused { reason: String },
    /// Present and exclusive to the departing tenant.
    Offered { default_on: bool },
}

/// One target in a [`DeletePlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTarget {
    pub kind: TargetKind,
    pub decision: TargetDecision,
}

/// Console-side leftovers this feature cannot delete.
///
/// Reported only when no survivor shares the identifier: recommending a
/// console delete of a shared SA or log key would break the tenant the user
/// meant to keep.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManualCleanup {
    pub sa_id: Option<String>,
    pub api_key_id: Option<String>,
}

/// The complete, I/O-free decision for removing one tenant entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlan {
    pub tenant_name: String,
    pub targets: Vec<PlannedTarget>,
    pub manual: ManualCleanup,
}

impl DeletePlan {
    /// Decision for `kind`. Every plan contains each [`TargetKind`] once.
    pub fn decision(&self, kind: TargetKind) -> &TargetDecision {
        &self
            .targets
            .iter()
            .find(|target| target.kind == kind)
            .expect("DeletePlan always includes every TargetKind")
            .decision
    }
}

/// What exists locally for the tenant being removed.
///
/// Slice B probes disk/vault and fills this in. The planner never touches
/// the filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// Private SA JWK present in the vault for this tenant name.
    pub service_account_jwk: bool,
    /// Stored log API key id, if a pair is present.
    pub log_api_key_id: Option<String>,
    /// This install's Trusted JWT kid, if a key record is present.
    pub issuer_kid: Option<String>,
    pub logs_database: bool,
    pub idm_store: bool,
    pub workspace: bool,
    pub sync_state: bool,
    pub undo_entries: bool,
}

/// A tenant entry that will remain after the delete, plus the credential
/// identities it currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survivor {
    pub name: String,
    pub base_url: String,
    pub sa_id: Option<String>,
    pub log_api_key_id: Option<String>,
    pub issuer_kid: Option<String>,
}

impl Survivor {
    pub fn from_tenant(
        tenant: &Tenant,
        log_api_key_id: Option<String>,
        issuer_kid: Option<String>,
    ) -> Self {
        Self {
            name: tenant.name.clone(),
            base_url: tenant.base_url.clone(),
            sa_id: tenant.sa_id.clone(),
            log_api_key_id,
            issuer_kid,
        }
    }
}

/// Decide what is safe to remove for `tenant`.
///
/// Sharing is resource identity, not tenant name: same `sa_id`, same log
/// `api_key_id`, same (`base_url`, kid), or a colliding sanitised file name.
pub fn plan(tenant: &Tenant, inventory: &Inventory, survivors: &[Survivor]) -> DeletePlan {
    DeletePlan {
        tenant_name: tenant.name.clone(),
        targets: TargetKind::ALL
            .into_iter()
            .map(|kind| PlannedTarget {
                kind,
                decision: decide(kind, tenant, inventory, survivors),
            })
            .collect(),
        manual: manual_cleanup(tenant, inventory, survivors),
    }
}

fn decide(
    kind: TargetKind,
    tenant: &Tenant,
    inventory: &Inventory,
    survivors: &[Survivor],
) -> TargetDecision {
    match kind {
        TargetKind::ServiceAccountJwk => decide_sa(tenant, inventory, survivors),
        TargetKind::LogApiKey => decide_log_key(tenant, inventory, survivors),
        TargetKind::IssuerSigningKey => decide_issuer(tenant, inventory, survivors),
        TargetKind::LogsDatabase => {
            decide_store_path(inventory.logs_database, &tenant.name, survivors)
        }
        TargetKind::IdmStore => decide_store_path(inventory.idm_store, &tenant.name, survivors),
        TargetKind::Workspace => decide_named_path(inventory.workspace),
        TargetKind::SyncState => decide_named_path(inventory.sync_state),
        TargetKind::UndoLog => decide_named_path(inventory.undo_entries),
    }
}

fn decide_sa(tenant: &Tenant, inventory: &Inventory, survivors: &[Survivor]) -> TargetDecision {
    if !inventory.service_account_jwk {
        return TargetDecision::Absent;
    }
    if let Some(survivor) = shared_by(nonempty(tenant.sa_id.as_deref()), survivors, |s| {
        nonempty(s.sa_id.as_deref())
    }) {
        return refused(survivor, "still uses this service account");
    }
    offered(tenant.provenance.service_account)
}

fn decide_log_key(
    tenant: &Tenant,
    inventory: &Inventory,
    survivors: &[Survivor],
) -> TargetDecision {
    let Some(id) = nonempty(inventory.log_api_key_id.as_deref()) else {
        return TargetDecision::Absent;
    };
    if let Some(survivor) = shared_by(Some(id), survivors, |s| {
        nonempty(s.log_api_key_id.as_deref())
    }) {
        return refused(survivor, "still uses this log API key");
    }
    offered(tenant.provenance.log_key)
}

fn decide_issuer(tenant: &Tenant, inventory: &Inventory, survivors: &[Survivor]) -> TargetDecision {
    let Some(kid) = nonempty(inventory.issuer_kid.as_deref()) else {
        return TargetDecision::Absent;
    };
    if let Some(survivor) = survivors.iter().find(|survivor| {
        nonempty(survivor.issuer_kid.as_deref()) == Some(kid)
            && survivor.base_url == tenant.base_url
    }) {
        return refused(survivor, "still publishes this issuer signing key");
    }
    TargetDecision::Offered { default_on: true }
}

/// A store whose file name is the *sanitised* tenant name.
///
/// `logs/db.rs` and `idmstore/state.rs` both name their file with
/// [`tenant_file_name`], which is not injective — `a b` and `a_b` resolve to one
/// file — so a collision here really would destroy a survivor's store.
fn decide_store_path(present: bool, tenant_name: &str, survivors: &[Survivor]) -> TargetDecision {
    if !present {
        return TargetDecision::Absent;
    }
    let file = tenant_file_name(tenant_name);
    if let Some(survivor) = survivors
        .iter()
        .find(|survivor| tenant_file_name(&survivor.name) == file)
    {
        return TargetDecision::Refused {
            reason: format!(
                "tenant {} shares the sanitised file name {file}",
                survivor.name
            ),
        };
    }
    TargetDecision::Offered { default_on: true }
}

/// An artifact addressed by the *raw* tenant name: `workspace/<tenant>/`, the
/// `.aic-sync/` tree inside it, and undo entries, which store the name verbatim.
///
/// These deliberately do **not** borrow [`decide_store_path`]'s collision
/// refusal. Tenant names are exactly unique — `onboard::common` matches an
/// existing name exactly, so no two entries can differ only by sanitisation —
/// which makes a collision here impossible and the refusal doubly wrong: it
/// would strand the directory *and* tell the user a survivor still needed it.
fn decide_named_path(present: bool) -> TargetDecision {
    if present {
        TargetDecision::Offered { default_on: true }
    } else {
        TargetDecision::Absent
    }
}

fn manual_cleanup(tenant: &Tenant, inventory: &Inventory, survivors: &[Survivor]) -> ManualCleanup {
    let sa_id = nonempty(tenant.sa_id.as_deref())
        .filter(|id| {
            survivors
                .iter()
                .all(|survivor| nonempty(survivor.sa_id.as_deref()) != Some(*id))
        })
        .map(str::to_string);
    let api_key_id = nonempty(inventory.log_api_key_id.as_deref())
        .filter(|id| {
            survivors
                .iter()
                .all(|survivor| nonempty(survivor.log_api_key_id.as_deref()) != Some(*id))
        })
        .map(str::to_string);
    ManualCleanup { sa_id, api_key_id }
}

fn offered(source: Option<CredentialSource>) -> TargetDecision {
    TargetDecision::Offered {
        default_on: !matches!(source, Some(CredentialSource::External)),
    }
}

fn refused(survivor: &Survivor, because: &str) -> TargetDecision {
    TargetDecision::Refused {
        reason: format!("tenant {} {because}", survivor.name),
    }
}

fn shared_by<'a>(
    id: Option<&str>,
    survivors: &'a [Survivor],
    of: impl Fn(&Survivor) -> Option<&str>,
) -> Option<&'a Survivor> {
    let id = id?;
    survivors.iter().find(|survivor| of(survivor) == Some(id))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::config::{CredentialSource, Provenance, Tenant, TenantTheme};

    use super::*;

    const UAT_URL: &str = "https://tenant.example.com";
    const UAT_SA: &str = "2f1882d0-df7b-4067-8b58-03fda365acf8";
    const DUP_SA: &str = "b55e7f59-3fc1-4512-9843-8925cff63e90";
    const SHARED_LOG_KEY: &str = "shared-log-key";

    fn tenant(name: &str, base_url: &str, sa_id: Option<&str>) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: base_url.into(),
            theme: TenantTheme::Sandbox,
            sa_id: sa_id.map(str::to_string),
            scopes: Vec::new(),
            provenance: Provenance::default(),
        }
    }

    fn survivor(
        name: &str,
        base_url: &str,
        sa_id: Option<&str>,
        log_api_key_id: Option<&str>,
        issuer_kid: Option<&str>,
    ) -> Survivor {
        Survivor {
            name: name.into(),
            base_url: base_url.into(),
            sa_id: sa_id.map(str::to_string),
            log_api_key_id: log_api_key_id.map(str::to_string),
            issuer_kid: issuer_kid.map(str::to_string),
        }
    }

    fn present() -> Inventory {
        Inventory {
            service_account_jwk: true,
            log_api_key_id: Some(SHARED_LOG_KEY.into()),
            issuer_kid: Some("kid-uat".into()),
            logs_database: true,
            idm_store: true,
            workspace: true,
            sync_state: true,
            undo_entries: true,
        }
    }

    fn offered_on() -> TargetDecision {
        TargetDecision::Offered { default_on: true }
    }

    fn offered_off() -> TargetDecision {
        TargetDecision::Offered { default_on: false }
    }

    #[test]
    fn plan_includes_every_target_kind_exactly_once() {
        // Dropping a TargetKind from ALL, or emitting it twice, lets a
        // later surface silently skip a purge the user never saw.
        let plan = plan(&tenant("uat", UAT_URL, Some(DUP_SA)), &present(), &[]);
        let kinds: Vec<_> = plan.targets.iter().map(|target| target.kind).collect();
        assert_eq!(kinds, TargetKind::ALL);
    }

    #[test]
    fn two_uat_distinct_sa_is_offered_shared_log_key_is_refused() {
        // Matching on tenant name or on base_url (instead of sa_id / api_key_id)
        // refuses the distinct SA and offers the shared log key.
        let departing = tenant("UAT", UAT_URL, Some(DUP_SA));
        let keep = survivor("uat", UAT_URL, Some(UAT_SA), Some(SHARED_LOG_KEY), None);
        let plan = plan(&departing, &present(), &[keep]);

        assert_eq!(
            plan.decision(TargetKind::ServiceAccountJwk),
            &offered_on(),
            "distinct sa_id must be offered"
        );
        match plan.decision(TargetKind::LogApiKey) {
            TargetDecision::Refused { reason } => {
                assert!(
                    reason.contains("uat"),
                    "refused log key must name the survivor: {reason}"
                );
            }
            other => panic!("shared log key must be refused, got {other:?}"),
        }
        assert_eq!(plan.manual.sa_id.as_deref(), Some(DUP_SA));
        assert_eq!(plan.manual.api_key_id, None);
    }

    #[test]
    fn shared_sa_id_is_refused_even_when_names_differ() {
        // Comparing tenant names rather than sa_id offers a purge that
        // strands the survivor.
        let departing = tenant("UAT", UAT_URL, Some(UAT_SA));
        let keep = survivor("uat", UAT_URL, Some(UAT_SA), None, None);
        let plan = plan(&departing, &present(), &[keep]);

        match plan.decision(TargetKind::ServiceAccountJwk) {
            TargetDecision::Refused { reason } => {
                assert!(reason.contains("uat"), "{reason}");
            }
            other => panic!("shared sa_id must be refused, got {other:?}"),
        }
        assert_eq!(plan.manual.sa_id, None);
    }

    #[test]
    fn issuer_key_is_refused_only_for_same_base_url_and_kid() {
        // Sharing on tenant name, or on kid alone, either refuses a key that
        // belongs to a different AIC tenant or offers one that revokes the
        // survivor's published kid.
        let departing = tenant("UAT", UAT_URL, Some(DUP_SA));
        let mut inventory = present();
        inventory.issuer_kid = Some("shared-kid".into());

        let same_tenant = survivor("uat", UAT_URL, Some(UAT_SA), None, Some("shared-kid"));
        match plan(&departing, &inventory, &[same_tenant]).decision(TargetKind::IssuerSigningKey) {
            TargetDecision::Refused { reason } => assert!(reason.contains("uat"), "{reason}"),
            other => panic!("same base_url + kid must be refused, got {other:?}"),
        }

        let other_host = survivor(
            "prod",
            "https://openam-other.example",
            None,
            None,
            Some("shared-kid"),
        );
        assert_eq!(
            plan(&departing, &inventory, &[other_host]).decision(TargetKind::IssuerSigningKey),
            &offered_on()
        );

        let other_kid = survivor("uat", UAT_URL, Some(UAT_SA), None, Some("other-kid"));
        assert_eq!(
            plan(&departing, &inventory, &[other_kid]).decision(TargetKind::IssuerSigningKey),
            &offered_on()
        );
    }

    #[test]
    fn provenance_sets_the_default_for_offered_credentials() {
        // Collapsing None into External (or Created) flips the unknown row.
        // Skipping the External arm defaults a pasted credential on.
        for (source, expected) in [
            (Some(CredentialSource::Created), true),
            (Some(CredentialSource::External), false),
            (None, true),
        ] {
            let mut departing = tenant("UAT", UAT_URL, Some(DUP_SA));
            departing.provenance = Provenance {
                service_account: source,
                log_key: source,
            };
            let plan = plan(&departing, &present(), &[]);
            assert_eq!(
                plan.decision(TargetKind::ServiceAccountJwk),
                &TargetDecision::Offered {
                    default_on: expected
                },
                "sa source {source:?}"
            );
            assert_eq!(
                plan.decision(TargetKind::LogApiKey),
                &TargetDecision::Offered {
                    default_on: expected
                },
                "log source {source:?}"
            );
        }
    }

    #[test]
    fn missing_artifacts_are_absent_not_offered_off() {
        // Mapping "not stored" onto Offered { default_on: false } makes
        // every row below fail: the user must not see a checkbox for
        // something that is not there.
        let plan = plan(
            &tenant("UAT", UAT_URL, Some(DUP_SA)),
            &Inventory::default(),
            &[],
        );
        for kind in TargetKind::ALL {
            assert_eq!(
                plan.decision(kind),
                &TargetDecision::Absent,
                "{kind:?} must be absent when nothing is stored"
            );
        }
        assert_eq!(plan.manual.sa_id.as_deref(), Some(DUP_SA));
        assert_eq!(plan.manual.api_key_id, None);
    }

    #[test]
    fn sanitised_name_collision_refuses_only_the_sanitised_stores() {
        // Both halves are load-bearing, and they fail to opposite edits.
        // Comparing raw names in `decide_store_path` offers deletion of `a b`'s
        // DuckDB file while `a_b` still needs it. Routing the raw-name targets
        // through `decide_store_path` instead strands `workspace/a b/` and
        // blames a survivor that never shared it.
        let departing = tenant("a b", UAT_URL, Some(DUP_SA));
        let keep = survivor("a_b", UAT_URL, Some(UAT_SA), None, None);
        let plan = plan(&departing, &present(), &[keep]);

        for kind in [TargetKind::LogsDatabase, TargetKind::IdmStore] {
            match plan.decision(kind) {
                TargetDecision::Refused { reason } => {
                    assert!(
                        reason.contains("a_b"),
                        "{kind:?} must name the survivor: {reason}"
                    );
                }
                other => panic!("{kind:?} must be refused on sanitised collision, got {other:?}"),
            }
        }

        for kind in [
            TargetKind::Workspace,
            TargetKind::SyncState,
            TargetKind::UndoLog,
        ] {
            assert_eq!(
                plan.decision(kind),
                &offered_on(),
                "{kind:?} is addressed by the raw tenant name and cannot collide"
            );
        }
        assert_eq!(plan.decision(TargetKind::ServiceAccountJwk), &offered_on());
    }

    #[test]
    fn absent_overrides_a_share_that_cannot_be_deleted() {
        // Offering (or refusing) a credential that is not stored collapses
        // Absent into a checkbox. Presence is checked before sharing.
        let departing = tenant("UAT", UAT_URL, Some(UAT_SA));
        let keep = survivor("uat", UAT_URL, Some(UAT_SA), Some(SHARED_LOG_KEY), None);
        let plan = plan(&departing, &Inventory::default(), &[keep]);
        assert_eq!(
            plan.decision(TargetKind::ServiceAccountJwk),
            &TargetDecision::Absent
        );
        assert_eq!(
            plan.decision(TargetKind::LogApiKey),
            &TargetDecision::Absent
        );
    }

    #[test]
    fn blank_identifiers_do_not_count_as_shared() {
        // Treating "" as an id makes two log-only tenants refuse each
        // other's empty SA, and two SA-only tenants refuse empty log keys.
        let departing = tenant("UAT", UAT_URL, Some(""));
        let keep = survivor("uat", UAT_URL, Some(""), Some(""), Some(""));
        let mut inventory = present();
        inventory.service_account_jwk = true;
        inventory.log_api_key_id = Some(String::new());
        inventory.issuer_kid = Some("kid".into());
        let plan = plan(&departing, &inventory, &[keep]);
        assert_eq!(plan.decision(TargetKind::ServiceAccountJwk), &offered_on());
        assert_eq!(
            plan.decision(TargetKind::LogApiKey),
            &TargetDecision::Absent
        );
        assert_eq!(plan.decision(TargetKind::IssuerSigningKey), &offered_on());
    }

    #[test]
    fn offered_off_is_not_absent_or_refused() {
        // Pinning the three-way distinction: External + present must stay
        // Offered { default_on: false }, not Absent and not Refused.
        let mut departing = tenant("UAT", UAT_URL, Some(DUP_SA));
        departing.provenance.service_account = Some(CredentialSource::External);
        let plan = plan(&departing, &present(), &[]);
        assert_eq!(plan.decision(TargetKind::ServiceAccountJwk), &offered_off());
    }
}
