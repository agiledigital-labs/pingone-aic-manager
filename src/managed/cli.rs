//! `aic managed` parser and managed-schema command implementation.

use clap::{ArgAction, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::cli::force::OperationForce;
use crate::cli::{WriteOk, ensure_prod_confirmed, print_json, print_table, tenant_for};
use crate::managed::{api, ops, spec, state, verify};
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
    let object = api::object_named(&api::get_managed(&tenant_for(tenant)?).await?, &name)?.clone();
    let view = spec::prepare_managed_get(object, &name, &fields, hook_sources)
        .map_err(crate::Error::Config)?;
    let whole = projection == spec::GetProjection::Object;
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
    Ok(())
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
            put_managed(&ok, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {name} created"));
            output(json, &updated)
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
            put_managed(&ok, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {old} renamed to {new}"));
            output(json, &updated)
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
            let (updated, _) =
                ops::delete_object_in_doc(&doc, &name).map_err(crate::Error::Config)?;
            let mut undo = DiskLog::load_default()?;
            ops::record_delete_object_undo(&mut undo, &tenant, &name, &doc)?;
            let expect = [api::ConfigConfirm::ObjectAbsent { name: name.clone() }];
            put_managed(&ok, updated.clone(), &expect).await?;
            confirmation(&format!("managed object {name} deleted"));
            output(json, &updated)
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
            put_managed(&ok, updated.clone(), &expect).await?;
            confirmation(&format!("relationship {source_object}.{key} set"));
            output(json, &updated)
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
            let leftovers = verify::delete_leftovers(&api::get_repo_ds(&tenant).await?, &ends);
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Restore relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            put_managed(&ok, updated.clone(), &expect).await?;
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
            output(json, &updated)
        }
        RelationshipCommand::Verify {
            target,
            sample,
            scan,
            tenant,
            json,
        } => relationship_verify(target, sample, scan, tenant, json).await,
    }
}

#[derive(Serialize)]
struct DirectionOutput {
    from: String,
    to: String,
    /// Records read looking for links.
    scanned: usize,
    links: Vec<SampledLink>,
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
    Ok(DirectionOutput {
        from: format!("{}.{}", near.object, near.key),
        to: format!("{}.{}", far.object, far.key),
        scanned: records.len(),
        links,
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
    api::replace_object(doc, object, updated.clone())?;
    let expect = [api::ConfigConfirm::ObjectContent {
        name: object.to_string(),
        content: updated,
    }];
    put_managed(ok, doc.clone(), &expect).await
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
fn confirmation(message: &str) {
    eprintln!("{message}; undo it from the TUI history overlay");
}
/// The only path from a mutated document to the tenant.
async fn put_managed(ok: &WriteOk<'_>, doc: Value, expect: &[api::ConfigConfirm]) -> Result<()> {
    api::replace_managed_confirmed(ok.tenant, doc, expect, ok.confirmed_prod).await
}

fn relationship_expectations(previous: &Value, updated: &Value) -> Result<Vec<api::ConfigConfirm>> {
    Ok(api::objects(updated)?
        .iter()
        .filter_map(|object| {
            let name = object.get("name").and_then(Value::as_str)?;
            let before = api::object_named(previous, name).ok()?;
            (!api::object_content_equal(before, object)).then(|| {
                api::ConfigConfirm::ObjectContent {
                    name: name.to_string(),
                    content: object.clone(),
                }
            })
        })
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
