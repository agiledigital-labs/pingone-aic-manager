//! `aic managed` parser and managed-schema command implementation.

use clap::{ArgAction, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::cli::force::OperationForce;
use crate::cli::{WriteOk, ensure_prod_confirmed, print_json, print_table, tenant_for};
use crate::managed::{api, ops, relink, spec, state, verify};
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
        #[arg(long)]
        tenant: Option<String>,
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
    Add {
        field: String,
        #[arg(long = "type")]
        field_type: String,
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
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(cmd: ManagedCommand) -> Result<()> {
    match cmd {
        ManagedCommand::List { tenant, json } => list(tenant, json).await,
        ManagedCommand::Get {
            name,
            fields,
            hook_sources,
            property,
            schema_only,
            tenant,
        } => {
            let projection = match (property, schema_only) {
                (Some(property), _) => spec::GetProjection::Property(property),
                (None, true) => spec::GetProjection::Schema,
                (None, false) => spec::GetProjection::Object,
            };
            get(name, fields, hook_sources, projection, tenant).await
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
    if let Some((shown, total)) = view.fields_shown {
        eprintln!("showing {shown} of {total} properties");
    }
    for pattern in &view.unmatched_patterns {
        eprintln!("--fields {pattern:?} matched no properties");
    }
    // Object-level hooks sit outside `schema`, so a projection never shows
    // them and has nothing to report about eliding them.
    if whole && view.elided_hooks > 0 {
        let noun = if view.elided_hooks == 1 {
            "source"
        } else {
            "sources"
        };
        eprintln!(
            "omitted {} inline hook {noun}; pass --hook-sources to include them",
            view.elided_hooks
        );
    }
    print_relationship_storage(&tenant, &managed, &name, storage_key).await;
    Ok(())
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
    eprintln!("relationship storage (config/repo.ds):");
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
            tenant,
            yes,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            let old_object = api::object_named(&doc, &old)?;
            refuse_shipped(old_object, "renamed")?;
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
            tenant,
            yes,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let doc = api::get_managed(&tenant).await?;
            refuse_shipped(api::object_named(&doc, &name)?, "deleted")?;
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
    let HookCommand::Add {
        object,
        hook,
        tenant,
        yes,
        json,
    } = command;
    let tenant = tenant_for(tenant)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let mut doc = api::get_managed(&tenant).await?;
    let previous = api::object_named(&doc, &object)?.clone();
    let updated = ops::apply_add_hook(&previous, &object, &hook).map_err(crate::Error::Config)?;
    if updated == previous {
        return Err(crate::Error::Config(format!(
            "hook '{hook}' already exists on {object}"
        )));
    }
    replace_object_write(&ok, &mut doc, &object, &previous, updated.clone()).await?;
    confirmation(&format!("hook {object}.{hook} added"));
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
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Revert relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            put_managed(&ok, &doc, updated.clone(), &expect).await?;
            confirmation(&format!("relationship {source_object}.{key} set"));
            let activation = wait_for_new_ends(&tenant, &doc, &updated).await;
            let one_sided = match relink::reverse_added(&doc, &updated, &source_object, &key) {
                Some((far, far_key)) => {
                    sample_one_sided(&tenant, &updated, &source_object, &key, &far, &far_key).await
                }
                None => None,
            };
            output(
                json,
                &serde_json::json!({
                    "changed": changed_objects(&doc, &updated)?,
                    "activation": activation,
                    "one_sided": one_sided,
                }),
            )
        }
        RelationshipCommand::Delete {
            field,
            tenant,
            yes,
            json,
        } => {
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
            // Informational only: the write itself never needs repo.ds for a
            // removal, so failing to read it must not block one (round 7).
            let leftovers = match api::get_repo_ds(&tenant).await {
                Ok(repo_ds) => verify::delete_leftovers(&repo_ds, &ends),
                Err(error) => {
                    eprintln!(
                        "note: could not read config/repo.ds ({error}); not checking for \
                         mappings this delete leaves behind"
                    );
                    Vec::new()
                }
            };
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Restore relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            put_managed(&ok, &doc, updated.clone(), &expect).await?;
            confirmation(&format!("relationship {source_object}.{key} deleted"));
            if !leftovers.is_empty() {
                eprintln!(
                    "note: config/repo.ds still maps {}; aic does not edit it. Recreating the \
                     same key with the same target reused the mapping when measured; aic \
                     refuses one with another target (docs/api/10-managed-objects.md, \
                     \"Relationship storage\")",
                    leftovers.join(", ")
                );
            }
            output(
                json,
                &serde_json::json!({ "changed": changed_objects(&doc, &updated)? }),
            )
        }
        RelationshipCommand::Verify {
            target,
            sample,
            scan,
            tenant,
            json,
        } => relationship_verify(target, sample, scan, tenant, json).await,
        RelationshipCommand::Relink {
            field,
            dry_run,
            force,
            tenant,
            yes,
            json,
        } => relationship_relink(field, dry_run, force.operation(), tenant, yes, json).await,
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

/// Waits for each relationship end `updated` declares that `doc` did not:
/// a write through one before it is live answers 200 and is lost (measured
/// 2026-10-31). Never fails the command — the schema write has landed — but
/// says loudly when it could not confirm.
async fn wait_for_new_ends(tenant: &str, doc: &Value, updated: &Value) -> Vec<ActivationOutput> {
    let mut out = Vec::new();
    for end in relink::new_ends(doc, updated) {
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

#[derive(Serialize)]
struct RelinkOutput<'a> {
    #[serde(flatten)]
    plan: &'a relink::Plan,
    relinked: Vec<relink::Edge>,
    /// Re-created, but the far end did not list the new edge when re-read.
    unconfirmed: Vec<relink::Edge>,
    /// Changed on the tenant between the plan and its turn; not written.
    changed: Vec<relink::Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
    let near_array = end_is_array(&managed, &object, &key);
    let far_array = end_is_array(&managed, &far, &far_key);
    let far_collection = format!("managed/{far}");
    let near_collection = format!("managed/{object}");

    let records = api::all_records_with(&tenant, &object, &key).await?;
    let mut judged = Vec::new();
    for record in &records {
        for edge in relink::edges_of(record, &key).map_err(crate::Error::Config)? {
            if edge.link.to_collection != far_collection {
                continue;
            }
            let outcome = far_outcome(
                &tenant,
                &far,
                &far_key,
                far_array,
                &edge.link,
                &near_collection,
            )
            .await?;
            judged.push((edge, outcome));
            if judged.len() % 100 == 0 {
                eprintln!("relink: {} links checked", judged.len());
            }
        }
    }
    let plan = relink::plan(judged);
    let mut out = RelinkOutput {
        plan: &plan,
        relinked: Vec::new(),
        unconfirmed: Vec::new(),
        changed: Vec::new(),
        error: None,
    };
    eprintln!(
        "{field}: {} records, {} links listed by {far}.{far_key}, {} to relink, {} skipped",
        records.len(),
        plan.resolved,
        plan.relink.len(),
        plan.skipped.len()
    );
    for skipped in &plan.skipped {
        eprintln!(
            "  skip {} -> {}: {}",
            skipped.edge.link.from_id, skipped.edge.link.to_id, skipped.reason
        );
    }
    if plan.relink.is_empty() || dry_run {
        return finish_relink(&out, json);
    }
    let consented = force
        || crate::cli::confirm_destructive(
            "relationship relink",
            &format!(
                "Remove and re-create {} link(s) on {field}? Each gets a new edge id; whether \
                 onUpdate hooks fire for it is unmeasured",
                plan.relink.len()
            ),
            "--force",
        )?;
    let Some(permit) = relink::authorize(dry_run, consented) else {
        return Err(crate::Error::Config(
            "relationship relink not confirmed".into(),
        ));
    };
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    for edge in &plan.relink {
        match relink_one(
            &ok,
            &permit,
            (&object, &key, near_array),
            (&far, &far_key, far_array),
            edge,
        )
        .await
        {
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

async fn far_outcome(
    tenant: &str,
    far: &str,
    far_key: &str,
    far_array: bool,
    link: &verify::Link,
    near_collection: &str,
) -> Result<verify::LinkOutcome> {
    let far_record =
        api::far_end(tenant, far, &link.to_id, far_key, far_array, &link.from_id).await?;
    let far_links = far_record
        .map(|record| verify::links_of(&record, far_key))
        .transpose()
        .map_err(crate::Error::Config)?;
    Ok(verify::judge(link, near_collection, far_links.as_deref()))
}

enum RelinkStep {
    Done,
    Unconfirmed,
    Changed,
}

/// Rechecks, rewrites and confirms one link. An `Err` is a message that
/// already says whether the link was left removed.
async fn relink_one(
    ok: &WriteOk<'_>,
    permit: &relink::RelinkPermit,
    (object, key, near_array): (&str, &str, bool),
    (far, far_key, far_array): (&str, &str, bool),
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
    let current = api::record_with(ok.tenant, object, &edge.link.from_id, key)
        .await
        .map_err(read("recheck failed; nothing written"))?;
    let current = match current {
        Some(record) => relink::edges_of(&record, key)?,
        None => Vec::new(),
    };
    if !relink::still_planned(&current, edge) {
        return Ok(RelinkStep::Changed);
    }
    let [remove, create] = relink::writes(permit, object, key, near_array, edge)?;
    api::send_relink(ok.tenant, &remove, ok.confirmed_prod)
        .await
        .map_err(read(
            "removing the link failed; it may or may not be removed",
        ))?;
    if let Err(error) = api::send_relink(ok.tenant, &create, ok.confirmed_prod).await {
        return Err(format!(
            "{label}: the link was removed and re-creating it failed ({error}). Restore it with \
             {} {} {}",
            create.method,
            create.path,
            create.body.unwrap_or(Value::Null)
        ));
    }
    let near_collection = format!("managed/{object}");
    let after = api::record_with(ok.tenant, object, &edge.link.from_id, key)
        .await
        .map_err(read("re-created, but the confirming read failed"))?;
    let after = match after {
        Some(record) => relink::edges_of(&record, key)?,
        None => Vec::new(),
    };
    let Some(new) = after.iter().find(|candidate| {
        candidate.link.to_id == edge.link.to_id
            && candidate.link.to_collection == edge.link.to_collection
    }) else {
        return Ok(RelinkStep::Unconfirmed);
    };
    let outcome = far_outcome(
        ok.tenant,
        far,
        far_key,
        far_array,
        &new.link,
        &near_collection,
    )
    .await
    .map_err(read("re-created, but the far end could not be read"))?;
    Ok(if outcome == verify::LinkOutcome::Resolved {
        RelinkStep::Done
    } else {
        RelinkStep::Unconfirmed
    })
}

fn finish_relink(out: &RelinkOutput<'_>, json: bool) -> Result<()> {
    if json {
        print_json(out)?;
    } else {
        for (label, edges) in [
            ("relinked", &out.relinked),
            ("unconfirmed", &out.unconfirmed),
            ("changed since the plan, not written", &out.changed),
        ] {
            for edge in edges {
                println!("{label}: {} -> {}", edge.link.from_id, edge.link.to_id);
            }
        }
    }
    if let Some(error) = &out.error {
        return Err(crate::Error::Config(format!(
            "relationship relink stopped after {} relinked: {error}",
            out.relinked.len()
        )));
    }
    if !out.unconfirmed.is_empty() || !out.changed.is_empty() {
        return Err(crate::Error::Config(format!(
            "relationship relink: {} unconfirmed, {} changed since the plan; re-run to check",
            out.unconfirmed.len(),
            out.changed.len()
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
    Ok(DirectionOutput {
        from: format!("{}.{}", near.object, near.key),
        to: format!("{}.{}", far.object, far.key),
        scanned: records.len(),
        links,
        ref_filter,
    })
}

async fn relationship_verify(
    target: String,
    sample: usize,
    scan: usize,
    tenant: Option<String>,
    json: bool,
) -> Result<()> {
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
    let mut pairs = Vec::new();
    for pair in report.pairs {
        let mut directions = Vec::new();
        if sample > 0
            && pair.sampleable()
            && let Some(reverse) = &pair.reverse
        {
            directions.push(sample_direction(&tenant, &pair.forward, reverse, sample, scan).await?);
            directions.push(sample_direction(&tenant, reverse, &pair.forward, sample, scan).await?);
        }
        pairs.push(PairOutput { pair, directions });
    }
    let failures = report.stale.len()
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
                        })
                        .count()
            })
            .sum::<usize>();
    if json {
        print_json(&serde_json::json!({
            "pairs": pairs,
            "stale": report.stale,
            "failures": failures,
        }))?;
    } else {
        print_verify(&pairs, &report.stale, sample);
    }
    if failures > 0 {
        return Err(crate::Error::Config(format!(
            "relationship verify found {failures} problem(s)"
        )));
    }
    Ok(())
}

/// The end a direction's links point at.
fn direction_far<'a>(pair: &'a PairOutput, direction: &DirectionOutput) -> Option<&'a verify::End> {
    let reverse = pair.pair.reverse.as_ref()?;
    let label = |end: &verify::End| format!("{}.{}", end.object, end.key);
    [&pair.pair.forward, reverse]
        .into_iter()
        .find(|end| label(end) == direction.to)
}

fn print_verify(pairs: &[PairOutput], stale: &[verify::Problem], sample: usize) {
    for pair in pairs {
        let forward = &pair.pair.forward;
        let mode = |end: &verify::End| match &end.mapping {
            Some(mapping) => format!("mapped {}", mapping.kind),
            None => "generic".into(),
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
        for direction in &pair.directions {
            let resolved = direction
                .links
                .iter()
                .filter(|link| link.outcome == verify::LinkOutcome::Resolved)
                .count();
            if direction.links.is_empty() {
                println!(
                    "  {} -> {}: untested, no links in {} records read",
                    direction.from, direction.to, direction.scanned
                );
                continue;
            }
            println!(
                "  {} -> {}: {resolved} of {} sampled links resolve from the far end",
                direction.from,
                direction.to,
                direction.links.len()
            );
            for link in &direction.links {
                let detail = match &link.outcome {
                    verify::LinkOutcome::Resolved => continue,
                    verify::LinkOutcome::Missing => "the far end does not list it".to_string(),
                    verify::LinkOutcome::FarRecordMissing => "the far record does not exist".into(),
                    verify::LinkOutcome::EdgeDiffers { far_edges } => {
                        format!("the far end lists it under edge {}", far_edges.join(", "))
                    }
                };
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

fn parse_object_key(value: &str) -> Result<(String, String)> {
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
    match value {
        "string" => Ok(spec::ScalarFieldType::String),
        "boolean" => Ok(spec::ScalarFieldType::Boolean),
        "number" => Ok(spec::ScalarFieldType::Number),
        "string[]" => Ok(spec::ScalarFieldType::StringArray),
        _ => Err(crate::Error::Config(
            "--type must be string, boolean, number, or string[]".into(),
        )),
    }
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
fn refuse_shipped(object: &Value, action: &str) -> Result<()> {
    if state::is_ping_shipped_object(object) {
        Err(crate::Error::Config(format!(
            "Ping-shipped objects cannot be {action}"
        )))
    } else {
        Ok(())
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
    use clap::Parser;

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
}
