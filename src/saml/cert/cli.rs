//! `aic saml cert` — parser and command implementations, plus the
//! certificate-update half of `aic saml import`.
//!
//! Every decision is made in [`super::spec`] from documents [`super::ops`]
//! fetched; this module owns the order of the calls and the reporting. The
//! confirmation order is the sibling verbs' (`rotate stage` and `complete`):
//! plan → print → `spec::authorize` (a dry run stops holding no permit) →
//! the consent flag or `prompt_available() && confirm_destructive(..)` →
//! `spec::write_ok` → the production gate → the write.

use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::cli::force::OperationForce;
use crate::cli::{
    confirm_destructive, ensure_prod_confirmed, print_json, prompt_available, realm_arg,
    tenant_config_for,
};
use crate::config::tenant::Tenant;
use crate::saml::cert::ops;
use crate::saml::cert::spec::{self, CertPlan, ImportMode, Planned};
use crate::saml::rotate::pem;
use crate::saml::rotate::spec::Decision;
use crate::saml::spec::Role;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum CertCommand {
    /// List the certificates a SAML entity publishes, with their expiry.
    ///
    /// Reads the metadata export, which takes no authentication, so this
    /// works with the agent locked. Hosted entities list too.
    List {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// Only this role's certificates.
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add a signing certificate to a remote entity.
    ///
    /// The document sent is the entity's own export with one signing
    /// KeyDescriptor spliced in; encryption keys are sent back untouched. A
    /// certificate the role already publishes for signing is a no-op.
    Add {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// The certificate: one PEM `CERTIFICATE` block, or DER.
        #[arg(long)]
        cert_file: PathBuf,
        /// Required when the entity holds both roles.
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Print the plan and send nothing.
        #[arg(long)]
        dry_run: bool,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        force: OperationForce,
    },
    /// Remove a signing certificate from a remote entity.
    ///
    /// Refuses, with no override, to remove a role's last signing
    /// certificate.
    Remove {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// The certificate's SHA-256 fingerprint, or an unambiguous prefix.
        cert: String,
        /// Required when the certificate signs for both roles.
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Print the plan and send nothing.
        #[arg(long)]
        dry_run: bool,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        force: OperationForce,
    },
}

/// Exhaustive, like `saml::cli::needs_tenant_auth`: `list` reads only the
/// unauthenticated export; `add` and `remove` list the realm over the bearer
/// and write.
pub fn needs_tenant_auth(command: &CertCommand) -> bool {
    match command {
        CertCommand::List { .. } => false,
        CertCommand::Add { .. } | CertCommand::Remove { .. } => true,
    }
}

pub async fn run(command: CertCommand) -> Result<()> {
    match command {
        CertCommand::List {
            entity_id,
            role,
            realm,
            tenant,
            json,
        } => list(tenant, realm, &entity_id, role, json).await,
        CertCommand::Add {
            entity_id,
            cert_file,
            role,
            realm,
            tenant,
            dry_run,
            yes,
            force,
        } => {
            add(
                tenant, realm, &entity_id, &cert_file, role, dry_run, yes, force,
            )
            .await
        }
        CertCommand::Remove {
            entity_id,
            cert,
            role,
            realm,
            tenant,
            dry_run,
            yes,
            force,
        } => remove(tenant, realm, &entity_id, &cert, role, dry_run, yes, force).await,
    }
}

async fn list(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    role: Option<Role>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let export = ops::read_export(&tenant, &realm, entity_id).await?;
    let rows = spec::cert_rows(&export, role)?;
    let now = chrono::Utc::now();
    if json {
        print_json(&spec::list_json(&rows, now))
    } else {
        for line in spec::list_lines(entity_id, &rows, now) {
            println!("{line}");
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn add(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    cert_file: &Path,
    role: Option<Role>,
    dry_run: bool,
    yes: bool,
    force: OperationForce,
) -> Result<()> {
    // The file first: a certificate we cannot read is not a plan, and that
    // should not depend on a context or a network call.
    let bytes = std::fs::read(cert_file).map_err(|error| {
        Error::Config(format!("read certificate {}: {error}", cert_file.display()))
    })?;
    let cert = pem::read_certificate(&bytes)?;
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let target = ops::read_target(&tenant.name, &realm, entity_id).await?;
    let export = ops::read_export(&tenant, &realm, entity_id).await?;
    let plan = match spec::plan_add(&target, &export, &cert.der, &cert.sha256, role)? {
        Planned::NoChange(sentence) => {
            println!("{sentence}");
            return Ok(());
        }
        Planned::Change(plan) => plan,
    };
    execute(&tenant, &plan, dry_run, yes, "--force", |plan| {
        Ok(force.operation()
            || (prompt_available()
                && confirm_destructive(
                    "changing a SAML entity's signing certificates",
                    &format!(
                        "Add signing certificate {} to {}? Its peers' signatures will be \
                         verified against it from now.",
                        cert.sha256, plan.entity_id
                    ),
                    "--force",
                )?))
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn remove(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    selector: &str,
    role: Option<Role>,
    dry_run: bool,
    yes: bool,
    force: OperationForce,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let target = ops::read_target(&tenant.name, &realm, entity_id).await?;
    let export = ops::read_export(&tenant, &realm, entity_id).await?;
    let plan = match spec::plan_remove(&target, &export, selector, role)? {
        Planned::NoChange(sentence) => {
            println!("{sentence}");
            return Ok(());
        }
        Planned::Change(plan) => plan,
    };
    execute(&tenant, &plan, dry_run, yes, "--force", |plan| {
        let removing = plan
            .changes
            .iter()
            .flat_map(|change| &change.removed)
            .map(|cert| cert.sha256.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Ok(force.operation()
            || (prompt_available()
                && confirm_destructive(
                    "changing a SAML entity's signing certificates",
                    &format!(
                        "Remove signing certificate {removing} from {}? Assertions signed with \
                         it will stop verifying.",
                        plan.entity_id
                    ),
                    "--force",
                )?))
    })
    .await
}

/// The shared tail of every certificate write, in the sibling verbs' order.
async fn execute(
    tenant: &Tenant,
    plan: &CertPlan,
    dry_run: bool,
    yes: bool,
    flag: &str,
    consent: impl FnOnce(&CertPlan) -> Result<bool>,
) -> Result<()> {
    for line in spec::plan_lines(plan) {
        eprintln!("{line}");
    }
    let permit = match spec::authorize(dry_run, plan) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    let confirmed = consent(plan)?;
    spec::write_ok(confirmed, plan, flag)?;
    let ok = ensure_prod_confirmed(&tenant.name, yes)?;
    ops::apply(tenant, plan, ok.confirmed_prod, &permit)
        .await
        .map_err(|failure| {
            Error::Config(spec::failure_message(failure.status, &failure.source, plan))
        })?;
    for line in spec::outcome_lines(plan) {
        println!("{line}");
    }
    Ok(())
}

/// `aic saml import` over one existing remote entity: a certificate update.
///
/// `file` is the document the import would otherwise have sent. None of its
/// bytes are sent — the plan is the entity's export with the file's signing
/// certificates spliced in, and every other difference is reported as not
/// applied. `certs` is the consent flag, the way `--force` is for `add`;
/// without it a terminal offers add / replace / cancel, and a headless run
/// is refused.
pub async fn import_update(
    tenant_name: &str,
    realm: &str,
    entity_id: &str,
    file: &[u8],
    dry_run: bool,
    certs: Option<ImportMode>,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_config_for(Some(tenant_name.to_string()))?;
    let target = ops::read_target(&tenant.name, realm, entity_id).await?;
    let export = ops::read_export(&tenant, realm, entity_id).await?;
    let comparison = spec::compare_import(&target, &export, file)?;
    eprintln!(
        "update      {entity_id} exists as a remote entity in realm {realm}, so this import \
         is a certificate update"
    );
    for line in comparison.differences.lines() {
        eprintln!("{line}");
    }

    let mode = match certs {
        Some(mode) => mode,
        None if dry_run => {
            for mode in [ImportMode::Add, ImportMode::Replace] {
                eprintln!("with --certs {}:", mode.as_str());
                match spec::plan_import(&comparison, mode) {
                    Ok(Planned::Change(plan)) => {
                        for line in spec::plan_lines(&plan) {
                            eprintln!("  {line}");
                        }
                    }
                    Ok(Planned::NoChange(sentence)) => eprintln!("  {sentence}"),
                    Err(error) => eprintln!("  refused: {error}"),
                }
            }
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        None if prompt_available() => match choose_mode()? {
            Some(mode) => mode,
            None => {
                eprintln!("cancelled: nothing was sent");
                return Ok(());
            }
        },
        None => {
            return Err(Error::Config(format!(
                "{entity_id} already exists as a remote entity in realm {realm}, so this import \
                 would update its certificates; with no terminal to ask, pass --certs add \
                 (merge the file's signing certificates in) or --certs replace (make them \
                 exactly the file's)"
            )));
        }
    };

    let plan = match spec::plan_import(&comparison, mode)? {
        Planned::NoChange(sentence) => {
            println!("{sentence}");
            return Ok(());
        }
        Planned::Change(plan) => plan,
    };
    let flag = format!("--certs {}", mode.as_str());
    execute(&tenant, &plan, dry_run, yes, &flag, |plan| {
        Ok(certs.is_some()
            || (prompt_available()
                && confirm_destructive(
                    "changing a SAML entity's signing certificates",
                    &format!(
                        "Apply these certificate changes to {} ({})?",
                        plan.entity_id,
                        mode.as_str()
                    ),
                    &flag,
                )?))
    })
    .await
}

/// add / replace / cancel, at a terminal.
fn choose_mode() -> Result<Option<ImportMode>> {
    use inquire::{Select, error::InquireError};

    let options = vec![
        "add — merge the file's signing certificates into the entity's",
        "replace — make each role's signing certificates exactly the file's",
        "cancel — send nothing",
    ];
    match Select::new("This entity exists. Update its certificates how?", options).raw_prompt() {
        Ok(answer) => Ok(match answer.index {
            0 => Some(ImportMode::Add),
            1 => Some(ImportMode::Replace),
            _ => None,
        }),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
        Err(InquireError::NotTTY) => Err(Error::Config(
            "no terminal to ask on; pass --certs add or --certs replace".into(),
        )),
        Err(error) => Err(Error::Config(format!("certificate mode prompt: {error}"))),
    }
}
