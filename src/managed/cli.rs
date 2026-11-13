//! `aic managed` parser and managed-schema command implementation.

use clap::{ArgAction, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::cli::force::OperationForce;
use crate::cli::{WriteOk, ensure_prod_confirmed, print_json, print_table, tenant_for};
use crate::managed::{api, diff, ops, recovery, relink, repair, spec, state, storage, verify};
use crate::undo::{DiskLog, UndoLog};

#[derive(Subcommand, Debug)]
pub enum ManagedCommand {
    /// List managed object types with property and hook counts.
    List {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print one managed object's definition as JSON.
    Get {
        name: String,
        /// Restrict schema.properties to matching keys (repeatable, OR-ed globs).
        #[arg(long, value_name = "GLOB")]
        fields: Vec<String>,
        /// Include inline hook source bodies instead of script-pull references.
        #[arg(long)]
        hook_sources: bool,
        /// Print only this property's definition, exactly as stored.
        #[arg(long, value_name = "NAME", conflicts_with_all = ["fields", "schema_only"])]
        property: Option<String>,
        /// Print only the `schema` member (still narrowed by --fields).
        #[arg(long)]
        schema_only: bool,
        /// Print the definition and nothing else, on either stream: no
        /// "omitted … hook sources" or "showing N of M" notes and no
        /// relationship-storage lines (which also skips the `config/repo.ds`
        /// read). Content is unchanged: hook sources stay replaced unless
        /// --hook-sources is given. A --fields pattern that matched nothing is
        /// still named on stderr, since that is usually a typo.
        #[arg(long)]
        json: bool,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Compare one object with a repository file — an object definition or a
    /// whole config/managed document — after normalising known
    /// representation differences (listed in the output). Read-only; exits
    /// non-zero when they differ.
    Diff {
        name: String,
        file: crate::config::UserPath,
        /// Compare exactly, applying no normalisation.
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Create, rename, or delete managed object definitions.
    Object {
        #[command(subcommand)]
        command: ObjectCommand,
    },
    /// Add, edit, rename, or delete scalar managed fields.
    Field {
        #[command(subcommand)]
        command: FieldCommand,
    },
    /// Register an empty lifecycle hook for later script editing.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Create, edit, or delete managed-object relationships.
    Relationship {
        #[command(subcommand)]
        command: RelationshipCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ObjectCommand {
    Create {
        name: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Rename {
        old: String,
        new: String,
        /// Override the guard for an object that carries Ping's
        /// `"Managed Object"` marker under a name that is not a Ping object
        /// (console-created objects carry it). Ping's own objects stay refused.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        name: String,
        /// Override the guard for an object that carries Ping's
        /// `"Managed Object"` marker under a name that is not a Ping object
        /// (console-created objects carry it). Ping's own objects stay refused.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum FieldCommand {
    /// Add a property to a managed object.
    #[command(
        after_help = "Examples:\n  aic managed field add custom_widget.code --type string --nullable\n  aic managed field add custom_widget.details --type object --after code\n  aic managed field add custom_widget.items --type object[]"
    )]
    Add {
        /// `<object>.<key>`, e.g. `custom_widget.code` (a `/` is not accepted).
        field: String,
        #[arg(long = "type", value_parser = ["string", "boolean", "number", "string[]", "object", "object[]"])]
        field_type: String,
        /// Declare the type as `[<type>, "null"]`, as repo-exported schemas do.
        #[arg(long)]
        nullable: bool,
        /// Place the new key right after this property in `schema.order`
        /// instead of at the end.
        #[arg(long, value_name = "KEY")]
        after: Option<String>,
        #[command(flatten)]
        attrs: FieldAttrs,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Edit {
        field: String,
        #[command(flatten)]
        attrs: FieldAttrs,
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Rename {
        field: String,
        new_key: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        field: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args, Debug, Default)]
pub struct FieldAttrs {
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long, action = ArgAction::Set)]
    required: Option<bool>,
    #[arg(long, action = ArgAction::Set)]
    searchable: Option<bool>,
    #[arg(long, action = ArgAction::Set)]
    viewable: Option<bool>,
    #[arg(long = "user-editable", action = ArgAction::Set)]
    user_editable: Option<bool>,
    /// Allowed value, optionally followed by a display title. Repeat to replace the set.
    #[arg(long = "enum", conflicts_with = "clear_enum")]
    enum_values: Vec<String>,
    /// Remove the allowed-value constraint.
    #[arg(long, conflicts_with = "enum_values")]
    clear_enum: bool,
    /// Value applied on record create when the property is omitted. Must match the field type.
    #[arg(long = "default", conflicts_with = "clear_default")]
    default_value: Option<String>,
    /// Remove the default value.
    #[arg(long, conflicts_with = "default_value")]
    clear_default: bool,
}

#[derive(Subcommand, Debug)]
pub enum HookCommand {
    Add {
        object: String,
        hook: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Remove a lifecycle hook and its source from the object's definition.
    /// Undo it from the TUI history overlay. A synced workspace copy is left
    /// in place.
    Delete {
        object: String,
        hook: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum RelationshipCommand {
    Set {
        field: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        forward: String,
        /// Omit to keep whatever the relationship already declares — including
        /// a reverse that names a property the target does not have. Defaults
        /// to `none` only when creating.
        #[arg(long)]
        reverse: Option<String>,
        #[arg(long = "reverse-key")]
        reverse_key: Option<String>,
        #[command(flatten)]
        attrs: Box<FieldAttrs>,
        #[arg(long, action = ArgAction::Set)]
        validate: Option<bool>,
        #[arg(long = "ref-property")]
        ref_properties: Vec<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        /// Exit non-zero unless every end this write declares was confirmed
        /// live. The schema write has landed either way; this is for scripts
        /// that write links straight afterwards, since an end on a type with
        /// no records cannot be probed.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        field: String,
        /// Also remove the config/repo.ds mappings the delete would leave
        /// behind, each the way `relationship repair` does — reverse end first,
        /// nothing else touched. Planned in full before anything is written:
        /// if any mapping could not be removed, nothing is deleted.
        #[arg(long)]
        remove_mapping: bool,
        /// Skip the confirmation prompt for --remove-mapping.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Re-create the links on `<object>.<key>` that its reverse end does not
    /// list — the links written before the reverse was declared, which IDM
    /// never migrates. Each is removed and created again, so it gets a new
    /// edge id; custom `_refProperties` are carried over.
    Relink {
        field: String,
        /// Read and report the plan; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the confirmation prompt.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Convert a generic relationship to mapped storage: durably capture links,
    /// add both mappings in one PUT, restore and verify the exact captured set.
    Map {
        field: String,
        /// Durable capture path; an existing in-place capture resumes this pair.
        #[arg(long)]
        capture: Option<crate::config::UserPath>,
        #[arg(long)]
        dry_run: bool,
        /// Acknowledge that the saved pending request has finished and cannot complete later.
        #[arg(long)]
        acknowledge_request_finished: bool,
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Capture upper-environment links before promoting newly mapped pairs.
    CaptureLinks {
        #[arg(long)]
        lower: String,
        #[arg(long)]
        upper: String,
        #[arg(long)]
        output: crate::config::UserPath,
        /// Explicitly acknowledge an upper pair that has never existed there.
        /// All bound and schema reverse ends must be absent in upper; never bypass Q14.
        #[arg(long = "new-pair")]
        new_pairs: Vec<String>,
        #[command(flatten)]
        options: super::conversion_cli::Options,
    },
    /// Restore links after map or promotion, using the durable capture.
    RestoreLinks {
        file: crate::config::UserPath,
        #[arg(long)]
        tenant: Option<String>,
        #[command(flatten)]
        options: super::conversion_cli::RecoveryOptions,
    },
    /// Explicitly undo an in-place conversion; unavailable for promotion captures.
    RollbackMap {
        file: crate::config::UserPath,
        #[arg(long)]
        tenant: Option<String>,
        #[command(flatten)]
        options: super::conversion_cli::RecoveryOptions,
    },
    /// Remove a config/repo.ds relationship mapping that names no declared
    /// property — the leftover a relationship delete leaves, which makes a
    /// later `set` on that key refuse. Remove only: it never adds or edits a
    /// mapping. Replaces the whole document after backing it up locally, and
    /// confirms by reading it back.
    ///
    /// With `--empty-entry`, instead remove the empty per-type
    /// `genericMapping."managed/<object>"` entry that deleting a type (or its
    /// last mapped property) leaves behind, for a deleted or an existing
    /// type. Only while the entry maps nothing, no mapping names the type, and
    /// the entry is the layout aic generates (anything else is refused).
    Repair {
        /// `<object>.<key>` of the stale mapping; with `--empty-entry`, the
        /// `<object>` whose empty per-type entry to remove.
        field: String,
        /// Remove `<object>`'s empty per-type entry rather than one mapping.
        #[arg(long)]
        empty_entry: bool,
        /// Report the plan; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the confirmation prompt.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Save a relationship's definition — both ends when it declares a
    /// reverse — so `restore` can merge its metadata back after the console
    /// recreates it. Writes JSON to stdout, or to --output.
    Snapshot {
        field: String,
        #[arg(long, short)]
        output: Option<crate::config::UserPath>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Merge a snapshot's `title`, `description` and `query.fields` back
    /// into the relationship ends it names. Changes nothing else.
    Restore {
        file: crate::config::UserPath,
        /// Report what would change; write nothing.
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Read-only: check each relationship pair's two ends agree, in
    /// config/managed and config/repo.ds, and sample links from both ends.
    /// Exits non-zero on any disagreement or unresolved link.
    Verify {
        /// `<object>` for every relationship on it, or `<object>.<key>`.
        target: String,
        /// Links to sample in each direction; 0 checks configuration only.
        #[arg(long, default_value_t = 5)]
        sample: usize,
        /// Records read per end when looking for links (one request).
        #[arg(long, default_value_t = 200)]
        scan: usize,
        /// Also write one test link per pair from the forward end, read it
        /// from the reverse end, and remove it — for pairs with no links yet.
        /// Writes to two existing records, whose hooks fire.
        #[arg(long)]
        probe: bool,
        /// With --probe, skip the confirmation prompt.
        #[command(flatten)]
        force: OperationForce,
        #[arg(long)]
        tenant: Option<String>,
        /// With --probe, confirm writes to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(cmd: ManagedCommand) -> Result<()> {
    match cmd {
        ManagedCommand::List { tenant, json } => list(tenant, json).await,
        ManagedCommand::Diff {
            name,
            file,
            strict,
            tenant,
            json,
        } => managed_diff(name, file, strict, tenant, json).await,
        ManagedCommand::Get {
            name,
            fields,
            hook_sources,
            property,
            schema_only,
            json,
            tenant,
        } => {
            let projection = match (property, schema_only) {
                (Some(property), _) => spec::GetProjection::Property(property),
                (None, true) => spec::GetProjection::Schema,
                (None, false) => spec::GetProjection::Object,
            };
            get(name, fields, hook_sources, projection, json, tenant).await
        }
        ManagedCommand::Object { command } => object(command).await,
        ManagedCommand::Field { command } => field(command).await,
        ManagedCommand::Hook { command } => hook(command).await,
        ManagedCommand::Relationship { command } => relationship(command).await,
    }
}

async fn get(
    name: String,
    fields: Vec<String>,
    hook_sources: bool,
    projection: spec::GetProjection,
    json: bool,
    tenant: Option<String>,
) -> Result<()> {
    let tenant = tenant_for(tenant)?;
    let managed = api::get_managed(&tenant).await?;
    let object = api::object_named(&managed, &name)?.clone();
    let view = spec::prepare_managed_get(object, &name, &fields, hook_sources)
        .map_err(crate::Error::Config)?;
    let whole = projection == spec::GetProjection::Object;
    let storage_key = match &projection {
        spec::GetProjection::Property(property) => Some(property.as_str()),
        _ => None,
    };
    print_json(
        &spec::project_managed_get(view.object, &name, &projection)
            .map_err(crate::Error::Config)?,
    )?;
    for note in get_notes(
        json,
        whole,
        view.fields_shown,
        &view.unmatched_patterns,
        view.elided_hooks,
    ) {
        eprintln!("{note}");
    }
    if json {
        return Ok(());
    }
    print_relationship_storage(&tenant, &managed, &name, storage_key).await;
    Ok(())
}

/// The stderr notes `managed get` prints after the definition. `--json` keeps
/// only the unmatched-pattern warning: the rest is commentary on the output,
/// and a caller that merges the streams must get the definition alone.
/// Object-level hooks sit outside `schema`, so a projection (`whole` false)
/// never shows them and has nothing to report about eliding them.
fn get_notes(
    json: bool,
    whole: bool,
    fields_shown: Option<(usize, usize)>,
    unmatched_patterns: &[String],
    elided_hooks: usize,
) -> Vec<String> {
    let mut notes = Vec::new();
    if !json && let Some((shown, total)) = fields_shown {
        notes.push(format!("showing {shown} of {total} properties"));
    }
    for pattern in unmatched_patterns {
        notes.push(format!("--fields {pattern:?} matched no properties"));
    }
    if !json && whole && elided_hooks > 0 {
        let noun = if elided_hooks == 1 {
            "source"
        } else {
            "sources"
        };
        notes.push(format!(
            "omitted {elided_hooks} inline hook {noun}; pass --hook-sources to include them"
        ));
    }
    notes
}

/// Where each relationship's links are stored, on stderr so the JSON on
/// stdout stays the object itself (and diffable against a repo file).
/// Informational: a failed `repo.ds` read is a note, not the command's error.
async fn print_relationship_storage(
    tenant: &str,
    managed: &serde_json::Value,
    object: &str,
    key: Option<&str>,
) {
    let repo_ds = match api::get_repo_ds(tenant).await {
        Ok(repo_ds) => repo_ds,
        Err(error) => {
            eprintln!(
                "note: could not read config/repo.ds ({error}); relationship storage not shown"
            );
            return;
        }
    };
    // A non-relationship property, or an object with none, has nothing to say.
    let Ok(report) = verify::report(managed, &repo_ds, object, key) else {
        return;
    };
    let lines = verify::storage_lines(&report);
    if lines.is_empty() {
        return;
    }
    eprintln!(
        "relationship storage, as stored in config/repo.ds (`aic managed relationship verify` \
         checks what the running IDM uses):"
    );
    for line in lines {
        eprintln!("  {line}");
    }
}

async fn list(tenant: Option<String>, json_output: bool) -> Result<()> {
    let doc = api::get_managed(&tenant_for(tenant)?).await?;
    let summaries = api::summarize(&doc)?;
    if json_output {
        print_json(&summaries.iter().map(summary_output).collect::<Vec<_>>())?;
    } else {
        let rows = summaries
            .iter()
            .map(|s| {
                vec![
                    s.name.clone(),
                    s.properties.to_string(),
                    join_or_dash(&s.hooks_inline),
                    join_or_dash(&s.hooks_file),
                ]
            })
            .collect::<Vec<_>>();
        print_table(
            &["OBJECT", "PROPERTIES", "SYNCABLE_HOOKS", "FILE_HOOKS"],
            &rows,
        );
    }
    Ok(())
}

async fn object(command: ObjectCommand) -> Result<()> {
    match command {
        ObjectCommand::Create {
            name,
            title,
            description,
            tenant,
            yes,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            let names = object_names(&doc);
            state::validate_object_name(&name, &names, "").map_err(crate::Error::Config)?;
            let title = title.unwrap_or_default();
            let description = description.unwrap_or_default();
            let updated = ops::create_object_in_doc(&doc, &name, &title, &description)
                .map_err(crate::Error::Config)?;
            let mut undo = DiskLog::load_default()?;
            ops::record_create_object_undo(&mut undo, &tenant, &name, &title, &description, &doc)?;
            let expect = [api::ConfigConfirm::ObjectPresent { name: name.clone() }];
            put_managed(&ok, &doc, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {name} created"));
            output(json, api::object_named(&updated, &name)?)
        }
        ObjectCommand::Rename {
            old,
            new,
            force,
            tenant,
            yes,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            let old_object = api::object_named(&doc, &old)?;
            refuse_shipped(old_object, "renamed", force.operation())?;
            state::validate_object_name(&new, &object_names(&doc), &old)
                .map_err(crate::Error::Config)?;
            ops::refuse_mapped_rename(&tenant, &old).await?;
            let (updated, _) =
                ops::rename_object_in_doc(&doc, &old, &new).map_err(crate::Error::Config)?;
            let mut undo = DiskLog::load_default()?;
            ops::record_rename_object_undo(&mut undo, &tenant, &old, &new, &doc)?;
            let expect = [
                api::ConfigConfirm::ObjectAbsent { name: old.clone() },
                api::ConfigConfirm::ObjectPresent { name: new.clone() },
            ];
            put_managed(&ok, &doc, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {old} renamed to {new}"));
            output(json, api::object_named(&updated, &new)?)
        }
        ObjectCommand::Delete {
            name,
            force,
            tenant,
            yes,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            refuse_shipped(
                api::object_named(&doc, &name)?,
                "deleted",
                force.operation(),
            )?;
            let (updated, inbound) =
                ops::delete_object_in_doc(&doc, &name).map_err(crate::Error::Config)?;
            let mut undo = DiskLog::load_default()?;
            ops::record_delete_object_undo(&mut undo, &tenant, &name, &doc)?;
            let expect = [api::ConfigConfirm::ObjectAbsent { name: name.clone() }];
            put_managed(&ok, &doc, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {name} deleted"));
            for change in &inbound {
                eprintln!("also changed: {}", change.describe(&name));
            }
            untrack_hooks(&tenant, &name);
            output(
                json,
                &serde_json::json!({
                    "deleted": name,
                    "changed": changed_objects(&doc, &updated)?,
                }),
            )
        }
    }
}

async fn field(command: FieldCommand) -> Result<()> {
    match command {
        FieldCommand::Add {
            field,
            field_type,
            nullable,
            after,
            attrs,
            tenant,
            yes,
            json,
        } => {
            if attrs.clear_enum {
                return Err(crate::Error::Config(
                    "field add cannot use --clear-enum: a new field has no allowed-value constraint"
                        .into(),
                ));
            }
            if attrs.clear_default {
                return Err(crate::Error::Config(
                    "field add cannot use --clear-default: a new field has no default".into(),
                ));
            }
            let (object, key) = parse_object_key(&field)?;
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let mut doc = api::get_managed(&tenant).await?;
            let previous = api::object_named(&doc, &object)?.clone();
            let enum_values = enum_spec(&attrs)?;
            let spec = spec::AddFieldSpec {
                key,
                field_type: scalar_type(&field_type)?,
                title: attrs.title,
                description: attrs.description,
                required: attrs.required.unwrap_or(false),
                searchable: attrs.searchable.unwrap_or(false),
                viewable: attrs.viewable.unwrap_or(true),
                user_editable: attrs.user_editable.unwrap_or(true),
                enum_values,
                default_value: attrs.default_value,
                nullable,
                after,
            };
            let applied = ops::apply_add_field(&previous, &spec).map_err(crate::Error::Config)?;
            replace_object_write(&ok, &mut doc, &object, &previous, applied.object.clone()).await?;
            confirmation(&format!(
                "managed field {object}.{} added",
                applied.field_key
            ));
            output(json, &applied.object)
        }
        FieldCommand::Edit {
            field,
            attrs,
            force,
            tenant,
            yes,
            json,
        } => {
            ensure_edit_attrs(&attrs)?;
            let (object, key) = parse_object_key(&field)?;
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let mut doc = api::get_managed(&tenant).await?;
            let previous = api::object_named(&doc, &object)?.clone();
            let edit_spec = field_edit_spec(attrs, force.operation())?;
            let removed = state::properties(&previous)
                .and_then(|properties| properties.get(&key))
                .map(|property| ops::removed_enum_values(property, &edit_spec.enum_change))
                .unwrap_or_default();
            ensure_narrowing_consent(&removed, edit_spec.allow_narrowing)?;
            let applied =
                ops::apply_field_edit(&previous, &key, &edit_spec).map_err(crate::Error::Config)?;
            replace_object_write(&ok, &mut doc, &object, &previous, applied.object.clone()).await?;
            if !removed.is_empty() {
                eprintln!(
                    "WARNING: dropped enum values {}. Records still holding them will fail whole-record read-modify-write updates.",
                    removed.join(", ")
                );
            }
            confirmation(&format!(
                "managed field {object}.{} edited",
                applied.field_key
            ));
            output(json, &applied.object)
        }
        FieldCommand::Rename {
            field,
            new_key,
            tenant,
            yes,
            json,
        } => {
            object_field_write(field, tenant, yes, json, |previous, key| {
                ops::apply_rename_field(previous, key, &spec::RenameFieldSpec { new_key })
            })
            .await
        }
        FieldCommand::Delete {
            field,
            tenant,
            yes,
            json,
        } => {
            object_field_write(field, tenant, yes, json, |previous, key| {
                ops::apply_delete_field(previous, key, &spec::DeleteFieldSpec)
            })
            .await
        }
    }
}

async fn hook(command: HookCommand) -> Result<()> {
    let (object, hook, tenant, yes, json, add) = match command {
        HookCommand::Add {
            object,
            hook,
            tenant,
            yes,
            json,
        } => (object, hook, tenant, yes, json, true),
        HookCommand::Delete {
            object,
            hook,
            tenant,
            yes,
            json,
        } => (object, hook, tenant, yes, json, false),
    };
    let tenant = tenant_for(tenant)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let mut doc = api::get_managed(&tenant).await?;
    let previous = api::object_named(&doc, &object)?.clone();
    let updated = if add {
        ops::apply_add_hook(&previous, &object, &hook)
    } else {
        ops::apply_delete_hook(&previous, &object, &hook)
    }
    .map_err(crate::Error::Config)?;
    if updated == previous {
        return Err(crate::Error::Config(format!(
            "hook '{hook}' already exists on {object}"
        )));
    }
    replace_object_write(&ok, &mut doc, &object, &previous, updated.clone()).await?;
    confirmation(&format!(
        "hook {object}.{hook} {}",
        if add { "added" } else { "deleted" }
    ));
    output(json, &updated)
}

async fn relationship(command: RelationshipCommand) -> Result<()> {
    match command {
        RelationshipCommand::Set {
            field,
            target,
            forward,
            reverse,
            reverse_key,
            attrs,
            validate,
            ref_properties,
            tenant,
            yes,
            check,
            json,
        } => {
            if !attrs.enum_values.is_empty()
                || attrs.clear_enum
                || attrs.default_value.is_some()
                || attrs.clear_default
            {
                return Err(crate::Error::Config(
                    "relationship set cannot use --enum, --clear-enum, --default, or --clear-default".into(),
                ));
            }
            let (source_object, key) = parse_object_key(&field)?;
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            let property = state::properties(api::object_named(&doc, &source_object)?)
                .and_then(|p| p.get(&key));
            let parsed = property.and_then(ops::parse_relationship);
            let previous = parsed.clone().map(|parsed| state::PreviousRelationship {
                old_key: key.clone(),
                old_target: parsed.target,
                old_reverse_key: parsed.reverse_key,
            });
            if property.is_some() && parsed.is_none() {
                return Err(crate::Error::Config(format!(
                    "field '{field}' is not a relationship"
                )));
            }
            let existing_title = property
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let existing_description = property
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let existing_required = api::object_named(&doc, &source_object)?
                .pointer("/schema/required")
                .and_then(Value::as_array)
                .is_some_and(|keys| keys.iter().any(|entry| entry.as_str() == Some(&key)));
            let existing_ref_properties =
                property.map(ops::parse_ref_properties).unwrap_or_default();
            let spec = state::RelationshipSpec {
                source_object: source_object.clone(),
                key: key.clone(),
                title: attrs.title.unwrap_or_else(|| existing_title.to_string()),
                description: attrs
                    .description
                    .unwrap_or_else(|| existing_description.to_string()),
                target_object: target,
                forward: cardinality(&forward)?,
                // Every other optional field on this verb merges with what the
                // schema has; the reverse used to default to `none` instead,
                // so a `set` that only changed a title stripped the relationship's
                // reverse declaration. Absent now means keep.
                reverse: match &reverse {
                    Some(value) => reverse_cardinality(value)?,
                    None => match &parsed {
                        Some(parsed) => ops::existing_reverse_cardinality(
                            &doc,
                            &parsed.target,
                            parsed.reverse_key.as_deref(),
                        ),
                        None => state::ReverseCardinality::None,
                    },
                },
                reverse_key: reverse_key
                    .or_else(|| {
                        parsed
                            .as_ref()
                            .and_then(|parsed| parsed.reverse_key.clone())
                    })
                    .unwrap_or_default(),
                searchable: attrs
                    .searchable
                    .unwrap_or_else(|| parsed.as_ref().is_some_and(|value| value.searchable)),
                viewable: attrs
                    .viewable
                    .unwrap_or_else(|| parsed.as_ref().is_none_or(|value| value.viewable)),
                user_editable: attrs
                    .user_editable
                    .unwrap_or_else(|| parsed.as_ref().is_none_or(|value| value.user_editable)),
                required: attrs.required.unwrap_or(existing_required),
                validate: validate
                    .unwrap_or_else(|| parsed.as_ref().is_some_and(|value| value.validate)),
                ref_properties: if ref_properties.is_empty() {
                    existing_ref_properties
                } else {
                    ref_properties
                        .iter()
                        .map(|value| ref_property(value))
                        .collect::<Result<Vec<_>>>()?
                },
            };
            ops::refuse_mapped_retarget(&tenant, &spec, previous.as_ref()).await?;
            let updated = ops::apply_relationship_spec(&doc, &spec, previous.as_ref())
                .map_err(crate::Error::Config)?;
            let ends = activation_ends(&doc, &updated, &source_object, &key, check);
            if changed_objects(&doc, &updated)?.is_empty() {
                // Nothing to confirm, so nothing to write: the confirmed write
                // refuses an empty expectation list rather than guess.
                eprintln!(
                    "relationship {source_object}.{key} is already as requested; nothing written"
                );
                if !check {
                    return output(json, &serde_json::json!({ "changed": [] }));
                }
                // A re-run after a failed `--check` must not pass on the
                // strength of having nothing to write: it checks again.
                let activation = wait_for_ends(&tenant, ends).await;
                let unverified = unverified_ends(&activation);
                output(
                    json,
                    &serde_json::json!({
                        "changed": [],
                        "activation": activation,
                        "activation_verified": unverified.is_empty(),
                    }),
                )?;
                return activation_check(&source_object, &key, &unverified, false);
            }
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Revert relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            let mapping = storage::set_with(
                property.is_none(),
                async || put_managed(&ok, &doc, updated.clone(), &expect).await,
                async || map_created_pair(&ok, &source_object, &key).await,
            )
            .await?;
            confirmation(&format!("relationship {source_object}.{key} set"));
            let activation = wait_for_ends(&tenant, ends).await;
            let one_sided = match relink::reverse_added(&doc, &updated, &source_object, &key) {
                Some((far, far_key)) => {
                    sample_one_sided(&tenant, &updated, &source_object, &key, &far, &far_key).await
                }
                None => None,
            };
            let unverified = unverified_ends(&activation);
            output(
                json,
                &serde_json::json!({
                    "changed": changed_objects(&doc, &updated)?,
                    "activation": activation,
                    "activation_verified": unverified.is_empty(),
                    "one_sided": one_sided,
                    "mapping_verification": mapping,
                }),
            )?;
            if check {
                return activation_check(&source_object, &key, &unverified, true);
            }
            Ok(())
        }
        RelationshipCommand::Delete {
            field,
            remove_mapping,
            force,
            tenant,
            yes,
            json,
        } => {
            if force.operation() && !remove_mapping {
                return Err(crate::Error::Config(
                    "relationship delete takes --force only with --remove-mapping; the delete \
                     itself asks nothing"
                        .into(),
                ));
            }
            let (source_object, key) = parse_object_key(&field)?;
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            let updated = ops::apply_delete_relationship(&doc, &source_object, &key)
                .map_err(crate::Error::Config)?;
            let reverse = api::object_named(&doc, &source_object)
                .ok()
                .and_then(state::properties)
                .and_then(|properties| properties.get(&key))
                .and_then(|property| {
                    let target = ops::relationship_targets(property)
                        .first()?
                        .strip_prefix("managed/")?
                        .to_string();
                    Some((target, ops::declared_reverse_key(property)?.to_string()))
                });
            let mut ends = vec![(source_object.as_str(), key.as_str())];
            if let Some((target, reverse_key)) = &reverse {
                ends.push((target.as_str(), reverse_key.as_str()));
            }
            // Informational only without --remove-mapping: the write itself
            // never needs repo.ds for a removal, so failing to read it must not
            // block one (round 7). With it, the read is the plan.
            let repo_ds = match api::get_repo_ds(&tenant).await {
                Ok(repo_ds) => Some(repo_ds),
                Err(error) if remove_mapping => return Err(error),
                Err(error) => {
                    eprintln!(
                        "note: could not read config/repo.ds ({error}); not checking for \
                         mappings this delete leaves behind"
                    );
                    None
                }
            };
            let leftovers = repo_ds
                .as_ref()
                .map(|repo_ds| verify::delete_leftovers(repo_ds, &ends))
                .unwrap_or_default();
            let removals = match (&repo_ds, remove_mapping) {
                (Some(repo_ds), true) => {
                    let mapped: Vec<(String, String)> = ends
                        .iter()
                        .filter(|(object, key)| leftovers.contains(&format!("{object}.{key}")))
                        .map(|(object, key)| (object.to_string(), key.to_string()))
                        .collect();
                    let plans =
                        repair::plan_sequence(&updated, repo_ds, &mapped).map_err(|reasons| {
                            crate::Error::Config(format!(
                                "refusing relationship delete {field} --remove-mapping; nothing \
                                 was written: {}",
                                reasons.join("; ")
                            ))
                        })?;
                    for plan in &plans {
                        eprintln!(
                            "  then remove {}'s config/repo.ds mapping ({})",
                            plan.label(),
                            plan.mapping.kind
                        );
                        for note in &plan.notes {
                            eprintln!("    note: {note}");
                        }
                    }
                    plans
                }
                _ => Vec::new(),
            };
            let permit = if removals.is_empty() {
                None
            } else {
                let consented = force.operation()
                    || crate::cli::confirm_destructive(
                        "relationship delete",
                        &format!(
                            "Delete {field} and remove {} config/repo.ds mapping(s)? The whole \
                             document is replaced for each; the current one is backed up \
                             locally first",
                            removals.len()
                        ),
                        "--force",
                    )?;
                Some(repair::authorize(false, consented).ok_or_else(|| {
                    crate::Error::Config(
                        "relationship delete not confirmed; nothing written".into(),
                    )
                })?)
            };
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Restore relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            if let Err(error) = put_managed(&ok, &doc, updated.clone(), &expect).await {
                if permit.is_none() {
                    return Err(error);
                }
                return Err(crate::Error::Config(format!(
                    "{error}. The delete of {field} may or may not have landed, and no \
                     config/repo.ds mapping was touched; check with `aic managed relationship \
                     verify {source_object}` before re-running"
                )));
            }
            confirmation(&format!("relationship {source_object}.{key} deleted"));
            let mut removed = Vec::new();
            if let Some(permit) = &permit {
                for (index, plan) in removals.iter().enumerate() {
                    let label = plan.label();
                    let outcome = match send_repo_change(&ok, plan, permit).await {
                        Ok(applied) => {
                            repair_readback(&label, &repair_check(&plan.object, false), &applied)
                                .map(|()| applied)
                        }
                        Err(failure) => Err(repair_failure(
                            &label,
                            &repair_check(&plan.object, false),
                            failure,
                        )),
                    };
                    match outcome {
                        Ok(applied) => {
                            eprintln!(
                                "removed {label}'s config/repo.ds mapping (confirmed by re-read; \
                                 backup {})",
                                applied.backup.display()
                            );
                            removed.push(label);
                        }
                        Err(error) => {
                            // §5's batch rule: say what landed before what failed.
                            // The failing removal's own state is whatever its
                            // error says (it may have landed unconfirmed), so
                            // it is not listed as still mapped; only the ones
                            // after it are known untouched.
                            let labels: Vec<_> =
                                removals[index..].iter().map(repair::Plan::label).collect();
                            return Err(crate::Error::Config(removal_failure_message(
                                &field,
                                &source_object,
                                &removed,
                                &labels,
                                &error,
                            )));
                        }
                    }
                }
            } else if !leftovers.is_empty() {
                eprintln!(
                    "note: config/repo.ds still maps {}. Recreating the same key with the \
                     same target reused the mapping when measured; aic refuses one with \
                     another target. `--remove-mapping` removes them with the delete, or \
                     `aic managed relationship repair <object>.<key>` one at a time \
                     (docs/api/10-managed-objects.md, \"Relationship storage\")",
                    leftovers.join(", ")
                );
            }
            output(
                json,
                &serde_json::json!({
                    "changed": changed_objects(&doc, &updated)?,
                    "mappings_removed": removed,
                }),
            )
        }
        RelationshipCommand::Verify {
            target,
            sample,
            scan,
            probe,
            force,
            tenant,
            yes,
            json,
        } => {
            let probe = probe.then_some(ProbeRequest {
                force: force.operation(),
                yes,
            });
            relationship_verify(target, sample, scan, probe, tenant, json).await
        }
        RelationshipCommand::Snapshot {
            field,
            output,
            tenant,
        } => {
            let (object, key) = parse_object_key(&field)?;
            let tenant = tenant_for(tenant)?;
            let managed = api::get_managed(&tenant).await?;
            let property_of = |object: &str, key: &str| {
                api::object_named(&managed, object)
                    .ok()
                    .and_then(state::properties)
                    .and_then(|properties| properties.get(key))
                    .cloned()
            };
            let property = property_of(&object, &key)
                .filter(state::is_relationship_property)
                .ok_or_else(|| crate::Error::Config(format!("{field} is not a relationship")))?;
            let mut ends = vec![spec::SnapshotEnd {
                object: object.clone(),
                key: key.clone(),
                property: property.clone(),
            }];
            if let Some(reverse_key) = ops::declared_reverse_key(&property)
                && let [target] = ops::relationship_targets(&property)[..]
                && let Some(target) = target.strip_prefix("managed/")
                && let Some(reverse) = property_of(target, reverse_key)
            {
                ends.push(spec::SnapshotEnd {
                    object: target.to_string(),
                    key: reverse_key.to_string(),
                    property: reverse,
                });
            }
            let text = serde_json::to_string_pretty(&spec::snapshot_document(&ends))?;
            match output {
                Some(path) => {
                    std::fs::write(&path, format!("{text}\n")).map_err(|error| {
                        crate::Error::Config(format!("write {}: {error}", path.display()))
                    })?;
                    eprintln!(
                        "saved {} to {}",
                        ends.iter()
                            .map(|end| format!("{}.{}", end.object, end.key))
                            .collect::<Vec<_>>()
                            .join(" and "),
                        path.display()
                    );
                }
                None => println!("{text}"),
            }
            Ok(())
        }
        RelationshipCommand::Restore {
            file,
            dry_run,
            tenant,
            yes,
            json,
        } => {
            let text = std::fs::read_to_string(&file).map_err(|error| {
                crate::Error::Config(format!("read {}: {error}", file.display()))
            })?;
            let parsed: Value = serde_json::from_str(&text).map_err(|error| {
                crate::Error::Config(format!("parse {}: {error}", file.display()))
            })?;
            let ends = spec::parse_snapshot(&parsed).map_err(crate::Error::Config)?;
            let tenant = tenant_for(tenant)?;
            let doc = api::get_managed(&tenant).await?;
            let mut updated = doc.clone();
            let mut changes = Vec::new();
            for end in &ends {
                let (next, mut more) = ops::apply_relationship_metadata(
                    &updated,
                    &end.object,
                    &end.key,
                    &end.property,
                )
                .map_err(crate::Error::Config)?;
                updated = next;
                changes.append(&mut more);
            }
            for change in &changes {
                eprintln!("{}{change}", if dry_run { "would set " } else { "set " });
            }
            if changes.is_empty() {
                eprintln!("nothing to restore: the metadata already matches the snapshot");
            } else if !dry_run {
                let ok = ensure_prod_confirmed(&tenant, yes)?;
                record_config_undo(
                    &tenant,
                    &doc,
                    &updated,
                    format!("Revert relationship restore from {}", file.display()),
                )?;
                let expect = relationship_expectations(&doc, &updated)?;
                put_managed(&ok, &doc, updated.clone(), &expect).await?;
                confirmation("relationship metadata restored");
            }
            output(
                json,
                &serde_json::json!({ "changes": changes, "dry_run": dry_run }),
            )
        }
        RelationshipCommand::Relink {
            field,
            dry_run,
            force,
            tenant,
            yes,
            json,
        } => relationship_relink(field, dry_run, force.operation(), tenant, yes, json).await,
        RelationshipCommand::Map {
            field,
            capture,
            dry_run,
            acknowledge_request_finished,
            force,
            tenant,
            yes,
            json,
        } => {
            super::conversion_cli::map(
                field,
                capture.map(Into::into),
                tenant,
                super::conversion_cli::Flags {
                    dry_run,
                    acknowledge_request_finished,
                    force: force.operation(),
                    yes,
                    json,
                },
            )
            .await
        }
        RelationshipCommand::CaptureLinks {
            lower,
            upper,
            output,
            new_pairs,
            options,
        } => {
            super::conversion_cli::capture_links(lower, upper, output.into(), new_pairs, options)
                .await
        }
        RelationshipCommand::RestoreLinks {
            file,
            tenant,
            options,
        } => super::conversion_cli::restore_links(file.into(), tenant, options).await,
        RelationshipCommand::RollbackMap {
            file,
            tenant,
            options,
        } => super::conversion_cli::rollback_map(file.into(), tenant, options).await,
        RelationshipCommand::Repair {
            field,
            empty_entry,
            dry_run,
            force,
            tenant,
            yes,
            json,
        } => {
            relationship_repair(
                field,
                empty_entry,
                dry_run,
                force.operation(),
                tenant,
                yes,
                json,
            )
            .await
        }
    }
}

async fn managed_diff(
    name: String,
    file: crate::config::UserPath,
    strict: bool,
    tenant: Option<String>,
    json: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(&file)
        .map_err(|error| crate::Error::Config(format!("read {}: {error}", file.display())))?;
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|error| crate::Error::Config(format!("parse {}: {error}", file.display())))?;
    let repo = diff::select_object(&parsed, &name).map_err(crate::Error::Config)?;
    let tenant = tenant_for(tenant)?;
    let managed = api::get_managed(&tenant).await?;
    let remote = api::object_named(&managed, &name)?.clone();
    let (remote, repo, mut rules) = if strict {
        (remote, repo, Vec::new())
    } else {
        let (remote, mut rules) = diff::normalise(&remote);
        let (repo, repo_rules) = diff::normalise(&repo);
        rules.extend(repo_rules);
        (remote, repo, rules)
    };
    rules.sort_by_key(|rule| *rule as u8);
    rules.dedup();
    let differences = diff::differences(&remote, &repo);
    if json {
        print_json(&serde_json::json!({
            "object": name,
            "normalised": rules,
            "differences": differences,
        }))?;
    } else {
        for rule in &rules {
            eprintln!("normalised: {}", rule.describe());
        }
        let render = |value: &Value| serde_json::to_string(value).unwrap_or_default();
        for difference in &differences {
            match difference {
                diff::Difference::TenantOnly { path, tenant } => {
                    println!("- {path}  tenant only: {}", render(tenant));
                }
                diff::Difference::RepoOnly { path, repo } => {
                    println!("+ {path}  repo only: {}", render(repo));
                }
                diff::Difference::Changed { path, tenant, repo } => {
                    println!(
                        "~ {path}  tenant {} != repo {}",
                        render(tenant),
                        render(repo)
                    );
                }
            }
        }
    }
    if differences.is_empty() {
        eprintln!("managed/{name} matches {}", file.display());
        Ok(())
    } else {
        Err(crate::Error::Config(format!(
            "managed/{name} differs from {} in {} place(s)",
            file.display(),
            differences.len()
        )))
    }
}

/// How long `relationship set` waits for a new end to go live. Measured
/// flips took under a second; this bounds a tenant having a bad day.
const ACTIVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const ACTIVATION_POLL: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Serialize)]
struct ActivationOutput {
    end: String,
    /// `live`, `timeout`, `no_records` (nothing to probe through) or `error`.
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// The ends a `relationship set` could not confirm live, for `--check` and
/// the JSON's `activation_verified`: anything but `live` — a type with no
/// records to probe through is unverified, not exempt.
fn unverified_ends(activation: &[ActivationOutput]) -> Vec<String> {
    activation
        .iter()
        .filter(|entry| entry.outcome != "live")
        .map(|entry| format!("{} ({})", entry.end, entry.outcome))
        .collect()
}

/// Waits for each relationship end `updated` declares that `doc` did not:
/// a write through one before it is live answers 200 and is lost (measured
/// 2026-10-31). Never fails the command — the schema write has landed — but
/// says loudly when it could not confirm.
/// The relationship ends `relationship set` waits on: every end the write
/// introduces, and with `--check` also both ends of the relationship being
/// set, so an edit to an existing end — or a re-run with nothing to write —
/// is probed rather than passing on an empty list. On an existing end the
/// probe says little more than that it answers, but on an empty type it
/// reports `no_records`, which is exactly what a failed `--check` re-run must
/// not paper over.
fn activation_ends(
    before: &Value,
    after: &Value,
    object: &str,
    key: &str,
    check: bool,
) -> Vec<relink::NewEnd> {
    let mut ends = relink::new_ends(before, after);
    if !check {
        return ends;
    }
    let reverse = api::object_named(after, object)
        .ok()
        .and_then(state::properties)
        .and_then(|properties| properties.get(key))
        .and_then(|property| {
            let target = ops::relationship_targets(property)
                .first()?
                .strip_prefix("managed/")?
                .to_string();
            Some((target, ops::declared_reverse_key(property)?.to_string()))
        });
    for (object, key) in std::iter::once((object.to_string(), key.to_string())).chain(reverse) {
        let end = relink::NewEnd { object, key };
        if !ends.contains(&end) {
            ends.push(end);
        }
    }
    ends
}

/// The report for `delete --remove-mapping` stopping at `pending[0]`. That
/// removal's state is whatever `error` says — it may have landed
/// unconfirmed — so it is not called still mapped; only the ones after it
/// are known untouched.
fn removal_failure_message(
    field: &str,
    object: &str,
    removed: &[String],
    pending: &[String],
    error: &crate::Error,
) -> String {
    let (failed, untouched) = pending
        .split_first()
        .map_or(("", &[][..]), |(f, rest)| (f.as_str(), rest));
    format!(
        "{field} was deleted{}, but removing {failed}'s mapping failed: {error}. {}Run `aic \
         managed relationship verify {object}` to see what config/repo.ds maps now, then `aic \
         managed relationship repair` what remains, in the order {}",
        if removed.is_empty() {
            String::new()
        } else {
            format!(" and {} unmapped", removed.join(", "))
        },
        if untouched.is_empty() {
            String::new()
        } else {
            format!("Not attempted: {}. ", untouched.join(", "))
        },
        pending.join(", ")
    )
}

fn activation_check(object: &str, key: &str, unverified: &[String], written: bool) -> Result<()> {
    if unverified.is_empty() {
        return Ok(());
    }
    Err(crate::Error::Config(format!(
        "relationship {object}.{key} {}, but activation is unverified for {}",
        if written {
            "was written"
        } else {
            "needed no write"
        },
        unverified.join(", ")
    )))
}

/// Conversion uses the same read-only activation wait as set. A capture can
/// survive a later declaration repair, whose routes may not yet be live.
pub(super) async fn require_live_ends(tenant: &str, ends: Vec<relink::NewEnd>) -> Result<()> {
    let activation = wait_for_ends(tenant, ends).await;
    let unverified = unverified_ends(&activation);
    if !unverified.is_empty() {
        return Err(crate::Error::Config(format!(
            "refusing record writes: relationship ends not confirmed live: {}; keep the capture and retry once the declarations are live",
            unverified.join(", ")
        )));
    }
    Ok(())
}

async fn wait_for_ends(tenant: &str, ends: Vec<relink::NewEnd>) -> Vec<ActivationOutput> {
    let mut out = Vec::new();
    for end in ends {
        let label = format!("{}.{}", end.object, end.key);
        let report = |outcome, seconds, detail| ActivationOutput {
            end: label.clone(),
            outcome,
            seconds,
            detail,
        };
        let id = match api::any_record_id(tenant, &end.object).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                eprintln!(
                    "note: managed/{} has no records to probe {label} through; wait a few \
                     seconds before writing through it — a write made before it is live is lost",
                    end.object
                );
                out.push(report("no_records", None, None));
                continue;
            }
            Err(error) => {
                eprintln!(
                    "warning: could not probe {label} ({error}); wait before writing through it"
                );
                out.push(report("error", None, Some(error.to_string())));
                continue;
            }
        };
        let started = std::time::Instant::now();
        let entry = loop {
            let probe = api::probe_relationship(tenant, &end.object, &id, &end.key).await;
            match relink::activation(&probe) {
                Ok(relink::Activation::Live) => {
                    let seconds = started.elapsed().as_secs_f64();
                    eprintln!("{label} is live ({seconds:.1}s)");
                    break report("live", Some(seconds), None);
                }
                Ok(relink::Activation::Pending) if started.elapsed() < ACTIVATION_TIMEOUT => {
                    tokio::time::sleep(ACTIVATION_POLL).await;
                }
                Ok(relink::Activation::Pending) => {
                    eprintln!(
                        "warning: {label} was not live after {}s; a write through it before it \
                         is live answers 200 and is lost. Re-run `aic managed relationship \
                         verify` before relying on it",
                        ACTIVATION_TIMEOUT.as_secs()
                    );
                    break report("timeout", Some(started.elapsed().as_secs_f64()), None);
                }
                Err(detail) => {
                    eprintln!("warning: could not confirm {label} is live ({detail})");
                    break report("error", None, Some(detail));
                }
            }
        };
        out.push(entry);
    }
    out
}

/// Links sampled when a reverse is added to a relationship that has links.
const ONE_SIDED_SCAN: usize = 200;
const ONE_SIDED_SAMPLE: usize = 20;

#[derive(Serialize)]
struct OneSidedOutput {
    sampled: usize,
    one_sided: usize,
}

/// After a reverse is added to `object.key`, samples its existing links: each
/// was written when the pair had no reverse, so the new far end does not list
/// it, and IDM never migrates it. Informational — the write has landed.
async fn sample_one_sided(
    tenant: &str,
    updated: &Value,
    object: &str,
    key: &str,
    far: &str,
    far_key: &str,
) -> Option<OneSidedOutput> {
    let far_array = end_is_array(updated, far, far_key);
    let sampled = async {
        let records = api::records_with(tenant, object, key, ONE_SIDED_SCAN).await?;
        let far_collection = format!("managed/{far}");
        let near_collection = format!("managed/{object}");
        let mut links = Vec::new();
        for record in &records {
            links.extend(verify::links_of(record, key).map_err(crate::Error::Config)?);
        }
        let mut outcome = OneSidedOutput {
            sampled: 0,
            one_sided: 0,
        };
        for link in links
            .into_iter()
            .filter(|link| link.to_collection == far_collection)
            .take(ONE_SIDED_SAMPLE)
        {
            let far_record =
                api::far_end(tenant, far, &link.to_id, far_key, far_array, &link.from_id).await?;
            let far_links = far_record
                .map(|record| verify::links_of(&record, far_key))
                .transpose()
                .map_err(crate::Error::Config)?;
            outcome.sampled += 1;
            if verify::judge(&link, &near_collection, far_links.as_deref())
                == verify::LinkOutcome::Missing
            {
                outcome.one_sided += 1;
            }
        }
        Ok::<_, crate::Error>(outcome)
    }
    .await;
    match sampled {
        Ok(outcome) => {
            if outcome.one_sided > 0 {
                eprintln!(
                    "warning: {} of {} sampled links on {object}.{key} are not listed by \
                     {far}.{far_key}: links written before the reverse existed are not \
                     migrated. `aic managed relationship relink {object}.{key}` re-creates them",
                    outcome.one_sided, outcome.sampled
                );
            }
            Some(outcome)
        }
        Err(error) => {
            eprintln!(
                "note: could not sample {object}.{key} for links the new reverse does not list \
                 ({error}); `aic managed relationship relink {object}.{key} --dry-run` checks"
            );
            None
        }
    }
}

fn end_is_array(doc: &Value, object: &str, key: &str) -> bool {
    api::object_named(doc, object)
        .ok()
        .and_then(state::properties)
        .and_then(|properties| properties.get(key))
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str)
        == Some("array")
}

/// `object.key` as `config/managed` declares it now, for judging a recovery
/// entry; `None` when it is no longer a relationship.
fn current_shape(doc: &Value, object: &str, key: &str) -> Option<recovery::Shape> {
    let property = api::object_named(doc, object)
        .ok()
        .and_then(state::properties)
        .and_then(|properties| properties.get(key))
        .filter(|property| state::is_relationship_property(property))?;
    Some(recovery::Shape {
        array: end_is_array(doc, object, key),
        targets: ops::relationship_targets(property)
            .into_iter()
            .map(str::to_string)
            .collect(),
    })
}

#[derive(Serialize)]
struct RelinkOutput<'a> {
    #[serde(flatten)]
    plan: &'a relink::Plan,
    /// Recovery-journal entries this run re-creates (or, with `--dry-run`,
    /// would), before the plan above.
    to_restore: Vec<String>,
    /// Links an earlier, interrupted run removed, re-created from the
    /// recovery journal.
    restored: Vec<String>,
    relinked: Vec<relink::Edge>,
    /// Re-created, but the far end did not list the new edge when re-read.
    unconfirmed: Vec<relink::Edge>,
    /// Changed on the tenant between the plan and its turn; not written.
    changed: Vec<relink::Edge>,
    /// Recovery-journal entries that still need a person.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attention: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// A record's links of `key` and its `_rev`; no links and no `_rev` when the
/// record is gone.
/// What the running IDM uses to store `end`, judged from record `id`, which
/// was just seen holding the link `edge`; `None` when that can no longer be
/// told.
async fn runtime_of(
    tenant: &str,
    end: &verify::End,
    id: &str,
    edge: Option<&str>,
) -> Result<Option<verify::Runtime>> {
    let found = api::present_filter_finds(tenant, &end.object, id, &end.key).await?;
    let reread = if found {
        None
    } else {
        Some(read_edges(tenant, &end.object, id, &end.key).await?.0)
    };
    Ok(runtime_verdict(end, found, edge, reread.as_deref()))
}

/// The pure half of [`runtime_of`].
///
/// A `pr` hit needs no control: some link was there to find. A miss is only
/// evidence while the record still holds **the same edge** that justified
/// the query: a link removed by another writer also misses, on a mapped end
/// as on a generic one, and a replacement link created after the query says
/// nothing about what the query saw. So a miss is judged only when the
/// re-read still lists `edge`, and is "unknown" otherwise — including when
/// the edge id was never known.
fn runtime_verdict(
    end: &verify::End,
    found: bool,
    edge: Option<&str>,
    reread: Option<&[relink::Edge]>,
) -> Option<verify::Runtime> {
    if found {
        return Some(verify::Runtime::judge(end, true));
    }
    let edge = edge?;
    reread?
        .iter()
        .any(|held| held.link.edge_id.as_deref() == Some(edge))
        .then(|| verify::Runtime::judge(end, false))
}

async fn read_edges(
    tenant: &str,
    object: &str,
    id: &str,
    key: &str,
) -> Result<(Vec<relink::Edge>, Option<String>)> {
    Ok(match api::record_with(tenant, object, id, key).await? {
        Some(record) => (
            relink::edges_of(&record, key).map_err(crate::Error::Config)?,
            relink::rev_of(&record),
        ),
        None => (Vec::new(), None),
    })
}

/// What the recovery journal holds for `tenant`, judged against the tenant:
/// settled entries are forgotten, `restore` holds the relink entries for
/// `field` that a run of it should re-create, and `attention` says what is
/// left for a person.
struct Reconciled {
    restore: Vec<recovery::Pending>,
    attention: Vec<String>,
}

async fn reconcile(tenant: &str, managed: &Value, field: Option<&str>) -> Result<Reconciled> {
    let mut out = Reconciled {
        restore: Vec::new(),
        attention: Vec::new(),
    };
    for pending in recovery::pending(tenant)? {
        let label = pending.label();
        let since = &pending.started_at;
        // Shape first: a field that is no longer this relationship may not
        // read as one, and one entry that cannot be read must not stop the
        // others being judged.
        let shape = current_shape(managed, &pending.object, &pending.key);
        let verdict = if recovery::fits(&pending, shape.as_ref()) {
            match read_edges(tenant, &pending.object, &pending.from_id, &pending.key).await {
                Ok((current, _)) => recovery::judge(&pending, shape.as_ref(), &current),
                Err(error) => {
                    out.attention.push(format!(
                        "{label}: recorded at {since}; the record could not be read ({error}), \
                         so the entry is kept and not acted on"
                    ));
                    continue;
                }
            }
        } else {
            recovery::Recovery::Reshaped
        };
        match verdict {
            recovery::Recovery::Settled => recovery::clear(&pending.id)?,
            recovery::Recovery::Reshaped => out.attention.push(format!(
                "{label}: recorded at {since} while {} was {} to {}; it has changed shape since, \
                 so the entry is not acted on. Check the record by hand, then remove the entry \
                 from {}",
                pending.field(),
                if pending.array { "has-many" } else { "has-one" },
                pending.to_collection,
                recovery::file().display()
            )),
            recovery::Recovery::Altered => out.attention.push(format!(
                "{label}: removed by a relink interrupted at {since}; the record links that \
                 target again but without the custom properties it had ({}). Restore them by \
                 hand if they matter, then remove the entry from {}",
                Value::Object(pending.properties.clone()),
                recovery::file().display()
            )),
            recovery::Recovery::Lost if field == Some(pending.field().as_str()) => {
                out.restore.push(pending);
            }
            recovery::Recovery::Lost => out.attention.push(format!(
                "{label}: removed by a relink interrupted at {since} and not re-created; run \
                 `aic managed relationship relink {}` to restore it",
                pending.field()
            )),
            recovery::Recovery::LostAndOccupied => out.attention.push(format!(
                "{label}: removed by a relink interrupted at {since}; the property now holds \
                 another link, so it is not restored over it. Restore it by hand if it should \
                 win, then remove the entry from {}",
                recovery::file().display()
            )),
            recovery::Recovery::Leftover { edge_id } => out.attention.push(format!(
                "{label}: a `verify --probe` test link (edge {edge_id}) interrupted at {since} \
                 is still there; remove it with DELETE /openidm/managed/{}/{}/{}/{edge_id} \
                 (has-many) or a PATCH removing /{} (has-one)",
                pending.object, pending.from_id, pending.key, pending.key
            )),
            recovery::Recovery::Ambiguous => out.attention.push(format!(
                "{label}: a `verify --probe` interrupted at {since} before reading its test link \
                 back, and the record links that target; check whether it is the probe's, then \
                 remove the entry from {}",
                recovery::file().display()
            )),
        }
    }
    for line in &out.attention {
        eprintln!("recovery: {line}");
    }
    Ok(out)
}

/// A complete projected scan, on both ends: `pr` cannot see generic links.
async fn mapping_empty(tenant: &str, pair: &verify::PairReport) -> Result<()> {
    let forward = api::all_records_with(tenant, &pair.forward.object, &pair.forward.key).await?;
    let reverse = match &pair.reverse {
        Some(end) => api::all_records_with(tenant, &end.object, &end.key).await?,
        None => Vec::new(),
    };
    let mut ends = vec![(pair.forward.key.as_str(), forward.as_slice())];
    if let Some(end) = &pair.reverse {
        ends.push((end.key.as_str(), reverse.as_slice()));
    }
    let count = storage::link_count(&ends).map_err(crate::Error::Config)?;
    storage::require_empty(count).map_err(crate::Error::Config)
}

fn print_mapping_plan(plan: &repair::Plan) {
    if let Some(addition) = plan.addition() {
        eprintln!(
            "map {}: forward reference uses {}",
            plan.label(),
            addition.column
        );
        for (object, entry) in &addition.added_types {
            eprintln!("  add per-type genericMapping managed/{object}: {entry}");
        }
        for entry in &addition.entries {
            eprintln!("  add {}.{}: {}", entry.object, entry.key, entry.value);
        }
        eprintln!("  both ends go in one PUT; pause all link writers until this command finishes");
    }
}

/// The second half of a new `set`. A failure before the repo.ds write leaves
/// the consistent generic declaration available for `map` to finish.
async fn map_created_pair(ok: &WriteOk<'_>, object: &str, key: &str) -> Result<VerificationOutput> {
    let field = format!("{object}.{key}");
    let mut generic = false;
    let prepared = async {
        let managed = api::get_managed(ok.tenant).await?;
        let repo = api::get_repo_ds(ok.tenant).await?;
        // Recreating a property over its matching leftover mapping is an
        // existing supported path. Do not change that pair's storage.
        let pair = storage::pair(&managed, &repo, object, key).map_err(crate::Error::Config)?;
        if storage::mode(&pair) == storage::Mode::Mapped {
            eprintln!("{field} already has matching mappings; checking for orphaned values");
            return Ok(None);
        }
        generic = true;
        let plan = repair::plan_add(&managed, &repo, object, key).map_err(crate::Error::Config)?;
        print_mapping_plan(&plan);
        Ok::<_, crate::Error>(Some(plan))
    }
    .await
    .map_err(|error| {
        crate::Error::Config(format!(
            "config/managed was written, but mapping {field} was not attempted: {error}. {} \
             Run `aic managed relationship verify {field}`; if generic, finish with \
             `aic managed relationship map {field} --force` before writing links",
            if generic {
                "The pair remains a consistent generic pair unless another writer changed it."
            } else {
                "Storage could not be established from the current documents."
            }
        ))
    })?;
    if let Some(plan) = prepared {
        let permit = repair::authorize(false, true)
            .ok_or_else(|| crate::Error::Config("mapping authorization failed".into()))?;
        apply_mapping(ok, &plan, &permit).await?;
    } else {
        check_orphans(ok.tenant, object, key, "existing mapping").await?;
    }
    let verification =
        collect_verification(field, 5, 200, None, Some(ok.tenant.to_string())).await?;
    verification_check(&verification)?;
    eprintln!(
        "mapping configuration verified; runtime storage mode is untested without links (run verify --probe later if appropriate)"
    );
    Ok(verification)
}

async fn apply_mapping(
    ok: &WriteOk<'_>,
    plan: &repair::Plan,
    permit: &repair::WritePermit,
) -> Result<repair::Applied> {
    let field = plan.label();
    let applied = send_repo_change(ok, plan, permit).await.map_err(|failure| crate::Error::Config(match failure {
        repair::Failure::NotWritten(error) => format!(
            "mapping {field} was not written: {error}. This command sent no mapping PUT. \
             If no other writer changed it, the pair remains consistent and generic. \
             Verify its current state, then finish with `aic managed relationship map {field} --force` \
             after pausing all link writers"),
        repair::Failure::SendFailed { error, backup } => format!(
            "mapping {field} PUT failed ({error}); whether it landed is unknown. \
             config/managed remains declared. Run `aic managed relationship verify {field}` \
             before retrying `relationship map`; backup: {}", backup.display()),
    }))?;
    eprintln!("  repo.ds backup: {}", applied.backup.display());
    if applied.readback != repair::Readback::Confirmed {
        return Err(crate::Error::Config(format!(
            "mapping {field} was not confirmed ({:?}); storage is uncertain. Run \
             `aic managed relationship verify {field}` before retrying map; backup: {}",
            applied.readback,
            applied.backup.display()
        )));
    }
    let column = plan
        .addition()
        .map_or("unknown column", |addition| addition.column.as_str());
    check_orphans(ok.tenant, &plan.object, &plan.key, column).await?;
    eprintln!("mapped {field} (config confirmed by re-read; no orphaned values found)");
    if let Some(warning) = plan.addition().and_then(storage::activation_warning) {
        eprintln!("{warning}");
    }
    Ok(applied)
}

async fn check_orphans(tenant: &str, object: &str, key: &str, column: &str) -> Result<()> {
    let records = api::present_records(tenant, object, key)
        .await
        .map_err(|error| {
            crate::Error::Config(format!(
                "mapping {object}.{key} is installed, but orphan checking failed: {error}. \
         Keep link writers paused and query /{key} pr before using it"
            ))
        })?;
    storage::no_orphans(object, key, column, &records).map_err(crate::Error::Config)
}

async fn relationship_repair(
    field: String,
    empty_entry: bool,
    dry_run: bool,
    force: bool,
    tenant: Option<String>,
    yes: bool,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant)?;
    let managed = api::get_managed(&tenant).await?;
    let repo_ds = api::get_repo_ds(&tenant).await?;
    let (object, plan) = if empty_entry {
        if field.contains('.') {
            return Err(crate::Error::Config(format!(
                "--empty-entry takes an object name, not {field:?}; it removes the object's \
                 per-type entry, not one mapping"
            )));
        }
        let plan =
            repair::plan_remove_entry(&managed, &repo_ds, &field).map_err(crate::Error::Config)?;
        (field.clone(), plan)
    } else {
        let (object, key) = parse_object_key(&field)?;
        let plan = repair::plan(&managed, &repo_ds, &object, &key).map_err(crate::Error::Config)?;
        (object, plan)
    };
    let mapping = match (&plan.mapping.resource_path, &plan.mapping.property_name) {
        (Some(path), Some(through)) => format!("{} of {path} through {through}", plan.mapping.kind),
        (Some(path), None) => format!("{} to {path}", plan.mapping.kind),
        _ => plan.mapping.kind.clone(),
    };
    if empty_entry {
        eprintln!("repair {field}: remove its empty config/repo.ds per-type entry ({mapping})");
    } else {
        eprintln!("repair {field}: remove its config/repo.ds mapping ({mapping})");
    }
    for note in &plan.notes {
        eprintln!("  note: {note}");
    }
    if dry_run {
        return output(json, &serde_json::json!({ "plan": &plan, "dry_run": true }));
    }
    let consented = force
        || crate::cli::confirm_destructive(
            "relationship repair",
            &format!(
                "Remove {} from config/repo.ds? The whole document is replaced; \
                 the current one is backed up locally first",
                if empty_entry {
                    format!("{field}'s empty per-type entry")
                } else {
                    format!("{field}'s mapping")
                }
            ),
            "--force",
        )?;
    let Some(permit) = repair::authorize(dry_run, consented) else {
        return Err(crate::Error::Config(
            "relationship repair not confirmed".into(),
        ));
    };
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    match send_repo_change(&ok, &plan, &permit).await {
        Err(failure) => Err(repair_failure(
            &field,
            &repair_check(&object, empty_entry),
            failure,
        )),
        Ok(applied) => {
            eprintln!("  backup: {}", applied.backup.display());
            if json {
                print_json(&serde_json::json!({ "plan": &plan, "applied": &applied }))?;
            }
            repair_readback(&field, &repair_check(&object, empty_entry), &applied)?;
            // No undo entry: the backup above is the way back.
            if empty_entry {
                eprintln!("removed {field}'s per-type config/repo.ds entry (confirmed by re-read)");
            } else {
                eprintln!("removed {field}'s config/repo.ds mapping (confirmed by re-read)");
            }
            Ok(())
        }
    }
}

/// One guarded mapping addition/removal over the tenant, polling the
/// read-back for 30 s.
async fn send_repo_change(
    ok: &crate::cli::WriteOk<'_>,
    plan: &repair::Plan,
    permit: &repair::WritePermit,
) -> std::result::Result<repair::Applied, repair::Failure> {
    let delays = [std::time::Duration::from_secs(3); 10];
    repair::apply_with(
        plan,
        permit,
        async || {
            if let Some(addition) = plan.addition() {
                mapping_empty(ok.tenant, &addition.pair).await?;
            }
            // The census may be long: re-read schema after it, immediately
            // beside the repo.ds recheck rather than before the scan.
            api::get_managed(ok.tenant).await
        },
        async || api::get_repo_ds(ok.tenant).await,
        |document| {
            crate::access::ops::backup_config_document(
                "repo.ds",
                ok.tenant,
                document,
                chrono::Utc::now(),
            )
        },
        async |document| api::put_repo_ds(ok.tenant, permit, document, ok.confirmed_prod).await,
        &delays,
    )
    .await
}

/// What to run to see how a repo.ds write settled. `relationship verify` reads
/// the object out of `config/managed`, so it refuses a type that is no longer
/// declared, which is the usual subject of `--empty-entry`: that path is pointed
/// at the repair's own dry run, which reports whether the entry is still there.
fn repair_check(object: &str, empty_entry: bool) -> String {
    if empty_entry {
        format!(
            "`aic managed relationship repair {object} --empty-entry --dry-run` \
             (`relationship verify` refuses a type config/managed no longer declares)"
        )
    } else {
        format!("`aic managed relationship verify {object}`")
    }
}

fn repair_failure(field: &str, check: &str, failure: repair::Failure) -> crate::Error {
    crate::Error::Config(match failure {
        repair::Failure::NotWritten(message) => {
            format!("relationship repair {field}: {message}")
        }
        repair::Failure::SendFailed { error, backup } => format!(
            "relationship repair {field}: the write failed ({error}), and whether it landed is \
             unknown. Run {check} to see. The document it \
             replaced is at {}",
            backup.display()
        ),
    })
}

/// `Ok` only when a read-back showed exactly the document sent.
fn repair_readback(field: &str, check: &str, applied: &repair::Applied) -> Result<()> {
    match &applied.readback {
        repair::Readback::Confirmed => Ok(()),
        repair::Readback::Unchanged => Err(crate::Error::Config(format!(
            "relationship repair {field}: the write was accepted, but config/repo.ds \
             still reads as it did before it after 30s. Nothing appears to have \
             changed; run repair again, or {check}"
        ))),
        repair::Readback::Different => Err(crate::Error::Config(format!(
            "relationship repair {field}: the write was accepted, but a read of \
             config/repo.ds returned neither the document sent nor the one it replaced — another \
             writer, or the tenant rewrote it. Run {check}; the \
             replaced document is at {}",
            applied.backup.display()
        ))),
        repair::Readback::Unread { error } => Err(crate::Error::Config(format!(
            "relationship repair {field}: the write was accepted, but reading it back \
             failed ({error}). Run {check}; the replaced \
             document is at {}",
            applied.backup.display()
        ))),
    }
}

async fn relationship_relink(
    field: String,
    dry_run: bool,
    force: bool,
    tenant: Option<String>,
    yes: bool,
    json: bool,
) -> Result<()> {
    let (object, key) = parse_object_key(&field)?;
    let tenant = tenant_for(tenant)?;
    let managed = api::get_managed(&tenant).await?;
    let property = api::object_named(&managed, &object)
        .ok()
        .and_then(state::properties)
        .and_then(|properties| properties.get(&key))
        .filter(|property| state::is_relationship_property(property))
        .ok_or_else(|| crate::Error::Config(format!("{field} is not a relationship")))?;
    let far_key = ops::declared_reverse_key(property)
        .ok_or_else(|| {
            crate::Error::Config(format!(
                "{field} declares no reverse, so there is nothing to relink to; add one with \
                 `aic managed relationship set {field} --reverse ...` first"
            ))
        })?
        .to_string();
    let far = ops::relationship_targets(property)
        .first()
        .and_then(|target| target.strip_prefix("managed/"))
        .ok_or_else(|| crate::Error::Config(format!("{field} has no managed target")))?
        .to_string();
    if ops::relationship_targets(property).len() > 1 {
        return Err(crate::Error::Config(format!(
            "{field} has more than one target; relink handles a single-target pair"
        )));
    }
    // A link re-created on a pair whose ends disagree about storage can land
    // in a store the other end does not read — the measured mixed state.
    let repo_ds = api::get_repo_ds(&tenant).await?;
    let problems: Vec<String> = verify::pairs(&managed, &repo_ds, &object, &key)
        .iter()
        .flat_map(|pair| pair.problems.iter().map(verify::Problem::message))
        .collect();
    if !problems.is_empty() {
        return Err(crate::Error::Config(format!(
            "relationship relink refuses {field}: `relationship verify` reports {}",
            problems.join("; ")
        )));
    }
    let near_array = end_is_array(&managed, &object, &key);
    let far_array = end_is_array(&managed, &far, &far_key);
    let far_collection = format!("managed/{far}");
    let near_collection = format!("managed/{object}");
    let far_end = (far.as_str(), far_key.as_str(), far_array);

    let reconciled = reconcile(&tenant, &managed, Some(&field)).await?;
    let records = api::all_records_with(&tenant, &object, &key).await?;
    let mut judged = Vec::new();
    for record in &records {
        for edge in relink::edges_of(record, &key).map_err(crate::Error::Config)? {
            if edge.link.to_collection != far_collection {
                continue;
            }
            let (outcome, slot) =
                far_outcome(&tenant, far_end, &edge.link, &near_collection).await?;
            judged.push((edge, outcome, slot));
            if judged.len() % 100 == 0 {
                eprintln!("relink: {} links checked", judged.len());
            }
        }
    }
    let plan = relink::plan(judged);
    let mut out = RelinkOutput {
        plan: &plan,
        to_restore: reconciled
            .restore
            .iter()
            .map(recovery::Pending::label)
            .collect(),
        restored: Vec::new(),
        relinked: Vec::new(),
        unconfirmed: Vec::new(),
        changed: Vec::new(),
        attention: reconciled.attention,
        error: None,
    };
    eprintln!(
        "{field}: {} records, {} links listed by {far}.{far_key}, {} to relink, {} skipped",
        records.len(),
        plan.resolved,
        plan.relink.len(),
        plan.skipped.len()
    );
    for pending in &reconciled.restore {
        eprintln!(
            "  restore {}: removed by a run interrupted at {}",
            pending.label(),
            pending.started_at
        );
    }
    for skipped in &plan.skipped {
        eprintln!(
            "  skip {} -> {}: {}",
            skipped.edge.link.from_id, skipped.edge.link.to_id, skipped.reason
        );
    }
    if (plan.relink.is_empty() && reconciled.restore.is_empty()) || dry_run {
        return finish_relink(&out, json);
    }
    let consented = force
        || crate::cli::confirm_destructive(
            "relationship relink",
            &format!(
                "Remove and re-create {} link(s) on {field}{}? Each gets a new edge id; whether \
                 onUpdate hooks fire for it is unmeasured",
                plan.relink.len(),
                match reconciled.restore.len() {
                    0 => String::new(),
                    n => format!(", and re-create {n} an interrupted run removed"),
                }
            ),
            "--force",
        )?;
    let Some(permit) = relink::authorize(dry_run, consented) else {
        return Err(crate::Error::Config(
            "relationship relink not confirmed".into(),
        ));
    };
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    eprintln!(
        "relink: each link is recorded in {} before it is removed; if this run stops, \
         re-running it restores what it removed",
        recovery::file().display()
    );
    let near_end = (object.as_str(), key.as_str(), near_array);
    let shape = current_shape(&managed, &object, &key);
    for pending in &reconciled.restore {
        match restore_one(&ok, &permit, near_end, far_end, shape.as_ref(), pending).await {
            Ok(Restored::Confirmed) => out.restored.push(pending.label()),
            Ok(Restored::OneSided) => out.attention.push(format!(
                "{}: restored, but the far end does not list it yet; re-run relink to check",
                pending.label()
            )),
            Ok(Restored::NotWritten(why)) => out.attention.push(format!(
                "{}: not restored, because {why}; re-run to check",
                pending.label()
            )),
            Err(error) => {
                out.error = Some(error);
                return finish_relink(&out, json);
            }
        }
    }
    for edge in &plan.relink {
        match relink_one(&ok, &permit, near_end, far_end, edge).await {
            Ok(RelinkStep::Done) => out.relinked.push(edge.clone()),
            Ok(RelinkStep::Unconfirmed) => out.unconfirmed.push(edge.clone()),
            Ok(RelinkStep::Changed) => out.changed.push(edge.clone()),
            Err(error) => {
                // §5's batch rule: stop, and say what landed.
                out.error = Some(error);
                break;
            }
        }
    }
    finish_relink(&out, json)
}

/// The far end's verdict on `link`, and whether a has-one far end holds some
/// other link.
async fn far_outcome(
    tenant: &str,
    (far, far_key, far_array): (&str, &str, bool),
    link: &verify::Link,
    near_collection: &str,
) -> Result<(verify::LinkOutcome, relink::FarSlot)> {
    let far_record =
        api::far_end(tenant, far, &link.to_id, far_key, far_array, &link.from_id).await?;
    let far_links = far_record
        .map(|record| verify::links_of(&record, far_key))
        .transpose()
        .map_err(crate::Error::Config)?;
    let outcome = verify::judge(link, near_collection, far_links.as_deref());
    let slot = relink::FarSlot::of(far_array, &outcome, far_links.as_deref());
    Ok((outcome, slot))
}

fn is_precondition_failed(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Api { status: 412, .. })
}

enum RelinkStep {
    Done,
    Unconfirmed,
    Changed,
}

/// Rechecks, rewrites and confirms one link. An `Err` is a message that
/// already says whether the link was left removed.
///
/// The removed link is in the recovery journal from before its removal until
/// a read shows it re-created, so a run killed between the two writes is
/// finished by the next one ([`reconcile`]).
async fn relink_one(
    ok: &WriteOk<'_>,
    permit: &relink::RelinkPermit,
    (object, key, near_array): (&str, &str, bool),
    far_end: (&str, &str, bool),
    edge: &relink::Edge,
) -> std::result::Result<RelinkStep, String> {
    let label = format!(
        "{object}/{}.{key} -> {}",
        edge.link.from_id, edge.link.to_id
    );
    let read = |context: &'static str| {
        let label = label.clone();
        move |error: crate::Error| format!("{label}: {context}: {error}")
    };
    let (current, rev) = read_edges(ok.tenant, object, &edge.link.from_id, key)
        .await
        .map_err(read("recheck failed; nothing written"))?;
    if !relink::still_planned(&current, edge) {
        return Ok(RelinkStep::Changed);
    }
    // The far end too: a has-one reverse that gained a link since the plan
    // makes the re-creation fail or come out one-sided (measured: IDM refuses
    // a link into an occupied has-one with 400, except generic has-one to
    // has-one, which accepts it one-sided). Neither displaces that link, but
    // the first leaves this one removed, so do not start.
    let near_collection = format!("managed/{object}");
    let (outcome, slot) = far_outcome(ok.tenant, far_end, &edge.link, &near_collection)
        .await
        .map_err(read("far-end recheck failed; nothing written"))?;
    if outcome != verify::LinkOutcome::Missing || slot == relink::FarSlot::Occupied {
        return Ok(RelinkStep::Changed);
    }
    let pending = recovery::Pending::relink(ok.tenant, object, key, near_array, edge);
    recovery::record(&pending).map_err(read(
        "could not record it in the recovery journal; nothing written",
    ))?;
    let forget = || {
        if let Err(error) = recovery::clear(&pending.id) {
            eprintln!(
                "warning: {label}: could not clear its recovery entry ({error}); the next run will"
            );
        }
    };
    let [remove, mut create] =
        relink::writes(permit, object, key, near_array, edge, rev.as_deref())?;
    match api::send_relink(ok.tenant, object, &remove, ok.confirmed_prod).await {
        Ok(_) => {}
        Err(error) if is_precondition_failed(&error) => {
            forget();
            return Ok(RelinkStep::Changed);
        }
        Err(error) => {
            return Err(format!(
                "{label}: removing the link failed ({error}); it may or may not be removed. \
                 Re-running relink restores it from the recovery journal if it was"
            ));
        }
    }
    let restore_hint = |create: &relink::Write| {
        format!(
            "re-running relink restores it from the recovery journal, or send {} {} {}",
            create.method,
            create.path,
            create.body.clone().unwrap_or(Value::Null)
        )
    };
    if !near_array {
        // Re-read before re-creating: a has-one that gained a link in the
        // gap would be overwritten by the replace.
        let (between, rev) = read_edges(ok.tenant, object, &edge.link.from_id, key)
            .await
            .map_err(|error| {
                format!(
                    "{label}: the link was removed and the re-read failed ({error}); {}",
                    restore_hint(&create)
                )
            })?;
        if !between.is_empty() {
            return Err(format!(
                "{label}: the link was removed, and the property gained another link before it \
                 was re-created; that link is not overwritten. The removed one stays in the \
                 recovery journal"
            ));
        }
        create.if_match = rev;
    }
    let written = match api::send_relink(ok.tenant, object, &create, ok.confirmed_prod).await {
        Ok(response) => relink::written_edge(&response, key, near_array),
        Err(error) => {
            let why = if is_precondition_failed(&error) {
                "the record changed between the re-read and the re-creation, which was refused"
                    .to_string()
            } else {
                format!("re-creating it failed ({error})")
            };
            return Err(format!(
                "{label}: the link was removed and {why}; {}",
                restore_hint(&create)
            ));
        }
    };
    let (after, _) = read_edges(ok.tenant, object, &edge.link.from_id, key)
        .await
        .map_err(read("re-created, but the confirming read failed"))?;
    // Confirmed only with the properties it carried: until then the journal
    // holds the only copy of them.
    let Some(new) = recreated(&after, edge, written.as_deref()) else {
        return Ok(RelinkStep::Unconfirmed);
    };
    forget();
    let (outcome, _) = far_outcome(ok.tenant, far_end, &new.link, &near_collection)
        .await
        .map_err(read("re-created, but the far end could not be read"))?;
    Ok(if outcome == verify::LinkOutcome::Resolved {
        RelinkStep::Done
    } else {
        RelinkStep::Unconfirmed
    })
}

/// The link a relink or restore re-created, among the record's links after
/// it: the edge the write's response named when it named one, else one to
/// the same target with the same properties. Never a link to the target
/// alone — another writer's is not this one.
fn recreated<'a>(
    after: &'a [relink::Edge],
    edge: &relink::Edge,
    written: Option<&str>,
) -> Option<&'a relink::Edge> {
    after.iter().find(|candidate| {
        candidate.link.to_id == edge.link.to_id
            && candidate.link.to_collection == edge.link.to_collection
            && candidate.properties == edge.properties
            && written.is_none_or(|id| candidate.link.edge_id.as_deref() == Some(id))
    })
}

enum Restored {
    /// Re-created, and the far end lists it.
    Confirmed,
    /// Re-created and listed by the record, but not yet by the far end: the
    /// journal entry is done, the link is one-sided like any other.
    OneSided,
    /// Not written; says what changed.
    NotWritten(String),
}

/// Re-creates a link an interrupted run removed, under the same guards as
/// [`relink_one`]: judged against the field's current `shape`, the near record
/// must still lack it (and, has-one, hold nothing), and the far record must
/// exist with a has-one end not holding another link. Confirmed from both
/// ends afterwards, like a relink.
async fn restore_one(
    ok: &WriteOk<'_>,
    permit: &relink::RelinkPermit,
    (object, key, near_array): (&str, &str, bool),
    far_end: (&str, &str, bool),
    shape: Option<&recovery::Shape>,
    pending: &recovery::Pending,
) -> std::result::Result<Restored, String> {
    let label = pending.label();
    let (current, rev) = read_edges(ok.tenant, object, &pending.from_id, key)
        .await
        .map_err(|error| format!("{label}: recheck failed ({error}); nothing written"))?;
    if recovery::judge(pending, shape, &current) != recovery::Recovery::Lost {
        return Ok(Restored::NotWritten("the record changed".into()));
    }
    if !near_array && !current.is_empty() {
        // `judge` says the same for an unchanged shape; this does not rely on it.
        return Ok(Restored::NotWritten(
            "the has-one property is occupied".into(),
        ));
    }
    let edge = relink::Edge {
        link: verify::Link {
            from_id: pending.from_id.clone(),
            to_collection: pending.to_collection.clone(),
            to_id: pending.to_id.clone(),
            edge_id: pending.edge_id.clone(),
        },
        properties: pending.properties.clone(),
    };
    let near_collection = format!("managed/{object}");
    let (outcome, slot) = far_outcome(ok.tenant, far_end, &edge.link, &near_collection)
        .await
        .map_err(|error| format!("{label}: far-end recheck failed ({error}); nothing written"))?;
    if outcome == verify::LinkOutcome::FarRecordMissing {
        return Ok(Restored::NotWritten("the far record is gone".into()));
    }
    if slot == relink::FarSlot::Occupied {
        return Ok(Restored::NotWritten(
            "the far end is a has-one holding another link".into(),
        ));
    }
    let [_, mut create] = relink::writes(permit, object, key, near_array, &edge, None)?;
    if !near_array {
        create.if_match = rev;
    }
    let written = match api::send_relink(ok.tenant, object, &create, ok.confirmed_prod).await {
        Ok(response) => relink::written_edge(&response, key, near_array),
        Err(error) if is_precondition_failed(&error) => {
            return Ok(Restored::NotWritten(
                "the record changed before the write".into(),
            ));
        }
        Err(error) => {
            return Err(format!(
                "{label}: restoring it failed ({error}); it stays in the recovery journal"
            ));
        }
    };
    let (after, _) = read_edges(ok.tenant, object, &pending.from_id, key)
        .await
        .map_err(|error| format!("{label}: restored, but the confirming read failed ({error})"))?;
    if recovery::judge(pending, shape, &after) != recovery::Recovery::Settled {
        return Err(format!(
            "{label}: the restore was accepted but the record does not list it with its \
             properties; it stays in the recovery journal"
        ));
    }
    recovery::clear(&pending.id).map_err(|error| {
        format!("{label}: restored, but its recovery entry could not be cleared ({error})")
    })?;
    let Some(new) = recreated(&after, &edge, written.as_deref()) else {
        return Ok(Restored::OneSided);
    };
    let (outcome, _) = far_outcome(ok.tenant, far_end, &new.link, &near_collection)
        .await
        .map_err(|error| {
            format!("{label}: restored, but the far end could not be read ({error})")
        })?;
    Ok(if outcome == verify::LinkOutcome::Resolved {
        Restored::Confirmed
    } else {
        Restored::OneSided
    })
}

fn finish_relink(out: &RelinkOutput<'_>, json: bool) -> Result<()> {
    if json {
        print_json(out)?;
    } else {
        for restored in &out.restored {
            println!("restored: {restored}");
        }
        for (label, edges) in [
            ("relinked", &out.relinked),
            ("unconfirmed", &out.unconfirmed),
            ("changed since the plan, not written", &out.changed),
        ] {
            for edge in edges {
                println!("{label}: {} -> {}", edge.link.from_id, edge.link.to_id);
            }
        }
        for line in &out.attention {
            println!("needs attention: {line}");
        }
    }
    if let Some(error) = &out.error {
        return Err(crate::Error::Config(format!(
            "relationship relink stopped after {} relinked: {error}",
            out.relinked.len()
        )));
    }
    if !out.unconfirmed.is_empty() || !out.changed.is_empty() || !out.attention.is_empty() {
        return Err(crate::Error::Config(format!(
            "relationship relink: {} unconfirmed, {} changed since the plan, {} needing \
             attention; re-run to check",
            out.unconfirmed.len(),
            out.changed.len(),
            out.attention.len()
        )));
    }
    Ok(())
}

#[derive(Serialize)]
struct DirectionOutput {
    from: String,
    to: String,
    /// Records read looking for links.
    scanned: usize,
    links: Vec<SampledLink>,
    /// The `/_ref eq` filter, tried on the far end for one resolved link when
    /// that end is a has-many (a singular end's collection is 501).
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_filter: Option<verify::RefFilter>,
    /// Whether the running IDM stores the near end as `repo.ds` says, from
    /// one resolved link; absent when none resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<verify::Runtime>,
}

#[derive(Serialize)]
struct SampledLink {
    #[serde(flatten)]
    link: verify::Link,
    #[serde(flatten)]
    outcome: verify::LinkOutcome,
}

#[derive(Serialize)]
struct PairOutput {
    #[serde(flatten)]
    pair: verify::PairReport,
    directions: Vec<DirectionOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    probe: Option<ProbeOutput>,
}

#[derive(Serialize)]
struct ProbeOutput {
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    near_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    far_id: Option<String>,
    /// What the far end said about the test link; absent when none was made.
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<verify::LinkOutcome>,
    /// Why no link was made or judged, or what went wrong removing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// The test link is gone again (re-read). `false` is a leftover to remove.
    removed: bool,
    /// Each end's runtime storage, judged while the test link existed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    runtime: Vec<RuntimeOutput>,
    #[serde(skip)]
    failed: bool,
}

#[derive(Serialize)]
struct RuntimeOutput {
    end: String,
    runtime: verify::Runtime,
    #[serde(skip)]
    message: String,
}

impl ProbeOutput {
    fn new(near: &verify::End, far: &verify::End) -> Self {
        ProbeOutput {
            from: format!("{}.{}", near.object, near.key),
            to: format!("{}.{}", far.object, far.key),
            near_id: None,
            far_id: None,
            outcome: None,
            note: None,
            removed: false,
            runtime: Vec::new(),
            failed: false,
        }
    }

    fn untested(near: &verify::End, far: &verify::End, note: String) -> Self {
        ProbeOutput {
            note: Some(note),
            ..Self::new(near, far)
        }
    }

    fn is_failure(&self) -> bool {
        self.failed
    }

    /// The test link resolved from the far end and is gone again: the pair is
    /// healthy on the probe's evidence, whatever the sample found.
    fn verified(&self) -> bool {
        self.outcome == Some(verify::LinkOutcome::Resolved) && self.removed && !self.failed
    }
}

/// A direction with no existing links to sample. "Untested" only when no
/// probe stands in for the sample, or the output contradicts itself: the
/// directions say untested and the probe line below says verified.
fn unsampled_direction_line(direction: &DirectionOutput, probe_verified: bool) -> String {
    if probe_verified {
        format!(
            "{} -> {}: no pre-existing links in {} records read; probe-verified (below)",
            direction.from, direction.to, direction.scanned
        )
    } else {
        format!(
            "{} -> {}: untested, no links in {} records read",
            direction.from, direction.to, direction.scanned
        )
    }
}

/// Writes one test link from a forward-end record to a reverse-end record,
/// judges it from the reverse end, and removes it. Records are chosen so the
/// link displaces nothing: a has-one end only through a record whose property
/// is empty.
/// The first end of the pair its object's schema lists as `required`.
///
/// A probe has to remove its test link, and IDM's policy check refuses the
/// removal through a required end: on 2026-11-01 a probe through the required
/// has-one `test_from.aicRel` wrote its link and then got `403 Failed policy
/// validation` (REQUIRED) on the cleanup, stranding it. So a required end is
/// not probed at all.
fn required_end(managed: &Value, near: &verify::End, far: &verify::End) -> Option<String> {
    [near, far].into_iter().find_map(|end| {
        let object = api::object_named(managed, &end.object).ok()?;
        state::required_fields(object)
            .contains(&end.key)
            .then(|| format!("{}.{}", end.object, end.key))
    })
}

async fn probe_pair(
    ok: &WriteOk<'_>,
    permit: &relink::ProbePermit,
    near: &verify::End,
    far: &verify::End,
    scan: usize,
) -> ProbeOutput {
    let mut out = ProbeOutput::new(near, far);
    let fail = |mut out: ProbeOutput, note: String| {
        out.note = Some(note);
        out.failed = true;
        out
    };
    let candidates = async {
        let near_records = api::records_with(ok.tenant, &near.object, &near.key, scan).await?;
        let far_records = api::records_with(ok.tenant, &far.object, &far.key, scan).await?;
        let near_id = relink::probe_candidate(&near_records, &near.key, near.array, None);
        let exclude = near_id.as_deref().filter(|_| near.object == far.object);
        let far_id = relink::probe_candidate(&far_records, &far.key, far.array, exclude);
        Ok::<_, crate::Error>((near_id, far_id))
    }
    .await;
    let (near_id, far_id) = match candidates {
        Ok((Some(near_id), Some(far_id))) => (near_id, far_id),
        Ok((near_id, _)) => {
            let empty = if near_id.is_none() { near } else { far };
            out.note = Some(format!(
                "untested: no record of managed/{} in the {scan} read {}",
                empty.object,
                if empty.array {
                    "to link through".to_string()
                } else {
                    format!("with {} empty", empty.key)
                }
            ));
            return out;
        }
        Err(error) => return fail(out, format!("could not read records: {error}")),
    };
    out.near_id = Some(near_id.clone());
    out.far_id = Some(far_id.clone());
    let shape = (near.object.as_str(), near.key.as_str(), near.array);
    let read_near = || read_edges(ok.tenant, &near.object, &near_id, &near.key);
    let (before, rev) = match read_near().await {
        Ok(read) => read,
        Err(error) => return fail(out, format!("could not read {near_id}: {error}")),
    };
    // The candidates came from a page read some time ago. A has-one that has
    // since gained a link would be overwritten by the probe — and then
    // emptied by its cleanup — so both ends are read again here.
    if !near.array && !before.is_empty() {
        out.note = Some(format!(
            "untested: managed/{}/{near_id}.{} gained a link after it was chosen; not \
             overwritten",
            near.object, near.key
        ));
        return out;
    }
    if !far.array {
        match read_edges(ok.tenant, &far.object, &far_id, &far.key).await {
            Ok((held, _)) if !held.is_empty() => {
                out.note = Some(format!(
                    "untested: managed/{}/{far_id}.{} gained a link after it was chosen; a test \
                     link into it is refused or one-sided, so it proves nothing",
                    far.object, far.key
                ));
                return out;
            }
            Ok(_) => {}
            Err(error) => return fail(out, format!("could not read {far_id}: {error}")),
        }
    }
    let pending = recovery::Pending::probe(ok.tenant, shape, &near_id, &far.object, &far_id);
    if let Err(error) = recovery::record(&pending) {
        return fail(
            out,
            format!(
                "could not record the probe in the recovery journal ({error}); nothing written"
            ),
        );
    }
    let forget = || {
        if let Err(error) = recovery::clear(&pending.id) {
            eprintln!(
                "warning: could not clear the probe's recovery entry ({error}); the next run will"
            );
        }
    };
    let link = relink::probe_link(
        permit,
        shape,
        &near_id,
        &far.object,
        &far_id,
        rev.as_deref(),
    );
    let written = match api::send_relink(ok.tenant, &near.object, &link, ok.confirmed_prod).await {
        Ok(response) => relink::written_edge(&response, &near.key, near.array),
        Err(error) if is_precondition_failed(&error) => {
            forget();
            out.note = Some(format!(
                "untested: {near_id} changed between its read and the test link; nothing written"
            ));
            return out;
        }
        Err(error) => {
            return fail(
                out,
                format!("the test link write failed ({error}); it may or may not exist"),
            );
        }
    };
    // Recorded before anything else can fail, so an interrupted run leaves
    // the next one the edge to look for.
    if let Some(edge_id) = &written
        && let Err(error) = recovery::set_edge(&pending.id, edge_id)
    {
        eprintln!("warning: could not record the probe's edge id ({error})");
    }
    let after = match read_near().await {
        Ok((edges, _)) => edges,
        Err(error) => {
            return fail(
                out,
                format!(
                    "linked, then could not re-read {near_id} ({error}); remove the link by hand"
                ),
            );
        }
    };
    let far_collection = format!("managed/{}", far.object);
    // Only an edge the write's own response named is the probe's to remove:
    // a link that is merely new since the read may be another writer's.
    let Some(edge_id) = written else {
        return fail(
            out,
            format!(
                "the test link was written, but the response did not name its edge, so it \
                 cannot be told apart from another writer's link to {far_id} and is not \
                 removed; it stays in the recovery journal. Check {near_id}.{} by hand",
                near.key
            ),
        );
    };
    let Some(new) = relink::new_link(&after, &far_collection, &far_id, &edge_id).cloned() else {
        return fail(
            out,
            format!(
                "the write succeeded but {near_id} does not list its edge {edge_id} — another \
                 writer may have replaced it — so nothing is removed; check {near_id}.{} by hand",
                near.key
            ),
        );
    };
    let near_collection = format!("managed/{}", near.object);
    match far_outcome(
        ok.tenant,
        (&far.object, &far.key, far.array),
        &new.link,
        &near_collection,
    )
    .await
    {
        Ok((outcome, _)) => {
            out.failed = outcome != verify::LinkOutcome::Resolved;
            if outcome == verify::LinkOutcome::Resolved {
                for (end, id) in [(near, &near_id), (far, &far_id)] {
                    match runtime_of(ok.tenant, end, id, Some(&edge_id)).await {
                        Ok(None) => eprintln!(
                            "note: {}.{}'s runtime storage is unknown: the test link was gone \
                             before it could be checked",
                            end.object, end.key
                        ),
                        Ok(Some(runtime)) => {
                            out.failed |= runtime.is_failure();
                            out.runtime.push(RuntimeOutput {
                                end: format!("{}.{}", end.object, end.key),
                                runtime,
                                message: runtime.message(end),
                            });
                        }
                        Err(error) => eprintln!(
                            "warning: could not check {}.{}'s runtime storage ({error})",
                            end.object, end.key
                        ),
                    }
                }
            }
            out.outcome = Some(outcome);
        }
        Err(error) => {
            out.failed = true;
            out.note = Some(format!("could not read the far end: {error}"));
        }
    }
    // A has-many removal names the edge. A has-one removal names the
    // property, so it is sent only while a fresh read shows the property
    // still holding the test edge, and conditional on that read's `_rev`.
    let unlink = if near.array {
        Ok(relink::probe_unlink(
            permit, shape, &near_id, &edge_id, None,
        ))
    } else {
        match read_near().await {
            Ok((held, rev))
                if held
                    .iter()
                    .any(|edge| edge.link.edge_id == new.link.edge_id) =>
            {
                Ok(relink::probe_unlink(
                    permit,
                    shape,
                    &near_id,
                    &edge_id,
                    rev.as_deref(),
                ))
            }
            Ok(_) => Err(Some(format!(
                "{near_id}.{} no longer holds the test link (edge {edge_id}) — another writer \
                 replaced or removed it — so nothing was removed",
                near.key
            ))),
            Err(error) => Err(Some(format!(
                "could not re-read {near_id} before removing the test link ({error}); remove \
                 edge {edge_id} by hand"
            ))),
        }
    };
    let unlink = match unlink {
        Ok(unlink) => unlink,
        Err(note) => {
            out.failed = true;
            out.note = note;
            // The test edge is not there, or was not seen: the journal entry
            // stays so the next run checks again.
            return out;
        }
    };
    let removed = match api::send_relink(ok.tenant, &near.object, &unlink, ok.confirmed_prod).await
    {
        Ok(_) => read_near().await.map(|(edges, _)| {
            !edges
                .iter()
                .any(|edge| edge.link.edge_id == new.link.edge_id)
        }),
        Err(error) => Err(error),
    };
    match removed {
        Ok(true) => {
            out.removed = true;
            forget();
        }
        Ok(false) | Err(_) => {
            let detail = removed
                .err()
                .map(|error| format!(" ({error})"))
                .unwrap_or_default();
            out.failed = true;
            out.note = Some(format!(
                "the test link {near_id} -> {far_id} (edge {edge_id}) was not removed{detail}; \
                 remove it with {} {}",
                unlink.method, unlink.path
            ));
        }
    }
    out
}

async fn sample_direction(
    tenant: &str,
    near: &verify::End,
    far: &verify::End,
    sample: usize,
    scan: usize,
) -> Result<DirectionOutput> {
    let records = api::records_with(tenant, &near.object, &near.key, scan).await?;
    let far_collection = format!("managed/{}", far.object);
    let mut found = Vec::new();
    for record in &records {
        found.extend(verify::links_of(record, &near.key).map_err(crate::Error::Config)?);
    }
    let mut links = Vec::new();
    for link in found
        .into_iter()
        .filter(|link| link.to_collection == far_collection)
        .take(sample)
    {
        let far_record = api::far_end(
            tenant,
            &far.object,
            &link.to_id,
            &far.key,
            far.array,
            &link.from_id,
        )
        .await?;
        let far_links = far_record
            .map(|record| verify::links_of(&record, &far.key))
            .transpose()
            .map_err(crate::Error::Config)?;
        let near_collection = format!("managed/{}", near.object);
        let outcome = verify::judge(&link, &near_collection, far_links.as_deref());
        links.push(SampledLink { link, outcome });
    }
    let mut ref_filter = None;
    if far.array
        && let Some(sampled) = links
            .iter()
            .find(|sampled| sampled.outcome == verify::LinkOutcome::Resolved)
    {
        let reference = format!("managed/{}/{}", near.object, sampled.link.from_id);
        let found = api::ref_filter_finds(
            tenant,
            &far.object,
            &sampled.link.to_id,
            &far.key,
            &reference,
        )
        .await?;
        ref_filter = Some(verify::RefFilter::judge(far, found));
    }
    let runtime = match links
        .iter()
        .find(|sampled| sampled.outcome == verify::LinkOutcome::Resolved)
    {
        Some(sampled) => {
            runtime_of(
                tenant,
                near,
                &sampled.link.from_id,
                sampled.link.edge_id.as_deref(),
            )
            .await?
        }
        None => None,
    };
    Ok(DirectionOutput {
        from: format!("{}.{}", near.object, near.key),
        to: format!("{}.{}", far.object, far.key),
        scanned: records.len(),
        links,
        ref_filter,
        runtime,
    })
}

struct ProbeRequest {
    force: bool,
    yes: bool,
}

async fn collect_verification(
    target: String,
    sample: usize,
    scan: usize,
    probe: Option<ProbeRequest>,
    tenant: Option<String>,
) -> Result<VerificationOutput> {
    let (object, key) = match target.split_once('.') {
        Some(_) => {
            let (object, key) = parse_object_key(&target)?;
            (object, Some(key))
        }
        None => (target, None),
    };
    let tenant = tenant_for(tenant)?;
    let managed = api::get_managed(&tenant).await?;
    let repo_ds = api::get_repo_ds(&tenant).await?;
    let report = verify::report(&managed, &repo_ds, &object, key.as_deref())
        .map_err(crate::Error::Config)?;
    let probing = match &probe {
        Some(request) => {
            let count = report.pairs.iter().filter(|pair| pair.sampleable()).count();
            let consented = request.force
                || crate::cli::confirm_destructive(
                    "relationship verify --probe",
                    &format!(
                        "Write and remove one test link on each of {count} pair(s)? It links two \
                         existing records, and their hooks fire"
                    ),
                    "--force",
                )?;
            let Some(permit) = relink::authorize_probe(consented) else {
                return Err(crate::Error::Config(
                    "relationship verify --probe not confirmed".into(),
                ));
            };
            Some((permit, ensure_prod_confirmed(&tenant, request.yes)?))
        }
        None => None,
    };
    // A probe or relink this install interrupted earlier is reported before
    // another probe writes, and counts as a failure until it is resolved.
    let attention = if probing.is_some() {
        reconcile(&tenant, &managed, None).await?.attention
    } else {
        Vec::new()
    };
    let mut pairs = Vec::new();
    for pair in report.pairs {
        let mut directions = Vec::new();
        let mut probe = None;
        if pair.sampleable()
            && let Some(reverse) = &pair.reverse
            && (sample > 0 || probing.is_some())
        {
            if sample > 0 {
                directions
                    .push(sample_direction(&tenant, &pair.forward, reverse, sample, scan).await?);
                directions
                    .push(sample_direction(&tenant, reverse, &pair.forward, sample, scan).await?);
            }
            if let Some((permit, ok)) = &probing {
                probe = Some(match required_end(&managed, &pair.forward, reverse) {
                    Some(end) => ProbeOutput::untested(
                        &pair.forward,
                        reverse,
                        format!(
                            "untested: {end} is required, so policy refuses removing a test \
                             link through it and the probe would strand one; nothing written"
                        ),
                    ),
                    None => probe_pair(ok, permit, &pair.forward, reverse, scan).await,
                });
            }
        }
        pairs.push(PairOutput {
            pair,
            directions,
            probe,
        });
    }
    let failures = report.stale.len()
        + attention.len()
        + pairs
            .iter()
            .map(|pair| {
                pair.pair.problems.len()
                    + pair
                        .directions
                        .iter()
                        .flat_map(|direction| &direction.links)
                        .filter(|link| link.outcome != verify::LinkOutcome::Resolved)
                        .count()
                    + pair
                        .directions
                        .iter()
                        .filter(|direction| {
                            direction
                                .ref_filter
                                .is_some_and(verify::RefFilter::is_failure)
                                || direction.runtime.is_some_and(verify::Runtime::is_failure)
                        })
                        .count()
                    + usize::from(pair.probe.as_ref().is_some_and(ProbeOutput::is_failure))
            })
            .sum::<usize>();
    Ok(VerificationOutput {
        pairs,
        stale: report.stale,
        recovery: attention,
        failures,
    })
}

#[derive(Serialize)]
struct VerificationOutput {
    pairs: Vec<PairOutput>,
    stale: Vec<verify::Problem>,
    recovery: Vec<String>,
    failures: usize,
}

fn verification_check(report: &VerificationOutput) -> Result<()> {
    if report.failures > 0 {
        return Err(crate::Error::Config(format!(
            "relationship verify found {} problem(s)",
            report.failures
        )));
    }
    Ok(())
}

async fn relationship_verify(
    target: String,
    sample: usize,
    scan: usize,
    probe: Option<ProbeRequest>,
    tenant: Option<String>,
    json: bool,
) -> Result<()> {
    let report = collect_verification(target, sample, scan, probe, tenant).await?;
    if json {
        print_json(&report)?;
    } else {
        print_verify(&report.pairs, &report.stale, sample);
    }
    verification_check(&report)
}

/// The end a direction's links start from.
fn direction_near<'a>(
    pair: &'a PairOutput,
    direction: &DirectionOutput,
) -> Option<&'a verify::End> {
    let reverse = pair.pair.reverse.as_ref()?;
    [&pair.pair.forward, reverse]
        .into_iter()
        .find(|end| format!("{}.{}", end.object, end.key) == direction.from)
}

/// The end a direction's links point at.
fn direction_far<'a>(pair: &'a PairOutput, direction: &DirectionOutput) -> Option<&'a verify::End> {
    let reverse = pair.pair.reverse.as_ref()?;
    let label = |end: &verify::End| format!("{}.{}", end.object, end.key);
    [&pair.pair.forward, reverse]
        .into_iter()
        .find(|end| label(end) == direction.to)
}

fn outcome_detail(outcome: &verify::LinkOutcome) -> String {
    match outcome {
        verify::LinkOutcome::Resolved => "the far end lists it".into(),
        verify::LinkOutcome::Missing => "the far end does not list it".into(),
        verify::LinkOutcome::FarRecordMissing => "the far record does not exist".into(),
        verify::LinkOutcome::EdgeDiffers { far_edges } => {
            format!("the far end lists it under edge {}", far_edges.join(", "))
        }
    }
}

/// One line for a probe's result, from the same predicate the exit code uses.
fn probe_verdict(probe: &ProbeOutput) -> String {
    match (&probe.outcome, probe.removed) {
        _ if probe.verified() => {
            "verified — a test link resolved from the far end and was removed".to_string()
        }
        (Some(verify::LinkOutcome::Resolved), true) => {
            "a test link resolved from the far end and was removed, but a check below failed"
                .to_string()
        }
        (Some(verify::LinkOutcome::Resolved), false) => {
            "a test link resolved from the far end, but was not removed".to_string()
        }
        (Some(outcome), _) => format!("test link: {}", outcome_detail(outcome)),
        (None, _) => "no test link made".to_string(),
    }
}

fn print_verify(pairs: &[PairOutput], stale: &[verify::Problem], sample: usize) {
    for pair in pairs {
        let forward = &pair.pair.forward;
        let mode = |end: &verify::End| match &end.mapping {
            Some(mapping) => format!("stored mapping, {}", mapping.kind),
            None => "stored generic".into(),
        };
        match &pair.pair.reverse {
            Some(reverse) => println!(
                "{}.{} ({}) <-> {}.{} ({})",
                forward.object,
                forward.key,
                mode(forward),
                reverse.object,
                reverse.key,
                mode(reverse)
            ),
            None => println!(
                "{}.{} ({}), no reverse",
                forward.object,
                forward.key,
                mode(forward)
            ),
        }
        for problem in &pair.pair.problems {
            println!("  problem: {}", problem.message());
        }
        if !pair.pair.unchecked.is_empty() {
            println!(
                "  unchecked: {} (not in config/managed; its schema is not read)",
                pair.pair.unchecked.join(", ")
            );
        }
        let probe_verified = pair.probe.as_ref().is_some_and(ProbeOutput::verified);
        for direction in &pair.directions {
            let resolved = direction
                .links
                .iter()
                .filter(|link| link.outcome == verify::LinkOutcome::Resolved)
                .count();
            if direction.links.is_empty() {
                println!("  {}", unsampled_direction_line(direction, probe_verified));
                continue;
            }
            println!(
                "  {} -> {}: {resolved} of {} sampled links resolve from the far end",
                direction.from,
                direction.to,
                direction.links.len()
            );
            for link in &direction.links {
                if link.outcome == verify::LinkOutcome::Resolved {
                    continue;
                }
                let detail = outcome_detail(&link.outcome);
                println!(
                    "    {} -> {} (edge {}): {detail}",
                    link.link.from_id,
                    link.link.to_id,
                    link.link.edge_id.as_deref().unwrap_or("none")
                );
            }
            if let (Some(filter), Some(far)) =
                (direction.ref_filter, direction_far(pair, direction))
            {
                println!("  {}", filter.message(far));
            }
            if let (Some(runtime), Some(near)) =
                (direction.runtime, direction_near(pair, direction))
            {
                println!("  {}", runtime.message(near));
            }
        }
        if let Some(probe) = &pair.probe {
            println!(
                "  probe {} -> {}: {}",
                probe.from,
                probe.to,
                probe_verdict(probe)
            );
            for runtime in &probe.runtime {
                println!("    {}", runtime.message);
            }
            if let Some(note) = &probe.note {
                println!("    {note}");
            }
        }
        if sample > 0 && pair.directions.is_empty() && pair.pair.reverse.is_some() {
            println!("  links: not sampled, the ends do not point at each other");
        }
    }
    for problem in stale {
        println!("stale: {}", problem.message());
    }
}

async fn object_field_write<F>(
    field: String,
    tenant_arg: Option<String>,
    yes: bool,
    json: bool,
    transform: F,
) -> Result<()>
where
    F: FnOnce(&Value, &str) -> std::result::Result<Value, String>,
{
    let (object, key) = parse_object_key(&field)?;
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let mut doc = api::get_managed(&tenant).await?;
    let previous = api::object_named(&doc, &object)?.clone();
    let updated = transform(&previous, &key).map_err(crate::Error::Config)?;
    replace_object_write(&ok, &mut doc, &object, &previous, updated.clone()).await?;
    confirmation(&format!("managed field {field} updated"));
    output(json, &updated)
}

async fn replace_object_write(
    ok: &WriteOk<'_>,
    doc: &mut Value,
    object: &str,
    previous: &Value,
    updated: Value,
) -> Result<()> {
    let mut undo = DiskLog::load_default()?;
    ops::record_replace_undo(&mut undo, ok.tenant, object, previous, &updated)?;
    let base = doc.clone();
    api::replace_object(doc, object, updated.clone())?;
    let expect = [api::ConfigConfirm::ObjectContent {
        name: object.to_string(),
        content: updated,
    }];
    put_managed(ok, &base, doc.clone(), &expect).await
}

fn record_config_undo(
    tenant: &str,
    previous: &Value,
    updated: &Value,
    description: String,
) -> Result<()> {
    use crate::undo::{Capability, ConflictCheck, Sensitivity, UndoEntry, UndoOp};
    let mut undo = DiskLog::load_default()?;
    undo.record(UndoEntry::pending(
        tenant.to_string(),
        "managed",
        description,
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: tenant.to_string(),
            body: previous.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: updated.clone(),
        },
    ))?;
    Ok(())
}

pub(super) fn parse_object_key(value: &str) -> Result<(String, String)> {
    let mut parts = value.split('.');
    let (Some(object), Some(key), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(crate::Error::Config(format!(
            "expected <object>.<key>, got '{value}'"
        )));
    };
    if object.is_empty() || key.is_empty() {
        return Err(crate::Error::Config(format!(
            "expected <object>.<key>, got '{value}'"
        )));
    }
    Ok((object.into(), key.into()))
}
fn scalar_type(value: &str) -> Result<spec::ScalarFieldType> {
    spec::ScalarFieldType::from_label(value).ok_or_else(|| {
        crate::Error::Config(
            "--type must be string, boolean, number, string[], object, or object[]".into(),
        )
    })
}
fn cardinality(value: &str) -> Result<state::Cardinality> {
    match value {
        "one" => Ok(state::Cardinality::One),
        "many" => Ok(state::Cardinality::Many),
        _ => Err(crate::Error::Config("--forward must be one or many".into())),
    }
}
fn reverse_cardinality(value: &str) -> Result<state::ReverseCardinality> {
    match value {
        "none" => Ok(state::ReverseCardinality::None),
        "one" => Ok(state::ReverseCardinality::One),
        "many" => Ok(state::ReverseCardinality::Many),
        _ => Err(crate::Error::Config(
            "--reverse must be none, one, or many".into(),
        )),
    }
}
fn ref_property(value: &str) -> Result<state::RefProperty> {
    let mut parts = value.split(':');
    let (Some(name), Some(label), Some(kind), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(crate::Error::Config(format!(
            "--ref-property must be <name>:<label>:<type>, got '{value}'"
        )));
    };
    let kind = match kind {
        "string" => state::RefPropType::String,
        "number" => state::RefPropType::Number,
        "boolean" => state::RefPropType::Boolean,
        _ => {
            return Err(crate::Error::Config(
                "ref-property type must be string, number, or boolean".into(),
            ));
        }
    };
    Ok(state::RefProperty {
        name: name.into(),
        label: label.into(),
        kind,
    })
}
fn enum_spec(attrs: &FieldAttrs) -> Result<Option<spec::EnumSpec>> {
    if attrs.enum_values.is_empty() {
        Ok(None)
    } else {
        spec::parse_enum_items(&attrs.enum_values)
            .map(Some)
            .map_err(crate::Error::Config)
    }
}
fn field_edit_spec(attrs: FieldAttrs, allow_narrowing: bool) -> Result<spec::FieldEditSpec> {
    let enum_change = if attrs.clear_enum {
        spec::EnumChange::Clear
    } else {
        enum_spec(&attrs)?.map_or(spec::EnumChange::Unchanged, spec::EnumChange::Set)
    };
    let default_change = if attrs.clear_default {
        spec::DefaultChange::Clear
    } else {
        attrs
            .default_value
            .map_or(spec::DefaultChange::Unchanged, spec::DefaultChange::Set)
    };
    Ok(spec::FieldEditSpec {
        new_key: None,
        title: attrs.title,
        description: attrs.description,
        required: attrs.required,
        searchable: attrs.searchable,
        viewable: attrs.viewable,
        user_editable: attrs.user_editable,
        enum_change,
        default_change,
        allow_narrowing,
    })
}

fn ensure_narrowing_consent(removed: &[String], allow_narrowing: bool) -> Result<()> {
    if removed.is_empty() || allow_narrowing {
        Ok(())
    } else {
        Err(crate::Error::Config(format!(
            "dropping enum values {} would leave records that fail whole-record updates; pass --force to confirm",
            removed.join(", ")
        )))
    }
}
fn attrs_present(attrs: &FieldAttrs) -> bool {
    attrs.title.is_some()
        || attrs.description.is_some()
        || attrs.required.is_some()
        || attrs.searchable.is_some()
        || attrs.viewable.is_some()
        || attrs.user_editable.is_some()
        || !attrs.enum_values.is_empty()
        || attrs.clear_enum
        || attrs.default_value.is_some()
        || attrs.clear_default
}
fn ensure_edit_attrs(attrs: &FieldAttrs) -> Result<()> {
    if attrs_present(attrs) {
        Ok(())
    } else {
        Err(crate::Error::Config(
            "field edit needs at least one attribute flag".into(),
        ))
    }
}
fn object_names(doc: &Value) -> Vec<String> {
    api::objects(doc)
        .map(|objects| {
            objects
                .iter()
                .filter_map(|object| {
                    object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}
fn refuse_shipped(object: &Value, action: &str, force: bool) -> Result<()> {
    match state::object_protection(object) {
        state::ObjectProtection::Shipped => Err(crate::Error::Config(format!(
            "Ping-shipped objects cannot be {action}"
        ))),
        state::ObjectProtection::Marked if !force => {
            let name = object.get("name").and_then(Value::as_str).unwrap_or("?");
            Err(crate::Error::Config(format!(
                "{name} carries Ping's \"type\": \"Managed Object\" marker but is not a Ping \
                 object (alpha_/bravo_ user, role, group, organization, application, \
                 assignment); console-created objects carry it too. Pass --force if {name} \
                 is yours and should be {action}"
            )))
        }
        _ => Ok(()),
    }
}
/// After a delete, stop tracking the object's hooks: their slots went with
/// it. Reported, never fatal — the tenant write already landed.
fn untrack_hooks(tenant: &str, object: &str) {
    match crate::scripts::sync::forget_object_hooks(tenant, object) {
        Ok(forgotten) if forgotten.is_empty() => {}
        Ok(forgotten) => eprintln!(
            "untracked hook(s) {} from script sync (local files kept)",
            forgotten
                .iter()
                .map(|name| format!("managed/{name}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Err(error) => eprintln!(
            "note: could not untrack {object}'s hooks from script sync ({error}); \
             `aic script sync --resolve remote --force` forgets them"
        ),
    }
}

fn confirmation(message: &str) {
    eprintln!("{message}; undo it from the TUI history overlay");
}
/// The only path from a mutated document to the tenant.
async fn put_managed(
    ok: &WriteOk<'_>,
    base: &Value,
    doc: Value,
    expect: &[api::ConfigConfirm],
) -> Result<()> {
    api::replace_managed_confirmed(ok.tenant, base, doc, expect, ok.confirmed_prod).await
}

fn relationship_expectations(previous: &Value, updated: &Value) -> Result<Vec<api::ConfigConfirm>> {
    Ok(changed_objects(previous, updated)?
        .into_iter()
        .filter_map(|object| {
            let name = object.get("name").and_then(Value::as_str)?.to_string();
            Some(api::ConfigConfirm::ObjectContent {
                name,
                content: object,
            })
        })
        .collect())
}

/// The objects present on both sides whose content a write changed — what a
/// multi-object verb's `--json` prints, rather than the whole document
/// (67 objects, 1.8 MB, on one operator's tenant).
fn changed_objects(previous: &Value, updated: &Value) -> Result<Vec<Value>> {
    Ok(api::objects(updated)?
        .iter()
        .filter(|object| {
            object
                .get("name")
                .and_then(Value::as_str)
                .and_then(|name| api::object_named(previous, name).ok())
                .is_some_and(|before| !api::object_content_equal(before, object))
        })
        .cloned()
        .collect())
}
fn output(json_output: bool, value: &Value) -> Result<()> {
    if json_output {
        print_json(value)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct ManagedSummaryOutput {
    name: String,
    properties: usize,
    hooks_inline: Vec<String>,
    hooks_file: Vec<String>,
}
fn summary_output(summary: &api::ObjectSummary) -> ManagedSummaryOutput {
    ManagedSummaryOutput {
        name: summary.name.clone(),
        properties: summary.properties,
        hooks_inline: summary.hooks_inline.clone(),
        hooks_file: summary.hooks_file.clone(),
    }
}
fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".into()
    } else {
        values.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;

    /// `relationship verify` refuses a type `config/managed` no longer declares,
    /// which is the usual subject of `--empty-entry`, so a read-back failure on
    /// that path must not send the operator to it. The other path still does.
    #[test]
    fn repair_readback_points_empty_entry_at_its_own_dry_run() {
        let applied = repair::Applied {
            backup: std::path::PathBuf::from("/backups/repo.ds.json"),
            readback: repair::Readback::Unread {
                error: "boom".into(),
            },
        };
        let entry = repair_readback("thing", &repair_check("thing", true), &applied)
            .unwrap_err()
            .to_string();
        assert!(
            entry.contains("repair thing --empty-entry --dry-run"),
            "{entry}"
        );
        assert!(
            !entry.contains("Run `aic managed relationship verify"),
            "{entry}"
        );
        let mapping = repair_readback("thing.owner", &repair_check("thing", false), &applied)
            .unwrap_err()
            .to_string();
        assert!(
            mapping.contains("`aic managed relationship verify thing`"),
            "{mapping}"
        );
    }

    /// `--force` overrides the marker and never a Ping object.
    #[test]
    fn force_overrides_the_marker_but_never_a_ping_object() {
        let marked = serde_json::json!({"name": "console_widget", "type": "Managed Object"});
        let ping = serde_json::json!({"name": "alpha_user", "type": "Managed Object"});
        let custom = serde_json::json!({"name": "alpha_lock"});
        assert!(refuse_shipped(&marked, "deleted", false).is_err());
        assert!(refuse_shipped(&marked, "deleted", true).is_ok());
        assert!(refuse_shipped(&ping, "deleted", true).is_err());
        assert!(refuse_shipped(&custom, "deleted", false).is_ok());
    }
    use clap::Parser;

    #[test]
    fn relationship_map_accepts_the_operation_gates_and_rejects_other_force_kinds() {
        let cli = Cli::try_parse_from([
            "aic",
            "managed",
            "relationship",
            "map",
            "a.link",
            "--dry-run",
            "--force",
            "--yes",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(crate::cli::Command::Managed {
                command: ManagedCommand::Relationship {
                    command: RelationshipCommand::Map {
                        dry_run: true,
                        yes: true,
                        json: true,
                        ..
                    }
                }
            })
        ));
        assert!(
            Cli::try_parse_from([
                "aic",
                "managed",
                "relationship",
                "map",
                "a.link",
                "--force=syntax-check"
            ])
            .is_err()
        );
    }

    #[test]
    fn relationship_conversion_verbs_parse_with_distinct_metadata_restore() {
        for args in [
            vec![
                "aic",
                "managed",
                "relationship",
                "capture-links",
                "--lower",
                "dev",
                "--upper",
                "uat",
                "--output",
                "capture.json",
                "--new-pair",
                "new_widget.owner",
                "--dry-run",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "restore-links",
                "capture.json",
                "--tenant",
                "uat",
                "--force",
                "--yes",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "rollback-map",
                "capture.json",
                "--force",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "map",
                "a.link",
                "--capture",
                "capture.json",
                "--force",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "restore",
                "definition.json",
                "--dry-run",
            ],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
        assert!(
            Cli::try_parse_from([
                "aic",
                "managed",
                "relationship",
                "restore-links",
                "capture.json",
                "--force=syntax-check"
            ])
            .is_err()
        );
    }

    /// A multi-object verb's `--json` names only what it changed: not an
    /// untouched object, and not one that exists on one side only (a created
    /// or deleted object is reported by its own field).
    #[test]
    fn changed_objects_are_the_ones_present_on_both_sides_and_different() {
        let before = serde_json::json!({"objects": [
            {"name": "a", "schema": {"title": "A"}},
            {"name": "b", "schema": {"title": "B"}},
            {"name": "gone", "schema": {}},
        ]});
        let after = serde_json::json!({"objects": [
            {"name": "a", "schema": {"title": "A2"}},
            {"name": "b", "schema": {"title": "B"}},
            {"name": "new", "schema": {}},
        ]});
        let changed = changed_objects(&before, &after).unwrap();
        assert_eq!(
            changed,
            [serde_json::json!({"name": "a", "schema": {"title": "A2"}})]
        );
    }

    #[test]
    fn object_key_parser_rejects_missing_or_extra_dots() {
        assert_eq!(
            parse_object_key("alpha_user.name").unwrap(),
            ("alpha_user".into(), "name".into())
        );
        for bad in ["alpha_user", ".name", "alpha_user.", "a.b.c"] {
            assert!(parse_object_key(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn field_edit_requires_an_attribute() {
        assert!(ensure_edit_attrs(&FieldAttrs::default()).is_err());
    }

    #[test]
    fn enum_flags_parse_and_conflict() {
        for args in [
            [
                "aic",
                "managed",
                "field",
                "add",
                "test.status",
                "--type",
                "string",
                "--enum",
                "new",
            ]
            .as_slice(),
            [
                "aic",
                "managed",
                "field",
                "edit",
                "test.status",
                "--enum",
                "new",
                "--enum",
                "done:Done",
            ]
            .as_slice(),
            [
                "aic",
                "managed",
                "field",
                "edit",
                "test.status",
                "--clear-enum",
            ]
            .as_slice(),
            [
                "aic",
                "managed",
                "field",
                "edit",
                "test.status",
                "--enum",
                "new",
                "--force",
            ]
            .as_slice(),
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
        assert!(
            Cli::try_parse_from([
                "aic",
                "managed",
                "field",
                "edit",
                "test.status",
                "--enum",
                "new",
                "--clear-enum",
            ])
            .is_err()
        );
    }

    #[test]
    fn field_edit_force_reaches_the_domain_narrowing_consent() {
        let object = serde_json::json!({
            "name": "test",
            "schema": {
                "properties": {
                    "status": {"type": "string", "enum": ["new", "done"]}
                },
                "required": [],
                "order": ["status"]
            }
        });

        for (force_arg, allowed) in [(None, false), (Some("--force"), true)] {
            let mut args = vec![
                "aic",
                "managed",
                "field",
                "edit",
                "test.status",
                "--enum",
                "new",
            ];
            if let Some(force_arg) = force_arg {
                args.push(force_arg);
            }
            let cli = Cli::try_parse_from(args).unwrap();
            let (attrs, force) = match cli.command {
                Some(crate::cli::Command::Managed {
                    command:
                        ManagedCommand::Field {
                            command: FieldCommand::Edit { attrs, force, .. },
                        },
                }) => (attrs, force),
                other => panic!("not a managed field edit: {other:?}"),
            };
            let edit = field_edit_spec(attrs, force.operation()).unwrap();
            assert_eq!(edit.allow_narrowing, allowed);
            let result = ops::apply_field_edit(&object, "status", &edit);
            assert_eq!(result.is_ok(), allowed);
        }
    }

    #[test]
    fn omitted_field_edit_force_names_the_new_spelling() {
        let error = ensure_narrowing_consent(&["done".into()], false).unwrap_err();
        assert!(
            matches!(error, crate::Error::Config(message) if message.contains("--force") && !message.contains("--allow-narrowing"))
        );
    }

    #[test]
    fn ineffective_narrowing_flags_are_removed_without_replacement() {
        for args in [
            vec![
                "aic",
                "managed",
                "field",
                "add",
                "test.status",
                "--type",
                "string",
                "--allow-narrowing",
            ],
            vec![
                "aic",
                "managed",
                "field",
                "add",
                "test.status",
                "--type",
                "string",
                "--force",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "set",
                "test.owner",
                "--target",
                "test2",
                "--forward",
                "one",
                "--allow-narrowing",
            ],
            vec![
                "aic",
                "managed",
                "relationship",
                "set",
                "test.owner",
                "--target",
                "test2",
                "--forward",
                "one",
                "--force",
            ],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn get_flags_parse() {
        let parsed = Cli::try_parse_from([
            "aic",
            "managed",
            "get",
            "alpha_user",
            "--fields",
            "custom_idProofing*",
            "--fields",
            "name",
            "--hook-sources",
        ])
        .unwrap();
        match parsed.command {
            Some(crate::cli::Command::Managed {
                command:
                    ManagedCommand::Get {
                        name,
                        fields,
                        hook_sources,
                        ..
                    },
            }) => {
                assert_eq!(name, "alpha_user");
                assert_eq!(fields, ["custom_idProofing*", "name"]);
                assert!(hook_sources);
            }
            other => panic!("not a managed get: {other:?}"),
        }

        let parsed = Cli::try_parse_from(["aic", "managed", "get", "alpha_user"]).unwrap();
        match parsed.command {
            Some(crate::cli::Command::Managed {
                command:
                    ManagedCommand::Get {
                        fields,
                        hook_sources,
                        ..
                    },
            }) => {
                assert!(fields.is_empty());
                assert!(!hook_sources);
            }
            other => panic!("not a managed get: {other:?}"),
        }
    }

    #[test]
    fn repair_empty_entry_flag_parses() {
        let parsed = Cli::try_parse_from([
            "aic",
            "managed",
            "relationship",
            "repair",
            "gone",
            "--empty-entry",
            "--dry-run",
        ])
        .unwrap();
        let Some(crate::cli::Command::Managed {
            command:
                ManagedCommand::Relationship {
                    command:
                        RelationshipCommand::Repair {
                            field,
                            empty_entry,
                            dry_run,
                            ..
                        },
                },
        }) = parsed.command
        else {
            panic!("not a relationship repair")
        };
        assert_eq!(field, "gone");
        assert!(empty_entry && dry_run);
    }

    #[test]
    fn get_json_keeps_only_the_unmatched_pattern_warning() {
        let unmatched = ["nope*".to_string()];
        let plain = get_notes(false, true, Some((1, 4)), &unmatched, 2);
        assert_eq!(plain.len(), 3, "{plain:?}");
        assert!(plain.iter().any(|n| n.starts_with("omitted 2 inline hook")));
        assert!(plain.iter().any(|n| n == "showing 1 of 4 properties"));
        let json = get_notes(true, true, Some((1, 4)), &unmatched, 2);
        assert_eq!(json, ["--fields \"nope*\" matched no properties"]);
        assert!(get_notes(true, true, None, &[], 3).is_empty());
        // A projection never reports elided hooks, json or not.
        assert!(get_notes(false, false, None, &[], 3).is_empty());

        let parsed =
            Cli::try_parse_from(["aic", "managed", "get", "alpha_user", "--json"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(crate::cli::Command::Managed {
                command: ManagedCommand::Get { json: true, .. }
            })
        ));
    }

    #[test]
    fn managed_write_commands_parse() {
        for args in [
            ["aic", "managed", "object", "create", "test"].as_slice(),
            ["aic", "managed", "object", "rename", "old", "new"].as_slice(),
            ["aic", "managed", "object", "delete", "test"].as_slice(),
            [
                "aic",
                "managed",
                "field",
                "add",
                "test.code",
                "--type",
                "string",
            ]
            .as_slice(),
            [
                "aic",
                "managed",
                "field",
                "edit",
                "test.code",
                "--title",
                "Code",
            ]
            .as_slice(),
            ["aic", "managed", "field", "rename", "test.code", "new_code"].as_slice(),
            ["aic", "managed", "field", "delete", "test.code"].as_slice(),
            ["aic", "managed", "hook", "add", "test", "onCreate"].as_slice(),
            [
                "aic",
                "managed",
                "relationship",
                "set",
                "test.owner",
                "--target",
                "test2",
                "--forward",
                "one",
            ]
            .as_slice(),
            ["aic", "managed", "relationship", "delete", "test.owner"].as_slice(),
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn omitted_reverse_flag_is_absent_rather_than_none() {
        // `--reverse` used to carry `default_value = "none"`, which made a
        // `set` that only touched a title strip the reverse declaration. Absent
        // has to reach the handler as `None` so it can mean "keep".
        let parsed = Cli::try_parse_from([
            "aic",
            "managed",
            "relationship",
            "set",
            "test.owner",
            "--target",
            "test2",
            "--forward",
            "one",
        ])
        .unwrap();
        let reverse = relationship_set_reverse(&parsed);
        assert_eq!(reverse, None);

        let parsed = Cli::try_parse_from([
            "aic",
            "managed",
            "relationship",
            "set",
            "test.owner",
            "--target",
            "test2",
            "--forward",
            "one",
            "--reverse",
            "many",
        ])
        .unwrap();
        assert_eq!(relationship_set_reverse(&parsed).as_deref(), Some("many"));
    }

    /// The `--reverse` value as parsed, or `None` when the flag was omitted.
    fn relationship_set_reverse(cli: &Cli) -> Option<String> {
        match &cli.command {
            Some(crate::cli::Command::Managed { command }) => match command {
                ManagedCommand::Relationship {
                    command: RelationshipCommand::Set { reverse, .. },
                } => reverse.clone(),
                other => panic!("not a relationship set: {other:?}"),
            },
            other => panic!("not a managed command: {other:?}"),
        }
    }

    /// Round-3 suggestion 5: an empty pair read "untested" per direction and
    /// then "resolved" from the probe. A verified probe replaces "untested";
    /// a probe that did not resolve, or left its link behind, does not.
    #[test]
    fn a_verified_probe_replaces_untested_on_an_unsampled_direction() {
        let direction = DirectionOutput {
            from: "a.b".into(),
            to: "c.d".into(),
            scanned: 3,
            links: vec![],
            ref_filter: None,
            runtime: None,
        };
        let probe =
            |outcome: Option<verify::LinkOutcome>, removed: bool, failed: bool| ProbeOutput {
                from: "a.b".into(),
                to: "c.d".into(),
                near_id: None,
                far_id: None,
                outcome,
                note: None,
                removed,
                runtime: Vec::new(),
                failed,
            };
        assert!(probe(Some(verify::LinkOutcome::Resolved), true, false).verified());
        // The discriminating case: resolved and removed, but a runtime
        // check failed. Neither the predicate nor the line may say verified.
        let disagreed = probe(Some(verify::LinkOutcome::Resolved), true, true);
        assert!(!disagreed.verified());
        assert!(!probe_verdict(&disagreed).starts_with("verified"));
        assert!(
            probe_verdict(&probe(Some(verify::LinkOutcome::Resolved), true, false))
                .starts_with("verified")
        );
        assert!(!probe(Some(verify::LinkOutcome::Resolved), false, true).verified());
        assert!(!probe(Some(verify::LinkOutcome::Missing), true, true).verified());
        assert!(!probe(None, false, false).verified());
        let verified = unsampled_direction_line(&direction, true);
        assert!(!verified.contains("untested"), "{verified}");
        assert!(verified.contains("no pre-existing links in 3 records read"));
        assert!(verified.contains("probe-verified"));
        assert!(unsampled_direction_line(&direction, false).contains("untested"));
    }

    #[test]
    fn a_required_end_on_either_side_is_not_probed() {
        let end = |object: &str, key: &str| verify::End {
            object: object.into(),
            key: key.into(),
            targets: vec![],
            array: false,
            reverse_key: None,
            mapping: None,
            conflicting: vec![],
        };
        let managed = |required: serde_json::Value| {
            serde_json::json!({"objects": [
                {"name": "a", "schema": {"properties": {"one": {}}, "required": required}},
                {"name": "b", "schema": {"properties": {"many": {}}, "required": []}},
            ]})
        };
        let (a, b) = (end("a", "one"), end("b", "many"));
        assert_eq!(required_end(&managed(serde_json::json!([])), &a, &b), None);
        assert_eq!(
            required_end(&managed(serde_json::json!(["other"])), &a, &b),
            None
        );
        let doc = managed(serde_json::json!(["one"]));
        assert_eq!(required_end(&doc, &a, &b).as_deref(), Some("a.one"));
        assert_eq!(required_end(&doc, &b, &a).as_deref(), Some("a.one"));
        let note = ProbeOutput::untested(&a, &b, "untested: x".into());
        assert!(!note.failed && !note.verified() && note.outcome.is_none());
    }

    #[test]
    fn check_probes_both_ends_even_when_nothing_is_new() {
        let doc = serde_json::json!({"objects": [
            {"name": "a", "schema": {"properties": {"link": {
                "type": "relationship",
                "reverseRelationship": true,
                "reversePropertyName": "back",
                "resourceCollection": [{"path": "managed/b"}],
            }}}},
            {"name": "b", "schema": {"properties": {"back": {
                "type": "array",
                "items": {
                    "type": "relationship",
                    "reverseRelationship": true,
                    "reversePropertyName": "link",
                    "resourceCollection": [{"path": "managed/a"}],
                },
            }}}},
        ]});
        let labels = |ends: Vec<relink::NewEnd>| {
            ends.into_iter()
                .map(|end| format!("{}.{}", end.object, end.key))
                .collect::<Vec<_>>()
        };
        assert!(activation_ends(&doc, &doc, "a", "link", false).is_empty());
        assert_eq!(
            labels(activation_ends(&doc, &doc, "a", "link", true)),
            ["a.link", "b.back"]
        );
        let empty = serde_json::json!({"objects": [
            {"name": "a", "schema": {"properties": {}}},
            {"name": "b", "schema": {"properties": {}}},
        ]});
        let mut new = labels(activation_ends(&empty, &doc, "a", "link", true));
        new.sort();
        assert_eq!(new, ["a.link", "b.back"], "no end is probed twice");
        assert!(activation_check("a", "link", &[], false).is_ok());
        assert!(activation_check("a", "link", &["a.link".into()], false).is_err());
    }

    #[test]
    fn a_failed_removal_is_not_reported_as_still_mapped() {
        let error = crate::Error::Config("the write's outcome is unknown".into());
        let pending = ["b.back".to_string(), "a.link".to_string()];
        let message = removal_failure_message("a.link", "a", &["c.x".into()], &pending, &error);
        assert!(!message.contains("Still mapped"), "{message}");
        assert!(message.contains("and c.x unmapped"), "{message}");
        assert!(
            message.contains("removing b.back's mapping failed"),
            "{message}"
        );
        assert!(message.contains("Not attempted: a.link."), "{message}");
        assert!(message.contains("verify a"), "{message}");
        let last = removal_failure_message("a.link", "a", &[], &pending[1..], &error);
        assert!(!last.contains("Not attempted"), "{last}");
    }

    #[test]
    fn a_pr_miss_is_judged_only_while_the_same_edge_is_held() {
        let end = |mapped: bool| verify::End {
            object: "a".into(),
            key: "link".into(),
            targets: vec!["managed/b".into()],
            array: false,
            reverse_key: None,
            mapping: mapped.then(|| verify::Mapping {
                kind: "reference".into(),
                resource_path: None,
                property_name: None,
            }),
            conflicting: vec![],
        };
        let edge = |id: &str| relink::Edge {
            link: verify::Link {
                from_id: "r".into(),
                to_collection: "managed/b".into(),
                to_id: "t".into(),
                edge_id: Some(id.into()),
            },
            properties: serde_json::Map::new(),
        };
        let (mapped, generic) = (end(true), end(false));
        let e1 = [edge("E1")];
        let e2 = [edge("E2")];
        // Still holding E1: the miss is evidence.
        assert_eq!(
            runtime_verdict(&mapped, false, Some("E1"), Some(&e1)),
            Some(verify::Runtime::MappedNotLive)
        );
        assert_eq!(
            runtime_verdict(&generic, false, Some("E1"), Some(&e1)),
            Some(verify::Runtime::GenericConfirmed)
        );
        // E1 removed, E2 created before the re-read: unknown, both ways.
        assert_eq!(runtime_verdict(&mapped, false, Some("E1"), Some(&e2)), None);
        assert_eq!(
            runtime_verdict(&generic, false, Some("E1"), Some(&e2)),
            None
        );
        // Gone, or never identified.
        assert_eq!(runtime_verdict(&mapped, false, Some("E1"), Some(&[])), None);
        assert_eq!(runtime_verdict(&mapped, false, None, Some(&e1)), None);
        // A hit is a hit.
        assert_eq!(
            runtime_verdict(&mapped, true, None, None),
            Some(verify::Runtime::MappedConfirmed)
        );
    }

    #[test]
    fn only_a_live_end_counts_as_verified() {
        let entry = |end: &str, outcome| ActivationOutput {
            end: end.into(),
            outcome,
            seconds: None,
            detail: None,
        };
        assert!(unverified_ends(&[]).is_empty());
        assert!(unverified_ends(&[entry("a.b", "live")]).is_empty());
        assert_eq!(
            unverified_ends(&[
                entry("a.b", "live"),
                entry("c.d", "no_records"),
                entry("e.f", "timeout"),
                entry("g.h", "error"),
            ]),
            ["c.d (no_records)", "e.f (timeout)", "g.h (error)"]
        );
    }
    #[test]
    fn request_finished_acknowledgment_parses_only_on_recovery_verbs() {
        use crate::cli::Cli;
        use clap::Parser;
        for (verb, operand) in [
            ("map", "a.link"),
            ("restore-links", "capture.json"),
            ("rollback-map", "capture.json"),
        ] {
            assert!(
                Cli::try_parse_from([
                    "aic",
                    "managed",
                    "relationship",
                    verb,
                    operand,
                    "--acknowledge-request-finished",
                    "--dry-run"
                ])
                .is_ok(),
                "{verb}"
            );
        }
        assert!(
            Cli::try_parse_from([
                "aic",
                "managed",
                "relationship",
                "capture-links",
                "--lower",
                "dev",
                "--upper",
                "uat",
                "--output",
                "capture.json",
                "--acknowledge-request-finished"
            ])
            .is_err()
        );
    }
}
