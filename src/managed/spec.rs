//! Plain input specifications for managed-object field transforms.
//!
//! These types deliberately contain no TUI state so the same schema changes
//! can be performed by both the interactive editor and command-line callers.

/// Scalar property type supported by the managed field creator.
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
            Self::String => Self::Boolean,
            Self::Boolean => Self::Number,
            Self::Number => Self::StringArray,
            Self::StringArray => Self::String,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::String => Self::StringArray,
            Self::Boolean => Self::String,
            Self::Number => Self::Boolean,
            Self::StringArray => Self::Number,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::StringArray => "string[]",
        }
    }
}

/// Requested changes to one managed property.
///
/// Each optional attribute is left untouched when it is `None`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FieldEditSpec {
    pub new_key: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub searchable: Option<bool>,
    pub viewable: Option<bool>,
    pub user_editable: Option<bool>,
    pub enum_change: EnumChange,
    pub allow_narrowing: bool,
}

/// One allowed value and its optional display label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    pub value: String,
    pub title: Option<String>,
}

/// A property's allowed-value constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSpec {
    pub values: Vec<EnumValue>,
}

impl EnumSpec {
    /// Renders the constraint back into the item grammar.
    pub fn to_items(&self) -> Vec<String> {
        self.values
            .iter()
            .map(|item| match &item.title {
                Some(title) => format!("{}:{title}", item.value),
                None => item.value.clone(),
            })
            .collect()
    }
}

/// What an edit should do to a property's allowed-value constraint.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum EnumChange {
    /// Leave the constraint — or its absence — alone.
    #[default]
    Unchanged,
    /// Replace the constraint with this set.
    Set(EnumSpec),
    /// Drop the constraint. Widening, so always safe.
    Clear,
}

/// Parses `value` / `value:Title` items into a constraint.
pub fn parse_enum_items<S: AsRef<str>>(items: &[S]) -> Result<EnumSpec, String> {
    let mut values = Vec::new();
    for item in items {
        let item = item.as_ref().trim();
        if item.is_empty() {
            continue;
        }
        let (value, title) = match item.split_once(':') {
            Some((value, title)) => (value.trim(), Some(title.trim().to_string())),
            None => (item, None),
        };
        if value.is_empty() {
            return Err(format!("enum item '{item}' has an empty value"));
        }
        if values
            .iter()
            .any(|existing: &EnumValue| existing.value == value)
        {
            return Err(format!("duplicate enum value '{value}'"));
        }
        values.push(EnumValue {
            value: value.to_string(),
            title,
        });
    }
    if values.is_empty() {
        return Err("enum needs at least one value; use Clear to remove it".into());
    }
    Ok(EnumSpec { values })
}

/// Definition of a scalar managed property to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFieldSpec {
    pub key: String,
    pub field_type: ScalarFieldType,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub enum_values: Option<EnumSpec>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_items_parse_and_round_trip() {
        let parsed = parse_enum_items(&[" new ", "done:All done: now", " "]).unwrap();
        assert_eq!(parsed.values[0].value, "new");
        assert_eq!(parsed.values[1].title.as_deref(), Some("All done: now"));
        assert_eq!(parse_enum_items(&parsed.to_items()).unwrap(), parsed);
    }

    #[test]
    fn enum_items_reject_empty_values_duplicates_and_empty_lists() {
        assert!(
            parse_enum_items(&[":Title"])
                .unwrap_err()
                .contains("empty value")
        );
        assert!(
            parse_enum_items(&["new", "new"])
                .unwrap_err()
                .contains("new")
        );
        assert!(parse_enum_items(&["", " "]).is_err());
    }
}

/// Input for deleting a managed property.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeleteFieldSpec;

/// Requested replacement key for a managed property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameFieldSpec {
    pub new_key: String,
}
