//! `aic saml cert` — parser and command implementations, plus the
//! certificate-update half of `aic saml import`.
//!
//! Every decision is made in [`super::spec`] from documents [`super::ops`]
//! fetched; this module owns the order of the calls and the reporting. The
//! confirmation order is the sibling verbs' (`rotate stage` and `complete`):
//! plan → print → `spec::authorize` (a dry run stops holding no permit) →
//! [`consent`] (`--force`, or a yes at a terminal) → `spec::write_ok` → the
//! production gate → the write. `import`'s `--certs` picks a mode; it is not
//! consent.

use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::cli::force::OperationForce;
use crate::cli::{
    confirm_destructive, ensure_prod_confirmed, print_json, prompt_available, realm_arg,
    tenant_config_for,
};
use crate::config::tenant::Tenant;
use crate::saml::cert::import::{self, ImportMode};
use crate::saml::cert::ops;
use crate::saml::cert::spec::{self, CertPlan, Planned};
use crate::saml::pem;
use crate::saml::spec::Role;
use crate::saml::write::{Decision, consent};
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

/// The three flags every certificate write takes, so a verb passes one
/// value rather than three booleans.
#[derive(Debug, Clone, Copy)]
pub struct WriteFlags {
    /// Print the plan and send nothing.
    pub dry_run: bool,
    /// Confirm a write to a production-themed tenant.
    pub yes: bool,
    /// `--force` (operation): consent without a prompt.
    pub force: bool,
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
            let flags = WriteFlags {
                dry_run,
                yes,
                force: force.operation(),
            };
            add(tenant, realm, &entity_id, &cert_file, role, flags).await
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
        } => {
            let flags = WriteFlags {
                dry_run,
                yes,
                force: force.operation(),
            };
            remove(tenant, realm, &entity_id, &cert, role, flags).await
        }
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

async fn add(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    cert_file: &Path,
    role: Option<Role>,
    flags: WriteFlags,
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
    let question = format!(
        "Add signing certificate {} to {}? Its peers' signatures will be verified against it \
         from now.",
        cert.sha256, plan.entity_id
    );
    execute(&tenant, &plan, flags, &question).await
}

async fn remove(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    selector: &str,
    role: Option<Role>,
    flags: WriteFlags,
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
    let removing = plan
        .changes
        .iter()
        .flat_map(|change| &change.removed)
        .map(|cert| cert.sha256.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let question = format!(
        "Remove signing certificate {removing} from {}? Assertions signed with it will stop \
         verifying.",
        plan.entity_id
    );
    execute(&tenant, &plan, flags, &question).await
}

/// The shared tail of every certificate write, in the sibling verbs' order:
/// print the plan, `spec::authorize` (a dry run stops holding no permit),
/// [`consent`] — `--force`, or a yes at a terminal — then `spec::write_ok`,
/// which refuses a headless run without `--force` by naming what would
/// change, then the production gate, then the write.
async fn execute(
    tenant: &Tenant,
    plan: &CertPlan,
    flags: WriteFlags,
    question: &str,
) -> Result<()> {
    for line in spec::plan_lines(plan) {
        eprintln!("{line}");
    }
    let permit = match spec::authorize(flags.dry_run, plan) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    let confirmed = consent(flags.force, prompt_available(), || {
        confirm_destructive(
            "changing a SAML entity's signing certificates",
            question,
            "--force",
        )
    })?;
    spec::write_ok(confirmed, plan)?;
    let ok = ensure_prod_confirmed(&tenant.name, flags.yes)?;
    ops::apply(tenant, plan, ok.confirmed_prod, &permit)
        .await
        .map_err(|failure| Error::Config(spec::failure_message(&failure, plan)))?;
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
/// applied.
///
/// `--certs` chooses the **mode** and nothing else. Without it a terminal
/// offers add / replace / cancel, a dry run prints both plans, and a headless
/// run is refused. Consent is then exactly `cert add|remove`'s: a prompt at a
/// terminal, `--force` without one — so `--certs replace` at a terminal still
/// shows its summary and asks.
pub async fn import_update(
    tenant_name: &str,
    realm: &str,
    entity_id: &str,
    file: &[u8],
    certs: Option<ImportMode>,
    flags: WriteFlags,
) -> Result<()> {
    let tenant = tenant_config_for(Some(tenant_name.to_string()))?;
    let target = ops::read_target(&tenant.name, realm, entity_id).await?;
    let export = ops::read_export(&tenant, realm, entity_id).await?;
    let comparison = import::compare_import(&target, &export, file)?;
    eprintln!(
        "update      {entity_id} exists as a remote entity in realm {realm}, so this import \
         is a certificate update"
    );
    for line in comparison.differences.lines() {
        eprintln!("{line}");
    }

    let mode = match certs {
        Some(mode) => mode,
        None if flags.dry_run => {
            for mode in [ImportMode::Add, ImportMode::Replace] {
                eprintln!("with --certs {}:", mode.as_str());
                match import::plan_import(&comparison, mode) {
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
        None if prompt_available() => choose_mode()?,
        None => {
            return Err(Error::Config(format!(
                "{entity_id} already exists as a remote entity in realm {realm}, so this import \
                 would update its certificates; with no terminal to ask, pass --certs add \
                 (merge the file's signing certificates in) or --certs replace (make them \
                 exactly the file's), and --force to confirm"
            )));
        }
    };

    let plan = match import::plan_import(&comparison, mode)? {
        Planned::NoChange(sentence) => {
            println!("{sentence}");
            return Ok(());
        }
        Planned::Change(plan) => plan,
    };
    let question = format!(
        "Apply these certificate changes to {} ({})?",
        plan.entity_id,
        mode.as_str()
    );
    execute(&tenant, &plan, flags, &question).await
}

/// add / replace / cancel, at a terminal. Cancelling is an error, so it
/// exits non-zero the way a declined confirmation does: nothing was applied,
/// and a zero would say the import happened.
fn choose_mode() -> Result<ImportMode> {
    use inquire::{Select, error::InquireError};

    let cancelled = || Error::Config("cancelled: nothing was sent".into());
    let options = vec![
        "add — merge the file's signing certificates into the entity's",
        "replace — make each role's signing certificates exactly the file's",
        "cancel — send nothing",
    ];
    match Select::new("This entity exists. Update its certificates how?", options).raw_prompt() {
        Ok(answer) => match answer.index {
            0 => Ok(ImportMode::Add),
            1 => Ok(ImportMode::Replace),
            _ => Err(cancelled()),
        },
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            Err(cancelled())
        }
        Err(InquireError::NotTTY) => Err(Error::Config(
            "no terminal to ask on; pass --certs add or --certs replace, and --force".into(),
        )),
        Err(error) => Err(Error::Config(format!("certificate mode prompt: {error}"))),
    }
}
