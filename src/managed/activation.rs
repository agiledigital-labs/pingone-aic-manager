//! Storage-activation follow-up after a new per-type `config/repo.ds` entry.
//!
//! Config read-back is immediate and does not say the new store is live. On
//! the sandbox (2026-11-14) no read-only check flipped from "not yet active"
//! to "active" once the entry was visible, so `--wait-activation` writes one
//! record, reads it back by id, deletes it, and confirms the delete. The
//! functions above [`prepare`] take no network; [`prepare`] and [`finish`] do
//! the I/O.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use super::{api, state, storage};
use crate::{Error, Result};

pub(crate) const MAX_TIMEOUT_SECS: u64 = 3600;
const POLL: Duration = Duration::from_millis(500);
const SYNC_PATH: &str = "/openidm/config/sync";

pub(crate) const WAIT_ACTIVATION_HELP: &str =
    "Probe each new per-type store with one record (optional seconds, default 60)";

pub(crate) const WAIT_ACTIVATION_LONG: &str = "\
After a write that adds a per-type config/repo.ds entry, create one record in each such type, \
read it back by id, delete it, and read again to confirm the delete. SECONDS bounds that whole \
check (default 60, from 1 to 3600).

Config read-back is immediate and does not show that the new store is live. On the sandbox \
(2026-11-14) no read-only check flipped from \"not yet active\" to \"active\" after the entry \
was visible: query shape, _countOnly, info/ping, and the raw repo endpoints did not distinguish \
the stores, and a missing-id 404 changes when the object's OU is first created (a record in the \
generic store does that too) and does not change back when the entry is removed. A record \
written just after the entry stayed readable for the whole watch.

The probe is a real record. It runs onCreate and postCreate when the object has them, and \
onDelete and postDelete when it has them. onUpdate and postUpdate are not invoked. Any sync \
mapping whose source or target is managed/<object> can run on both the create and the delete. \
The command lists the hooks and mappings it found and asks before writing. --yes skips that \
prompt and is also the production gate. A non-interactive run without --yes refuses before any \
probe record is created. On relationship set the schema write has already landed and the \
per-type entry is not added: finish with relationship map --wait-activation --yes. On \
relationship map nothing is switched. A readable probe means the record was read back and the delete was \
confirmed. It does not prove a later record cannot land in the old generic store: that lag was \
reported from a client tenant (2026-11-03) and was not reproducible here. Without this flag the \
command only warns.

--json adds storage_activation with outcome readable, not_confirmed, declined, or not_needed. \
That is separate from activation, the relationship-end liveness wait. --dry-run writes no \
probe. A type that rejects an empty create (required fields) ends not_confirmed and is not \
retried.";

/// What to do with a per-type entry that was, or was not, added.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FollowUp {
    /// No new per-type entry and no flag.
    Quiet,
    /// New per-type entry, flag absent: the existing warning.
    Warn,
    /// Flag set, nothing new to wait for.
    NotNeeded,
    /// Flag set and at least one new per-type entry.
    Probe(Duration),
}

pub(crate) fn parse_timeout(seconds: u64) -> std::result::Result<Duration, &'static str> {
    if (1..=MAX_TIMEOUT_SECS).contains(&seconds) {
        Ok(Duration::from_secs(seconds))
    } else {
        Err("--wait-activation must be from 1 to 3600 seconds")
    }
}

pub(crate) fn follow_up(
    added_type_count: usize,
    wait_seconds: Option<u64>,
) -> std::result::Result<FollowUp, &'static str> {
    let timeout = wait_seconds.map(parse_timeout).transpose()?;
    Ok(match (added_type_count == 0, timeout) {
        (true, None) => FollowUp::Quiet,
        (true, Some(_)) => FollowUp::NotNeeded,
        (false, None) => FollowUp::Warn,
        (false, Some(timeout)) => FollowUp::Probe(timeout),
    })
}

/// Hooks and sync mappings a probe record would run. `hooks_unknown` and
/// `mappings_unknown` mean the list could not be read; an empty list then
/// means the object has none, and the plan must not call that "none".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SideEffects {
    pub object: String,
    pub hooks: Vec<String>,
    pub hooks_unknown: bool,
    pub mappings: Vec<String>,
    pub mappings_unknown: bool,
}

pub(crate) fn side_effects(
    object_name: &str,
    object: Option<&Value>,
    sync: Option<&Value>,
    managed_unreadable: bool,
) -> SideEffects {
    let hooks_unknown = managed_unreadable || object.is_none();
    let hooks = if hooks_unknown {
        Vec::new()
    } else {
        present_hooks(object.unwrap_or(&Value::Null))
    };
    let (mappings, mappings_unknown) = match sync {
        None => (Vec::new(), true),
        Some(document) => match listed_mappings(object_name, document) {
            Some(mappings) => (mappings, false),
            None => (Vec::new(), true),
        },
    };
    SideEffects {
        object: object_name.to_string(),
        hooks,
        hooks_unknown,
        mappings,
        mappings_unknown,
    }
}

fn present_hooks(object: &Value) -> Vec<String> {
    state::HOOK_EVENTS
        .into_iter()
        .filter(|event| matches!(object.get(*event), Some(value) if !value.is_null()))
        .map(str::to_string)
        .collect()
}

/// `None` when `mappings` is present and not an array: the document was read
/// and still could not be listed. A missing `mappings` member is an empty list.
fn listed_mappings(object: &str, sync: &Value) -> Option<Vec<String>> {
    let resource = format!("managed/{object}");
    let items = match sync.get("mappings") {
        None => return Some(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => return None,
    };
    Some(
        items
            .iter()
            .filter_map(|mapping| {
                let source = mapping.get("source").and_then(Value::as_str).unwrap_or("");
                let target = mapping.get("target").and_then(Value::as_str).unwrap_or("");
                if source != resource && target != resource {
                    return None;
                }
                let name = mapping
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("(unnamed)");
                Some(format!("{name}: {source} -> {target}"))
            })
            .collect(),
    )
}

pub(crate) fn plan_text(effects: &[SideEffects]) -> String {
    let mut lines = vec![
        "--wait-activation will create one record in each new per-type store, read it back by \
         id, delete it, and read again to confirm the delete."
            .to_string(),
        "A readable record was stored and then deleted. It does not prove a later record cannot \
         land in the old generic store: on the sandbox (2026-11-14) a record written just after \
         the entry stayed readable, and no read-only check separated \"not yet active\" from \
         \"active\"."
            .to_string(),
        "The create runs onCreate and postCreate when the object has them. The delete runs \
         onDelete and postDelete when the object has them. onUpdate and postUpdate are not \
         invoked. A sync mapping whose source or target is the object can run on both."
            .to_string(),
    ];
    for effect in effects {
        lines.push(format!("managed/{}:", effect.object));
        lines.push(format!(
            "  hooks present: {}",
            named(&effect.hooks, effect.hooks_unknown)
        ));
        if effect.mappings_unknown {
            lines.push(
                "  sync mappings: could not be listed (config/sync was not read)".to_string(),
            );
        } else if effect.mappings.is_empty() {
            lines.push("  sync mappings: none".to_string());
        } else {
            lines.push("  sync mappings:".to_string());
            for mapping in &effect.mappings {
                lines.push(format!("    {mapping}"));
            }
        }
    }
    lines.join("\n")
}

fn named(items: &[String], unknown: bool) -> String {
    if unknown {
        "could not be listed".to_string()
    } else if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeConsent {
    Granted,
    Declined,
}

/// `yes` grants without a prompt. `answered` is `Some` only when a prompt ran.
/// `None` with `yes == false` is a non-interactive refusal: no record is written.
pub(crate) fn consent(
    yes: bool,
    answered: Option<bool>,
) -> std::result::Result<ProbeConsent, &'static str> {
    if yes {
        return Ok(ProbeConsent::Granted);
    }
    match answered {
        Some(true) => Ok(ProbeConsent::Granted),
        Some(false) => Ok(ProbeConsent::Declined),
        None => Err("no terminal to confirm the probe record; pass --yes to write it"),
    }
}

/// How the create call ended. `NotStarted` means the shared timeout had already
/// elapsed, so no request was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreateAttempt {
    Created,
    Rejected { status: u16 },
    GaveUp,
    NotStarted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeFacts {
    pub create: CreateAttempt,
    pub id: Option<String>,
    pub read_back: bool,
    pub delete_attempted: bool,
    pub delete_confirmed: bool,
}

/// Create: 4xx is final. Anything else, including a transport failure (`None`),
/// may be retried until the deadline.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CreateClass {
    Reject(u16),
    Retry,
}

pub(crate) fn classify_create(status: Option<u16>) -> CreateClass {
    match status {
        Some(status) if (400..500).contains(&status) => CreateClass::Reject(status),
        _ => CreateClass::Retry,
    }
}

/// Read-back: 404 keeps polling (the record may not be visible yet). Another
/// 4xx stops. 5xx and transport failures keep polling.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadClass {
    Found,
    Missing,
    Retry,
    Stop,
}

pub(crate) fn classify_read(status: Option<u16>) -> ReadClass {
    match status {
        None => ReadClass::Retry,
        Some(404) => ReadClass::Missing,
        Some(status) if (200..300).contains(&status) => ReadClass::Found,
        Some(status) if (400..500).contains(&status) => ReadClass::Stop,
        Some(_) => ReadClass::Retry,
    }
}

/// Confirming the delete: 404 means the record is gone. A live read (2xx) keeps
/// polling until the deadline. Another 4xx stops, unconfirmed. 5xx and a
/// transport failure (`None`) keep polling.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConfirmClass {
    Gone,
    Present,
    Retry,
    Stop,
}

pub(crate) fn classify_confirm(status: Option<u16>) -> ConfirmClass {
    match status {
        Some(404) => ConfirmClass::Gone,
        None => ConfirmClass::Retry,
        Some(status) if (200..300).contains(&status) => ConfirmClass::Present,
        Some(status) if (500..600).contains(&status) => ConfirmClass::Retry,
        Some(_) => ConfirmClass::Stop,
    }
}

pub(crate) fn judge(facts: &ProbeFacts) -> (&'static str, Option<String>) {
    match &facts.create {
        CreateAttempt::Rejected { status } => (
            "not_confirmed",
            Some(format!(
                "create returned {status}; an empty record was refused, so the new store was not probed"
            )),
        ),
        CreateAttempt::GaveUp => (
            "not_confirmed",
            Some("create did not succeed within the timeout".to_string()),
        ),
        CreateAttempt::NotStarted => (
            "not_confirmed",
            Some("not started; the timeout elapsed".to_string()),
        ),
        CreateAttempt::Created if facts.read_back && facts.delete_confirmed => ("readable", None),
        CreateAttempt::Created if facts.delete_confirmed => (
            "not_confirmed",
            Some(match &facts.id {
                Some(id) => {
                    format!("record {id} was not readable within the timeout; it was deleted")
                }
                None => "record was not readable within the timeout; it was deleted".to_string(),
            }),
        ),
        CreateAttempt::Created => (
            "not_confirmed",
            Some(match &facts.id {
                Some(id) => format!("record {id} was not confirmed deleted; remove it by hand"),
                None => "record was not confirmed deleted".to_string(),
            }),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ObjectProbe {
    pub object: String,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StorageActivation {
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectProbe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub(crate) fn object_probe(object: &str, facts: &ProbeFacts, seconds: f64) -> ObjectProbe {
    let (outcome, detail) = judge(facts);
    ObjectProbe {
        object: object.to_string(),
        outcome,
        seconds: Some(seconds),
        id: if outcome == "readable" {
            None
        } else {
            facts.id.clone()
        },
        detail,
    }
}

pub(crate) fn not_needed() -> StorageActivation {
    StorageActivation {
        outcome: "not_needed",
        seconds: None,
        timeout_seconds: None,
        objects: Vec::new(),
        detail: Some("no new per-type repo.ds entry was added".to_string()),
    }
}

pub(crate) fn declined(objects: &[String]) -> StorageActivation {
    StorageActivation {
        outcome: "declined",
        seconds: None,
        timeout_seconds: None,
        objects: objects
            .iter()
            .map(|object| ObjectProbe {
                object: object.clone(),
                outcome: "declined",
                seconds: None,
                id: None,
                detail: None,
            })
            .collect(),
        detail: Some("probe declined; no record written".to_string()),
    }
}

pub(crate) fn from_objects(
    timeout: Duration,
    seconds: f64,
    objects: Vec<ObjectProbe>,
) -> StorageActivation {
    let outcome = if objects.iter().all(|object| object.outcome == "readable") {
        "readable"
    } else {
        "not_confirmed"
    };
    let failed = objects
        .iter()
        .filter(|object| object.outcome != "readable")
        .count();
    StorageActivation {
        outcome,
        seconds: Some(seconds),
        timeout_seconds: Some(timeout.as_secs()),
        detail: (outcome != "readable")
            .then(|| format!("{failed} of {} not confirmed", objects.len())),
        objects,
    }
}

pub(crate) fn readable_note(objects: &[String], seconds: f64) -> String {
    let subject = if objects.len() == 1 {
        format!("managed/{} probe record was", objects[0])
    } else {
        let names = objects
            .iter()
            .map(|object| format!("managed/{object}"))
            .collect::<Vec<_>>()
            .join(" and ");
        format!("probe records for {names} were")
    };
    format!(
        "{subject} readable and the delete was confirmed ({seconds:.1}s). This does not prove a \
         later record cannot land in the old generic store: that lag was reported from a client \
         tenant (2026-11-03) and was not reproducible on the sandbox (2026-11-14)"
    )
}

pub(crate) fn attach(body: &mut Value, activation: Option<&StorageActivation>) -> Result<()> {
    if let Some(activation) = activation {
        body["storage_activation"] = serde_json::to_value(activation)?;
    }
    Ok(())
}

pub(crate) fn added_type_names(addition: Option<&storage::Addition>) -> Vec<String> {
    addition
        .map(|addition| addition.added_types.keys().cloned().collect())
        .unwrap_or_default()
}

/// Decided before the `repo.ds` write. `Probe` has already asked, when a
/// prompt was required.
pub(crate) enum Pending {
    Quiet,
    Warn,
    NotNeeded,
    Probe {
        timeout: Duration,
        consent: ProbeConsent,
    },
}

/// Print the probe plan and ask, when a probe will run. Call this before the
/// `repo.ds` write so a non-interactive refusal creates no record and adds no
/// per-type entry.
pub(crate) async fn prepare(
    tenant: &str,
    added: &[String],
    wait_seconds: Option<u64>,
    yes: bool,
) -> Result<Pending> {
    let timeout = match follow_up(added.len(), wait_seconds)
        .map_err(|error| Error::Config(error.to_string()))?
    {
        FollowUp::Quiet => return Ok(Pending::Quiet),
        FollowUp::Warn => return Ok(Pending::Warn),
        FollowUp::NotNeeded => return Ok(Pending::NotNeeded),
        FollowUp::Probe(timeout) => timeout,
    };
    let effects = load_effects(tenant, added).await;
    eprintln!("{}", plan_text(&effects));
    let granted = if yes {
        consent(true, None)
    } else {
        let answered = crate::cli::confirm_destructive(
            "storage activation probe",
            "create, read, and delete one probe record in each new per-type store named above?",
            "--yes",
        )?;
        consent(false, Some(answered))
    }
    .map_err(|error| Error::Config(error.to_string()))?;
    Ok(Pending::Probe {
        timeout,
        consent: granted,
    })
}

/// Carry out [`prepare`]'s decision after the per-type entry is confirmed.
/// A declined probe and an unconfirmed one both return `Ok`: the mapping has
/// already landed. The `Err` from [`prepare`] is the refusal that happens
/// before that write.
pub(crate) async fn finish(
    tenant: &str,
    confirmed_prod: bool,
    added: &[String],
    pending: Pending,
) -> Result<Option<StorageActivation>> {
    let activation = match pending {
        Pending::Quiet => return Ok(None),
        Pending::Warn => {
            if let Some(warning) = storage::activation_warning_for(added) {
                eprintln!("{warning}");
            }
            return Ok(None);
        }
        Pending::NotNeeded => {
            eprintln!("no new per-type repo.ds entry; storage activation was not probed");
            not_needed()
        }
        Pending::Probe {
            consent: ProbeConsent::Declined,
            ..
        } => {
            if let Some(warning) = storage::activation_warning_for(added) {
                eprintln!("{warning}");
            }
            eprintln!("probe declined; no record written");
            declined(added)
        }
        Pending::Probe {
            timeout,
            consent: ProbeConsent::Granted,
        } => {
            let started = Instant::now();
            let deadline = started + timeout;
            let mut objects = Vec::with_capacity(added.len());
            for name in added {
                objects.push(probe_object(tenant, confirmed_prod, name, deadline).await);
            }
            let activation = from_objects(timeout, started.elapsed().as_secs_f64(), objects);
            announce(added, &activation);
            activation
        }
    };
    Ok(Some(activation))
}

/// [`prepare`] then [`finish`], for a path that has nothing to write between them.
pub(crate) async fn settle(
    tenant: &str,
    confirmed_prod: bool,
    added: &[String],
    wait_seconds: Option<u64>,
    yes: bool,
) -> Result<Option<StorageActivation>> {
    let pending = prepare(tenant, added, wait_seconds, yes).await?;
    finish(tenant, confirmed_prod, added, pending).await
}

fn announce(added: &[String], activation: &StorageActivation) {
    match activation.outcome {
        "readable" => eprintln!(
            "{}",
            readable_note(added, activation.seconds.unwrap_or(0.0))
        ),
        _ => {
            if let Some(warning) = storage::activation_warning_for(added) {
                eprintln!("{warning}");
            }
            for object in &activation.objects {
                if object.outcome != "readable" {
                    let detail = object
                        .detail
                        .as_deref()
                        .map(|detail| format!(": {detail}"))
                        .unwrap_or_default();
                    eprintln!(
                        "warning: managed/{} storage activation not confirmed{detail}",
                        object.object
                    );
                }
            }
        }
    }
}

async fn load_effects(tenant: &str, added: &[String]) -> Vec<SideEffects> {
    let managed = api::get_managed(tenant).await.ok();
    let sync = crate::aic::api::get(tenant, SYNC_PATH).await.ok();
    added
        .iter()
        .map(|name| {
            let object = managed
                .as_ref()
                .and_then(|document| api::object_named(document, name).ok());
            side_effects(name, object, sync.as_ref(), managed.is_none())
        })
        .collect()
}

async fn probe_object(
    tenant: &str,
    confirmed_prod: bool,
    object: &str,
    deadline: Instant,
) -> ObjectProbe {
    let started = Instant::now();
    let mut facts = ProbeFacts {
        create: CreateAttempt::GaveUp,
        id: None,
        read_back: false,
        delete_attempted: false,
        delete_confirmed: false,
    };
    if Instant::now() >= deadline {
        facts.create = CreateAttempt::NotStarted;
        return object_probe(object, &facts, started.elapsed().as_secs_f64());
    }
    let create_path = format!(
        "/openidm/managed/{}?_action=create",
        crate::idmstore::api::path_segment(object)
    );
    loop {
        match crate::aic::api::post(tenant, &create_path, json!({}), confirmed_prod).await {
            Ok(body) => {
                facts.create = match body.get("_id").and_then(Value::as_str) {
                    Some(id) => {
                        facts.id = Some(id.to_string());
                        eprintln!("probe managed/{object} id {id}");
                        CreateAttempt::Created
                    }
                    None => CreateAttempt::GaveUp,
                };
                break;
            }
            Err(error) => match classify_create(api_status(&error)) {
                CreateClass::Reject(status) => {
                    facts.create = CreateAttempt::Rejected { status };
                    break;
                }
                CreateClass::Retry if Instant::now() < deadline => pause(deadline).await,
                CreateClass::Retry => {
                    facts.create = CreateAttempt::GaveUp;
                    break;
                }
            },
        }
    }
    let Some(id) = facts.id.clone() else {
        return object_probe(object, &facts, started.elapsed().as_secs_f64());
    };
    let read_path = format!(
        "/openidm/managed/{}/{}?_fields=_id",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(&id)
    );
    loop {
        match crate::aic::api::get(tenant, &read_path).await {
            Ok(_) => {
                facts.read_back = true;
                break;
            }
            Err(error) => match classify_read(api_status(&error)) {
                ReadClass::Found => {
                    facts.read_back = true;
                    break;
                }
                ReadClass::Stop => break,
                ReadClass::Missing | ReadClass::Retry => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    pause(deadline).await;
                }
            },
        }
    }
    facts.delete_attempted = true;
    let delete_path = format!(
        "/openidm/managed/{}/{}",
        crate::idmstore::api::path_segment(object),
        crate::idmstore::api::path_segment(&id)
    );
    // A failed delete still falls through to the confirm read. The id stays in
    // the outcome when that read is not a 404.
    let _ = crate::aic::api::delete(tenant, &delete_path, confirmed_prod).await;
    loop {
        match crate::aic::api::get(tenant, &read_path).await {
            Ok(_) => {
                if Instant::now() >= deadline {
                    break;
                }
                pause(deadline).await;
            }
            Err(error) => match classify_confirm(api_status(&error)) {
                ConfirmClass::Gone => {
                    facts.delete_confirmed = true;
                    break;
                }
                ConfirmClass::Stop => break,
                ConfirmClass::Retry | ConfirmClass::Present if Instant::now() < deadline => {
                    pause(deadline).await;
                }
                ConfirmClass::Retry | ConfirmClass::Present => break,
            },
        }
    }
    object_probe(object, &facts, started.elapsed().as_secs_f64())
}

fn api_status(error: &Error) -> Option<u16> {
    match error {
        Error::Api { status, .. } => Some(*status),
        _ => None,
    }
}

async fn pause(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return;
    }
    tokio::time::sleep(POLL.min(remaining)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn follow_up_warns_probes_or_says_nothing() {
        assert_eq!(follow_up(0, None).unwrap(), FollowUp::Quiet);
        assert_eq!(follow_up(0, Some(60)).unwrap(), FollowUp::NotNeeded);
        assert_eq!(follow_up(2, None).unwrap(), FollowUp::Warn);
        assert_eq!(
            follow_up(1, Some(60)).unwrap(),
            FollowUp::Probe(Duration::from_secs(60))
        );
        assert_eq!(
            follow_up(1, None).unwrap(),
            FollowUp::Warn,
            "the flag absent keeps the warning and does not probe"
        );
    }

    #[test]
    fn timeout_is_bounded() {
        assert!(parse_timeout(0).is_err());
        assert!(parse_timeout(3601).is_err());
        assert_eq!(parse_timeout(1).unwrap(), Duration::from_secs(1));
        assert_eq!(parse_timeout(3600).unwrap(), Duration::from_secs(3600));
        assert!(follow_up(1, Some(0)).is_err());
        assert_eq!(
            follow_up(0, Some(0)).unwrap_err(),
            parse_timeout(0).unwrap_err()
        );
    }

    #[test]
    fn consent_grants_declines_or_refuses() {
        assert_eq!(consent(true, None).unwrap(), ProbeConsent::Granted);
        assert_eq!(consent(true, Some(false)).unwrap(), ProbeConsent::Granted);
        assert_eq!(consent(false, Some(true)).unwrap(), ProbeConsent::Granted);
        assert_eq!(consent(false, Some(false)).unwrap(), ProbeConsent::Declined);
        assert!(consent(false, None).unwrap_err().contains("--yes"));
    }

    #[test]
    fn side_effects_name_hooks_and_mappings_and_unknowns() {
        let object = json!({
            "name": "thing",
            "onCreate": { "type": "text/javascript", "source": "a" },
            "onUpdate": { "source": "b" },
            "onDelete": null,
            "postDelete": { "type": "text/javascript", "source": "c" }
        });
        let sync = json!({
            "mappings": [
                {"name": "keep", "source": "managed/thing", "target": "managed/other"},
                {"name": "also", "source": "system/ldap/account", "target": "managed/thing"},
                {"name": "skip", "source": "managed/else", "target": "managed/else"},
                {"source": "managed/thing", "target": "managed/thing"}
            ]
        });
        let effects = side_effects("thing", Some(&object), Some(&sync), false);
        assert_eq!(effects.hooks, ["onCreate", "onUpdate", "postDelete"]);
        assert!(!effects.hooks_unknown);
        assert_eq!(
            effects.mappings,
            [
                "keep: managed/thing -> managed/other",
                "also: system/ldap/account -> managed/thing",
                "(unnamed): managed/thing -> managed/thing",
            ]
        );
        assert!(!effects.mappings_unknown);

        let unknown = side_effects("thing", None, None, true);
        assert!(unknown.hooks_unknown);
        assert!(unknown.mappings_unknown);
        assert!(unknown.hooks.is_empty());
        let plan = plan_text(&[unknown]);
        assert!(
            plan.contains("hooks present: could not be listed"),
            "{plan}"
        );
        assert!(
            plan.contains("sync mappings: could not be listed"),
            "{plan}"
        );
        assert!(!plan.contains("sync mappings: none"), "{plan}");

        let empty = side_effects(
            "thing",
            Some(&json!({"name": "thing"})),
            Some(&json!({})),
            false,
        );
        let plan = plan_text(&[empty]);
        assert!(plan.contains("hooks present: none"), "{plan}");
        assert!(plan.contains("sync mappings: none"), "{plan}");
        assert!(plan.contains("onCreate"), "{plan}");
        assert!(plan.contains("does not prove"), "{plan}");

        let unreadable_shape = side_effects(
            "thing",
            Some(&json!({"name": "thing"})),
            Some(&json!({"mappings": "nope"})),
            false,
        );
        assert!(unreadable_shape.mappings_unknown);
    }

    fn facts(
        create: CreateAttempt,
        id: Option<&str>,
        read_back: bool,
        delete_confirmed: bool,
    ) -> ProbeFacts {
        ProbeFacts {
            create,
            id: id.map(str::to_string),
            read_back,
            delete_attempted: delete_confirmed || id.is_some(),
            delete_confirmed,
        }
    }

    #[test]
    fn judge_is_readable_only_when_the_delete_is_confirmed() {
        let cases = [
            (
                facts(CreateAttempt::Created, Some("id-1"), true, true),
                "readable",
                None,
            ),
            (
                facts(CreateAttempt::Rejected { status: 400 }, None, false, false),
                "not_confirmed",
                Some("create returned 400"),
            ),
            (
                facts(CreateAttempt::GaveUp, None, false, false),
                "not_confirmed",
                Some("create did not succeed"),
            ),
            (
                facts(CreateAttempt::NotStarted, None, false, false),
                "not_confirmed",
                Some("not started"),
            ),
            (
                facts(CreateAttempt::Created, Some("id-2"), false, true),
                "not_confirmed",
                Some("id-2"),
            ),
            (
                facts(CreateAttempt::Created, Some("id-3"), true, false),
                "not_confirmed",
                Some("id-3"),
            ),
            (
                facts(CreateAttempt::Created, Some("id-4"), false, false),
                "not_confirmed",
                Some("not confirmed deleted"),
            ),
        ];
        for (facts, outcome, needle) in cases {
            let (got, detail) = judge(&facts);
            assert_eq!(got, outcome, "{detail:?}");
            if let Some(needle) = needle {
                let detail = detail.unwrap();
                assert!(detail.contains(needle), "{detail}");
            } else {
                assert_eq!(detail, None);
            }
            let object = object_probe("thing", &facts, 0.4);
            if outcome == "readable" {
                assert_eq!(object.id, None);
            } else if let Some(id) = &facts.id {
                assert_eq!(object.id.as_deref(), Some(id.as_str()));
            }
        }
    }

    #[test]
    fn one_unconfirmed_object_fails_the_whole_check() {
        let ok = object_probe(
            "thing",
            &facts(CreateAttempt::Created, Some("a"), true, true),
            0.2,
        );
        let bad = object_probe(
            "owner",
            &facts(CreateAttempt::Created, Some("b"), true, false),
            0.2,
        );
        let all = from_objects(Duration::from_secs(60), 0.4, vec![ok.clone()]);
        assert_eq!(all.outcome, "readable");
        assert_eq!(all.detail, None);
        let mixed = from_objects(Duration::from_secs(60), 0.4, vec![ok, bad]);
        assert_eq!(mixed.outcome, "not_confirmed");
        assert_eq!(mixed.detail.as_deref(), Some("1 of 2 not confirmed"));
        assert_eq!(mixed.timeout_seconds, Some(60));
        let value = serde_json::to_value(&mixed).unwrap();
        assert_eq!(value["outcome"], "not_confirmed");
        assert!(value.get("storage_activation").is_none());
        assert_eq!(value["objects"][1]["id"], "b");
        assert!(value["objects"][0].get("id").is_none());
    }

    #[test]
    fn outcomes_for_a_flag_that_does_not_probe() {
        assert_eq!(not_needed().outcome, "not_needed");
        assert!(not_needed().objects.is_empty());
        let declined = declined(&["thing".into(), "owner".into()]);
        assert_eq!(declined.outcome, "declined");
        assert_eq!(declined.objects.len(), 2);
        assert!(declined.objects.iter().all(|object| object.id.is_none()));
        let mut body = json!({"changed": []});
        attach(&mut body, Some(&declined)).unwrap();
        assert_eq!(body["storage_activation"]["outcome"], "declined");
        let mut fresh = json!({"changed": []});
        attach(&mut fresh, None).unwrap();
        assert!(fresh.get("storage_activation").is_none());
    }

    #[test]
    fn status_classes_match_the_retry_rules() {
        assert_eq!(classify_create(Some(400)), CreateClass::Reject(400));
        assert_eq!(classify_create(Some(404)), CreateClass::Reject(404));
        assert_eq!(classify_create(Some(500)), CreateClass::Retry);
        assert_eq!(classify_create(None), CreateClass::Retry);
        assert_eq!(classify_read(Some(404)), ReadClass::Missing);
        assert_eq!(classify_read(Some(200)), ReadClass::Found);
        assert_eq!(classify_read(Some(400)), ReadClass::Stop);
        assert_eq!(classify_read(Some(503)), ReadClass::Retry);
        assert_eq!(classify_read(None), ReadClass::Retry);
        assert_eq!(classify_confirm(Some(404)), ConfirmClass::Gone);
        assert_eq!(classify_confirm(Some(200)), ConfirmClass::Present);
        assert_eq!(classify_confirm(Some(400)), ConfirmClass::Stop);
        assert_eq!(classify_confirm(Some(502)), ConfirmClass::Retry);
        assert_eq!(classify_confirm(None), ConfirmClass::Retry);
    }

    #[test]
    fn readable_note_names_each_object_once() {
        let one = readable_note(&["thing".into()], 1.25);
        assert!(
            one.contains("managed/thing probe record was readable"),
            "{one}"
        );
        assert!(one.contains("1.3s") || one.contains("1.2s"), "{one}");
        assert!(one.contains("2026-11-14"), "{one}");
        let two = readable_note(&["thing".into(), "owner".into()], 2.0);
        assert!(two.contains("managed/thing and managed/owner"), "{two}");
        assert!(two.contains("were readable"), "{two}");
    }
}
