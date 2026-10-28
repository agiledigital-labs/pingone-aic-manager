//! `aic esv` parser and ESV-variable command implementation.

use clap::Subcommand;
use serde_json::Value;

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use crate::cli::{
    clip, json_bool_cell, json_str_cell, print_json, print_table, prod_hint, tenant_for,
};
use crate::esv::api::StartupStatus;
use crate::esv::state::ApplyState;
use crate::secrets::cli::SecretCommand;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum EsvCommand {
    /// List ESV variables.
    List {
        /// Override the current context for this call.
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print variables as JSON")]
        json: bool,
    },
    /// Get a single variable as JSON.
    Get {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Create or update a variable.
    Set {
        id: String,
        /// Plain value (stored base64-encoded as `valueBase64`).
        #[arg(long)]
        value: String,
        /// expressionType: string, int, bool, list, object, array, keyvaluelist.
        #[arg(long = "type", default_value = "string")]
        expr_type: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete a variable.
    Delete {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Whether changes are waiting to be applied, and whether a restart is
    /// running. Deletes are never reported as pending (measured).
    Status {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print the status as JSON")]
        json: bool,
    },
    /// Apply pending changes by restarting the tenant runtime. Refused while
    /// a restart is already running.
    Apply {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        /// Wait until the restart has finished, showing its progress.
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many minutes (the restart carries on).
        #[arg(long, requires = "wait", value_name = "MINUTES")]
        timeout: Option<u64>,
    },
    /// Secret operations (versioned, write-only values).
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
}

pub async fn run(cmd: EsvCommand) -> Result<()> {
    use crate::esv::api;
    match cmd {
        EsvCommand::List { tenant, json } => {
            let t = tenant_for(tenant)?;
            let variables = api::list_variables(&t).await?;
            if json {
                print_json(&variables)
            } else {
                print_variables(&variables);
                Ok(())
            }
        }
        EsvCommand::Get { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&api::get_variable(&t, &id).await?)
        }
        EsvCommand::Set {
            id,
            value,
            expr_type,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            use base64::Engine as _;
            let value_b64 = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
            // Shared with the TUI: handles the AIC quirk that an existing
            // variable's type can't change in place (DELETE-then-PUT).
            let saved = prod_hint(
                api::save_variable(&t, &id, &description, &expr_type, &value_b64, yes, None).await,
            )?;
            let verb = if saved.created { "created" } else { "saved" };
            let extra = if saved.type_deleted {
                " (type changed — recreated)"
            } else {
                ""
            };
            println!("variable {id} {verb}{extra}");
            Ok(())
        }
        EsvCommand::Delete { id, tenant, yes } => {
            let t = tenant_for(tenant)?;
            prod_hint(api::delete_variable(&t, &id, yes).await)?;
            println!("variable {id} deleted");
            Ok(())
        }
        EsvCommand::Status { tenant, json } => {
            let t = tenant_for(tenant)?;
            let (startup, pending) = read_state(&t).await?;
            if json {
                print_json(&status_json(startup, &pending))
            } else {
                print_status(startup, &pending);
                Ok(())
            }
        }
        EsvCommand::Apply {
            tenant,
            yes,
            wait,
            timeout,
        } => {
            let t = tenant_for(tenant)?;
            if api::startup_status(&t).await? == StartupStatus::Restarting {
                return Err(Error::Config(format!(
                    "{t} is already restarting; `aic esv status --tenant {t}` shows when it is done"
                )));
            }
            let body = prod_hint(api::trigger_restart(&t, yes).await)?;
            if !wait {
                return print_json(&body);
            }
            let limit = Duration::from_secs(60 * timeout.unwrap_or(DEFAULT_WAIT_MINUTES));
            wait_for_restart(&t, limit).await
        }
        EsvCommand::Secret { command } => crate::secrets::cli::run(command).await,
    }
}

fn print_variables(variables: &[Value]) {
    let rows = variables
        .iter()
        .map(|variable| {
            vec![
                json_str_cell(variable, "_id"),
                json_str_cell(variable, "expressionType"),
                json_bool_cell(variable, "loaded"),
                json_str_cell(variable, "lastChangeDate"),
                clip(&json_str_cell(variable, "description"), 72),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        &["ID", "TYPE", "LOADED", "LAST_CHANGE", "DESCRIPTION"],
        &rows,
    );
}

/// The ids `?_onlyPending=true` names: the same two lists the ESV tab polls.
/// Deletes are not among them (measured 2026-05-29, `docs/api/03-esvs.md`).
struct Pending {
    variables: Vec<String>,
    secrets: Vec<String>,
}

impl Pending {
    fn count(&self) -> usize {
        self.variables.len() + self.secrets.len()
    }
}

async fn read_state(tenant: &str) -> Result<(StartupStatus, Pending)> {
    use crate::esv::api;
    let ids = |rows: Vec<Value>| -> Vec<String> {
        rows.iter()
            .map(|row| {
                row.get("_id")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string()
            })
            .collect()
    };
    let (startup, variables, secrets) = tokio::try_join!(
        api::startup_status(tenant),
        api::list_pending_variables(tenant),
        api::list_pending_secrets(tenant),
    )?;
    Ok((
        startup,
        Pending {
            variables: ids(variables),
            secrets: ids(secrets),
        },
    ))
}

fn headline(startup: StartupStatus, pending: &Pending) -> String {
    match ApplyState::from_authoritative(startup, pending.count()) {
        ApplyState::NoChanges => "no pending changes".into(),
        ApplyState::Unapplied(n) => format!("{n} pending — `aic esv apply` restarts to load them"),
        ApplyState::Restarting(0) => "restarting".into(),
        ApplyState::Restarting(n) => format!("restarting, {n} still pending"),
    }
}

fn print_status(startup: StartupStatus, pending: &Pending) {
    println!("{}", headline(startup, pending));
    for id in &pending.variables {
        println!("  variable  {id}");
    }
    for id in &pending.secrets {
        println!("  secret    {id}");
    }
    println!("deletes are never listed as pending, so a pending delete does not show here");
}

fn status_json(startup: StartupStatus, pending: &Pending) -> Value {
    serde_json::json!({
        "restartStatus": match startup {
            StartupStatus::Ready => "ready",
            StartupStatus::Restarting => "restarting",
        },
        "pending": { "variables": pending.variables, "secrets": pending.secrets },
    })
}

const DEFAULT_WAIT_MINUTES: u64 = 15;

/// Between status reads while waiting. Reads, not restarts, so they are not
/// what the tighter restart rate limit is about; the TUI reads every 30s.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Consecutive failed reads that end the wait, matching `aic promotion`'s.
const POLL_READ_FAILURES: u32 = 6;

/// Whether a restart that was sent has finished. `ready` ends it only after
/// a read said `restarting`, because `ready` is also where it starts. The
/// first read after the restart call already said `restarting` (measured
/// 2026-10-28), so this does not wait on a restart that never shows.
#[derive(Default)]
struct Watch {
    seen_restarting: bool,
}

impl Watch {
    fn finished(&mut self, startup: StartupStatus) -> bool {
        match startup {
            StartupStatus::Restarting => {
                self.seen_restarting = true;
                false
            }
            StartupStatus::Ready => self.seen_restarting,
        }
    }
}

async fn wait_for_restart(tenant: &str, limit: Duration) -> Result<()> {
    let started = Instant::now();
    let mut progress = Progress::new();
    let mut watch = Watch::default();
    let mut failures = 0;
    loop {
        match read_state(tenant).await {
            Ok((startup, pending)) => {
                failures = 0;
                if watch.finished(startup) {
                    progress.clear();
                    let took = started.elapsed().as_secs();
                    println!("{tenant} restarted in {}m{:02}s", took / 60, took % 60);
                    return match pending.count() {
                        0 => Ok(()),
                        n => Err(Error::Config(format!(
                            "{n} change(s) still pending after the restart — `aic esv status --tenant {tenant}`"
                        ))),
                    };
                }
                progress.show(&headline(startup, &pending), started.elapsed());
            }
            Err(error) => {
                failures += 1;
                progress.clear();
                eprintln!("  could not read the status ({failures}/{POLL_READ_FAILURES}): {error}");
                if failures >= POLL_READ_FAILURES {
                    return Err(Error::Config(format!(
                        "gave up reading {tenant}'s status; the restart carries on — `aic esv status --tenant {tenant}`"
                    )));
                }
            }
        }
        if started.elapsed() >= limit {
            progress.clear();
            return Err(Error::Config(format!(
                "still not finished after {} minutes; the restart carries on — `aic esv status --tenant {tenant}`",
                limit.as_secs() / 60
            )));
        }
        progress
            .tick_until(started, Instant::now() + POLL_INTERVAL)
            .await;
    }
}

/// One live stderr line at a terminal; elsewhere, one line per change of
/// state, so a log is not flooded. The total is unknown, so it shows elapsed
/// time, not a fraction.
struct Progress {
    live: bool,
    last: String,
}

impl Progress {
    fn new() -> Self {
        Self {
            live: std::io::stderr().is_terminal(),
            last: String::new(),
        }
    }

    fn show(&mut self, state: &str, elapsed: Duration) {
        if self.live {
            let secs = elapsed.as_secs();
            let spin = ['|', '/', '-', '\\'][(secs % 4) as usize];
            eprint!(
                "\r\x1b[2K  {spin} {state} — {}m{:02}s",
                secs / 60,
                secs % 60
            );
            let _ = std::io::stderr().flush();
        } else if state != self.last {
            eprintln!("  {state}");
        }
        self.last = state.to_string();
    }

    async fn tick_until(&mut self, started: Instant, until: Instant) {
        while Instant::now() < until {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if self.live && !self.last.is_empty() {
                let state = self.last.clone();
                self.show(&state, started.elapsed());
            }
        }
    }

    fn clear(&self) {
        if self.live {
            eprint!("\r\x1b[2K");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_ends_a_restart_only_after_restarting_was_read() {
        let mut watch = Watch::default();
        assert!(
            !watch.finished(StartupStatus::Ready),
            "ready is also where it starts"
        );
        assert!(!watch.finished(StartupStatus::Restarting));
        assert!(!watch.finished(StartupStatus::Restarting));
        assert!(watch.finished(StartupStatus::Ready));
    }

    #[test]
    fn headline_names_the_apply_state() {
        let none = Pending {
            variables: vec![],
            secrets: vec![],
        };
        let two = Pending {
            variables: vec!["esv-a".into()],
            secrets: vec!["esv-b".into()],
        };
        assert_eq!(headline(StartupStatus::Ready, &none), "no pending changes");
        assert!(headline(StartupStatus::Ready, &two).starts_with("2 pending"));
        assert_eq!(headline(StartupStatus::Restarting, &none), "restarting");
        assert_eq!(
            headline(StartupStatus::Restarting, &two),
            "restarting, 2 still pending"
        );
    }

    #[test]
    fn status_json_lists_ids_by_kind() {
        let pending = Pending {
            variables: vec!["esv-a".into()],
            secrets: vec![],
        };
        assert_eq!(
            status_json(StartupStatus::Restarting, &pending),
            serde_json::json!({
                "restartStatus": "restarting",
                "pending": { "variables": ["esv-a"], "secrets": [] },
            })
        );
    }
}
