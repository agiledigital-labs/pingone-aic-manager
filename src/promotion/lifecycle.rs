//! Tenant-free half of the promotion write verbs: which state a lock, a
//! promotion or an unlock may start from, the permits those checks mint, and
//! when a started operation has settled.
//!
//! Measured 2026-10-27 on a dev → UAT pair, every call sent to the upper
//! environment (`docs/api/23-promotion.md`, "The write cycle"). Each phase
//! below is a combination of fields that was **observed**; anything else is
//! [`Phase::Unsettled`], which no verb starts from.

use serde_json::Value;

use super::spec::{Assessment, LockState};

fn is(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Where the pair stands, read from both sources at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// `lock/state` and both sides unlocked; the promotion status `UNLOCKED`.
    Unlocked,
    /// `lock/state` and both sides locked; the promotion status `READY` with
    /// `globalLock: LOCKED`. Measured both after a lock settled and after a
    /// dry run finished. Carries the id an unlock releases.
    Ready { promotion_id: Option<String> },
    /// The promotion status says `RUNNING`.
    Running { message: Option<String> },
    /// The status reports `blockingError: true`.
    Blocked,
    /// Any other combination: a transition (`locking`, `unlocking`, one side
    /// ahead of the other), or a value never measured. Carries a description.
    Unsettled(String),
}

fn sides(lock: &LockState) -> [Option<&str>; 2] {
    [
        lock.lower.as_ref().map(|s| s.state.as_str()),
        lock.upper.as_ref().map(|s| s.state.as_str()),
    ]
}

/// Whether `lock/state` and both sides read `want`. A side the response
/// omits fails — see [`pair_ok`] for why that is refused up front.
fn lock_reads(lock: &LockState, want: &str) -> bool {
    is(&lock.result, want) && sides(lock).iter().all(|s| s.is_some_and(|s| is(s, want)))
}

fn describe(a: &Assessment) -> String {
    let [lower, upper] = sides(&a.lock);
    format!(
        "lock/state `{}` (lower `{}`, upper `{}`), promotion status `{}` (globalLock `{}`){}",
        a.lock.result,
        lower.unwrap_or("-"),
        upper.unwrap_or("-"),
        a.status.status,
        a.status.global_lock.as_deref().unwrap_or("-"),
        a.status
            .message
            .as_ref()
            .map(|m| format!(": {m}"))
            .unwrap_or_default()
    )
}

pub fn phase(a: &Assessment) -> Phase {
    let status = &a.status;
    let global = |want: &str| status.global_lock.as_deref().is_some_and(|g| is(g, want));
    if a.blocking() {
        Phase::Blocked
    } else if is(&status.status, "RUNNING") {
        Phase::Running {
            message: status.message.clone(),
        }
    } else if is(&status.status, "UNLOCKED")
        && global("UNLOCKED")
        && lock_reads(&a.lock, "unlocked")
    {
        Phase::Unlocked
    } else if is(&status.status, "READY") && global("LOCKED") && lock_reads(&a.lock, "locked") {
        Phase::Ready {
            promotion_id: status.promotion_id.clone(),
        }
    } else {
        Phase::Unsettled(describe(a))
    }
}

/// Every write verb is sent to the **upper** environment of a pair: only it
/// answers `lock/state` with both sides (measured), and starting a lock from
/// the lower one was never tried.
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

fn refused(tenant: &str, a: &Assessment, verb: &str, phase: &Phase) -> String {
    match phase {
        Phase::Blocked => format!(
            "{tenant}'s promotion status reports a blocking error, so it will not {verb}: {}",
            describe(a)
        ),
        Phase::Running { message } => format!(
            "a promotion is running on {tenant} ({}); wait for it before you {verb}",
            message.as_deref().unwrap_or("no message")
        ),
        _ => format!("{tenant} is not in a state to {verb} from: {}", describe(a)),
    }
}

/// Proof that a lock was checked and consented to. The field is private and
/// [`authorize_lock`] is the only constructor, so `api::lock` cannot be
/// reached without both.
#[derive(Debug)]
pub struct LockPermit {
    _minted_by_authorize_lock: (),
}

/// Proof for a dry run or a promotion; which one is fixed at minting.
#[derive(Debug)]
pub struct PromotePermit {
    dry_run: bool,
}

impl PromotePermit {
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
}

/// Proof for an unlock, carrying the id read from the tenant — never one the
/// operator typed: releasing another promotion's lock is unmeasured.
#[derive(Debug)]
pub struct UnlockPermit {
    promotion_id: String,
}

impl UnlockPermit {
    pub fn promotion_id(&self) -> &str {
        &self.promotion_id
    }
}

/// Whether a lock may start: from [`Phase::Unlocked`] only. A second lock
/// over a held one is unmeasured, and the held one may be someone else's.
pub fn check_lock(a: &Assessment, tenant: &str) -> Result<(), String> {
    pair_ok(a, tenant)?;
    match phase(a) {
        Phase::Unlocked => Ok(()),
        Phase::Ready { promotion_id } => Err(format!(
            "{tenant} is already locked (promotionId {}); a second lock is unmeasured — \
             `aic promotion unlock --tenant {tenant}` releases the held one",
            promotion_id.as_deref().unwrap_or("not reported")
        )),
        other => Err(refused(tenant, a, "lock", &other)),
    }
}

pub fn authorize_lock(a: &Assessment, tenant: &str, consented: bool) -> Result<LockPermit, String> {
    check_lock(a, tenant)?;
    if !consented {
        return Err(format!(
            "would lock {tenant} and the environment it promotes from — neither accepts \
             configuration changes until it is unlocked; pass --force to lock"
        ));
    }
    Ok(LockPermit {
        _minted_by_authorize_lock: (),
    })
}

/// Whether a dry run or a promotion may start: from [`Phase::Ready`] only —
/// locked, and nothing running.
pub fn check_promote(a: &Assessment, tenant: &str) -> Result<(), String> {
    pair_ok(a, tenant)?;
    match phase(a) {
        Phase::Ready { .. } => Ok(()),
        Phase::Unlocked => Err(format!(
            "{tenant} is not locked; `aic promotion lock --tenant {tenant}` first"
        )),
        other => Err(refused(tenant, a, "promote", &other)),
    }
}

pub fn authorize_promote(
    a: &Assessment,
    tenant: &str,
    dry_run: bool,
    consented: bool,
) -> Result<PromotePermit, String> {
    check_promote(a, tenant)?;
    if !consented {
        return Err(if dry_run {
            format!(
                "would start a dry-run promotion into {tenant} (measured: ~8 minutes, no \
                 configuration change); pass --force to start it"
            )
        } else {
            format!(
                "would promote into {tenant}: every configuration difference from the lower \
                 environment becomes live there; pass --force to promote"
            )
        });
    }
    Ok(PromotePermit { dry_run })
}

/// Whether an unlock may start: from [`Phase::Ready`] with an id. Never while
/// a promotion runs — unlocking mid-promotion is unmeasured.
pub fn check_unlock(a: &Assessment, tenant: &str) -> Result<String, String> {
    pair_ok(a, tenant)?;
    match phase(a) {
        Phase::Ready {
            promotion_id: Some(id),
        } => Ok(id),
        Phase::Ready { promotion_id: None } => Err(format!(
            "{tenant} is locked but its promotion status names no promotionId to release"
        )),
        other => Err(refused(tenant, a, "unlock", &other)),
    }
}

pub fn authorize_unlock(
    a: &Assessment,
    tenant: &str,
    consented: bool,
) -> Result<UnlockPermit, String> {
    let promotion_id = check_unlock(a, tenant)?;
    if !consented {
        return Err(format!(
            "would unlock {tenant} (promotionId {promotion_id}); pass --force to unlock"
        ));
    }
    Ok(UnlockPermit { promotion_id })
}

/// One poll's verdict on a started operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settle {
    Pending(String),
    Done,
    Failed(String),
}

/// A lock has settled at [`Phase::Ready`]. Reading unlocked is pending: the
/// `202` arrives before either side moves.
pub fn lock_settled(a: &Assessment) -> Settle {
    match phase(a) {
        Phase::Ready { .. } => Settle::Done,
        Phase::Blocked => Settle::Failed(describe(a)),
        Phase::Running { .. } => Settle::Failed(format!("a promotion started: {}", describe(a))),
        _ => Settle::Pending(describe(a)),
    }
}

/// An unlock has settled at [`Phase::Unlocked`].
pub fn unlock_settled(a: &Assessment) -> Settle {
    match phase(a) {
        Phase::Unlocked => Settle::Done,
        Phase::Blocked | Phase::Running { .. } => Settle::Failed(describe(a)),
        _ => Settle::Pending(describe(a)),
    }
}

/// What a promotion's poll is judged against: the status before the `POST`,
/// and whether `RUNNING` has been seen since.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromoteWatch {
    pub timestamp_before: Option<String>,
    pub seen_running: bool,
}

/// A promotion has settled when the status is back at `READY` **after it
/// started** — `RUNNING` was seen, or the status timestamp moved. `READY` is
/// also the state it starts from, so a first poll that lands before the
/// service has moved must not read as finished. A status other than `READY`
/// or `RUNNING` once it has started is a failure, reported verbatim: no
/// failed run has been measured, so nothing narrower is claimed.
pub fn promote_settled(a: &Assessment, watch: &mut PromoteWatch) -> Settle {
    match phase(a) {
        Phase::Running { message } => {
            watch.seen_running = true;
            Settle::Pending(message.unwrap_or_else(|| "running".into()))
        }
        Phase::Blocked => Settle::Failed(describe(a)),
        Phase::Ready { .. } => {
            let moved =
                a.status.timestamp.is_some() && a.status.timestamp != watch.timestamp_before;
            if watch.seen_running || moved {
                Settle::Done
            } else {
                Settle::Pending("not started yet".into())
            }
        }
        _ if watch.seen_running => Settle::Failed(describe(a)),
        _ => Settle::Pending(describe(a)),
    }
}

/// The report ids a promotion produced: entries of `after` absent from
/// `before`, for this `promotionId` and this kind of run. A dry run and a
/// real promotion inside one lock share the `promotionId` (one lock, one id),
/// so the id alone does not say which run a report belongs to.
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::promotion::spec::assess;

    const ID: &str = "b5dfc43b-9da9-4c27-9ed8-47722bcab7f4";

    fn lock(result: &str, lower: &str, upper: &str) -> Value {
        json!({
            "result": result,
            "lowerEnv": {"state": lower, "proxyState": lower},
            "upperEnv": {"state": upper, "proxyState": upper},
        })
    }

    /// The four observations measured 2026-10-27, as the tenant sent them.
    fn unlocked() -> Assessment {
        assess(
            &lock("unlocked", "unlocked", "unlocked"),
            &json!({"status": "UNLOCKED", "message": "Environment unlocked",
                    "globalLock": "UNLOCKED", "blockingError": false}),
        )
        .unwrap()
    }

    fn ready(ts: &str) -> Assessment {
        assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "message": "Environment ready for promotion",
                    "globalLock": "LOCKED", "blockingError": false,
                    "promotionId": ID, "timeStamp": ts, "type": "promotion"}),
        )
        .unwrap()
    }

    fn running() -> Assessment {
        assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "RUNNING", "message": "Clone repos", "globalLock": "LOCKED",
                    "blockingError": false, "promotionId": ID}),
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
            phase(&ready("t1")),
            Phase::Ready {
                promotion_id: Some(ID.into())
            }
        );
        assert!(matches!(phase(&running()), Phase::Running { .. }));
        assert!(matches!(phase(&locking_upper_first()), Phase::Unsettled(_)));
    }

    #[test]
    fn a_phase_needs_every_source_to_agree() {
        // READY with one side still locking is not ready to promote.
        let half = assess(
            &lock("locked", "locking", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "promotionId": ID}),
        )
        .unwrap();
        assert!(matches!(phase(&half), Phase::Unsettled(_)));
        // UNLOCKED status over a lock/state still unlocking is not unlocked.
        let lagging = assess(
            &lock("unlocking", "unlocking", "unlocking"),
            &json!({"status": "UNLOCKED", "globalLock": "UNLOCKED"}),
        )
        .unwrap();
        assert!(matches!(phase(&lagging), Phase::Unsettled(_)));
        // A missing globalLock is not an agreeing one.
        let silent = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "promotionId": ID}),
        )
        .unwrap();
        assert!(matches!(phase(&silent), Phase::Unsettled(_)));
    }

    #[test]
    fn a_blocking_error_outranks_every_other_reading() {
        let blocked = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED", "blockingError": true}),
        )
        .unwrap();
        assert_eq!(phase(&blocked), Phase::Blocked);
        assert!(check_promote(&blocked, "uat").is_err());
    }

    #[test]
    fn verbs_refuse_on_the_lower_environment() {
        // The lower env answers lock/state with `result` alone (measured).
        let lower = assess(
            &json!({"result": "unlocked", "description": "Environment unlocked"}),
            &json!({"status": "UNLOCKED", "globalLock": "UNLOCKED"}),
        )
        .unwrap();
        let err = check_lock(&lower, "dev").unwrap_err();
        assert!(err.contains("not the upper environment"), "{err}");
        assert!(check_promote(&lower, "dev").is_err());
        assert!(check_unlock(&lower, "dev").is_err());
    }

    #[test]
    fn lock_starts_only_from_unlocked_and_needs_consent() {
        assert!(check_lock(&unlocked(), "uat").is_ok());
        let err = authorize_lock(&unlocked(), "uat", false).unwrap_err();
        assert!(err.contains("--force"), "{err}");
        assert!(authorize_lock(&unlocked(), "uat", true).is_ok());
        let held = check_lock(&ready("t"), "uat").unwrap_err();
        assert!(held.contains(ID), "{held}");
        assert!(check_lock(&running(), "uat").is_err());
        assert!(check_lock(&locking_upper_first(), "uat").is_err());
    }

    #[test]
    fn promote_starts_only_from_ready_and_keeps_its_kind() {
        assert!(
            check_promote(&unlocked(), "uat")
                .unwrap_err()
                .contains("not locked")
        );
        assert!(
            check_promote(&running(), "uat")
                .unwrap_err()
                .contains("running")
        );
        assert!(check_promote(&locking_upper_first(), "uat").is_err());
        assert!(authorize_promote(&ready("t"), "uat", true, false).is_err());
        assert!(
            authorize_promote(&ready("t"), "uat", true, true)
                .unwrap()
                .dry_run()
        );
        assert!(
            !authorize_promote(&ready("t"), "uat", false, true)
                .unwrap()
                .dry_run()
        );
        let real = authorize_promote(&ready("t"), "uat", false, false).unwrap_err();
        assert!(real.contains("becomes live"), "{real}");
    }

    #[test]
    fn unlock_releases_the_id_the_tenant_reported_and_never_mid_run() {
        let permit = authorize_unlock(&ready("t"), "uat", true).unwrap();
        assert_eq!(permit.promotion_id(), ID);
        assert!(authorize_unlock(&ready("t"), "uat", false).is_err());
        assert!(
            check_unlock(&running(), "uat")
                .unwrap_err()
                .contains("running")
        );
        assert!(check_unlock(&unlocked(), "uat").is_err());
        let anonymous = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "READY", "globalLock": "LOCKED"}),
        )
        .unwrap();
        assert!(
            check_unlock(&anonymous, "uat")
                .unwrap_err()
                .contains("no promotionId")
        );
    }

    #[test]
    fn lock_and_unlock_settle_only_at_their_end_state() {
        assert!(matches!(lock_settled(&unlocked()), Settle::Pending(_)));
        assert!(matches!(
            lock_settled(&locking_upper_first()),
            Settle::Pending(_)
        ));
        assert_eq!(lock_settled(&ready("t")), Settle::Done);
        assert!(matches!(lock_settled(&running()), Settle::Failed(_)));

        assert!(matches!(unlock_settled(&ready("t")), Settle::Pending(_)));
        assert_eq!(unlock_settled(&unlocked()), Settle::Done);
        assert!(matches!(unlock_settled(&running()), Settle::Failed(_)));
    }

    #[test]
    fn a_promotion_is_not_finished_by_the_ready_it_started_from() {
        // The discriminating case: READY with the pre-POST timestamp is the
        // state a promotion starts from, not its end.
        let mut watch = PromoteWatch {
            timestamp_before: Some("t0".into()),
            seen_running: false,
        };
        assert!(matches!(
            promote_settled(&ready("t0"), &mut watch),
            Settle::Pending(_)
        ));
        assert!(matches!(
            promote_settled(&running(), &mut watch),
            Settle::Pending(_)
        ));
        assert!(watch.seen_running);
        assert_eq!(promote_settled(&ready("t0"), &mut watch), Settle::Done);

        // A run too quick to be seen RUNNING still ends: the timestamp moved.
        let mut quick = PromoteWatch {
            timestamp_before: Some("t0".into()),
            seen_running: false,
        };
        assert_eq!(promote_settled(&ready("t1"), &mut quick), Settle::Done);
    }

    #[test]
    fn a_promotion_that_leaves_ready_and_running_has_failed() {
        let mut watch = PromoteWatch {
            timestamp_before: Some("t0".into()),
            seen_running: true,
        };
        assert!(matches!(
            promote_settled(&unlocked(), &mut watch),
            Settle::Failed(_)
        ));
        let odd = assess(
            &lock("locked", "locked", "locked"),
            &json!({"status": "ERROR", "globalLock": "LOCKED", "message": "boom"}),
        )
        .unwrap();
        let Settle::Failed(why) = promote_settled(&odd, &mut watch) else {
            panic!("an unmeasured status after starting must fail");
        };
        assert!(why.contains("ERROR") && why.contains("boom"), "{why}");
        // …but before it has started, the same reading is only pending.
        let mut fresh = PromoteWatch::default();
        assert!(matches!(
            promote_settled(&odd, &mut fresh),
            Settle::Pending(_)
        ));
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
