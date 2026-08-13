//! `aic ctx rm` — print the planner's decision, then execute it.

use std::collections::HashSet;
use std::io::Write;

use clap::Args;
use serde::Serialize;

use crate::cli::{ensure_prod_confirmed, print_json, prompt_available, prompting_disabled};
use crate::config::{self, ProjectConfig};
use crate::offboard::ops::{self, ExecuteReport, Layout, LiveIo, Step, StepStatus};
use crate::offboard::spec::{
    self, DeletePlan, Inventory, PromptAction, ResolvedPurge, TargetDecision, TargetKind,
};
use crate::{Error, Result};

#[derive(Args, Debug)]
pub struct RmOptions {
    /// Accept every offered artifact and skip prompts, including the
    /// typed-name confirmation. Forces past a prompt, never past the
    /// sharing guard — a credential another surviving tenant still needs
    /// is not deleted.
    #[arg(long)]
    pub delete_keys: bool,
    /// Print the plan and exit, changing nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Print the plan as JSON and exit, changing nothing.
    #[arg(long)]
    pub json: bool,
    /// Confirm a write to a production-themed tenant.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Serialize)]
struct PlanJson<'a> {
    tenant: &'a str,
    base_url: &'a str,
    sa_id: Option<&'a str>,
    api_key_id: Option<&'a str>,
    kid: Option<&'a str>,
    targets: Vec<TargetJson<'a>>,
    manual: ManualJson<'a>,
}

#[derive(Serialize)]
struct TargetJson<'a> {
    kind: &'a str,
    identifier: Option<&'a str>,
    state: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_on: Option<bool>,
}

#[derive(Serialize)]
struct ManualJson<'a> {
    sa_id: Option<&'a str>,
    api_key_id: Option<&'a str>,
}

pub async fn run(tenant: String, options: RmOptions) -> Result<()> {
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let departing = cfg
        .tenants
        .iter()
        .find(|candidate| candidate.name == tenant)
        .cloned()
        .ok_or_else(|| Error::Config(format!("no tenant named '{tenant}' in config")))?;

    let names: Vec<String> = cfg
        .tenants
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    let vault = ops::probe_vault(&names).await?;
    let paths = ops::probe_paths(&tenant, &Layout::live());
    let (inventory, _survivors, plan) = ops::plan_for(&departing, &cfg.tenants, &vault, &paths);

    if options.json {
        print_json(&plan_json(&departing, &inventory, &plan))?;
        return Ok(());
    }

    print_plan(&departing, &inventory, &plan);

    if options.dry_run {
        println!("dry-run: nothing changed");
        return Ok(());
    }

    let _ok = ensure_prod_confirmed(&tenant, options.yes)?;
    let purge = collect_purge(&plan, options.delete_keys)?;

    if !options.delete_keys {
        confirm_typed_name(&tenant)?;
    }

    let current = config::read_current_context()?;
    let mut io = LiveIo::default();
    let report = ops::execute(
        &departing,
        &cfg,
        current.as_deref(),
        &inventory,
        &purge,
        &Layout::live(),
        &mut io,
    )
    .await;

    print_report(&plan, &inventory, &purge, &report);
    if !report.config_removed {
        return Err(Error::Config(
            "tenant entry was left in place so the removal can be retried".into(),
        ));
    }
    Ok(())
}

fn collect_purge(plan: &DeletePlan, delete_keys: bool) -> Result<ResolvedPurge> {
    if delete_keys {
        return Ok(plan.resolve_purge(offered_kinds(plan)));
    }
    if prompting_disabled() {
        return Err(Error::Config(
            "removing a tenant requires confirmation; pass --delete-keys with --no-prompt".into(),
        ));
    }

    let mut accepted = HashSet::new();
    for target in &plan.targets {
        match plan.prompt_for(target.kind, &accepted) {
            PromptAction::Absent | PromptAction::Refused { .. } => {}
            PromptAction::Implied { .. } => {
                accepted.insert(target.kind);
            }
            PromptAction::Ask { default_on } => {
                if confirm_target(target.kind.label(), default_on)? {
                    accepted.insert(target.kind);
                }
            }
        }
    }
    Ok(plan.resolve_purge(accepted))
}

fn offered_kinds(plan: &DeletePlan) -> impl Iterator<Item = TargetKind> + '_ {
    plan.targets.iter().filter_map(|target| {
        matches!(target.decision, TargetDecision::Offered { .. }).then_some(target.kind)
    })
}

fn print_plan(tenant: &crate::config::Tenant, inventory: &Inventory, plan: &DeletePlan) {
    println!("remove tenant {} ({})", tenant.name, tenant.base_url);
    println!("  sa_id:      {}", display_id(tenant.sa_id.as_deref()));
    println!(
        "  api_key_id: {}",
        display_id(inventory.log_api_key_id.as_deref())
    );
    println!(
        "  kid:        {}",
        display_id(inventory.issuer_kid.as_deref())
    );
    println!();
    for target in &plan.targets {
        let id = spec::identifier(target.kind, tenant, inventory);
        let id = match id {
            Some(id) => format!(" ({id})"),
            None => String::new(),
        };
        match &target.decision {
            TargetDecision::Absent => {
                println!("  {:<22} absent", format!("{}{id}", target.kind.label()));
            }
            TargetDecision::Refused { reason } => {
                println!(
                    "  {:<22} refused — {reason}",
                    format!("{}{id}", target.kind.label())
                );
            }
            TargetDecision::Offered { default_on } => {
                let default = if *default_on { "remove" } else { "keep" };
                println!(
                    "  {:<22} offered (default: {default})",
                    format!("{}{id}", target.kind.label())
                );
            }
        }
    }
    println!();
}

fn plan_json<'a>(
    tenant: &'a crate::config::Tenant,
    inventory: &'a Inventory,
    plan: &'a DeletePlan,
) -> PlanJson<'a> {
    PlanJson {
        tenant: &tenant.name,
        base_url: &tenant.base_url,
        sa_id: tenant.sa_id.as_deref(),
        api_key_id: inventory.log_api_key_id.as_deref(),
        kid: inventory.issuer_kid.as_deref(),
        targets: plan
            .targets
            .iter()
            .map(|target| {
                let (state, reason, default_on) = match &target.decision {
                    TargetDecision::Absent => ("absent", None, None),
                    TargetDecision::Refused { reason } => ("refused", Some(reason.as_str()), None),
                    TargetDecision::Offered { default_on } => ("offered", None, Some(*default_on)),
                };
                TargetJson {
                    kind: target.kind.label(),
                    identifier: spec::identifier(target.kind, tenant, inventory),
                    state,
                    reason,
                    default_on,
                }
            })
            .collect(),
        manual: ManualJson {
            sa_id: plan.manual.sa_id.as_deref(),
            api_key_id: plan.manual.api_key_id.as_deref(),
        },
    }
}

fn print_report(
    plan: &DeletePlan,
    inventory: &Inventory,
    purge: &ResolvedPurge,
    report: &ExecuteReport,
) {
    if let Some(path) = &report.backup_path {
        println!(
            "backup: {} (identifiers only; re-onboard with these values — this is not an undo)",
            path.display()
        );
    }
    for step in &report.steps {
        match &step.status {
            StepStatus::Ok => match step.step {
                Step::Backup => {}
                Step::RemoteIssuer => println!("removed: issuer signing key from default issuer"),
                Step::Vault(kind) | Step::Path(kind) => {
                    println!("removed: {}", kind.label());
                }
                Step::ConfigEntry => println!("removed: tenant entry from .aic/config.toml"),
            },
            StepStatus::Failed(error) => {
                println!("failed:  {}: {error}", step_label(step.step));
            }
            StepStatus::Skipped(why) => {
                println!("skipped: {}: {why}", step_label(step.step));
            }
        }
    }

    for target in &plan.targets {
        if matches!(target.decision, TargetDecision::Offered { .. })
            && !purge.contains(&target.kind)
        {
            println!("kept:    {}", target.kind.label());
        }
    }

    println!();
    println!("Console cleanup — aic cannot delete these; remove them in the AIC admin console:");
    if let Some(id) = &plan.manual.sa_id {
        println!(
            "  service account  {id}  (Identity Cloud admin console; a service-account bearer gets 403)"
        );
    }
    if let Some(id) = &plan.manual.api_key_id {
        println!("  log API key      {id}  (Tenant Settings → Log API Keys)");
    }
    if plan.manual.sa_id.is_none()
        && plan.manual.api_key_id.is_none()
        && report.remote_error.is_none()
    {
        println!("  (nothing — a surviving tenant still holds every remote identity)");
    }
    if let (Some(error), Some(kid)) = (&report.remote_error, inventory.issuer_kid.as_deref()) {
        println!(
            "  issuer kid       {kid}  (unpublish failed: {error}; remove it from the default Trusted JWT Issuer in the console)"
        );
    }
    if report.config_removed {
        match &report.next_context {
            Some(name) => println!("context is now {name}"),
            None => println!("context cleared (no tenants remain)"),
        }
    }
}

fn step_label(step: Step) -> String {
    match step {
        Step::Backup => "backup".into(),
        Step::RemoteIssuer => "issuer signing key (remote)".into(),
        Step::Vault(kind) | Step::Path(kind) => kind.label().into(),
        Step::ConfigEntry => "config entry".into(),
    }
}

fn display_id(value: Option<&str>) -> &str {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value,
        None => "—",
    }
}

fn confirm_target(label: &str, default_on: bool) -> Result<bool> {
    if !prompt_available() {
        return Err(Error::Config(
            "removing a tenant requires confirmation; pass --delete-keys when no terminal is available".into(),
        ));
    }
    let hint = if default_on { "Y/n" } else { "y/N" };
    eprint!("  remove {label}? [{hint}] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    match line.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default_on),
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        other => Err(Error::Config(format!(
            "expected y or n, got {other:?}; nothing was deleted"
        ))),
    }
}

fn confirm_typed_name(name: &str) -> Result<()> {
    if prompting_disabled() {
        return Err(Error::Config(
            "typed-name confirmation disabled by --no-prompt; pass --delete-keys to skip prompts"
                .into(),
        ));
    }
    if !prompt_available() {
        return Err(Error::Config(
            "deleting a tenant requires typing its name; pass --delete-keys when no terminal is available".into(),
        ));
    }
    eprint!("Type {name:?} to delete this tenant entry: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if line.trim() != name {
        return Err(Error::Config(
            "tenant name did not match; nothing was deleted".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Tenant, TenantTheme};
    use crate::offboard::spec::{self, Inventory};

    fn tenant(name: &str, sa_id: &str) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: "https://example.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some(sa_id.into()),
            scopes: Vec::new(),
            provenance: crate::config::Provenance::default(),
        }
    }

    #[test]
    fn delete_keys_selection_is_exactly_the_offered_set() {
        let departing = tenant("UAT", "dup-sa");
        let keep = spec::Survivor {
            name: "uat".into(),
            base_url: departing.base_url.clone(),
            sa_id: Some("other-sa".into()),
            log_api_key_id: Some("shared".into()),
            issuer_kid: None,
        };
        let inventory = Inventory {
            service_account_jwk: true,
            log_api_key_id: Some("shared".into()),
            issuer_kid: Some("kid".into()),
            logs_database: true,
            idm_store: false,
            workspace: true,
            sync_state: true,
            undo_entries: false,
        };
        let plan = spec::plan(&departing, &inventory, &[keep]);
        let purge = plan.resolve_purge(offered_kinds(&plan));
        assert!(purge.contains(&TargetKind::ServiceAccountJwk));
        assert!(purge.contains(&TargetKind::IssuerSigningKey));
        assert!(purge.contains(&TargetKind::LogsDatabase));
        assert!(purge.contains(&TargetKind::Workspace));
        assert!(purge.contains(&TargetKind::SyncState));
        assert!(!purge.contains(&TargetKind::LogApiKey));
        assert!(!purge.contains(&TargetKind::IdmStore));
        assert!(!purge.contains(&TargetKind::UndoLog));
    }
}
