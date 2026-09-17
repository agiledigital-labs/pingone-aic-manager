//! `aic saml rotate` — parser and command implementations.
//!
//! Four verbs, one shared read. Everything each verb refuses is decided in
//! [`super::spec`] from documents this module fetched; what lives here is the
//! order of the calls and the reporting.
//!
//! Two shapes are worth knowing before changing anything:
//!
//! - **`--dry-run` stops by holding no permit**, not by returning in front of
//!   the write. Every tenant write on this path goes through a
//!   [`super::ops`] wrapper that requires a permission token, and the only
//!   things that mint one are `spec::authorize_*`. A preview that fell through
//!   to a write would not compile. (The proof is about *routing* through this
//!   feature, the way `scripts::gate`'s `WritePermit` is: `esv::api` and
//!   `secretmap::api` still have their own callers elsewhere.)
//! - **Nothing reports a post-state it did not read.** A stage is reported
//!   from the tenant's own re-export, never from the version object the
//!   create returned, and a `PUT` of the entity is compared whole against a
//!   fresh read.

use std::path::PathBuf;

use clap::Subcommand;

use crate::cli::force::OperationForce;
use crate::cli::{
    confirm_destructive, ensure_prod_confirmed, print_json, prompt_available, realm_arg,
    tenant_config_for,
};
use crate::saml::rotate::journal::{self, Key, StagedRecord};
use crate::saml::rotate::ops::{self, Settlement};
use crate::saml::rotate::pem::{self, KeyPair};
use crate::saml::rotate::spec::{self, CompletePlan, Decision, RotationState};
use crate::saml::spec::{Location, Role};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum RotateCommand {
    /// Report where this role's signing-key rotation currently stands.
    ///
    /// Reads the entity, the secret mapping, the ESV secret and its versions,
    /// and the published metadata, and says what they mean together — plus
    /// what they cannot say.
    Status {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// Required when the entity holds both roles.
        #[arg(long, value_enum)]
        role: Option<Role>,
        /// Skip the lookup and read this collection directly.
        #[arg(long, value_enum)]
        location: Option<Location>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// One-time setup: give the role its own signing label and back it with an
    /// ESV secret.
    ///
    /// The only path that writes the SAML entity, and it writes it as a full
    /// replace of a document it has just read. Each of its three steps is
    /// skipped when the tenant already shows it done, so an interrupted run
    /// resumes by re-running.
    Init {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// The label namespace to mint, e.g. `spa`. Letters and digits only.
        #[arg(long)]
        identifier: String,
        /// The ESV secret to create or adopt, e.g. `esv-saml-sp-a-signing`.
        #[arg(long)]
        secret_id: String,
        /// `cat key.pem cert.pem` — the private key and its certificate.
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,
        /// Read the key pair from stdin instead.
        #[arg(long)]
        key_stdin: bool,
        /// Description for the created ESV secret.
        #[arg(long)]
        description: Option<String>,
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long, value_enum)]
        location: Option<Location>,
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
    },
    /// Publish a second certificate: add an ESV secret version.
    ///
    /// Preserves the identifier and its mapping — a certificate rotation is
    /// not entity repointing, which orphans the old mapping and rotates
    /// nothing until a new one is made.
    Stage {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// `cat key.pem cert.pem` — the **new** private key and certificate.
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,
        /// Read the key pair from stdin instead.
        #[arg(long)]
        key_stdin: bool,
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long, value_enum)]
        location: Option<Location>,
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
    },
    /// Close the window: disable the old ESV secret version.
    ///
    /// Run it once the peer holds the two-certificate metadata. Disabling is
    /// reversible (`aic esv secret enable`); destroying the version is not,
    /// and is deliberately a separate, explicit command.
    Complete {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// The SHA-256 of the certificate to keep. Required when this install
        /// has no record of staging the rollover.
        #[arg(long, value_name = "SHA256")]
        retain: Option<String>,
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long, value_enum)]
        location: Option<Location>,
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

pub async fn run(command: RotateCommand) -> Result<()> {
    match command {
        RotateCommand::Status {
            entity_id,
            role,
            location,
            realm,
            tenant,
            json,
        } => status(tenant, realm, &entity_id, location, role, json).await,
        RotateCommand::Init {
            entity_id,
            identifier,
            secret_id,
            key_file,
            key_stdin,
            description,
            role,
            location,
            realm,
            tenant,
            dry_run,
            yes,
        } => {
            init(
                tenant,
                realm,
                &entity_id,
                location,
                role,
                &identifier,
                &secret_id,
                read_key_pair(key_file.as_deref(), key_stdin)?,
                description.as_deref(),
                dry_run,
                yes,
            )
            .await
        }
        RotateCommand::Stage {
            entity_id,
            key_file,
            key_stdin,
            role,
            location,
            realm,
            tenant,
            dry_run,
            yes,
        } => {
            let key = read_key_pair(key_file.as_deref(), key_stdin)?.ok_or_else(|| {
                Error::Config(
                    "--key-file (or --key-stdin) is required: staging a rollover means adding an \
                     ESV secret version, and the version's value is the new private key and \
                     certificate concatenated"
                        .into(),
                )
            })?;
            stage(tenant, realm, &entity_id, location, role, key, dry_run, yes).await
        }
        RotateCommand::Complete {
            entity_id,
            retain,
            role,
            location,
            realm,
            tenant,
            dry_run,
            yes,
            force,
        } => {
            complete(
                tenant,
                realm,
                &entity_id,
                location,
                role,
                retain.as_deref(),
                dry_run,
                yes,
                force,
            )
            .await
        }
    }
}

/// Every verb here reads the tenant, so every verb needs an unlocked agent —
/// even `status`, which writes nothing. The metadata export inside it is
/// unauthenticated, but the entity, mapping and ESV secret reads are not.
///
/// **Exhaustive, not a wildcard**, matching [`crate::saml::cli::needs_tenant_auth`]:
/// the answer is uniform today because all four verbs correlate authenticated
/// reads, and a verb that stopped doing so — a purely offline plan printer,
/// say — should have to say so here rather than inherit an unlock it does not
/// need.
pub fn needs_tenant_auth(command: &RotateCommand) -> bool {
    match command {
        RotateCommand::Status { .. }
        | RotateCommand::Init { .. }
        | RotateCommand::Stage { .. }
        | RotateCommand::Complete { .. } => true,
    }
}

async fn status(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    role: Option<Role>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let state = ops::read_state(&tenant, &realm, entity_id, location, role).await?;
    let phase = spec::phase(&state);
    if json {
        return print_json(&spec::status_json(&state, &phase));
    }
    for line in spec::status_lines(&state, &phase) {
        println!("{line}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn init(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    role: Option<Role>,
    identifier: &str,
    secret_id: &str,
    key: Option<KeyPair>,
    description: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    // A mapping `PUT` is the static-content edit `aic secretmap` restricts to
    // lower environments, and `init` performs one. `stage` and `complete` do
    // not, so they are not gated: a production signing key still has to be
    // rotatable.
    if !tenant.allows_secret_mappings() {
        return Err(Error::Config(format!(
            "`rotate init` maps a secret label, and secret mappings are only edited on \
             sandbox/development tenants (this one is '{}') — they are static content promoted \
             up from lower environments. `rotate stage` and `rotate complete` do not touch a \
             mapping and work anywhere.",
            tenant.theme.label()
        )));
    }
    let realm = realm_arg("saml", realm_arg_value)?;
    let state = ops::read_state(&tenant, &realm, entity_id, location, role).await?;

    // Two reads `read_state` cannot make for us: it follows the identifier the
    // entity has now, and `init` is about the one it is being given.
    let label = spec::signing_label(spec::validate_identifier(identifier)?);
    let label_mapping = ops::mapping_alias(&tenant.name, &realm, &label).await?;
    let existing_secret = ops::secret_facts(&tenant.name, secret_id).await?;

    let plan = spec::plan_init(
        &state,
        identifier,
        secret_id,
        label_mapping.as_deref(),
        existing_secret.as_ref(),
        key.as_ref(),
    )?;
    for line in plan.lines(&state) {
        eprintln!("{line}");
    }
    if plan.is_noop() {
        println!(
            "{} ({}) is already set up: {} maps to {}",
            state.entity_id,
            spec::role_descriptor(state.role),
            plan.label,
            plan.secret_id
        );
        println!("`aic saml rotate status {entity_id} --realm {realm}` reads it back");
        return Ok(());
    }

    let permit = match spec::authorize_init(dry_run) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    let ok = ensure_prod_confirmed(&tenant.name, yes)?;

    if plan.set_identifier {
        ops::set_identifier(&state, &plan.identifier, ok.confirmed_prod, &permit).await?;
        println!(
            "entity {entity_id} now points at secretIdIdentifier {:?} (read back and compared \
             whole)",
            plan.identifier
        );
    }
    if plan.create_secret {
        let key = key.as_ref().expect("plan_init requires a key to create");
        ops::create_key_secret(
            &tenant.name,
            &plan.secret_id,
            &key.value,
            &description
                .map(str::to_string)
                .unwrap_or_else(|| default_description(&state)),
            ok.confirmed_prod,
            &permit,
        )
        .await?;
        println!(
            "ESV secret {} created — encoding pem, useInPlaceholders false, certificate {}",
            plan.secret_id, key.sha256
        );
    }
    if plan.map_label {
        ops::map_label(
            &tenant.name,
            &realm,
            &plan.label,
            &plan.secret_id,
            ok.confirmed_prod,
            &permit,
        )
        .await?;
        println!("label {} now maps to {}", plan.label, plan.secret_id);
    }

    // What the tenant publishes now, not what we expect it to.
    let expected = key
        .as_ref()
        .map(|key| std::iter::once(key.sha256.clone()).collect());
    finish(
        &tenant,
        &realm,
        entity_id,
        &state,
        expected.as_ref(),
        |sha| {
            key.as_ref().filter(|key| key.sha256 == *sha).map(|key| {
                (
                    "1".to_string(),
                    key.sha256.clone(),
                    plan.secret_id.clone(),
                    plan.identifier.clone(),
                )
            })
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn stage(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    role: Option<Role>,
    key: KeyPair,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let state = ops::read_state(&tenant, &realm, entity_id, location, role).await?;
    let plan = spec::plan_stage(&state, &key)?;

    eprintln!(
        "will add a version to ESV secret {} holding certificate {}",
        plan.secret_id, plan.incoming
    );
    eprintln!(
        "  {} ({}) will then publish both {} and {}",
        state.entity_id,
        spec::role_descriptor(state.role),
        plan.retained,
        plan.incoming
    );
    eprintln!(
        "  the certificate already published ({}) stays; nothing is retired here",
        plan.retained
    );

    let permit = match spec::authorize_stage(dry_run) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    let ok = ensure_prod_confirmed(&tenant.name, yes)?;

    let created = ops::add_version(
        &tenant.name,
        &plan.secret_id,
        &key.value,
        ok.confirmed_prod,
        &permit,
    )
    .await?;
    let version = created
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::Config(format!(
                "ESV secret {} accepted the new version but its response named no version \
                 number, so this rollover cannot be recorded. `aic esv secret versions {}` \
                 lists what exists; finish with `aic saml rotate complete --retain {}`.",
                plan.secret_id, plan.secret_id, plan.incoming
            ))
        })?
        .to_string();
    println!("ESV secret {} version {version} added", plan.secret_id);

    let secret_id = plan.secret_id.clone();
    let identifier = state.identifier.clone().unwrap_or_default();
    finish(
        &tenant,
        &realm,
        entity_id,
        &state,
        Some(&plan.expected),
        |sha| {
            (*sha == key.sha256).then(|| {
                (
                    version.clone(),
                    key.sha256.clone(),
                    secret_id.clone(),
                    identifier.clone(),
                )
            })
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn complete(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    role: Option<Role>,
    retain: Option<&str>,
    dry_run: bool,
    yes: bool,
    force: OperationForce,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let state = ops::read_state(&tenant, &realm, entity_id, location, role).await?;
    let plan = spec::plan_complete(&state, retain)?;

    for line in complete_plan_lines(&plan, &state) {
        eprintln!("{line}");
    }

    let permit = match spec::authorize_complete(dry_run) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    // The prompt is attempted only where there is a terminal to attempt it
    // on, so the refusal on a headless run comes from `spec::complete_ok` —
    // which names the version, the secret and the consequence — rather than
    // from `confirm_destructive`'s generic "pass --force". `prompt_available`
    // is the right test and `prompting_disabled` is not: `--no-prompt` is one
    // of four reasons a prompt cannot be answered, and a pipe is the common
    // one.
    let confirmed = force.operation()
        || (prompt_available()
            && confirm_destructive(
                "closing a SAML certificate rollover",
                &format!(
                    "Disable version {} of {} and stop publishing the other certificate?",
                    plan.disable_version, plan.secret_id
                ),
                "--force",
            )?);
    spec::complete_ok(confirmed, &plan, &state)?;
    let ok = ensure_prod_confirmed(&tenant.name, yes)?;

    ops::disable_version(
        &tenant.name,
        &plan.secret_id,
        &plan.disable_version,
        ok.confirmed_prod,
        &permit,
    )
    .await?;
    println!(
        "ESV secret {} version {} disabled",
        plan.secret_id, plan.disable_version
    );

    let expected = std::iter::once(plan.retain.clone()).collect();
    let settled = ops::wait_for_export(&tenant, &realm, entity_id, state.role, &expected).await?;
    report_settlement(&settled, &state);
    if matches!(settled, Settlement::Settled) {
        journal::clear(&key_for(&state))?;
        println!(
            "rollover complete: {} ({}) publishes only {}",
            state.entity_id,
            spec::role_descriptor(state.role),
            plan.retain
        );
        println!(
            "version {} is DISABLED, not destroyed — `aic esv secret enable {} {}` puts the old \
             certificate back, and `aic esv secret destroy {} {}` removes it for good.",
            plan.disable_version,
            plan.secret_id,
            plan.disable_version,
            plan.secret_id,
            plan.disable_version
        );
        return Ok(());
    }
    Err(settlement_error(&state, "complete"))
}

fn complete_plan_lines(plan: &CompletePlan, state: &RotationState) -> Vec<String> {
    let mut lines = vec![
        format!(
            "will disable version {} of ESV secret {}",
            plan.disable_version, plan.secret_id
        ),
        format!(
            "  {} ({}) must then publish only {}",
            state.entity_id,
            spec::role_descriptor(state.role),
            plan.retain
        ),
    ];
    lines.push(match plan.expected_drop.as_deref() {
        Some(drop) => format!("  the certificate expected to stop being published is {drop}"),
        None => format!(
            "  which certificate stops being published is not known in advance — this install \
             has no record tying a version to a fingerprint, so the check is that only {} \
             survives",
            plan.retain
        ),
    });
    lines.push("  disabling is reversible; nothing is destroyed here".to_string());
    lines
}

/// Re-read the export, report what the tenant now publishes, and record the
/// version/fingerprint pairing only when that read confirms it.
///
/// `.ai/core.md` §5, applied to a store that will not read a value back: a
/// record taken from what was sent is a claim the tenant accepted it verbatim,
/// and this is the one place that claim can be checked.
async fn finish(
    tenant: &crate::config::Tenant,
    realm: &str,
    entity_id: &str,
    state: &RotationState,
    expected: Option<&std::collections::BTreeSet<String>>,
    attribute: impl Fn(&String) -> Option<(String, String, String, String)>,
) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let settled = ops::wait_for_export(tenant, realm, entity_id, state.role, expected).await?;
    report_settlement(&settled, state);
    if !matches!(settled, Settlement::Settled) {
        return Err(settlement_error(state, "confirm"));
    }
    for sha in expected {
        if let Some((version, sha256, secret_id, identifier)) = attribute(sha) {
            journal::record(StagedRecord {
                tenant: tenant.name.clone(),
                realm: realm.to_string(),
                entity_id: entity_id.to_string(),
                role: state.role.wire().to_string(),
                identifier,
                secret_id,
                version,
                sha256,
                staged_at: chrono::Utc::now().to_rfc3339(),
            })?;
        }
    }
    println!(
        "{} ({}) now publishes {}",
        state.entity_id,
        spec::role_descriptor(state.role),
        expected.iter().cloned().collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

fn report_settlement(settled: &Settlement, state: &RotationState) {
    if let Settlement::TimedOut {
        missing,
        unexpected,
    } = settled
    {
        eprintln!(
            "the write landed, but {} ({}) has not published what was expected yet",
            state.entity_id,
            spec::role_descriptor(state.role)
        );
        for sha in missing {
            eprintln!("  not published: {sha}");
        }
        for sha in unexpected {
            eprintln!("  still published: {sha}");
        }
    }
}

fn settlement_error(state: &RotationState, verb: &str) -> Error {
    Error::Config(format!(
        "could not {verb} the change against {}'s published metadata. The tenant write \
         succeeded, so **nothing has been undone and nothing should be re-sent**: re-run \
         `aic saml rotate status {} --realm {}` in a minute. Every change measured on a live \
         tenant appeared within single-digit seconds, so a longer wait than this usually means \
         something else resolves this label.",
        state.tenant, state.entity_id, state.realm
    ))
}

fn key_for(state: &RotationState) -> Key {
    Key {
        tenant: state.tenant.clone(),
        realm: state.realm.clone(),
        entity_id: state.entity_id.clone(),
        role: state.role.wire().to_string(),
    }
}

fn default_description(state: &RotationState) -> String {
    format!(
        "SAML {} signing key for {} — managed by `aic saml rotate`",
        spec::role_descriptor(state.role),
        state.entity_id
    )
}

/// Read and validate the key pair, before any tenant is resolved.
///
/// A key pair that is not one is not a rotation, so this refuses before
/// `tenant_config_for`, before the realm is worked out and before a single
/// request is sent. It does **not** run before the unlock: the root pre-flight
/// classifies `rotate` through [`needs_tenant_auth`] and starts the agent
/// first, so on a locked daemon a malformed `--key-file` is reported after the
/// lock is, not instead of it. Measured, not assumed — an earlier draft of this
/// comment claimed a daemon was not needed and it is.
///
/// There is no interactive prompt: the value is a multi-line PEM document, and
/// `--key-stdin` is the pipeline form.
fn read_key_pair(key_file: Option<&std::path::Path>, key_stdin: bool) -> Result<Option<KeyPair>> {
    let bytes = match (key_file, key_stdin) {
        (Some(_), true) => {
            return Err(Error::Config(
                "provide only one of --key-file / --key-stdin".into(),
            ));
        }
        (Some(path), false) => std::fs::read(path).map_err(|error| {
            Error::Config(format!("read --key-file {}: {error}", path.display()))
        })?,
        (None, true) => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|error| Error::Config(format!("read the key pair from stdin: {error}")))?;
            bytes
        }
        (None, false) => return Ok(None),
    };
    Ok(Some(pem::validate_key_pair(&bytes)?))
}
