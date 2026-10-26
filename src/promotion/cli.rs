//! `aic promotion` parser and command implementation. Read-only: there is no
//! verb here that locks, promotes or unlocks.

use clap::Subcommand;

use futures::StreamExt;

use crate::cli::{print_json, print_table, tenant_config_for, tenant_for};
use crate::promotion::{api, spec, verify};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum PromotionCommand {
    /// Lock state and promotion status side by side, naming any disagreement.
    Status {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List promotion reports (they live on the upper environment of a pair).
    Reports {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a promotion report: the latest, one by id, or the provisional
    /// one, generated without a lock. Its items are the promotion service's
    /// claim, not a live diff — measured listing items identical on both
    /// tenants (docs/api/23-promotion.md).
    Report {
        /// Report id from `aic promotion reports`; omit for the latest.
        #[arg(conflicts_with = "provisional")]
        id: Option<String>,
        #[arg(long)]
        provisional: bool,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Check a report against both tenants: fetch every item it lists from
    /// the lower and the upper environment and compare the content, ignoring
    /// `_rev` and audit stamps. Exits non-zero if any item differs or could
    /// not be checked. Reads only.
    Verify {
        /// The environment promoted from.
        #[arg(long)]
        lower: String,
        /// The environment promoted to; the report is read from here.
        #[arg(long)]
        upper: String,
        /// Report id to check; omit for the provisional report.
        #[arg(long)]
        report: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

/// A promotion call's error, with what its status means here when that is
/// known — a 403 for scope is not a lock state, a 503 is no service at all.
fn explained<T>(result: Result<T>, tenant: &str) -> Result<T> {
    result.map_err(|error| match spec::explain(&error, tenant) {
        Some(why) => Error::Config(format!("{why}\n  ({error})")),
        None => error,
    })
}

fn decoded<T>(result: std::result::Result<T, String>) -> Result<T> {
    result.map_err(Error::Config)
}

fn render_status(tenant: &str, a: &spec::Assessment) {
    let none = || "-".to_string();
    let (lock, status) = (&a.lock, &a.status);
    let mut rows = vec![
        vec![
            "lock/state".into(),
            lock.result.clone(),
            lock.description.clone().unwrap_or_else(none),
        ],
        vec![
            "promotion".into(),
            status.status.clone(),
            status.message.clone().unwrap_or_else(none),
        ],
    ];
    for (name, side) in [("lower env", &lock.lower), ("upper env", &lock.upper)] {
        if let Some(side) = side {
            rows.push(vec![
                name.into(),
                side.state.clone(),
                format!("proxy {}", side.proxy_state.clone().unwrap_or_else(none)),
            ]);
        }
    }
    print_table(&["SOURCE", "STATE", "DETAIL"], &rows);
    if lock.upper.is_none() && lock.lower.is_none() {
        println!(
            "\n{tenant} returned no environment-pair details (measured: only the upper environment of a pair does)"
        );
    }
    if a.blocking() {
        println!("\nthe promotion status reports a blocking error");
    }
    if !a.disagreements.is_empty() {
        println!("\nthe sources describe the lock differently, so neither is the answer:");
        for line in &a.disagreements {
            println!("  - {line}");
        }
    }
}

pub async fn run(command: PromotionCommand) -> Result<()> {
    match command {
        PromotionCommand::Status { tenant, json } => {
            let tenant = tenant_for(tenant)?;
            let lock_raw = explained(api::lock_state(&tenant).await, &tenant)?;
            let status_raw = explained(api::status(&tenant).await, &tenant)?;
            // Decided before rendering, so --json exits exactly as the table does.
            let assessment = decoded(spec::assess(&lock_raw, &status_raw))?;
            if json {
                print_json(&serde_json::json!({
                    "lockState": lock_raw,
                    "status": status_raw,
                    "disagreements": assessment.disagreements,
                    "blockingError": assessment.blocking(),
                    "ok": assessment.ok(),
                }))?;
            } else {
                render_status(&tenant, &assessment);
            }
            if assessment.ok() {
                Ok(())
            } else {
                Err(Error::Config(if assessment.disagreements.is_empty() {
                    "the promotion status reports a blocking error".into()
                } else {
                    "the promotion sources describe the lock differently".into()
                }))
            }
        }
        PromotionCommand::Reports { tenant, json } => {
            let tenant = tenant_for(tenant)?;
            let raw = explained(api::reports(&tenant).await, &tenant)?;
            if json {
                return print_json(&raw);
            }
            let list = raw
                .as_array()
                .ok_or_else(|| Error::Config(format!("reports is not an array: {raw}")))?;
            if list.is_empty() {
                println!(
                    "{tenant} has no promotion reports — on a pair, reports live on the upper environment"
                );
                return Ok(());
            }
            let field = |v: &serde_json::Value, k: &str| match v.get(k) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => "-".into(),
            };
            let rows = list
                .iter()
                .map(|r| {
                    vec![
                        field(r, "createdDate"),
                        field(r, "type"),
                        field(r, "dryRun"),
                        field(r, "promoter"),
                        field(r, "reportId"),
                    ]
                })
                .collect::<Vec<_>>();
            print_table(
                &["CREATED", "TYPE", "DRY RUN", "PROMOTER", "REPORT ID"],
                &rows,
            );
            Ok(())
        }
        PromotionCommand::Report {
            id,
            provisional,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let which = match (id, provisional) {
                (_, true) => spec::Which::Provisional,
                (Some(id), false) => spec::Which::Id(id),
                (None, false) => spec::Which::Latest,
            };
            let raw = explained(api::report(&tenant, &which).await, &tenant)?;
            if json {
                return print_json(&raw);
            }
            let groups = decoded(spec::summarise(&raw))?;
            for key in [
                "reportName",
                "createdDate",
                "promoter",
                "promotionDescription",
            ] {
                if let Some(value) = raw.get(key).and_then(serde_json::Value::as_str) {
                    println!("{key}: {value}");
                }
            }
            let total: usize = groups.iter().map(|g| g.names.len()).sum();
            if total == 0 {
                println!("\nno changes listed");
                return Ok(());
            }
            println!("\n{total} change(s):");
            for group in &groups {
                println!(
                    "\n{} / {} — {} {}",
                    group.section,
                    group.item,
                    group.names.len(),
                    group.change
                );
                for name in &group.names {
                    println!("  {name}");
                }
            }
            Ok(())
        }
        PromotionCommand::Verify {
            lower,
            upper,
            report,
            json,
        } => run_verify(lower, upper, report, json).await,
    }
}

/// How many items are fetched at once, per tenant pair.
const VERIFY_CONCURRENCY: usize = 8;

async fn judge_entry(lower: &str, upper: &str, entry: &verify::Entry) -> verify::Verdict {
    let target = verify::target(entry);
    if let verify::Target::Unchecked(why) = target {
        return verify::Verdict::Unchecked(why);
    }
    let (l, u) = futures::join!(api::fetch(lower, &target), api::fetch(upper, &target));
    match (l, u) {
        (Ok(l), Ok(u)) => verify::settle(
            &entry.change,
            verify::judge(target.shape(), l.as_ref(), u.as_ref()),
        ),
        (Err(e), _) => verify::Verdict::Unchecked(format!("{lower}: {e}")),
        (_, Err(e)) => verify::Verdict::Unchecked(format!("{upper}: {e}")),
    }
}

async fn run_verify(
    lower: String,
    upper: String,
    report: Option<String>,
    json: bool,
) -> Result<()> {
    let (l, u) = (
        tenant_config_for(Some(lower))?,
        tenant_config_for(Some(upper))?,
    );
    if verify::same_tenant(&l.base_url, &u.base_url) {
        return Err(Error::Config(format!(
            "{} and {} are the same tenant; verify compares two",
            l.name, u.name
        )));
    }
    let which = report.map_or(spec::Which::Provisional, spec::Which::Id);
    let raw = explained(api::report(&u.name, &which).await, &u.name)?;
    let entries = decoded(verify::entries(&raw))?;
    let verdicts: Vec<verify::Verdict> = futures::stream::iter(&entries)
        .map(|entry| judge_entry(&l.name, &u.name, entry))
        .buffered(VERIFY_CONCURRENCY)
        .collect()
        .await;
    let pairs = entries.iter().zip(&verdicts);
    if json {
        let items: Vec<_> = pairs
            .map(|(e, v)| {
                serde_json::json!({
                    "section": e.section,
                    "item": e.item,
                    "change": e.change,
                    "name": e.name,
                    "realm": e.realm,
                    "verdict": v.label(),
                    "detail": detail(v),
                    "inSync": v.in_sync(),
                })
            })
            .collect();
        print_json(&serde_json::json!({ "lower": l.name, "upper": u.name, "items": items }))?;
    } else {
        let rows: Vec<Vec<String>> = pairs
            .map(|(e, v)| {
                vec![
                    v.label().into(),
                    e.item.clone(),
                    e.change.clone(),
                    e.name.clone(),
                    e.realm.clone().unwrap_or_else(|| "-".into()),
                    detail(v),
                ]
            })
            .collect();
        if rows.is_empty() {
            println!("the report lists no changes");
        } else {
            print_table(
                &["VERDICT", "ITEM", "REPORTED", "NAME", "REALM", "DETAIL"],
                &rows,
            );
        }
    }
    let count = |f: fn(&verify::Verdict) -> bool| verdicts.iter().filter(|v| f(v)).count();
    let in_sync = count(verify::Verdict::in_sync);
    let unchecked = count(|v| matches!(v, verify::Verdict::Unchecked(_)));
    let differ = verdicts.len() - in_sync - unchecked;
    if !json {
        println!(
            "\n{} item(s): {in_sync} in sync, {differ} differ, {unchecked} unchecked ({} → {})",
            verdicts.len(),
            l.name,
            u.name
        );
    }
    if differ + unchecked == 0 {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{differ} item(s) differ and {unchecked} could not be checked"
        )))
    }
}

fn detail(verdict: &verify::Verdict) -> String {
    match verdict {
        verify::Verdict::Differs(paths) => paths.join(", "),
        verify::Verdict::Unchecked(why) => why.clone(),
        _ => String::new(),
    }
}
