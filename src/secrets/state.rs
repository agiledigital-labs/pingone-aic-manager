//! Pure state and projections for ESV secrets.

use std::collections::{HashMap, HashSet};

use crate::app::App;
use crate::esv::state::{LoadState, id_of};
use crate::tui::list_state::TenantListState;
use crate::tui::widgets::TextField;

/// The four secret kinds the console offers map 1:1 to the API `encoding`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Generic,
    Pem,
    Base64Hmac,
    Base64Aes,
}

impl Encoding {
    pub const ALL: &'static [Encoding] = &[
        Encoding::Generic,
        Encoding::Pem,
        Encoding::Base64Hmac,
        Encoding::Base64Aes,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Generic => "generic",
            Encoding::Pem => "pem",
            Encoding::Base64Hmac => "base64hmac",
            Encoding::Base64Aes => "base64aes",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Generic => "Secret (generic)",
            Encoding::Pem => "PEM-encoded",
            Encoding::Base64Hmac => "Base64 HMAC key",
            Encoding::Base64Aes => "Base64 AES key",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|e| *e == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|e| *e == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Focusable rows in the create form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateField {
    Id,
    Description,
    Encoding,
    Placeholders,
    Json,
    Value,
    Save,
}

impl CreateField {
    const ORDER: &'static [CreateField] = &[
        CreateField::Id,
        CreateField::Description,
        CreateField::Encoding,
        CreateField::Placeholders,
        CreateField::Json,
        CreateField::Value,
        CreateField::Save,
    ];

    pub fn next(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

/// In-progress secret create form.
#[derive(Debug)]
pub struct CreateForm {
    pub id: TextField,
    pub description: TextField,
    pub encoding: Encoding,
    pub use_in_placeholders: bool,
    /// Generic-only: validate the value parses as JSON before submit.
    pub as_json: bool,
    pub value: TextField,
    pub focused: CreateField,
    pub error: Option<String>,
}

impl CreateForm {
    pub fn new() -> Self {
        Self {
            id: TextField::single_line("Secret ID (esv-…)").with_locked_prefix("esv-"),
            description: TextField::single_line("Description"),
            encoding: Encoding::Generic,
            use_in_placeholders: true,
            as_json: false,
            value: TextField::masked("Value"),
            focused: CreateField::Id,
            error: None,
        }
    }
}

/// In-progress "add a new version" form.
#[derive(Debug)]
pub struct AddVersionForm {
    pub tenant: String,
    pub id: String,
    pub encoding: Encoding,
    pub value: TextField,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreatePlan {
    pub tenant: String,
    pub id: String,
    pub encoding: String,
    pub use_in_placeholders: bool,
    pub value_b64: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct VersionAddPlan {
    pub tenant: String,
    pub id: String,
    pub value_b64: String,
}

/// A secret pending the y/n delete confirmation.
#[derive(Debug, Clone)]
pub struct DeletePlan {
    pub tenant: String,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct SetDescriptionPlan {
    pub tenant: String,
    pub id: String,
    pub description: String,
    pub previous: String,
}

/// Which mutation an operation result reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOpKind {
    Create,
    AddVersion,
    StatusChange,
    Destroy,
    Delete,
    SetDescription,
}

/// Which half of the open secret-detail pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailFocus {
    Versions,
    Description,
}

#[derive(Debug)]
pub struct State {
    /// Shared per-tenant list mechanics (data, pending ids, query, cursor).
    pub list: TenantListState,
    /// Versions of a secret, loaded on demand when the version panel opens.
    pub versions: HashMap<(String, String), LoadState>,
    pub version_selected: usize,
    /// Stable target for the open version panel.
    pub version_target: Option<(String, String)>,
    pub pending_version_destroy: Option<(String, String, String)>,
    pub detail_focus: DetailFocus,
    pub description: TextField,
    /// (tenant, id) for secrets with a mutation in flight.
    pub in_flight: HashSet<(String, String)>,
    pub create: Option<CreateForm>,
    pub add_version: Option<AddVersionForm>,
    pub pending_delete: Option<DeletePlan>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            list: TenantListState::new(),
            versions: HashMap::new(),
            version_selected: 0,
            version_target: None,
            pending_version_destroy: None,
            detail_focus: DetailFocus::Versions,
            description: TextField::single_line("Description"),
            in_flight: HashSet::new(),
            create: None,
            add_version: None,
            pending_delete: None,
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_view(&mut self) {
        self.list.query.clear();
        self.list.selected = 0;
        self.list.scroll = 0;
        self.version_target = None;
        self.version_selected = 0;
        self.pending_version_destroy = None;
        self.detail_focus = DetailFocus::Versions;
        self.description = TextField::single_line("Description");
    }
}

pub fn use_in_placeholders(v: &serde_json::Value) -> bool {
    v.get("useInPlaceholders")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
}

pub fn encoding_of(v: &serde_json::Value) -> &str {
    v.get("encoding").and_then(|x| x.as_str()).unwrap_or("?")
}

pub fn description_of(v: &serde_json::Value) -> &str {
    v.get("description").and_then(|x| x.as_str()).unwrap_or("")
}

pub fn pending_count(app: &App, tenant: &str) -> usize {
    app.secret
        .list
        .pending_ids
        .get(tenant)
        .map(|s| s.len())
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct SecretRow {
    pub idx: usize,
    pub id: String,
    pub encoding: String,
    pub use_in_placeholders: bool,
    pub pending: bool,
}

pub fn rows(app: &App, tenant: Option<&str>) -> Vec<SecretRow> {
    let Some(name) = tenant else {
        return Vec::new();
    };
    let Some(LoadState::Loaded(items)) = app.secret.list.data.get(name) else {
        return Vec::new();
    };
    let query = app.secret.list.query.value().to_lowercase();
    let pending = app.secret.list.pending_ids.get(name);
    let mut out: Vec<SecretRow> = items
        .iter()
        .enumerate()
        .filter(|(_, v)| query.is_empty() || id_of(v).to_lowercase().contains(&query))
        .map(|(idx, v)| {
            let id = id_of(v).to_string();
            SecretRow {
                idx,
                pending: pending.is_some_and(|s| s.contains(&id)),
                encoding: encoding_of(v).to_string(),
                use_in_placeholders: use_in_placeholders(v),
                id,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn selected_secret(app: &App) -> Option<serde_json::Value> {
    let tenant = app.active_tenant()?.name.clone();
    let rows = rows(app, Some(&tenant));
    let row = rows.get(app.secret.list.selected)?;
    let LoadState::Loaded(items) = app.secret.list.data.get(&tenant)? else {
        return None;
    };
    items.get(row.idx).cloned()
}

pub fn secret_in_cache(app: &App, tenant: &str, id: &str) -> Option<serde_json::Value> {
    match app.secret.list.data.get(tenant) {
        Some(LoadState::Loaded(items)) => items.iter().find(|v| id_of(v) == id).cloned(),
        _ => None,
    }
}

pub enum VersionsView {
    Loading,
    Failed(String),
    Loaded {
        tenant: String,
        id: String,
        versions: Vec<serde_json::Value>,
    },
}

pub fn versions_view(app: &App) -> Option<VersionsView> {
    let (tenant, id) = app.secret.version_target.clone()?;
    Some(
        match app.secret.versions.get(&(tenant.clone(), id.clone())) {
            Some(LoadState::Loaded(vs)) => VersionsView::Loaded {
                tenant,
                id,
                versions: vs.clone(),
            },
            Some(LoadState::Failed(e)) => VersionsView::Failed(e.clone()),
            Some(LoadState::Loading) | None => VersionsView::Loading,
        },
    )
}

pub(crate) fn encode_value(
    encoding: Encoding,
    value: &str,
    as_json: bool,
) -> std::result::Result<String, String> {
    crate::esv::api::encode_secret_value(encoding.as_str(), value, as_json)
}

pub(crate) fn version_num(v: &serde_json::Value) -> Option<String> {
    v.get("version").and_then(|x| {
        x.as_str()
            .map(|s| s.to_string())
            .or_else(|| x.as_u64().map(|n| n.to_string()))
    })
}

pub(crate) fn version_status(v: &serde_json::Value) -> &str {
    v.get("status").and_then(|x| x.as_str()).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    fn decode(b64: &str) -> Vec<u8> {
        B64.decode(b64).unwrap()
    }

    #[test]
    fn generic_encodes_raw_bytes() {
        let b64 = encode_value(Encoding::Generic, "hello", false).unwrap();
        assert_eq!(decode(&b64), b"hello");
    }

    #[test]
    fn generic_json_toggle_validates() {
        assert!(encode_value(Encoding::Generic, "{not json", true).is_err());
        assert!(encode_value(Encoding::Generic, "{\"k\":1}", true).is_ok());
        assert!(encode_value(Encoding::Generic, "{not json", false).is_ok());
    }

    #[test]
    fn empty_value_rejected() {
        assert!(encode_value(Encoding::Generic, "", false).is_err());
    }

    #[test]
    fn pem_requires_begin_block() {
        assert!(encode_value(Encoding::Pem, "not a pem", false).is_err());
        let pem = "-----BEGIN CERTIFICATE-----\nMII...\n-----END CERTIFICATE-----";
        let b64 = encode_value(Encoding::Pem, pem, false).unwrap();
        assert_eq!(decode(&b64), pem.as_bytes());
    }

    #[test]
    fn base64_key_is_double_encoded() {
        let key = B64.encode([7u8; 32]);
        let b64 = encode_value(Encoding::Base64Hmac, &key, false).unwrap();
        assert_eq!(decode(&b64), key.as_bytes());
        assert!(encode_value(Encoding::Base64Hmac, "@@not base64@@", false).is_err());
    }

    #[test]
    fn aes_key_length_validated() {
        let ok = B64.encode([0u8; 16]);
        assert!(encode_value(Encoding::Base64Aes, &ok, false).is_ok());
        let bad = B64.encode([0u8; 20]);
        assert!(encode_value(Encoding::Base64Aes, &bad, false).is_err());
    }
}
