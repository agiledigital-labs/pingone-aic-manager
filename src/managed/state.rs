use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::tui::widgets::{LineEditor, TextField};

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Value),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ManagedMatch {
    /// Index into the summaries returned by `managed::api::summarize`.
    pub idx: usize,
    pub name: String,
    pub properties: usize,
    pub hooks_inline: usize,
    pub hooks_file: usize,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectClass {
    Standard,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldTier {
    StandardFieldOnStandardObject,
    CustomFieldOnStandardObject,
    FieldOnCustomObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEditAttrs {
    Standard,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAttr {
    Title,
    Description,
    Required,
    Searchable,
    Viewable,
    UserEditable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldCaps {
    pub tier: FieldTier,
    pub attrs: FieldEditAttrs,
    pub change_type: bool,
    pub rename_key: bool,
    pub delete: bool,
}

impl FieldCaps {
    pub fn can_edit_attr(self, attr: FieldAttr) -> bool {
        match self.attrs {
            FieldEditAttrs::Standard | FieldEditAttrs::Full => matches!(
                attr,
                FieldAttr::Title
                    | FieldAttr::Description
                    | FieldAttr::Required
                    | FieldAttr::Searchable
                    | FieldAttr::Viewable
                    | FieldAttr::UserEditable
            ),
        }
    }
}

/// Ping-shipped realm objects carry both top-level markers; custom objects do
/// not. This mirrors the verified 2026-06-14 schema behaviour.
pub fn object_class(object_def: &Value) -> ObjectClass {
    if object_def.get("type").is_some() && object_def.get("meta").is_some() {
        ObjectClass::Standard
    } else {
        ObjectClass::Custom
    }
}

pub fn is_standard_object(object_def: &Value) -> bool {
    object_class(object_def) == ObjectClass::Standard
}

/// Ping-provided objects cannot be renamed.  Some shipped objects omit `meta`,
/// so this intentionally differs from `is_standard_object`.
pub fn is_ping_shipped_object(object_def: &Value) -> bool {
    object_def.get("type").is_some()
}

/// Validates the identity used as an IDM managed-object path component.
pub fn validate_object_name(name: &str, existing: &[String], old: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Managed object name is required".into());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("Managed object name is required".into());
    };
    if !first.is_ascii_alphabetic() || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return Err("Managed object name must start with a letter and contain only letters, numbers, or underscores".into());
    }
    if name == old {
        return Err("New managed object name must be different".into());
    }
    if existing.iter().any(|candidate| candidate == name) {
        return Err(format!("Managed object '{name}' already exists"));
    }
    Ok(())
}

pub fn is_custom_object(object_def: &Value) -> bool {
    object_class(object_def) == ObjectClass::Custom
}

pub fn is_custom_field(object_def: &Value, prop_key: &str) -> bool {
    match object_class(object_def) {
        ObjectClass::Standard => prop_key.starts_with("custom_"),
        ObjectClass::Custom => true,
    }
}

pub fn field_capability(object_def: &Value, prop_key: &str) -> FieldCaps {
    match (object_class(object_def), prop_key.starts_with("custom_")) {
        (ObjectClass::Standard, false) => FieldCaps {
            tier: FieldTier::StandardFieldOnStandardObject,
            attrs: FieldEditAttrs::Standard,
            change_type: false,
            rename_key: false,
            delete: false,
        },
        (ObjectClass::Standard, true) => FieldCaps {
            tier: FieldTier::CustomFieldOnStandardObject,
            attrs: FieldEditAttrs::Full,
            change_type: true,
            rename_key: true,
            delete: true,
        },
        (ObjectClass::Custom, _) => FieldCaps {
            tier: FieldTier::FieldOnCustomObject,
            attrs: FieldEditAttrs::Full,
            change_type: true,
            rename_key: true,
            delete: true,
        },
    }
}

pub fn field_capability_for_property(
    object_def: &Value,
    prop_key: &str,
    property: &Value,
) -> FieldCaps {
    let mut caps = field_capability(object_def, prop_key);
    if is_relationship_property(property) {
        caps.rename_key = false;
    }
    caps
}

pub fn is_relationship_property(property: &Value) -> bool {
    property.get("type").and_then(Value::as_str) == Some("relationship")
        || (property.get("type").and_then(Value::as_str) == Some("array")
            && property.pointer("/items/type").and_then(Value::as_str) == Some("relationship"))
}

pub fn normalize_new_property_key(object_def: &Value, raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("Property key is required".into());
    }
    let key = if object_class(object_def) == ObjectClass::Standard {
        let key = if key.starts_with("custom_") {
            key.to_string()
        } else {
            format!("custom_{key}")
        };
        if key == "custom_" {
            return Err("Custom property key needs a suffix after custom_".into());
        }
        key
    } else {
        key.to_string()
    };
    validate_property_key(&key)?;
    Ok(key)
}

/// Rejects property keys that cannot be represented in a managed schema.
pub fn validate_property_key(key: &str) -> Result<(), String> {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err("Property key is required".into());
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(
            "Property key must start with a letter or underscore and contain only letters, numbers, or underscores"
                .into(),
        );
    }
    if chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        Ok(())
    } else {
        Err(
            "Property key must start with a letter or underscore and contain only letters, numbers, or underscores"
                .into(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditFieldFocus {
    Key,
    Title,
    Description,
    Required,
    Searchable,
    Viewable,
    UserEditable,
    Save,
}

impl EditFieldFocus {
    fn order(caps: FieldCaps) -> Vec<EditFieldFocus> {
        let mut order = Vec::new();
        if caps.rename_key {
            order.push(EditFieldFocus::Key);
        }
        if caps.can_edit_attr(FieldAttr::Title) {
            order.push(EditFieldFocus::Title);
        }
        if caps.can_edit_attr(FieldAttr::Description) {
            order.push(EditFieldFocus::Description);
        }
        if caps.can_edit_attr(FieldAttr::Required) {
            order.push(EditFieldFocus::Required);
        }
        if caps.can_edit_attr(FieldAttr::Searchable) {
            order.push(EditFieldFocus::Searchable);
        }
        if caps.can_edit_attr(FieldAttr::Viewable) {
            order.push(EditFieldFocus::Viewable);
        }
        if caps.can_edit_attr(FieldAttr::UserEditable) {
            order.push(EditFieldFocus::UserEditable);
        }
        order.push(EditFieldFocus::Save);
        order
    }

    pub fn next(self, caps: FieldCaps) -> Self {
        let order = Self::order(caps);
        let i = order.iter().position(|field| *field == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn prev(self, caps: FieldCaps) -> Self {
        let order = Self::order(caps);
        let i = order.iter().position(|field| *field == self).unwrap_or(0);
        order[(i + order.len() - 1) % order.len()]
    }

    pub fn is_bool(self) -> bool {
        matches!(
            self,
            EditFieldFocus::Required
                | EditFieldFocus::Searchable
                | EditFieldFocus::Viewable
                | EditFieldFocus::UserEditable
        )
    }
}

#[derive(Debug)]
pub struct FieldEditState {
    pub tenant_name: String,
    pub object_name: String,
    pub field_key: String,
    pub property_type: String,
    pub original_object: Value,
    pub original_property: Value,
    pub caps: FieldCaps,
    pub key: TextField,
    pub title: TextField,
    pub description: TextField,
    pub required: bool,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub focused: EditFieldFocus,
    pub error: Option<String>,
}

impl FieldEditState {
    pub fn from_property(
        tenant_name: String,
        object_name: String,
        field_key: String,
        object_def: Value,
        property: Value,
        required: bool,
    ) -> Self {
        let caps = field_capability_for_property(&object_def, &field_key, &property);
        let title = property
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let description = property
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let searchable = property
            .get("searchable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let viewable = property
            .get("viewable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let user_editable = property
            .get("userEditable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let property_type = crate::managed::state::property_type(&property);
        let mut key = TextField::single_line("Key").with_initial(field_key.clone());
        if object_class(&object_def) == ObjectClass::Standard && field_key.starts_with("custom_") {
            key.locked_prefix = "custom_".into();
            key.cursor = key.value.chars().count();
        }

        Self {
            tenant_name,
            object_name,
            field_key,
            property_type,
            original_object: object_def,
            original_property: property,
            caps,
            key,
            title: TextField::single_line("Title").with_initial(title),
            description: TextField::textarea("Description").with_initial(description),
            required,
            searchable,
            viewable,
            user_editable,
            focused: EditFieldFocus::Title,
            error: None,
        }
    }

    pub fn toggle_focused_bool(&mut self) {
        match self.focused {
            EditFieldFocus::Required => self.required = !self.required,
            EditFieldFocus::Searchable => self.searchable = !self.searchable,
            EditFieldFocus::Viewable => self.viewable = !self.viewable,
            EditFieldFocus::UserEditable => self.user_editable = !self.user_editable,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarFieldType {
    String,
    Boolean,
    Number,
    StringArray,
}

impl ScalarFieldType {
    pub fn next(self) -> Self {
        match self {
            ScalarFieldType::String => ScalarFieldType::Boolean,
            ScalarFieldType::Boolean => ScalarFieldType::Number,
            ScalarFieldType::Number => ScalarFieldType::StringArray,
            ScalarFieldType::StringArray => ScalarFieldType::String,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ScalarFieldType::String => ScalarFieldType::StringArray,
            ScalarFieldType::Boolean => ScalarFieldType::String,
            ScalarFieldType::Number => ScalarFieldType::Boolean,
            ScalarFieldType::StringArray => ScalarFieldType::Number,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScalarFieldType::String => "string",
            ScalarFieldType::Boolean => "boolean",
            ScalarFieldType::Number => "number",
            ScalarFieldType::StringArray => "string[]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddFieldFocus {
    Key,
    Title,
    Description,
    Type,
    Searchable,
    Viewable,
    UserEditable,
    Required,
    Save,
}

impl AddFieldFocus {
    const ORDER: [AddFieldFocus; 9] = [
        AddFieldFocus::Key,
        AddFieldFocus::Title,
        AddFieldFocus::Description,
        AddFieldFocus::Type,
        AddFieldFocus::Searchable,
        AddFieldFocus::Viewable,
        AddFieldFocus::UserEditable,
        AddFieldFocus::Required,
        AddFieldFocus::Save,
    ];

    pub fn next(self) -> Self {
        let i = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    pub fn is_bool(self) -> bool {
        matches!(
            self,
            AddFieldFocus::Searchable
                | AddFieldFocus::Viewable
                | AddFieldFocus::UserEditable
                | AddFieldFocus::Required
        )
    }
}

#[derive(Debug)]
pub struct AddFieldState {
    pub tenant_name: String,
    pub object_name: String,
    pub original_object: Value,
    pub key: TextField,
    pub title: TextField,
    pub description: TextField,
    pub field_type: ScalarFieldType,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub required: bool,
    pub focused: AddFieldFocus,
    pub error: Option<String>,
}

impl AddFieldState {
    pub fn new(tenant_name: String, object_name: String, object_def: Value) -> Self {
        let key = property_key_field(&object_def);
        Self {
            tenant_name,
            object_name,
            original_object: object_def,
            key,
            title: TextField::single_line("Title"),
            description: TextField::textarea("Description"),
            field_type: ScalarFieldType::String,
            searchable: false,
            viewable: true,
            user_editable: true,
            required: false,
            focused: AddFieldFocus::Key,
            error: None,
        }
    }

    pub fn toggle_focused_bool(&mut self) {
        match self.focused {
            AddFieldFocus::Searchable => self.searchable = !self.searchable,
            AddFieldFocus::Viewable => self.viewable = !self.viewable,
            AddFieldFocus::UserEditable => self.user_editable = !self.user_editable,
            AddFieldFocus::Required => self.required = !self.required,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddRelationshipFocus {
    Key,
    Title,
    Description,
    Target,
    Collection,
    Validate,
    ReversePropertyName,
    Save,
}

impl AddRelationshipFocus {
    const ORDER: [AddRelationshipFocus; 8] = [
        AddRelationshipFocus::Key,
        AddRelationshipFocus::Title,
        AddRelationshipFocus::Description,
        AddRelationshipFocus::Target,
        AddRelationshipFocus::Collection,
        AddRelationshipFocus::Validate,
        AddRelationshipFocus::ReversePropertyName,
        AddRelationshipFocus::Save,
    ];

    pub fn next(self) -> Self {
        let i = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    pub fn is_bool(self) -> bool {
        matches!(
            self,
            AddRelationshipFocus::Collection | AddRelationshipFocus::Validate
        )
    }
}

#[derive(Debug)]
pub struct AddRelationshipState {
    pub tenant_name: String,
    pub object_name: String,
    pub original_object: Value,
    pub key: TextField,
    pub title: TextField,
    pub description: TextField,
    pub collection: bool,
    pub validate: bool,
    pub reverse_property_name: TextField,
    pub target_name: Option<String>,
    pub target_query: LineEditor,
    pub target_selected: usize,
    pub focused: AddRelationshipFocus,
    pub error: Option<String>,
}

impl AddRelationshipState {
    pub fn new(tenant_name: String, object_name: String, object_def: Value) -> Self {
        let key = property_key_field(&object_def);
        Self {
            tenant_name,
            object_name,
            original_object: object_def,
            key,
            title: TextField::single_line("Title"),
            description: TextField::textarea("Description"),
            collection: false,
            validate: true,
            reverse_property_name: TextField::single_line("Reverse property"),
            target_name: None,
            target_query: LineEditor::new(),
            target_selected: 0,
            focused: AddRelationshipFocus::Key,
            error: None,
        }
    }

    pub fn toggle_focused_bool(&mut self) {
        match self.focused {
            AddRelationshipFocus::Collection => self.collection = !self.collection,
            AddRelationshipFocus::Validate => self.validate = !self.validate,
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct AddHookState {
    pub tenant_name: String,
    pub object_name: String,
    pub original_object: Value,
    pub events: Vec<&'static str>,
    pub selected: usize,
    pub error: Option<String>,
}

impl AddHookState {
    pub fn new(tenant_name: String, object_name: String, object_def: Value) -> Self {
        let events = available_hook_events(&object_def);
        Self {
            tenant_name,
            object_name,
            original_object: object_def,
            events,
            selected: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeleteFieldState {
    pub tenant_name: String,
    pub object_name: String,
    pub field_key: String,
    pub original_object: Value,
    pub is_relationship: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The kind of managed property to add from the chooser.
pub enum AddKind {
    Field,
    Relationship,
}

impl AddKind {
    /// Switches between the two add flows.
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Field => Self::Relationship,
            Self::Relationship => Self::Field,
        };
    }
}

#[derive(Debug)]
/// Draft state for selecting the managed-property add flow.
pub struct AddChooseState {
    pub kind: AddKind,
}

impl Default for AddChooseState {
    fn default() -> Self {
        Self {
            kind: AddKind::Field,
        }
    }
}

#[derive(Debug)]
/// Draft for renaming a selected managed property key.
pub struct RenameFieldState {
    pub tenant_name: String,
    pub object_name: String,
    pub old_key: String,
    pub original_object: Value,
    pub key: TextField,
    pub focused: RenameFieldFocus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The single focusable control in the property-key rename draft.
pub enum RenameFieldFocus {
    Key,
}

impl RenameFieldState {
    /// Seeds a rename draft with the selected key.
    pub fn new(
        tenant_name: String,
        object_name: String,
        old_key: String,
        original_object: Value,
    ) -> Self {
        Self {
            tenant_name,
            object_name,
            key: TextField::single_line("Key").with_initial(old_key.clone()),
            old_key,
            original_object,
            error: None,
            focused: RenameFieldFocus::Key,
        }
    }
}

#[derive(Debug)]
pub struct RenameObjectState {
    pub tenant_name: String,
    pub old_name: String,
    pub original_doc: Value,
    pub key: TextField,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewObjectFocus {
    Name,
    Title,
    Description,
    Save,
}

impl NewObjectFocus {
    const ORDER: [NewObjectFocus; 4] = [
        NewObjectFocus::Name,
        NewObjectFocus::Title,
        NewObjectFocus::Description,
        NewObjectFocus::Save,
    ];

    pub fn next(self) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        Self::ORDER[(index + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        Self::ORDER[(index + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug)]
pub struct NewObjectState {
    pub tenant_name: String,
    pub original_doc: Value,
    pub name: TextField,
    pub title: TextField,
    pub description: TextField,
    pub focused: NewObjectFocus,
    pub error: Option<String>,
}

impl NewObjectState {
    pub fn new(tenant_name: String, original_doc: Value) -> Self {
        Self {
            tenant_name,
            original_doc,
            name: TextField::single_line("Name"),
            title: TextField::single_line("Title"),
            description: TextField::textarea("Description"),
            focused: NewObjectFocus::Name,
            error: None,
        }
    }
}

impl RenameObjectState {
    pub fn new(tenant_name: String, old_name: String, original_doc: Value) -> Self {
        Self {
            tenant_name,
            key: TextField::single_line("Name").with_initial(old_name.clone()),
            old_name,
            original_doc,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct RenameObjectConfirmState {
    pub draft: RenameObjectState,
    pub repoints: usize,
    pub record_count: Option<crate::managed::api::RecordCount>,
    pub count_error: Option<String>,
}

pub const HOOK_EVENTS: [&str; 6] = [
    "onCreate",
    "onUpdate",
    "onDelete",
    "postCreate",
    "postUpdate",
    "postDelete",
];

pub fn available_hook_events(object_def: &Value) -> Vec<&'static str> {
    HOOK_EVENTS
        .into_iter()
        .filter(|event| object_def.get(*event).is_none())
        .collect()
}

fn property_key_field(object_def: &Value) -> TextField {
    if object_class(object_def) == ObjectClass::Standard {
        TextField::single_line("Key").with_locked_prefix("custom_")
    } else {
        TextField::single_line("Key")
    }
}

pub fn properties(object_def: &Value) -> Option<&serde_json::Map<String, Value>> {
    object_def
        .pointer("/schema/properties")
        .and_then(Value::as_object)
}

pub fn property_names(object_def: &Value) -> Vec<String> {
    let mut names: Vec<String> = properties(object_def)
        .into_iter()
        .flat_map(|properties| properties.keys().cloned())
        .collect();
    names.sort();
    names
}

pub fn required_fields(object_def: &Value) -> HashSet<String> {
    object_def
        .pointer("/schema/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

pub fn property_type(property: &Value) -> String {
    match property.get("type") {
        Some(Value::String(kind)) => base_type(kind, property),
        Some(Value::Array(kinds)) if kinds.iter().any(|kind| kind.as_str() == Some("null")) => {
            let base = kinds
                .iter()
                .filter_map(Value::as_str)
                .find(|kind| *kind != "null")
                .map(|kind| base_type(kind, property))
                .unwrap_or_else(|| "any".to_string());
            format!("{base}?")
        }
        _ => "any".to_string(),
    }
}

fn base_type(kind: &str, property: &Value) -> String {
    match kind {
        "string" | "boolean" | "number" | "integer" | "object" | "relationship" => kind.to_string(),
        "array" => {
            let item = property
                .pointer("/items/type")
                .and_then(Value::as_str)
                .map(|kind| base_type(kind, &Value::Null))
                .unwrap_or_else(|| "any".to_string());
            format!("{item}[]")
        }
        _ => kind.to_string(),
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub data: HashMap<String, LoadState>,
    pub refreshing: HashSet<String>,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
    pub property_selected: usize,
    pub editing: Option<FieldEditState>,
    pub add_field: Option<AddFieldState>,
    pub add_relationship: Option<AddRelationshipState>,
    pub add_choose: Option<AddChooseState>,
    pub add_hook: Option<AddHookState>,
    pub pending_delete: Option<DeleteFieldState>,
    pub renaming: Option<RenameFieldState>,
    pub renaming_object: Option<RenameObjectState>,
    pub rename_object_confirm: Option<RenameObjectConfirmState>,
    pub new_object: Option<NewObjectState>,
    pub in_flight_writes: HashSet<(String, String)>,
    pub failed_writes: HashSet<(String, String)>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.property_selected = 0;
        self.clear_active_drafts();
    }

    pub fn clamp_selection(&mut self, n: usize) {
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    pub fn select_object(&mut self, idx: usize) {
        if self.selected != idx {
            self.selected = idx;
            self.property_selected = 0;
        }
    }

    pub fn clamp_property_selection(&mut self, n: usize) {
        if self.property_selected >= n {
            self.property_selected = n.saturating_sub(1);
        }
    }

    pub fn clear_active_drafts(&mut self) {
        self.editing = None;
        self.add_field = None;
        self.add_relationship = None;
        self.add_choose = None;
        self.add_hook = None;
        self.pending_delete = None;
        self.renaming = None;
        self.renaming_object = None;
        self.rename_object_confirm = None;
        self.new_object = None;
    }

    pub fn matches(&self, tenant: Option<&str>) -> Vec<ManagedMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(doc)) = self.data.get(tenant) else {
            return Vec::new();
        };
        let Ok(summaries) = crate::managed::api::summarize(doc) else {
            return Vec::new();
        };

        if self.query.is_empty() {
            let mut matches: Vec<ManagedMatch> = summaries
                .iter()
                .enumerate()
                .map(|(idx, summary)| ManagedMatch {
                    idx,
                    name: summary.name.clone(),
                    properties: summary.properties,
                    hooks_inline: summary.hooks_inline.len(),
                    hooks_file: summary.hooks_file.len(),
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
            matches.sort_by(|a, b| a.name.cmp(&b.name));
            return matches;
        }

        let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(self.query.value());
        let mut matches = Vec::new();
        for (idx, summary) in summaries.iter().enumerate() {
            if let Some((score, positions)) = matcher.match_indices(&summary.name) {
                matches.push(ManagedMatch {
                    idx,
                    name: summary.name.clone(),
                    properties: summary.properties,
                    hooks_inline: summary.hooks_inline.len(),
                    hooks_file: summary.hooks_file.len(),
                    score,
                    positions,
                });
            }
        }
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        matches
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state_with_objects(objects: Value) -> State {
        let mut state = State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(objects));
        state
    }

    #[test]
    fn empty_query_sorts_objects_alphabetically() {
        let state = state_with_objects(json!({"objects": [
            {"name": "bravo_user", "schema": {"properties": {}}},
            {"name": "alpha_role", "schema": {"properties": {}}},
            {"name": "alpha_user", "schema": {"properties": {}}}
        ]}));

        let names: Vec<_> = state
            .matches(Some("sandbox"))
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(names, ["alpha_role", "alpha_user", "bravo_user"]);
    }

    #[test]
    fn query_fuzzy_filters_by_object_name() {
        let mut state = state_with_objects(json!({"objects": [
            {"name": "alpha_application", "schema": {"properties": {}}},
            {"name": "alpha_role", "schema": {"properties": {}}},
            {"name": "bravo_user", "schema": {"properties": {}}}
        ]}));
        state.query.set("arole");

        let matches = state.matches(Some("sandbox"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "alpha_role");
        assert!(!matches[0].positions.is_empty());
    }

    #[test]
    fn classification_uses_standard_markers_and_custom_prefix() {
        let standard = json!({"name": "alpha_user", "type": "managed", "meta": {}, "schema": {"properties": {}}});
        let custom = json!({"name": "alpha_lock", "schema": {"properties": {}}});

        assert!(is_standard_object(&standard));
        assert!(is_custom_object(&custom));
        assert!(!is_custom_field(&standard, "givenName"));
        assert!(is_custom_field(&standard, "custom_pet"));
        assert!(is_custom_field(&custom, "lockKey"));
    }

    #[test]
    fn field_capability_matches_tiers() {
        let standard = json!({"type": "managed", "meta": {}});
        let custom = json!({});

        let caps = field_capability(&standard, "givenName");
        assert_eq!(caps.tier, FieldTier::StandardFieldOnStandardObject);
        assert!(caps.can_edit_attr(FieldAttr::Description));
        assert!(!caps.change_type);
        assert!(!caps.rename_key);
        assert!(!caps.delete);

        let caps = field_capability(&standard, "custom_code");
        assert_eq!(caps.tier, FieldTier::CustomFieldOnStandardObject);
        assert!(caps.change_type);
        assert!(caps.rename_key);
        assert!(caps.delete);

        let caps = field_capability(&custom, "anything");
        assert_eq!(caps.tier, FieldTier::FieldOnCustomObject);
        assert!(caps.change_type);
        assert!(caps.rename_key);
        assert!(caps.delete);
    }

    #[test]
    fn relationship_capability_blocks_rename_but_allows_custom_delete() {
        let custom_object = json!({"name": "alpha_lock", "schema": {"properties": {}}});
        let relationship = json!({
            "type": "relationship",
            "resourceCollection": [{"path": "managed/alpha_user"}]
        });

        let caps = field_capability_for_property(&custom_object, "owner", &relationship);
        assert_eq!(caps.tier, FieldTier::FieldOnCustomObject);
        assert!(!caps.rename_key);
        assert!(caps.delete);

        let standard_object = json!({"type": "managed", "meta": {}});
        let caps = field_capability_for_property(&standard_object, "manager", &relationship);
        assert_eq!(caps.tier, FieldTier::StandardFieldOnStandardObject);
        assert!(!caps.rename_key);
        assert!(!caps.delete);
    }

    #[test]
    fn normalize_property_key_rejects_invalid_characters() {
        let standard = json!({"name": "alpha_user", "type": "managed", "meta": {}});
        let custom = json!({"name": "alpha_lock"});

        assert!(normalize_new_property_key(&standard, "my field").is_err());
        assert!(normalize_new_property_key(&custom, "1field").is_err());
        assert!(normalize_new_property_key(&custom, "field-name").is_err());
    }

    #[test]
    fn normalize_property_key_preserves_prefix_rules() {
        let standard = json!({"name": "alpha_user", "type": "managed", "meta": {}});
        let custom = json!({"name": "alpha_lock"});

        assert_eq!(
            normalize_new_property_key(&standard, "custom_code").unwrap(),
            "custom_code"
        );
        assert_eq!(
            normalize_new_property_key(&standard, "foo").unwrap(),
            "custom_foo"
        );
        assert_eq!(normalize_new_property_key(&custom, "foo").unwrap(), "foo");
        assert!(normalize_new_property_key(&standard, "custom_").is_err());
    }

    #[test]
    fn object_name_validation_rejects_invalid_collision_and_noop() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        assert!(validate_object_name("", &names, "alpha").is_err());
        assert!(validate_object_name("1bad", &names, "alpha").is_err());
        assert!(validate_object_name("bad-name", &names, "alpha").is_err());
        assert!(validate_object_name("beta", &names, "alpha").is_err());
        assert!(validate_object_name("alpha", &names, "alpha").is_err());
        assert!(validate_object_name("gamma_2", &names, "alpha").is_ok());
    }

    #[test]
    fn ping_shipped_guard_uses_type_without_meta() {
        assert!(is_ping_shipped_object(&json!({"type": "managed"})));
        assert!(!is_ping_shipped_object(&json!({"name": "test_object"})));
    }
}
