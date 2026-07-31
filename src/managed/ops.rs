//! Background writes and undo for the managed-object schema editor.
//!
//! The managed config document has no `_rev` and no object-level patch API.
//! Every write is a read-modify-write of `/openidm/config/managed`, guarded by
//! an object-subtree content snapshot so unrelated schema drift is not
//! overwritten silently.

use serde_json::{Value, json};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
#[derive(Debug)]
pub enum ProdAction {
    Update(Box<ObjectReplacePlan>),
    RenameObject(Box<RenameObjectRequest>),
    DeleteObject(Box<DeleteObjectRequest>),
    CreateObject(Box<CreateObjectRequest>),
    WriteRelationship(Box<RelationshipWriteRequest>),
    Undo(crate::undo::UndoId),
}
use crate::managed::screen::Event;
use crate::managed::state::{
    AddFieldState, Cardinality, DeleteFieldState, DeleteObjectState, EditFieldFocus, FieldAttr,
    FieldEditState, LoadState, ParsedRelationship, PreviousRelationship, RefProperty,
    RelationshipSpec, RenameFieldState, RenameObjectState, ReverseCardinality, ScalarFieldType,
    State,
};
use crate::undo::{Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoId, UndoOp};

#[derive(Debug)]
pub struct ObjectReplacePlan {
    pub(crate) tenant_name: String,
    pub(crate) object_name: String,
    pub(crate) previous_object: Value,
    pub(crate) new_object: Value,
    pub(crate) searchable_changed: bool,
    pub(crate) success_message: String,
}

#[derive(Debug)]
pub struct UpdateOutcome {
    pub(crate) object: Value,
    pub(crate) searchable_changed: bool,
    pub(crate) success_message: String,
}

#[derive(Debug)]
pub struct UndoOutcome {
    pub(crate) description: String,
    pub(crate) object: Option<(String, Value)>,
    pub(crate) doc: Option<Value>,
}

#[derive(Debug)]
pub struct RenameObjectRequest {
    pub(crate) tenant_name: String,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    pub(crate) previous_doc: Value,
}

#[derive(Debug)]
pub struct DeleteObjectRequest {
    pub(crate) tenant_name: String,
    pub(crate) object_name: String,
    pub(crate) previous_doc: Value,
}

#[derive(Debug)]
pub struct CreateObjectRequest {
    pub(crate) tenant_name: String,
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) previous_doc: Value,
}

#[derive(Debug)]
pub struct RelationshipWriteRequest {
    pub(crate) tenant_name: String,
    pub(crate) source_object: String,
    pub(crate) spec: RelationshipSpec,
    pub(crate) previous: Option<PreviousRelationship>,
    pub(crate) previous_doc: Value,
}

/// Appends a minimal custom managed object to a whole managed config document.
pub fn create_object_in_doc(
    doc: &Value,
    name: &str,
    title: &str,
    description: &str,
) -> Result<Value, String> {
    let mut created = doc.clone();
    let objects =
        crate::managed::api::objects_mut(&mut created).map_err(|error| error.to_string())?;
    if objects
        .iter()
        .any(|object| object.get("name").and_then(Value::as_str) == Some(name))
    {
        return Err(format!("Managed object '{name}' already exists"));
    }
    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert(
        "title".into(),
        Value::String(if title.is_empty() { name } else { title }.into()),
    );
    if !description.is_empty() {
        schema.insert("description".into(), Value::String(description.into()));
    }
    schema.insert("properties".into(), Value::Object(serde_json::Map::new()));
    schema.insert("required".into(), Value::Array(Vec::new()));
    schema.insert("order".into(), Value::Array(Vec::new()));
    objects.push(json!({"name": name, "schema": schema}));
    Ok(created)
}

/// Renames an object identity and every schema relationship path that targets it.
pub fn rename_object_in_doc(doc: &Value, old: &str, new: &str) -> Result<(Value, usize), String> {
    let mut renamed = doc.clone();
    let objects =
        crate::managed::api::objects_mut(&mut renamed).map_err(|error| error.to_string())?;
    if objects
        .iter()
        .any(|object| object.get("name").and_then(Value::as_str) == Some(new))
    {
        return Err(format!("Managed object '{new}' already exists"));
    }
    let Some(source) = objects
        .iter_mut()
        .find(|object| object.get("name").and_then(Value::as_str) == Some(old))
    else {
        return Err(format!("No managed object named '{old}'"));
    };
    let Some(map) = source.as_object_mut() else {
        return Err(format!("Managed object '{old}' is malformed"));
    };
    map.insert("name".into(), Value::String(new.into()));
    let old_path = format!("managed/{old}");
    let new_path = format!("managed/{new}");
    let mut count = 0;
    for object in objects {
        let Some(properties) = object
            .pointer_mut("/schema/properties")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        for property in properties.values_mut() {
            rewrite_relationship_paths(
                property.get_mut("resourceCollection"),
                &old_path,
                &new_path,
                &mut count,
            );
            rewrite_relationship_paths(
                property.pointer_mut("/items/resourceCollection"),
                &old_path,
                &new_path,
                &mut count,
            );
        }
    }
    Ok((renamed, count))
}

fn rewrite_relationship_paths(
    collection: Option<&mut Value>,
    old_path: &str,
    new_path: &str,
    count: &mut usize,
) {
    let Some(entries) = collection.and_then(Value::as_array_mut) else {
        return;
    };
    for entry in entries {
        if entry.get("path").and_then(Value::as_str) == Some(old_path) {
            if let Some(path) = entry.get_mut("path") {
                *path = Value::String(new_path.into());
                *count += 1;
            }
        }
    }
}

/// Returns every remaining object property whose relationship targets `name`.
fn relationship_properties_targeting(objects: &[Value], name: &str) -> Vec<(String, String)> {
    let target_path = format!("managed/{name}");
    let mut matches = Vec::new();
    for object in objects {
        let Some(object_name) = object.get("name").and_then(Value::as_str) else {
            continue;
        };
        if object_name == name {
            continue;
        }
        let Some(properties) = object
            .pointer("/schema/properties")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (key, property) in properties {
            if relationship_targets_path(property, &target_path) {
                matches.push((object_name.to_string(), key.clone()));
            }
        }
    }
    matches
}

fn relationship_targets_path(property: &Value, target_path: &str) -> bool {
    [
        property.get("resourceCollection"),
        property.pointer("/items/resourceCollection"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_array)
    .flatten()
    .any(|entry| entry.get("path").and_then(Value::as_str) == Some(target_path))
}

/// Reports relationship properties that would be removed when `name` is deleted.
pub fn inbound_relationships(doc: &Value, name: &str) -> Vec<(String, String)> {
    crate::managed::api::objects(doc)
        .map(|objects| relationship_properties_targeting(objects, name))
        .unwrap_or_default()
}

/// Removes an object and every relationship property that targets it.
///
/// The managed-config API does not reconcile `schema.order` or `schema.required`,
/// so this transform prunes both lists whenever it removes a property.
pub fn delete_object_in_doc(
    doc: &Value,
    name: &str,
) -> Result<(Value, Vec<(String, String)>), String> {
    let mut deleted = doc.clone();
    let objects =
        crate::managed::api::objects_mut(&mut deleted).map_err(|error| error.to_string())?;
    let Some(index) = objects
        .iter()
        .position(|object| object.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("No managed object named '{name}'"));
    };
    objects.remove(index);
    let inbound = relationship_properties_targeting(objects, name);
    for (object_name, property_key) in &inbound {
        let Some(object) = objects
            .iter_mut()
            .find(|object| object.get("name").and_then(Value::as_str) == Some(object_name))
        else {
            continue;
        };
        if let Some(properties) = object
            .pointer_mut("/schema/properties")
            .and_then(Value::as_object_mut)
        {
            properties.remove(property_key);
        }
        for list_path in ["/schema/order", "/schema/required"] {
            if let Some(entries) = object.pointer_mut(list_path).and_then(Value::as_array_mut) {
                entries.retain(|entry| entry.as_str() != Some(property_key));
            }
        }
    }
    Ok((deleted, inbound))
}

// ── Relationship document transforms ─────────────────────────────────────

fn build_relationship_node(
    target: &str,
    reverse_key: Option<&str>,
    validate: bool,
    ref_properties: &[RefProperty],
) -> Value {
    let mut ref_properties_map = serde_json::Map::new();
    ref_properties_map.insert("_id".into(), json!({"type": "string"}));
    for property in ref_properties {
        ref_properties_map.insert(
            property.name.clone(),
            json!({
                "type": property.kind.label(),
                "label": property.label,
                "propName": property.name,
            }),
        );
    }

    let mut node = serde_json::Map::new();
    node.insert("type".into(), Value::String("relationship".into()));
    node.insert("validate".into(), Value::Bool(validate));
    node.insert(
        "reverseRelationship".into(),
        Value::Bool(reverse_key.is_some()),
    );
    if let Some(reverse_key) = reverse_key {
        node.insert(
            "reversePropertyName".into(),
            Value::String(reverse_key.into()),
        );
    }
    node.insert(
        "resourceCollection".into(),
        json!([{"path": format!("managed/{target}"), "label": target}]),
    );
    node.insert(
        "properties".into(),
        json!({
            "_ref": {"type": "string"},
            "_refProperties": {"type": "object", "properties": ref_properties_map},
        }),
    );
    Value::Object(node)
}

fn wrap_with_attrs(
    mut node: Value,
    cardinality: Cardinality,
    title: &str,
    description: &str,
    searchable: bool,
    viewable: bool,
    user_editable: bool,
) -> Value {
    let mut attrs = serde_json::Map::new();
    if !title.is_empty() {
        attrs.insert("title".into(), Value::String(title.into()));
    }
    if !description.is_empty() {
        attrs.insert("description".into(), Value::String(description.into()));
    }
    attrs.insert("searchable".into(), Value::Bool(searchable));
    attrs.insert("viewable".into(), Value::Bool(viewable));
    attrs.insert("userEditable".into(), Value::Bool(user_editable));
    attrs.insert("returnByDefault".into(), Value::Bool(false));

    match cardinality {
        Cardinality::One => {
            let map = node
                .as_object_mut()
                .expect("relationship node builder always returns an object");
            map.extend(attrs);
            node
        }
        Cardinality::Many => {
            let mut property = serde_json::Map::new();
            property.insert("type".into(), Value::String("array".into()));
            property.extend(attrs);
            property.insert("items".into(), node);
            Value::Object(property)
        }
    }
}

fn wrap_relationship_property(node: Value, forward: Cardinality, spec: &RelationshipSpec) -> Value {
    wrap_with_attrs(
        node,
        forward,
        &spec.title,
        &spec.description,
        spec.searchable,
        spec.viewable,
        spec.user_editable,
    )
}

fn source_property(spec: &RelationshipSpec) -> Value {
    let reverse_key =
        (spec.reverse != ReverseCardinality::None).then_some(spec.reverse_key.as_str());
    wrap_relationship_property(
        build_relationship_node(
            &spec.target_object,
            reverse_key,
            spec.validate,
            &spec.ref_properties,
        ),
        spec.forward,
        spec,
    )
}

fn reverse_property(spec: &RelationshipSpec) -> Option<Value> {
    let cardinality = match spec.reverse {
        ReverseCardinality::None => return None,
        ReverseCardinality::One => Cardinality::One,
        ReverseCardinality::Many => Cardinality::Many,
    };
    Some(wrap_with_attrs(
        build_relationship_node(&spec.source_object, Some(&spec.key), false, &[]),
        cardinality,
        &spec.reverse_key,
        "",
        false,
        true,
        true,
    ))
}

/// Applies relationship intent to a complete managed config document.
///
/// The old pair is removed before collision checks and insertion, which keeps
/// key renames, target repoints, dropped reverses, and self-references coherent.
pub fn apply_relationship_spec(
    doc: &Value,
    spec: &RelationshipSpec,
    previous: Option<&PreviousRelationship>,
) -> Result<Value, String> {
    crate::managed::api::object_named(doc, &spec.source_object)
        .map_err(|error| error.to_string())?;
    crate::managed::api::object_named(doc, &spec.target_object)
        .map_err(|error| error.to_string())?;
    crate::managed::state::validate_property_key(&spec.key)?;
    if spec.reverse != ReverseCardinality::None {
        if spec.reverse_key.is_empty() {
            return Err(
                "Reverse property key is required when a reverse relationship is selected".into(),
            );
        }
        crate::managed::state::validate_property_key(&spec.reverse_key)?;
        if spec.source_object == spec.target_object && spec.key == spec.reverse_key {
            return Err(
                "A self-referential relationship needs different forward and reverse property keys"
                    .into(),
            );
        }
    }

    let mut updated = doc.clone();
    if let Some(previous) = previous {
        remove_relationship_property(&mut updated, &spec.source_object, &previous.old_key, true)?;
        if let Some(reverse_key) = &previous.old_reverse_key {
            remove_relationship_property(&mut updated, &previous.old_target, reverse_key, false)?;
        }
    }

    ensure_doc_property_available(&updated, &spec.source_object, &spec.key)?;
    if spec.reverse != ReverseCardinality::None {
        ensure_doc_property_available(&updated, &spec.target_object, &spec.reverse_key)?;
    }

    let source = crate::managed::api::object_named_mut(&mut updated, &spec.source_object)
        .map_err(|error| error.to_string())?;
    properties_mut(source)?.insert(spec.key.clone(), source_property(spec));
    append_order_key(source, &spec.key)?;
    set_required(source, &spec.key, spec.required)?;

    if let Some(property) = reverse_property(spec) {
        let target = crate::managed::api::object_named_mut(&mut updated, &spec.target_object)
            .map_err(|error| error.to_string())?;
        properties_mut(target)?.insert(spec.reverse_key.clone(), property);
        append_order_key(target, &spec.reverse_key)?;
    }
    Ok(updated)
}

fn remove_relationship_property(
    doc: &mut Value,
    object_name: &str,
    key: &str,
    remove_required: bool,
) -> Result<(), String> {
    let object = crate::managed::api::object_named_mut(doc, object_name)
        .map_err(|error| error.to_string())?;
    if let Some(properties) = object
        .pointer_mut("/schema/properties")
        .and_then(Value::as_object_mut)
    {
        properties.remove(key);
    }
    remove_order_key(object, key)?;
    if remove_required {
        set_required(object, key, false)?;
    }
    Ok(())
}

fn ensure_doc_property_available(doc: &Value, object_name: &str, key: &str) -> Result<(), String> {
    let object =
        crate::managed::api::object_named(doc, object_name).map_err(|error| error.to_string())?;
    if crate::managed::state::properties(object)
        .is_some_and(|properties| properties.contains_key(key))
    {
        return Err(format!("field '{key}' already exists"));
    }
    Ok(())
}

/// Parses a managed schema relationship property into its editable data.
pub fn parse_relationship(property: &Value) -> Option<ParsedRelationship> {
    if !crate::managed::state::is_relationship_property(property) {
        return None;
    }
    let forward = if property.get("type").and_then(Value::as_str) == Some("array") {
        Cardinality::Many
    } else {
        Cardinality::One
    };
    let node = match forward {
        Cardinality::One => property,
        Cardinality::Many => property.get("items")?,
    };
    let target = node
        .pointer("/resourceCollection/0/path")
        .and_then(Value::as_str)?
        .strip_prefix("managed/")?
        .to_string();
    let reverse_key = (node.get("reverseRelationship").and_then(Value::as_bool) == Some(true))
        .then(|| {
            node.get("reversePropertyName")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .flatten();
    let ref_property_names = node
        .pointer("/properties/_refProperties/properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .filter(|key| key.as_str() != "_id")
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Some(ParsedRelationship {
        forward,
        target,
        reverse_key,
        searchable: property
            .get("searchable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        viewable: property
            .get("viewable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        user_editable: property
            .get("userEditable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        validate: node
            .get("validate")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ref_property_names,
    })
}

/// Extracts custom relationship metadata definitions while preserving their labels and types.
pub fn parse_ref_properties(property: &Value) -> Vec<RefProperty> {
    let node = if property.get("type").and_then(Value::as_str) == Some("array") {
        property.get("items")
    } else {
        Some(property)
    };
    node.and_then(|node| node.pointer("/properties/_refProperties/properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .filter_map(|(name, definition)| {
                    if name == "_id" {
                        return None;
                    }
                    let kind = match definition.get("type").and_then(Value::as_str) {
                        Some("number") | Some("integer") => {
                            crate::managed::state::RefPropType::Number
                        }
                        Some("boolean") => crate::managed::state::RefPropType::Boolean,
                        _ => crate::managed::state::RefPropType::String,
                    };
                    Some(RefProperty {
                        name: name.clone(),
                        label: definition
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or(name)
                            .to_string(),
                        kind,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn start_record_count(app: &mut App, draft: RenameObjectState) {
    let tenant = draft.tenant_name.clone();
    let old_name = draft.old_name.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::managed::api::count_records(&tenant, &old_name)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::RenameRecordCount {
            draft,
            result,
        }));
    });
}

/// Counts records in the background before confirming a managed-object delete.
pub fn start_object_record_count(app: &mut App, draft: DeleteObjectState) {
    let tenant = draft.tenant_name.clone();
    let object_name = draft.object_name.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        // Keep the RecordCount as-is: collapsing AtLeast(n) to n would report
        // "100 records" for an object holding thousands, in a confirm modal
        // whose whole job is telling the user how much is at stake.
        let result = crate::managed::api::count_records(&tenant, &object_name)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::DeleteObjectRecordCount {
            draft,
            result,
        }));
    });
}

pub fn execute_rename_object(app: &mut App, request: RenameObjectRequest, confirmed_prod: bool) {
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!("Revert managed object rename {}", request.new_name),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: rename_object_in_doc(&request.previous_doc, &request.old_name, &request.new_name)
                .map_or(Value::Null, |(doc, _)| doc),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Rename cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    let tenant = request.tenant_name.clone();
    app.managed
        .in_flight_writes
        .insert((tenant.clone(), request.old_name.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = rename_object_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::RenameResult {
            tenant,
            old_name: request.old_name,
            new_name: request.new_name,
            undo_id,
            result,
        }));
    });
}

/// Deletes a custom managed object through a guarded whole-document replace.
pub fn execute_delete_object(app: &mut App, request: DeleteObjectRequest, confirmed_prod: bool) {
    let inbound_count = inbound_relationships(&request.previous_doc, &request.object_name).len();
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!("Restore managed object {}", request.object_name),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: delete_object_in_doc(&request.previous_doc, &request.object_name)
                .map_or(Value::Null, |(doc, _)| doc),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Delete cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    let tenant = request.tenant_name.clone();
    app.managed
        .in_flight_writes
        .insert((tenant.clone(), request.object_name.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = delete_object_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::DeleteObjectResult {
            tenant,
            object_name: request.object_name,
            inbound_count,
            undo_id,
            result,
        }));
    });
}

pub fn execute_create_object(app: &mut App, request: CreateObjectRequest, confirmed_prod: bool) {
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!("Remove managed object {}", request.name),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: create_object_in_doc(
                &request.previous_doc,
                &request.name,
                &request.title,
                &request.description,
            )
            .unwrap_or(Value::Null),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Create cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tenant = request.tenant_name.clone();
    let name = request.name.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = create_object_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::CreateResult {
            tenant,
            name,
            undo_id,
            result,
        }));
    });
}

/// Validates and submits the active relationship form as a whole-document write.
pub fn commit_relationship(app: &mut App) {
    let Some(form) = app.managed.relationship_form.as_mut() else {
        return;
    };
    let Some(target_object) = form.target_name.clone() else {
        form.error = Some("Choose a target object".into());
        return;
    };
    if form.reverse != ReverseCardinality::None && form.reverse_key.trimmed().is_empty() {
        form.error =
            Some("Reverse property key is required when a reverse relationship is selected".into());
        return;
    }
    let spec = RelationshipSpec {
        source_object: form.source_object.clone(),
        key: form.key.trimmed().to_string(),
        title: form.title.trimmed().to_string(),
        description: form.description.trimmed().to_string(),
        target_object,
        forward: form.forward,
        reverse: form.reverse,
        reverse_key: form.reverse_key.trimmed().to_string(),
        searchable: form.searchable,
        viewable: form.viewable,
        user_editable: form.user_editable,
        required: form.required,
        validate: form.validate,
        ref_properties: form.ref_properties.clone(),
    };
    if let Err(error) = apply_relationship_spec(&form.original_doc, &spec, form.previous.as_ref()) {
        form.error = Some(error);
        return;
    }
    let request = RelationshipWriteRequest {
        tenant_name: form.tenant_name.clone(),
        source_object: form.source_object.clone(),
        spec,
        previous: form.previous.clone(),
        previous_doc: form.original_doc.clone(),
    };
    if app
        .active_tenant()
        .is_some_and(|tenant| tenant.theme == TenantTheme::Production)
    {
        app.prod_confirm.pending = Some(PendingProdAction::Managed(ProdAction::WriteRelationship(
            Box::new(request),
        )));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_relationship_write(app, request, false);
    }
}

/// Validates and applies one custom `_refProperties` draft to a relationship form.
pub fn commit_ref_prop(
    form: &mut crate::managed::state::RelationshipFormState,
    draft: &crate::managed::state::RefPropDraft,
) -> Result<(), String> {
    let name = draft.name.trimmed();
    crate::managed::state::validate_property_key(name)?;
    if name == "_id" {
        return Err("_id is reserved".into());
    }
    if form
        .ref_properties
        .iter()
        .enumerate()
        .any(|(index, property)| property.name == name && Some(index) != draft.editing_index)
    {
        return Err(format!(
            "A custom relationship property named {name} already exists"
        ));
    }

    let property = RefProperty {
        name: name.to_string(),
        label: if draft.label.trimmed().is_empty() {
            name.to_string()
        } else {
            draft.label.trimmed().to_string()
        },
        kind: draft.kind,
    };
    if let Some(index) = draft.editing_index {
        let Some(existing) = form.ref_properties.get_mut(index) else {
            return Err("Custom relationship property no longer exists".into());
        };
        *existing = property;
    } else {
        form.ref_properties.push(property);
        form.ref_selected = form.ref_properties.len().saturating_sub(1);
    }
    Ok(())
}

pub fn execute_relationship_write(
    app: &mut App,
    request: RelationshipWriteRequest,
    confirmed_prod: bool,
) {
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!(
            "Revert relationship {}.{}",
            request.source_object, request.spec.key
        ),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: apply_relationship_spec(
                &request.previous_doc,
                &request.spec,
                request.previous.as_ref(),
            )
            .unwrap_or(Value::Null),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Relationship save cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    app.managed
        .in_flight_writes
        .insert((request.tenant_name.clone(), request.source_object.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tenant = request.tenant_name.clone();
    let source_object = request.source_object.clone();
    let key = request.spec.key.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = relationship_write_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::RelationshipResult {
            tenant,
            source_object,
            key,
            undo_id,
            result,
        }));
    });
}

async fn relationship_write_request(
    request: &RelationshipWriteRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    let updated = apply_relationship_spec(&live, &request.spec, request.previous.as_ref())
        .map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, updated, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    let source =
        crate::managed::api::object_named(&confirmed, &request.source_object).map_err(|_| {
            crate::Error::Config("relationship write returned but read-back did not match".into())
        })?;
    let forward_ok = crate::managed::state::properties(source)
        .is_some_and(|properties| properties.contains_key(&request.spec.key));
    let reverse_ok = request.spec.reverse == ReverseCardinality::None
        || crate::managed::api::object_named(&confirmed, &request.spec.target_object)
            .ok()
            .and_then(crate::managed::state::properties)
            .is_some_and(|properties| properties.contains_key(&request.spec.reverse_key));
    if !forward_ok || !reverse_ok {
        return Err(crate::Error::Config(
            "relationship write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

pub fn apply_relationship_result(
    app: &mut App,
    tenant: String,
    source_object: String,
    key: String,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    match result {
        Ok(doc) => {
            app.managed
                .data
                .insert(tenant.clone(), LoadState::Loaded(doc));
            app.managed
                .in_flight_writes
                .remove(&(tenant.clone(), source_object.clone()));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                if let Some(index) = app
                    .managed
                    .matches(Some(&tenant))
                    .iter()
                    .position(|item| item.name == source_object)
                {
                    app.managed.selected = index;
                }
            }
            app.push_toast(
                ToastKind::Success,
                format!("Saved relationship {source_object}.{key}. Press ^Z to undo."),
            );
        }
        Err(error) => {
            app.managed
                .in_flight_writes
                .remove(&(tenant, source_object.clone()));
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Relationship save failed: {source_object}.{key}: {error}"),
            );
        }
    }
}

async fn create_object_request(
    request: &CreateObjectRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&live, &request.name).is_ok() {
        return Err(crate::Error::Config(format!(
            "managed object '{}' already exists (created since you opened the form)",
            request.name
        )));
    }
    let new_doc = create_object_in_doc(&live, &request.name, &request.title, &request.description)
        .map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, new_doc, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&confirmed, &request.name).is_err() {
        return Err(crate::Error::Config(
            "managed object create write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

pub fn apply_create_result(
    app: &mut App,
    tenant: String,
    name: String,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    match result {
        Ok(doc) => {
            app.managed
                .data
                .insert(tenant.clone(), LoadState::Loaded(doc));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                if let Some(index) = app
                    .managed
                    .matches(Some(&tenant))
                    .iter()
                    .position(|item| item.name == name)
                {
                    app.managed.selected = index;
                    app.managed.property_selected = 0;
                }
            }
            app.push_toast(
                ToastKind::Success,
                format!("Created managed object {name}. Press ^Z to undo."),
            );
        }
        Err(error) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Managed create failed: {name}: {error}"),
            );
        }
    }
}

async fn rename_object_request(
    request: &RenameObjectRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    let live_old = crate::managed::api::object_named(&live, &request.old_name)?;
    let snapshot_old = crate::managed::api::object_named(&request.previous_doc, &request.old_name)?;
    if !crate::managed::api::object_content_equal(live_old, snapshot_old) {
        return Err(crate::Error::Config(
            "managed object changed since you opened it; refresh and retry".into(),
        ));
    }
    let (new_doc, _) = rename_object_in_doc(&live, &request.old_name, &request.new_name)
        .map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, new_doc, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&confirmed, &request.new_name).is_err()
        || crate::managed::api::object_named(&confirmed, &request.old_name).is_ok()
    {
        return Err(crate::Error::Config(
            "managed object rename write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

async fn delete_object_request(
    request: &DeleteObjectRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    let live_object = crate::managed::api::object_named(&live, &request.object_name)?;
    let snapshot_object =
        crate::managed::api::object_named(&request.previous_doc, &request.object_name)?;
    if !crate::managed::api::object_content_equal(live_object, snapshot_object) {
        return Err(crate::Error::Config(
            "managed object changed since you opened it; refresh and retry".into(),
        ));
    }
    let (new_doc, _) =
        delete_object_in_doc(&live, &request.object_name).map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, new_doc, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&confirmed, &request.object_name).is_ok() {
        return Err(crate::Error::Config(
            "managed object delete write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

pub fn apply_rename_result(
    app: &mut App,
    tenant: String,
    old_name: String,
    new_name: String,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    app.managed
        .in_flight_writes
        .remove(&(tenant.clone(), old_name.clone()));
    match result {
        Ok(doc) => {
            app.managed.data.insert(tenant, LoadState::Loaded(doc));
            app.push_toast(
                ToastKind::Success,
                format!("Renamed managed object {old_name} to {new_name}. Press ^Z to undo."),
            );
        }
        Err(error) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Managed rename failed: {old_name}: {error}"),
            );
        }
    }
}

pub fn apply_delete_object_result(
    app: &mut App,
    tenant: String,
    object_name: String,
    inbound_count: usize,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    app.managed
        .in_flight_writes
        .remove(&(tenant.clone(), object_name.clone()));
    match result {
        Ok(doc) => {
            app.managed.data.insert(tenant, LoadState::Loaded(doc));
            let suffix = if inbound_count == 0 {
                String::new()
            } else {
                format!(" Also removed {inbound_count} inbound relationship(s).")
            };
            app.push_toast(
                ToastKind::Success,
                format!("Deleted managed object {object_name}. Press ^Z to undo.{suffix}"),
            );
        }
        Err(error) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Managed delete failed: {object_name}: {error}"),
            );
        }
    }
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

pub fn build_edit_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let edit = app.managed.editing.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(edit.tenant_name.clone(), edit.object_name.clone()))
    {
        edit.error = Some("Write already in progress for this object".into());
        return None;
    }

    let applied = match apply_field_edit(edit) {
        Ok(applied) => applied,
        Err(message) => {
            edit.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&applied.object, &edit.original_object) {
        edit.error = Some("No changes to save".into());
        return None;
    }

    let success_message = if applied.renamed {
        format!(
            "Renamed managed field {}.{} to {}. Press ^Z to undo.",
            edit.object_name, edit.field_key, applied.field_key
        )
    } else {
        "Saved managed field attributes. Press ^Z to undo.".to_string()
    };

    Some(ObjectReplacePlan {
        tenant_name: edit.tenant_name.clone(),
        object_name: edit.object_name.clone(),
        previous_object: edit.original_object.clone(),
        new_object: applied.object,
        searchable_changed: applied.searchable_changed,
        success_message,
    })
}

pub fn build_add_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let draft = app.managed.add_field.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(draft.tenant_name.clone(), draft.object_name.clone()))
    {
        draft.error = Some("Write already in progress for this object".into());
        return None;
    }

    let applied = match apply_add_field(draft) {
        Ok(applied) => applied,
        Err(message) => {
            draft.error = Some(message);
            return None;
        }
    };

    Some(ObjectReplacePlan {
        tenant_name: draft.tenant_name.clone(),
        object_name: draft.object_name.clone(),
        previous_object: draft.original_object.clone(),
        new_object: applied.object,
        searchable_changed: applied.searchable_changed,
        success_message: format!(
            "Added managed field {}.{}. Press ^Z to undo.",
            draft.object_name, applied.field_key
        ),
    })
}

pub fn build_add_hook_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let draft = app.managed.add_hook.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(draft.tenant_name.clone(), draft.object_name.clone()))
    {
        draft.error = Some("Write already in progress for this object".into());
        return None;
    }

    let event = match draft.events.get(draft.selected).copied() {
        Some(event) => event,
        None => {
            draft.error = Some("No hook event available to add".into());
            return None;
        }
    };
    let new_object = match apply_add_hook(&draft.original_object, &draft.object_name, event) {
        Ok(object) => object,
        Err(message) => {
            draft.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&new_object, &draft.original_object) {
        draft.error = Some("Hook already exists on this object".into());
        return None;
    }

    Some(ObjectReplacePlan {
        tenant_name: draft.tenant_name.clone(),
        object_name: draft.object_name.clone(),
        previous_object: draft.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message: format!(
            "Hook registered — edit it with `aic script pull managed/{}.{event}`",
            draft.object_name
        ),
    })
}

pub fn build_delete_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let pending = app.managed.pending_delete.as_ref()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(pending.tenant_name.clone(), pending.object_name.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", pending.object_name),
        );
        return None;
    }

    let new_object = match apply_delete_field(pending) {
        Ok(object) => object,
        Err(message) => {
            app.push_toast(ToastKind::Error, message);
            return None;
        }
    };
    let mut success_message = format!(
        "Deleted field {}.{}. Press ^Z to undo.",
        pending.object_name, pending.field_key
    );
    if pending.is_relationship {
        success_message = format!(
            "Deleted relationship {}.{}. Reverse links on the target were not changed. Press ^Z to undo.",
            pending.object_name, pending.field_key
        );
    }

    Some(ObjectReplacePlan {
        tenant_name: pending.tenant_name.clone(),
        object_name: pending.object_name.clone(),
        previous_object: pending.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message,
    })
}

/// Builds the undoable whole-object replacement plan for a property-key rename.
pub fn build_rename_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let rename = app.managed.renaming.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(rename.tenant_name.clone(), rename.object_name.clone()))
    {
        rename.error = Some("Write already in progress for this object".into());
        return None;
    }

    let new_key = rename.key.value.clone();
    let new_object = match apply_rename_field(rename, &new_key) {
        Ok(object) => object,
        Err(message) => {
            rename.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&new_object, &rename.original_object) {
        rename.error = Some("No changes to save".into());
        return None;
    }

    Some(ObjectReplacePlan {
        tenant_name: rename.tenant_name.clone(),
        object_name: rename.object_name.clone(),
        previous_object: rename.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message: format!(
            "Renamed managed field {}.{} to {}. Press ^Z to undo.",
            rename.object_name, rename.old_key, new_key
        ),
    })
}

pub fn execute_update_plan(app: &mut App, plan: ObjectReplacePlan, confirmed_prod: bool) {
    let ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    } = plan;

    let undo_id = match record_replace_undo(
        app,
        &tenant_name,
        &object_name,
        &previous_object,
        &new_object,
    ) {
        Ok(undo_id) => undo_id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Save cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };

    set_cached_object(app, &tenant_name, &object_name, new_object.clone());
    app.managed
        .in_flight_writes
        .insert((tenant_name.clone(), object_name.clone()));
    app.managed
        .failed_writes
        .remove(&(tenant_name.clone(), object_name.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;

    let request = ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    };
    let event_tenant = request.tenant_name.clone();
    let event_object = request.object_name.clone();
    let event_previous_object = request.previous_object.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = replace_object_request(request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::UpdateResult {
            tenant: event_tenant,
            object_name: event_object,
            undo_id,
            previous_object: event_previous_object,
            result,
        }));
    });
}

fn record_replace_undo(
    app: &mut App,
    tenant_name: &str,
    object_name: &str,
    previous_object: &Value,
    new_object: &Value,
) -> crate::Result<UndoId> {
    app.undo.record(UndoEntry::pending(
        tenant_name.to_string(),
        "managed",
        format!("Revert managed object {object_name}"),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedObjectReplace {
            tenant: tenant_name.to_string(),
            object_name: object_name.to_string(),
            body: previous_object.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: new_object.clone(),
        },
    ))
}

async fn replace_object_request(
    plan: ObjectReplacePlan,
    confirmed_prod: bool,
) -> crate::Result<UpdateOutcome> {
    let ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    } = plan;

    let confirmed = replace_object_with_snapshot(
        &tenant_name,
        &object_name,
        &previous_object,
        &new_object,
        confirmed_prod,
    )
    .await?;
    Ok(UpdateOutcome {
        object: confirmed,
        searchable_changed,
        success_message,
    })
}

async fn replace_object_with_snapshot(
    tenant_name: &str,
    object_name: &str,
    expected_current: &Value,
    replacement: &Value,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let (mut doc, current_object) =
        crate::managed::api::get_managed_with_object(tenant_name, object_name).await?;
    if !crate::managed::api::object_content_equal(&current_object, expected_current) {
        return Err(crate::Error::Config(format!(
            "remote managed object '{object_name}' changed since you opened it; refresh and retry"
        )));
    }

    crate::managed::api::replace_object(&mut doc, object_name, replacement.clone())?;
    crate::managed::api::replace_managed(tenant_name, doc, confirmed_prod).await?;

    // Managed-config read-back is strongly consistent for schema storage
    // (verified 2026-06-14), so one fresh GET is enough. Hook runtime
    // activation has a separate lag and is handled by scripts/managed_hooks.
    let (_, confirmed_object) =
        crate::managed::api::get_managed_with_object(tenant_name, object_name).await?;
    if !crate::managed::api::object_content_equal(&confirmed_object, replacement) {
        return Err(crate::Error::Config(format!(
            "managed object '{object_name}' write returned but read-back did not match"
        )));
    }
    Ok(confirmed_object)
}

pub fn apply_update_result(
    app: &mut App,
    tenant: String,
    object_name: String,
    undo_id: UndoId,
    previous_object: Value,
    result: Result<UpdateOutcome, String>,
) {
    app.managed
        .in_flight_writes
        .remove(&(tenant.clone(), object_name.clone()));
    match result {
        Ok(UpdateOutcome {
            object,
            searchable_changed,
            success_message,
        }) => {
            set_cached_object(app, &tenant, &object_name, object);
            app.managed
                .failed_writes
                .remove(&(tenant.clone(), object_name.clone()));
            let mut message = success_message;
            if searchable_changed {
                // This flag only changes the managed schema. Full directory
                // indexing also requires repo.ds mapping/index work, which is
                // deliberately outside this managed-object editor slice.
                message.push_str(" Searchable only updates schema; repo.ds indexing is separate.");
            }
            app.push_toast(ToastKind::Success, message);
        }
        Err(error) => {
            if let Err(mark_error) = revert_failed_update(
                &mut app.managed,
                app.undo.as_mut(),
                &tenant,
                &object_name,
                undo_id,
                previous_object,
            ) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Failed to expire undo for failed managed save: {mark_error}"),
                );
            }
            app.push_toast(
                ToastKind::Error,
                format!("Managed save failed: {object_name}: {error}"),
            );
        }
    }
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let Some(undo_id) = latest_pending_managed_undo(app, &tenant_name) else {
        app.push_toast(ToastKind::Info, "No managed-object undo for this tenant");
        return;
    };

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Managed(ProdAction::Undo(undo_id)));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_undo(app, undo_id, false);
    }
}

fn latest_pending_managed_undo(app: &App, tenant: &str) -> Option<UndoId> {
    app.undo
        .list(100)
        .into_iter()
        .filter(|summary| {
            summary.tenant == tenant
                && summary.status == EntryStatus::Pending
                && matches!(
                    summary.capability,
                    Capability::Undoable | Capability::BestEffort
                )
        })
        .find_map(|summary| {
            let entry = app.undo.load(summary.id).ok()?;
            matches!(
                entry.op,
                Some(UndoOp::ManagedObjectReplace { .. } | UndoOp::ManagedConfigReplace { .. })
            )
            .then_some(summary.id)
        })
}

pub fn execute_undo(app: &mut App, undo_id: UndoId, confirmed_prod: bool) {
    let entry = match app.undo.load(undo_id) {
        Ok(entry) => entry,
        Err(error) => {
            app.push_toast(ToastKind::Error, format!("Undo failed: {error}"));
            return;
        }
    };
    if entry.status != EntryStatus::Pending {
        app.push_toast(ToastKind::Info, "Undo entry is no longer pending");
        return;
    }
    if entry.op.is_none() || entry.capability == Capability::Irreversible {
        app.push_toast(ToastKind::Warning, "This change cannot be undone");
        return;
    }
    if !matches!(
        entry.op,
        Some(UndoOp::ManagedObjectReplace { .. } | UndoOp::ManagedConfigReplace { .. })
    ) {
        app.push_toast(ToastKind::Info, "Undo entry is not a managed-object change");
        return;
    }

    let event_tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::Managed(Event::UndoResult {
            undo_id,
            tenant: event_tenant,
            result,
        }));
    });
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> Result<UndoOutcome, UndoFailure> {
    let op = entry
        .op
        .clone()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no operation".into()))?;
    let expected_current = match entry.conflict_check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => body,
        _ => {
            return Err(UndoFailure::Failed(
                "managed-object undo has no content snapshot".into(),
            ));
        }
    };

    let outcome = match op {
        UndoOp::ManagedObjectReplace {
            tenant,
            object_name,
            body,
        } => {
            let object = replace_object_with_snapshot(
                &tenant,
                &object_name,
                &expected_current,
                &body,
                confirmed_prod,
            )
            .await
            .map_err(undo_failure)?;
            UndoOutcome {
                description: entry.description,
                object: Some((object_name, object)),
                doc: None,
            }
        }
        UndoOp::ManagedConfigReplace { tenant, body } => {
            let live = crate::managed::api::get_managed(&tenant)
                .await
                .map_err(undo_failure)?;
            if live != expected_current {
                return Err(UndoFailure::Conflict(
                    "managed config changed since the rename; refresh and retry".into(),
                ));
            }
            crate::managed::api::replace_managed(&tenant, body.clone(), confirmed_prod)
                .await
                .map_err(undo_failure)?;
            let confirmed = crate::managed::api::get_managed(&tenant)
                .await
                .map_err(undo_failure)?;
            if confirmed != body {
                return Err(UndoFailure::Failed(
                    "managed config undo write returned but read-back did not match".into(),
                ));
            }
            UndoOutcome {
                description: entry.description,
                object: None,
                doc: Some(confirmed),
            }
        }
        _ => {
            return Err(UndoFailure::Failed(
                "undo entry is not a managed-object operation".into(),
            ));
        }
    };
    Ok(outcome)
}

fn undo_failure(error: crate::Error) -> UndoFailure {
    match error {
        crate::Error::Config(message) if message.contains("changed since") => {
            UndoFailure::Conflict(message)
        }
        other => UndoFailure::Failed(other.to_string()),
    }
}

pub fn apply_undo_result(
    app: &mut App,
    undo_id: UndoId,
    tenant: String,
    result: Result<UndoOutcome, UndoFailure>,
) {
    match result {
        Ok(UndoOutcome {
            description,
            object,
            doc,
        }) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {error}"),
                );
            }
            if let Some((object_name, object)) = object {
                set_cached_object(app, &tenant, &object_name, object);
                app.managed
                    .failed_writes
                    .remove(&(tenant.clone(), object_name));
            }
            if let Some(doc) = doc {
                app.managed
                    .data
                    .insert(tenant.clone(), LoadState::Loaded(doc));
            }
            app.push_toast(ToastKind::Success, format!("Undone: {description}"));
        }
        Err(UndoFailure::Conflict(message)) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedConflict) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo conflict log update failed: {error}"),
                );
            }
            app.push_toast(ToastKind::Warning, format!("Undo conflict: {message}"));
        }
        Err(UndoFailure::Failed(message)) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedFailure) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo failure log update failed: {error}"),
                );
            }
            app.push_toast(ToastKind::Error, format!("Undo failed: {message}"));
        }
    }
}

pub(crate) fn set_cached_object(app: &mut App, tenant: &str, object_name: &str, object: Value) {
    if let Err(error) = set_cached_object_in_state(&mut app.managed, tenant, object_name, object) {
        tracing::warn!("failed to update managed cache for {object_name}: {error}");
    }
}

fn set_cached_object_in_state(
    managed: &mut State,
    tenant: &str,
    object_name: &str,
    object: Value,
) -> crate::Result<()> {
    if let Some(LoadState::Loaded(doc)) = managed.data.get_mut(tenant) {
        crate::managed::api::replace_object(doc, object_name, object)?;
    }
    Ok(())
}

fn revert_failed_update(
    managed: &mut State,
    undo: &mut dyn crate::undo::UndoLog,
    tenant: &str,
    object_name: &str,
    undo_id: UndoId,
    previous_object: Value,
) -> crate::Result<()> {
    if let Err(error) = set_cached_object_in_state(managed, tenant, object_name, previous_object) {
        tracing::warn!("failed to restore managed cache for {object_name}: {error}");
    }
    managed
        .failed_writes
        .insert((tenant.to_string(), object_name.to_string()));
    undo.mark_applied(undo_id, EntryStatus::Expired)
}

#[derive(Debug)]
struct FieldEditApplied {
    object: Value,
    field_key: String,
    searchable_changed: bool,
    renamed: bool,
}

#[derive(Debug)]
struct AddFieldApplied {
    object: Value,
    field_key: String,
    searchable_changed: bool,
}

fn apply_field_edit(edit: &FieldEditState) -> Result<FieldEditApplied, String> {
    let mut object = edit.original_object.clone();
    let mut property = edit.original_property.clone();
    let property_map = property.as_object_mut().ok_or_else(|| {
        format!(
            "field '{}' is not an object-valued property",
            edit.field_key
        )
    })?;

    if edit.caps.can_edit_attr(FieldAttr::Title) {
        set_optional_string(property_map, "title", &edit.title.value);
    }
    if edit.caps.can_edit_attr(FieldAttr::Description) {
        set_optional_string(property_map, "description", &edit.description.value);
    }
    if edit.caps.can_edit_attr(FieldAttr::Searchable) {
        property_map.insert("searchable".into(), Value::Bool(edit.searchable));
    }
    if edit.caps.can_edit_attr(FieldAttr::Viewable) {
        property_map.insert("viewable".into(), Value::Bool(edit.viewable));
    }
    if edit.caps.can_edit_attr(FieldAttr::UserEditable) {
        property_map.insert("userEditable".into(), Value::Bool(edit.user_editable));
    }

    let field_key = if edit.caps.rename_key {
        crate::managed::state::normalize_new_property_key(&edit.original_object, &edit.key.value)?
    } else {
        edit.field_key.clone()
    };
    if !edit.caps.rename_key && field_key != edit.field_key {
        return Err("This field key cannot be renamed".into());
    }
    if edit.caps.rename_key && crate::managed::state::is_relationship_property(&property) {
        return Err(
            "Relationship keys cannot be renamed; delete and recreate the relationship".into(),
        );
    }

    upsert_property(&mut object, &edit.field_key, &field_key, property)?;

    if edit.caps.can_edit_attr(FieldAttr::Required) {
        set_required(&mut object, &field_key, edit.required)?;
    }
    let searchable_changed = edit
        .original_property
        .get("searchable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        != edit.searchable;

    Ok(FieldEditApplied {
        object,
        renamed: field_key != edit.field_key,
        field_key,
        searchable_changed,
    })
}

fn apply_add_field(draft: &AddFieldState) -> Result<AddFieldApplied, String> {
    let mut object = draft.original_object.clone();
    let field_key = crate::managed::state::normalize_new_property_key(
        &draft.original_object,
        &draft.key.value,
    )?;
    ensure_property_available(&object, &field_key, None)?;

    let mut property = match draft.field_type {
        ScalarFieldType::String => json!({"type": "string"}),
        ScalarFieldType::Boolean => json!({"type": "boolean"}),
        ScalarFieldType::Number => json!({"type": "number"}),
        ScalarFieldType::StringArray => json!({"type": "array", "items": {"type": "string"}}),
    };
    let property_map = property
        .as_object_mut()
        .ok_or_else(|| "new scalar property is not object-valued".to_string())?;
    set_optional_string(property_map, "title", draft.title.trimmed());
    set_optional_string(property_map, "description", draft.description.trimmed());
    property_map.insert("searchable".into(), Value::Bool(draft.searchable));
    property_map.insert("viewable".into(), Value::Bool(draft.viewable));
    property_map.insert("userEditable".into(), Value::Bool(draft.user_editable));

    insert_new_property(&mut object, &field_key, property, draft.required)?;
    Ok(AddFieldApplied {
        object,
        field_key,
        searchable_changed: draft.searchable,
    })
}

fn apply_add_hook(object_def: &Value, object_name: &str, event: &str) -> Result<Value, String> {
    let mut object = object_def.clone();
    let object_map = object
        .as_object_mut()
        .ok_or_else(|| format!("managed object '{object_name}' is not object-valued"))?;
    if object_map.contains_key(event) {
        return Ok(object);
    }
    object_map.insert(
        event.to_string(),
        json!({
            "type": "text/javascript",
            "source": format!("// {event} for {object_name}\n"),
        }),
    );
    Ok(object)
}

fn apply_delete_field(pending: &DeleteFieldState) -> Result<Value, String> {
    let mut object = pending.original_object.clone();
    let property = properties_mut(&mut object)?
        .remove(&pending.field_key)
        .ok_or_else(|| format!("field '{}' no longer exists", pending.field_key))?;
    let caps = crate::managed::state::field_capability_for_property(
        &pending.original_object,
        &pending.field_key,
        &property,
    );
    if !caps.delete {
        return Err("Standard fields cannot be deleted".into());
    }
    remove_order_key(&mut object, &pending.field_key)?;
    set_required(&mut object, &pending.field_key, false)?;
    Ok(object)
}

fn apply_rename_field(rename: &RenameFieldState, new_key: &str) -> Result<Value, String> {
    if new_key.trim() != new_key {
        return Err("Property key cannot have leading or trailing whitespace".into());
    }
    crate::managed::state::validate_property_key(new_key)?;

    let mut object = rename.original_object.clone();
    ensure_property_available(&object, new_key, Some(&rename.old_key))?;
    if new_key == rename.old_key {
        return Ok(object);
    }

    let properties = properties_mut(&mut object)?;
    let previous = std::mem::take(properties);
    let mut renamed = false;
    for (key, property) in previous {
        if key == rename.old_key {
            properties.insert(new_key.to_string(), property);
            renamed = true;
        } else {
            properties.insert(key, property);
        }
    }
    if !renamed {
        return Err(format!("field '{}' no longer exists", rename.old_key));
    }

    replace_key_in_array(&mut object, "/schema/order", &rename.old_key, new_key);
    replace_key_in_array(&mut object, "/schema/required", &rename.old_key, new_key);
    Ok(object)
}

fn replace_key_in_array(object: &mut Value, pointer: &str, old_key: &str, new_key: &str) {
    let Some(values) = object.pointer_mut(pointer).and_then(Value::as_array_mut) else {
        return;
    };
    for value in values {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
        }
    }
}

fn set_optional_string(map: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    if value.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.into(), Value::String(value.to_string()));
    }
}

fn insert_new_property(
    object: &mut Value,
    field_key: &str,
    property: Value,
    required: bool,
) -> Result<(), String> {
    ensure_property_available(object, field_key, None)?;
    properties_mut(object)?.insert(field_key.to_string(), property);
    append_order_key(object, field_key)?;
    set_required(object, field_key, required)?;
    Ok(())
}

fn upsert_property(
    object: &mut Value,
    old_key: &str,
    new_key: &str,
    property: Value,
) -> Result<(), String> {
    ensure_property_available(object, new_key, Some(old_key))?;
    let properties = properties_mut(object)?;
    if old_key == new_key {
        properties.insert(old_key.to_string(), property);
        return Ok(());
    }
    properties.remove(old_key);
    properties.insert(new_key.to_string(), property);
    rename_order_key(object, old_key, new_key)?;
    rename_required_key(object, old_key, new_key)?;
    Ok(())
}

fn ensure_property_available(
    object: &Value,
    field_key: &str,
    current_key: Option<&str>,
) -> Result<(), String> {
    let Some(properties) = crate::managed::state::properties(object) else {
        return Ok(());
    };
    if properties.contains_key(field_key) && current_key != Some(field_key) {
        return Err(format!("field '{field_key}' already exists"));
    }
    Ok(())
}

fn properties_mut(object: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    let schema = schema_mut(object)?;
    schema
        .entry("properties".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "managed object has no schema.properties map".to_string())
}

fn schema_mut(object: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    let map = object
        .as_object_mut()
        .ok_or_else(|| "managed object is not an object".to_string())?;
    map.entry("schema".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "managed object has no schema object".to_string())
}

fn append_order_key(object: &mut Value, field_key: &str) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let order_value = schema
        .entry("order".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let order = order_value
        .as_array_mut()
        .ok_or_else(|| "schema.order is not an array".to_string())?;
    if !order.iter().any(|value| value.as_str() == Some(field_key)) {
        order.push(Value::String(field_key.to_string()));
    }
    Ok(())
}

fn rename_order_key(object: &mut Value, old_key: &str, new_key: &str) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let order_value = schema
        .entry("order".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let order = order_value
        .as_array_mut()
        .ok_or_else(|| "schema.order is not an array".to_string())?;
    let mut replaced = false;
    for value in order.iter_mut() {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
            replaced = true;
        }
    }
    order.retain(|value| value.as_str() != Some(old_key));
    if !replaced && !order.iter().any(|value| value.as_str() == Some(new_key)) {
        order.push(Value::String(new_key.to_string()));
    }
    dedupe_string_array(order);
    Ok(())
}

fn remove_order_key(object: &mut Value, field_key: &str) -> Result<(), String> {
    let Some(order) = object
        .pointer_mut("/schema/order")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    order.retain(|value| value.as_str() != Some(field_key));
    Ok(())
}

fn rename_required_key(object: &mut Value, old_key: &str, new_key: &str) -> Result<(), String> {
    let Some(required) = object
        .pointer_mut("/schema/required")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for value in required.iter_mut() {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
        }
    }
    required.retain(|value| value.as_str() != Some(old_key));
    dedupe_string_array(required);
    Ok(())
}

fn dedupe_string_array(values: &mut Vec<Value>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| match value.as_str() {
        Some(text) => seen.insert(text.to_string()),
        None => true,
    });
}

fn set_required(object: &mut Value, field_key: &str, required: bool) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let required_value = schema
        .entry("required".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let required_array = required_value
        .as_array_mut()
        .ok_or_else(|| "schema.required is not an array".to_string())?;

    if required {
        if !required_array
            .iter()
            .any(|value| value.as_str() == Some(field_key))
        {
            required_array.push(Value::String(field_key.to_string()));
        }
    } else {
        required_array.retain(|value| value.as_str() != Some(field_key));
    }
    Ok(())
}

pub fn commit_edit(app: &mut App) {
    let Some(plan) = build_edit_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_add_field(app: &mut App) {
    let Some(plan) = build_add_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_add_hook(app: &mut App) {
    let Some(plan) = build_add_hook_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_delete_field(app: &mut App) {
    let Some(plan) = build_delete_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

/// Submits the active property-key rename through the normal update pipeline.
pub fn commit_rename_field(app: &mut App) {
    let Some(plan) = build_rename_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

fn submit_plan(app: &mut App, plan: ObjectReplacePlan) {
    let is_prod = app
        .active_tenant()
        .is_some_and(|tenant| tenant.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::Managed(ProdAction::Update(Box::new(
            plan,
        ))));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_update_plan(app, plan, false);
}

pub fn cancel_active_draft(app: &mut App) {
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
}

pub fn advance_focus(edit: &mut FieldEditState, forward: bool) {
    edit.focused = if forward {
        edit.focused.next(edit.caps)
    } else {
        edit.focused.prev(edit.caps)
    };
}

pub fn advance_add_field_focus(draft: &mut AddFieldState, forward: bool) {
    draft.focused = if forward {
        draft.focused.next()
    } else {
        draft.focused.prev()
    };
}

/// Advances the rename draft's single focusable field.
pub fn advance_rename_field_focus(_draft: &mut RenameFieldState, _forward: bool) {}

pub fn handle_enter_in_edit(app: &mut App) {
    let Some(focused) = app.managed.editing.as_ref().map(|edit| edit.focused) else {
        return;
    };
    match focused {
        EditFieldFocus::Save => commit_edit(app),
        focus if focus.is_bool() => {
            if let Some(edit) = app.managed.editing.as_mut() {
                edit.toggle_focused_bool();
                edit.error = None;
            }
        }
        _ => {
            if let Some(edit) = app.managed.editing.as_mut() {
                advance_focus(edit, true);
            }
        }
    }
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Update(plan) => execute_update_plan(app, *plan, true),
        ProdAction::RenameObject(request) => execute_rename_object(app, *request, true),
        ProdAction::DeleteObject(request) => execute_delete_object(app, *request, true),
        ProdAction::CreateObject(request) => execute_create_object(app, *request, true),
        ProdAction::WriteRelationship(request) => execute_relationship_write(app, *request, true),
        ProdAction::Undo(undo_id) => execute_undo(app, undo_id, true),
    }
}

pub fn resume_mode(app: &App, _action: &ProdAction) -> InputMode {
    crate::managed::screen::resume_mode_after_prod_cancel(app)
        .map(InputMode::Managed)
        .unwrap_or(InputMode::Normal)
}

pub fn describe_prod_action(_action: &ProdAction) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use crate::undo::{MemoryLog, UndoLog};
    use serde_json::json;

    use super::*;

    fn edit_state(required: bool) -> FieldEditState {
        let required_value = if required {
            json!(["custom_code"])
        } else {
            json!([])
        };
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {
                        "title": "Old",
                        "description": "Old desc",
                        "type": "string",
                        "searchable": false,
                        "viewable": true,
                        "userEditable": false
                    }
                },
                "required": required_value,
                "order": ["custom_code"]
            }
        });
        let property = object["schema"]["properties"]["custom_code"].clone();
        FieldEditState::from_property(
            "sandbox".into(),
            "alpha_user".into(),
            "custom_code".into(),
            object,
            property,
            required,
        )
    }

    fn managed_doc(object: Value) -> Value {
        json!({ "objects": [object] })
    }

    fn cached_object<'a>(
        state: &'a crate::managed::state::State,
        tenant: &str,
        object_name: &str,
    ) -> &'a Value {
        let LoadState::Loaded(doc) = state.data.get(tenant).unwrap() else {
            panic!("managed cache is not loaded");
        };
        crate::managed::api::object_named(doc, object_name).unwrap()
    }

    #[test]
    fn field_edit_updates_property_and_required_without_touching_order() {
        let mut edit = edit_state(false);
        edit.title.set("New");
        edit.description.set("New desc");
        edit.required = true;
        edit.searchable = true;
        edit.user_editable = true;

        let object = apply_field_edit(&edit).unwrap().object;
        let property = &object["schema"]["properties"]["custom_code"];
        assert_eq!(property["title"], json!("New"));
        assert_eq!(property["description"], json!("New desc"));
        assert_eq!(property["searchable"], json!(true));
        assert_eq!(property["viewable"], json!(true));
        assert_eq!(property["userEditable"], json!(true));
        assert_eq!(object["schema"]["required"], json!(["custom_code"]));
        assert_eq!(object["schema"]["order"], json!(["custom_code"]));
    }

    #[test]
    fn field_edit_removes_required_when_cleared() {
        let mut edit = edit_state(true);
        edit.required = false;

        let object = apply_field_edit(&edit).unwrap().object;
        assert_eq!(object["schema"]["required"], json!([]));
    }

    #[test]
    fn pure_attribute_edit_leaves_missing_order_entry_unchanged() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {
                        "title": "Old",
                        "type": "string",
                        "searchable": false,
                        "viewable": true,
                        "userEditable": false
                    }
                },
                "required": [],
                "order": ["givenName"]
            }
        });
        let property = object["schema"]["properties"]["custom_code"].clone();
        let mut edit = FieldEditState::from_property(
            "sandbox".into(),
            "alpha_user".into(),
            "custom_code".into(),
            object,
            property,
            false,
        );
        edit.title.set("New");

        let object = apply_field_edit(&edit).unwrap().object;
        assert_eq!(
            object["schema"]["properties"]["custom_code"]["title"],
            json!("New")
        );
        assert_eq!(object["schema"]["order"], json!(["givenName"]));
    }

    #[test]
    fn add_field_auto_prefixes_standard_object_custom_key() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut draft = AddFieldState::new("sandbox".into(), "alpha_user".into(), object);
        draft.key.set("loyaltyId");
        draft.title.set("Loyalty ID");
        draft.required = true;

        let applied = apply_add_field(&draft).unwrap();
        assert_eq!(applied.field_key, "custom_loyaltyId");
        assert!(applied.object["schema"]["properties"]["custom_loyaltyId"].is_object());
        assert!(applied.object["schema"]["properties"]["loyaltyId"].is_null());
        assert_eq!(
            applied.object["schema"]["order"],
            json!(["custom_loyaltyId"])
        );
        assert_eq!(
            applied.object["schema"]["required"],
            json!(["custom_loyaltyId"])
        );
    }

    #[test]
    fn add_field_materializes_missing_schema() {
        let object = json!({"name": "test_empty"});
        let mut draft = AddFieldState::new("sandbox".into(), "test_empty".into(), object);
        draft.key.set("first");

        let applied = apply_add_field(&draft).unwrap();
        assert!(applied.object["schema"].is_object());
        assert!(applied.object["schema"]["properties"]["first"].is_object());
        assert_eq!(applied.object["schema"]["order"], json!(["first"]));
    }

    #[test]
    fn add_field_materializes_missing_properties() {
        let object = json!({"name": "test_empty", "schema": {}});
        let mut draft = AddFieldState::new("sandbox".into(), "test_empty".into(), object);
        draft.key.set("first");

        let applied = apply_add_field(&draft).unwrap();
        assert!(applied.object["schema"]["properties"]["first"].is_object());
        assert_eq!(applied.object["schema"]["order"], json!(["first"]));
    }

    #[test]
    fn add_hook_inserts_top_level_sibling_and_preserves_existing() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []}
        });

        let added = apply_add_hook(&object, "alpha_lock", "onCreate").unwrap();
        assert_eq!(added["onCreate"]["type"], json!("text/javascript"));
        assert_eq!(
            added["onCreate"]["source"],
            json!("// onCreate for alpha_lock\n")
        );
        assert!(added["schema"].is_object());

        let existing = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []},
            "onCreate": {"type": "text/javascript", "source": "old();"}
        });
        let unchanged = apply_add_hook(&existing, "alpha_lock", "onCreate").unwrap();
        assert_eq!(unchanged, existing);
    }

    #[test]
    fn rename_custom_field_keeps_order_and_required_in_sync() {
        let mut edit = edit_state(true);
        edit.key.set("custom_new_code");

        let object = apply_field_edit(&edit).unwrap().object;
        assert!(object["schema"]["properties"]["custom_code"].is_null());
        assert!(object["schema"]["properties"]["custom_new_code"].is_object());
        assert_eq!(object["schema"]["order"], json!(["custom_new_code"]));
        assert_eq!(object["schema"]["required"], json!(["custom_new_code"]));
    }

    #[test]
    fn rename_field_plan_mutation_updates_order_required_and_rejects_collisions() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {
                "properties": {
                    "before": {"type": "string"},
                    "old_key": {"type": "string"},
                    "after": {"type": "string"}
                },
                "order": ["before", "old_key", "after"],
                "required": ["old_key"]
            }
        });
        let mut rename = RenameFieldState::new(
            "sandbox".into(),
            "alpha_lock".into(),
            "old_key".into(),
            object,
        );
        rename.key.set("new_key");

        let renamed = apply_rename_field(&rename, &rename.key.value).unwrap();
        let properties = renamed["schema"]["properties"].as_object().unwrap();
        assert!(properties.contains_key("new_key"));
        assert!(!properties.contains_key("old_key"));
        // Field display order is governed by schema.order, not properties key
        // position, so we only assert order/required are kept in sync.
        assert_eq!(
            renamed["schema"]["order"],
            json!(["before", "new_key", "after"])
        );
        assert_eq!(renamed["schema"]["required"], json!(["new_key"]));

        rename.key.set("after");
        let error = apply_rename_field(&rename, &rename.key.value).unwrap_err();
        assert_eq!(error, "field 'after' already exists");
    }

    #[test]
    fn delete_custom_field_keeps_order_and_required_in_sync() {
        let edit = edit_state(true);
        let pending = DeleteFieldState {
            tenant_name: "sandbox".into(),
            object_name: "alpha_user".into(),
            field_key: "custom_code".into(),
            original_object: edit.original_object,
            is_relationship: false,
        };

        let object = apply_delete_field(&pending).unwrap();
        assert!(object["schema"]["properties"]["custom_code"].is_null());
        assert_eq!(object["schema"]["order"], json!([]));
        assert_eq!(object["schema"]["required"], json!([]));
    }

    #[test]
    fn delete_standard_field_is_blocked_by_capability() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {"givenName": {"type": "string"}},
                "required": ["givenName"],
                "order": ["givenName"]
            }
        });
        let pending = DeleteFieldState {
            tenant_name: "sandbox".into(),
            object_name: "alpha_user".into(),
            field_key: "givenName".into(),
            original_object: object,
            is_relationship: false,
        };

        let error = apply_delete_field(&pending).unwrap_err();
        assert!(error.contains("Standard fields"));
    }

    #[test]
    fn failed_update_reverts_cache_and_expires_pending_undo() {
        let previous = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {"title": "Old", "type": "string"}
                },
                "required": [],
                "order": ["custom_code"]
            }
        });
        let optimistic = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {"title": "Unsaved", "type": "string"}
                },
                "required": [],
                "order": ["custom_code"]
            }
        });
        let mut state = crate::managed::state::State::new();
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(managed_doc(optimistic.clone())),
        );
        state
            .in_flight_writes
            .insert(("sandbox".into(), "alpha_user".into()));

        let mut undo = MemoryLog::new();
        let undo_id = undo
            .record(UndoEntry::pending(
                "sandbox".to_string(),
                "managed",
                "Revert managed object alpha_user",
                Sensitivity::TenantConfig,
                Capability::Undoable,
                Some(UndoOp::ManagedObjectReplace {
                    tenant: "sandbox".to_string(),
                    object_name: "alpha_user".to_string(),
                    body: previous.clone(),
                }),
                ConflictCheck::ContentEqualsAfter { body: optimistic },
            ))
            .unwrap();

        revert_failed_update(
            &mut state,
            &mut undo,
            "sandbox",
            "alpha_user",
            undo_id,
            previous.clone(),
        )
        .unwrap();

        assert!(crate::managed::api::object_content_equal(
            cached_object(&state, "sandbox", "alpha_user"),
            &previous
        ));
        let resnapshot = cached_object(&state, "sandbox", "alpha_user").clone();
        assert!(crate::managed::api::object_content_equal(
            &resnapshot,
            &previous
        ));
        assert!(
            state
                .failed_writes
                .contains(&("sandbox".into(), "alpha_user".into()))
        );
        assert_eq!(undo.load(undo_id).unwrap().status, EntryStatus::Expired);
    }

    #[test]
    fn failed_update_does_not_reinsert_object_missing_from_cache() {
        let previous = json!({
            "name": "alpha_user",
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut state = crate::managed::state::State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(json!({"objects": []})));
        let mut undo = MemoryLog::new();
        let undo_id = undo
            .record(UndoEntry::pending(
                "sandbox".to_string(),
                "managed",
                "Revert managed object alpha_user",
                Sensitivity::TenantConfig,
                Capability::Undoable,
                Some(UndoOp::ManagedObjectReplace {
                    tenant: "sandbox".to_string(),
                    object_name: "alpha_user".to_string(),
                    body: previous.clone(),
                }),
                ConflictCheck::ContentEqualsAfter { body: json!({}) },
            ))
            .unwrap();

        revert_failed_update(
            &mut state,
            &mut undo,
            "sandbox",
            "alpha_user",
            undo_id,
            previous,
        )
        .unwrap();

        let LoadState::Loaded(doc) = state.data.get("sandbox").unwrap() else {
            panic!("managed cache is not loaded");
        };
        assert!(crate::managed::api::objects(doc).unwrap().is_empty());
        assert_eq!(undo.load(undo_id).unwrap().status, EntryStatus::Expired);
    }

    #[test]
    fn rename_object_repoints_scalar_and_array_relationships() {
        let doc = json!({"objects": [
            {"name": "A", "schema": {"properties": {}}},
            {"name": "B", "schema": {"properties": {"a": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]}}}},
            {"name": "C", "schema": {"properties": {"as": {"type": "array", "items": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]}}}}}
        ]});
        let (renamed, count) = rename_object_in_doc(&doc, "A", "A2").unwrap();
        assert_eq!(count, 2);
        assert_eq!(renamed["objects"][0]["name"], "A2");
        assert_eq!(
            renamed["objects"][1]["schema"]["properties"]["a"]["resourceCollection"][0]["path"],
            "managed/A2"
        );
        assert_eq!(
            renamed["objects"][2]["schema"]["properties"]["as"]["items"]["resourceCollection"][0]["path"],
            "managed/A2"
        );
    }

    #[test]
    fn rename_object_rejects_collision_and_missing_source() {
        let doc = json!({"objects": [{"name": "A"}, {"name": "B"}]});
        assert!(rename_object_in_doc(&doc, "A", "B").is_err());
        assert!(rename_object_in_doc(&doc, "missing", "C").is_err());
    }

    #[test]
    fn delete_object_removes_inbound_relationships_and_schema_references() {
        let doc = json!({"objects": [
            {"name": "A", "schema": {"properties": {}}},
            {"name": "B", "schema": {
                "properties": {
                    "a": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]},
                    "keep": {"type": "string"}
                },
                "order": ["a", "keep"],
                "required": ["a", "keep"]
            }},
            {"name": "C", "schema": {
                "properties": {
                    "as": {"type": "array", "items": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]}}
                },
                "order": ["as"],
                "required": ["as"]
            }}
        ]});
        let (deleted, inbound) = delete_object_in_doc(&doc, "A").unwrap();
        assert_eq!(
            inbound,
            vec![("B".into(), "a".into()), ("C".into(), "as".into())]
        );
        assert_eq!(deleted["objects"].as_array().unwrap().len(), 2);
        assert!(
            deleted["objects"][0]["schema"]["properties"]
                .get("a")
                .is_none()
        );
        assert_eq!(deleted["objects"][0]["schema"]["order"], json!(["keep"]));
        assert_eq!(deleted["objects"][0]["schema"]["required"], json!(["keep"]));
        assert!(
            deleted["objects"][1]["schema"]["properties"]
                .get("as")
                .is_none()
        );
        assert_eq!(deleted["objects"][1]["schema"]["order"], json!([]));
        assert_eq!(deleted["objects"][1]["schema"]["required"], json!([]));
    }

    #[test]
    fn delete_object_rejects_unknown_object() {
        let doc = json!({"objects": [{"name": "A"}]});
        assert_eq!(
            delete_object_in_doc(&doc, "missing").unwrap_err(),
            "No managed object named 'missing'"
        );
    }

    #[test]
    fn delete_object_without_inbound_relationships_reports_none() {
        let doc = json!({"objects": [
            {"name": "A", "schema": {"properties": {}}},
            {"name": "B", "schema": {"properties": {"name": {"type": "string"}}}}
        ]});
        let (_, inbound) = delete_object_in_doc(&doc, "A").unwrap();
        assert!(inbound.is_empty());
        assert!(inbound_relationships(&doc, "A").is_empty());
    }

    #[test]
    fn create_object_appends_minimal_shape() {
        let doc = json!({"objects": []});
        let created = create_object_in_doc(&doc, "test_object", "Test object", "").unwrap();
        let object = &created["objects"][0];
        assert_eq!(object["name"], json!("test_object"));
        assert_eq!(object["schema"]["type"], json!("object"));
        assert_eq!(object["schema"]["title"], json!("Test object"));
        assert_eq!(object["schema"]["properties"], json!({}));
        assert_eq!(object["schema"]["required"], json!([]));
        assert_eq!(object["schema"]["order"], json!([]));
        assert!(object["schema"].get("description").is_none());
    }

    #[test]
    fn create_object_includes_description_and_falls_back_to_name_for_title() {
        let doc = json!({"objects": []});
        let created = create_object_in_doc(&doc, "test_object", "", "Description").unwrap();
        assert_eq!(
            created["objects"][0]["schema"]["title"],
            json!("test_object")
        );
        assert_eq!(
            created["objects"][0]["schema"]["description"],
            json!("Description")
        );
    }

    #[test]
    fn create_object_rejects_name_collision() {
        let doc = json!({"objects": [{"name": "test_object"}]});
        assert!(create_object_in_doc(&doc, "test_object", "", "").is_err());
    }

    fn relationship_doc(names: &[&str]) -> Value {
        json!({"objects": names
            .iter()
            .map(|name| json!({"name": name, "schema": {"properties": {}, "order": [], "required": []}}))
            .collect::<Vec<_>>()
        })
    }

    fn relationship_spec(forward: Cardinality, reverse: ReverseCardinality) -> RelationshipSpec {
        RelationshipSpec {
            source_object: "a".into(),
            key: "owner".into(),
            title: "Owner".into(),
            description: "Owning object".into(),
            target_object: "b".into(),
            forward,
            reverse,
            reverse_key: "owned".into(),
            searchable: true,
            viewable: false,
            user_editable: false,
            required: true,
            validate: true,
            ref_properties: Vec::new(),
        }
    }

    #[test]
    fn relationship_create_supports_all_cardinality_combinations() {
        for forward in [Cardinality::One, Cardinality::Many] {
            for reverse in [
                ReverseCardinality::None,
                ReverseCardinality::One,
                ReverseCardinality::Many,
            ] {
                let spec = relationship_spec(forward, reverse);
                let updated =
                    apply_relationship_spec(&relationship_doc(&["a", "b"]), &spec, None).unwrap();
                let source = &updated["objects"][0]["schema"]["properties"]["owner"];
                let source_node = if forward == Cardinality::Many {
                    &source["items"]
                } else {
                    source
                };
                assert_eq!(
                    source["type"],
                    json!(if forward == Cardinality::Many {
                        "array"
                    } else {
                        "relationship"
                    })
                );
                assert_eq!(
                    source_node["reverseRelationship"],
                    json!(reverse != ReverseCardinality::None)
                );
                assert_eq!(
                    source_node.get("reversePropertyName").is_some(),
                    reverse != ReverseCardinality::None
                );
                assert_eq!(updated["objects"][0]["schema"]["order"], json!(["owner"]));
                assert_eq!(
                    updated["objects"][0]["schema"]["required"],
                    json!(["owner"])
                );

                let reverse_property = &updated["objects"][1]["schema"]["properties"]["owned"];
                if reverse == ReverseCardinality::None {
                    assert!(reverse_property.is_null());
                    assert_eq!(updated["objects"][1]["schema"]["order"], json!([]));
                } else {
                    assert_eq!(
                        reverse_property["type"],
                        json!(if reverse == ReverseCardinality::Many {
                            "array"
                        } else {
                            "relationship"
                        })
                    );
                    let reverse_node = if reverse == ReverseCardinality::Many {
                        &reverse_property["items"]
                    } else {
                        reverse_property
                    };
                    assert_eq!(reverse_node["reversePropertyName"], json!("owner"));
                    assert_eq!(updated["objects"][1]["schema"]["order"], json!(["owned"]));
                    assert_eq!(updated["objects"][1]["schema"]["required"], json!([]));
                }
            }
        }
    }

    #[test]
    fn relationship_self_reference_keeps_both_ends_on_one_object() {
        let mut spec = relationship_spec(Cardinality::One, ReverseCardinality::Many);
        spec.source_object = "a".into();
        spec.target_object = "a".into();
        let updated = apply_relationship_spec(&relationship_doc(&["a"]), &spec, None).unwrap();
        let properties = &updated["objects"][0]["schema"]["properties"];
        assert_eq!(properties["owner"]["reversePropertyName"], json!("owned"));
        assert_eq!(
            properties["owned"]["items"]["reversePropertyName"],
            json!("owner")
        );

        spec.reverse_key = "owner".into();
        assert!(apply_relationship_spec(&relationship_doc(&["a"]), &spec, None).is_err());
    }

    #[test]
    fn relationship_places_attributes_and_custom_ref_properties_on_the_right_nodes() {
        let mut spec = relationship_spec(Cardinality::Many, ReverseCardinality::One);
        spec.ref_properties.push(RefProperty {
            name: "grantType".into(),
            label: "Grant".into(),
            kind: crate::managed::state::RefPropType::String,
        });
        let updated = apply_relationship_spec(&relationship_doc(&["a", "b"]), &spec, None).unwrap();
        let source = &updated["objects"][0]["schema"]["properties"]["owner"];
        assert_eq!(source["searchable"], json!(true));
        assert_eq!(source["viewable"], json!(false));
        assert_eq!(source["userEditable"], json!(false));
        assert_eq!(source["returnByDefault"], json!(false));
        assert_eq!(source["items"]["validate"], json!(true));
        assert_eq!(
            source["items"]["properties"]["_refProperties"]["properties"]["grantType"]["label"],
            json!("Grant")
        );
        assert_eq!(
            updated["objects"][1]["schema"]["properties"]["owned"]["properties"]["_refProperties"]
                ["properties"],
            json!({"_id": {"type": "string"}})
        );
    }

    #[test]
    fn commit_ref_prop_adds_a_serialized_custom_property() {
        let doc = relationship_doc(&["a", "b"]);
        let mut form = crate::managed::state::RelationshipFormState::new_create(
            "sandbox".into(),
            "a".into(),
            doc.clone(),
        );
        let mut draft = crate::managed::state::RefPropDraft::new_add();
        draft.name.set("grantType");
        draft.label.set("Grant");

        commit_ref_prop(&mut form, &draft).unwrap();

        assert_eq!(form.ref_properties.len(), 1);
        let mut spec = relationship_spec(Cardinality::One, ReverseCardinality::None);
        spec.ref_properties = form.ref_properties;
        let updated = apply_relationship_spec(&doc, &spec, None).unwrap();
        assert_eq!(
            updated["objects"][0]["schema"]["properties"]["owner"]["properties"]["_refProperties"]
                ["properties"]["grantType"],
            json!({"type": "string", "label": "Grant", "propName": "grantType"})
        );
        assert_eq!(
            updated["objects"][0]["schema"]["properties"]["owner"]["properties"]["_refProperties"]
                ["properties"]["_id"],
            json!({"type": "string"})
        );
    }

    #[test]
    fn commit_ref_prop_validates_names_and_replaces_in_place() {
        let doc = relationship_doc(&["a", "b"]);
        let mut form = crate::managed::state::RelationshipFormState::new_create(
            "sandbox".into(),
            "a".into(),
            doc,
        );
        let mut draft = crate::managed::state::RefPropDraft::new_add();
        for invalid in ["", "_id", "not-valid"] {
            draft.name.set(invalid);
            assert!(commit_ref_prop(&mut form, &draft).is_err());
        }

        draft.name.set("note");
        commit_ref_prop(&mut form, &draft).unwrap();
        assert!(commit_ref_prop(&mut form, &draft).is_err());

        let mut edit = crate::managed::state::RefPropDraft::edit(0, &form.ref_properties[0]);
        edit.label.set("Note");
        edit.kind = crate::managed::state::RefPropType::Boolean;
        commit_ref_prop(&mut form, &edit).unwrap();

        assert_eq!(form.ref_properties.len(), 1);
        assert_eq!(form.ref_properties[0].name, "note");
        assert_eq!(form.ref_properties[0].label, "Note");
        assert_eq!(
            form.ref_properties[0].kind,
            crate::managed::state::RefPropType::Boolean
        );
    }

    #[test]
    fn relationship_edit_reconciles_renames_reverse_drops_and_target_repoints() {
        let original = apply_relationship_spec(
            &relationship_doc(&["a", "b", "c"]),
            &relationship_spec(Cardinality::One, ReverseCardinality::One),
            None,
        )
        .unwrap();
        let previous = PreviousRelationship {
            old_key: "owner".into(),
            old_target: "b".into(),
            old_reverse_key: Some("owned".into()),
        };
        let mut renamed = relationship_spec(Cardinality::Many, ReverseCardinality::None);
        renamed.key = "owners".into();
        let dropped = apply_relationship_spec(&original, &renamed, Some(&previous)).unwrap();
        assert!(dropped["objects"][0]["schema"]["properties"]["owner"].is_null());
        assert_eq!(
            dropped["objects"][0]["schema"]["required"],
            json!(["owners"])
        );
        assert!(dropped["objects"][1]["schema"]["properties"]["owned"].is_null());

        let mut repointed = relationship_spec(Cardinality::One, ReverseCardinality::Many);
        repointed.target_object = "c".into();
        let repointed = apply_relationship_spec(&original, &repointed, Some(&previous)).unwrap();
        assert!(repointed["objects"][1]["schema"]["properties"]["owned"].is_null());
        assert!(repointed["objects"][2]["schema"]["properties"]["owned"].is_object());
        assert_eq!(
            repointed["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"][0]["path"],
            json!("managed/c")
        );
    }

    #[test]
    fn relationship_rejects_invalid_intent_and_property_collisions() {
        let spec = relationship_spec(Cardinality::One, ReverseCardinality::One);
        assert!(apply_relationship_spec(&relationship_doc(&["b"]), &spec, None).is_err());
        let mut invalid = spec.clone();
        invalid.key = "not-valid".into();
        assert!(apply_relationship_spec(&relationship_doc(&["a", "b"]), &invalid, None).is_err());
        invalid = spec.clone();
        invalid.reverse_key.clear();
        assert!(apply_relationship_spec(&relationship_doc(&["a", "b"]), &invalid, None).is_err());
        let colliding = json!({"objects": [
            {"name": "a", "schema": {"properties": {"owner": {"type": "string"}}}},
            {"name": "b", "schema": {"properties": {}}}
        ]});
        assert!(apply_relationship_spec(&colliding, &spec, None).is_err());
    }

    #[test]
    fn parse_relationship_round_trips_source_property() {
        let mut spec = relationship_spec(Cardinality::Many, ReverseCardinality::One);
        spec.ref_properties.push(RefProperty {
            name: "grantType".into(),
            label: "Grant".into(),
            kind: crate::managed::state::RefPropType::String,
        });
        assert_eq!(
            parse_relationship(&source_property(&spec)),
            Some(ParsedRelationship {
                forward: Cardinality::Many,
                target: "b".into(),
                reverse_key: Some("owned".into()),
                searchable: true,
                viewable: false,
                user_editable: false,
                validate: true,
                ref_property_names: vec!["grantType".into()],
            })
        );
    }

    #[test]
    fn parse_ref_properties_keeps_custom_definitions() {
        let property = json!({"type": "relationship", "properties": {"_refProperties": {"properties": {"_id": {"type": "string"}, "note": {"type": "string", "label": "Note"}, "rank": {"type": "number"}, "enabled": {"type": "boolean"}}}}});
        let properties = parse_ref_properties(&property);
        assert_eq!(properties.len(), 3);
        assert_eq!(properties[0].name, "enabled");
        assert_eq!(properties[1].label, "Note");
        assert_eq!(
            properties[2].kind,
            crate::managed::state::RefPropType::Number
        );
    }
}
