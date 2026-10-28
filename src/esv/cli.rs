//! `aic esv` parser and ESV-variable command implementation.

use clap::Subcommand;
use serde_json::Value;

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use tokio::time::Instant;

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
        #[arg(long, requires = "wait", value_name = "MINUTES",
              value_parser = clap::value_parser!(u64).range(1..=MAX_WAIT_MINUTES))]
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
            let limit = Duration::from_secs(60 * timeout.unwrap_or(DEFAULT_WAIT_MINUTES));
            if api::startup_status(&t).await? == StartupStatus::Restarting {
                return Err(Error::Config(format!(
                    "{t} is already restarting; `aic esv status --tenant {t}` shows when it is done"
                )));
            }
            let body = prod_hint(api::trigger_restart(&t, yes).await)?;
            if !wait {
                return print_json(&body);
            }
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
#[derive(Debug, PartialEq)]
struct Pending {
    variables: Vec<String>,
    secrets: Vec<String>,
}

impl Pending {
    fn count(&self) -> usize {
        self.variables.len() + self.secrets.len()
    }
}

/// The ids in one `?_onlyPending=true` answer, refusing one that says more
/// pages follow: a partial list would under-count, and an empty first page
/// would read as "nothing pending". An empty cookie ends a list (measured,
/// `docs/api/03-esvs.md`); `remainingPagedResults` of -1 means unknown. Paging
/// metadata of any other type is refused, not ignored: ignoring it is what
/// would turn "more follow" into "complete".
fn complete_ids(body: &Value, path: &str) -> Result<Vec<String>> {
    let shape = || Error::Config(format!("unexpected {path} response shape: {body}"));
    let more = match body.get("pagedResultsCookie") {
        None | Some(Value::Null) => false,
        Some(Value::String(cookie)) => !cookie.is_empty(),
        Some(_) => return Err(shape()),
    } || match body.get("remainingPagedResults") {
        None | Some(Value::Null) => false,
        Some(n) => n.as_i64().ok_or_else(shape)? > 0,
    };
    if more {
        return Err(Error::Config(format!(
            "{path} answered one page of several; refusing a partial pending list"
        )));
    }
    body.get("result")
        .and_then(Value::as_array)
        .ok_or_else(shape)?
        .iter()
        .map(|row| {
            row.get("_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(shape)
        })
        .collect()
}

async fn pending_ids(tenant: &str, path: &str) -> Result<Vec<String>> {
    complete_ids(&crate::aic::api::get(tenant, path).await?, path)
}

async fn read_pending(tenant: &str) -> Result<Pending> {
    let (variables, secrets) = tokio::try_join!(
        pending_ids(tenant, "/environment/variables?_onlyPending=true"),
        pending_ids(tenant, "/environment/secrets?_onlyPending=true"),
    )?;
    Ok(Pending { variables, secrets })
}

async fn read_state(tenant: &str) -> Result<(StartupStatus, Pending)> {
    tokio::try_join!(
        crate::esv::api::startup_status(tenant),
        read_pending(tenant)
    )
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
const MAX_WAIT_MINUTES: u64 = 24 * 60;

/// Between status reads while waiting. Reads, not restarts, so they are not
/// what the tighter restart rate limit is about; the TUI reads every 30s.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Consecutive failed reads that end the wait, matching `aic promotion`'s.
const POLL_READ_FAILURES: u32 = 6;

/// Whether a restart that was sent has finished. `ready` ends it only after
/// a read said `restarting`, because `ready` is also where it starts. The
/// first read after the restart call already said `restarting` (measured
/// 2026-10-28), so this does not wait on a restart that never shows. The API
/// names no restart, so one someone else started in the gap between the
/// pre-check and the `POST` is indistinguishable from this one.
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

#[derive(Debug)]
enum WaitEnd {
    /// The restart finished, and these were pending at a read taken after.
    Finished(Pending),
    /// The deadline passed; `restarted` says whether completion was seen.
    TimedOut { restarted: bool },
    /// Too many reads in a row failed; the last error, and whether
    /// completion had been seen.
    Unreadable { error: Error, restarted: bool },
}

enum Seen<'a> {
    Restarting,
    Restarted,
    ReadFailed(u32, &'a Error),
}

/// The waiting itself, with every read and pause injected so a test drives
/// the real sequence. Two phases, never mixed: startup reads until `Watch`
/// says the restart finished, then fresh pending reads, so the final
/// pending list is always one taken after completion and a pending-list
/// failure can never hide a `restarting` read. Every read and every pause
/// is bounded by `deadline`, because the transport has no response timeout.
async fn wait_with<S, SF, P, PF, Z, ZF>(
    deadline: Instant,
    mut startup: S,
    mut pending: P,
    mut pause: Z,
    mut seen: impl FnMut(Seen<'_>),
) -> WaitEnd
where
    S: FnMut() -> SF,
    SF: Future<Output = Result<StartupStatus>>,
    P: FnMut() -> PF,
    PF: Future<Output = Result<Pending>>,
    Z: FnMut(Instant) -> ZF,
    ZF: Future<Output = ()>,
{
    let mut watch = Watch::default();
    let mut restarted = false;
    let mut failures = 0;
    loop {
        // `timeout_at` polls the read before its deadline, so a read that
        // answers at once would otherwise still count after the deadline.
        if Instant::now() >= deadline {
            return WaitEnd::TimedOut { restarted };
        }
        let failed = if restarted {
            match tokio::time::timeout_at(deadline, pending()).await {
                Err(_) => return WaitEnd::TimedOut { restarted },
                // The read was polled before the timer, so one that
                // answered only after the deadline still lands here.
                Ok(Ok(_)) if Instant::now() > deadline => {
                    return WaitEnd::TimedOut { restarted };
                }
                Ok(Ok(list)) => return WaitEnd::Finished(list),
                Ok(Err(error)) => error,
            }
        } else {
            match tokio::time::timeout_at(deadline, startup()).await {
                Err(_) => return WaitEnd::TimedOut { restarted },
                Ok(Ok(_)) if Instant::now() > deadline => {
                    return WaitEnd::TimedOut { restarted };
                }
                Ok(Ok(status)) => {
                    failures = 0;
                    restarted = watch.finished(status);
                    if restarted {
                        seen(Seen::Restarted);
                        continue;
                    }
                    if status == StartupStatus::Restarting {
                        seen(Seen::Restarting);
                    }
                    pause_until(deadline, &mut pause).await;
                    continue;
                }
                Ok(Err(error)) => error,
            }
        };
        failures += 1;
        seen(Seen::ReadFailed(failures, &failed));
        if failures >= POLL_READ_FAILURES {
            return WaitEnd::Unreadable {
                error: failed,
                restarted,
            };
        }
        pause_until(deadline, &mut pause).await;
    }
}

/// One poll interval, cut short at the deadline; the next read then times
/// out at once rather than starting late.
async fn pause_until<Z, ZF>(deadline: Instant, pause: &mut Z)
where
    Z: FnMut(Instant) -> ZF,
    ZF: Future<Output = ()>,
{
    pause((Instant::now() + POLL_INTERVAL).min(deadline)).await;
}

async fn wait_for_restart(tenant: &str, limit: Duration) -> Result<()> {
    let started = Instant::now();
    let progress = &Progress::new(started);
    let end = wait_with(
        started + limit,
        || crate::esv::api::startup_status(tenant),
        || read_pending(tenant),
        move |until| async move { progress.tick_until(until).await },
        |event| progress.seen(event),
    )
    .await;
    progress.clear();
    let status = format!("`aic esv status --tenant {tenant}`");
    match end {
        WaitEnd::Finished(pending) => {
            let took = started.elapsed().as_secs();
            println!("{tenant} restarted in {}m{:02}s", took / 60, took % 60);
            match pending.count() {
                0 => Ok(()),
                n => Err(Error::Config(format!(
                    "{n} change(s) still pending after the restart — {status}"
                ))),
            }
        }
        WaitEnd::TimedOut { restarted: true } => Err(Error::Config(format!(
            "{tenant} restarted, but the pending lists could not be read in time — {status}"
        ))),
        WaitEnd::TimedOut { restarted: false } => Err(Error::Config(format!(
            "still not finished after {} minutes; the restart carries on — {status}",
            limit.as_secs() / 60
        ))),
        WaitEnd::Unreadable {
            error,
            restarted: true,
        } => Err(Error::Config(format!(
            "{tenant} restarted, but the pending lists could not be read ({error}) — {status}"
        ))),
        WaitEnd::Unreadable {
            error,
            restarted: false,
        } => Err(Error::Config(format!(
            "gave up after {POLL_READ_FAILURES} failed reads ({error}); the restart carries on — {status}"
        ))),
    }
}

/// One live stderr line at a terminal; elsewhere, one line per change, so a
/// log is not flooded. The total is unknown, so it shows elapsed time, not
/// a fraction.
struct Progress {
    live: bool,
    started: Instant,
    restarting: std::cell::Cell<bool>,
}

impl Progress {
    fn new(started: Instant) -> Self {
        Self {
            live: std::io::stderr().is_terminal(),
            started,
            restarting: std::cell::Cell::new(false),
        }
    }

    fn seen(&self, event: Seen<'_>) {
        match event {
            Seen::Restarting => {
                if !self.restarting.replace(true) && !self.live {
                    eprintln!("  restarting");
                }
                self.draw();
            }
            Seen::Restarted => {
                self.restarting.set(false);
                self.clear();
            }
            Seen::ReadFailed(n, error) => {
                self.clear();
                eprintln!("  could not read the status ({n}/{POLL_READ_FAILURES}): {error}");
                self.draw();
            }
        }
    }

    fn draw(&self) {
        if self.live && self.restarting.get() {
            let secs = self.started.elapsed().as_secs();
            let spin = ['|', '/', '-', '\\'][(secs % 4) as usize];
            eprint!(
                "\r\x1b[2K  {spin} restarting — {}m{:02}s",
                secs / 60,
                secs % 60
            );
            let _ = std::io::stderr().flush();
        }
    }

    async fn tick_until(&self, until: Instant) {
        while Instant::now() < until {
            let next = (Instant::now() + Duration::from_secs(1)).min(until);
            tokio::time::sleep_until(next).await;
            self.draw();
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
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    fn none() -> Pending {
        Pending {
            variables: vec![],
            secrets: vec![],
        }
    }

    /// Scripted reads that log their order; an exhausted script never answers.
    struct Script {
        log: RefCell<Vec<String>>,
        startup: RefCell<VecDeque<Result<StartupStatus>>>,
        pending: RefCell<VecDeque<Result<Pending>>>,
    }

    impl Script {
        fn new(startup: Vec<Result<StartupStatus>>, pending: Vec<Result<Pending>>) -> Self {
            Self {
                log: RefCell::new(vec![]),
                startup: RefCell::new(startup.into()),
                pending: RefCell::new(pending.into()),
            }
        }

        async fn run(&self, within: Duration) -> WaitEnd {
            self.run_pausing(within, Duration::ZERO).await
        }

        /// `overshoot` makes every pause end that long after the instant
        /// asked for, as a late scheduler would.
        async fn run_pausing(&self, within: Duration, overshoot: Duration) -> WaitEnd {
            wait_with(
                Instant::now() + within,
                || async {
                    self.log.borrow_mut().push("startup".into());
                    let next = self.startup.borrow_mut().pop_front();
                    match next {
                        Some(r) => r,
                        None => std::future::pending().await,
                    }
                },
                || async {
                    self.log.borrow_mut().push("pending".into());
                    let next = self.pending.borrow_mut().pop_front();
                    match next {
                        Some(r) => r,
                        None => std::future::pending().await,
                    }
                },
                |until| async move {
                    if !overshoot.is_zero() {
                        tokio::time::sleep_until(until + overshoot).await;
                    }
                },
                |_| {},
            )
            .await
        }
    }

    fn failed() -> Error {
        Error::Config("500".into())
    }

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

    #[tokio::test]
    async fn pending_is_read_only_after_the_restart_finished() {
        let still = || Pending {
            variables: vec!["esv-a".into()],
            secrets: vec![],
        };
        let script = Script::new(
            vec![Ok(StartupStatus::Restarting), Ok(StartupStatus::Ready)],
            vec![Ok(still())],
        );
        let end = script.run(Duration::from_secs(5)).await;
        assert!(matches!(end, WaitEnd::Finished(p) if p == still()));
        assert_eq!(*script.log.borrow(), ["startup", "startup", "pending"]);
    }

    #[tokio::test]
    async fn a_failed_pending_read_is_retried_not_counted_as_finished() {
        let script = Script::new(
            vec![Ok(StartupStatus::Restarting), Ok(StartupStatus::Ready)],
            vec![Err(failed()), Ok(none())],
        );
        let end = script.run(Duration::from_secs(5)).await;
        assert!(matches!(end, WaitEnd::Finished(_)));
        assert_eq!(
            *script.log.borrow(),
            ["startup", "startup", "pending", "pending"]
        );
    }

    #[tokio::test]
    async fn a_read_that_never_answers_ends_at_the_deadline() {
        let script = Script::new(vec![Ok(StartupStatus::Restarting)], vec![]);
        let end = tokio::time::timeout(
            Duration::from_secs(5),
            script.run(Duration::from_millis(50)),
        )
        .await
        .expect("the deadline must bound a stalled read");
        assert!(matches!(end, WaitEnd::TimedOut { restarted: false }));
    }

    #[tokio::test]
    async fn a_read_due_after_the_deadline_is_not_taken() {
        let script = Script::new(
            vec![Ok(StartupStatus::Restarting), Ok(StartupStatus::Ready)],
            vec![Ok(none())],
        );
        let end = script
            .run_pausing(Duration::from_millis(20), Duration::from_millis(50))
            .await;
        assert!(matches!(end, WaitEnd::TimedOut { restarted: false }));
        assert_eq!(*script.log.borrow(), ["startup"]);
    }

    #[tokio::test]
    async fn a_stalled_pending_read_after_completion_says_it_restarted() {
        let script = Script::new(
            vec![Ok(StartupStatus::Restarting), Ok(StartupStatus::Ready)],
            vec![],
        );
        let end = tokio::time::timeout(
            Duration::from_secs(5),
            script.run(Duration::from_millis(50)),
        )
        .await
        .expect("the deadline must bound a stalled read");
        assert!(matches!(end, WaitEnd::TimedOut { restarted: true }));
    }

    #[tokio::test]
    async fn six_failed_reads_in_a_row_end_the_wait() {
        let mut startup = vec![Ok(StartupStatus::Restarting)];
        startup.extend((0..POLL_READ_FAILURES).map(|_| Err(failed())));
        let script = Script::new(startup, vec![]);
        let end = script.run(Duration::from_secs(5)).await;
        assert!(matches!(
            end,
            WaitEnd::Unreadable {
                restarted: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn pending_reads_failing_after_completion_say_it_restarted() {
        let script = Script::new(
            vec![Ok(StartupStatus::Restarting), Ok(StartupStatus::Ready)],
            (0..POLL_READ_FAILURES).map(|_| Err(failed())).collect(),
        );
        let end = script.run(Duration::from_secs(5)).await;
        assert!(matches!(
            end,
            WaitEnd::Unreadable {
                restarted: true,
                ..
            }
        ));
    }

    /// A read whose answer is ready only once the deadline has passed: the
    /// thread is held inside the poll, as a late-scheduled task would be.
    /// `late_startup` makes the second startup read (`ready`) the late one,
    /// so completion must not be recorded; otherwise the pending read is late
    /// and must not become `Finished`. The margins are wide so preemption
    /// before the late read cannot spend the deadline first.
    #[tokio::test]
    async fn an_answer_after_the_deadline_is_not_taken() {
        for late_startup in [true, false] {
            let deadline = Instant::now() + Duration::from_millis(300);
            let reads = std::cell::Cell::new(0);
            let late = || std::thread::sleep(Duration::from_millis(600));
            let end = wait_with(
                deadline,
                || async {
                    reads.set(reads.get() + 1);
                    if reads.get() == 1 {
                        return Ok(StartupStatus::Restarting);
                    }
                    if late_startup {
                        late();
                    }
                    Ok(StartupStatus::Ready)
                },
                || async {
                    late();
                    Ok(none())
                },
                |_| async {},
                |_| {},
            )
            .await;
            assert!(
                matches!(end, WaitEnd::TimedOut { restarted } if restarted != late_startup),
                "late_startup={late_startup}: {end:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_success_resets_the_failure_count() {
        let mut startup: Vec<Result<StartupStatus>> = vec![];
        for _ in 0..2 {
            startup.extend((1..POLL_READ_FAILURES).map(|_| Err(failed())));
            startup.push(Ok(StartupStatus::Restarting));
        }
        startup.push(Ok(StartupStatus::Ready));
        let script = Script::new(startup, vec![Ok(none())]);
        let end = script.run(Duration::from_secs(5)).await;
        assert!(matches!(end, WaitEnd::Finished(_)));
    }

    #[test]
    fn a_page_that_says_more_follow_is_refused() {
        let path = "/environment/variables?_onlyPending=true";
        let page = |extra: Value| {
            let mut body = serde_json::json!({"result": [{"_id": "esv-a"}]});
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            complete_ids(&body, path)
        };
        assert!(page(serde_json::json!({"pagedResultsCookie": "next"})).is_err());
        assert!(page(serde_json::json!({"remainingPagedResults": 1})).is_err());
        assert_eq!(
            page(serde_json::json!({"pagedResultsCookie": "", "remainingPagedResults": -1}))
                .unwrap(),
            ["esv-a"]
        );
        assert!(complete_ids(&serde_json::json!({"result": [{}]}), path).is_err());
        let empty = |extra: Value| {
            let mut body = serde_json::json!({"result": []});
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            complete_ids(&body, path)
        };
        assert!(
            empty(serde_json::json!({"remainingPagedResults": 9_223_372_036_854_775_808u64}))
                .is_err()
        );
        assert!(empty(serde_json::json!({"remainingPagedResults": "1"})).is_err());
        assert!(empty(serde_json::json!({"pagedResultsCookie": 7})).is_err());
        assert!(
            empty(serde_json::json!({"pagedResultsCookie": null, "remainingPagedResults": 0}))
                .unwrap()
                .is_empty()
        );
        assert!(complete_ids(&serde_json::json!({}), path).is_err());
    }

    #[test]
    fn headline_names_the_apply_state() {
        let two = Pending {
            variables: vec!["esv-a".into()],
            secrets: vec!["esv-b".into()],
        };
        assert_eq!(
            headline(StartupStatus::Ready, &none()),
            "no pending changes"
        );
        assert!(headline(StartupStatus::Ready, &two).starts_with("2 pending"));
        assert_eq!(headline(StartupStatus::Restarting, &none()), "restarting");
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
