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
use crate::saml::rotate::spec::{self, CompletePlan, Decision, RotationState, VersionChoice};
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
        /// The ESV secret version to disable. Required alongside `--retain`
        /// when this install has no record of the stage: a fingerprint does
        /// not name a version, and nothing readable pairs the two. When there
        /// **is** a record, it decides and this may only agree with it.
        #[arg(long, value_name = "N")]
        disable_version: Option<String>,
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
            disable_version,
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
                disable_version.as_deref(),
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
    // Surveyed here and not only in the write verbs: `status` is where every
    // sharing refusal sends the operator, and a report that could not say who
    // else resolves the secret would send them nowhere. A role with no
    // identifier resolves no label of its own, so there is nothing to survey
    // and the reads are not made.
    let consumers = match state.consumer() {
        Some(mine) => Some(
            ops::read_consumers(&tenant.name, &realm, &mine, state.mapped_alias.as_deref()).await?,
        ),
        None => None,
    };
    if json {
        return print_json(&spec::status_json(&state, &phase, consumers.as_ref()));
    }
    for line in spec::status_lines(&state, &phase, consumers.as_ref()) {
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
    // Before the plan is printed, because a plan for a rollover that cannot
    // happen is a plan to act on. `init` is where sharing is *created* — an
    // identifier a second entity already names, or a secret a second label
    // already maps to — so this is the cheapest place to refuse it.
    let exclusive = spec::exclusive_ok(
        "init",
        &ops::read_consumers(
            &tenant.name,
            &realm,
            &spec::Consumer {
                entity_id: state.entity_id.clone(),
                location: state.location,
                role: state.role,
                label: plan.label.clone(),
            },
            Some(&plan.secret_id),
        )
        .await?,
    )?;

    for line in plan.lines(&state) {
        eprintln!("{line}");
    }
    if plan.is_noop() {
        // The same evidence as the adoption path below, and for the same
        // reason: "every step is already done" is a statement about three
        // documents, and a label backed by a secret with no ENABLED version
        // satisfies all three while publishing nothing at all. Read first and
        // claim afterwards, so the claim never precedes what supports it.
        let published = ops::wait_for_any_cert(&tenant, &realm, entity_id, state.role).await?;
        let lines = spec::adoption_outcome(
            &state,
            &plan.secret_id,
            &published,
            key.as_ref().map(|key| key.sha256.as_str()),
            spec::Adoption::AlreadyDone,
        )?;
        println!(
            "{} ({}) is already set up: {} maps to {}",
            state.entity_id,
            spec::role_descriptor(state.role),
            plan.label,
            plan.secret_id
        );
        for line in lines {
            println!("{line}");
        }
        println!("`aic saml rotate status {entity_id} --realm {realm}` reads it back");
        return Ok(());
    }

    let permit = match spec::authorize_init(dry_run, &exclusive) {
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
            label_mapping.as_deref(),
            ok.confirmed_prod,
            &permit,
        )
        .await?;
        println!("label {} now maps to {}", plan.label, plan.secret_id);
    }

    // What the tenant publishes now, not what we expect it to — and the two
    // branches differ in what "expect" can even mean. A key pair this run
    // wrote gives a fingerprint to wait for; a secret this run only adopted
    // does not, because its value was never seen from here.
    if plan.create_secret {
        let key = key
            .as_ref()
            .expect("plan_init requires a key to create the secret");
        let expected = std::iter::once(key.sha256.clone()).collect();
        // No journal entry, and that is the point: `init` stages nothing. The
        // record answers "which ESV secret version holds which certificate"
        // during a **two-certificate window**, and there is one certificate
        // here. Writing one anyway — version "1", from a run that never
        // staged — handed a later `complete` a pairing describing the setup
        // rather than a rollover, and with a higher DISABLED spare version
        // AIC's latest-version refusal no longer catches the result.
        return confirm_publication(&tenant, &realm, entity_id, &state, &expected).await;
    }

    let published = ops::wait_for_any_cert(&tenant, &realm, entity_id, state.role).await?;
    for line in spec::adoption_outcome(
        &state,
        &plan.secret_id,
        &published,
        key.as_ref().map(|key| key.sha256.as_str()),
        spec::Adoption::Applied,
    )? {
        println!("{line}");
    }
    Ok(())
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
    let exclusive = spec::exclusive_ok(
        "stage",
        &ops::read_consumers(
            &tenant.name,
            &realm,
            &rotating(&state),
            Some(&plan.secret_id),
        )
        .await?,
    )?;

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

    let permit = match spec::authorize_stage(dry_run, &exclusive) {
        Decision::Preview => {
            eprintln!("dry run: nothing was sent");
            return Ok(());
        }
        Decision::Send(permit) => permit,
    };
    let ok = ensure_prod_confirmed(&tenant.name, yes)?;

    let created = ops::add_version(
        &tenant,
        &state,
        &plan,
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
                 number, so this rollover cannot be recorded, and the record is the only \
                 thing that ties a certificate to a version. \
                 `aic esv secret versions {}` lists what exists, including the one that was \
                 just added; finish with `aic saml rotate complete --retain {} \
                 --disable-version <n>`, naming the version holding the certificate being \
                 retired. Both halves are needed: {} names a certificate, not a version.",
                plan.secret_id, plan.secret_id, plan.incoming, plan.incoming
            ))
        })?
        .to_string();
    println!("ESV secret {} version {version} added", plan.secret_id);

    // The one ENABLED version before this one, which `plan_stage` proved
    // there was exactly one of. Named here because it is what the recovery
    // instruction below needs and what nothing will be able to work out once
    // this process is gone.
    let previous = state
        .enabled_versions()
        .first()
        .map(|version| version.version.clone())
        .unwrap_or_default();

    confirm_publication(&tenant, &realm, entity_id, &state, &plan.expected).await?;
    journal::record(
        StagedRecord {
            tenant: tenant.name.clone(),
            realm: realm.clone(),
            entity_id: entity_id.to_string(),
            role: state.role.wire().to_string(),
            identifier: state.identifier.clone().unwrap_or_default(),
            secret_id: plan.secret_id.clone(),
            version: version.clone(),
            sha256: key.sha256.clone(),
            staged_at: chrono::Utc::now().to_rfc3339(),
        },
        &permit,
    )
    .map_err(|error| {
        Error::Config(format!(
            "the rollover is staged and the tenant confirmed it — ESV secret {} version \
             {version} holds certificate {}, and {} now publishes both — but this install \
             could not write that down: {error}. **Nothing needs re-sending.** The record is \
             the only thing that ties a certificate to a version, so close the window by \
             naming both halves yourself: `aic saml rotate complete {} --realm {realm} \
             --retain {} --disable-version {previous}`.",
            plan.secret_id, key.sha256, state.entity_id, entity_id, key.sha256,
        ))
    })
}

#[allow(clippy::too_many_arguments)]
async fn complete(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    role: Option<Role>,
    retain: Option<&str>,
    disable_version: Option<&str>,
    dry_run: bool,
    yes: bool,
    force: OperationForce,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let state = ops::read_state(&tenant, &realm, entity_id, location, role).await?;
    let plan = spec::plan_complete(&state, retain, disable_version)?;
    let exclusive = spec::exclusive_ok(
        "complete",
        &ops::read_consumers(
            &tenant.name,
            &realm,
            &rotating(&state),
            Some(&plan.secret_id),
        )
        .await?,
    )?;

    for line in complete_plan_lines(&plan, &state) {
        eprintln!("{line}");
    }

    let permit = match spec::authorize_complete(dry_run, &exclusive) {
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

    ops::disable_version(&tenant, &state, &plan, ok.confirmed_prod, &permit).await?;
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

/// The rollover's own entry in the consumer survey.
///
/// `stage` and `complete` plan only from `Settled` or `Staged`, and both
/// phases require the role to name an identifier — the survey is of the label
/// that identifier mints.
fn rotating(state: &RotationState) -> spec::Consumer {
    state
        .consumer()
        .expect("a planned rollover means the role names a secretIdIdentifier")
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
    // Whose claim the version is. Disabling a version retires whatever
    // certificate it holds, so where that pairing came from is the line an
    // operator has to read before confirming.
    lines.push(match plan.chosen_by {
        VersionChoice::Recorded => format!(
            "  version {} follows from this install's record of the stage, which is what ties a \
             certificate to a version",
            plan.disable_version
        ),
        VersionChoice::Named => format!(
            "  version {} was named with --disable-version; nothing readable pairs a version \
             with a certificate, so which one it holds is your claim, not this command's",
            plan.disable_version
        ),
    });
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

/// Re-read the export and report what the tenant now publishes, refusing to
/// claim a post-state it has not shown.
///
/// `.ai/core.md` §5, applied to a store that will not read a value back.
///
/// `expected` is not optional, and used not to be enforced: an `init` with no
/// fingerprint to wait for fell through here and returned success having read
/// nothing. A caller that cannot name a fingerprint has a different, weaker
/// claim to make and makes it through [`spec::adoption_outcome`].
///
/// It does **not** write the journal, and that is a boundary rather than a
/// tidy-up. `stage` records the version/fingerprint pairing here because this
/// is where the tenant's own export confirms it; `init` has no pairing to
/// record, because it opens no two-certificate window. A shared callback that
/// wrote "the same record, conditionally" is how the one that stages nothing
/// came to write one.
async fn confirm_publication(
    tenant: &crate::config::Tenant,
    realm: &str,
    entity_id: &str,
    state: &RotationState,
    expected: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let settled = ops::wait_for_export(tenant, realm, entity_id, state.role, expected).await?;
    report_settlement(&settled, state);
    if !matches!(settled, Settlement::Settled) {
        return Err(settlement_error(state, "confirm"));
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
