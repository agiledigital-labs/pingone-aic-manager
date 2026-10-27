//! `aic promotion` parser and command implementation. `status`, `reports`,
//! `report` and `verify` read; `lock`, `dry-run`, `promote` and `unlock`
//! write, each through a `lifecycle` permit.

use std::time::{Duration, Instant};

use clap::{Args, Subcommand};

use futures::StreamExt;

use crate::cli::force::OperationForce;
use crate::cli::{
    confirm_destructive, ensure_prod_confirmed, print_json, print_table, prompt_available,
    tenant_config_for, tenant_for,
};
use crate::promotion::lifecycle::{self, Phase, Settle};
use crate::promotion::{api, spec, verify};
use crate::saml::write::consent;
use crate::{Error, Result};

/// What every write verb takes. Sent to the **upper** environment of the
/// pair — the one promoted to; the lower one is refused.
#[derive(Args, Debug)]
pub struct WriteArgs {
    /// The upper environment of the pair (the one promoted *to*).
    #[arg(long)]
    tenant: Option<String>,
    /// Confirm a write to a production-themed tenant.
    #[arg(long)]
    yes: bool,
    /// Proceed without a prompt.
    #[command(flatten)]
    force: OperationForce,
    /// Return once the tenant accepts the request, instead of waiting for it
    /// to settle.
    #[arg(long)]
    no_wait: bool,
    /// Minutes to wait before giving up on the tenant settling (the
    /// operation carries on there). Default: 15 for lock/unlock, 45 for
    /// dry-run/promote — measured ~3 and ~8½.
    #[arg(long, value_name = "MINUTES")]
    timeout: Option<u64>,
}

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
    /// Lock the pair for promotion: neither environment accepts configuration
    /// changes until `unlock`. Measured to settle in ~3 minutes.
    Lock(WriteArgs),
    /// Run a dry-run promotion on a locked pair and name the report it
    /// produced. Changes no configuration; measured ~8½ minutes. A report
    /// lists items the promotion did not change, so check one with `verify`.
    DryRun(WriteArgs),
    /// Promote the lower environment's configuration into the upper one, on
    /// a locked pair. Every difference becomes live on the upper environment.
    Promote(WriteArgs),
    /// Release the lock, by the promotionId the tenant reports. Refused while
    /// a promotion is running.
    Unlock(WriteArgs),
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
        PromotionCommand::Lock(args) => run_write(Verb::Lock, args).await,
        PromotionCommand::DryRun(args) => run_write(Verb::Promote { dry_run: true }, args).await,
        PromotionCommand::Promote(args) => run_write(Verb::Promote { dry_run: false }, args).await,
        PromotionCommand::Unlock(args) => run_write(Verb::Unlock, args).await,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Lock,
    Promote { dry_run: bool },
    Unlock,
}

impl Verb {
    fn name(self) -> &'static str {
        match self {
            Verb::Lock => "lock",
            Verb::Promote { dry_run: true } => "dry run",
            Verb::Promote { dry_run: false } => "promotion",
            Verb::Unlock => "unlock",
        }
    }

    fn default_timeout(self) -> Duration {
        Duration::from_secs(
            60 * if matches!(self, Verb::Promote { .. }) {
                45
            } else {
                15
            },
        )
    }
}

/// Between status reads while waiting. Each settle measured took minutes;
/// the reads are cheap but there is no reason to hammer the service.
const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// Consecutive failed status reads tolerated while waiting before giving up.
const POLL_READ_FAILURES: u32 = 3;

async fn assessment(tenant: &str) -> Result<spec::Assessment> {
    let lock_raw = explained(api::lock_state(tenant).await, tenant)?;
    let status_raw = explained(api::status(tenant).await, tenant)?;
    decoded(spec::assess(&lock_raw, &status_raw))
}

fn prompt_for(verb: Verb, tenant: &str, a: &spec::Assessment) -> String {
    match verb {
        Verb::Lock => format!(
            "Lock {tenant} and the environment it promotes from? Neither accepts configuration              changes until unlocked."
        ),
        Verb::Promote { dry_run: true } => {
            format!("Start a dry-run promotion into {tenant}? (No configuration change.)")
        }
        Verb::Promote { dry_run: false } => format!(
            "Promote into {tenant}? Every configuration difference from the lower environment              becomes live on {tenant}."
        ),
        Verb::Unlock => format!(
            "Unlock {tenant} (promotionId {})?",
            a.status.promotion_id.as_deref().unwrap_or("-")
        ),
    }
}

/// The three writes share one shape: read, check the state, consent, mint
/// the permit, production gate, send, then wait for the tenant to settle.
/// The state check runs before the prompt so a refused state never asks.
async fn run_write(verb: Verb, args: WriteArgs) -> Result<()> {
    let tenant = tenant_for(args.tenant)?;
    let a = assessment(&tenant).await?;
    if verb == Verb::Unlock && lifecycle::phase(&a) == Phase::Unlocked {
        lifecycle::pair_ok(&a, &tenant).map_err(Error::Config)?;
        println!("{tenant} is already unlocked");
        return Ok(());
    }
    let check = match verb {
        Verb::Lock => lifecycle::check_lock(&a, &tenant),
        Verb::Promote { .. } => lifecycle::check_promote(&a, &tenant),
        Verb::Unlock => lifecycle::check_unlock(&a, &tenant).map(|_| ()),
    };
    check.map_err(Error::Config)?;

    let consented = consent(args.force.operation(), prompt_available(), || {
        confirm_destructive(verb.name(), &prompt_for(verb, &tenant, &a), "--force")
    })?;

    // Each arm mints its own permit and sends; the permit types differ, so
    // the send cannot be shared without erasing which permit it needed.
    let reports_before = match verb {
        Verb::Promote { .. } => Some(explained(api::reports(&tenant).await, &tenant)?),
        _ => None,
    };
    let response = match verb {
        Verb::Lock => {
            let permit =
                lifecycle::authorize_lock(&a, &tenant, consented).map_err(Error::Config)?;
            let ok = ensure_prod_confirmed(&tenant, args.yes)?;
            api::lock(&tenant, ok.confirmed_prod, &permit).await
        }
        Verb::Promote { dry_run } => {
            let permit = lifecycle::authorize_promote(&a, &tenant, dry_run, consented)
                .map_err(Error::Config)?;
            let ok = ensure_prod_confirmed(&tenant, args.yes)?;
            api::promote(&tenant, ok.confirmed_prod, &permit).await
        }
        Verb::Unlock => {
            let permit =
                lifecycle::authorize_unlock(&a, &tenant, consented).map_err(Error::Config)?;
            let ok = ensure_prod_confirmed(&tenant, args.yes)?;
            api::unlock(&tenant, ok.confirmed_prod, &permit).await
        }
    };
    let response = explained(crate::cli::prod_hint(response), &tenant)?;
    let said = |k: &str| response.get(k).and_then(serde_json::Value::as_str);
    println!(
        "{tenant} accepted the {}: {}",
        verb.name(),
        said("description")
            .or(said("result"))
            .unwrap_or("(no message)")
    );
    if args.no_wait {
        println!("not waiting — `aic promotion status --tenant {tenant}` shows progress");
        return Ok(());
    }

    let timeout = args
        .timeout
        .map_or_else(|| verb.default_timeout(), |m| Duration::from_secs(60 * m));
    let mut watch = lifecycle::PromoteWatch {
        timestamp_before: a.status.timestamp.clone(),
        seen_running: false,
    };
    let settled = wait(&tenant, verb, timeout, |a| match verb {
        Verb::Lock => lifecycle::lock_settled(a),
        Verb::Promote { .. } => lifecycle::promote_settled(a, &mut watch),
        Verb::Unlock => lifecycle::unlock_settled(a),
    })
    .await?;

    match verb {
        Verb::Lock => println!(
            "{tenant} is locked (promotionId {})",
            settled
                .status
                .promotion_id
                .as_deref()
                .unwrap_or("not reported")
        ),
        Verb::Unlock => println!("{tenant} is unlocked"),
        Verb::Promote { dry_run } => {
            println!(
                "the {} finished: {}",
                verb.name(),
                settled.status.message.as_deref().unwrap_or("(no message)")
            );
            report_produced(&tenant, &settled, reports_before.as_ref(), dry_run).await?;
            println!(
                "{tenant} is still locked — `aic promotion unlock --tenant {tenant}` releases it"
            );
        }
    }
    Ok(())
}

/// Poll until `judge` says done or failed, printing each new reading once.
async fn wait(
    tenant: &str,
    verb: Verb,
    timeout: Duration,
    mut judge: impl FnMut(&spec::Assessment) -> Settle,
) -> Result<spec::Assessment> {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    let mut failures = 0;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let a = match assessment(tenant).await {
            Ok(a) => {
                failures = 0;
                a
            }
            Err(error) => {
                failures += 1;
                eprintln!("  could not read the status ({failures}/{POLL_READ_FAILURES}): {error}");
                if failures >= POLL_READ_FAILURES {
                    return Err(Error::Config(format!(
                        "lost track of the {} on {tenant}; it may still be running there —                          `aic promotion status --tenant {tenant}`",
                        verb.name()
                    )));
                }
                continue;
            }
        };
        match judge(&a) {
            Settle::Done => return Ok(a),
            Settle::Failed(why) => {
                return Err(Error::Config(format!(
                    "the {} on {tenant} did not settle as expected: {why}",
                    verb.name()
                )));
            }
            Settle::Pending(now) => {
                if now != last {
                    eprintln!("  {now}");
                    last = now;
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::Config(format!(
                "the {} on {tenant} did not settle within {} minutes; it carries on there —                  `aic promotion status --tenant {tenant}`",
                verb.name(),
                timeout.as_secs() / 60
            )));
        }
    }
}

/// Name the report a dry run or promotion produced, by difference from the
/// list read before it started.
async fn report_produced(
    tenant: &str,
    settled: &spec::Assessment,
    before: Option<&serde_json::Value>,
    dry_run: bool,
) -> Result<()> {
    let (Some(before), Some(id)) = (before, settled.status.promotion_id.as_deref()) else {
        println!("could not identify the report: no promotionId on the final status");
        return Ok(());
    };
    let after = explained(api::reports(tenant).await, tenant)?;
    match decoded(lifecycle::new_reports(before, &after, id, dry_run))?.as_slice() {
        [] => println!("no new report found for promotionId {id}"),
        ids => {
            for report in ids {
                println!("report {report} — `aic promotion report {report} --tenant {tenant}`");
            }
            println!(
                "a report lists items the promotion did not change (measured);                  `aic promotion verify --lower <env> --upper {tenant} --report <id>` checks them live"
            );
        }
    }
    Ok(())
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
