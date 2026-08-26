//! `aic script` parser and command implementation.

use clap::Subcommand;

use crate::cli::{
    confirm_destructive, print_json, print_table, prod_hint, prompt_available, tenant_for,
};
use crate::config::{self, ProjectConfig, TenantTheme};
use crate::scripts::{self as script, Namespace};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum ScriptCommand {
    /// List scripts on the tenant. Optional <ref> narrows the listing:
    /// a namespace (`bravo`, `endpoint`) or one script (`bravo/Foo`).
    List {
        /// Namespace or full-name to filter by (default: everything).
        reference: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print scripts as JSON")]
        json: bool,
    },
    /// Create a new standalone script and pull its canonical server form.
    Create {
        #[arg(help = "<namespace>/<name>")]
        reference: String,
        #[arg(long, help = "AM scripting context or workspace folder slug")]
        context: Option<String>,
        #[arg(long, value_name = "FILE", help = "Read the source from FILE")]
        from: Option<std::path::PathBuf>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long = "evaluator-version")]
        evaluator_version: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Confirm the write")]
        yes: bool,
    },
    /// Copy a standalone script, including its complete raw config.
    Copy {
        source: String,
        destination: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Confirm the write")]
        yes: bool,
    },
    /// Delete a standalone script, retaining its local source file.
    Delete {
        #[arg(help = "<namespace>/<name>")]
        reference: String,
        #[arg(long, help = "Required: delete the remote script")]
        force: bool,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Confirm the write")]
        yes: bool,
    },
    /// Pull script(s) into the workspace.
    ///
    /// With no <ref>, opens a fuzzy picker (alphabetical; `!` = local changes,
    /// `-` = not pulled). Otherwise <ref> is `<namespace>/<name>` for one (e.g.
    /// `bravo/Foo`, `endpoint/validateQueryFilter`), a bare namespace
    /// (`bravo`, `endpoint`) for all of it, or `all` for everything. A bare
    /// name uses the namespace of your current directory.
    Pull {
        #[arg(help = "<namespace>/<name>, a namespace, `all`, or empty to pick interactively")]
        reference: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Overwrite local edits without backing them up first.
        #[arg(long)]
        force: bool,
    },
    /// Push a local edit back to the tenant (requires a prior pull). With no
    /// <ref>, opens a fuzzy picker (changed scripts marked `!`, listed first).
    /// `all` pushes every synced script. <ref> is `<namespace>/<name>`.
    Push {
        #[arg(help = "<namespace>/<name>, `all`, or empty to pick interactively")]
        reference: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Push past a remote-drift conflict (overwrites remote).
        #[arg(long)]
        force: bool,
        /// Confirm the write.
        #[arg(long)]
        yes: bool,
    },
    /// Show the sync state of synced scripts. Optional <ref> filters by
    /// namespace (`bravo`, `endpoint`).
    Status {
        #[arg(
            help = "Filter: group (am/idm), namespace (alpha/endpoint/…), full-name (alpha/Email OTP), or any fragment (default: all)"
        )]
        reference: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Show who created and last modified a script, and when.
    ///
    /// Resolves AM's principal DNs to names. Only AM scripts record this —
    /// every IDM config object (`endpoint/`, `schedule/`, `managed/`, `sync/`)
    /// carries no authorship at all, and `who` says so rather than guessing.
    /// The fields only ever name the *latest* writer; `--history` lists earlier
    /// ones from the log API.
    Who {
        #[arg(
            help = "<namespace>/<name> (e.g. alpha/Email OTP), or a bare name in a workspace subdir"
        )]
        reference: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(
            long,
            help = "Also list recent successful updates from the log API (30-day server retention)"
        )]
        history: bool,
        #[arg(
            long,
            default_value_t = 60,
            help = "History lookback in minutes (max 1440)"
        )]
        minutes: u32,
        #[arg(long, help = "Print as JSON")]
        json: bool,
    },
    /// Bidirectionally sync the workspace with the tenant: push local-only
    /// changes, pull remote-only changes, and resolve conflicts (both changed).
    /// Scope with an optional <ref>; default reconciles everything synced.
    Sync {
        #[arg(help = "namespace, <namespace>/<name>, `all`, or empty for everything synced")]
        reference: Option<String>,
        /// Auto-resolve every conflict this way (default: prompt; skip if no TTY).
        #[arg(long, value_enum)]
        resolve: Option<Resolution>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm writes.
        #[arg(long)]
        yes: bool,
    },
    /// Watch the workspace and push each `.cjs` you save back to the tenant
    /// (runs until Ctrl-C). Reacts to local saves only — run `sync`/`pull` to
    /// take in remote changes. A save whose remote drifted is reported + skipped.
    Watch {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm writes.
        #[arg(long)]
        yes: bool,
    },
    /// Diff a script (colored, via `git diff`). Default compares your local
    /// copy against the tenant. With no <ref>, opens a fuzzy picker over synced
    /// scripts.
    Diff {
        #[arg(help = "<namespace>/<name>, or empty to pick interactively")]
        reference: Option<String>,
        /// Diff your local file against the last-synced snapshot (your edits only).
        #[arg(long, conflicts_with = "snapshot_vs_remote")]
        local_vs_snapshot: bool,
        /// Diff the last-synced snapshot against the tenant (remote drift).
        #[arg(long)]
        snapshot_vs_remote: bool,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

/// How `sync` resolves a both-changed conflict when `--resolve` is given.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Resolution {
    /// Overwrite the tenant with your local copy.
    Local,
    /// Overwrite your local copy with the tenant's.
    Remote,
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceCommand {
    /// Create the per-tenant workspace tree (both realms + IDM) + type defs.
    Init {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Refresh managed type/config files to the latest bundled version.
    Update {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

pub async fn run(cmd: ScriptCommand) -> Result<()> {
    use crate::scripts::{sync, workspace};

    match cmd {
        ScriptCommand::List {
            reference,
            tenant,
            json,
        } => {
            let t = tenant_for(tenant)?;
            let mut out = Vec::new();
            let mut rows = Vec::new();
            for job in parse_ref(reference)? {
                for sref in job.ns.kind.list(&t, job.ns.realm_arg()).await? {
                    // A specific-name ref filters the listing to that script.
                    if let script::sync::Selector::Name(ref n) = job.selector {
                        if sref.name != *n {
                            continue;
                        }
                    }
                    rows.push(listed_row(&sref, &job.ns));
                    out.push(listed(&sref, &job.ns));
                }
            }
            if json {
                print_json(&out)
            } else {
                print_table(
                    &["REF", "KIND", "CONTEXT", "ENGINE", "DEFAULT", "ID"],
                    &rows,
                );
                Ok(())
            }
        }
        ScriptCommand::Create {
            reference,
            context,
            from,
            language,
            evaluator_version,
            description,
            tenant,
            yes,
        } => {
            let tenant = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&tenant)?;
            require_workspace(&tenant)?;
            let (ns, name) = parse_one(&reference)?;
            require_standalone(ns.kind, &name)?;
            if ns.kind != script::Kind::Am && context.is_some() {
                return Err(Error::Config(
                    "--context is only valid for AM scripts".into(),
                ));
            }
            let source = match from {
                Some(path) => std::fs::read(&path).map_err(|e| {
                    Error::Config(format!("read source file {}: {e}", path.display()))
                })?,
                None => format!("// {name}\n").into_bytes(),
            };
            let mut opts = script::NewScriptOpts {
                context,
                language,
                evaluator_version,
                description,
            };
            if ns.kind == script::Kind::Am {
                let input = opts
                    .context
                    .as_deref()
                    .ok_or_else(|| Error::Config("AM script create requires --context".into()))?;
                let contexts = script::am::list_contexts(&tenant).await?;
                let (resolved, forced_version) = script::am::resolve_context(input, &contexts)?;
                opts.context = Some(resolved);
                if opts.evaluator_version.is_none() {
                    opts.evaluator_version = forced_version;
                }
            }
            let new_script = ns.kind.new_script(&name, &source, &opts)?;
            let created =
                prod_hint(sync::create_new(&tenant, ns.realm_arg(), &new_script, yes).await)?;
            let path = ProjectConfig::workspace_tree(&tenant).join(
                created
                    .reference
                    .kind
                    .workspace_subpath(&created.reference, ns.realm_arg()),
            );
            let full = script::full_name(ns.kind, ns.realm.as_deref(), &name);
            if ns.kind == script::Kind::Am {
                println!(
                    "created {full} ({}, {}) -> {}",
                    created.reference.context.as_deref().unwrap_or("unknown"),
                    created
                        .reference
                        .evaluator_version
                        .as_deref()
                        .unwrap_or("2.0"),
                    path.display()
                );
            } else {
                println!("created {full} -> {}", path.display());
            }
            Ok(())
        }
        ScriptCommand::Copy {
            source,
            destination,
            tenant,
            yes,
        } => {
            let tenant = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&tenant)?;
            require_workspace(&tenant)?;
            let (source_ns, source_name) = parse_one(&source)?;
            let (destination_ns, destination_name) = parse_one(&destination)?;
            require_standalone(source_ns.kind, &source_name)?;
            require_standalone(destination_ns.kind, &destination_name)?;
            validate_copy(&source_ns, &source_name, &destination_ns, &destination_name)?;
            let source_ref = source_ns
                .kind
                .list(&tenant, source_ns.realm_arg())
                .await?
                .into_iter()
                .find(|r| r.name == source_name)
                .ok_or_else(|| Error::Config(format!("no script named {source_name:?}")))?;
            let fetched = source_ns
                .kind
                .fetch(&tenant, source_ns.realm_arg(), &source_ref.id)
                .await?;
            prod_hint(
                sync::copy(
                    &tenant,
                    destination_ns.realm_arg(),
                    &fetched,
                    &destination_name,
                    yes,
                )
                .await,
            )?;
            println!(
                "copied {} -> {}",
                script::full_name(source_ns.kind, source_ns.realm.as_deref(), &source_name),
                script::full_name(
                    destination_ns.kind,
                    destination_ns.realm.as_deref(),
                    &destination_name
                )
            );
            Ok(())
        }
        ScriptCommand::Delete {
            reference,
            force,
            tenant,
            yes,
        } => {
            let tenant = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&tenant)?;
            require_workspace(&tenant)?;
            let (ns, name) = parse_one(&reference)?;
            require_standalone(ns.kind, &name)?;
            let full = script::full_name(ns.kind, ns.realm.as_deref(), &name);
            if !force {
                eprintln!("would delete {full} from {tenant}; pass --force to delete it");
                return Err(Error::Config("script delete requires --force".into()));
            }
            let reference = ns
                .kind
                .list(&tenant, ns.realm_arg())
                .await?
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::Config(format!("no script named {name:?}")))?;
            if ns.kind == script::Kind::Am && reference.is_default {
                return Err(Error::Config(format!(
                    "default script {name:?} cannot be deleted"
                )));
            }
            let path = ProjectConfig::workspace_tree(&tenant)
                .join(ns.kind.workspace_subpath(&reference, ns.realm_arg()));
            prod_hint(sync::delete(&tenant, ns.realm_arg(), ns.kind, &reference, yes).await)?;
            println!("deleted {full}; local file kept at {}", path.display());
            Ok(())
        }
        ScriptCommand::Pull {
            reference,
            tenant,
            force,
        } => {
            let t = tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            // Be friendly: scaffold the workspace on first use so the pulled
            // sources land next to their type definitions.
            if workspace::applied_version(&t)? == 0 {
                let r = workspace::init(&t)?;
                println!("initialised workspace at {}", r.tree.display());
                println!(
                    "next: cd {} && npm install   (installs the lint/type-check toolchain)",
                    r.tree.display()
                );
            }
            // No ref → fuzzy-pick one script; otherwise expand the ref to jobs.
            let jobs = match reference {
                None => match pick(
                    "Pull which script?",
                    sync::pull_candidates(&t).await?,
                    false,
                )? {
                    Some((ns, name)) => vec![Job {
                        ns,
                        selector: sync::Selector::Name(name),
                    }],
                    None => return Ok(()),
                },
                some => parse_ref(some)?,
            };
            let mut any = false;
            for job in jobs {
                // For a single named target, confirm before clobbering local
                // edits. Confirmation only grants permission to proceed — the
                // snapshot-backup still happens (only an explicit `--force`
                // skips it). Bulk pulls don't prompt.
                if let sync::Selector::Name(name) = &job.selector {
                    if !force
                        && sync::local_state(&t, job.ns.kind, job.ns.realm_arg(), name)?
                            == sync::LocalState::Modified
                    {
                        let full = script::full_name(job.ns.kind, job.ns.realm.as_deref(), name);
                        // `Some(false)` = declined; `Some(true)`/`None` (no TTY)
                        // → proceed (the snapshot-backup still happens).
                        if let Some(false) = confirm_overwrite(&format!(
                            "{full} has local changes — overwrite them? (a backup is kept under .aic-sync/backups/)"
                        ))? {
                            println!("{full}: skipped (kept local changes)");
                            continue;
                        }
                    }
                }
                for o in
                    sync::pull(&t, job.ns.realm_arg(), job.ns.kind, &job.selector, force).await?
                {
                    any = true;
                    let what = match &o.status {
                        sync::PullStatus::Created => "pulled (new)".to_string(),
                        sync::PullStatus::Updated => "pulled (updated)".to_string(),
                        sync::PullStatus::Unchanged => "unchanged".to_string(),
                        sync::PullStatus::LocalBackedUp(p) => {
                            format!("pulled; local edits backed up to {}", p.display())
                        }
                    };
                    println!(
                        "  {}: {what}",
                        script::full_name(o.kind, job.ns.realm.as_deref(), &o.name)
                    );
                }
            }
            if !any {
                println!("nothing to pull");
            }
            workspace_update_hint(&t)?;
            Ok(())
        }
        ScriptCommand::Push {
            reference,
            tenant,
            force,
            yes,
        } => {
            let t = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            if reference.as_deref() == Some("all") {
                return push_all(&t, force, yes).await;
            }
            // No ref → fuzzy-pick one (changed scripts marked `!`, first).
            let (ns, name) = match reference {
                Some(s) => parse_one(&s)?,
                None => match pick("Push which script?", sync::push_candidates(&t)?, true)? {
                    Some(x) => x,
                    None => return Ok(()),
                },
            };
            push_one(&t, &ns, &name, force, yes).await?;
            workspace_update_hint(&t)?;
            Ok(())
        }
        ScriptCommand::Status { reference, tenant } => {
            let t = tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            // `reference` is a free-text filter: a group (`am`/`idm`), a
            // namespace (`alpha`/`endpoint`/…), a full-name (`alpha/Email OTP`),
            // or any fragment. See `script::matches_term`.
            let filter = reference.filter(|s| !s.trim().is_empty());
            let mut total = 0;
            let mut shown = 0;
            for e in sync::status(&t, None).await? {
                total += 1;
                if let Some(term) = &filter {
                    if !script::matches_term(term, e.kind, e.realm.as_deref(), &e.name) {
                        continue;
                    }
                }
                let label = match e.state {
                    sync::ScriptState::InSync => "in sync",
                    sync::ScriptState::LocallyModified => "modified locally",
                    sync::ScriptState::RemotelyModified => "modified on remote",
                    sync::ScriptState::BothModified => "CONFLICT (both changed)",
                    sync::ScriptState::LocalMissing => "local file missing",
                };
                let full = script::full_name(e.kind, e.realm.as_deref(), &e.name);
                println!("  {full:<48} {label}");
                shown += 1;
            }
            if shown == 0 {
                match &filter {
                    Some(term) if total > 0 => {
                        println!("no synced script matches {term:?} ({total} synced)");
                    }
                    _ => println!("nothing synced yet — `aic script pull …` first"),
                }
            }
            Ok(())
        }
        ScriptCommand::Who {
            reference,
            tenant,
            history,
            minutes,
            json,
        } => {
            let t = tenant_for(tenant)?;
            let (ns, name) = parse_one(&reference)?;
            who(&t, &ns, &name, history, minutes, json).await
        }
        ScriptCommand::Sync {
            reference,
            resolve,
            tenant,
            yes,
        } => {
            let t = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            let cands = select_synced(sync::push_candidates(&t)?, reference)?;
            if cands.is_empty() {
                println!("nothing synced to reconcile — `aic script pull …` first");
                return Ok(());
            }
            let (mut pushed, mut pulled, mut in_sync) = (0u32, 0u32, 0u32);
            let mut conflicts: Vec<String> = Vec::new();
            for c in cands {
                let full = full_of(&c);
                let ns = Namespace {
                    kind: c.kind,
                    realm: c.realm.clone(),
                };
                match prod_hint(sync::reconcile(&t, ns.realm_arg(), c.kind, &c.name, yes).await)? {
                    sync::ReconcileOutcome::InSync => in_sync += 1,
                    sync::ReconcileOutcome::Pushed => {
                        pushed += 1;
                        println!("→ pushed {full}");
                    }
                    sync::ReconcileOutcome::Pulled => {
                        pulled += 1;
                        println!("← pulled {full}");
                    }
                    sync::ReconcileOutcome::Converged => {
                        in_sync += 1;
                        println!("= {full}: converged");
                    }
                    sync::ReconcileOutcome::Conflict(_) => {
                        let choice = match resolve {
                            Some(Resolution::Local) => ConflictChoice::Local,
                            Some(Resolution::Remote) => ConflictChoice::Remote,
                            None => prompt_conflict(&full, true)?,
                        };
                        match choice {
                            ConflictChoice::Local => {
                                prod_hint(
                                    sync::push(&t, ns.realm_arg(), c.kind, &c.name, true, yes)
                                        .await,
                                )?;
                                pushed += 1;
                                println!("→ pushed {full} (resolved: local)");
                            }
                            ConflictChoice::Remote => {
                                sync::pull(
                                    &t,
                                    ns.realm_arg(),
                                    c.kind,
                                    &sync::Selector::Name(c.name.clone()),
                                    false,
                                )
                                .await?;
                                pulled += 1;
                                println!("← pulled {full} (resolved: remote; local backed up)");
                            }
                            ConflictChoice::Skip => conflicts.push(full),
                            ConflictChoice::Stop => {
                                println!("\nstopped.");
                                return Ok(());
                            }
                        }
                    }
                }
            }
            println!(
                "\nsync: pushed {pushed} · pulled {pulled} · in sync {in_sync} · conflicts {}",
                conflicts.len()
            );
            if !conflicts.is_empty() {
                println!("unresolved (try `aic script diff <ref>`, then push/pull --force):");
                for c in &conflicts {
                    println!("  {c}");
                }
            }
            workspace_update_hint(&t)?;
            Ok(())
        }
        ScriptCommand::Watch { tenant, yes } => {
            let t = writable_tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            watch(&t, yes).await
        }
        ScriptCommand::Diff {
            reference,
            local_vs_snapshot,
            snapshot_vs_remote,
            tenant,
        } => {
            let t = tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            // No ref → pick from synced scripts (diff needs a snapshot).
            let (ns, name) = match reference {
                Some(s) => parse_one(&s)?,
                None => match pick("Diff which script?", sync::push_candidates(&t)?, true)? {
                    Some(x) => x,
                    None => return Ok(()),
                },
            };
            let full = script::full_name(ns.kind, ns.realm.as_deref(), &name);
            // `-` is the left/older side, `+` is the right/newer side.
            let (mode, ll, rl) = if local_vs_snapshot {
                (sync::DiffMode::LocalVsSnapshot, "snapshot", "local")
            } else if snapshot_vs_remote {
                (sync::DiffMode::SnapshotVsRemote, "snapshot", "tenant")
            } else {
                (sync::DiffMode::RemoteVsLocal, "tenant", "local")
            };
            let pair = sync::diff(&t, ns.realm_arg(), ns.kind, &name, mode).await?;
            show_diff(&full, ll, &pair.left, rl, &pair.right)?;
            Ok(())
        }
    }
}

pub async fn run_workspace(command: WorkspaceCommand) -> Result<()> {
    use crate::scripts::workspace;

    match command {
        WorkspaceCommand::Init { tenant } => {
            let t = tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            let r = workspace::init(&t)?;
            let managed_types = generate_managed_types(&t).await;
            let sync_types = generate_sync_mapping_types(&t).await;
            println!(
                "workspace ready at {} ({} files written, {} managed type files, {} sync type files, templates v{})",
                r.tree.display(),
                r.written.len(),
                managed_types,
                sync_types,
                workspace::TEMPLATES_VERSION
            );
            print_seed_notes(&r);
            println!(
                "next: cd {} && npm install   (installs the lint/type-check toolchain)",
                r.tree.display()
            );
            Ok(())
        }
        WorkspaceCommand::Update { tenant } => {
            let t = tenant_for(tenant)?;
            guard_legacy_workspace(&t)?;
            let r = workspace::update(&t)?;
            let managed_types = generate_managed_types(&t).await;
            let sync_types = generate_sync_mapping_types(&t).await;
            println!(
                "templates refreshed to v{} ({} files written, {} managed type files, {} sync type files) at {}",
                workspace::TEMPLATES_VERSION,
                r.written.len(),
                managed_types,
                sync_types,
                r.tree.display()
            );
            print_seed_notes(&r);
            Ok(())
        }
    }
}

fn print_seed_notes(report: &crate::scripts::workspace::WorkspaceReport) {
    for path in &report.drifted {
        let rel = path.strip_prefix(&report.tree).unwrap_or(path);
        println!(
            "{rel}: skipped (you edited it; it may no longer compile against the framework)",
            rel = rel.display()
        );
    }
    for path in &report.unverifiable {
        let rel = path.strip_prefix(&report.tree).unwrap_or(path);
        println!(
            "{rel}: skipped (no recorded seed hash; not overwritten)",
            rel = rel.display()
        );
    }
}

/// Say what a workspace without tenant types can and cannot do, and leave the
/// same note in `src/generated/managed.ts` so it is findable from the editor.
///
/// Without this the failure is a one-line warning and the consequence lands much
/// later: `npm run type-check` fails in a freshly scaffolded workspace, because
/// the seeded `example-managed-users.ts` is written against the tenant's types
/// and `ManagedName` is `never` until they exist.
fn explain_missing_managed_types(tenant: &str) {
    const NOTE: &str = "\
// NOT GENERATED. `aic workspace init`/`update` could not reach the tenant, so
// this file holds no managed-object types and `ManagedObjects` stays empty.
//
// What that costs, until `aic login && aic workspace update` replaces this file:
//   * `openidm` calls still compile, but hand back the generic CREST resource,
//     so members are index-only -- `record[\"userName\"]`, not `record.userName`;
//   * a `fields` argument on a `managed/...` path is rejected outright;
//   * `ManagedName` is `never`, so `src/endpoints/example-managed-users.ts`
//     (seeded against this tenant's schema) does NOT type-check yet. Delete it
//     if you would rather not wait.
//
// Gitignored either way. See docs/typescript-endpoints.md.
export {};
";
    eprintln!(
        "         `typescript/src/endpoints/example-managed-users.ts` needs them, so\n         \
         `npm run type-check` will fail until you run `aic login && aic workspace update`."
    );
    let path = ProjectConfig::workspace_tree(tenant)
        .join("typescript")
        .join("src")
        .join("generated")
        .join("managed.ts");
    // Never clobber a good module with the note: a transient fetch failure on a
    // workspace that already has types must leave them alone.
    if path.exists() || path.parent().is_none_or(|p| !p.is_dir()) {
        return;
    }
    if let Err(error) = std::fs::write(&path, NOTE) {
        eprintln!("warning: could not write {}: {error}", path.display());
    }
}

/// Fetch the tenant's managed schema and write its generated per-object types.
/// This is best-effort because the embedded workspace scaffold is still useful
/// when the agent is locked or unavailable.
async fn generate_managed_types(tenant: &str) -> usize {
    let schema = match crate::aic::api::get(tenant, "/openidm/config/managed").await {
        Ok(schema) => schema,
        Err(error) => {
            eprintln!("warning: could not fetch managed schema (types not generated): {error}");
            explain_missing_managed_types(tenant);
            return 0;
        }
    };
    let files = match crate::scripts::managed_types::generate(&schema) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("warning: could not generate managed types: {error}");
            explain_missing_managed_types(tenant);
            return 0;
        }
    };

    let tree = ProjectConfig::workspace_tree(tenant);
    let mut written = 0;
    for (relative, contents) in files {
        let path = tree.join(relative);
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "warning: could not create managed type directory {}: {error}",
                    parent.display()
                );
                continue;
            }
        }
        match std::fs::write(&path, contents) {
            Ok(()) => written += 1,
            Err(error) => eprintln!(
                "warning: could not write managed type {}: {error}",
                path.display()
            ),
        }
    }
    warn_dangling_reverses(&schema);
    written
}

/// Name the relationships whose declared reverse property was never created on
/// the target object, so a missing member in the generated types reads as a
/// tenant defect rather than a generation bug. The reverse side is absent from
/// the runtime as well — see `managed::ops::dangling_reverses`.
fn warn_dangling_reverses(schema: &serde_json::Value) {
    let dangling = crate::managed::ops::dangling_reverses(schema);
    if dangling.is_empty() {
        return;
    }
    let subject = match dangling.len() {
        1 => "1 relationship names".to_string(),
        n => format!("{n} relationships name"),
    };
    eprintln!(
        "warning: {subject} a reverse property that does not exist on the\n         \
         target object. It is missing from the runtime too, so the generated types\n         \
         leave it out — the tenant's schema is half-declared, not the generator:"
    );
    for entry in dangling {
        eprintln!(
            "           {}.{} -> {}.{}",
            entry.source_object, entry.key, entry.target_object, entry.reverse_key
        );
    }
}

/// Fetch the tenant's sync mappings and managed schema, then write generated
/// per-mapping binding files. Best-effort for the same reason as managed
/// types: the static workspace scaffold should still succeed while locked.
async fn generate_sync_mapping_types(tenant: &str) -> usize {
    let sync_doc = match crate::aic::api::get(tenant, "/openidm/config/sync").await {
        Ok(sync_doc) => sync_doc,
        Err(error) => {
            eprintln!("warning: could not fetch sync mappings (types not generated): {error}");
            return 0;
        }
    };
    let managed_schema = match crate::aic::api::get(tenant, "/openidm/config/managed").await {
        Ok(managed_schema) => managed_schema,
        Err(error) => {
            eprintln!("warning: could not fetch managed schema for sync types: {error}");
            return 0;
        }
    };
    let files = match crate::scripts::sync_types::generate(&sync_doc, &managed_schema) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("warning: could not generate sync mapping types: {error}");
            return 0;
        }
    };

    let tree = ProjectConfig::workspace_tree(tenant);
    let mut written = 0;
    for (relative, contents) in files {
        let path = tree.join(relative);
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "warning: could not create sync type directory {}: {error}",
                    parent.display()
                );
                continue;
            }
        }
        match std::fs::write(&path, contents) {
            Ok(()) => written += 1,
            Err(error) => eprintln!(
                "warning: could not write sync type {}: {error}",
                path.display()
            ),
        }
    }
    written
}

/// `aic script who` — who created and last modified a script.
///
/// Two calls for the fields (list to find the wire id, then a read), plus at
/// most one principal lookup per distinct author. `--history` adds the log
/// query, which is a different API with its own credentials.
async fn who(
    tenant: &str,
    ns: &Namespace,
    name: &str,
    history: bool,
    minutes: u32,
    json: bool,
) -> Result<()> {
    use script::authorship::{self as auth, Authorship, PrincipalCache};

    let full = script::full_name(ns.kind, ns.realm.as_deref(), name);

    // Only AM records this. Saying so beats inventing a fallback — verified
    // 2026-08-10 that IDM config objects carry no authorship and no `_rev`.
    if !ns.kind.has_authorship() {
        if json {
            return print_json(&serde_json::json!({
                "script": full,
                "kind": ns.kind.as_str(),
                "hasAuthorship": false,
            }));
        }
        println!("{full}");
        println!("  no authorship recorded — IDM config objects store neither an author");
        println!("  nor a revision, so there is nothing to attribute this write to.");
        println!("  The logs are the only route:");
        println!("      aic logs range --source idm-access …");
        return Ok(());
    }

    let refs = ns.kind.list(tenant, ns.realm_arg()).await?;
    let sref = refs
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::Config(format!("no script named {name:?} in {}", ns.realm_arg())))?;
    let remote = ns.kind.fetch(tenant, ns.realm_arg(), &sref.id).await?;
    let a = Authorship::from_config(&remote.raw_config);

    // One lookup per distinct principal: created and modified are usually the
    // same account, and the cache makes that one request rather than two.
    let mut cache = PrincipalCache::default();
    let created = cache.resolve(tenant, &a.created.by).await;
    let modified = cache.resolve(tenant, &a.modified.by).await;

    let events = if history {
        Some(update_history(tenant, &sref.id, minutes, &mut cache).await?)
    } else {
        None
    };

    if json {
        let mut out = serde_json::json!({
            "script": full,
            "kind": ns.kind.as_str(),
            "id": sref.id,
            "hasAuthorship": true,
            "created": author_json(&a.created, created.as_ref()),
            "modified": author_json(&a.modified, modified.as_ref()),
        });
        if let Some(events) = &events {
            out["history"] = serde_json::json!(
                events
                    .iter()
                    .map(|e| serde_json::json!({ "at": e.at, "by": e.by }))
                    .collect::<Vec<_>>()
            );
        }
        return print_json(&out);
    }

    println!("{full}");
    println!(
        "  last modified  {}  by {}",
        auth::format_local(a.modified.at),
        auth::describe_author(&a.modified.by, modified.as_ref())
    );
    println!(
        "  created        {}  by {}",
        auth::format_local(a.created.at),
        auth::describe_author(&a.created.by, created.as_ref())
    );
    // Said once, not per line: `aic`'s own pushes all land on the shared SA, so
    // without this the output reads as though a person made the change.
    if auth::is_service_account(modified.as_ref()) || auth::is_service_account(created.as_ref()) {
        println!(
            "  note: a service account is a shared credential — it does not identify\n        \
             which operator ran the write."
        );
    }

    if let Some(events) = events {
        println!();
        println!("  successful updates in the last {minutes} min (log retention is 30 days):");
        if events.is_empty() {
            println!("    none");
        }
        for e in events {
            println!("    {}  {}", e.at, e.by);
        }
    }
    Ok(())
}

/// The log API's own per-query span limit.
///
/// 1440 is the **server's** number, not a client-side courtesy: a wider span is
/// rejected with `400 … "Cannot request more than one days worth of logs"`
/// (verified 2026-08-10 with a 13.89-day window; also `docs/api/08-logs.md`).
/// Retention is much longer — about 30 days — so the cap is on the *window*, not
/// on how far back events live. Checked locally so the operator gets that
/// distinction rather than the server's phrasing.
const MAX_HISTORY_MINUTES: u32 = 24 * 60;

fn validate_minutes(minutes: u32) -> Result<()> {
    if (1..=MAX_HISTORY_MINUTES).contains(&minutes) {
        return Ok(());
    }
    Err(Error::Config(format!(
        "--minutes must be between 1 and {MAX_HISTORY_MINUTES}: the log API refuses a \
         query spanning more than one day. Older events are retained (about 30 days) \
         but need a narrower window placed further back."
    )))
}

/// One `am-access` update event, already resolved to a name.
struct UpdateEvent {
    at: String,
    by: String,
}

/// Recent **successful** script updates from `am-access`.
///
/// Two filters are load-bearing, both verified 2026-08-10:
/// `AM-ACCESS-OUTCOME` (attempt and outcome are logged as a pair), and
/// `status eq "SUCCESSFUL"` — because every `PUT` update *also* logs a phantom
/// `CREATE`/`FAILED`/412 "already exist" event sharing one `transactionId` with
/// the real `UPDATE`. Without both, the history shows failures that never
/// happened. `userId` here is the full DN, identical to `lastModifiedBy`, so it
/// feeds the same resolver and the same cache.
async fn update_history(
    tenant: &str,
    script_id: &str,
    minutes: u32,
    cache: &mut script::authorship::PrincipalCache,
) -> Result<Vec<UpdateEvent>> {
    use script::authorship::{self as auth, Author};

    validate_minutes(minutes)?;
    let end = chrono::Utc::now();
    let begin = end - chrono::Duration::minutes(minutes as i64);
    let stamp = |t: chrono::DateTime<chrono::Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Matches on the resource id, never a path prefix: `am-access` records the
    // URL exactly as the client sent it, and the realm segment has three
    // interchangeable spellings (CLAUDE.md §4).
    let filter = format!(
        "/payload/component eq \"Script\" \
         and /payload/eventName eq \"AM-ACCESS-OUTCOME\" \
         and /payload/response/status eq \"SUCCESSFUL\" \
         and /payload/request/operation eq \"UPDATE\" \
         and /payload/http/request/path co \"{script_id}\""
    );
    let params = vec![
        ("source".to_string(), "am-access".to_string()),
        ("beginTime".to_string(), stamp(begin)),
        ("endTime".to_string(), stamp(end)),
        ("_queryFilter".to_string(), filter),
    ];

    let context = crate::logs::ops::fetch_context(Some(tenant.to_string())).await?;
    let rows =
        crate::logs::api::fetch_all(&context.client, &context.base_url, &context.key, &params)
            .await?;

    let mut events = Vec::new();
    for row in rows {
        let payload = row.get("payload").unwrap_or(&row);
        let at = payload
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let author = Author::parse(payload.get("userId"));
        let identity = cache.resolve(tenant, &author).await;
        events.push(UpdateEvent {
            at,
            by: auth::describe_author(&author, identity.as_ref()),
        });
    }
    Ok(events)
}

/// A `Change` as JSON, with the machine-readable author kind alongside the
/// display string so a consumer never has to parse prose.
fn author_json(
    change: &script::authorship::Change,
    resolved: Option<&script::authorship::Identity>,
) -> serde_json::Value {
    use script::authorship as auth;
    serde_json::json!({
        "by": auth::describe_author(&change.by, resolved),
        "kind": auth::author_kind(&change.by, resolved),
        "principalId": change.by.principal_id(),
        "at": auth::format_iso(change.at),
    })
}

/// One unit of script-sync work: a namespace and which scripts within it.
struct Job {
    ns: Namespace,
    selector: script::sync::Selector,
}

/// Expand a positional `<ref>` into jobs. `None` → every namespace (bulk);
/// `<prefix>` → that whole namespace; `<prefix>/<name>` → one script; a bare
/// `<name>` → one script in the current directory's namespace.
fn parse_ref(arg: Option<String>) -> Result<Vec<Job>> {
    // `None` and the explicit keyword `all` both mean every namespace.
    let Some(s) = arg.filter(|s| s != "all") else {
        return Ok(Namespace::all()
            .into_iter()
            .map(|ns| Job {
                ns,
                selector: script::sync::Selector::All,
            })
            .collect());
    };
    if let Some((prefix, name)) = s.split_once('/') {
        let ns = Namespace::parse(prefix).ok_or_else(|| unknown_ns(prefix))?;
        return Ok(vec![Job {
            ns,
            selector: script::sync::Selector::Name(name.to_string()),
        }]);
    }
    if let Some(ns) = Namespace::parse(&s) {
        return Ok(vec![Job {
            ns,
            selector: script::sync::Selector::All,
        }]);
    }
    let (ns, name) = resolve_bare(&s)?;
    Ok(vec![Job {
        ns,
        selector: script::sync::Selector::Name(name),
    }])
}

/// Parse a `<ref>` that must identify exactly one script (push / diff).
fn parse_one(arg: &str) -> Result<(Namespace, String)> {
    if let Some((prefix, name)) = arg.split_once('/') {
        let ns = Namespace::parse(prefix).ok_or_else(|| unknown_ns(prefix))?;
        return Ok((ns, name.to_string()));
    }
    if Namespace::parse(arg).is_some() {
        return Err(Error::Config(format!(
            "{arg:?} is a whole namespace — name a specific script, e.g. {arg}/<name>"
        )));
    }
    resolve_bare(arg)
}

/// A bare name (no prefix): take the namespace from the current directory.
fn resolve_bare(name: &str) -> Result<(Namespace, String)> {
    let prefix = config::workspace_context().namespace.ok_or_else(|| {
        Error::Config(format!(
            "ambiguous {name:?} — prefix with a namespace (e.g. bravo/{name}) or run from inside a workspace subdir"
        ))
    })?;
    let ns = Namespace::parse(&prefix)
        .ok_or_else(|| Error::Config("unexpected workspace namespace".into()))?;
    Ok((ns, name.to_string()))
}

fn unknown_ns(prefix: &str) -> Error {
    Error::Config(format!(
        "unknown namespace {prefix:?} (use alpha | bravo | endpoint | schedule | managed | sync)"
    ))
}

fn require_standalone(kind: script::Kind, name: &str) -> Result<()> {
    if kind.standalone() {
        Ok(())
    } else {
        Err(script::embedded_kind_error(kind, name))
    }
}

fn validate_copy(
    source_ns: &Namespace,
    source_name: &str,
    destination_ns: &Namespace,
    destination_name: &str,
) -> Result<()> {
    if source_ns.kind != destination_ns.kind {
        return Err(Error::Config(
            "copy source and destination must have the same script kind".into(),
        ));
    }
    if source_ns == destination_ns && source_name == destination_name {
        return Err(Error::Config(
            "copy source and destination are identical".into(),
        ));
    }
    Ok(())
}

/// Whether a tenant theme permits direct script writes.
fn scripts_are_writable(theme: TenantTheme) -> bool {
    theme.allows_static_content()
}

/// Resolve the tenant for a script **write**, refusing the environments where
/// scripts are immutable.
///
/// Scripts are static content: AIC promotes them up from sandbox/development,
/// and staging/production hold them read-only. Failing here — before a token is
/// even spent — beats surfacing whatever the tenant returns. Reads (`list`,
/// `pull`, `status`, `diff`) are deliberately unrestricted: comparing a higher
/// environment against your workspace is exactly how you check a promotion.
fn writable_tenant_for(tenant: Option<String>) -> Result<String> {
    let tenant = crate::cli::tenant_config_for(tenant)?;
    if !scripts_are_writable(tenant.theme) {
        return Err(Error::Config(format!(
            "scripts are immutable on '{}' tenants like '{}' — they are static content promoted up from sandbox/development, so change the script there and promote it",
            tenant.theme.label(),
            tenant.name
        )));
    }
    Ok(tenant.name)
}

/// Lifecycle commands must be able to update a real workspace + snapshot,
/// rather than creating a remote-only resource that later commands cannot see.
fn require_workspace(tenant: &str) -> Result<()> {
    if crate::scripts::workspace::applied_version(tenant)? == 0 {
        return Err(Error::Config(format!(
            "workspace for {tenant} is not initialised — run `aic workspace init` first"
        )));
    }
    Ok(())
}

/// Render a listed script as JSON, tagged with its copy-pasteable `ref`.
fn listed(r: &script::RemoteRef, ns: &Namespace) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "ref".to_string(),
            serde_json::Value::String(script::full_name(r.kind, ns.realm.as_deref(), &r.name)),
        );
    }
    v
}

fn listed_row(r: &script::RemoteRef, ns: &Namespace) -> Vec<String> {
    vec![
        script::full_name(r.kind, ns.realm.as_deref(), &r.name),
        r.kind.as_str().to_string(),
        r.context.as_deref().unwrap_or("-").to_string(),
        r.evaluator_version.as_deref().unwrap_or("-").to_string(),
        r.is_default.to_string(),
        r.id.clone(),
    ]
}

fn full_of(c: &script::sync::Candidate) -> String {
    script::full_name(c.kind, c.realm.as_deref(), &c.name)
}

/// Picker line prefix for a local state: `!` changed on disk, `-` no local
/// file yet, blank = in sync with the snapshot.
fn mark(s: script::sync::LocalState) -> &'static str {
    use script::sync::LocalState::*;
    match s {
        Modified => "! ",
        Missing => "- ",
        Clean => "  ",
    }
}

/// Interactive single-select over candidates; type to filter. `!`/`-`/blank
/// prefixes show local state. When `prioritise`, locally-changed scripts sort
/// to the top (for push); otherwise alphabetical (for pull). Returns the chosen
/// (namespace, name), or `None` if the user cancels / there's nothing to pick.
fn pick(
    prompt: &str,
    mut candidates: Vec<script::sync::Candidate>,
    prioritise: bool,
) -> Result<Option<(Namespace, String)>> {
    use inquire::{Select, error::InquireError};
    use script::sync::LocalState;
    if candidates.is_empty() {
        println!("nothing to choose from");
        return Ok(None);
    }
    if crate::cli::prompting_disabled() {
        return Err(Error::Config(
            "script picker disabled by --no-prompt; pass a script ref or `all`".into(),
        ));
    }
    let rank = |s: LocalState| match s {
        LocalState::Modified => 0,
        LocalState::Missing => 1,
        LocalState::Clean => 2,
    };
    candidates.sort_by(|a, b| {
        if prioritise {
            rank(a.local)
                .cmp(&rank(b.local))
                .then_with(|| full_of(a).cmp(&full_of(b)))
        } else {
            full_of(a).cmp(&full_of(b))
        }
    });
    let labels: Vec<String> = candidates
        .iter()
        .map(|c| format!("{}{}", mark(c.local), full_of(c)))
        .collect();
    match Select::new(prompt, labels).with_page_size(15).raw_prompt() {
        Ok(opt) => {
            let c = &candidates[opt.index];
            Ok(Some((
                Namespace {
                    kind: c.kind,
                    realm: c.realm.clone(),
                },
                c.name.clone(),
            )))
        }
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
        Err(InquireError::NotTTY) => Err(Error::Config(
            "no terminal for the picker — pass a script ref (e.g. bravo/Foo) or `all`".into(),
        )),
        Err(e) => Err(Error::Config(format!("picker: {e}"))),
    }
}

enum ConflictChoice {
    /// Ctrl-C at the prompt. `inquire` is in raw mode, so the terminal never
    /// raises SIGINT and no signal handler sees it — it arrives only as this,
    /// and reading it as "skip" is what made Ctrl-C unable to stop a watch.
    Stop,
    Local,
    Remote,
    Skip,
}

/// Prompt how to resolve a both-changed conflict during `sync`. No TTY (or
/// cancel) → `Skip` (reported at the end), never a silent clobber.
fn prompt_conflict(full: &str, allow_local: bool) -> Result<ConflictChoice> {
    use inquire::{Select, error::InquireError};
    if crate::cli::prompting_disabled() {
        return Ok(ConflictChoice::Skip);
    }
    let opts = if allow_local {
        vec![
            "skip — leave both, resolve later",
            "local — overwrite the tenant with your copy",
            "remote — overwrite your copy with the tenant's (local backed up)",
        ]
    } else {
        vec![
            "skip — leave both, resolve later",
            "remote — overwrite your copy with the tenant's (local backed up)",
        ]
    };
    match Select::new(
        &format!("Conflict on {full} (both changed) — resolve:"),
        opts,
    )
    .raw_prompt()
    {
        Ok(o) if allow_local => Ok(match o.index {
            1 => ConflictChoice::Local,
            2 => ConflictChoice::Remote,
            _ => ConflictChoice::Skip,
        }),
        Ok(o) => Ok(if o.index == 1 {
            ConflictChoice::Remote
        } else {
            ConflictChoice::Skip
        }),
        Err(InquireError::OperationInterrupted) => Ok(ConflictChoice::Stop),
        Err(InquireError::OperationCanceled | InquireError::NotTTY) => Ok(ConflictChoice::Skip),
        Err(e) => Err(Error::Config(format!("conflict prompt: {e}"))),
    }
}

/// Map a workspace file path back to its script ref, or `None` if it isn't a
/// syncable source file. Inverse of the per-kind `workspace_subpath`:
/// `am/<realm>/<type>/<Name>.cjs`, `idm/endpoint/<name>.cjs`,
/// `idm/schedule/<name>.cjs`.
fn workspace_path_ref(
    tree: &std::path::Path,
    path: &std::path::Path,
) -> Option<(Namespace, String)> {
    let rel = path.strip_prefix(tree).ok()?;
    if rel.extension().and_then(|e| e.to_str()) != Some("cjs") {
        return None;
    }
    let name = rel.file_stem()?.to_str()?.to_string();
    let parent: Vec<String> = rel
        .parent()?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parent.as_slice() {
        [a, realm, _type] if a == "am" && (realm == "alpha" || realm == "bravo") => Some((
            Namespace {
                kind: script::Kind::Am,
                realm: Some(realm.clone()),
            },
            name,
        )),
        [i, k] if i == "idm" && k == "endpoint" => Some((
            Namespace {
                kind: script::Kind::IdmEndpoint,
                realm: None,
            },
            name,
        )),
        [i, k] if i == "idm" && k == "schedule" => Some((
            Namespace {
                kind: script::Kind::IdmSchedule,
                realm: None,
            },
            name,
        )),
        _ => None,
    }
}

/// Collect the `.cjs` paths that still exist from a notify event.
fn collect_cjs(
    set: &mut std::collections::BTreeSet<std::path::PathBuf>,
    res: notify::Result<notify::Event>,
) {
    if let Ok(ev) = res {
        for p in ev.paths {
            if p.extension().is_some_and(|e| e == "cjs") && p.is_file() {
                // Canonicalise so paths match the canonical workspace root
                // regardless of whether notify reports relative or absolute.
                set.insert(std::fs::canonicalize(&p).unwrap_or(p));
            }
        }
    }
}

/// Watch the tenant workspace and push each saved `.cjs` (debounced). Pushes a
/// file only if it's a tracked (synced) script; remote drift is resolved with
/// the same choices as `sync`. Runs until Ctrl-C.
async fn watch(tenant: &str, yes: bool) -> Result<()> {
    use notify::{RecursiveMode, Watcher};
    use script::sync::{LocalState, PushOutcome};

    let tree = ProjectConfig::workspace_tree(tenant);
    if !tree.exists() {
        return Err(Error::Config(format!(
            "no workspace at {} — `aic script pull …` first",
            tree.display()
        )));
    }
    // Absolute, so it matches the (canonicalised) event paths from notify.
    let tree = std::fs::canonicalize(&tree).map_err(|e| Error::Config(format!("watch: {e}")))?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| Error::Config(format!("watch: {e}")))?;
    watcher
        .watch(&tree, RecursiveMode::Recursive)
        .map_err(|e| Error::Config(format!("watch {}: {e}", tree.display())))?;
    println!(
        "watching {} — save a script to push it; Ctrl-C to stop",
        tree.display()
    );

    // One handler for the whole run. A fresh `tokio::signal::ctrl_c()` per
    // `select!` only remembers signals delivered while that particular future
    // is alive, so every Ctrl-C landing in a gap — mid-debounce, or while
    // `inquire` owns the terminal and swallows the interrupt — was dropped.
    // Latching it once means a Ctrl-C is never missed, only acted on late.
    let stop = Stop::latch_ctrl_c();

    'watching: loop {
        let first = tokio::select! {
            _ = stop.wait() => break,
            ev = rx.recv() => match ev { Some(e) => e, None => break },
        };
        // Debounce: coalesce the burst of events an editor emits per save,
        // under a ceiling — a tree something else is writing to continuously
        // must not hold the loop open past the next Ctrl-C.
        let mut changed = std::collections::BTreeSet::new();
        collect_cjs(&mut changed, first);
        let ceiling = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            tokio::select! {
                _ = stop.wait() => break,
                _ = tokio::time::sleep_until(ceiling) => break,
                _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => break,
                ev = rx.recv() => match ev {
                    Some(e) => collect_cjs(&mut changed, e),
                    None => break,
                },
            }
        }
        if stop.stopped() {
            break;
        }
        // Re-read each round: the save that woke us may have been the
        // TypeScript build emitting a brand-new endpoint plus its manifest.
        let declared = script::ts_project::declared_endpoints(tenant);
        for path in changed {
            if stop.stopped() {
                break 'watching;
            }
            let Some((ns, name)) = workspace_path_ref(&tree, &path) else {
                continue;
            };
            let full = script::full_name(ns.kind, ns.realm.as_deref(), &name);
            // Only push tracked scripts (`Missing` = not synced) — except a
            // generated endpoint the TypeScript project declares it owns, which
            // has no snapshot precisely because it has never existed remotely.
            match script::sync::local_state(tenant, ns.kind, ns.realm_arg(), &name) {
                Ok(LocalState::Missing) => {
                    if ns.kind != script::Kind::IdmEndpoint || !declared.contains(&name) {
                        continue;
                    }
                    match adopt_generated(tenant, &ns, &name, &path, yes, &stop).await {
                        Ok(Adoption::Created) => {
                            println!("{}", watch_green(&format!("+ created {full}")));
                            continue;
                        }
                        // Now tracked, so fall through and let the ordinary
                        // push decide what to do with the local build.
                        Ok(Adoption::Adopted(backup)) => {
                            println!(
                                "{}",
                                watch_green(&format!("+ adopted {full} from the tenant"))
                            );
                            if let Some(backup) = backup {
                                println!("  its copy differed — saved to {}", backup.display());
                            }
                        }
                        Err(e) if is_fatal_watch_error(&e) => {
                            eprintln!("! watch stopped: {e}");
                            return Err(e);
                        }
                        Err(e) => {
                            eprintln!("{}", watch_red(&format!("! {full}: {e}")));
                            continue;
                        }
                    }
                }
                Err(_) => continue,
                Ok(_) => {}
            }
            let push = script::sync::push(tenant, ns.realm_arg(), ns.kind, &name, false, yes);
            tokio::pin!(push);
            let result = tokio::select! {
                _ = stop.wait() => {
                    println!("\nstopped watching.");
                    return Ok(());
                }
                result = &mut push => prod_hint(result),
            };
            match result {
                Ok(PushOutcome::Pushed) => println!("{}", watch_green(&format!("→ pushed {full}"))),
                Ok(PushOutcome::Unchanged | PushOutcome::AlreadyInSync) => {}
                Ok(PushOutcome::Conflict(_)) => {
                    eprintln!(
                        "{}",
                        watch_red(&format!("! {full}: remote changed — choose a resolution"))
                    );
                    match prompt_conflict(&full, true)? {
                        ConflictChoice::Local => {
                            let push = script::sync::push(
                                tenant,
                                ns.realm_arg(),
                                ns.kind,
                                &name,
                                true,
                                yes,
                            );
                            tokio::pin!(push);
                            let result = tokio::select! {
                                _ = stop.wait() => {
                                    println!("\nstopped watching.");
                                    return Ok(());
                                }
                                result = &mut push => prod_hint(result),
                            };
                            match result {
                                Ok(PushOutcome::Pushed) => println!(
                                    "{}",
                                    watch_green(&format!("→ pushed {full} (resolved: local)"))
                                ),
                                Ok(_) => {}
                                Err(e) if is_fatal_watch_error(&e) => {
                                    eprintln!("! watch stopped: {e}");
                                    return Err(e);
                                }
                                Err(e) => eprintln!("! {full}: {e}"),
                            }
                        }
                        ConflictChoice::Remote => {
                            let selector = script::sync::Selector::Name(name.clone());
                            let pull = script::sync::pull(
                                tenant,
                                ns.realm_arg(),
                                ns.kind,
                                &selector,
                                false,
                            );
                            tokio::pin!(pull);
                            let result = tokio::select! {
                                _ = stop.wait() => {
                                    println!("\nstopped watching.");
                                    return Ok(());
                                }
                                result = &mut pull => result,
                            };
                            match result {
                                Ok(_) => {
                                    println!("← pulled {full} (resolved: remote; local backed up)")
                                }
                                Err(e) if is_fatal_watch_error(&e) => {
                                    eprintln!("! watch stopped: {e}");
                                    return Err(e);
                                }
                                Err(e) => eprintln!("! {full}: {e}"),
                            }
                        }
                        ConflictChoice::Skip => eprintln!(
                            "{}",
                            watch_red(&format!("! {full}: remote changed — skipped"))
                        ),
                        ConflictChoice::Stop => {
                            println!("\nstopped watching.");
                            return Ok(());
                        }
                    }
                }
                Err(e) if is_fatal_watch_error(&e) => {
                    eprintln!("! watch stopped: {e}");
                    return Err(e);
                }
                Err(e) => eprintln!("! {full}: {e}"),
            }
        }
    }
    if stop.stopped() {
        println!("\nstopped watching.");
    }
    Ok(())
}

/// How a generated endpoint came under sync.
enum Adoption {
    /// It did not exist on the tenant; we made it.
    Created,
    /// It was already there. We took its copy as the baseline (backing that
    /// copy up when it differed from the file on disk) and wrote nothing.
    Adopted(Option<std::path::PathBuf>),
}

/// Bring an endpoint that exists only as a build artefact under sync.
///
/// `aic script watch` otherwise skips any untracked file, which made a
/// generated endpoint impossible to deploy: it has no snapshot until it exists
/// remotely, and it cannot exist remotely until something pushes it. The
/// TypeScript project's manifest breaks the cycle by declaring the file as
/// intentionally owned rather than stray. `sync::create` carries the same prod
/// guard as a push and pulls the server's copy straight back, so the next save
/// takes the ordinary tracked path.
///
/// A name already on the tenant used to end here, permanently: `create` refuses
/// it and `push` refuses it too (no snapshot), so every save repeated the same
/// refusal and nothing the message suggested could clear it. Adopting the
/// tenant's copy as the baseline is the missing step — after it, the file is an
/// ordinary local edit and the next push is conflict-aware like any other.
async fn adopt_generated(
    tenant: &str,
    ns: &Namespace,
    name: &str,
    path: &std::path::Path,
    yes: bool,
    stop: &Stop,
) -> Result<Adoption> {
    let source =
        std::fs::read(path).map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
    let new_script = ns
        .kind
        .new_script(name, &source, &script::NewScriptOpts::default())?;
    let create = script::sync::create(tenant, ns.realm_arg(), &new_script, yes);
    tokio::pin!(create);
    let outcome = tokio::select! {
        _ = stop.wait() => return Err(Error::Config("stopped watching.".into())),
        result = &mut create => prod_hint(result)?,
    };
    match outcome {
        script::sync::CreateOutcome::Created(_) => Ok(Adoption::Created),
        script::sync::CreateOutcome::NameTaken(existing) => {
            let adopt = script::sync::adopt(tenant, ns.realm_arg(), &existing);
            tokio::pin!(adopt);
            tokio::select! {
                _ = stop.wait() => Err(Error::Config("stopped watching.".into())),
                backup = &mut adopt => Ok(Adoption::Adopted(backup?)),
            }
        }
    }
}

/// A Ctrl-C that is remembered rather than raced for.
///
/// `tokio::signal::ctrl_c()` builds a fresh listener each call and only sees
/// signals that arrive while its future is being polled. A loop that awaits a
/// new one per `select!` therefore drops every interrupt landing in between —
/// and `inquire` maps the one arriving at a prompt to "skip", so during a busy
/// watch there may be no window at all in which Ctrl-C reaches this process.
/// Latching it once makes the signal sticky: whenever the loop next looks, it
/// is still set.
#[derive(Clone)]
struct Stop {
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl Stop {
    fn new() -> Self {
        Self {
            stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Latch the stop. Waking waiters is a courtesy — the flag is the record,
    /// so a caller that was not waiting at the time still sees it later.
    fn trip(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn latch_ctrl_c() -> Self {
        let stop = Self::new();
        let handle = stop.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                handle.trip();
            }
        });
        stop
    }

    fn stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resolves once Ctrl-C has been seen — including before this was called.
    /// The `notified()` future is created before the flag is read, so a signal
    /// arriving between the two still wakes it.
    async fn wait(&self) {
        let notified = self.notify.notified();
        if self.stopped() {
            return;
        }
        notified.await;
    }
}

fn is_fatal_watch_error(error: &Error) -> bool {
    match error {
        Error::Config(message) => message.contains("no JWK on file"),
        Error::Auth(_) | Error::Api { status: 401, .. } => true,
        _ => false,
    }
}

fn watch_green(line: &str) -> String {
    use crossterm::style::Stylize;
    use std::io::IsTerminal;

    if std::io::stdout().is_terminal() {
        line.green().to_string()
    } else {
        line.to_string()
    }
}

fn watch_red(line: &str) -> String {
    use crossterm::style::Stylize;
    use std::io::IsTerminal;

    if std::io::stderr().is_terminal() {
        line.red().to_string()
    } else {
        line.to_string()
    }
}

/// Does a candidate belong to namespace `ns`? (Realm matters only for AM.)
fn same_ns(c: &script::sync::Candidate, ns: &Namespace) -> bool {
    c.kind == ns.kind && (c.kind != script::Kind::Am || c.realm.as_deref() == ns.realm.as_deref())
}

/// Narrow the synced candidates by an optional `sync` ref: `None`/`all` →
/// everything; a namespace → that namespace; a full-name/bare name → that one
/// script (errors if it isn't synced).
fn select_synced(
    cands: Vec<script::sync::Candidate>,
    reference: Option<String>,
) -> Result<Vec<script::sync::Candidate>> {
    let Some(s) = reference.filter(|s| s != "all") else {
        return Ok(cands);
    };
    if !s.contains('/') {
        if let Some(ns) = Namespace::parse(&s) {
            return Ok(cands.into_iter().filter(|c| same_ns(c, &ns)).collect());
        }
    }
    let (ns, name) = parse_one(&s)?;
    let found: Vec<_> = cands
        .into_iter()
        .filter(|c| same_ns(c, &ns) && c.name == name)
        .collect();
    if found.is_empty() {
        return Err(Error::Config(format!(
            "{s:?} isn't synced — `aic script pull {s}` first"
        )));
    }
    Ok(found)
}

/// Interactive yes/no (default no). `Some(answer)` if asked; `None` if there's
/// no terminal to prompt on (caller falls back to non-interactive behaviour).
fn confirm_overwrite(prompt: &str) -> Result<Option<bool>> {
    if !prompt_available() {
        return Ok(None);
    }
    confirm_destructive("script overwrite", prompt, "--force").map(Some)
}

async fn push_one(tenant: &str, ns: &Namespace, name: &str, force: bool, yes: bool) -> Result<()> {
    use script::sync::PushOutcome;
    let full = script::full_name(ns.kind, ns.realm.as_deref(), name);
    match prod_hint(script::sync::push(tenant, ns.realm_arg(), ns.kind, name, force, yes).await)? {
        PushOutcome::Pushed => println!("pushed {full}"),
        PushOutcome::Unchanged => println!("{full}: no local changes to push"),
        PushOutcome::AlreadyInSync => {
            println!("{full}: remote already matched local; snapshot refreshed")
        }
        // Remote drifted since our last sync — offer to overwrite it.
        PushOutcome::Conflict(tw) => {
            match confirm_overwrite(&format!(
                "{full} changed on the tenant since you last synced — overwrite the remote?"
            ))? {
                Some(true) => {
                    prod_hint(
                        script::sync::push(tenant, ns.realm_arg(), ns.kind, name, true, yes).await,
                    )?;
                    println!("pushed {full} (overwrote remote changes)");
                }
                Some(false) => println!("{full}: skipped (remote changed)"),
                None => {
                    // no TTY to prompt on
                    print_conflict(&full, &tw);
                    return Err(Error::Config(
                        "remote changed since last sync — resolve, or re-run with --force".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Push every synced script with local changes. Clean / never-pulled scripts
/// are skipped (nothing to push); product defaults are skipped (push them
/// explicitly with `--force`); remote-drift conflicts are reported and skipped
/// rather than aborting the batch.
async fn push_all(tenant: &str, force: bool, yes: bool) -> Result<()> {
    use script::sync::{LocalState, PushOutcome};
    let changed: Vec<_> = script::sync::push_candidates(tenant)?
        .into_iter()
        .filter(|c| c.local == LocalState::Modified)
        .collect();
    if changed.is_empty() {
        println!("nothing changed to push");
        return Ok(());
    }
    for c in changed {
        let full = full_of(&c);
        let ns = Namespace {
            kind: c.kind,
            realm: c.realm.clone(),
        };
        match prod_hint(
            script::sync::push(tenant, ns.realm_arg(), c.kind, &c.name, force, yes).await,
        )? {
            PushOutcome::Pushed => println!("pushed {full}"),
            PushOutcome::Unchanged | PushOutcome::AlreadyInSync => {}
            PushOutcome::Conflict(_) => {
                println!("{full}: CONFLICT — skipped (`diff {full}`, or `push {full} --force`)")
            }
        }
    }
    workspace_update_hint(tenant)?;
    Ok(())
}

/// Refuse to operate when a pre-redesign per-realm workspace is present, so we
/// don't auto-init a fresh per-tenant tree over it and strand local edits.
fn guard_legacy_workspace(tenant: &str) -> Result<()> {
    if let Some(old) = crate::scripts::workspace::legacy_layout(tenant) {
        return Err(Error::Config(format!(
            "old per-realm workspace at {} — the layout is now per-tenant (am/<realm>/…). \
             Rescue any unpushed edits from the old <realm>/ dirs, delete them, then re-run \
             (`aic workspace init` + pull rebuilds the new tree).",
            old.display()
        )));
    }
    Ok(())
}

/// Print a "templates out of date" nudge if the workspace predates the bundled
/// template version (mirrors p1-sync's update prompt).
fn workspace_update_hint(tenant: &str) -> Result<()> {
    use crate::scripts::workspace;
    let applied = workspace::applied_version(tenant)?;
    if applied != 0 && applied < workspace::TEMPLATES_VERSION {
        println!(
            "note: workspace templates v{applied} → v{} available — run `aic workspace update`",
            workspace::TEMPLATES_VERSION
        );
    }
    Ok(())
}

/// Show `left` vs `right` as a real diff by shelling out to `git diff
/// --no-index` (stdio inherited) — so your git pager/color (delta, …) apply
/// interactively, and `aic script diff X | <tool>` pipes a plain unified diff.
/// `left`/`right` are side labels (e.g. "tenant", "local", "snapshot") shown in
/// the headers. Requires `git` on PATH.
fn create_diff_dir() -> Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;

    let dir = std::env::temp_dir().join(format!("aic-diff-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| Error::Config(format!("create temp dir {}: {e}", dir.display())))?;
    Ok(dir)
}

fn write_diff_file(dir: &std::path::Path, name: &str, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let path = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| Error::Config(format!("create temp file {}: {e}", path.display())))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| Error::Config(format!("write temp file {}: {e}", path.display())))
}

fn show_diff(
    full: &str,
    left_label: &str,
    left: &str,
    right_label: &str,
    right: &str,
) -> Result<()> {
    use std::process::Command;

    if left == right {
        println!("{full}: {left_label} and {right_label} are identical");
        return Ok(());
    }
    let dir = create_diff_dir()?;
    // `--no-prefix` makes the headers read `--- <name> (tenant)` etc.; `/` in
    // the full-name isn't path-safe, so swap it for `_`.
    let safe = full.replace('/', "_");
    let left_name = format!("{safe} ({left_label})");
    let right_name = format!("{safe} ({right_label})");
    let render = (|| -> Result<()> {
        for (name, contents) in [(&left_name, left), (&right_name, right)] {
            write_diff_file(&dir, name, contents)?;
        }
        // Run git *in* the temp dir with relative names so the diff headers read
        // `--- <name> (tenant)` rather than the full temp path.
        let status = Command::new("git")
            .current_dir(&dir)
            .args(["diff", "--no-index", "--no-prefix", "--"])
            .arg(&left_name)
            .arg(&right_name)
            .status()
            .map_err(|e| {
                Error::Config(format!(
                    "couldn't run `git` to render the diff ({e}) — is git on your PATH?"
                ))
            })?;
        // `git diff --no-index` exits 1 when the files differ.
        match status.code() {
            Some(0 | 1) => Ok(()),
            Some(code) => Err(Error::Config(format!(
                "`git diff --no-index` failed with exit code {code}"
            ))),
            None => Err(Error::Config(
                "`git diff --no-index` terminated by signal".into(),
            )),
        }
    })();
    let cleanup = std::fs::remove_dir_all(&dir);
    match (render, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(e),
        (Ok(()), Err(e)) => Err(Error::Config(format!(
            "remove temp dir {}: {e}",
            dir.display()
        ))),
        (Err(render), Err(cleanup)) => Err(Error::Config(format!(
            "{render}; also couldn't remove temp dir {}: {cleanup}",
            dir.display()
        ))),
    }
}

fn print_conflict(name: &str, tw: &crate::scripts::sync::ThreeWay) {
    println!("=== {name}: last-synced ===\n{}", tw.last_synced);
    println!("=== {name}: remote ===\n{}", tw.remote);
    println!("=== {name}: local ===\n{}", tw.local);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn history_window_is_capped_at_the_servers_one_day_limit() {
        // The boundary itself, from both sides: 1440 is accepted because the
        // server accepts exactly one day, and 1441 is refused *locally* so the
        // operator is told the window moved rather than being handed the log
        // API's own "13.89 days worth of data requested" phrasing.
        assert!(validate_minutes(1).is_ok());
        assert!(validate_minutes(60).is_ok());
        assert!(validate_minutes(MAX_HISTORY_MINUTES).is_ok());

        let err = validate_minutes(MAX_HISTORY_MINUTES + 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("1440"), "{err}");
        // The retention-vs-window distinction is the actionable part of the
        // message: the events are still there, the query just cannot span to them.
        assert!(err.contains("30 days"), "{err}");
        // Zero is not "no limit".
        assert!(validate_minutes(0).is_err());
    }

    /// The bug this replaced: a fresh `tokio::signal::ctrl_c()` per `select!`
    /// only sees signals delivered while that future is polled, so an interrupt
    /// arriving in a gap was dropped and the watcher ran on.
    #[tokio::test]
    async fn a_stop_that_arrives_with_nobody_waiting_is_still_seen() {
        let stop = Stop::new();
        stop.trip();
        assert!(stop.stopped());
        tokio::time::timeout(std::time::Duration::from_secs(5), stop.wait())
            .await
            .expect("a stop latched before the wait must resolve at once");
    }

    #[tokio::test]
    async fn a_waiter_wakes_on_a_later_stop() {
        let stop = Stop::new();
        let waiting = stop.clone();
        let waiter = tokio::spawn(async move { waiting.wait().await });
        tokio::task::yield_now().await;
        stop.trip();
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("a waiter must wake when the stop is tripped")
            .expect("waiter task");
    }

    /// The advice has to be a verb that works. `push` needs a snapshot, so on a
    /// name that was never synced it fails with `not synced yet` — following it
    /// left the caller exactly where they started.
    #[test]
    fn a_taken_name_points_at_the_verb_that_can_clear_it() {
        let message = script::sync::name_taken_message("myEndpoint");
        assert!(message.contains("aic script pull myEndpoint"), "{message}");
        assert!(!message.contains("push"), "{message}");
    }

    #[test]
    fn fatal_watch_errors_stop_the_loop() {
        assert!(is_fatal_watch_error(&Error::Config(
            "no JWK on file for tenant sandbox".into()
        )));
        assert!(is_fatal_watch_error(&Error::Auth("session expired".into())));
        assert!(is_fatal_watch_error(&Error::Api {
            status: 401,
            body: "unauthorized".into(),
        }));
    }

    #[test]
    fn nonfatal_watch_errors_keep_the_loop_running() {
        assert!(!is_fatal_watch_error(&Error::Config(
            "script transform failed".into()
        )));
        assert!(!is_fatal_watch_error(&Error::Api {
            status: 409,
            body: "conflict".into(),
        }));
        assert!(!is_fatal_watch_error(&Error::ProdConfirmRequired));
    }

    #[test]
    fn diff_temp_files_are_private_and_exclusive() {
        let dir = create_diff_dir().unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o077,
            0
        );

        write_diff_file(&dir, "left", "contents").unwrap();
        let file = dir.join("left");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o077,
            0
        );
        assert!(write_diff_file(&dir, "left", "replacement").is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lifecycle_commands_parse_and_copy_validation_is_strict() {
        let create = crate::cli::Cli::try_parse_from([
            "aic",
            "script",
            "create",
            "alpha/Foo",
            "--context",
            "decision-node",
        ])
        .unwrap();
        assert!(matches!(
            create.command,
            Some(crate::cli::Command::Script {
                command: ScriptCommand::Create { .. }
            })
        ));
        let copy =
            crate::cli::Cli::try_parse_from(["aic", "script", "copy", "alpha/Foo", "bravo/Foo"])
                .unwrap();
        assert!(matches!(
            copy.command,
            Some(crate::cli::Command::Script {
                command: ScriptCommand::Copy { .. }
            })
        ));
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "script",
                "copy",
                "alpha/Foo",
                "bravo/Foo",
                "--to-tenant",
                "uat",
            ])
            .is_err()
        );
        let delete =
            crate::cli::Cli::try_parse_from(["aic", "script", "delete", "alpha/Foo", "--force"])
                .unwrap();
        assert!(matches!(
            delete.command,
            Some(crate::cli::Command::Script {
                command: ScriptCommand::Delete { force: true, .. }
            })
        ));
        assert!(crate::cli::Cli::try_parse_from(["aic", "script", "delete", "alpha/Foo"]).is_ok());

        let alpha = Namespace::parse("alpha").unwrap();
        let bravo = Namespace::parse("bravo").unwrap();
        let endpoint = Namespace::parse("endpoint").unwrap();
        assert!(validate_copy(&alpha, "Foo", &bravo, "Foo").is_ok());
        assert!(validate_copy(&alpha, "Foo", &endpoint, "Foo").is_err());
        assert!(validate_copy(&alpha, "Foo", &alpha, "Foo").is_err());
        assert!(require_standalone(script::Kind::IdmManagedHook, "user.onCreate").is_err());
        assert!(require_standalone(script::Kind::IdmSyncMapping, "map.onUpdate").is_err());
    }

    #[test]
    fn script_write_rule_matches_static_content_theme_rule() {
        for theme in TenantTheme::all() {
            assert_eq!(scripts_are_writable(*theme), theme.allows_static_content());
        }
    }
}
