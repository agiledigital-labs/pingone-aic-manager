//! `aic sync` queue diagnostics and reconciliation commands.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::Serialize;

use crate::agent::duration::{format_duration, parse_duration};
use crate::cli::{clip, print_json, print_table, prod_hint, tenant_config_for};
use crate::mappings::api::{self, MappingSummary, QueueItem, ReconStatus};
use crate::{Error, Result};

const ACTIONS: [&str; 3] = ["notifyCreate", "notifyUpdate", "notifyDelete"];
const DEFAULT_RECON_TIMEOUT: &str = "10m";
const DEFAULT_CLAIM_PAGE_SIZE: u64 = 100;
const RECON_POLL_DELAY: Duration = Duration::from_secs(2);

#[derive(Subcommand, Debug)]
pub enum SyncCommand {
    /// List mappings and their queued implicit-sync settings.
    Mappings {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Diagnose the persistent queued implicit-sync backlog.
    Queue {
        #[arg(long)]
        mapping: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
        /// Re-probe queue depth every N seconds (minimum: 2 seconds).
        #[arg(long, value_parser = watch_interval)]
        watch: Option<u64>,
    },
    /// Start a full or single-source reconciliation. Writes target data.
    Recon {
        mapping: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        wait: bool,
        /// Maximum wait, such as 10m (default: 10m).
        #[arg(long, default_value = DEFAULT_RECON_TIMEOUT)]
        timeout: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a reconciliation write to a production tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show one reconciliation or recent/active reconciliations.
    #[command(name = "recon-status")]
    ReconStatus {
        recon_id: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
struct MappingOutput {
    mapping: String,
    source: String,
    target: String,
    scripts: usize,
    queued: String,
    poll_ceiling_events_per_sec_per_node: Option<f64>,
}

#[derive(Serialize)]
struct QueueOutput {
    depth_estimate: Option<u64>,
    mapping_depth_estimate: Option<u64>,
    mapping_counts: Vec<CountOutput>,
    action_counts: Vec<CountOutput>,
    oldest: Option<QueueBoundaryOutput>,
    newest: Option<QueueBoundaryOutput>,
    oldest_age_seconds: Option<u64>,
    claim_query_latency_ms: u128,
    claim_sample_size: usize,
    claimed_in_sample: usize,
    node_ids_in_sample: Vec<String>,
}

#[derive(Serialize)]
struct CountOutput {
    value: String,
    estimate: Option<u64>,
}
#[derive(Serialize)]
struct QueueBoundaryOutput {
    id: String,
    mapping: String,
    create_date: Option<String>,
}

#[derive(Serialize)]
struct ReconOutput {
    id: String,
    mapping: String,
    state: String,
    stage: String,
    stage_description: String,
    processed: u64,
    created: u64,
    updated: u64,
    deleted: u64,
    duration_ms: Option<i64>,
    records_per_sec: Option<f64>,
}

pub async fn run(command: SyncCommand) -> Result<()> {
    match command {
        SyncCommand::Mappings { tenant, json } => mappings(tenant, json).await,
        SyncCommand::Queue {
            mapping,
            tenant,
            json,
            watch,
        } => queue(mapping, tenant, json, watch).await,
        SyncCommand::Recon {
            mapping,
            id,
            wait,
            timeout,
            tenant,
            yes,
            json,
        } => recon(&mapping, id.as_deref(), wait, &timeout, tenant, yes, json).await,
        SyncCommand::ReconStatus {
            recon_id,
            tenant,
            json,
        } => recon_status(recon_id.as_deref(), tenant, json).await,
    }
}

async fn mappings(tenant_arg: Option<String>, json: bool) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let mappings = api::list_mappings(&tenant.name).await?;
    let output = mappings.iter().map(mapping_output).collect::<Vec<_>>();
    if json {
        return print_json(&output);
    }
    let rows = output
        .iter()
        .map(|item| {
            vec![
                item.mapping.clone(),
                item.source.clone(),
                item.target.clone(),
                item.scripts.to_string(),
                item.queued.clone(),
                item.poll_ceiling_events_per_sec_per_node
                    .map_or_else(|| "-".into(), |v| format!("{v:.2}")),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        &[
            "MAPPING",
            "SOURCE",
            "TARGET",
            "SCRIPTS",
            "QUEUED",
            "POLL CEILING",
        ],
        &rows,
    );
    Ok(())
}

fn mapping_output(mapping: &MappingSummary) -> MappingOutput {
    let (queued, ceiling) = match &mapping.queued_sync {
        None => ("off".into(), None),
        Some(config) if !config.enabled => ("disabled".into(), None),
        Some(config) => (
            "on".into(),
            poll_ceiling(config.page_size, config.polling_interval_ms),
        ),
    };
    MappingOutput {
        mapping: mapping.name.clone(),
        source: mapping.source.clone(),
        target: mapping.target.clone(),
        scripts: mapping.inline_script_count,
        queued,
        poll_ceiling_events_per_sec_per_node: ceiling,
    }
}

async fn queue(
    mapping: Option<String>,
    tenant_arg: Option<String>,
    json: bool,
    watch: Option<u64>,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    if let Some(interval) = watch {
        return watch_queue(&tenant.name, mapping.as_deref(), interval, json).await;
    }
    let output = queue_output(&tenant.name, mapping.as_deref()).await?;
    print_queue_output(&output, json)
}

async fn queue_output(tenant: &str, mapping: Option<&str>) -> Result<QueueOutput> {
    let depth = api::queue_depth(tenant, None).await?;
    let mapping_depth = match mapping {
        Some(name) => api::queue_depth(tenant, Some(name)).await?,
        None => None,
    };
    // `--mapping` narrows every probe below, so the breakdown decision follows
    // the narrowed depth: an empty slice of a busy queue still needs no probes.
    let effective_depth = mapping_depth.or(depth);
    // One config/sync read serves both the breakdown and the claim page size.
    let mappings = if should_query_breakdown(effective_depth) {
        api::list_mappings(tenant).await?
    } else {
        Vec::new()
    };
    let mut mapping_counts = Vec::new();
    let mut action_counts = Vec::new();
    for name in mappings.iter().map(|item| item.name.clone()) {
        mapping_counts.push(CountOutput {
            estimate: api::queue_count(tenant, Some(&format!("mapping eq \"{name}\""))).await?,
            value: name,
        });
    }
    for action in ACTIONS.into_iter().filter(|_| !mappings.is_empty()) {
        action_counts.push(CountOutput {
            estimate: api::queue_count(tenant, Some(&format!("syncAction eq \"{action}\"")))
                .await?,
            value: action.into(),
        });
    }
    let oldest = api::queue_boundary(tenant, mapping, false).await?;
    let newest = api::queue_boundary(tenant, mapping, true).await?;
    let oldest_age_seconds = oldest
        .as_ref()
        .and_then(|item| backlog_age_seconds(item.create_date.as_deref(), Utc::now()));
    let page_size = claim_page_size(&mappings, mapping);
    // The query itself is the diagnostic; its response body is intentionally discarded.
    let started = Instant::now();
    api::queue_claim_probe(tenant, mapping, page_size).await?;
    let latency = started.elapsed().as_millis();
    let sample = api::queue_sample(tenant, mapping).await?;
    let (claimed, node_ids) = claim_state(&sample);
    Ok(QueueOutput {
        depth_estimate: depth,
        mapping_depth_estimate: mapping_depth,
        mapping_counts,
        action_counts,
        oldest: boundary_output(oldest),
        newest: boundary_output(newest),
        oldest_age_seconds,
        claim_query_latency_ms: latency,
        claim_sample_size: sample.len(),
        claimed_in_sample: claimed,
        node_ids_in_sample: node_ids,
    })
}

/// A known-empty queue needs no dimension probes; unknown totals still do,
/// because IDM's missing/-1 sentinel is not evidence of an empty queue.
fn should_query_breakdown(depth: Option<u64>) -> bool {
    depth != Some(0)
}

/// The page size the poller would claim with, so the probe below measures the
/// query the node actually runs. Falls back to the documented default.
fn claim_page_size(mappings: &[MappingSummary], mapping: Option<&str>) -> u64 {
    mapping
        .and_then(|name| mappings.iter().find(|item| item.name == name))
        .and_then(|item| item.queued_sync.as_ref())
        .map(|config| config.page_size)
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_CLAIM_PAGE_SIZE)
}

fn boundary_output(item: Option<QueueItem>) -> Option<QueueBoundaryOutput> {
    item.map(|item| QueueBoundaryOutput {
        id: item.id,
        mapping: item.mapping,
        create_date: item.create_date,
    })
}

fn print_queue_output(output: &QueueOutput, json: bool) -> Result<()> {
    if json {
        return print_json(output);
    }
    println!(
        "depth estimate: {}",
        output
            .depth_estimate
            .map_or_else(|| "unknown".into(), |n| n.to_string())
    );
    if let Some(depth) = output.mapping_depth_estimate {
        println!("mapping depth estimate: {depth}");
    }
    if !output.mapping_counts.is_empty() {
        println!("breakdown (estimates):");
        let rows = output
            .mapping_counts
            .iter()
            .chain(output.action_counts.iter())
            .map(|count| {
                vec![
                    count.value.clone(),
                    count
                        .estimate
                        .map_or_else(|| "unknown".into(), |n| n.to_string()),
                ]
            })
            .collect::<Vec<_>>();
        print_table(&["VALUE", "QUEUE ESTIMATE"], &rows);
    }
    println!("oldest: {}", boundary_text(output.oldest.as_ref()));
    println!("newest: {}", boundary_text(output.newest.as_ref()));
    if let Some(age) = output.oldest_age_seconds {
        println!("oldest age: {}", format_duration(age));
    }
    println!("claim-query latency: {} ms", output.claim_query_latency_ms);
    println!(
        "claim state: sample of {}; {} claimed; nodes: {}",
        output.claim_sample_size,
        output.claimed_in_sample,
        if output.node_ids_in_sample.is_empty() {
            "(none)".into()
        } else {
            output.node_ids_in_sample.join(", ")
        }
    );
    Ok(())
}

fn boundary_text(boundary: Option<&QueueBoundaryOutput>) -> String {
    boundary.map_or_else(
        || "unknown".into(),
        |item| {
            format!(
                "{} {} ({})",
                item.create_date.as_deref().unwrap_or("unknown"),
                item.mapping,
                item.id
            )
        },
    )
}

async fn watch_queue(
    tenant: &str,
    mapping: Option<&str>,
    interval_secs: u64,
    json: bool,
) -> Result<()> {
    let mut previous: Option<(Option<u64>, Instant)> = None;
    loop {
        let now = Instant::now();
        let depth = api::queue_depth(tenant, mapping).await?;
        let metrics = previous
            .map(|(old, then)| drain_metrics(old, depth, now.duration_since(then).as_secs_f64()));
        if json {
            println!(
                "{}",
                serde_json::to_string(&WatchOutput {
                    depth_estimate: depth,
                    drain_events_per_sec: metrics.and_then(|m| m.rate),
                    eta_seconds: metrics.and_then(|m| m.eta_seconds)
                })?
            );
        } else {
            let rate = metrics
                .and_then(|m| m.rate)
                .map_or_else(|| "-".into(), |v| format!("{v:+.2} events/s"));
            let eta = metrics
                .and_then(|m| m.eta_seconds)
                .map_or_else(|| "-".into(), format_duration);
            println!(
                "depth estimate: {}; drain: {rate}; eta: {eta}",
                depth.map_or_else(|| "unknown".into(), |n| n.to_string())
            );
        }
        previous = Some((depth, now));
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

#[derive(Serialize)]
struct WatchOutput {
    depth_estimate: Option<u64>,
    drain_events_per_sec: Option<f64>,
    eta_seconds: Option<u64>,
}
#[derive(Clone, Copy)]
struct DrainMetrics {
    rate: Option<f64>,
    eta_seconds: Option<u64>,
}

fn drain_metrics(previous: Option<u64>, current: Option<u64>, elapsed_secs: f64) -> DrainMetrics {
    let Some((previous, current)) = previous.zip(current) else {
        return DrainMetrics {
            rate: None,
            eta_seconds: None,
        };
    };
    if elapsed_secs <= 0.0 {
        return DrainMetrics {
            rate: None,
            eta_seconds: None,
        };
    }
    let rate = (previous as f64 - current as f64) / elapsed_secs;
    let eta_seconds = (rate > 0.0).then(|| (current as f64 / rate).ceil() as u64);
    DrainMetrics {
        rate: Some(rate),
        eta_seconds,
    }
}

async fn recon(
    mapping: &str,
    source_id: Option<&str>,
    wait: bool,
    timeout: &str,
    tenant_arg: Option<String>,
    yes: bool,
    json: bool,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let timeout = parse_duration(timeout).map_err(Error::Config)?;
    if let Some(source_id) = source_id {
        let body =
            prod_hint(api::start_recon_by_id(&tenant.name, mapping, source_id, wait, yes).await)?;
        // A waited reconById returns the per-record outcome, which is the whole
        // point of the flag — and on failure that body is an error document, not
        // a recon status. Print it raw rather than failing to parse it.
        return match api::parse_recon_status(&body) {
            Ok(status) if wait && !json => print_recon(&status, false),
            _ => print_json(&body),
        };
    }
    let id = prod_hint(api::start_recon(&tenant.name, mapping, yes).await)?;
    if !wait {
        if json {
            return print_json(&serde_json::json!({"id": id, "state": "ACTIVE"}));
        }
        println!("reconciliation started: {id}");
        return Ok(());
    }
    let status = wait_for_recon(&tenant.name, &id, timeout).await?;
    print_recon(&status, json)
}

async fn wait_for_recon(tenant: &str, id: &str, timeout_secs: u64) -> Result<ReconStatus> {
    let started = Instant::now();
    loop {
        let status = api::recon_status(tenant, id).await?;
        if api::state_is_terminal(&status.state) {
            return Ok(status);
        }
        if started.elapsed() >= Duration::from_secs(timeout_secs) {
            return Err(Error::Config(format!(
                "reconciliation {id} did not finish within {}",
                format_duration(timeout_secs)
            )));
        }
        tokio::time::sleep(RECON_POLL_DELAY).await;
    }
}

async fn recon_status(id: Option<&str>, tenant_arg: Option<String>, json: bool) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    if let Some(id) = id {
        return print_recon(&api::recon_status(&tenant.name, id).await?, json);
    }
    let statuses = api::recon_list(&tenant.name).await?;
    if json {
        return print_json(&statuses.iter().map(recon_output).collect::<Vec<_>>());
    }
    let rows = statuses.iter().map(recon_row).collect::<Vec<_>>();
    print_table(
        &[
            "ID",
            "MAPPING",
            "STATE",
            "STAGE",
            "PROCESSED",
            "CREATED",
            "UPDATED",
            "DELETED",
            "DURATION",
            "RATE",
        ],
        &rows,
    );
    Ok(())
}

fn print_recon(status: &ReconStatus, json: bool) -> Result<()> {
    if json {
        return print_json(&recon_output(status));
    }
    print_table(
        &[
            "ID",
            "MAPPING",
            "STATE",
            "STAGE",
            "PROCESSED",
            "CREATED",
            "UPDATED",
            "DELETED",
            "DURATION",
            "RATE",
        ],
        &[recon_row(status)],
    );
    if !status.stage_description.is_empty() {
        println!("detail: {}", clip(&status.stage_description, 160));
    }
    Ok(())
}
fn recon_output(status: &ReconStatus) -> ReconOutput {
    ReconOutput {
        id: status.id.clone(),
        mapping: status.mapping.clone(),
        state: status.state.clone(),
        stage: status.stage.clone(),
        stage_description: status.stage_description.clone(),
        processed: status.processed,
        created: status.created,
        updated: status.updated,
        deleted: status.deleted,
        duration_ms: status.duration,
        records_per_sec: records_per_second(status.processed, status.duration),
    }
}
fn recon_row(status: &ReconStatus) -> Vec<String> {
    let output = recon_output(status);
    vec![
        output.id,
        output.mapping,
        output.state,
        output.stage,
        output.processed.to_string(),
        output.created.to_string(),
        output.updated.to_string(),
        output.deleted.to_string(),
        output
            .duration_ms
            .map_or_else(|| "-".into(), |v| format!("{v}ms")),
        output
            .records_per_sec
            .map_or_else(|| "-".into(), |v| format!("{v:.2}/s")),
    ]
}
fn records_per_second(processed: u64, duration_ms: Option<i64>) -> Option<f64> {
    duration_ms
        .filter(|duration| *duration > 0)
        .map(|duration| processed as f64 / duration as f64 * 1000.0)
}
fn poll_ceiling(page_size: u64, interval_ms: u64) -> Option<f64> {
    (interval_ms > 0).then(|| page_size as f64 / interval_ms as f64 * 1000.0)
}
fn backlog_age_seconds(value: Option<&str>, now: DateTime<Utc>) -> Option<u64> {
    let created = DateTime::parse_from_rfc3339(value?)
        .ok()?
        .with_timezone(&Utc);
    now.signed_duration_since(created)
        .num_seconds()
        .try_into()
        .ok()
}
fn claim_state(sample: &[QueueItem]) -> (usize, Vec<String>) {
    let mut ids = sample
        .iter()
        .filter_map(|item| item.node_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    (
        sample.iter().filter(|item| item.node_id.is_some()).count(),
        ids,
    )
}
fn watch_interval(value: &str) -> std::result::Result<u64, String> {
    let value = value
        .parse::<u64>()
        .map_err(|_| "watch interval must be whole seconds".to_string())?;
    if value < 2 {
        Err("watch interval must be at least 2 seconds".into())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use chrono::TimeZone;
    use clap::Parser;

    #[test]
    fn backlog_age_uses_supplied_now() {
        assert_eq!(
            backlog_age_seconds(
                Some("2026-07-29T00:00:00.123456789Z"),
                Utc.with_ymd_and_hms(2026, 7, 29, 0, 1, 0).unwrap()
            ),
            Some(59)
        );
    }
    #[test]
    fn drain_rate_and_eta_cover_growth_and_zero_elapsed() {
        let normal = drain_metrics(Some(120), Some(20), 10.0);
        assert_eq!(normal.rate, Some(10.0));
        assert_eq!(normal.eta_seconds, Some(2));
        let growing = drain_metrics(Some(10), Some(20), 5.0);
        assert_eq!(growing.rate, Some(-2.0));
        assert_eq!(growing.eta_seconds, None);
        assert_eq!(drain_metrics(Some(10), Some(10), 0.0).rate, None);
    }
    #[test]
    fn records_rate_requires_positive_duration() {
        assert_eq!(records_per_second(100, Some(2000)), Some(50.0));
        assert_eq!(records_per_second(100, Some(0)), None);
        assert_eq!(records_per_second(100, None), None);
    }
    #[test]
    fn breakdown_is_probed_unless_the_queue_is_known_empty() {
        assert!(should_query_breakdown(Some(5)));
        assert!(!should_query_breakdown(Some(0)));
        // IDM's -1/missing total parses to None; unknown is not empty.
        assert!(should_query_breakdown(None));
    }

    #[test]
    fn claim_page_size_follows_the_mappings_poller_settings() {
        let queued = |page_size| {
            Some(api::QueuedSync {
                enabled: true,
                page_size,
                polling_interval_ms: 1000,
                max_queue_size: 1000,
                max_retries: 5,
                retry_delay_ms: 1000,
                post_retry_action: "logged-ignore".into(),
            })
        };
        let summary = |name: &str, queued_sync| MappingSummary {
            name: name.into(),
            source: "managed/a".into(),
            target: "managed/b".into(),
            inline_script_count: 0,
            queued_sync,
        };
        let mappings = vec![
            summary("m", queued(250)),
            summary("z", queued(0)),
            summary("p", None),
        ];

        assert_eq!(claim_page_size(&mappings, Some("m")), 250);
        // A zero/absent pageSize and an unfiltered run both fall back.
        assert_eq!(
            claim_page_size(&mappings, Some("z")),
            DEFAULT_CLAIM_PAGE_SIZE
        );
        assert_eq!(
            claim_page_size(&mappings, Some("p")),
            DEFAULT_CLAIM_PAGE_SIZE
        );
        assert_eq!(claim_page_size(&mappings, None), DEFAULT_CLAIM_PAGE_SIZE);
    }
    #[test]
    fn clap_parses_sync_commands_and_rejects_fast_watch() {
        assert!(matches!(
            Cli::try_parse_from(["aic", "sync", "queue", "--mapping", "x", "--json"])
                .unwrap()
                .command,
            Some(crate::cli::Command::Sync { .. })
        ));
        assert!(Cli::try_parse_from(["aic", "sync", "recon", "m", "--id", "1", "--wait"]).is_ok());
        assert!(Cli::try_parse_from(["aic", "sync", "recon-status"]).is_ok());
        assert!(Cli::try_parse_from(["aic", "sync", "queue", "--watch", "1"]).is_err());
    }
}
