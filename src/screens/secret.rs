//! ESV **secrets** sub-view of the ESVs tab.
//!
//! Secrets differ from variables enough to warrant their own screen module
//! (see `docs/api/03-esvs.md`, verified 2026-05-30):
//!   * `PUT` is create-only; values change by adding a **version**.
//!   * Values are write-only — never displayed, only set.
//!   * `encoding` + `useInPlaceholders` are required at create and immutable.
//!   * Versions carry status (ENABLED/DISABLED/DESTROYED); the latest can't be
//!     disabled and DESTROYED is one-way.
//!   * `useInPlaceholders:false` secrets load immediately (no restart).
//!
//! The list + pending state come through the shared ESV poll
//! (`esv::apply_refresh` calls [`apply_refresh`] here). All mutations run as
//! background tasks and trigger a re-poll on completion rather than merging
//! optimistically — secret metadata is cheap to refetch and avoids the
//! write-only-value bookkeeping that optimism would require.

use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::event::{AppEvent, ToastKind};
use crate::screens::esv::{LoadState, id_of};
use crate::screens::list_state::TenantListState;
use crate::screens::prod_confirm::PendingProdAction;
use crate::ui::widgets::TextField;
use crate::undo::{Capability, ConflictCheck, Sensitivity, UndoEntry, UndoOp};

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
    /// Generic-only: validate the value parses as JSON before submit (mirrors
    /// the console's JSON toggle). Ignored for the other encodings.
    pub as_json: bool,
    pub value: TextField,
    pub focused: CreateField,
    pub error: Option<String>,
}

impl CreateForm {
    fn new() -> Self {
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

/// In-progress "add a new version" form (just a value).
#[derive(Debug)]
pub struct AddVersionForm {
    pub tenant: String,
    pub id: String,
    pub encoding: Encoding,
    pub value: TextField,
    pub error: Option<String>,
}

/// A secret pending the y/n delete confirmation.
#[derive(Debug, Clone)]
pub struct DeletePlan {
    pub tenant: String,
    pub id: String,
}

/// Which mutation a `SecretOpResult` reports back, so the completion handler
/// records the right undo entry only after the op actually succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOpKind {
    Create,
    AddVersion,
    StatusChange,
    Destroy,
    Delete,
    SetDescription,
}

/// Which half of the open secret-detail pane has keyboard focus. The pane shows
/// the read-only metadata, an editable description, and the version list; `Tab`
/// toggles between editing the description and navigating the versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailFocus {
    Versions,
    Description,
}

#[derive(Debug)]
pub struct State {
    /// Shared per-tenant list mechanics (data, pending ids, query, cursor).
    /// `pending_ids` here is authoritative for which secrets gate a restart
    /// (only `useInPlaceholders:true` secrets ever appear).
    pub list: TenantListState,

    /// Versions of a secret, loaded on demand when the version panel opens.
    pub versions: HashMap<(String, String), LoadState>,
    pub version_selected: usize,
    /// The (tenant, id) the open version panel targets. Stored so a background
    /// refresh that re-sorts/re-selects the list can't make the panel display
    /// or mutate a different secret than the one it was opened on.
    pub version_target: Option<(String, String)>,
    /// (tenant, id, version) awaiting the irreversible-destroy confirmation.
    pub pending_version_destroy: Option<(String, String, String)>,

    /// Which half of the open detail pane has focus (description vs versions).
    pub detail_focus: DetailFocus,
    /// Edit buffer for the open secret's description. Initialised from the
    /// secret when the panel opens; committed via `setDescription`.
    pub description: TextField,

    /// (tenant, id) for secrets with a mutation in flight — gates re-issuing.
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

    /// Drop view state (filter, selection, open version panel) on tenant
    /// switch — the data behind it changed.
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

/// `useInPlaceholders` of a secret object (default false if absent).
pub fn use_in_placeholders(v: &serde_json::Value) -> bool {
    v.get("useInPlaceholders")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
}

/// `encoding` of a secret object.
pub fn encoding_of(v: &serde_json::Value) -> &str {
    v.get("encoding").and_then(|x| x.as_str()).unwrap_or("?")
}

/// `description` of a secret object (empty if absent).
pub fn description_of(v: &serde_json::Value) -> &str {
    v.get("description").and_then(|x| x.as_str()).unwrap_or("")
}

/// Number of secrets gating a restart for `tenant` — the `?_onlyPending=true`
/// set. Folded into the ESV tab's overall pending count.
pub fn pending_count(app: &App, tenant: &str) -> usize {
    app.secret
        .list
        .pending_ids
        .get(tenant)
        .map(|s| s.len())
        .unwrap_or(0)
}

/// One row in the rendered secrets list.
#[derive(Debug, Clone)]
pub struct SecretRow {
    pub idx: usize,
    pub id: String,
    pub encoding: String,
    pub use_in_placeholders: bool,
    pub pending: bool,
}

/// The filtered + sorted secret rows for `tenant`. Simple case-insensitive
/// substring filter on the id (secrets have no fuzzy haystack tags yet).
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
            let is_pending = pending.is_some_and(|s| s.contains(&id));
            SecretRow {
                idx,
                id,
                encoding: encoding_of(v).to_string(),
                use_in_placeholders: use_in_placeholders(v),
                pending: is_pending,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// The currently-selected secret object, if any.
pub fn selected_secret(app: &App) -> Option<serde_json::Value> {
    let tenant = app.active_tenant()?.name.clone();
    let rows = rows(app, Some(&tenant));
    let row = rows.get(app.secret.list.selected)?;
    let LoadState::Loaded(items) = app.secret.list.data.get(&tenant)? else {
        return None;
    };
    items.get(row.idx).cloned()
}

// --- Refresh integration (called from esv::apply_refresh) ----------------

pub fn apply_refresh(
    app: &mut App,
    tenant: &str,
    secrets: &std::result::Result<Vec<serde_json::Value>, String>,
    pending: &std::result::Result<Vec<serde_json::Value>, String>,
) {
    match secrets {
        Ok(vs) => {
            app.secret
                .list
                .data
                .insert(tenant.to_string(), LoadState::Loaded(vs.clone()));
            let n = rows(app, Some(tenant)).len();
            if app.secret.list.selected >= n {
                app.secret.list.selected = n.saturating_sub(1);
            }
        }
        Err(e) => {
            if !matches!(app.secret.list.data.get(tenant), Some(LoadState::Loaded(_))) {
                app.secret
                    .list
                    .data
                    .insert(tenant.to_string(), LoadState::Failed(e.clone()));
            } else {
                tracing::warn!("secret refresh failed for {tenant}: {e}");
            }
        }
    }
    if let Ok(vs) = pending {
        app.secret.list.pending_ids.insert(
            tenant.to_string(),
            vs.iter().map(|v| id_of(v).to_string()).collect(),
        );
    }
}

// --- Normal-mode keys (secrets view) -------------------------------------

// --- Create form ----------------------------------------------------------

/// Open the create-secret form. Entry point for the keymap (`^N` in the
/// secrets view).
pub fn start_create(app: &mut App) {
    app.secret.create = Some(CreateForm::new());
    app.input_mode = InputMode::SecretCreate;
}

pub fn handle_create_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(form) = app.secret.create.as_mut() else {
        return Ok(());
    };
    // Form-level keys, mirroring the variables edit form: Tab/Shift-Tab move
    // focus, Enter advances (or commits on Value/Save), Esc cancels.
    let focused = form.focused;
    match key.code {
        KeyCode::Esc => {
            app.secret.create = None;
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
        KeyCode::Tab => {
            form.focused = focused.next();
            return Ok(());
        }
        KeyCode::BackTab => {
            form.focused = focused.prev();
            return Ok(());
        }
        KeyCode::Enter => {
            match focused {
                CreateField::Value | CreateField::Save => commit_create(app),
                _ => form.focused = focused.next(),
            }
            return Ok(());
        }
        // ←/→ cycle the chip / toggle rows; on text fields they fall through
        // to cursor movement below.
        KeyCode::Left | KeyCode::Right => {
            let left = key.code == KeyCode::Left;
            match focused {
                CreateField::Encoding => {
                    form.encoding = if left { form.encoding.prev() } else { form.encoding.next() };
                    return Ok(());
                }
                CreateField::Placeholders => {
                    form.use_in_placeholders = !form.use_in_placeholders;
                    return Ok(());
                }
                CreateField::Json => {
                    form.as_json = !form.as_json;
                    return Ok(());
                }
                _ => {}
            }
        }
        _ => {}
    }

    form.error = None;
    match focused {
        CreateField::Id => {
            form.id.handle_key(&key);
        }
        CreateField::Description => {
            form.description.handle_key(&key);
        }
        CreateField::Value => {
            form.value.handle_key(&key);
        }
        // Space also toggles the boolean rows (in addition to ←/→ above).
        CreateField::Placeholders if key.code == KeyCode::Char(' ') => {
            form.use_in_placeholders = !form.use_in_placeholders;
        }
        CreateField::Json if key.code == KeyCode::Char(' ') => {
            form.as_json = !form.as_json;
        }
        _ => {}
    }
    Ok(())
}

fn commit_create(app: &mut App) {
    let Some(form) = app.secret.create.as_ref() else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    let id = form.id.trimmed().to_string();
    let encoding = form.encoding;
    let use_in_placeholders = form.use_in_placeholders;
    let description = form.description.value.clone();

    // Validate id. The `esv-` prefix is locked in the field, so the only id
    // problem left is an empty suffix.
    if id == "esv-" || id.is_empty() {
        set_create_error(app, "Give the secret a name after 'esv-'");
        return;
    }
    if !id.starts_with("esv-") {
        set_create_error(app, "Secret ID must start with 'esv-'");
        return;
    }
    if app
        .secret
        .list
        .data
        .get(&tenant)
        .and_then(|s| match s {
            LoadState::Loaded(items) => Some(items.iter().any(|v| id_of(v) == id)),
            _ => None,
        })
        .unwrap_or(false)
    {
        set_create_error(app, "A secret with that ID already exists (PUT is create-only)");
        return;
    }

    // Validate + encode the value.
    let value_b64 = match encode_value(encoding, &form.value.value, form.as_json) {
        Ok(v) => v,
        Err(e) => {
            set_create_error(app, &e);
            return;
        }
    };

    let plan = CreatePlan {
        tenant,
        id,
        encoding: encoding.as_str().to_string(),
        use_in_placeholders,
        value_b64,
        description,
    };
    app.secret.create = None;

    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretCreate(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_create(app, plan, false);
    }
}

fn set_create_error(app: &mut App, msg: &str) {
    if let Some(form) = app.secret.create.as_mut() {
        form.error = Some(msg.to_string());
    }
}

/// Turn the on-screen value into the wire `valueBase64`. Thin wrapper over the
/// shared [`crate::aic::esv::encode_secret_value`] (keyed by encoding string),
/// so the TUI and CLI validate identically.
fn encode_value(encoding: Encoding, value: &str, as_json: bool) -> std::result::Result<String, String> {
    crate::aic::esv::encode_secret_value(encoding.as_str(), value, as_json)
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

pub fn execute_create(app: &mut App, plan: CreatePlan, confirmed_prod: bool) {
    let key = (plan.tenant.clone(), plan.id.clone());
    app.secret.in_flight.insert(key);
    app.input_mode = InputMode::Normal;

    // The undo entry is recorded *after* the create succeeds (see
    // `apply_op_result`) so a failed create can't leave a `^Z` that deletes a
    // pre-existing secret of the same id.
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::create_secret(
            &plan.tenant,
            &plan.id,
            &plan.encoding,
            plan.use_in_placeholders,
            &plan.value_b64,
            &plan.description,
            confirmed_prod,
        )
        .await
        .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::Create,
            label: "Created secret".to_string(),
            reload_versions: false,
            result,
        });
    });
}

// --- Add version ----------------------------------------------------------

fn open_add_version(app: &mut App) {
    // Resolve from the open panel's target, not the live list selection.
    let Some((tenant, id)) = app.secret.version_target.clone() else {
        return;
    };
    let encoding = secret_in_cache(app, &tenant, &id)
        .as_ref()
        .map(|s| match encoding_of(s) {
            "pem" => Encoding::Pem,
            "base64hmac" => Encoding::Base64Hmac,
            "base64aes" => Encoding::Base64Aes,
            _ => Encoding::Generic,
        })
        .unwrap_or(Encoding::Generic);
    app.secret.add_version = Some(AddVersionForm {
        tenant,
        id,
        encoding,
        value: TextField::masked("New version value"),
        error: None,
    });
    app.input_mode = InputMode::SecretAddVersion;
}

/// Look up a secret object in the per-tenant cache by id.
pub fn secret_in_cache(app: &App, tenant: &str, id: &str) -> Option<serde_json::Value> {
    match app.secret.list.data.get(tenant) {
        Some(LoadState::Loaded(items)) => items.iter().find(|v| id_of(v) == id).cloned(),
        _ => None,
    }
}

pub fn handle_add_version_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(form) = app.secret.add_version.as_mut() else {
        return Ok(());
    };
    match key.code {
        KeyCode::Esc => {
            app.secret.add_version = None;
            app.input_mode = InputMode::SecretVersions;
        }
        KeyCode::Enter => {
            commit_add_version(app);
        }
        _ => {
            form.error = None;
            form.value.handle_key(&key);
        }
    }
    Ok(())
}

fn commit_add_version(app: &mut App) {
    let Some(form) = app.secret.add_version.as_ref() else {
        return;
    };
    let value_b64 = match encode_value(form.encoding, &form.value.value, false) {
        Ok(v) => v,
        Err(e) => {
            if let Some(f) = app.secret.add_version.as_mut() {
                f.error = Some(e);
            }
            return;
        }
    };
    let tenant = form.tenant.clone();
    let id = form.id.clone();
    app.secret.add_version = None;

    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    let plan = VersionAddPlan {
        tenant,
        id,
        value_b64,
    };
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretAddVersion(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_add_version(app, plan, false);
    }
}

#[derive(Debug, Clone)]
pub struct VersionAddPlan {
    pub tenant: String,
    pub id: String,
    pub value_b64: String,
}

pub fn execute_add_version(app: &mut App, plan: VersionAddPlan, confirmed_prod: bool) {
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    app.input_mode = InputMode::SecretVersions;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::create_secret_version(
            &plan.tenant,
            &plan.id,
            &plan.value_b64,
            confirmed_prod,
        )
        .await
        .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::AddVersion,
            label: "Added secret version".to_string(),
            reload_versions: true,
            result,
        });
    });
}

// --- Version panel --------------------------------------------------------

/// Whether the version panel is currently showing in the detail pane. True
/// across the version-list mode and the two states layered on top of it
/// (add-version form, destroy confirmation), so the pane keeps the versions
/// visible underneath those overlays.
pub fn versions_panel_open(app: &App) -> bool {
    matches!(
        app.input_mode,
        InputMode::SecretVersions
            | InputMode::SecretAddVersion
            | InputMode::SecretVersionDestroyConfirm
    )
}

pub fn open_versions(app: &mut App) {
    let Some(secret) = selected_secret(app) else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    let id = id_of(&secret).to_string();
    app.secret.version_selected = 0;
    app.secret.version_target = Some((tenant.clone(), id.clone()));
    app.secret.detail_focus = DetailFocus::Versions;
    app.secret.description =
        TextField::single_line("Description").with_initial(description_of(&secret));
    app.secret
        .versions
        .insert((tenant.clone(), id.clone()), LoadState::Loading);
    app.input_mode = InputMode::SecretVersions;
    reload_versions(app, tenant, id);
}

fn reload_versions(app: &mut App, tenant: String, id: String) {
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::list_secret_versions(&tenant, &id)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretVersionsListed { tenant, id, result });
    });
}

/// Snapshot of the open version panel, resolved from the stored target (not
/// the live list selection) so a background refresh can't swap the subject.
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
    Some(match app.secret.versions.get(&(tenant.clone(), id.clone())) {
        Some(LoadState::Loaded(vs)) => VersionsView::Loaded {
            tenant,
            id,
            versions: vs.clone(),
        },
        Some(LoadState::Failed(e)) => VersionsView::Failed(e.clone()),
        Some(LoadState::Loading) | None => VersionsView::Loading,
    })
}

pub fn handle_versions_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    // Resolve the panel's subject + versions from the stored target.
    let (tenant, id, versions) = match versions_view(app) {
        Some(VersionsView::Loaded {
            tenant,
            id,
            versions,
        }) => (tenant, id, versions),
        // Loading / failed: only navigation-cancel is meaningful.
        Some(_) => {
            if matches!(key.code, KeyCode::Esc) {
                app.input_mode = InputMode::Normal;
            }
            return Ok(());
        }
        None => {
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
    };
    // Tab toggles focus between the description editor and the version list;
    // Esc always closes the panel.
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.secret.detail_focus = match app.secret.detail_focus {
                DetailFocus::Versions => DetailFocus::Description,
                DetailFocus::Description => DetailFocus::Versions,
            };
            return Ok(());
        }
        _ => {}
    }

    // Description focused: keystrokes edit the buffer; Enter commits it.
    if app.secret.detail_focus == DetailFocus::Description {
        if key.code == KeyCode::Enter {
            commit_description(app);
        } else {
            app.secret.description.handle_key(&key);
        }
        return Ok(());
    }

    // Versions focused: navigate / mutate the selected version.
    let n = versions.len();
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if n > 0 && app.secret.version_selected + 1 < n {
                app.secret.version_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.secret.version_selected = app.secret.version_selected.saturating_sub(1);
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_add_version(app);
        }
        KeyCode::Char('e') | KeyCode::Char('d') => {
            // Toggle ENABLED/DISABLED on the selected version.
            if let Some(v) = versions.get(app.secret.version_selected) {
                toggle_version_status(app, &tenant, &id, v);
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Some(v) = versions.get(app.secret.version_selected) {
                destroy_version(app, &tenant, &id, v);
            }
        }
        _ => {}
    }
    Ok(())
}

// --- Edit description -----------------------------------------------------

/// Commit the edited description for the open secret via `setDescription`.
/// No-ops (with an info toast) when the text is unchanged.
fn commit_description(app: &mut App) {
    let Some((tenant, id)) = app.secret.version_target.clone() else {
        return;
    };
    let new_desc = app.secret.description.value.clone();
    let previous = secret_in_cache(app, &tenant, &id)
        .map(|s| description_of(&s).to_string())
        .unwrap_or_default();
    if new_desc == previous {
        app.push_toast(ToastKind::Info, "Description unchanged".to_string());
        app.secret.detail_focus = DetailFocus::Versions;
        return;
    }

    let plan = SetDescriptionPlan {
        tenant,
        id,
        description: new_desc,
        previous,
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretSetDescription(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_set_description(app, plan, false);
    }
}

#[derive(Debug, Clone)]
pub struct SetDescriptionPlan {
    pub tenant: String,
    pub id: String,
    pub description: String,
    pub previous: String,
}

pub fn execute_set_description(app: &mut App, plan: SetDescriptionPlan, confirmed_prod: bool) {
    // Return focus to the version list so the user sees the edit was submitted
    // (the cursor leaves the description field).
    app.input_mode = InputMode::SecretVersions;
    app.secret.detail_focus = DetailFocus::Versions;
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));

    // Optimistic: reflect the new description in the local cache now, so the
    // detail pane shows it immediately instead of after the round-trip. The
    // server value reconciles on completion (`refresh_tenant`).
    set_local_secret_description(app, &plan.tenant, &plan.id, &plan.description);

    // Record the revert undo up front (like variable edits do): a failed write
    // leaves the description at `previous`, so the conflict guard in
    // `undo_set_description` (current == expected) refuses to fire — safe.
    record_set_description_undo(app, &plan);

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            crate::aic::esv::set_secret_description(&plan.tenant, &plan.id, &plan.description, confirmed_prod)
                .await
                .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::SetDescription,
            label: "Updated description".to_string(),
            reload_versions: false,
            result,
        });
    });
}

fn record_set_description_undo(app: &mut App, plan: &SetDescriptionPlan) {
    let entry = UndoEntry::pending(
        plan.tenant.clone(),
        "secret",
        format!("Revert description of {}", plan.id),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::SecretSetDescription {
            tenant: plan.tenant.clone(),
            id: plan.id.clone(),
            previous: plan.previous.clone(),
            expected: plan.description.clone(),
        }),
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        tracing::warn!("failed to record secret-description undo for {}: {e}", plan.id);
    }
}

/// Undo a description change: set it back to `previous`, but only if the live
/// description still equals `expected` (what we set), so a later edit isn't
/// clobbered. Lives here so the secret-specific conflict logic stays put.
pub async fn undo_set_description(
    tenant: &str,
    id: &str,
    previous: &str,
    expected: &str,
    confirmed_prod: bool,
) -> std::result::Result<(), crate::screens::esv::UndoFailure> {
    use crate::screens::esv::UndoFailure;
    match crate::aic::esv::get_secret(tenant, id).await {
        Ok(current) => {
            if description_of(&current) != expected {
                return Err(UndoFailure::Conflict(format!(
                    "{id}'s description changed since; refusing to overwrite"
                )));
            }
        }
        Err(crate::Error::Api { status: 404, .. }) => {
            return Err(UndoFailure::Conflict(format!("{id} no longer exists")));
        }
        Err(e) => return Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
    }
    crate::aic::esv::set_secret_description(tenant, id, previous, confirmed_prod)
        .await
        .map(|_| ())
        .map_err(|e| UndoFailure::Failed(e.to_string()))
}

/// Optimistically set a version's `status` in the local cache so the panel
/// repaints immediately. The authoritative list is reloaded on op completion
/// (success *or* failure), so a rejected change self-corrects.
fn set_local_version_status(app: &mut App, tenant: &str, id: &str, version: &str, status: &str) {
    if let Some(LoadState::Loaded(vs)) =
        app.secret.versions.get_mut(&(tenant.to_string(), id.to_string()))
    {
        for v in vs.iter_mut() {
            if version_num(v).as_deref() == Some(version) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("status".into(), serde_json::Value::String(status.to_string()));
                }
            }
        }
    }
}

/// Optimistically set a secret's `description` in the list cache.
fn set_local_secret_description(app: &mut App, tenant: &str, id: &str, description: &str) {
    if let Some(LoadState::Loaded(items)) = app.secret.list.data.get_mut(tenant) {
        for v in items.iter_mut() {
            if id_of(v) == id {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "description".into(),
                        serde_json::Value::String(description.to_string()),
                    );
                }
            }
        }
    }
}

fn version_num(v: &serde_json::Value) -> Option<String> {
    v.get("version")
        .and_then(|x| x.as_str().map(|s| s.to_string()).or_else(|| x.as_u64().map(|n| n.to_string())))
}

fn version_status(v: &serde_json::Value) -> &str {
    v.get("status").and_then(|x| x.as_str()).unwrap_or("?")
}

fn toggle_version_status(app: &mut App, tenant: &str, id: &str, v: &serde_json::Value) {
    let Some(version) = version_num(v) else {
        return;
    };
    let status = version_status(v);
    let new_status = match status {
        "ENABLED" => "DISABLED",
        "DISABLED" => "ENABLED",
        other => {
            app.push_toast(
                ToastKind::Info,
                format!("Version {version} is {other}; status can't change"),
            );
            return;
        }
    };
    let tenant = tenant.to_string();
    let id = id.to_string();
    let confirmed_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    // Status changes are low-risk; route prod through confirm too.
    if confirmed_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretVersionStatus {
            tenant,
            id,
            version,
            status: new_status.to_string(),
        });
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_version_status(app, tenant, id, version, new_status.to_string(), false);
}

pub fn execute_version_status(
    app: &mut App,
    tenant: String,
    id: String,
    version: String,
    status: String,
    confirmed_prod: bool,
) {
    app.input_mode = InputMode::SecretVersions;
    // Optimistic: flip the version's status locally so the panel repaints at
    // once; `apply_op_result` reloads the authoritative state when the call
    // returns (and reverts this on failure).
    set_local_version_status(app, &tenant, &id, &version, &status);
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            crate::aic::esv::change_version_status(&tenant, &id, &version, &status, confirmed_prod)
                .await
                .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant,
            id,
            kind: SecretOpKind::StatusChange,
            label: format!("Version {version} → {status}"),
            reload_versions: true,
            result,
        });
    });
}

fn destroy_version(app: &mut App, tenant: &str, id: &str, v: &serde_json::Value) {
    let Some(version) = version_num(v) else {
        return;
    };
    if version_status(v) == "DESTROYED" {
        app.push_toast(ToastKind::Info, format!("Version {version} already destroyed"));
        return;
    }
    // Destroy is irreversible, so always confirm locally first (even outside
    // prod), then layer the prod confirmation on top for prod tenants.
    app.secret.pending_version_destroy = Some((tenant.to_string(), id.to_string(), version));
    app.input_mode = InputMode::SecretVersionDestroyConfirm;
}

/// y/n confirmation before an irreversible version destroy.
pub fn handle_version_destroy_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some((tenant, id, version)) = app.secret.pending_version_destroy.take() else {
                app.input_mode = InputMode::SecretVersions;
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::SecretVersionDestroy {
                    tenant,
                    id,
                    version,
                });
                app.input_mode = InputMode::ProdConfirm;
            } else {
                execute_version_destroy(app, tenant, id, version, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.secret.pending_version_destroy = None;
            app.input_mode = InputMode::SecretVersions;
        }
        _ => {}
    }
    Ok(())
}

pub fn execute_version_destroy(
    app: &mut App,
    tenant: String,
    id: String,
    version: String,
    confirmed_prod: bool,
) {
    app.input_mode = InputMode::SecretVersions;
    // Optimistic: mark the version DESTROYED locally; reconciled on completion.
    set_local_version_status(app, &tenant, &id, &version, "DESTROYED");
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            crate::aic::esv::destroy_secret_version(&tenant, &id, &version, confirmed_prod)
                .await
                .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant,
            id,
            kind: SecretOpKind::Destroy,
            label: format!("Destroyed version {version}"),
            reload_versions: true,
            result,
        });
    });
}

// --- Delete secret --------------------------------------------------------

pub fn request_delete(app: &mut App) {
    let Some(secret) = selected_secret(app) else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    app.secret.pending_delete = Some(DeletePlan {
        tenant,
        id: id_of(&secret).to_string(),
    });
    app.input_mode = InputMode::SecretDeleteConfirm;
}

pub fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(plan) = app.secret.pending_delete.take() else {
                app.input_mode = InputMode::Normal;
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::SecretDelete(plan));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                execute_delete(app, plan, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.secret.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

pub fn execute_delete(app: &mut App, plan: DeletePlan, confirmed_prod: bool) {
    app.input_mode = InputMode::Normal;
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    // The irreversible history entry is recorded only once the delete actually
    // succeeds (see `apply_op_result`).
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::delete_secret(&plan.tenant, &plan.id, confirmed_prod)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::SecretOpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::Delete,
            label: "Deleted secret".to_string(),
            reload_versions: false,
            result,
        });
    });
}

// --- Event handlers (called from app::handle_event) -----------------------

pub fn apply_versions_listed(
    app: &mut App,
    tenant: String,
    id: String,
    result: std::result::Result<Vec<serde_json::Value>, String>,
) {
    let state = match result {
        Ok(vs) => {
            let n = vs.len();
            if app.secret.version_selected >= n {
                app.secret.version_selected = n.saturating_sub(1);
            }
            LoadState::Loaded(vs)
        }
        Err(e) => LoadState::Failed(e),
    };
    app.secret.versions.insert((tenant, id), state);
}

pub fn apply_op_result(
    app: &mut App,
    tenant: String,
    id: String,
    kind: SecretOpKind,
    label: String,
    reload: bool,
    result: std::result::Result<serde_json::Value, String>,
) {
    app.secret.in_flight.remove(&(tenant.clone(), id.clone()));
    match result {
        Ok(body) => {
            // Record undo / history only now that the op truly succeeded.
            match kind {
                SecretOpKind::Create => record_create_undo(app, &tenant, &id, &body),
                SecretOpKind::Delete => record_delete_history(app, &tenant, &id),
                _ => {}
            }
            // Create records its undo here; SetDescription recorded it up front
            // in `execute_set_description`. Both can be reverted with ^Z.
            let suffix = if matches!(kind, SecretOpKind::Create | SecretOpKind::SetDescription) {
                " — ^Z to undo"
            } else {
                ""
            };
            app.push_toast(ToastKind::Success, format!("{label}: {id}{suffix}"));
            // Re-poll the *event's* tenant (not whatever is active now, in case
            // the user switched tenants while the request was in flight).
            crate::screens::esv::refresh_tenant(app, &tenant, true);
            if reload {
                reload_versions(app, tenant, id);
            }
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("{label} failed: {id} — {e}"));
            // Reconcile any optimistic change back to the server's real state.
            if reload {
                reload_versions(app, tenant, id);
            } else {
                crate::screens::esv::refresh_tenant(app, &tenant, true);
            }
        }
    }
}

/// Record the post-success undo for a secret create: delete the secret we
/// just made, guarded by its `lastChangeDate` so the undo can never delete a
/// secret that has since been changed by someone else.
fn record_create_undo(app: &mut App, tenant: &str, id: &str, body: &serde_json::Value) {
    let active_version = body
        .get("activeVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("1")
        .to_string();
    let entry = UndoEntry::pending(
        tenant.to_string(),
        "secret",
        format!("Delete created secret {id}"),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::SecretDelete {
            tenant: tenant.to_string(),
            id: id.to_string(),
            active_version,
        }),
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        // Non-fatal: the secret exists; we just won't be able to ^Z it.
        tracing::warn!("failed to record secret-create undo for {id}: {e}");
    }
}

/// Record an irreversible history entry for a completed secret delete (no undo
/// op — the value can't be recovered).
fn record_delete_history(app: &mut App, tenant: &str, id: &str) {
    let entry = UndoEntry::pending(
        tenant.to_string(),
        "secret",
        format!("Deleted secret {id} (irreversible)"),
        Sensitivity::TenantConfig,
        Capability::Irreversible,
        None,
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        tracing::warn!("failed to record secret-delete history for {id}: {e}");
    }
}

/// Execute the undo of a secret create: refuse unless the secret still exists
/// and its `lastChangeDate` matches what the create returned, then delete it.
/// Lives here (not in `screens::esv`) so the secret-specific conflict logic
/// stays with the rest of the secrets code.
pub async fn undo_delete(
    tenant: &str,
    id: &str,
    active_version: &str,
    confirmed_prod: bool,
) -> std::result::Result<(), crate::screens::esv::UndoFailure> {
    use crate::screens::esv::UndoFailure;
    match crate::aic::esv::get_secret(tenant, id).await {
        Ok(current) => {
            let current_version = current
                .get("activeVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if current_version != active_version {
                return Err(UndoFailure::Conflict(format!(
                    "{id} gained new versions since it was created; refusing to delete"
                )));
            }
        }
        Err(crate::Error::Api { status: 404, .. }) => {
            return Err(UndoFailure::Conflict(format!("{id} no longer exists")));
        }
        Err(e) => return Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
    }
    crate::aic::esv::delete_secret(tenant, id, confirmed_prod)
        .await
        .map(|_| ())
        .map_err(|e| UndoFailure::Failed(e.to_string()))
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
        // With the toggle off, the same value is accepted as plain text.
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
        // A valid base64 key string → valueBase64 = base64(that string).
        let key = B64.encode([7u8; 32]); // 32-byte HMAC key, base64-encoded
        let b64 = encode_value(Encoding::Base64Hmac, &key, false).unwrap();
        assert_eq!(decode(&b64), key.as_bytes());
        // Non-base64 inner value is rejected.
        assert!(encode_value(Encoding::Base64Hmac, "@@not base64@@", false).is_err());
    }

    #[test]
    fn aes_key_length_validated() {
        let ok = B64.encode([0u8; 16]); // 128-bit
        assert!(encode_value(Encoding::Base64Aes, &ok, false).is_ok());
        let bad = B64.encode([0u8; 20]); // not 16/24/32
        assert!(encode_value(Encoding::Base64Aes, &bad, false).is_err());
    }
}
