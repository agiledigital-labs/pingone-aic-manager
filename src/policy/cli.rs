//! `aic policy` parser and command implementation.
//!
//! Three collections with three different create contracts and one shared
//! pull/push shape; see `docs/api/21-am-policies.md`. The verb asymmetry is
//! [`crate::policy::api`]'s problem, so everything here goes through
//! `upsert_*`.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::cli::{print_json, print_table, realm_arg, tenant_for};
use crate::config::ProjectConfig;
use crate::policy::{api, spec};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum PolicyCommand {
    /// List policies in a realm.
    List {
        /// Only policies in this policy set.
        #[arg(long = "set")]
        set: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show one policy.
    Show {
        name: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Pull a policy into the workspace tree, with a snapshot.
    Pull {
        /// Policy name; omit with --all.
        name: Option<String>,
        /// Pull every policy in the realm (or in --set).
        #[arg(long)]
        all: bool,
        #[arg(long = "set")]
        set: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Push a local policy, refusing when the remote drifted since the pull.
    Push {
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete a policy.
    Rm {
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Policy sets (`applications` on the wire).
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },
    /// Resource types.
    Rt {
        #[command(subcommand)]
        command: RtCommand,
    },
    /// The realm's subject- and condition-type catalogs.
    Types {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ask the PDP, and explain the answer.
    Eval(EvalArgs),
}

#[derive(Subcommand, Debug)]
pub enum SetCommand {
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        name: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    Pull {
        name: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    Push {
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    Rm {
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum RtCommand {
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        /// Resource-type id (`uuid`), not its display name.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    Pull {
        id: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    Push {
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    Rm {
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args, Debug)]
pub struct EvalArgs {
    /// The policy set to evaluate against.
    #[arg(long = "set")]
    pub set: String,
    /// A resource URL. Repeat for a batch; AM answers one row per resource.
    #[arg(long = "resource", required = true)]
    pub resources: Vec<String>,
    /// Actions you expect. Used only to sharpen the explanation.
    #[arg(long = "action")]
    pub actions: Vec<String>,
    /// The subject token. **AM does not verify it** — see
    /// `docs/api/21-am-policies.md`. Prefer --subject-jwt-file.
    #[arg(long = "subject-jwt", conflicts_with_all = ["subject_jwt_file", "subject_sso"])]
    pub subject_jwt: Option<String>,
    /// Read the subject token from a file, keeping it out of shell history.
    #[arg(long = "subject-jwt-file", conflicts_with = "subject_sso")]
    pub subject_jwt_file: Option<PathBuf>,
    /// An AM SSO token. An OAuth2 access token here is a 400.
    #[arg(long = "subject-sso")]
    pub subject_sso: Option<String>,
    /// Environment entry, `key=value`; repeat, and repeat a key to build an
    /// array. `OAuth2Scope` reads `scope`, singular.
    #[arg(long = "env")]
    pub environment: Vec<String>,
    /// Print the raw response and nothing else.
    #[arg(long)]
    pub json: bool,
    /// Skip the explanation of a `{}` answer.
    #[arg(long)]
    pub no_explain: bool,
    #[arg(long)]
    pub realm: Option<String>,
    #[arg(long)]
    pub tenant: Option<String>,
}

// ------------------------------------------------------------------ paths

/// The three collections share one workspace layout so `pull`/`push` is one
/// implementation rather than three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Policy,
    Set,
    ResourceType,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Self::Policy => "policies",
            Self::Set => "sets",
            Self::ResourceType => "resourcetypes",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Set => "policy set",
            Self::ResourceType => "resource type",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Set => "policy set",
            Self::ResourceType => "policy rt",
        }
    }
}

fn validate_file_name(kind: Kind, name: &str) -> Result<()> {
    if name.is_empty() || name.contains(std::path::is_separator) || name.starts_with('.') {
        return Err(Error::Config(format!(
            "{} name {name:?} is not usable as a file name",
            kind.label()
        )));
    }
    Ok(())
}

fn object_path(kind: Kind, tenant: &str, realm: &str, name: &str) -> Result<PathBuf> {
    validate_file_name(kind, name)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("policy")
        .join(realm)
        .join(kind.dir())
        .join(format!("{name}.json")))
}

fn snapshot_path(kind: Kind, tenant: &str, realm: &str, name: &str) -> Result<PathBuf> {
    validate_file_name(kind, name)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("policy")
        .join(realm)
        .join(kind.dir())
        .join(".snapshots")
        .join(format!("{name}.json")))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Config(format!("read {}: {error}", path.display())));
        }
    };
    let value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Config(format!("parse {}: {error}", path.display())))?;
    Ok(Some(value))
}

// ------------------------------------------------------------ push policy

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushDecision {
    NothingToDo,
    Push,
    BlockedMissingSnapshot,
    BlockedRemoteDrift,
}

/// CLAUDE.md §5, applied to objects that mostly have no `_rev`: compare
/// authored content, and let a remote that has drifted back to the snapshot
/// through.
fn push_decision(
    local: &Value,
    remote: Option<&Value>,
    snapshot: Option<&Value>,
    force: bool,
) -> PushDecision {
    let Some(remote) = remote else {
        // Nothing there to overwrite, so there is nothing to lose.
        return PushDecision::Push;
    };
    if spec::content_equal(local, remote) {
        return PushDecision::NothingToDo;
    }
    if force {
        return PushDecision::Push;
    }
    match snapshot {
        None => PushDecision::BlockedMissingSnapshot,
        Some(snapshot) if spec::content_equal(remote, snapshot) => PushDecision::Push,
        Some(_) => PushDecision::BlockedRemoteDrift,
    }
}

fn push_block_message(kind: Kind, name: &str, decision: &PushDecision) -> String {
    match decision {
        PushDecision::BlockedMissingSnapshot => format!(
            "no snapshot for {} {name:?}; run `aic {} pull {name}` first, or pass --force",
            kind.label(),
            kind.command()
        ),
        PushDecision::BlockedRemoteDrift => format!(
            "remote {} {name} changed since you last pulled; re-pull to see the drift, or pass --force to overwrite it",
            kind.label()
        ),
        _ => unreachable!("not a block"),
    }
}

// ------------------------------------------------------------ API bridging

async fn read_object(kind: Kind, tenant: &str, realm: &str, name: &str) -> Result<Value> {
    match kind {
        Kind::Policy => api::read_policy(tenant, realm, name).await,
        Kind::Set => api::read_set(tenant, realm, name).await,
        Kind::ResourceType => api::read_resource_type(tenant, realm, name).await,
    }
}

async fn read_object_opt(
    kind: Kind,
    tenant: &str,
    realm: &str,
    name: &str,
) -> Result<Option<Value>> {
    match read_object(kind, tenant, realm, name).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if api::is_not_found(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn list_objects(kind: Kind, tenant: &str, realm: &str) -> Result<Vec<Value>> {
    match kind {
        Kind::Policy => api::list_policies(tenant, realm).await,
        Kind::Set => api::list_sets(tenant, realm).await,
        Kind::ResourceType => api::list_resource_types(tenant, realm).await,
    }
}

async fn write_object(
    kind: Kind,
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    prod: bool,
) -> Result<api::Written> {
    match kind {
        Kind::Policy => api::upsert_policy(tenant, realm, name, body, prod)
            .await
            .map(|(_, written)| written),
        Kind::Set => api::upsert_set(tenant, realm, name, body, prod)
            .await
            .map(|(_, written)| written),
        // The one collection where `PUT` creates as well as updates. Probing
        // first anyway, purely so the log line can say which it was.
        Kind::ResourceType => {
            let existed = read_object_opt(kind, tenant, realm, name).await?.is_some();
            api::put_resource_type(tenant, realm, name, body, prod).await?;
            Ok(if existed {
                api::Written::Updated
            } else {
                api::Written::Created
            })
        }
    }
}

async fn delete_object(
    kind: Kind,
    tenant: &str,
    realm: &str,
    name: &str,
    prod: bool,
) -> Result<()> {
    match kind {
        Kind::Policy => api::delete_policy(tenant, realm, name, prod)
            .await
            .map(drop),
        Kind::Set => api::delete_set(tenant, realm, name, prod).await.map(drop),
        Kind::ResourceType => api::delete_resource_type(tenant, realm, name, prod)
            .await
            .map(drop),
    }
}

fn object_name(kind: Kind, value: &Value) -> String {
    let key = match kind {
        Kind::ResourceType => "uuid",
        _ => "name",
    };
    value
        .get(key)
        .or_else(|| value.get("name"))
        .or_else(|| value.get("_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

// ------------------------------------------------------------- operations

async fn pull(
    kind: Kind,
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    name: Option<String>,
    all: bool,
    set_filter: Option<String>,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("policy", realm_arg_value)?;

    let names = match (name, all) {
        (Some(_), true) => {
            return Err(Error::Config(format!(
                "pass a {} name or --all, not both",
                kind.label()
            )));
        }
        (Some(name), false) => vec![name],
        (None, true) => list_objects(kind, &tenant, &realm)
            .await?
            .iter()
            .filter(|value| in_set(kind, value, set_filter.as_deref()))
            .map(|value| object_name(kind, value))
            .filter(|name| !name.is_empty())
            .collect(),
        (None, false) => {
            return Err(Error::Config(format!(
                "name a {} to pull, or pass --all",
                kind.label()
            )));
        }
    };

    if names.is_empty() {
        println!("no {}s to pull from {tenant}/{realm}", kind.label());
        return Ok(());
    }

    for name in &names {
        let remote = read_object(kind, &tenant, &realm, name).await?;
        let content = spec::content(&remote);
        write_json(&object_path(kind, &tenant, &realm, name)?, &content)?;
        write_json(&snapshot_path(kind, &tenant, &realm, name)?, &content)?;
    }
    println!(
        "pulled {} {}(s) -> {}",
        names.len(),
        kind.label(),
        object_path(kind, &tenant, &realm, &names[0])?
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default()
    );
    Ok(())
}

fn in_set(kind: Kind, value: &Value, set_filter: Option<&str>) -> bool {
    let Some(set) = set_filter else { return true };
    if kind != Kind::Policy {
        return true;
    }
    value.get("applicationName").and_then(Value::as_str) == Some(set)
}

async fn push(
    kind: Kind,
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    name: String,
    force: bool,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("policy", realm_arg_value)?;
    let path = object_path(kind, &tenant, &realm, &name)?;
    let local = read_json(&path)?.ok_or_else(|| {
        Error::Config(format!(
            "no local {} at {}; run `aic {} pull {name}` first",
            kind.label(),
            path.display(),
            kind.command()
        ))
    })?;
    let remote = read_object_opt(kind, &tenant, &realm, &name).await?;
    let snapshot = read_json(&snapshot_path(kind, &tenant, &realm, &name)?)?;

    match push_decision(&local, remote.as_ref(), snapshot.as_ref(), force) {
        PushDecision::NothingToDo => {
            if let Some(remote) = remote.as_ref() {
                write_json(
                    &snapshot_path(kind, &tenant, &realm, &name)?,
                    &spec::content(remote),
                )?;
            }
            println!("{} {name} already matches {tenant}/{realm}", kind.label());
            Ok(())
        }
        blocked @ (PushDecision::BlockedMissingSnapshot | PushDecision::BlockedRemoteDrift) => {
            Err(Error::Config(push_block_message(kind, &name, &blocked)))
        }
        PushDecision::Push => {
            let body = spec::content(&local);
            let written = write_object(kind, &tenant, &realm, &name, body, yes).await?;
            let refreshed = read_object(kind, &tenant, &realm, &name).await?;
            write_json(
                &snapshot_path(kind, &tenant, &realm, &name)?,
                &spec::content(&refreshed),
            )?;
            println!(
                "{} {} {name} -> {tenant}/{realm}",
                written.as_str(),
                kind.label()
            );
            Ok(())
        }
    }
}

async fn remove(
    kind: Kind,
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    name: String,
    force: bool,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("policy", realm_arg_value)?;
    if !force {
        eprintln!(
            "would delete {} {name} from {tenant}/{realm}; pass --force to delete it",
            kind.label()
        );
        return Err(Error::Config(format!(
            "{} delete requires --force",
            kind.label()
        )));
    }
    delete_object(kind, &tenant, &realm, &name, yes).await?;
    let snapshot = snapshot_path(kind, &tenant, &realm, &name)?;
    if let Err(error) = std::fs::remove_file(&snapshot)
        && error.kind() != ErrorKind::NotFound
    {
        eprintln!("warning: remove snapshot {}: {error}", snapshot.display());
    }
    println!("deleted {} {name}", kind.label());
    Ok(())
}

async fn show(
    kind: Kind,
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    name: &str,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("policy", realm_arg_value)?;
    let value = read_object(kind, &tenant, &realm, name).await?;
    print_json(&value)
}

// --------------------------------------------------------------- listings

fn policy_rows(policies: &[Value]) -> Vec<Vec<String>> {
    policies
        .iter()
        .map(|policy| {
            let actions = policy
                .get("actionValues")
                .and_then(Value::as_object)
                .map(|actions| {
                    actions
                        .iter()
                        .map(|(name, allowed)| {
                            if allowed.as_bool().unwrap_or(false) {
                                name.clone()
                            } else {
                                format!("!{name}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            vec![
                text(policy, "name"),
                text(policy, "applicationName"),
                if policy
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    String::new()
                } else {
                    "inactive".to_string()
                },
                actions,
                elide(&list(policy, "resources"), 3),
            ]
        })
        .collect()
}

/// A stock `URL` resource type on a real tenant carries several hundred
/// patterns, and a list column that prints them all buries every other row.
/// Listings elide; `show` still prints everything.
fn elide(items: &[String], keep: usize) -> String {
    if items.len() <= keep {
        return items.join(" ");
    }
    format!("{} (+{} more)", items[..keep].join(" "), items.len() - keep)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn list(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

// ------------------------------------------------------------------- eval

fn parse_environment(entries: &[String]) -> Result<Option<Value>> {
    if entries.is_empty() {
        return Ok(None);
    }
    let mut map = serde_json::Map::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| Error::Config(format!("--env {entry:?} is not in key=value form")))?;
        // AM's environment values are arrays of strings throughout, so a
        // repeated key accumulates rather than replaces.
        let slot = map
            .entry(key.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(array) = slot.as_array_mut() {
            array.push(Value::String(value.to_string()));
        }
    }
    Ok(Some(Value::Object(map)))
}

/// The subject as AM wants it, plus what we can tell about it locally. The
/// claims are decoded, never verified — see [`spec::unverified_claims`].
struct Subject {
    wire: Option<Value>,
    kind: spec::SubjectKind,
    claims: Option<serde_json::Map<String, Value>>,
}

fn read_subject(args: &EvalArgs) -> Result<Subject> {
    let jwt = match (&args.subject_jwt, &args.subject_jwt_file) {
        (Some(jwt), _) => Some(jwt.clone()),
        (None, Some(path)) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| Error::Config(format!("read {}: {error}", path.display())))?;
            let text = text.trim().to_string();
            if text.is_empty() {
                return Err(Error::Config(format!("{} is empty", path.display())));
            }
            Some(text)
        }
        (None, None) => None,
    };
    if let Some(jwt) = jwt {
        let claims = spec::unverified_claims(&jwt);
        return Ok(Subject {
            wire: Some(json!({"jwt": jwt})),
            kind: spec::SubjectKind::Jwt,
            claims,
        });
    }
    if let Some(sso) = &args.subject_sso {
        return Ok(Subject {
            wire: Some(json!({"ssoToken": sso})),
            kind: spec::SubjectKind::SsoToken,
            claims: None,
        });
    }
    Ok(Subject {
        wire: None,
        kind: spec::SubjectKind::Caller,
        claims: None,
    })
}

async fn eval(args: EvalArgs) -> Result<()> {
    let tenant = tenant_for(args.tenant.clone())?;
    let realm = realm_arg("policy", args.realm.clone())?;
    let subject = read_subject(&args)?;
    let subject_kind = subject.kind;
    let environment = parse_environment(&args.environment)?;
    let environment_keys = environment
        .as_ref()
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let body = api::evaluate_body(&args.set, &args.resources, subject.wire, environment);
    let rows = api::evaluate(&tenant, &realm, body).await?;

    if args.json {
        return print_json(&rows);
    }

    let table = rows
        .iter()
        .map(|row| {
            let actions = row
                .get("actions")
                .and_then(Value::as_object)
                .map(|actions| {
                    if actions.is_empty() {
                        "{} no policy applied".to_string()
                    } else {
                        actions
                            .iter()
                            .map(|(name, allowed)| {
                                format!(
                                    "{name}={}",
                                    if allowed.as_bool().unwrap_or(false) {
                                        "allow"
                                    } else {
                                        "DENY"
                                    }
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                })
                .unwrap_or_default();
            vec![text(row, "resource"), actions]
        })
        .collect::<Vec<_>>();
    print_table(&["RESOURCE", "DECISION"], &table);

    if args.no_explain {
        return Ok(());
    }

    // The explanation needs the set, its resource types and its policies. One
    // extra round trip each, and only when something was not granted.
    let needs_explaining = rows.iter().any(|row| {
        let actions = row.get("actions").and_then(Value::as_object);
        match actions {
            None => true,
            Some(actions) if actions.is_empty() => true,
            Some(actions) => !args
                .actions
                .iter()
                .all(|wanted| actions.contains_key(wanted)),
        }
    });
    if !needs_explaining {
        return Ok(());
    }

    let set = api::read_set(&tenant, &realm, &args.set).await?;
    let mut resource_types = Vec::new();
    for id in list(&set, "resourceTypeUuids") {
        match api::read_resource_type(&tenant, &realm, &id).await {
            Ok(value) => resource_types.push(value),
            Err(error) => eprintln!("warning: could not read resource type {id}: {error}"),
        }
    }
    let policies = api::list_policies(&tenant, &realm)
        .await?
        .into_iter()
        .filter(|policy| text(policy, "applicationName") == args.set)
        .collect::<Vec<_>>();

    let empty = serde_json::Map::new();
    for row in &rows {
        let resource = text(row, "resource");
        let actions = row
            .get("actions")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let hints = spec::diagnose(&spec::Decision {
            resource: &resource,
            actions,
            wanted: &args.actions,
            resource_types: &resource_types,
            policies: &policies,
            subject_kind,
            environment_keys: &environment_keys,
            subject_claims: subject.claims.as_ref(),
        });
        if hints.is_empty() {
            continue;
        }
        println!("\n{resource}");
        for hint in hints {
            println!("  - {}", hint.text);
        }
    }
    if subject_kind == spec::SubjectKind::Jwt {
        println!(
            "\nnote: AM does not verify a `jwt` subject — not the signature, not the expiry. \
             A resource server must verify locally before it evaluates."
        );
    }
    Ok(())
}

// --------------------------------------------------------------- dispatch

pub async fn run(command: PolicyCommand) -> Result<()> {
    match command {
        PolicyCommand::List {
            set,
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("policy", realm)?;
            let policies = api::list_policies(&tenant, &realm)
                .await?
                .into_iter()
                .filter(|policy| in_set(Kind::Policy, policy, set.as_deref()))
                .collect::<Vec<_>>();
            if json {
                print_json(&policies)
            } else {
                print_table(
                    &["POLICY", "SET", "STATE", "ACTIONS", "RESOURCES"],
                    &policy_rows(&policies),
                );
                Ok(())
            }
        }
        PolicyCommand::Show {
            name,
            realm,
            tenant,
        } => show(Kind::Policy, tenant, realm, &name).await,
        PolicyCommand::Pull {
            name,
            all,
            set,
            realm,
            tenant,
        } => pull(Kind::Policy, tenant, realm, name, all, set).await,
        PolicyCommand::Push {
            name,
            force,
            realm,
            tenant,
            yes,
        } => push(Kind::Policy, tenant, realm, name, force, yes).await,
        PolicyCommand::Rm {
            name,
            force,
            realm,
            tenant,
            yes,
        } => remove(Kind::Policy, tenant, realm, name, force, yes).await,
        PolicyCommand::Set { command } => run_set(command).await,
        PolicyCommand::Rt { command } => run_rt(command).await,
        PolicyCommand::Types {
            realm,
            tenant,
            json,
        } => types(tenant, realm, json).await,
        PolicyCommand::Eval(args) => eval(args).await,
    }
}

async fn run_set(command: SetCommand) -> Result<()> {
    match command {
        SetCommand::List {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("policy", realm)?;
            let sets = api::list_sets(&tenant, &realm).await?;
            if json {
                print_json(&sets)
            } else {
                let rows = sets
                    .iter()
                    .map(|set| {
                        vec![
                            text(set, "name"),
                            elide(&list(set, "resourceTypeUuids"), 3),
                            text(set, "entitlementCombiner"),
                            list(set, "subjects").len().to_string(),
                            list(set, "conditions").len().to_string(),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(
                    &[
                        "SET",
                        "RESOURCE TYPES",
                        "COMBINER",
                        "SUBJECTS",
                        "CONDITIONS",
                    ],
                    &rows,
                );
                Ok(())
            }
        }
        SetCommand::Show {
            name,
            realm,
            tenant,
        } => show(Kind::Set, tenant, realm, &name).await,
        SetCommand::Pull {
            name,
            all,
            realm,
            tenant,
        } => pull(Kind::Set, tenant, realm, name, all, None).await,
        SetCommand::Push {
            name,
            force,
            realm,
            tenant,
            yes,
        } => push(Kind::Set, tenant, realm, name, force, yes).await,
        SetCommand::Rm {
            name,
            force,
            realm,
            tenant,
            yes,
        } => remove(Kind::Set, tenant, realm, name, force, yes).await,
    }
}

async fn run_rt(command: RtCommand) -> Result<()> {
    match command {
        RtCommand::List {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("policy", realm)?;
            let types = api::list_resource_types(&tenant, &realm).await?;
            if json {
                print_json(&types)
            } else {
                let rows = types
                    .iter()
                    .map(|rt| {
                        let actions = rt
                            .get("actions")
                            .and_then(Value::as_object)
                            .map(|actions| actions.keys().cloned().collect::<Vec<_>>().join(","))
                            .unwrap_or_default();
                        vec![
                            text(rt, "uuid"),
                            text(rt, "name"),
                            actions,
                            elide(&list(rt, "patterns"), 3),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(&["ID", "NAME", "ACTIONS", "PATTERNS"], &rows);
                Ok(())
            }
        }
        RtCommand::Show { id, realm, tenant } => show(Kind::ResourceType, tenant, realm, &id).await,
        RtCommand::Pull {
            id,
            all,
            realm,
            tenant,
        } => pull(Kind::ResourceType, tenant, realm, id, all, None).await,
        RtCommand::Push {
            id,
            force,
            realm,
            tenant,
            yes,
        } => push(Kind::ResourceType, tenant, realm, id, force, yes).await,
        RtCommand::Rm {
            id,
            force,
            realm,
            tenant,
            yes,
        } => remove(Kind::ResourceType, tenant, realm, id, force, yes).await,
    }
}

async fn types(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("policy", realm_arg_value)?;
    let subjects = api::subject_types(&tenant, &realm).await?;
    let conditions = api::condition_types(&tenant, &realm).await?;
    if json {
        return print_json(&json!({"subjects": subjects, "conditions": conditions}));
    }
    let rows = subjects
        .iter()
        .map(|value| vec!["subject".to_string(), type_row(value)])
        .chain(
            conditions
                .iter()
                .map(|value| vec!["condition".to_string(), type_row(value)]),
        )
        .collect::<Vec<_>>();
    print_table(&["KIND", "TYPE — FIELDS"], &rows);
    eprintln!(
        "\nnote: a type listed here is available in the realm; a policy may only use it if the \
         policy set permits it. Check `aic policy set show <name>`."
    );
    Ok(())
}

fn type_row(value: &Value) -> String {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| text(value, "_id"));
    let fields = value
        .get("config")
        .and_then(|config| config.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    if fields.is_empty() {
        title
    } else {
        format!("{title} — {fields}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(json: serde_json::Value) -> Value {
        json
    }

    #[test]
    fn a_first_push_with_no_remote_is_allowed_without_a_snapshot() {
        let local = v(json!({"name": "P"}));
        assert_eq!(push_decision(&local, None, None, false), PushDecision::Push);
    }

    #[test]
    fn an_unchanged_remote_is_nothing_to_do_even_with_different_audit_fields() {
        let local = v(json!({"name": "P", "active": true}));
        let remote = v(json!({"name": "P", "active": true, "lastModifiedDate": "later"}));
        assert_eq!(
            push_decision(&local, Some(&remote), None, false),
            PushDecision::NothingToDo
        );
    }

    #[test]
    fn a_drifted_remote_blocks_and_force_overrides() {
        let local = v(json!({"name": "P", "active": false}));
        let remote = v(json!({"name": "P", "active": true}));
        let snapshot = v(json!({"name": "P", "description": "what we forked from"}));
        assert_eq!(
            push_decision(&local, Some(&remote), Some(&snapshot), false),
            PushDecision::BlockedRemoteDrift
        );
        assert_eq!(
            push_decision(&local, Some(&remote), Some(&snapshot), true),
            PushDecision::Push
        );
    }

    #[test]
    fn a_remote_that_drifted_back_to_the_snapshot_is_pushable() {
        // The whole point of comparing content rather than a revision: the
        // remote was edited and edited back, so overwriting loses nothing.
        let local = v(json!({"name": "P", "active": false}));
        let remote = v(json!({"name": "P", "active": true, "lastModifiedDate": "much later"}));
        let snapshot = v(json!({"name": "P", "active": true}));
        assert_eq!(
            push_decision(&local, Some(&remote), Some(&snapshot), false),
            PushDecision::Push
        );
    }

    #[test]
    fn a_changed_remote_with_no_snapshot_blocks() {
        let local = v(json!({"name": "P", "active": false}));
        let remote = v(json!({"name": "P", "active": true}));
        assert_eq!(
            push_decision(&local, Some(&remote), None, false),
            PushDecision::BlockedMissingSnapshot
        );
    }

    #[test]
    fn a_repeated_env_key_accumulates_into_an_array() {
        let env = parse_environment(&["scope=a".to_string(), "scope=b".to_string()])
            .unwrap()
            .unwrap();
        assert_eq!(env, json!({"scope": ["a", "b"]}));
    }

    #[test]
    fn an_env_entry_without_an_equals_is_an_error() {
        assert!(parse_environment(&["scope".to_string()]).is_err());
    }

    #[test]
    fn a_value_containing_an_equals_keeps_it() {
        let env = parse_environment(&["q=a=b".to_string()]).unwrap().unwrap();
        assert_eq!(env, json!({"q": ["a=b"]}));
    }

    #[test]
    fn a_long_pattern_list_is_elided_but_a_short_one_is_printed_whole() {
        let three = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(elide(&three, 3), "a b c");
        let five = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
            "e".to_string(),
        ];
        assert_eq!(elide(&five, 3), "a b c (+2 more)");
    }

    #[test]
    fn a_name_that_would_escape_the_workspace_is_refused() {
        for bad in ["../evil", ".hidden", ""] {
            assert!(
                validate_file_name(Kind::Policy, bad).is_err(),
                "accepted {bad:?}"
            );
        }
        assert!(validate_file_name(Kind::Policy, "CapTokenDemoScope_orders.read").is_ok());
    }
}
