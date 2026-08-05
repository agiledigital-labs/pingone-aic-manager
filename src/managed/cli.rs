//! `aic managed` parser and managed-schema command implementation.

use clap::{ArgAction, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::cli::{print_json, print_table, tenant_config_for, tenant_for};
use crate::config::TenantTheme;
use crate::managed::{api, ops, spec, state};
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
    /// Print one managed object's full definition as JSON.
    Get {
        name: String,
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
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        name: String,
        #[arg(long)]
        tenant: Option<String>,
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
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Edit {
        field: String,
        #[command(flatten)]
        attrs: FieldAttrs,
        #[arg(long)]
        tenant: Option<String>,
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
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        field: String,
        #[arg(long)]
        tenant: Option<String>,
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
    /// Permit removing values from an existing allowed-value constraint.
    #[arg(long)]
    allow_narrowing: bool,
}

#[derive(Subcommand, Debug)]
pub enum HookCommand {
    Add {
        object: String,
        hook: String,
        #[arg(long)]
        tenant: Option<String>,
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
        #[arg(long, default_value = "none")]
        reverse: String,
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
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Delete {
        field: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(cmd: ManagedCommand) -> Result<()> {
    match cmd {
        ManagedCommand::List { tenant, json } => list(tenant, json).await,
        ManagedCommand::Get { name, tenant } => print_json(api::object_named(
            &api::get_managed(&tenant_for(tenant)?).await?,
            &name,
        )?),
        ManagedCommand::Object { command } => object(command).await,
        ManagedCommand::Field { command } => field(command).await,
        ManagedCommand::Hook { command } => hook(command).await,
        ManagedCommand::Relationship { command } => relationship(command).await,
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
            let edit_spec = field_edit_spec(attrs)?;
            let removed = state::properties(&previous)
                .and_then(|properties| properties.get(&key))
                .map(|property| ops::removed_enum_values(property, &edit_spec.enum_change))
                .unwrap_or_default();
            // The transform refuses this too, but its message can't name a flag
            // it doesn't know about.
            if !removed.is_empty() && !edit_spec.allow_narrowing {
                return Err(crate::Error::Config(format!(
                    "dropping enum values {} would leave records that fail whole-record updates; pass --allow-narrowing to confirm",
                    removed.join(", ")
                )));
            }
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
                reverse: reverse_cardinality(&reverse)?,
                reverse_key: reverse_key.unwrap_or_default(),
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
            let source = api::object_named(&doc, &source_object)?;
            let parsed = state::properties(source)
                .and_then(|p| p.get(&key))
                .and_then(ops::parse_relationship)
                .ok_or_else(|| {
                    crate::Error::Config(format!("field '{field}' is not a relationship"))
                })?;
            let spec = state::RelationshipSpec {
                source_object: source_object.clone(),
                key: "__deleted_relationship__".into(),
                title: String::new(),
                description: String::new(),
                target_object: parsed.target.clone(),
                forward: parsed.forward,
                reverse: state::ReverseCardinality::None,
                reverse_key: String::new(),
                searchable: false,
                viewable: false,
                user_editable: false,
                required: false,
                validate: false,
                ref_properties: Vec::new(),
            };
            let previous = state::PreviousRelationship {
                old_key: key.clone(),
                old_target: parsed.target,
                old_reverse_key: parsed.reverse_key,
            };
            let updated = ops::apply_relationship_spec(&doc, &spec, Some(&previous))
                .map_err(crate::Error::Config)?;
            // The transform intentionally requires a new key. Remove the sentinel it inserted.
            let mut updated = updated;
            let object = api::object_named(&updated, &source_object)?.clone();
            let clean = ops::apply_delete_field(
                &object,
                "__deleted_relationship__",
                &spec::DeleteFieldSpec,
            )
            .map_err(crate::Error::Config)?;
            api::replace_object(&mut updated, &source_object, clean)?;
            record_config_undo(
                &tenant,
                &doc,
                &updated,
                format!("Restore relationship {source_object}.{key}"),
            )?;
            let expect = relationship_expectations(&doc, &updated)?;
            put_managed(&ok, updated.clone(), &expect).await?;
            confirmation(&format!("relationship {source_object}.{key} deleted"));
            output(json, &updated)
        }
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
fn field_edit_spec(attrs: FieldAttrs) -> Result<spec::FieldEditSpec> {
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
        allow_narrowing: attrs.allow_narrowing,
    })
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
/// Permission to write to one tenant: either it isn't production, or `--yes` was
/// given. Only [`ensure_prod_confirmed`] constructs one and only [`put_managed`]
/// consumes one, so a command cannot reach the tenant without having passed the
/// check — the guard is duplicated across nine call sites and nothing else would
/// notice a tenth that forgot it.
#[must_use]
struct WriteOk<'a> {
    tenant: &'a str,
    /// Whether the user explicitly confirmed. Forwarded to the API layer, which
    /// applies its own production gate.
    confirmed_prod: bool,
}

/// Refuse an unconfirmed production write. Called *before* fetching the document
/// or recording undo, so a refusal leaves no undo entry for a write that never
/// happened.
fn ensure_prod_confirmed(tenant: &str, yes: bool) -> Result<WriteOk<'_>> {
    if tenant_config_for(Some(tenant.to_string()))?.theme == TenantTheme::Production && !yes {
        return Err(crate::Error::Config(
            "tenant is production — re-run with --yes to confirm the write".into(),
        ));
    }
    Ok(WriteOk {
        tenant,
        confirmed_prod: yes,
    })
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
                "--allow-narrowing",
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
}
