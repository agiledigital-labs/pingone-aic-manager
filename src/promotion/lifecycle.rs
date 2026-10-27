//! Tenant-free half of the promotion write verbs.
//!
//! Four separate questions, each answered here and nowhere else:
//!
//! - **Eligibility** — which reading a verb may start from ([`phase`],
//!   [`check`]). Only combinations measured on 2026-10-27 qualify: every
//!   source agreeing, proxies included, and `blockingError` reported `false`.
//! - **Consent** — asked by the caller, between two reads.
//! - **Identity** — the lock the operator approved is the lock written to:
//!   [`begin`] re-reads both sources *after* consent and refuses if the
//!   eligibility or the `promotionId` changed, then mints the [`Permit`] from
//!   that fresh read. This narrows the race; with no measured server
//!   precondition it cannot close it.
//! - **Completion** — [`Watch::judge`] reports done only on evidence under
//!   the same `promotionId`: for a promotion, `READY` after `RUNNING` was
//!   seen. A timestamp change proves nothing, and a report is not evidence
//!   either — when one becomes readable is unmeasured, and it may be another
//!   run's — so reports are
//!   only named afterwards, as candidates.
//!
//! Measured on a dev → UAT pair, every call sent to the upper environment
//! (`docs/api/23-promotion.md`, "The write cycle").

use serde_json::Value;

use super::spec::{Assessment, LockState};

fn is(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Where the pair stands, read from both sources at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Every source unlocked: `lock/state` and both sides (state and proxy),
    /// the status `UNLOCKED` with `globalLock: UNLOCKED`, `blockingError`
    /// reported `false`.
    Unlocked,
    /// Every source locked, the status `READY` with `globalLock: LOCKED`,
    /// `blockingError` reported `false`. Measured after a lock settled and
    /// after a dry run finished. Carries the id an unlock releases.
    Ready { promotion_id: Option<String> },
    /// The promotion status says `RUNNING`.
    Running { message: Option<String> },
    /// The status reports `blockingError: true`.
    Blocked,
    /// Anything else — a transition, one side or proxy ahead of the other, a
    /// missing `blockingError`, a value never measured. Carries a description.
    Unsettled(String),
}

fn sides(lock: &LockState) -> [Option<&super::spec::EnvLock>; 2] {
    [lock.lower.as_ref(), lock.upper.as_ref()]
}

/// `lock/state`, both sides and both proxies read `want`. A side or proxy the
/// response omits fails: only the upper environment reports them, and a
/// proxy disagreeing with its side is a transition, not a state.
fn lock_reads(lock: &LockState, want: &str) -> bool {
    is(&lock.result, want)
        && sides(lock).iter().all(|side| {
            side.is_some_and(|s| {
                is(&s.state, want) && s.proxy_state.as_deref().is_some_and(|p| is(p, want))
            })
        })
}

fn describe(a: &Assessment) -> String {
    let side = |s: Option<&super::spec::EnvLock>| match s {
        Some(s) => format!("{}/{}", s.state, s.proxy_state.as_deref().unwrap_or("-")),
        None => "-".into(),
    };
    let [lower, upper] = sides(&a.lock);
    format!(
        "lock/state `{}` (lower `{}`, upper `{}`), promotion status `{}` (globalLock `{}`, \
         blockingError `{}`, promotionId `{}`){}",
        a.lock.result,
        side(lower),
        side(upper),
        a.status.status,
        a.status.global_lock.as_deref().unwrap_or("-"),
        a.status
            .blocking_error
            .map_or("-".to_string(), |b| b.to_string()),
        a.status.promotion_id.as_deref().unwrap_or("-"),
        a.status
            .message
            .as_ref()
            .map(|m| format!(": {m}"))
            .unwrap_or_default()
    )
}

/// The lock held and agreed on by every lock source: `lock/state`, both
/// sides and proxies, `globalLock`. Says nothing about the promotion status.
fn held(a: &Assessment) -> bool {
    lock_reads(&a.lock, "locked")
        && a.status
            .global_lock
            .as_deref()
            .is_some_and(|g| is(g, "LOCKED"))
}

pub fn phase(a: &Assessment) -> Phase {
    let status = &a.status;
    let global = |want: &str| status.global_lock.as_deref().is_some_and(|g| is(g, want));
    // `spec::disagreements` is the diagnostic every surface prints; a reading
    // it flags is never a write state, whatever the fields below say. Today
    // the explicit checks below cover every rule it has (a mutation removing
    // this term survives for that reason); it stays so a rule added there
    // binds here too.
    let agreed = a.disagreements.is_empty() && status.blocking_error == Some(false);
    if status.blocking_error == Some(true) {
        Phase::Blocked
    } else if is(&status.status, "RUNNING") {
        Phase::Running {
            message: status.message.clone(),
        }
    } else if agreed
        && is(&status.status, "UNLOCKED")
        && global("UNLOCKED")
        && lock_reads(&a.lock, "unlocked")
    {
        Phase::Unlocked
    } else if agreed && is(&status.status, "READY") && held(a) {
        Phase::Ready {
            promotion_id: status.promotion_id.clone(),
        }
    } else {
        Phase::Unsettled(describe(a))
    }
}

/// Every write verb is sent to the **upper** environment of a pair: only it
/// answers `lock/state` with both sides (measured).
pub fn pair_ok(a: &Assessment, tenant: &str) -> Result<(), String> {
    if a.lock.lower.is_some() && a.lock.upper.is_some() {
        Ok(())
    } else {
        Err(format!(
            "{tenant} did not report both sides of an environment pair, so it is not the upper \
             environment — run promotion verbs against the environment promoted *to*"
        ))
    }
}

/// What the operator asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    Lock,
    Promote {
        dry_run: bool,
    },
    Unlock,
    /// Release a lock a blocked promotion left behind. The operator names the
    /// `promotionId` they expect, and it must match the tenant's exactly.
    Recover {
        expected: String,
    },
}

impl Verb {
    pub fn name(&self) -> &'static str {
        match self {
            Verb::Lock => "lock",
            Verb::Promote { dry_run: true } => "dry run",
            Verb::Promote { dry_run: false } => "promotion",
            Verb::Unlock => "unlock",
            Verb::Recover { .. } => "recovery unlock",
        }
    }
}

fn refused(tenant: &str, a: &Assessment, verb: &Verb, phase: &Phase) -> String {
    match phase {
        Phase::Blocked => format!(
            "{tenant}'s promotion status reports a blocking error, so it will not {}: {}{}",
            verb.name(),
            describe(a),
            if matches!(verb, Verb::Unlock) {
                format!(
                    " — if the lock is left over from that promotion, `aic promotion unlock \
                     --tenant {tenant} --recover <promotionId>` releases it"
                )
            } else {
                String::new()
            }
        ),
        Phase::Running { message } => format!(
            "a promotion is running on {tenant} ({}); wait for it before the {}",
            message.as_deref().unwrap_or("no message"),
            verb.name()
        ),
        _ => format!(
            "{tenant} is not in a state to start a {} from: {}",
            verb.name(),
            describe(a)
        ),
    }
}

/// What a check approved: the `promotionId` a write must still find. `None`
/// for a lock, which starts from no lock at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub promotion_id: Option<String>,
}

/// Whether `verb` may start from `a`.
///
/// - `Lock` — from [`Phase::Unlocked`] only. A second lock over a held one is
///   unmeasured, and the held one may be someone else's.
/// - `Promote` — from [`Phase::Ready`] with an id.
/// - `Unlock` — from [`Phase::Ready`] with an id; never while a promotion
///   runs. The id is the tenant's, never one typed.
/// - `Recover` — from [`Phase::Blocked`] with the status `READY` — the one
///   status measured as settled — over a lock every lock source agrees is
///   held, naming exactly the id the operator expects. No failed promotion
///   has been measured, and a blocking error does not say work has stopped,
///   so every other status (a transition, `RUNNING`, an unknown name) stays a
///   manual (console) recovery rather than be guessed safe.
pub fn check(verb: &Verb, a: &Assessment, tenant: &str) -> Result<Approval, String> {
    pair_ok(a, tenant)?;
    let phase = phase(a);
    let id_of = |id: &Option<String>| {
        id.clone().ok_or_else(|| {
            format!("{tenant} is locked but its promotion status names no promotionId")
        })
    };
    match (verb, &phase) {
        (Verb::Lock, Phase::Unlocked) => Ok(Approval { promotion_id: None }),
        (Verb::Lock, Phase::Ready { promotion_id }) => Err(format!(
            "{tenant} is already locked (promotionId {}); a second lock is unmeasured — \
             `aic promotion unlock --tenant {tenant}` releases the held one",
            promotion_id.as_deref().unwrap_or("not reported")
        )),
        (Verb::Promote { .. } | Verb::Unlock, Phase::Ready { promotion_id }) => Ok(Approval {
            promotion_id: Some(id_of(promotion_id)?),
        }),
        (Verb::Promote { .. }, Phase::Unlocked) => Err(format!(
            "{tenant} is not locked; `aic promotion lock --tenant {tenant}` first"
        )),
        (Verb::Recover { expected }, Phase::Blocked) => {
            let found = id_of(&a.status.promotion_id)?;
            if !held(a) || !is(&a.status.status, "READY") {
                Err(format!(
                    "{tenant} does not read as a held, stopped lock, so it will not be released \
                     blind: {}",
                    describe(a)
                ))
            } else if found != *expected {
                Err(format!(
                    "{tenant}'s lock is promotionId {found}, not {expected}; nothing was released"
                ))
            } else {
                Ok(Approval {
                    promotion_id: Some(found),
                })
            }
        }
        (Verb::Recover { .. }, Phase::Ready { .. }) => Err(format!(
            "{tenant} reports no blocking error; release it with `aic promotion unlock --tenant \
             {tenant}`"
        )),
        (Verb::Recover { .. }, _) => Err(format!(
            "{tenant} is not in a state a recovery unlock is measured to be safe from — release \
             it from the console if it is stuck: {}",
            describe(a)
        )),
        (_, other) => Err(refused(tenant, a, verb, other)),
    }
}

/// Proof that a write was checked against a fresh read and consented to.
/// The operation is a private field and [`authorize`] is the only
/// constructor, so `api::send` cannot be reached without both — and a permit
/// for one operation cannot be relabelled as another.
#[derive(Debug)]
pub struct Permit {
    op: Op,
}

/// The one write a [`Permit`] authorizes. Readable, never constructible
/// into a permit outside this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Lock,
    Promote { dry_run: bool },
    Unlock { promotion_id: String },
}

impl Permit {
    pub fn op(&self) -> &Op {
        &self.op
    }
}

/// Mint a permit from a **fresh** reading taken after consent: the verb must
/// still be eligible, and the `promotionId` must be the one approved. A lock
/// replaced while the prompt was open is someone else's.
pub fn authorize(
    verb: &Verb,
    approved: &Approval,
    fresh: &Assessment,
    tenant: &str,
    consented: bool,
) -> Result<Permit, String> {
    if !consented {
        return Err(match verb {
            Verb::Lock => format!(
                "would lock {tenant} and the environment it promotes from — neither accepts \
                 configuration changes until it is unlocked; pass --force to lock"
            ),
            Verb::Promote { dry_run: true } => format!(
                "would start a dry-run promotion into {tenant} (measured: ~8 minutes); pass \
                 --force to start it"
            ),
            Verb::Promote { dry_run: false } => format!(
                "would promote into {tenant} from the environment it is paired with; pass --force \
                 to promote"
            ),
            Verb::Unlock | Verb::Recover { .. } => format!(
                "would unlock {tenant} (promotionId {}); pass --force to unlock",
                approved.promotion_id.as_deref().unwrap_or("-")
            ),
        });
    }
    let now = check(verb, fresh, tenant)
        .map_err(|why| format!("{tenant} changed after it was checked: {why}"))?;
    if now != *approved {
        return Err(format!(
            "{tenant}'s lock changed after it was checked (promotionId {} then, {} now); \
             nothing was sent",
            approved.promotion_id.as_deref().unwrap_or("none"),
            now.promotion_id.as_deref().unwrap_or("none")
        ));
    }
    let op = match verb {
        Verb::Lock => Op::Lock,
        Verb::Promote { dry_run } => Op::Promote { dry_run: *dry_run },
        Verb::Unlock | Verb::Recover { .. } => Op::Unlock {
            promotion_id: now
                .promotion_id
                .expect("check returns an id for every unlock"),
        },
    };
    Ok(Permit { op })
}

/// What [`begin`] did.
#[derive(Debug)]
pub enum Begun<T> {
    /// Nothing to do: an unlock on a pair already unlocked.
    AlreadyUnlocked,
    /// Sent. `before` is the reading the write was authorized against.
    Sent { before: Box<Assessment>, ack: T },
}

/// The write sequence, with every tenant interaction injected so a test can
/// drive it: read and check; ask for consent; `prepare` (the production gate
/// and anything to capture before the write — it runs only once a write is
/// going ahead, so a refused state never demands `--yes`); read **again**
/// and mint the permit from that reading; send.
///
/// A send that fails says the request may have landed: the tenant answers
/// `202` and works asynchronously, so a lost response is not a refusal.
pub async fn begin<P, T>(
    verb: &Verb,
    tenant: &str,
    mut read: impl AsyncFnMut() -> crate::Result<Assessment>,
    consent: impl FnOnce(&Assessment) -> crate::Result<bool>,
    prepare: impl AsyncFnOnce() -> crate::Result<P>,
    send: impl AsyncFnOnce(Permit, P) -> crate::Result<T>,
) -> crate::Result<Begun<T>> {
    let first = read().await?;
    if matches!(verb, Verb::Unlock) && phase(&first) == Phase::Unlocked {
        pair_ok(&first, tenant).map_err(crate::Error::Config)?;
        return Ok(Begun::AlreadyUnlocked);
    }
    let approved = check(verb, &first, tenant).map_err(crate::Error::Config)?;
    let consented = consent(&first)?;
    if !consented {
        // Same refusal `authorize` words, without a second read.
        return Err(crate::Error::Config(
            authorize(verb, &approved, &first, tenant, false).unwrap_err(),
        ));
    }
    let prepared = prepare().await?;
    let fresh = read().await?;
    let permit = authorize(verb, &approved, &fresh, tenant, true).map_err(crate::Error::Config)?;
    match send(permit, prepared).await {
        Ok(ack) => Ok(Begun::Sent {
            before: Box::new(fresh),
            ack,
        }),
        Err(error) => Err(crate::Error::Config(format!(
            "{error}\n  the {} request may still have reached {tenant} — run `aic promotion \
             status --tenant {tenant}` before retrying",
            verb.name()
        ))),
    }
}

/// One poll's verdict on a started operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settle {
    Pending(String),
    Done,
    Failed(String),
}

/// What a started operation is judged against while it settles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    verb: Verb,
    /// The lock this operation belongs to: the approved one, or for a lock
    /// the id its acknowledgement carried. Required: without it nothing can
    /// tie a reading to this request, so the caller does not wait at all.
    promotion_id: String,
    seen_running: bool,
}

impl Watch {
    pub fn new(verb: Verb, promotion_id: String) -> Self {
        Self {
            verb,
            promotion_id,
            seen_running: false,
        }
    }

    /// Completion is judged from status alone. A report is not evidence: the
    /// status names a "Create report" step before later ones, when a report
    /// becomes readable is unmeasured, and one under the same id may be
    /// another run's — so reports are named after completion, as candidates.
    pub fn judge(&mut self, a: &Assessment) -> Settle {
        let phase = phase(a);
        // A state *about* a lock counts only under this operation's id: a
        // foreign one fails, a missing one proves nothing either way. What
        // the status carries mid-transition, or once unlocked, is unmeasured
        // and does not decide completion, so it is not judged.
        let about_a_lock = matches!(
            phase,
            Phase::Ready { .. } | Phase::Running { .. } | Phase::Blocked
        );
        if about_a_lock {
            match a.status.promotion_id.as_deref() {
                Some(now) if now == self.promotion_id => {}
                Some(other) => {
                    return Settle::Failed(format!(
                        "the pair is now under promotionId {other}, not {} — another operation \
                         took over",
                        self.promotion_id
                    ));
                }
                None => {
                    return Settle::Pending(format!(
                        "the status names no promotionId, so it is not counted: {}",
                        describe(a)
                    ));
                }
            }
        }
        match (&self.verb, phase) {
            // A recovery starts from the blocked state it is releasing.
            (Verb::Recover { .. }, Phase::Blocked) => Settle::Pending(describe(a)),
            (_, Phase::Blocked) => Settle::Failed(describe(a)),
            (Verb::Lock, Phase::Ready { .. }) => Settle::Done,
            (Verb::Lock, Phase::Running { .. }) => {
                Settle::Failed(format!("a promotion started: {}", describe(a)))
            }
            (Verb::Unlock | Verb::Recover { .. }, Phase::Unlocked) => Settle::Done,
            (Verb::Unlock | Verb::Recover { .. }, Phase::Running { .. }) => {
                Settle::Failed(describe(a))
            }
            (Verb::Promote { .. }, Phase::Running { message }) => {
                self.seen_running = true;
                Settle::Pending(message.unwrap_or_else(|| "running".into()))
            }
            // `READY` is also where a promotion starts, so it ends one only
            // after this run was seen `RUNNING`. A run too quick to be seen
            // stays unconfirmed until the timeout says so.
            (Verb::Promote { .. }, Phase::Ready { .. }) => {
                if self.seen_running {
                    Settle::Done
                } else {
                    Settle::Pending("not seen to start yet".into())
                }
            }
            (Verb::Promote { .. }, _) if self.seen_running => Settle::Failed(describe(a)),
            (_, _) => Settle::Pending(describe(a)),
        }
    }
}

/// The report ids a promotion produced: entries of `after` absent from
/// `before`, for this `promotionId` and this kind of run. A dry run and a
/// real promotion inside one lock share the `promotionId` (one lock, one id),
/// so the id alone does not say which run a report belongs to — and these
/// are candidates, not proof, if two operators run inside one lock.
pub fn new_reports(
    before: &Value,
    after: &Value,
    promotion_id: &str,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let ids = |list: &Value| -> Result<Vec<(String, Value)>, String> {
        list.as_array()
            .ok_or_else(|| format!("reports is not an array: {list}"))?
            .iter()
            .map(|entry| {
                entry
                    .get("reportId")
                    .and_then(Value::as_str)
                    .map(|id| (id.to_string(), entry.clone()))
                    .ok_or_else(|| format!("a report entry has no string `reportId`: {entry}"))
            })
            .collect()
    };
    let known: Vec<String> = ids(before)?.into_iter().map(|(id, _)| id).collect();
    Ok(ids(after)?
        .into_iter()
        .filter(|(id, entry)| {
            !known.contains(id)
                && entry.get("promotionId").and_then(Value::as_str) == Some(promotion_id)
                && entry.get("dryRun").and_then(Value::as_bool) == Some(dry_run)
        })
        .map(|(id, _)| id)
        .collect())
}

/// Which report a finished promotion produced, from [`new_reports`] read
/// after completion. None is an error: what the run did is then unconfirmed,
/// and a zero exit would read as success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identified {
    One(String),
    /// More than one new report of this kind under this id — another run
    /// shared the lock, so which is this one's is not known.
    Candidates(Vec<String>),
}

pub fn identify(
    produced: &[String],
    verb: &Verb,
    promotion_id: Option<&str>,
) -> Result<Identified, String> {
    match produced {
        [] => Err(format!(
            "no new report for promotionId {} was found, so what the {} did is unconfirmed",
            promotion_id.unwrap_or("(none)"),
            verb.name()
        )),
        [one] => Ok(Identified::One(one.clone())),
        many => Ok(Identified::Candidates(many.to_vec())),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::promotion::spec::assess;

    const ID: &str = "b5dfc43b-9da9-4c27-9ed8-47722bcab7f4";
    const OTHER: &str = "0e0e0e0e-0000-4000-8000-000000000000";

    fn lock(result: &str, lower: &str, upper: &str) -> Value {
        json!({
            "result": result,
            "lowerEnv": {"state": lower, "proxyState": lower},
            "upperEnv": {"state": upper, "proxyState": upper},
        })
    }

    /// The observations measured 2026-10-27, as the tenant sent them.
    fn unlocked() -> Assessment {
        assess(
            &lock("unlocked", "unlocked", "unlocked"),
            &json!({"status": "UNLOCKED", "message": "Environment unlocked",
                    "globalLock": "UNLOCKED", "blockingError": false}),
        )
        .unwrap()
    }

    fn ready_as(id: &str) -> Assessment {
        assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "message": "Environment ready for promotion",
                    "globalLock": "LOCKED", "blockingError": false,
                    "promotionId": id, "timeStamp": "t", "type": "promotion"}),
        )
        .unwrap()
    }

    fn ready() -> Assessment {
        ready_as(ID)
    }

    /// Only `GET promote` was polled during the measured run; the lock fields
    /// here are an assumption, which is why `Running` does not depend on them.
    fn running() -> Assessment {
        assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "RUNNING", "message": "Clone repos", "globalLock": "LOCKED",
                    "blockingError": false, "promotionId": ID}),
        )
        .unwrap()
    }

    fn blocked(status: &str, id: &str) -> Assessment {
        assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": status, "globalLock": "LOCKED", "blockingError": true,
                    "promotionId": id}),
        )
        .unwrap()
    }

    fn locking_upper_first() -> Assessment {
        assess(
            &lock("locking", "locking", "locked"),
            &json!({"status": "LOCKING", "globalLock": "LOCKING", "blockingError": false}),
        )
        .unwrap()
    }

    #[test]
    fn measured_observations_map_to_their_phases() {
        assert_eq!(phase(&unlocked()), Phase::Unlocked);
        assert_eq!(
            phase(&ready()),
            Phase::Ready {
                promotion_id: Some(ID.into())
            }
        );
        assert!(matches!(phase(&running()), Phase::Running { .. }));
        assert!(matches!(phase(&locking_upper_first()), Phase::Unsettled(_)));
    }

    #[test]
    fn a_write_phase_needs_every_source_to_agree() {
        // READY with one side still locking.
        let half = assess(
            &lock("locked", "locking", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "blockingError": false,
                    "promotionId": ID}),
        )
        .unwrap();
        assert!(matches!(phase(&half), Phase::Unsettled(_)));
        // UNLOCKED status over a lock/state still unlocking.
        let lagging = assess(
            &lock("unlocking", "unlocking", "unlocking"),
            &json!({"status": "UNLOCKED", "globalLock": "UNLOCKED", "blockingError": false}),
        )
        .unwrap();
        assert!(matches!(phase(&lagging), Phase::Unsettled(_)));
        // A missing globalLock is not an agreeing one.
        let silent = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "blockingError": false, "promotionId": ID}),
        )
        .unwrap();
        assert!(matches!(phase(&silent), Phase::Unsettled(_)));
        // A missing blockingError is not a reported `false`.
        let unsure = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "promotionId": ID}),
        )
        .unwrap();
        assert!(matches!(phase(&unsure), Phase::Unsettled(_)));
    }

    /// Both reviewers' discriminating input: every state agrees, one proxy
    /// does not. `spec::disagreements` flags it, so no verb may start.
    #[test]
    fn a_proxy_that_disagrees_is_not_a_write_state() {
        for proxy in ["unlocked", "locking"] {
            let a = assess(
                &json!({"result": "locked",
                        "lowerEnv": {"state": "locked", "proxyState": proxy},
                        "upperEnv": {"state": "locked", "proxyState": "locked"}}),
                &json!({"status": "READY", "globalLock": "LOCKED", "blockingError": false,
                        "promotionId": ID}),
            )
            .unwrap();
            assert!(!a.disagreements.is_empty());
            assert!(matches!(phase(&a), Phase::Unsettled(_)), "{proxy}");
            assert!(check(&Verb::Promote { dry_run: true }, &a, "uat").is_err());
            assert!(check(&Verb::Unlock, &a, "uat").is_err());
            let mut watch = Watch::new(Verb::Lock, ID.into());
            assert!(matches!(watch.judge(&a), Settle::Pending(_)));
        }
        // A missing proxy is not an agreeing one either.
        let a = assess(
            &json!({"result": "unlocked",
                    "lowerEnv": {"state": "unlocked"},
                    "upperEnv": {"state": "unlocked", "proxyState": "unlocked"}}),
            &json!({"status": "UNLOCKED", "globalLock": "UNLOCKED", "blockingError": false}),
        )
        .unwrap();
        assert!(check(&Verb::Lock, &a, "uat").is_err());
    }

    #[test]
    fn verbs_refuse_on_the_lower_environment() {
        // The lower env answers lock/state with `result` alone (measured).
        let lower = assess(
            &json!({"result": "unlocked", "description": "Environment unlocked"}),
            &json!({"status": "UNLOCKED", "globalLock": "UNLOCKED", "blockingError": false}),
        )
        .unwrap();
        for verb in [
            Verb::Lock,
            Verb::Promote { dry_run: true },
            Verb::Unlock,
            Verb::Recover {
                expected: ID.into(),
            },
        ] {
            let err = check(&verb, &lower, "dev").unwrap_err();
            assert!(err.contains("not the upper environment"), "{verb:?}: {err}");
        }
    }

    #[test]
    fn each_verb_starts_only_from_its_phase() {
        assert!(check(&Verb::Lock, &unlocked(), "uat").is_ok());
        assert!(
            check(&Verb::Lock, &ready(), "uat")
                .unwrap_err()
                .contains(ID)
        );
        assert!(check(&Verb::Lock, &running(), "uat").is_err());
        assert!(check(&Verb::Lock, &locking_upper_first(), "uat").is_err());

        let promote = Verb::Promote { dry_run: true };
        assert_eq!(
            check(&promote, &ready(), "uat")
                .unwrap()
                .promotion_id
                .as_deref(),
            Some(ID)
        );
        assert!(
            check(&promote, &unlocked(), "uat")
                .unwrap_err()
                .contains("not locked")
        );
        assert!(
            check(&promote, &running(), "uat")
                .unwrap_err()
                .contains("running")
        );

        assert!(check(&Verb::Unlock, &ready(), "uat").is_ok());
        assert!(
            check(&Verb::Unlock, &running(), "uat")
                .unwrap_err()
                .contains("running")
        );
        let anonymous = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "blockingError": false}),
        )
        .unwrap();
        assert!(
            check(&Verb::Unlock, &anonymous, "uat")
                .unwrap_err()
                .contains("no promotionId")
        );
    }

    /// The escape for a lock a failed promotion leaves: blocked, held by
    /// every source, not running, and exactly the id the operator expects.
    #[test]
    fn recovery_releases_only_the_expected_blocked_lock() {
        let recover = |id: &str| Verb::Recover {
            expected: id.into(),
        };
        let unlock = check(&Verb::Unlock, &blocked("READY", ID), "uat").unwrap_err();
        assert!(unlock.contains("--recover"), "{unlock}");
        assert_eq!(
            check(&recover(ID), &blocked("READY", ID), "uat")
                .unwrap()
                .promotion_id
                .as_deref(),
            Some(ID)
        );
        // A blocking error does not say work stopped: only READY, the one
        // status measured as settled, is released — not a transition, not an
        // unknown name.
        for status in ["ERROR", "LOCKING", "QUEUED", "RUNNING"] {
            assert!(
                check(&recover(ID), &blocked(status, ID), "uat").is_err(),
                "{status}"
            );
        }
        assert!(
            check(&recover(OTHER), &blocked("READY", ID), "uat")
                .unwrap_err()
                .contains(OTHER)
        );
        // Running, unblocked, or not held: never released blind.
        assert!(check(&recover(ID), &blocked("RUNNING", ID), "uat").is_err());
        assert!(
            check(&recover(ID), &ready(), "uat")
                .unwrap_err()
                .contains("no blocking error")
        );
        assert!(check(&recover(ID), &running(), "uat").is_err());
        let half = assess(
            &lock("locked", "unlocking", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "blockingError": true,
                    "promotionId": ID}),
        )
        .unwrap();
        assert!(check(&recover(ID), &half, "uat").is_err());
    }

    #[test]
    fn authorize_needs_consent_and_the_approved_lock() {
        let approved = check(&Verb::Unlock, &ready(), "uat").unwrap();
        assert!(
            authorize(&Verb::Unlock, &approved, &ready(), "uat", false)
                .unwrap_err()
                .contains("--force")
        );
        let permit = authorize(&Verb::Unlock, &approved, &ready(), "uat", true).unwrap();
        assert_eq!(
            permit.op(),
            &Op::Unlock {
                promotion_id: ID.into()
            }
        );
        // Replaced while the prompt was open.
        let err = authorize(&Verb::Unlock, &approved, &ready_as(OTHER), "uat", true).unwrap_err();
        assert!(err.contains(ID) && err.contains(OTHER), "{err}");
        // Started running while the prompt was open.
        assert!(authorize(&Verb::Unlock, &approved, &running(), "uat", true).is_err());
        // The promotion kind is fixed at minting.
        let approved = check(&Verb::Promote { dry_run: false }, &ready(), "uat").unwrap();
        let permit = authorize(
            &Verb::Promote { dry_run: false },
            &approved,
            &ready(),
            "uat",
            true,
        )
        .unwrap();
        assert_eq!(permit.op(), &Op::Promote { dry_run: false });
    }

    /// Drive [`begin`] with a scripted tenant: what each read returns, what
    /// consent says, and whether `send` was reached.
    async fn drive(
        verb: Verb,
        reads: Vec<Assessment>,
        consent: bool,
    ) -> (crate::Result<Begun<()>>, Option<Permit>, Vec<&'static str>) {
        let reads = RefCell::new(reads.into_iter());
        let events = RefCell::new(Vec::new());
        let sent = RefCell::new(None);
        let result = begin(
            &verb,
            "uat",
            async || {
                events.borrow_mut().push("read");
                Ok(reads.borrow_mut().next().expect("no more scripted reads"))
            },
            |_| {
                events.borrow_mut().push("consent");
                Ok(consent)
            },
            async || {
                events.borrow_mut().push("prepare");
                Ok(())
            },
            async |permit, ()| {
                events.borrow_mut().push("send");
                *sent.borrow_mut() = Some(permit);
                Ok(())
            },
        )
        .await;
        (result, sent.into_inner(), events.into_inner())
    }

    #[tokio::test]
    async fn begin_sends_from_a_fresh_read_after_consent() {
        let (result, sent, events) = drive(
            Verb::Promote { dry_run: true },
            vec![ready(), ready()],
            true,
        )
        .await;
        assert!(matches!(result, Ok(Begun::Sent { .. })));
        assert_eq!(sent.unwrap().op(), &Op::Promote { dry_run: true });
        assert_eq!(
            events,
            ["read", "consent", "prepare", "read", "send"],
            "the permit must come from a read taken after consent and the prod gate"
        );
    }

    /// The race both reviewers named: a promotion starts, or the lock is
    /// replaced, while the prompt is open. Nothing may be sent.
    #[tokio::test]
    async fn begin_refuses_what_changed_while_the_prompt_was_open() {
        for (verb, second) in [
            (Verb::Unlock, running()),
            (Verb::Unlock, ready_as(OTHER)),
            (Verb::Promote { dry_run: false }, unlocked()),
            (Verb::Promote { dry_run: true }, ready_as(OTHER)),
            (Verb::Lock, ready()),
        ] {
            let first = if verb == Verb::Lock {
                unlocked()
            } else {
                ready()
            };
            let (result, sent, _) = drive(verb.clone(), vec![first, second], true).await;
            assert!(result.is_err(), "{verb:?}");
            assert!(sent.is_none(), "{verb:?} sent after drift");
        }
    }

    #[tokio::test]
    async fn begin_without_consent_or_eligibility_neither_prepares_nor_sends() {
        let (result, sent, events) = drive(Verb::Lock, vec![unlocked()], false).await;
        assert!(result.unwrap_err().to_string().contains("--force"));
        assert!(sent.is_none());
        assert_eq!(events, ["read", "consent"]);
        // A refused state never reaches consent or the production gate.
        let (result, sent, events) = drive(Verb::Lock, vec![ready()], true).await;
        assert!(result.is_err() && sent.is_none());
        assert_eq!(events, ["read"]);
        // An unlock of an unlocked pair is nothing to do.
        let (result, sent, _) = drive(Verb::Unlock, vec![unlocked()], true).await;
        assert!(matches!(result, Ok(Begun::AlreadyUnlocked)) && sent.is_none());
    }

    #[tokio::test]
    async fn a_failed_send_says_the_request_may_have_landed() {
        let result: crate::Result<Begun<()>> = begin(
            &Verb::Lock,
            "uat",
            async || Ok(unlocked()),
            |_| Ok(true),
            async || Ok(()),
            async |_, ()| Err(crate::Error::Config("connection reset".into())),
        )
        .await;
        let message = result.unwrap_err().to_string();
        assert!(message.contains("may still have reached"), "{message}");
    }

    #[test]
    fn lock_and_unlock_settle_only_at_their_end_state_under_their_id() {
        let mut lock = Watch::new(Verb::Lock, ID.into());
        assert!(matches!(lock.judge(&unlocked()), Settle::Pending(_)));
        assert!(matches!(
            lock.judge(&locking_upper_first()),
            Settle::Pending(_)
        ));
        assert_eq!(lock.judge(&ready()), Settle::Done);
        assert!(matches!(lock.judge(&ready_as(OTHER)), Settle::Failed(_)));
        assert!(matches!(lock.judge(&running()), Settle::Failed(_)));

        let mut unlock = Watch::new(Verb::Unlock, ID.into());
        assert!(matches!(unlock.judge(&ready()), Settle::Pending(_)));
        assert_eq!(unlock.judge(&unlocked()), Settle::Done);
        assert!(matches!(unlock.judge(&running()), Settle::Failed(_)));
        assert!(matches!(
            unlock.judge(&blocked("READY", ID)),
            Settle::Failed(_)
        ));

        // A recovery waits through the blocked state it started from.
        let recover = Verb::Recover {
            expected: ID.into(),
        };
        let mut watch = Watch::new(recover, ID.into());
        assert!(matches!(
            watch.judge(&blocked("READY", ID)),
            Settle::Pending(_)
        ));
        assert!(matches!(
            watch.judge(&blocked("READY", OTHER)),
            Settle::Failed(_)
        ));

        // A transition carrying an old id is not a foreign lock.
        let mut lock = Watch::new(Verb::Lock, ID.into());
        let mut stale = locking_upper_first();
        stale.status.promotion_id = Some(OTHER.into());
        assert!(matches!(lock.judge(&stale), Settle::Pending(_)));
    }

    /// Both reviewers' discriminating inputs: a READY or RUNNING with no
    /// `promotionId` proves nothing about this operation, either way.
    #[test]
    fn a_reading_without_a_promotion_id_is_not_counted() {
        let anonymous = |mut a: Assessment| {
            a.status.promotion_id = None;
            a
        };
        let mut lock = Watch::new(Verb::Lock, ID.into());
        assert!(matches!(
            lock.judge(&anonymous(ready())),
            Settle::Pending(_)
        ));
        let mut promote = Watch::new(Verb::Promote { dry_run: false }, ID.into());
        assert!(matches!(
            promote.judge(&anonymous(running())),
            Settle::Pending(_)
        ));
        // …and it did not count as this run being seen to start.
        assert!(matches!(promote.judge(&ready()), Settle::Pending(_)));
        assert!(matches!(promote.judge(&running()), Settle::Pending(_)));
        assert!(matches!(
            promote.judge(&anonymous(ready())),
            Settle::Pending(_)
        ));
        assert_eq!(promote.judge(&ready()), Settle::Done);
    }

    /// The discriminating case: `READY` is also where a promotion starts, so
    /// without this run seen RUNNING it is not the end — whatever the
    /// timestamp did, and whatever reports appeared.
    #[test]
    fn a_promotion_is_not_finished_by_the_ready_it_started_from() {
        let mut watch = Watch::new(Verb::Promote { dry_run: true }, ID.into());
        let mut moved = ready();
        moved.status.timestamp = Some("t-later".into());
        assert!(matches!(watch.judge(&moved), Settle::Pending(_)));
        assert!(matches!(watch.judge(&running()), Settle::Pending(_)));
        assert_eq!(watch.judge(&ready()), Settle::Done);

        // A run too quick to be seen RUNNING stays unconfirmed: READY is
        // where it started, and no report is evidence that it ended.
        let mut quick = Watch::new(Verb::Promote { dry_run: true }, ID.into());
        for _ in 0..3 {
            assert!(matches!(quick.judge(&ready()), Settle::Pending(_)));
        }
    }

    #[test]
    fn a_promotion_under_another_lock_or_state_is_not_this_one() {
        let mut watch = Watch::new(Verb::Promote { dry_run: false }, ID.into());
        assert!(matches!(watch.judge(&running()), Settle::Pending(_)));
        let Settle::Failed(why) = watch.judge(&ready_as(OTHER)) else {
            panic!("a READY under another promotionId must not finish this run");
        };
        assert!(why.contains(OTHER), "{why}");

        let mut watch = Watch::new(Verb::Promote { dry_run: false }, ID.into());
        watch.judge(&running());
        assert!(matches!(watch.judge(&unlocked()), Settle::Failed(_)));
        let odd = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "ERROR", "globalLock": "LOCKED", "blockingError": false,
                    "message": "boom", "promotionId": ID}),
        )
        .unwrap();
        let Settle::Failed(why) = watch.judge(&odd) else {
            panic!("an unmeasured status after starting must fail");
        };
        assert!(why.contains("ERROR") && why.contains("boom"), "{why}");
        // …but before it has started, the same reading is only pending.
        let mut fresh = Watch::new(Verb::Promote { dry_run: false }, ID.into());
        assert!(matches!(fresh.judge(&odd), Settle::Pending(_)));
    }

    #[test]
    fn a_finished_promotion_without_its_report_is_unconfirmed() {
        let verb = Verb::Promote { dry_run: false };
        let err = identify(&[], &verb, Some(ID)).unwrap_err();
        assert!(err.contains(ID) && err.contains("unconfirmed"), "{err}");
        assert_eq!(
            identify(&["r".into()], &verb, Some(ID)),
            Ok(Identified::One("r".into()))
        );
        assert_eq!(
            identify(&["r".into(), "s".into()], &verb, Some(ID)),
            Ok(Identified::Candidates(vec!["r".into(), "s".into()]))
        );
    }

    #[test]
    fn new_reports_are_new_and_of_this_run_kind() {
        let old = json!({"reportId": "r-old", "promotionId": ID, "dryRun": true});
        let before = json!([old]);
        let after = json!([
            {"reportId": "r-dry", "promotionId": ID, "dryRun": true},
            {"reportId": "r-real", "promotionId": ID, "dryRun": false},
            {"reportId": "r-other", "promotionId": "someone-else", "dryRun": true},
            old,
        ]);
        assert_eq!(
            new_reports(&before, &after, ID, true).unwrap(),
            vec!["r-dry"]
        );
        assert_eq!(
            new_reports(&before, &after, ID, false).unwrap(),
            vec!["r-real"]
        );
        assert!(new_reports(&json!({}), &after, ID, true).is_err());
        assert!(new_reports(&before, &json!([{"promotionId": ID}]), ID, true).is_err());
    }
}
