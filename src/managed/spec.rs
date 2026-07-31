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
}

/// Input for deleting a managed property.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeleteFieldSpec;

/// Requested replacement key for a managed property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameFieldSpec {
    pub new_key: String,
}
