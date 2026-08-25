use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::managed::spec::{
    DefaultChange, EnumChange, EnumSpec, FieldEditSpec, ScalarFieldType, parse_enum_items,
};

use crate::tui::widgets::{LineEditor, TextField, TypedValueField};

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
    Enum,
    Default,
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
                    | FieldAttr::Enum
                    | FieldAttr::Default
                    | FieldAttr::Required
                    | FieldAttr::Searchable
                    | FieldAttr::Viewable
                    | FieldAttr::UserEditable
            ),
        }
    }
}

/// How much freedom this object's **fields** get. Distinct from
/// [`is_ping_shipped_object`], which protects the **object** itself.
///
/// The marker is `meta`, not `type`. Both are Ping-shipped signals, but they
/// mark different things: all ten realm objects carry
/// `"type": "Managed Object"`, while only `alpha_user` / `bravo_user` carry
/// `meta` (verified 2026-08-07, `docs/api/10-managed-objects.md`). The `*_user`
/// pair is where Ping constrains what you may add — the rest accept ordinary
/// un-prefixed properties, confirmed against `alpha_organization` on
/// 2026-08-07.
///
/// So `role`, `organization`, `assignment` and `application` classify as
/// [`ObjectClass::Custom`]: their fields can be renamed, retyped and deleted,
/// and additions need no `custom_` prefix. That is a deliberate trade — those
/// objects carry Ping-shipped fields like `alpha_role.name` that this grants
/// rights over — accepted because the alternative was forcing `custom_` names
/// the server does not require. The object itself still cannot be renamed or
/// deleted; that guard reads `type`.
pub fn object_class(object_def: &Value) -> ObjectClass {
    if object_def.get("meta").is_some() {
        ObjectClass::Standard
    } else {
        ObjectClass::Custom
    }
}

pub fn is_standard_object(object_def: &Value) -> bool {
    object_class(object_def) == ObjectClass::Standard
}

/// Ping-provided objects cannot be renamed or deleted. Keyed on the top-level
/// `type` marker value, so it covers all ten realm objects — a wider set than
/// [`object_class`], which keys on `meta` to decide field freedom.
pub fn is_ping_shipped_object(object_def: &Value) -> bool {
    object_def.get("type").and_then(Value::as_str) == Some("Managed Object")
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
    Enum,
    Default,
    Required,
    Searchable,
    Viewable,
    UserEditable,
    Save,
}

impl EditFieldFocus {
    fn order(caps: FieldCaps, enum_eligible: bool) -> Vec<EditFieldFocus> {
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
        if enum_eligible && caps.can_edit_attr(FieldAttr::Enum) {
            order.push(EditFieldFocus::Enum);
        }
        if caps.can_edit_attr(FieldAttr::Default) {
            order.push(EditFieldFocus::Default);
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

    pub fn next(self, caps: FieldCaps, enum_eligible: bool) -> Self {
        let order = Self::order(caps, enum_eligible);
        let i = order.iter().position(|field| *field == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn prev(self, caps: FieldCaps, enum_eligible: bool) -> Self {
        let order = Self::order(caps, enum_eligible);
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
    pub enum_values: TextField,
    /// Stable rendering of the stored constraint; preserves untouched edits.
    pub enum_seed: String,
    pub default_value: TypedValueField,
    /// Stable rendering of the stored default; preserves untouched edits.
    pub default_seed: String,
    pub required: bool,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub focused: EditFieldFocus,
    pub allow_narrowing: bool,
    pub narrowed_enum_values: Vec<String>,
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
        let scalar_type = scalar_type_for_property(&property);
        let enum_seed = crate::managed::ops::property_enum(&property)
            .map(|constraint| constraint.to_items().join(", "))
            .unwrap_or_default();
        let default_seed = crate::managed::ops::property_default(&property).unwrap_or_default();
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
            enum_values: TextField::single_line("Allowed values").with_initial(enum_seed.clone()),
            enum_seed,
            default_value: TypedValueField::new("Default", scalar_type.shape(), false)
                .with_initial(default_seed.clone()),
            default_seed,
            required,
            searchable,
            viewable,
            user_editable,
            focused: EditFieldFocus::Title,
            allow_narrowing: false,
            narrowed_enum_values: Vec::new(),
            error: None,
        }
    }

    /// Parses the draft's comma-separated enum row, retaining an untouched
    /// constraint verbatim rather than rewriting it on every field save.
    pub fn enum_change(&self) -> Result<EnumChange, String> {
        if self.enum_values.value == self.enum_seed {
            return Ok(EnumChange::Unchanged);
        }
        if self.enum_values.value.trim().is_empty() {
            return Ok(EnumChange::Clear);
        }
        let items = self.enum_values.value.split(',').collect::<Vec<_>>();
        parse_enum_items(&items).map(EnumChange::Set)
    }

    /// An empty string default is not expressible here: matching the enum row,
    /// blank input means clear. The CLI can still write an empty string.
    pub fn default_change(&self) -> DefaultChange {
        if self.default_value.value() == self.default_seed {
            DefaultChange::Unchanged
        } else {
            let value = self.default_value.trimmed();
            if value.is_empty() {
                DefaultChange::Clear
            } else {
                DefaultChange::Set(value.to_string())
            }
        }
    }

    pub fn edit_spec(&self) -> Result<FieldEditSpec, String> {
        if self.caps.can_edit_attr(FieldAttr::Default) {
            self.default_value.validate()?;
        }
        Ok(FieldEditSpec {
            new_key: Some(self.key.value.clone()),
            title: Some(self.title.value.clone()),
            description: Some(self.description.value.clone()),
            required: Some(self.required),
            searchable: Some(self.searchable),
            viewable: Some(self.viewable),
            user_editable: Some(self.user_editable),
            enum_change: self.enum_change()?,
            default_change: self.default_change(),
            allow_narrowing: self.allow_narrowing,
        })
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

/// Cardinality of a relationship property on its source object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    Many,
}

impl Cardinality {
    pub fn next(self) -> Self {
        match self {
            Self::One => Self::Many,
            Self::Many => Self::One,
        }
    }

    pub fn prev(self) -> Self {
        self.next()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::One => "has one",
            Self::Many => "has many",
        }
    }
}

/// Cardinality of the optional reverse property on the target object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseCardinality {
    None,
    One,
    Many,
    /// The source declares a `reversePropertyName` the target object does not
    /// have. Inherited from the tenant, never chosen: `config/managed` runs no
    /// cross-object validation on write, so half-declared pairs are storable
    /// and Ping ships six (`docs/api/10-managed-objects.md`). Kept as its own
    /// state so an edit that leaves the reverse alone re-writes the declaration
    /// verbatim, instead of quietly deciding to create the missing property or
    /// to strip the claim.
    Dangling,
}

impl ReverseCardinality {
    /// Cycling leaves `Dangling` and cannot return to it: it is a state you
    /// inherit, and offering it as a choice would mean offering to author a new
    /// dangling declaration. Cancel the form to get it back.
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::One,
            Self::One => Self::Many,
            Self::Many | Self::Dangling => Self::None,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::None | Self::Dangling => Self::Many,
            Self::One => Self::None,
            Self::Many => Self::One,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "has none",
            Self::One => "has one",
            Self::Many => "has many",
            Self::Dangling => "declared, missing on target",
        }
    }

    /// Whether a reverse property has to be written to the target object.
    /// `Dangling` claims one without owning one, so it writes nothing there.
    pub fn writes_target_property(self) -> bool {
        matches!(self, Self::One | Self::Many)
    }
}

/// Scalar type for custom metadata held in a relationship's `_refProperties`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPropType {
    String,
    Number,
    Boolean,
}

impl RefPropType {
    pub fn next(self) -> Self {
        match self {
            Self::String => Self::Number,
            Self::Number => Self::Boolean,
            Self::Boolean => Self::String,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::String => Self::Boolean,
            Self::Number => Self::String,
            Self::Boolean => Self::Number,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
        }
    }
}

/// Custom metadata definition for one relationship side.
#[derive(Debug, Clone)]
pub struct RefProperty {
    pub name: String,
    pub label: String,
    pub kind: RefPropType,
}

/// Fully resolved intent for a create-or-edit relationship write.
#[derive(Debug, Clone)]
pub struct RelationshipSpec {
    pub source_object: String,
    pub key: String,
    pub title: String,
    pub description: String,
    pub target_object: String,
    pub forward: Cardinality,
    pub reverse: ReverseCardinality,
    pub reverse_key: String,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub required: bool,
    pub validate: bool,
    pub ref_properties: Vec<RefProperty>,
}

/// Relationship wiring that must be removed before an edit is inserted.
#[derive(Debug, Clone)]
pub struct PreviousRelationship {
    pub old_key: String,
    pub old_target: String,
    pub old_reverse_key: Option<String>,
}

/// Relationship details extracted from a managed schema property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRelationship {
    pub forward: Cardinality,
    pub target: String,
    pub reverse_key: Option<String>,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub validate: bool,
    pub ref_property_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddFieldFocus {
    Key,
    Title,
    Description,
    Enum,
    Default,
    Type,
    Searchable,
    Viewable,
    UserEditable,
    Required,
    Save,
}

impl AddFieldFocus {
    const ORDER: [AddFieldFocus; 11] = [
        AddFieldFocus::Key,
        AddFieldFocus::Title,
        AddFieldFocus::Description,
        // After Type, because Type decides whether this row exists at all.
        AddFieldFocus::Type,
        AddFieldFocus::Enum,
        AddFieldFocus::Default,
        AddFieldFocus::Searchable,
        AddFieldFocus::Viewable,
        AddFieldFocus::UserEditable,
        AddFieldFocus::Required,
        AddFieldFocus::Save,
    ];

    pub fn next(self, enum_eligible: bool) -> Self {
        let order = Self::order(enum_eligible);
        let i = order.iter().position(|field| *field == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn prev(self, enum_eligible: bool) -> Self {
        let order = Self::order(enum_eligible);
        let i = order.iter().position(|field| *field == self).unwrap_or(0);
        order[(i + order.len() - 1) % order.len()]
    }

    fn order(enum_eligible: bool) -> Vec<AddFieldFocus> {
        let mut order = Self::ORDER.to_vec();
        if !enum_eligible {
            order.retain(|focus| *focus != Self::Enum);
        }
        order
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
    pub enum_values: TextField,
    pub default_value: TypedValueField,
    /// Private so it can't drift from `default_value`'s shape — go through
    /// [`Self::set_field_type`]. A default validated against the wrong shape
    /// is how a type-mismatched default reaches the tenant, and that bricks
    /// the managed object (`docs/api/10-managed-objects.md`).
    field_type: ScalarFieldType,
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
            enum_values: TextField::single_line("Allowed values"),
            default_value: TypedValueField::new("Default", ScalarFieldType::String.shape(), false),
            field_type: ScalarFieldType::String,
            searchable: false,
            viewable: true,
            user_editable: true,
            required: false,
            focused: AddFieldFocus::Key,
            error: None,
        }
    }

    pub fn field_type(&self) -> ScalarFieldType {
        self.field_type
    }

    /// Retype the field and re-shape the default row with it, so the default
    /// is always validated against the type it will actually be written under.
    pub fn set_field_type(&mut self, field_type: ScalarFieldType) {
        self.field_type = field_type;
        self.default_value.set_shape(field_type.shape());
    }

    pub fn enum_eligible(&self) -> bool {
        crate::managed::ops::scalar_type_supports_enum(self.field_type)
    }

    pub fn parsed_enum_values(&self) -> Result<Option<EnumSpec>, String> {
        if !self.enum_eligible() || self.enum_values.value.trim().is_empty() {
            return Ok(None);
        }
        let items = self.enum_values.value.split(',').collect::<Vec<_>>();
        parse_enum_items(&items).map(Some)
    }

    pub fn parsed_default(&self) -> Option<String> {
        let value = self.default_value.trimmed();
        (!value.is_empty()).then(|| value.to_string())
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
pub enum RelationshipFocus {
    Key,
    Title,
    Description,
    Target,
    Forward,
    Reverse,
    ReverseKey,
    Searchable,
    Viewable,
    UserEditable,
    Required,
    Validate,
    RefProperties,
    Save,
}

impl RelationshipFocus {
    fn order(reverse: ReverseCardinality) -> Vec<Self> {
        let mut order = vec![
            Self::Key,
            Self::Title,
            Self::Description,
            Self::Target,
            Self::Forward,
            Self::Reverse,
        ];
        if reverse != ReverseCardinality::None {
            order.push(Self::ReverseKey);
        }
        order.extend([
            Self::Searchable,
            Self::Viewable,
            Self::UserEditable,
            Self::Required,
            Self::Validate,
            Self::RefProperties,
            Self::Save,
        ]);
        order
    }
    pub fn next(self, reverse: ReverseCardinality) -> Self {
        let order = Self::order(reverse);
        let index = order.iter().position(|focus| *focus == self).unwrap_or(0);
        order[(index + 1) % order.len()]
    }
    pub fn prev(self, reverse: ReverseCardinality) -> Self {
        let order = Self::order(reverse);
        let index = order.iter().position(|focus| *focus == self).unwrap_or(0);
        order[(index + order.len() - 1) % order.len()]
    }
    pub fn is_bool(self) -> bool {
        matches!(
            self,
            Self::Searchable
                | Self::Viewable
                | Self::UserEditable
                | Self::Required
                | Self::Validate
        )
    }
}

/// Active input within the relationship custom-property sub-editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPropFocus {
    Name,
    Label,
    Type,
    Save,
}

impl RefPropFocus {
    const ORDER: [Self; 4] = [Self::Name, Self::Label, Self::Type, Self::Save];

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

/// Draft state for adding or replacing one custom relationship property.
#[derive(Debug)]
pub struct RefPropDraft {
    pub name: TextField,
    pub label: TextField,
    pub kind: RefPropType,
    pub editing_index: Option<usize>,
    pub focused: RefPropFocus,
    pub error: Option<String>,
}

impl RefPropDraft {
    /// Starts an empty custom-property draft.
    pub fn new_add() -> Self {
        Self {
            name: TextField::single_line("Name"),
            label: TextField::single_line("Label"),
            kind: RefPropType::String,
            editing_index: None,
            focused: RefPropFocus::Name,
            error: None,
        }
    }

    /// Seeds a draft that will replace the custom property at `index`.
    pub fn edit(index: usize, property: &RefProperty) -> Self {
        Self {
            name: TextField::single_line("Name").with_initial(property.name.clone()),
            label: TextField::single_line("Label").with_initial(property.label.clone()),
            kind: property.kind,
            editing_index: Some(index),
            focused: RefPropFocus::Name,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct RelationshipFormState {
    pub tenant_name: String,
    pub source_object: String,
    pub original_doc: Value,
    pub previous: Option<PreviousRelationship>,
    pub key: TextField,
    pub title: TextField,
    pub description: TextField,
    pub target_name: Option<String>,
    pub target_query: LineEditor,
    pub target_selected: usize,
    pub forward: Cardinality,
    pub reverse: ReverseCardinality,
    pub reverse_key: TextField,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub required: bool,
    pub validate: bool,
    pub ref_properties: Vec<RefProperty>,
    pub ref_selected: usize,
    pub focused: RelationshipFocus,
    pub error: Option<String>,
}

impl RelationshipFormState {
    pub fn new_create(tenant_name: String, source_object: String, original_doc: Value) -> Self {
        Self {
            tenant_name,
            source_object,
            original_doc,
            previous: None,
            key: TextField::single_line("Key"),
            title: TextField::single_line("Title"),
            description: TextField::textarea("Description"),
            target_name: None,
            target_query: LineEditor::new(),
            target_selected: 0,
            forward: Cardinality::One,
            reverse: ReverseCardinality::None,
            reverse_key: TextField::single_line("Reverse key"),
            searchable: false,
            viewable: true,
            user_editable: true,
            required: false,
            validate: true,
            ref_properties: Vec::new(),
            ref_selected: 0,
            focused: RelationshipFocus::Key,
            error: None,
        }
    }
    pub fn edit(
        tenant_name: String,
        source_object: String,
        original_doc: Value,
        key: String,
        property: Value,
    ) -> Option<Self> {
        let parsed = crate::managed::ops::parse_relationship(&property)?;
        // A `reversePropertyName` naming a property the target does not have is
        // `Dangling`, not `One`: seeding a cardinality made an otherwise
        // untouched save materialise the property on a second object, and
        // seeding `None` made the same save strip the source's claim. Neither
        // is a decision the operator asked for, so carry the tenant's actual
        // state and let them choose.
        let reverse = match &parsed.reverse_key {
            None => ReverseCardinality::None,
            Some(reverse_key) => crate::managed::api::object_named(&original_doc, &parsed.target)
                .ok()
                .and_then(properties)
                .and_then(|props| props.get(reverse_key))
                .map_or(ReverseCardinality::Dangling, |property| {
                    if property.get("type").and_then(Value::as_str) == Some("array") {
                        ReverseCardinality::Many
                    } else {
                        ReverseCardinality::One
                    }
                }),
        };
        let required = crate::managed::api::object_named(&original_doc, &source_object)
            .ok()
            .is_some_and(|object| required_fields(object).contains(&key));
        Some(Self {
            tenant_name,
            source_object,
            original_doc,
            previous: Some(PreviousRelationship {
                old_key: key.clone(),
                old_target: parsed.target.clone(),
                old_reverse_key: parsed.reverse_key.clone(),
            }),
            key: TextField::single_line("Key").with_initial(key),
            title: TextField::single_line("Title").with_initial(
                property
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            description: TextField::textarea("Description").with_initial(
                property
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            target_name: Some(parsed.target),
            target_query: LineEditor::new(),
            target_selected: 0,
            forward: parsed.forward,
            reverse,
            reverse_key: TextField::single_line("Reverse key")
                .with_initial(parsed.reverse_key.unwrap_or_default()),
            searchable: parsed.searchable,
            viewable: parsed.viewable,
            user_editable: parsed.user_editable,
            required,
            validate: parsed.validate,
            ref_properties: crate::managed::ops::parse_ref_properties(&property),
            ref_selected: 0,
            focused: RelationshipFocus::Key,
            error: None,
        })
    }

    /// Removes the selected custom relationship property and keeps selection in range.
    pub fn remove_selected_ref_property(&mut self) {
        if self.ref_selected < self.ref_properties.len() {
            self.ref_properties.remove(self.ref_selected);
        }
        self.ref_selected = self
            .ref_selected
            .min(self.ref_properties.len().saturating_sub(1));
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

#[derive(Debug, Clone)]
pub struct DeleteObjectState {
    pub tenant_name: String,
    pub object_name: String,
    pub original_doc: Value,
    /// (object, property key) pairs whose relationship targets this object and
    /// will be stripped by the delete.
    pub inbound: Vec<(String, String)>,
    /// Background record count: None = still counting.
    pub record_count: Option<Result<crate::managed::api::RecordCount, String>>,
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

fn scalar_type_for_property(property: &Value) -> ScalarFieldType {
    let kinds = match property.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if kinds.contains(&"boolean") {
        ScalarFieldType::Boolean
    } else if kinds.contains(&"number") || kinds.contains(&"integer") {
        ScalarFieldType::Number
    } else if kinds.contains(&"array")
        && property.pointer("/items/type").and_then(Value::as_str) == Some("string")
    {
        ScalarFieldType::StringArray
    } else {
        ScalarFieldType::String
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
    pub relationship_form: Option<RelationshipFormState>,
    pub ref_prop_draft: Option<RefPropDraft>,
    pub add_choose: Option<AddChooseState>,
    pub add_hook: Option<AddHookState>,
    pub pending_delete: Option<DeleteFieldState>,
    pub pending_object_delete: Option<DeleteObjectState>,
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
        self.relationship_form = None;
        self.ref_prop_draft = None;
        self.add_choose = None;
        self.add_hook = None;
        self.pending_delete = None;
        self.pending_object_delete = None;
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
        let standard = json!({"name": "alpha_user", "type": "Managed Object", "meta": {}, "schema": {"properties": {}}});
        let custom = json!({"name": "alpha_lock", "schema": {"properties": {}}});

        assert!(is_standard_object(&standard));
        assert!(is_custom_object(&custom));
        assert!(!is_custom_field(&standard, "givenName"));
        assert!(is_custom_field(&standard, "custom_pet"));
        assert!(is_custom_field(&custom, "lockKey"));
    }

    /// Only `*_user` carries `meta`; role/organization/assignment/application
    /// carry `type` alone and accept ordinary un-prefixed properties (confirmed
    /// against `alpha_organization`, 2026-08-07). Their fields are therefore
    /// fully editable and need no `custom_` prefix — the object itself is still
    /// protected, which is [`is_ping_shipped_object`]'s job, not this one's.
    #[test]
    fn ping_objects_without_meta_get_custom_field_freedom_but_stay_unrenamable() {
        let role =
            json!({"name": "alpha_role", "type": "Managed Object", "schema": {"properties": {}}});

        assert_eq!(object_class(&role), ObjectClass::Custom);
        assert_eq!(normalize_new_property_key(&role, "code").unwrap(), "code");

        // Accepted trade: this includes Ping's own shipped fields.
        let caps = field_capability(&role, "name");
        assert_eq!(caps.tier, FieldTier::FieldOnCustomObject);
        assert!(caps.delete && caps.rename_key && caps.change_type);

        // The object may not be renamed or deleted regardless.
        assert!(is_ping_shipped_object(&role));
    }

    /// The `*_user` pair is the one place Ping constrains additions, so it
    /// keeps the forced prefix and the shipped/custom field split.
    #[test]
    fn user_objects_keep_the_forced_prefix_and_protected_shipped_fields() {
        let user = json!({"name": "alpha_user", "type": "Managed Object", "meta": {}, "schema": {"properties": {}}});

        assert_eq!(object_class(&user), ObjectClass::Standard);
        assert_eq!(
            normalize_new_property_key(&user, "code").unwrap(),
            "custom_code"
        );

        let caps = field_capability(&user, "givenName");
        assert_eq!(caps.tier, FieldTier::StandardFieldOnStandardObject);
        assert!(!caps.delete && !caps.rename_key && !caps.change_type);

        let caps = field_capability(&user, "custom_pet");
        assert_eq!(caps.tier, FieldTier::CustomFieldOnStandardObject);
        assert!(caps.delete && caps.rename_key && caps.change_type);
    }

    #[test]
    fn field_capability_matches_tiers() {
        let standard = json!({"type": "Managed Object", "meta": {}});
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

        let standard_object = json!({"type": "Managed Object", "meta": {}});
        let caps = field_capability_for_property(&standard_object, "manager", &relationship);
        assert_eq!(caps.tier, FieldTier::StandardFieldOnStandardObject);
        assert!(!caps.rename_key);
        assert!(!caps.delete);
    }

    #[test]
    fn normalize_property_key_rejects_invalid_characters() {
        let standard = json!({"name": "alpha_user", "type": "Managed Object", "meta": {}});
        let custom = json!({"name": "alpha_lock"});

        assert!(normalize_new_property_key(&standard, "my field").is_err());
        assert!(normalize_new_property_key(&custom, "1field").is_err());
        assert!(normalize_new_property_key(&custom, "field-name").is_err());
    }

    #[test]
    fn normalize_property_key_preserves_prefix_rules() {
        let standard = json!({"name": "alpha_user", "type": "Managed Object", "meta": {}});
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
    fn ping_shipped_guard_uses_verified_type_value_without_meta() {
        assert!(is_ping_shipped_object(&json!({"type": "Managed Object"})));
        assert!(!is_ping_shipped_object(&json!({"type": "custom"})));
        assert!(!is_ping_shipped_object(&json!({"name": "test_object"})));
    }

    #[test]
    fn unrelated_top_level_type_does_not_force_custom_property_prefix() {
        let custom = json!({"name": "test_object", "type": "custom"});

        assert_eq!(object_class(&custom), ObjectClass::Custom);
        assert_eq!(normalize_new_property_key(&custom, "code").unwrap(), "code");
        let draft = AddFieldState::new("sandbox".into(), "test_object".into(), custom);
        assert_eq!(draft.key.value, "");
        assert_eq!(draft.key.locked_prefix, "");
    }

    /// Reverse cardinality is read off the target object, so the form has to be
    /// driven from a whole document, not from the source property alone.
    fn relationship_form_edit(target_reverse: Option<Value>) -> RelationshipFormState {
        let property = json!({"type": "array", "searchable": true, "viewable": false, "userEditable": false, "items": {"type": "relationship", "validate": true, "reverseRelationship": true, "reversePropertyName": "owners", "resourceCollection": [{"path": "managed/b"}], "properties": {"_refProperties": {"properties": {"_id": {"type": "string"}}}}}});
        let mut target_properties = serde_json::Map::new();
        if let Some(reverse) = target_reverse {
            target_properties.insert("owners".into(), reverse);
        }
        let doc = json!({"objects": [
            {"name": "a", "schema": {"properties": {"owner": property.clone()}, "required": ["owner"]}},
            {"name": "b", "schema": {"properties": target_properties}},
        ]});
        RelationshipFormState::edit("sandbox".into(), "a".into(), doc, "owner".into(), property)
            .unwrap()
    }

    #[test]
    fn relationship_form_edit_reads_reverse_cardinality_off_the_target() {
        for (target_reverse, expected) in [
            (json!({"type": "relationship"}), ReverseCardinality::One),
            (
                json!({"type": "array", "items": {"type": "relationship"}}),
                ReverseCardinality::Many,
            ),
        ] {
            assert_eq!(
                relationship_form_edit(Some(target_reverse)).reverse,
                expected
            );
        }

        let form = relationship_form_edit(Some(json!({"type": "relationship"})));
        assert_eq!(form.forward, Cardinality::Many);
        assert_eq!(form.target_name.as_deref(), Some("b"));
        assert!(form.searchable);
        assert!(form.required);
        assert_eq!(
            form.previous.unwrap().old_reverse_key.as_deref(),
            Some("owners")
        );
    }

    #[test]
    fn relationship_form_edit_reports_a_dangling_reverse_as_its_own_state() {
        // The stock `alpha_application.members` names `alpha_user.applications`,
        // which does not exist. `One` made an untouched save create it on the
        // target; `None` made the same save strip the source's claim. Neither is
        // the operator's decision, so the form carries the inherited state.
        let form = relationship_form_edit(None);

        assert_eq!(form.reverse, ReverseCardinality::Dangling);
        assert_eq!(form.reverse_key.value, "owners");
        assert_eq!(
            form.previous.unwrap().old_reverse_key.as_deref(),
            Some("owners")
        );
    }

    #[test]
    fn cycling_leaves_dangling_and_never_offers_it_back() {
        // Authoring a fresh dangling declaration is not a thing to offer; the
        // state only exists because a tenant already has one.
        assert_eq!(
            ReverseCardinality::Dangling.next(),
            ReverseCardinality::None
        );
        assert_eq!(
            ReverseCardinality::Dangling.prev(),
            ReverseCardinality::Many
        );
        let mut seen = ReverseCardinality::Dangling;
        for _ in 0..8 {
            seen = seen.next();
            assert_ne!(seen, ReverseCardinality::Dangling);
        }
        assert!(!ReverseCardinality::Dangling.writes_target_property());
        assert!(!ReverseCardinality::None.writes_target_property());
        assert!(ReverseCardinality::One.writes_target_property());
        assert!(ReverseCardinality::Many.writes_target_property());
    }

    #[test]
    fn removing_selected_ref_property_clamps_selection() {
        let mut form =
            RelationshipFormState::new_create("sandbox".into(), "a".into(), json!({"objects": []}));
        form.ref_properties = vec![
            RefProperty {
                name: "first".into(),
                label: "First".into(),
                kind: RefPropType::String,
            },
            RefProperty {
                name: "second".into(),
                label: "Second".into(),
                kind: RefPropType::Number,
            },
        ];
        form.ref_selected = 1;

        form.remove_selected_ref_property();

        assert_eq!(form.ref_properties.len(), 1);
        assert_eq!(form.ref_properties[0].name, "first");
        assert_eq!(form.ref_selected, 0);
    }

    #[test]
    fn enum_draft_parsing_uses_the_shared_item_grammar() {
        let mut draft = AddFieldState::new("sandbox".into(), "thing".into(), json!({}));
        draft.enum_values.value = " new:New, done:All done: now, ".into();

        let values = draft.parsed_enum_values().unwrap().unwrap();
        assert_eq!(values.to_items(), ["new:New", "done:All done: now"]);
    }

    #[test]
    fn enum_edit_seed_distinguishes_unchanged_clear_and_set() {
        let object = json!({"name": "thing", "schema": {"properties": {}}});
        let property = json!({
            "type": "string",
            "enum": ["new", "done"],
            "options": {"enum_titles": ["New", "Done"]}
        });
        let mut edit = FieldEditState::from_property(
            "sandbox".into(),
            "thing".into(),
            "status".into(),
            object,
            property,
            false,
        );

        assert!(matches!(edit.enum_change().unwrap(), EnumChange::Unchanged));
        edit.enum_values.value.clear();
        assert!(matches!(edit.enum_change().unwrap(), EnumChange::Clear));
        edit.enum_values.value = "new:Brand new, in_progress:In progress".into();
        assert_eq!(
            edit.enum_change().unwrap(),
            EnumChange::Set(EnumSpec {
                values: vec![
                    crate::managed::spec::EnumValue {
                        value: "new".into(),
                        title: Some("Brand new".into()),
                    },
                    crate::managed::spec::EnumValue {
                        value: "in_progress".into(),
                        title: Some("In progress".into()),
                    },
                ],
            })
        );
    }

    #[test]
    fn default_edit_seed_distinguishes_unchanged_clear_and_set() {
        let object = json!({"name": "thing", "schema": {"properties": {}}});
        let property = json!({"type": "string", "default": "saved"});
        let mut edit = FieldEditState::from_property(
            "sandbox".into(),
            "thing".into(),
            "status".into(),
            object,
            property,
            false,
        );

        assert_eq!(edit.default_change(), DefaultChange::Unchanged);
        edit.default_value.set("");
        assert_eq!(edit.default_change(), DefaultChange::Clear);
        edit.default_value.set("replacement");
        assert_eq!(
            edit.default_change(),
            DefaultChange::Set("replacement".into())
        );
    }

    #[test]
    fn blank_add_field_default_is_absent() {
        let mut draft = AddFieldState::new("sandbox".into(), "thing".into(), json!({}));
        assert_eq!(draft.parsed_default(), None);
        draft.default_value.set("  \t ");
        assert_eq!(draft.parsed_default(), None);
    }

    /// Retyping the field has to re-shape the default row with it. If the two
    /// drift, a `boolean` default gets validated as free text, and a
    /// type-mismatched default is exactly what bricks the managed object.
    #[test]
    fn retyping_the_field_reshapes_the_default() {
        use crate::tui::widgets::ValueShape;

        let mut draft = AddFieldState::new("sandbox".into(), "thing".into(), json!({}));
        draft.default_value.set("banana");
        assert_eq!(draft.default_value.shape(), ValueShape::Text);
        assert_eq!(draft.default_value.error(), None);

        draft.set_field_type(ScalarFieldType::Boolean);
        assert_eq!(draft.default_value.shape(), ValueShape::Bool);
        assert!(draft.default_value.error().is_some());

        draft.default_value.set("true");
        assert_eq!(draft.default_value.error(), None);
    }

    #[test]
    fn untouched_default_survives_an_edit_form_save() {
        let object = json!({
            "name": "thing",
            "schema": {
                "properties": {"status": {"type": "string", "default": "saved"}},
                "required": [],
                "order": ["status"]
            }
        });
        let property = object["schema"]["properties"]["status"].clone();
        let edit = FieldEditState::from_property(
            "sandbox".into(),
            "thing".into(),
            "status".into(),
            object.clone(),
            property,
            false,
        );

        let applied =
            crate::managed::ops::apply_field_edit(&object, "status", &edit.edit_spec().unwrap())
                .unwrap();
        // Opening and saving must not drop an existing default the user left alone.
        assert_eq!(
            applied.object["schema"]["properties"]["status"]["default"],
            json!("saved")
        );
    }

    #[test]
    fn enum_focus_sits_after_the_row_that_decides_its_type() {
        let caps = FieldCaps {
            tier: FieldTier::FieldOnCustomObject,
            attrs: FieldEditAttrs::Full,
            change_type: true,
            rename_key: true,
            delete: true,
        };
        assert_eq!(
            EditFieldFocus::Description.next(caps, true),
            EditFieldFocus::Enum
        );
        assert_eq!(
            EditFieldFocus::Enum.prev(caps, true),
            EditFieldFocus::Description
        );
        assert_eq!(
            EditFieldFocus::Enum.next(caps, true),
            EditFieldFocus::Default
        );
        // On the add form the type is still being chosen, and choosing a
        // boolean removes this row — so it follows Type rather than preceding
        // it.
        assert_eq!(AddFieldFocus::Type.next(true), AddFieldFocus::Enum);
        assert_eq!(AddFieldFocus::Enum.prev(true), AddFieldFocus::Type);
        assert_eq!(AddFieldFocus::Enum.next(true), AddFieldFocus::Default);
        assert_eq!(AddFieldFocus::Type.next(false), AddFieldFocus::Default);
    }
}
