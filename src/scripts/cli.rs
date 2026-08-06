//! `aic script` parser and command implementation.

use clap::Subcommand;

use crate::cli::{print_json, print_table, prod_hint, tenant_for};
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
            let created = prod_hint(sync::create(&tenant, ns.realm_arg(), &new_script, yes).await)?;
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
            Ok(())
        }
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
            return 0;
        }
    };
    let files = match crate::scripts::managed_types::generate(&schema) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("warning: could not generate managed types: {error}");
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
    written
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
        Err(
            InquireError::OperationCanceled
            | InquireError::OperationInterrupted
            | InquireError::NotTTY,
        ) => Ok(ConflictChoice::Skip),
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

    loop {
        let first = tokio::select! {
            _ = tokio::signal::ctrl_c() => { println!("\nstopped watching."); break; }
            ev = rx.recv() => match ev { Some(e) => e, None => break },
        };
        // Debounce: coalesce the burst of events an editor emits per save.
        let mut changed = std::collections::BTreeSet::new();
        collect_cjs(&mut changed, first);
        while let Ok(Some(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await
        {
            collect_cjs(&mut changed, ev);
        }
        for path in changed {
            let Some((ns, name)) = workspace_path_ref(&tree, &path) else {
                continue;
            };
            // Only push tracked scripts (`Missing` = not synced).
            match script::sync::local_state(tenant, ns.kind, ns.realm_arg(), &name) {
                Ok(LocalState::Missing) | Err(_) => continue,
                Ok(_) => {}
            }
            let full = script::full_name(ns.kind, ns.realm.as_deref(), &name);
            let push = script::sync::push(tenant, ns.realm_arg(), ns.kind, &name, false, yes);
            tokio::pin!(push);
            let result = tokio::select! {
                _ = tokio::signal::ctrl_c() => {
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
                                _ = tokio::signal::ctrl_c() => {
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
                                _ = tokio::signal::ctrl_c() => {
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
    Ok(())
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
    use inquire::{Confirm, error::InquireError};
    if crate::cli::prompting_disabled() {
        return Ok(None);
    }
    match Confirm::new(prompt).with_default(false).prompt() {
        Ok(b) => Ok(Some(b)),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            Ok(Some(false))
        }
        Err(InquireError::NotTTY) => Ok(None),
        Err(e) => Err(Error::Config(format!("confirm: {e}"))),
    }
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
