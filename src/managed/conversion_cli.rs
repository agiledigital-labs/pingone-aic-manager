//! Tenant and filesystem edge for relationship storage conversion. `map` and
//! promotion capture/restore share conversion's census, journal and restore
//! engine; all config writes still go through repair::apply_with. Upper
//! restore writes records only. No promotion lifecycle code is changed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use serde_json::{Value, json};

use super::conversion::{
    self, Capture, Census, FileSession, Link, Observation, Origin, Pair, State,
};
use super::{api, conversion_records, relink, repair, storage, verify};
use crate::cli::force::OperationForce;
use crate::cli::{WriteOk, ensure_prod_confirmed, print_json, tenant_config_for};
use crate::config::ProjectConfig;
use crate::{Error, Result};

#[derive(Args, Debug)]
pub struct Options {
    /// Report the plan without saving a capture or writing to the tenant.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub force: OperationForce,
    /// Confirm a write to a production-themed tenant.
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub json: bool,
}

pub struct Flags {
    pub dry_run: bool,
    pub force: bool,
    pub yes: bool,
    pub json: bool,
}
impl From<Options> for Flags {
    fn from(options: Options) -> Self {
        Self {
            dry_run: options.dry_run,
            force: options.force.operation(),
            yes: options.yes,
            json: options.json,
        }
    }
}

async fn load(path: &Path) -> Result<Capture> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || conversion::load(&path))
        .await
        .map_err(|error| Error::Config(error.to_string()))?
}

fn identity(capture: &Capture, tenant: Option<String>) -> Result<crate::config::Tenant> {
    let configured = tenant_config_for(tenant.or_else(|| Some(capture.tenant.clone())))?;
    if configured.name != capture.tenant
        || !configured
            .base_url
            .trim_end_matches('/')
            .eq_ignore_ascii_case(capture.base_url.trim_end_matches('/'))
    {
        return Err(Error::Config("capture belongs to a different tenant URL or name; do not restore it into this environment".into()));
    }
    Ok(configured)
}

fn column(pair: &Pair) -> &str {
    pair.mappings
        .first()
        .and_then(|entry| entry.value.get("ldapAttribute"))
        .and_then(Value::as_str)
        .unwrap_or("unknown reference column")
}

fn default_path() -> PathBuf {
    ProjectConfig::dir()
        .join("relationship-captures")
        .join(format!("{}.json", uuid::Uuid::new_v4()))
}

fn plan_text(capture: &Capture, path: &Path, operation: &str, saved: bool, switch: bool) -> String {
    let mut lines = vec![format!(
        "{operation}: {} pair(s), {} captured link(s); capture {}",
        capture.pairs.len(),
        capture.count(),
        path.display()
    )];
    lines.push(if saved {
        format!("  saved state: {:?}", capture.state)
    } else {
        "  planned capture; no file written".into()
    });
    for pair in &capture.pairs {
        lines.push(format!(
            "  {}: {} links",
            pair.forward.label(),
            pair.links.len()
        ));
    }
    lines.push(
        "  pause all link writers; no atomic lock exists across link reads and configuration"
            .into(),
    );
    if operation == "rollback-map" {
        lines.push("  preflight all mapping dependencies and removal prerequisites before any unlink; then submit unchanged repo.ds with backup/readback to prove config write access".into());
        lines.push("  remove captured mapped links first, then remove both mapping entries; original generic links become visible again".into());
        lines.push("  each removal is a record update and can invoke hooks, sync mappings and notifications".into());
    } else {
        if switch || operation == "capture-links before promotion" {
            lines.push(format!(
                "  {} links are invisible from the mapping switch/promotion until restore finishes",
                capture.count()
            ));
        } else {
            lines.push("  no mapping switch is planned; verify captured links already present and re-create only missing links".into());
        }
        if capture.state == State::RollingBack {
            lines.push("  resume restoration to reverse the interrupted rollback; the exact captured mappings must still be installed".into());
        }
        lines.push("  each re-create is a record update: budget for onUpdate/postUpdate hooks, sync mappings and notifications per link (exact firing counts are unmeasured)".into());
    }
    if switch && operation == "map" {
        for entry in &capture.pairs[0].mappings {
            lines.push(format!(
                "  map {}.{}: {}",
                entry.object, entry.key, entry.value
            ));
        }
    }
    lines.join("\n")
}

fn plan(capture: &Capture, path: &Path, operation: &str, saved: bool, switch: bool) {
    eprintln!("{}", plan_text(capture, path, operation, saved, switch));
}

fn consent(
    flags: &Flags,
    operation: &str,
    capture: &Capture,
) -> Result<Option<conversion::Permit>> {
    if flags.dry_run {
        return Ok(None);
    }
    let consented = flags.force
        || crate::cli::confirm_destructive(
            operation,
            &format!(
                "{operation} {} pair(s), {} links, with the printed visibility window and record-update effects? Keep link writers paused",
                capture.pairs.len(),
                capture.count()
            ),
            "--force",
        )?;
    conversion::authorize(flags.dry_run, consented)
        .map(Some)
        .ok_or_else(|| Error::Config(format!("{operation} not confirmed")))
}

fn output(flags: &Flags, value: &Value) -> Result<()> {
    if flags.json {
        print_json(value)?;
    }
    Ok(())
}

fn stopped(path: &Path, capture: &Capture, error: Error) -> Error {
    let resume = match capture.origin {
        Origin::InPlace => format!(
            "map {} --capture {} --force",
            capture.pairs[0].forward.label(),
            path.display()
        ),
        Origin::Promotion => format!("restore-links {} --force", path.display()),
    };
    Error::Config(format!(
        "{error}\nCapture retained at {}. Keep link writers paused. Correct the reported problem and re-run `aic managed relationship {resume}`; an in-place conversion can instead use rollback-map. No automatic rollback was attempted",
        path.display()
    ))
}

async fn census(tenant: &str, pair: &Pair) -> Result<Census> {
    let forward = api::all_records_with(tenant, &pair.forward.object, &pair.forward.key).await?;
    let target = pair
        .forward
        .target
        .strip_prefix("managed/")
        .ok_or_else(|| Error::Config("capture has a non-managed target".into()))?;
    let key = pair.reverse.as_ref().map_or("_id", |end| end.key.as_str());
    let target_records = api::all_records_with(tenant, target, key).await?;
    conversion::census(pair, &forward, &target_records)
}

async fn generic_snapshot(tenant: &str, pair: &Pair) -> Result<Census> {
    let current = census(tenant, pair).await?;
    let report = guard(tenant, pair, storage::Mode::Generic).await?;
    if let Some(edge) = current.edges.first() {
        for (end, id) in std::iter::once((&report.forward, edge.link.from_id.as_str())).chain(
            report
                .reverse
                .as_ref()
                .map(|end| (end, edge.link.to_id.as_str())),
        ) {
            let found = api::present_filter_finds(tenant, &end.object, id, &end.key).await?;
            if verify::Runtime::judge(end, found) != verify::Runtime::GenericConfirmed {
                return Err(Error::Config(format!(
                    "{}.{} runtime disagrees with generic repo.ds; run relationship verify and let configuration settle before capturing",
                    end.object, end.key
                )));
            }
        }
    }
    Ok(current)
}

async fn guard(tenant: &str, pair: &Pair, expected: storage::Mode) -> Result<verify::PairReport> {
    let managed = api::get_managed(tenant).await?;
    let repo = api::get_repo_ds(tenant).await?;
    conversion::guard(pair, &managed, &repo, expected)
}

async fn observe(tenant: &str, pair: &Pair, link: &Link) -> Result<Observation> {
    // Recur during a long restore: Q14 can remove a declaration after the
    // start-of-restore check succeeded.
    guard(tenant, pair, storage::Mode::Mapped).await?;
    let record = if pair.forward.array {
        api::far_end(
            tenant,
            &pair.forward.object,
            &link.from_id,
            &pair.forward.key,
            true,
            &link.to_id,
        )
        .await?
    } else {
        api::record_with(
            tenant,
            &pair.forward.object,
            &link.from_id,
            &pair.forward.key,
        )
        .await?
    }
    .ok_or_else(|| Error::Config(format!("source record {} no longer exists", link.from_id)))?;
    Ok(Observation {
        edges: relink::edges_of(&record, &pair.forward.key).map_err(Error::Config)?,
        rev: relink::rev_of(&record),
    })
}

fn edge(link: &Link) -> relink::Edge {
    relink::Edge {
        link: verify::Link {
            from_id: link.from_id.clone(),
            to_collection: link.to_collection.clone(),
            to_id: link.to_id.clone(),
            edge_id: None,
        },
        properties: link.properties.clone(),
    }
}

async fn check_edge(tenant: &str, pair: &Pair, link: &Link, edge: &relink::Edge) -> Result<()> {
    let report = guard(tenant, pair, storage::Mode::Mapped).await?;
    let found = api::present_filter_finds(
        tenant,
        &pair.forward.object,
        &link.from_id,
        &pair.forward.key,
    )
    .await?;
    if verify::Runtime::judge(&report.forward, found) != verify::Runtime::MappedConfirmed {
        return Err(Error::Config(format!(
            "{} has not loaded its mapped storage; keep writers paused and retry restore-links",
            pair.forward.label()
        )));
    }
    if let (Some(reverse), Some(far)) = (&pair.reverse, &report.reverse) {
        let record = api::far_end(
            tenant,
            &reverse.object,
            &link.to_id,
            &reverse.key,
            reverse.array,
            &link.from_id,
        )
        .await?;
        let links = record
            .as_ref()
            .map(|record| verify::links_of(record, &reverse.key))
            .transpose()
            .map_err(Error::Config)?;
        if verify::judge(
            &edge.link,
            &format!("managed/{}", pair.forward.object),
            links.as_deref(),
        ) != verify::LinkOutcome::Resolved
        {
            return Err(Error::Config(format!(
                "{} is not resolved from the far end; intent retained, retry restore-links",
                link.label()
            )));
        }
        let far_edges = record
            .as_ref()
            .map(|record| relink::edges_of(record, &reverse.key))
            .transpose()
            .map_err(Error::Config)?
            .unwrap_or_default();
        if !far_edges.iter().any(|far_edge| {
            far_edge.link.edge_id == edge.link.edge_id
                && far_edge.link.to_collection == format!("managed/{}", pair.forward.object)
                && far_edge.link.to_id == link.from_id
                && far_edge.properties
                    == *link.reverse_properties.as_ref().unwrap_or(&link.properties)
        }) {
            return Err(Error::Config(format!(
                "{} reverse custom _refProperties differ from capture; intent retained",
                link.label()
            )));
        }
        let found =
            api::present_filter_finds(tenant, &reverse.object, &link.to_id, &reverse.key).await?;
        if verify::Runtime::judge(far, found) != verify::Runtime::MappedConfirmed {
            return Err(Error::Config(format!(
                "{} has not loaded its mapped storage; retry restore-links",
                reverse.label()
            )));
        }
    } else {
        let target = link
            .to_collection
            .strip_prefix("managed/")
            .ok_or_else(|| Error::Config("invalid target".into()))?;
        if api::record_with(tenant, target, &link.to_id, "_id")
            .await?
            .is_none()
        {
            return Err(Error::Config(format!(
                "{} target no longer exists",
                link.label()
            )));
        }
    }
    Ok(())
}

async fn create(
    ok: &WriteOk<'_>,
    permit: &relink::RelinkPermit,
    pair: &Pair,
    link: &Link,
    rev: Option<&str>,
) -> Result<Option<String>> {
    guard(ok.tenant, pair, storage::Mode::Mapped).await?;
    if !pair.forward.array && rev.is_none() {
        return Err(Error::Config(
            "has-one source has no _rev; refusing an unconditional replacement".into(),
        ));
    }
    // A far has-one must not already hold another link. Read it regardless
    // of whether the near property's original preflight found it free.
    if let Some(reverse) = &pair.reverse {
        let record = api::far_end(
            ok.tenant,
            &reverse.object,
            &link.to_id,
            &reverse.key,
            reverse.array,
            &link.from_id,
        )
        .await?
        .ok_or_else(|| Error::Config(format!("target {} is absent", link.to_id)))?;
        let links = verify::links_of(&record, &reverse.key).map_err(Error::Config)?;
        if !reverse.array && !links.is_empty() {
            return Err(Error::Config(format!(
                "{}.{} is occupied; nothing displaced",
                link.to_id, reverse.key
            )));
        }
    }
    let write = relink::create_link(
        permit,
        &pair.forward.object,
        &pair.forward.key,
        pair.forward.array,
        &edge(link),
        rev,
    );
    let response =
        api::send_relink(ok.tenant, &pair.forward.object, &write, ok.confirmed_prod).await?;
    Ok(relink::written_edge(
        &response,
        &pair.forward.key,
        pair.forward.array,
    ))
}

struct TenantRead<'a>(&'a str);
impl conversion_records::Read for TenantRead<'_> {
    async fn config(&self) -> Result<(Value, Value)> {
        Ok((
            api::get_managed(self.0).await?,
            api::get_repo_ds(self.0).await?,
        ))
    }
    async fn scan(&self, object: &str, key: &str) -> Result<Vec<Value>> {
        api::all_records_with(self.0, object, key).await
    }
    async fn activate(&self, pair: &Pair) -> Result<()> {
        let ends = std::iter::once(&pair.forward)
            .chain(pair.reverse.iter())
            .map(|end| relink::NewEnd {
                object: end.object.clone(),
                key: end.key.clone(),
            })
            .collect();
        super::cli::require_live_ends(self.0, ends).await
    }
}

async fn ready(tenant: &str, pair: &Pair) -> Result<()> {
    conversion_records::ready(&TenantRead(tenant), pair).await
}

async fn finish_restore(
    ok: &WriteOk<'_>,
    capture: &mut Capture,
    file: &mut FileSession,
    permit: &conversion::Permit,
) -> Result<()> {
    let relink_permit = relink::authorize(false, true)
        .ok_or_else(|| Error::Config("record write not authorized".into()))?;
    conversion::restore_with(
        capture,
        permit,
        async |pair| ready(ok.tenant, pair).await,
        async |pair, link| observe(ok.tenant, pair, link).await,
        async |capture| file.save(capture).await,
        async |pair, link, rev| create(ok, &relink_permit, pair, link, rev).await,
        async |pair, link, edge| check_edge(ok.tenant, pair, link, edge).await,
    )
    .await?;
    for pair in &capture.pairs {
        guard(ok.tenant, pair, storage::Mode::Mapped).await?;
        let current = census(ok.tenant, pair).await?;
        conversion::exact(pair, &current)?;
        if let Some(edge) = current.edges.first() {
            check_edge(ok.tenant, pair, &Link::of(edge), edge).await?;
        }
        eprintln!(
            "{}: exact captured set confirmed, {} links; {}",
            pair.forward.label(),
            current.links.len(),
            if current.links.is_empty() {
                "runtime storage untested without a link"
            } else {
                "mapped runtime confirmed on each end"
            }
        );
    }
    capture.pending = None;
    capture.state = State::Complete;
    file.save(capture).await?;
    eprintln!(
        "capture marked complete and retained at {}",
        file.path.display()
    );
    Ok(())
}

/// The single repo.ds workflow is shared, including rollback. The callback
/// supplies operation-specific Q14/shape checks; no caller sends config itself.
async fn write_plan(
    ok: &WriteOk<'_>,
    pair: &Pair,
    plan: &repair::Plan,
    expected: storage::Mode,
) -> Result<()> {
    let permit = repair::authorize(false, true)
        .ok_or_else(|| Error::Config("config write not authorized".into()))?;
    let applied = repair::apply_with(
        plan,
        &permit,
        async || {
            let managed = api::get_managed(ok.tenant).await?;
            conversion::guard(pair, &managed, plan.before(), expected)?;
            Ok(managed)
        },
        async || api::get_repo_ds(ok.tenant).await,
        |document| {
            crate::access::ops::backup_config_document(
                "repo.ds",
                ok.tenant,
                document,
                chrono::Utc::now(),
            )
        },
        async |document| api::put_repo_ds(ok.tenant, &permit, document, ok.confirmed_prod).await,
        &[Duration::from_secs(3); 10],
    )
    .await
    .map_err(|failure| {
        Error::Config(match failure {
            repair::Failure::NotWritten(error) => format!("mapping not written: {error}"),
            repair::Failure::SendFailed { error, backup } => format!(
                "mapping send failed ({error}); whether it landed is unknown. Backup {}",
                backup.display()
            ),
        })
    })?;
    eprintln!("repo.ds backup: {}", applied.backup.display());
    if applied.readback != repair::Readback::Confirmed {
        return Err(Error::Config(format!(
            "mapping readback not confirmed: {:?}; inspect the tenant before retrying",
            applied.readback
        )));
    }
    Ok(())
}

pub async fn map(
    field: String,
    path: Option<PathBuf>,
    tenant: Option<String>,
    flags: Flags,
) -> Result<()> {
    let (object, key) = super::cli::parse_object_key(&field)?;
    let configured = tenant_config_for(tenant)?;
    let path = path.unwrap_or_else(default_path);
    let previous = if tokio::fs::try_exists(&path).await? {
        Some(load(&path).await?)
    } else {
        None
    };
    let mut capture = match &previous {
        Some(capture) => {
            identity(capture, Some(configured.name.clone()))?;
            if capture.origin != Origin::InPlace
                || capture.pairs.len() != 1
                || capture.pairs[0].forward.label() != field
            {
                return Err(Error::Config("map --capture needs this pair's in-place capture; use restore-links for a promotion capture".into()));
            }
            capture.clone()
        }
        None => {
            let managed = api::get_managed(&configured.name).await?;
            let repo = api::get_repo_ds(&configured.name).await?;
            let addition = storage::plan(&managed, &repo, &object, &key).map_err(Error::Config)?;
            let mut pair = Pair::of(&addition.pair, addition.entries)?;
            pair.links = generic_snapshot(&configured.name, &pair).await?.links;
            Capture::new(
                configured.name.clone(),
                configured.base_url.clone(),
                Origin::InPlace,
                vec![pair],
            )
        }
    };
    capture.validate()?;
    let managed = api::get_managed(&configured.name).await?;
    let repo = api::get_repo_ds(&configured.name).await?;
    let pair = &capture.pairs[0];
    let report = storage::pair(&managed, &repo, &object, &key)
        .map_err(|reason| Error::Config(format!("Q14/schema guard: {reason}")))?;
    conversion::guard(pair, &managed, &repo, storage::mode(&report))?;
    if storage::mode(&report) == storage::Mode::Generic {
        let addition = storage::plan(&managed, &repo, &object, &key).map_err(Error::Config)?;
        if addition.entries != pair.mappings {
            return Err(Error::Config(
                "column allocation changed since capture; inspect before a new conversion".into(),
            ));
        }
        for (object, entry) in &addition.added_types {
            eprintln!("  add per-type managed/{object}: {entry}");
        }
    }
    plan(
        &capture,
        &path,
        "map",
        previous.is_some(),
        storage::mode(&report) == storage::Mode::Generic,
    );
    let Some(permit) = consent(&flags, "map", &capture)? else {
        return output(
            &flags,
            &json!({"capture":path,"links":capture.count(),"dry_run":true,"mappings":capture.pairs[0].mappings}),
        );
    };
    let ok = ensure_prod_confirmed(&configured.name, flags.yes)?;
    let mut file = FileSession::open(path.clone(), previous).await?;
    file.save(&capture).await?;
    let result = async {
        let pair = capture.pairs[0].clone();
        let managed = api::get_managed(ok.tenant).await?;
        let repo = api::get_repo_ds(ok.tenant).await?;
        let report = storage::pair(&managed, &repo, &object, &key).map_err(|reason| Error::Config(format!("Q14/schema guard: {reason}")))?;
        match storage::mode(&report) {
            storage::Mode::Generic => {
                if !matches!(capture.state, State::Captured | State::Switching) {
                    return Err(Error::Config("mapping disappeared after restore began; inspect before remapping".into()));
                }
                conversion::guard(&pair, &managed, &repo, storage::Mode::Generic)?;
                let plan = repair::plan_add(&managed, &repo, &object, &key).map_err(Error::Config)?;
                if plan.addition().map(|addition| &addition.entries) != Some(&pair.mappings) {
                    return Err(Error::Config("column allocation changed since the capture; inspect before retrying with a new plan".into()));
                }
                conversion::switch_with(&mut capture, &permit,
                    async |capture| file.save(capture).await,
                    async || { conversion::exact(&pair, &generic_snapshot(ok.tenant, &pair).await?)?; guard(ok.tenant, &pair, storage::Mode::Generic).await?; Ok(()) },
                    async || write_plan(&ok, &pair, &plan, storage::Mode::Generic).await).await?;
                let records = api::present_records(ok.tenant, &object, &key).await?;
                storage::no_orphans(&object, &key, column(&pair), &records).map_err(Error::Config)?;
            },
            storage::Mode::Mapped => {
                conversion::guard(&pair, &managed, &repo, storage::Mode::Mapped)?;
                if matches!(capture.state, State::Captured | State::Switching) {
                    let records = api::present_records(ok.tenant, &object, &key).await?;
                    storage::no_orphans(&object, &key, column(&pair), &records).map_err(Error::Config)?;
                }
            },
            storage::Mode::Mixed => return Err(Error::Config("pair is mixed; inspect mappings before resuming".into())),
        }
        finish_restore(&ok, &mut capture, &mut file, &permit).await
    }.await;
    result.map_err(|error| stopped(&path, &capture, error))?;
    output(
        &flags,
        &json!({"capture":path,"links":capture.count(),"complete":true}),
    )
}

async fn read_promotion(lower: &str, upper: &str) -> Result<conversion::PromotionRead> {
    Ok(conversion::PromotionRead {
        lower_managed: api::get_managed(lower).await?,
        lower_repo: api::get_repo_ds(lower).await?,
        upper_managed: api::get_managed(upper).await?,
        upper_repo: api::get_repo_ds(upper).await?,
    })
}

async fn recheck_selection(
    initial: &conversion::PromotionRead,
    selection: &[Pair],
    new_pairs: &BTreeSet<String>,
    lower: &str,
    upper: &str,
) -> Result<()> {
    conversion::recheck_promotion(initial, selection, new_pairs, async || {
        read_promotion(lower, upper).await
    })
    .await
}

pub async fn capture_links(
    lower: String,
    upper: String,
    output_path: PathBuf,
    new_pairs: Vec<String>,
    options: Options,
) -> Result<()> {
    let flags = Flags::from(options);
    let lower = tenant_config_for(Some(lower))?;
    let upper = tenant_config_for(Some(upper))?;
    if lower
        .base_url
        .trim_end_matches('/')
        .eq_ignore_ascii_case(upper.base_url.trim_end_matches('/'))
    {
        return Err(Error::Config(
            "lower and upper must be different environments".into(),
        ));
    }
    let new_pairs = new_pairs.into_iter().collect::<BTreeSet<_>>();
    let conversion::PromotionPlan {
        initial,
        selection,
        linked: pairs,
    } = conversion::prepare_promotion(
        &new_pairs,
        async || read_promotion(&lower.name, &upper.name).await,
        async |pair| Ok(generic_snapshot(&upper.name, pair).await?.links),
    )
    .await?;
    for field in &new_pairs {
        eprintln!(
            "explicitly acknowledged new upper pair {field}: no capture; operator confirmed it has never existed there"
        );
    }
    if !new_pairs.is_empty() {
        eprintln!(
            "after promotion, check uncaptured new ends for orphaned column values before resuming writers"
        );
    }
    if pairs.is_empty() {
        eprintln!(
            "no linked generic pairs need capture on {}; no file written",
            upper.name
        );
        return output(
            &flags,
            &json!({"capture":null,"pairs":0,"new_pairs":new_pairs,"dry_run":flags.dry_run}),
        );
    }
    let mut capture = Capture::new(upper.name.clone(), upper.base_url, Origin::Promotion, pairs);
    capture.new_pairs = new_pairs.iter().cloned().collect();
    capture.validate()?;
    plan(
        &capture,
        &output_path,
        "capture-links before promotion",
        false,
        false,
    );
    let Some(_permit) = consent(&flags, "capture-links", &capture)? else {
        return output(
            &flags,
            &json!({"capture":output_path,"links":capture.count(),"pairs":capture.pairs.len(),"new_pairs":capture.new_pairs,"dry_run":true}),
        );
    };
    let _ok = ensure_prod_confirmed(&upper.name, flags.yes)?;
    let mut file = FileSession::open(output_path.clone(), None).await?;
    for pair in &capture.pairs {
        conversion::exact(pair, &generic_snapshot(&upper.name, pair).await?)?;
        guard(&upper.name, pair, storage::Mode::Generic).await?;
        guard(&lower.name, pair, storage::Mode::Mapped).await?;
    }
    recheck_selection(&initial, &selection, &new_pairs, &lower.name, &upper.name).await?;
    file.save(&capture).await?;
    eprintln!(
        "capture durable at {}. Keep writers paused through promotion; then run restore-links on {}",
        output_path.display(),
        upper.name
    );
    output(
        &flags,
        &json!({"capture":output_path,"links":capture.count(),"pairs":capture.pairs.len(),"new_pairs":capture.new_pairs}),
    )
}

pub async fn restore_links(path: PathBuf, tenant: Option<String>, options: Options) -> Result<()> {
    let flags = Flags::from(options);
    let mut capture = load(&path).await?;
    let configured = identity(&capture, tenant)?;
    // Refuse before consent too: a promotion has to land the mappings first.
    for pair in &capture.pairs {
        ready(&configured.name, pair).await?;
    }
    if capture.origin == Origin::InPlace
        && matches!(capture.state, State::Captured | State::Switching)
    {
        for pair in &capture.pairs {
            let records =
                api::present_records(&configured.name, &pair.forward.object, &pair.forward.key)
                    .await?;
            storage::no_orphans(
                &pair.forward.object,
                &pair.forward.key,
                column(pair),
                &records,
            )
            .map_err(Error::Config)?;
        }
    }
    plan(&capture, &path, "restore-links", true, false);
    let Some(permit) = consent(&flags, "restore-links", &capture)? else {
        return output(
            &flags,
            &json!({"capture":path,"links":capture.count(),"dry_run":true}),
        );
    };
    let ok = ensure_prod_confirmed(&configured.name, flags.yes)?;
    let mut file = FileSession::open(path.clone(), Some(capture.clone())).await?;
    finish_restore(&ok, &mut capture, &mut file, &permit)
        .await
        .map_err(|error| stopped(&path, &capture, error))?;
    output(
        &flags,
        &json!({"capture":path,"links":capture.count(),"complete":true}),
    )
}

struct TenantRemoval<'a, 'b> {
    ok: &'a WriteOk<'b>,
    file: &'a tokio::sync::Mutex<FileSession>,
}
impl conversion_records::Read for TenantRemoval<'_, '_> {
    async fn config(&self) -> Result<(Value, Value)> {
        conversion_records::Read::config(&TenantRead(self.ok.tenant)).await
    }
    async fn scan(&self, object: &str, key: &str) -> Result<Vec<Value>> {
        conversion_records::Read::scan(&TenantRead(self.ok.tenant), object, key).await
    }
    async fn activate(&self, pair: &Pair) -> Result<()> {
        conversion_records::Read::activate(&TenantRead(self.ok.tenant), pair).await
    }
}
impl conversion_records::Removal for TenantRemoval<'_, '_> {
    async fn observe(&self, pair: &Pair, link: &Link) -> Result<Observation> {
        observe(self.ok.tenant, pair, link).await
    }
    async fn check(&self, pair: &Pair, link: &Link, edge: &relink::Edge) -> Result<()> {
        check_edge(self.ok.tenant, pair, link, edge).await
    }
    async fn present(&self, pair: &Pair, link: &Link) -> Result<bool> {
        api::present_filter_finds(
            self.ok.tenant,
            &pair.forward.object,
            &link.from_id,
            &pair.forward.key,
        )
        .await
    }
    async fn save(&self, capture: &Capture) -> Result<()> {
        self.file.lock().await.save(capture).await
    }
    async fn send(&self, pair: &Pair, write: &relink::Write) -> Result<()> {
        api::send_relink(
            self.ok.tenant,
            &pair.forward.object,
            write,
            self.ok.confirmed_prod,
        )
        .await
        .map(|_| ())
    }
    async fn write_config(&self, pair: &Pair, plan: &repair::Plan) -> Result<()> {
        write_plan(self.ok, pair, plan, storage::Mode::Mapped).await
    }
}

pub async fn rollback_map(path: PathBuf, tenant: Option<String>, options: Options) -> Result<()> {
    let flags = Flags::from(options);
    let mut capture = load(&path).await?;
    let configured = identity(&capture, tenant)?;
    if capture.origin != Origin::InPlace || capture.pairs.len() != 1 {
        return Err(Error::Config("rollback-map requires an in-place capture; upper-environment restore never grants config-write authority".into()));
    }
    plan(&capture, &path, "rollback-map", true, false);
    let Some(permit) = consent(&flags, "rollback-map", &capture)? else {
        return output(
            &flags,
            &json!({"capture":path,"dry_run":true,"links":capture.count()}),
        );
    };
    let ok = ensure_prod_confirmed(&configured.name, flags.yes)?;
    let file =
        tokio::sync::Mutex::new(FileSession::open(path.clone(), Some(capture.clone())).await?);
    let pair = capture.pairs[0].clone();
    let result = async {
        let managed = api::get_managed(ok.tenant).await?;
        let repo = api::get_repo_ds(ok.tenant).await?;
        let report = storage::pair(&managed, &repo, &pair.forward.object, &pair.forward.key)
            .map_err(|reason| Error::Config(format!("Q14/schema guard: {reason}")))?;
        if storage::mode(&report) == storage::Mode::Mapped {
            let relink_permit = relink::authorize(false, true)
                .ok_or_else(|| Error::Config("rollback record writes not authorized".into()))?;
            conversion_records::rollback(
                &TenantRemoval {
                    ok: &ok,
                    file: &file,
                },
                &mut capture,
                &permit,
                &relink_permit,
            )
            .await?;
        }
        guard(ok.tenant, &pair, storage::Mode::Generic).await?;
        conversion::exact(&pair, &census(ok.tenant, &pair).await?)?;
        capture.pending = None;
        capture.state = State::RolledBack;
        file.lock().await.save(&capture).await?;
        eprintln!(
            "rollback confirmed: original generic links visible; capture retained at {}",
            path.display()
        );
        Ok(())
    }
    .await;
    result.map_err(|error: Error| Error::Config(format!("rollback stopped: {error}. Keep writers paused and re-run rollback-map {} to finish. If the captured mappings are still installed, restore-links with this same file explicitly reverses the interrupted rollback; capture retained", path.display())))?;
    output(&flags, &json!({"capture":path,"rolled_back":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::conversion::tests::captured;

    // Regression: dry-run previously claimed a saved state without any file.
    #[test]
    fn planned_capture_output_never_claims_a_durable_save() {
        let (capture, _) = captured(true);
        let text = plan_text(&capture, Path::new("capture.json"), "map", false, true);
        assert!(text.contains("planned capture; no file written"));
        assert!(!text.contains("saved state"));
        assert!(text.contains("1 links are invisible"));
        assert!(text.contains("  map a.link:"));
    }

    // Regression: an existing mapped capture must not print a second switch.
    #[test]
    fn restoring_and_complete_plans_describe_resume_without_mapping_writes() {
        let (mut capture, _) = captured(true);
        for state in [State::Restoring, State::Complete, State::Captured] {
            capture.state = state;
            let text = plan_text(&capture, Path::new("capture.json"), "map", true, false);
            assert!(text.contains(&format!("saved state: {state:?}")));
            assert!(text.contains("no mapping switch is planned"));
            assert!(!text.contains("links are invisible"));
            assert!(!text.contains("  map a.link:"));
        }
    }
}
