//! `aic promotion` parser and command implementation. Read-only: there is no
//! verb here that locks, promotes or unlocks.

use clap::Subcommand;

use crate::cli::{print_json, print_table, tenant_for};
use crate::promotion::{api, spec};
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
    /// one — what a promotion would change now, generated without a lock.
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

pub async fn run(command: PromotionCommand) -> Result<()> {
    match command {
        PromotionCommand::Status { tenant, json } => {
            let tenant = tenant_for(tenant)?;
            let lock_raw = explained(api::lock_state(&tenant).await, &tenant)?;
            let status_raw = explained(api::status(&tenant).await, &tenant)?;
            if json {
                return print_json(&serde_json::json!({
                    "lockState": lock_raw,
                    "status": status_raw,
                }));
            }
            let lock = decoded(spec::lock_state(&lock_raw))?;
            let status = decoded(spec::status(&status_raw))?;
            let none = || "-".to_string();
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
            if lock.upper.is_none() {
                println!(
                    "\n{tenant} reports no environment pair — on a pair, only the upper environment does"
                );
            }
            let disagree = spec::disagreements(&lock, &status);
            if !disagree.is_empty() {
                println!("\nthe sources disagree, so neither is the answer:");
                for line in &disagree {
                    println!("  - {line}");
                }
                if status.blocking_error == Some(true) {
                    println!("  - the promotion status reports a blocking error");
                }
                return Err(Error::Config("promotion state is inconsistent".into()));
            }
            Ok(())
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
                println!("\nno differences");
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
    }
}
