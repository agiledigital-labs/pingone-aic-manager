//! ESV tab — list, fuzzy search, preview. The state struct lives on `App`
//! as a single field (`app.esv`); handlers are free functions that take
//! `&mut App` so the dispatch table in `app.rs` stays one-liner per arm.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use crossterm::event::{KeyCode, KeyEvent};

use crate::aic::esv::StartupStatus;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::event::{AppEvent, ToastKind};
use crate::screens::list_state::TenantListState;
use crate::screens::prod_confirm::PendingProdAction;
use crate::ui::widgets::TextField;
use crate::undo::{Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoId, UndoOp};

/// Per-tenant ESV load state. `app.esv.list.data` maps tenant name → this.
#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Vec<serde_json::Value>),
    Failed(String),
}

/// Which half of the ESVs tab is showing. Toggled with Tab/Shift-Tab in
/// Normal mode. Secrets and variables share the tab's apply/restart banner
/// and a single background poll, but render and edit very differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsvView {
    Variables,
    Secrets,
}

impl EsvView {
    pub fn toggled(self) -> Self {
        match self {
            EsvView::Variables => EsvView::Secrets,
            EsvView::Secrets => EsvView::Variables,
        }
    }
}

/// Per-tenant apply state shown in the ESV banner. This is sticky: refreshes
/// keep showing the previous value until a tenant response lets us replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyState {
    NoChanges,
    Unapplied(usize),
    Restarting(usize),
}

impl ApplyState {
    pub fn from_authoritative(startup: StartupStatus, pending: usize) -> Self {
        match startup {
            StartupStatus::Restarting => Self::Restarting(pending),
            StartupStatus::Ready if pending > 0 => Self::Unapplied(pending),
            StartupStatus::Ready => Self::NoChanges,
        }
    }
}

/// Result of a background ESV refresh. Variables and startup status are kept
/// separate so we can update whichever half succeeded without clobbering the
/// cached state owned by the other half.
#[derive(Debug)]
pub struct RefreshOutcome {
    pub variables: std::result::Result<Vec<serde_json::Value>, String>,
    pub pending_variables: std::result::Result<Vec<serde_json::Value>, String>,
    pub secrets: std::result::Result<Vec<serde_json::Value>, String>,
    pub pending_secrets: std::result::Result<Vec<serde_json::Value>, String>,
    pub startup: std::result::Result<StartupStatus, String>,
}

/// One row in the rendered ESV list. The UI consumes `Vec<Match>` from
/// [`State::matches`]; sorted-by-score with match positions in `_id` for
/// per-char highlight.
#[derive(Debug, Clone)]
pub struct Match {
    pub idx: Option<usize>,
    pub id: String,
    pub score: u32,
    pub positions: Vec<u32>,
    pub deleted: bool,
}

/// Extract the `_id` field of an ESV variable; falls back to `"?"` when
/// the API returned something unexpected.
pub fn id_of(v: &serde_json::Value) -> &str {
    v.get("_id").and_then(|x| x.as_str()).unwrap_or("?")
}

/// True iff a variable's `loaded` flag is `false` — i.e. the runtime
/// hasn't picked it up yet and a tenant restart is needed. New variables
/// land with `loaded=false`, as do recently-saved edits.
pub fn is_pending(v: &serde_json::Value) -> bool {
    !v.get("loaded").and_then(|x| x.as_bool()).unwrap_or(true)
}

/// How many variables in this tenant's cached list still need a restart
/// to take effect. Drives the banner above the preview pane and gates
/// the `^S` apply keybind.
/// The single source of truth for the ESV tab's restart gating. Folds both
/// halves of the tab together: variable + secret changes that need a restart
/// (`pending`), and variable + secret writes still in flight (`in_flight`).
/// All banner / `^S` / apply-state decisions go through here so the two halves
/// can never disagree.
#[derive(Debug, Clone, Copy, Default)]
pub struct EsvPending {
    pub pending: usize,
    pub in_flight: usize,
}

pub fn pending_summary(app: &App, tenant: &str) -> EsvPending {
    let mut ids = app.esv.list.pending_ids.get(tenant).cloned().unwrap_or_default();
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get(tenant) {
        ids.extend(
            items
                .iter()
                .filter(|v| is_pending(v))
                .map(|v| id_of(v).to_string()),
        );
    }
    // Secrets share the tab's restart gate: a `useInPlaceholders:true` secret
    // whose active version hasn't loaded also needs the tenant restart.
    let pending = ids.len() + crate::screens::secret::pending_count(app, tenant);
    let var_in_flight = app
        .esv
        .in_flight_writes
        .iter()
        .filter(|(t, _)| t == tenant)
        .count();
    let secret_in_flight = app
        .secret
        .in_flight
        .iter()
        .filter(|(t, _)| t == tenant)
        .count();
    EsvPending {
        pending,
        in_flight: var_in_flight + secret_in_flight,
    }
}

pub fn pending_count(app: &App, tenant: &str) -> usize {
    pending_summary(app, tenant).pending
}

/// AIC's documented set of expression types for variables. Acts as a chip
/// cycle on the edit form — `←/→` step through `ORDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionType {
    String,
    Array,
    Bool,
    Int,
    Number,
    Object,
    List,
    KeyValueList,
    Base64EncodedInlined,
}

impl ExpressionType {
    pub const ORDER: &'static [ExpressionType] = &[
        ExpressionType::String,
        ExpressionType::Array,
        ExpressionType::Bool,
        ExpressionType::Int,
        ExpressionType::Number,
        ExpressionType::Object,
        ExpressionType::List,
        ExpressionType::KeyValueList,
        ExpressionType::Base64EncodedInlined,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ExpressionType::String => "string",
            ExpressionType::Array => "array",
            ExpressionType::Bool => "bool",
            ExpressionType::Int => "int",
            ExpressionType::Number => "number",
            ExpressionType::Object => "object",
            ExpressionType::List => "list",
            ExpressionType::KeyValueList => "keyvaluelist",
            ExpressionType::Base64EncodedInlined => "base64encodedinlined",
        }
    }

    /// Parse an AIC-emitted expressionType string. Falls back to `String`
    /// for unknown values rather than refusing to load — better to let the
    /// user re-pick than to block editing.
    pub fn parse(s: &str) -> Self {
        Self::ORDER
            .iter()
            .copied()
            .find(|e| e.as_str() == s)
            .unwrap_or(ExpressionType::String)
    }

    pub fn cycle(self, delta: i32) -> Self {
        let i = Self::ORDER.iter().position(|e| *e == self).unwrap_or(0) as i32;
        let n = Self::ORDER.len() as i32;
        let next = (i + delta).rem_euclid(n) as usize;
        Self::ORDER[next]
    }

    /// Local pre-flight validation of the on-screen value against the
    /// selected type. Catches type / value mismatches before we ship a
    /// PUT that would either be rejected by AIC or — worse — accepted
    /// and confuse the runtime. Returns `Ok(())` for types we don't have
    /// a strong contract for (string, list, keyvaluelist,
    /// base64encodedinlined).
    pub fn validate(self, value: &str) -> Result<(), String> {
        match self {
            ExpressionType::String
            | ExpressionType::List
            | ExpressionType::KeyValueList
            | ExpressionType::Base64EncodedInlined => Ok(()),
            ExpressionType::Bool => match value.trim() {
                "true" | "false" => Ok(()),
                _ => Err("Value must be 'true' or 'false'".into()),
            },
            ExpressionType::Int => value
                .trim()
                .parse::<i64>()
                .map(|_| ())
                .map_err(|_| "Value must be an integer".into()),
            ExpressionType::Number => value
                .trim()
                .parse::<f64>()
                .map(|_| ())
                .map_err(|_| "Value must be a number".into()),
            ExpressionType::Object => match serde_json::from_str::<serde_json::Value>(value.trim())
            {
                Ok(serde_json::Value::Object(_)) => Ok(()),
                Ok(_) => Err("Value must be a JSON object (e.g. {\"k\":\"v\"})".into()),
                Err(e) => Err(format!("Value must be valid JSON: {e}")),
            },
            ExpressionType::Array => {
                match serde_json::from_str::<serde_json::Value>(value.trim()) {
                    Ok(serde_json::Value::Array(_)) => Ok(()),
                    Ok(_) => Err("Value must be a JSON array (e.g. [1,2,3])".into()),
                    Err(e) => Err(format!("Value must be valid JSON: {e}")),
                }
            }
        }
    }
}

/// Focusable fields on the edit form. Tab/Shift-Tab walks through these
/// in order, skipping read-only rows. `Id` is only focusable when
/// creating a new variable; the edit form has it pinned as read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Id,
    Description,
    Type,
    Value,
    Save,
}

impl EditField {
    const EDIT_ORDER: &'static [EditField] = &[
        EditField::Description,
        EditField::Type,
        EditField::Value,
        EditField::Save,
    ];
    const CREATE_ORDER: &'static [EditField] = &[
        EditField::Id,
        EditField::Description,
        EditField::Type,
        EditField::Value,
        EditField::Save,
    ];

    fn order(creating: bool) -> &'static [EditField] {
        if creating {
            Self::CREATE_ORDER
        } else {
            Self::EDIT_ORDER
        }
    }

    pub fn next(self, creating: bool) -> Self {
        let order = Self::order(creating);
        let i = order.iter().position(|f| *f == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn prev(self, creating: bool) -> Self {
        let order = Self::order(creating);
        let i = order.iter().position(|f| *f == self).unwrap_or(0);
        order[(i + order.len() - 1) % order.len()]
    }
}

/// Live state for the edit form. `original` is the snapshot we were
/// editing from — we compare against a fresh refetch on Save for
/// content-based conflict detection (variables have no `_rev`). For a
/// create flow, `creating` is true, `original` is `null`, and `id_input`
/// is the user-typed identifier.
#[derive(Debug)]
pub struct EditState {
    pub id: String,
    pub original: serde_json::Value,
    pub creating: bool,
    /// Editable identifier when `creating`; ignored otherwise.
    pub id_input: TextField,
    pub description: TextField,
    pub expr_type: ExpressionType,
    pub value: TextField,
    pub focused: EditField,
    /// Pre-flight validation error to surface in the form. Set on bad
    /// id / type-value mismatch; cleared on any user input.
    pub error: Option<String>,
}

/// Fully-validated save payload, captured before any production confirmation
/// modal takes focus. Executing the plan performs the optimistic local update
/// and spawns the tenant write.
#[derive(Debug)]
pub struct SavePlan {
    tenant_name: String,
    id: String,
    description: String,
    expr_type: String,
    value_b64: String,
    original: Option<serde_json::Value>,
    optimistic: serde_json::Value,
    was_creating: bool,
}

struct SaveRequest {
    tenant_name: String,
    id: String,
    description: String,
    expr_type: String,
    value_b64: String,
    original: Option<serde_json::Value>,
}

/// Fully-captured delete payload. The original body is required both for
/// conflict detection and for undo.
#[derive(Debug, Clone)]
pub struct DeletePlan {
    tenant_name: String,
    id: String,
    original: serde_json::Value,
}

struct DeleteRequest {
    tenant_name: String,
    id: String,
    original: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct DeleteTombstone {
    pub deleted_at: Instant,
    pub body: serde_json::Value,
}

#[derive(Debug)]
pub struct State {
    /// Shared per-tenant list mechanics: cached data, pending ids, the search
    /// query, and cursor/scroll. See [`TenantListState`].
    pub list: TenantListState,
    /// Tenants whose ESV refetch is currently in flight — guards against
    /// duplicate spawns when the user re-enters the tab or the poll fires.
    pub refreshing: HashSet<String>,
    /// When the last poll-refresh ran. Drives the 30s cadence in `tick`.
    pub last_poll: Instant,

    /// In-flight edit. `None` = preview pane (read-only display). `Some`
    /// = the right pane is the editable form. See `start_edit` / `cancel_edit`.
    pub editing: Option<EditState>,

    /// Variables we just saved that may not yet appear in a polled list
    /// (AIC's `/environment/variables` is eventually consistent — a brand-
    /// new variable can take a few seconds to show up). Keyed by
    /// `(tenant, id)`; each value is `(saved_at, body)`. On every
    /// `apply_refresh` we re-merge any entries still within
    /// `RECENT_WRITE_TTL` so the local view never loses them.
    pub recent_writes: HashMap<(String, String), (Instant, serde_json::Value)>,

    /// Variables deleted locally/through AIC that we keep visible as local
    /// undo tombstones. Ping's pending endpoints do not report deletes.
    pub recent_deletes: HashMap<(String, String), DeleteTombstone>,

    /// (tenant, id) for any optimistic save whose background request
    /// failed. The local cache still holds the user's attempted body
    /// (via `recent_writes`); the list renders these rows in red so the
    /// user can re-open and retry. Cleared on a subsequent successful
    /// save for the same id.
    pub failed_writes: HashSet<(String, String)>,

    /// Last known apply state per tenant. Refreshes update this only after
    /// authoritative tenant responses arrive; while a refresh is in flight
    /// the previous value keeps rendering.
    pub apply_states: HashMap<String, ApplyState>,

    /// When a restart first moved into `ApplyState::Restarting`. This is
    /// display metadata only; it never times the state out.
    pub restart_started_at: HashMap<String, Instant>,

    /// (tenant, id) for every optimistic save whose background PUT
    /// hasn't returned yet. Drives the "queued" banner colour and gates
    /// the `^S` apply keybind — we don't want to restart while a write
    /// is in flight. Cleared by `apply_save_result` regardless of
    /// success / failure.
    pub in_flight_writes: HashSet<(String, String)>,

    /// Original values for optimistic deletes that are still in flight.
    /// Used to restore the local cache if the background DELETE fails.
    pub in_flight_deletes: HashMap<(String, String), serde_json::Value>,

    /// Delete plan waiting on the local y/n confirmation popover.
    pub pending_delete: Option<DeletePlan>,

    /// Which half of the tab (variables / secrets) is showing.
    pub view: EsvView,
}

/// How long we keep a freshly-saved variable pinned to the local cache
/// after a save. Long enough to cover AIC's indexing lag on creates.
pub const RECENT_WRITE_TTL: std::time::Duration = std::time::Duration::from_secs(120);

/// How long we keep a delete tombstone: both the red `!` ghost row in the
/// list and the negative pin that suppresses the deleted id if AIC's
/// eventually-consistent list endpoint still returns it. After this the
/// ghost drops off; the undo entry itself outlives it (recover via `^Y`).
pub const DELETE_TOMBSTONE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

impl Default for State {
    fn default() -> Self {
        Self {
            list: TenantListState::new(),
            refreshing: HashSet::new(),
            last_poll: Instant::now(),
            editing: None,
            recent_writes: HashMap::new(),
            recent_deletes: HashMap::new(),
            failed_writes: HashSet::new(),
            apply_states: HashMap::new(),
            restart_started_at: HashMap::new(),
            in_flight_writes: HashSet::new(),
            in_flight_deletes: HashMap::new(),
            pending_delete: None,
            view: EsvView::Variables,
        }
    }
}

/// Banner display mode — picks the colour and the wording. Computed
/// fresh on every draw from the cached apply state plus in-flight saves.
#[derive(Debug, Clone, Copy)]
pub enum BannerState {
    None,
    /// `n` variables have been saved and need a tenant restart to take
    /// effect. The default state — pastel blue.
    ToApply(usize),
    /// `n` background saves haven't returned yet. Pastel purple. `^S`
    /// is disabled until queued drops to zero.
    Queued(usize),
    /// `n` variables are mid-restart. Pastel yellow. `^S` is a no-op.
    Applying(usize),
}

/// Number of background ESV saves currently in flight for `tenant`.
pub fn queued_count(app: &App, tenant: &str) -> usize {
    // In-flight writes for *both* halves of the tab — a secret mutation in
    // flight must gate `^S` just like a variable one.
    pending_summary(app, tenant).in_flight
}

/// Pick which banner (if any) to display. Precedence is applying, then
/// queued, then to-apply. The applying state wins even when queued > 0
/// because a restart in progress is the more important signal.
pub fn banner_state(app: &App, tenant: &str) -> BannerState {
    if let Some(ApplyState::Restarting(n)) = app.esv.apply_states.get(tenant).copied() {
        return BannerState::Applying(n);
    }
    let q = queued_count(app, tenant);
    if q > 0 {
        return BannerState::Queued(q);
    }
    if let Some(ApplyState::Unapplied(n)) = app.esv.apply_states.get(tenant).copied() {
        return BannerState::ToApply(n);
    }
    BannerState::None
}

/// True when the "applying" banner state should be shown. This is driven by
/// cached tenant state, not elapsed local time.
pub fn is_applying(app: &App, tenant: &str) -> bool {
    matches!(
        app.esv.apply_states.get(tenant),
        Some(ApplyState::Restarting(_))
    )
}

/// True when `^S` can open the restart confirmation for this tenant.
pub fn can_request_restart(app: &App, tenant: &str) -> bool {
    matches!(
        app.esv.apply_states.get(tenant),
        Some(ApplyState::Unapplied(n)) if *n > 0
    ) && queued_count(app, tenant) == 0
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop view state (filter + selection). Called on tenant switch.
    pub fn reset_view(&mut self) {
        self.list.query.clear();
        self.list.selected = 0;
        self.list.scroll = 0;
    }

    /// Apply the fuzzy filter to a tenant's loaded ESV list. Returns
    /// matches sorted by score (descending), with match positions for
    /// per-char highlighting. Empty when the tenant isn't `Loaded`.
    pub fn matches(&self, tenant: Option<&str>) -> Vec<Match> {
        let Some(name) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(items)) = self.list.data.get(name) else {
            return Vec::new();
        };
        if self.list.query.is_empty() {
            let mut indexed: Vec<Match> = items
                .iter()
                .enumerate()
                .map(|(i, v)| Match {
                    idx: Some(i),
                    id: id_of(v).to_string(),
                    score: 0,
                    positions: Vec::new(),
                    deleted: false,
                })
                .collect();
            let live_ids: HashSet<String> = items.iter().map(|v| id_of(v).to_string()).collect();
            indexed.extend(
                self.recent_deletes
                    .iter()
                    .filter(|((t, id), _)| t == name && !live_ids.contains(id))
                    .map(|((_, id), _)| Match {
                        idx: None,
                        id: id.clone(),
                        score: 0,
                        positions: Vec::new(),
                        deleted: true,
                    }),
            );
            indexed.sort_by(|a, b| a.id.cmp(&b.id));
            return indexed;
        }
        use nucleo_matcher::{
            Config, Matcher, Utf32Str,
            pattern::{AtomKind, CaseMatching, Normalization, Pattern},
        };
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::new(
            self.list.query.value(),
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut out: Vec<Match> = Vec::new();
        let mut buf = Vec::new();
        let mut positions: Vec<u32> = Vec::new();
        let live_ids: HashSet<String> = items.iter().map(|v| id_of(v).to_string()).collect();
        let mut rows: Vec<(Option<usize>, String, bool, bool)> = items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let id = id_of(v).to_string();
                let pending = is_pending(v)
                    || self
                        .list
                        .pending_ids
                        .get(name)
                        .is_some_and(|ids| ids.contains(&id));
                (Some(i), id, false, pending)
            })
            .collect();
        rows.extend(
            self.recent_deletes
                .iter()
                .filter(|((t, id), _)| t == name && !live_ids.contains(id))
                .map(|((_, id), _)| (None, id.clone(), true, false)),
        );
        for (idx, id, deleted, pending) in rows {
            // Build the fuzzy haystack from `id` plus synthetic tags so
            // `/!`, `/!pending`, `/!failed` filter to those rows. The
            // tags are stripped from the highlight positions so the id
            // renders without spurious tag chars.
            let failed = self.failed_writes.contains(&(name.to_string(), id.clone()));
            let mut haystack_text = id.clone();
            if pending {
                haystack_text.push_str(" !pending");
            }
            if failed {
                haystack_text.push_str(" !failed");
            }
            if deleted {
                haystack_text.push_str(" !deleted");
            }
            let id_chars = id.chars().count();
            let haystack = Utf32Str::new(&haystack_text, &mut buf);
            positions.clear();
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                let display_positions: Vec<u32> = positions
                    .iter()
                    .copied()
                    .filter(|p| (*p as usize) < id_chars)
                    .collect();
                out.push(Match {
                    idx,
                    id: id.clone(),
                    score,
                    positions: display_positions,
                    deleted,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// Re-clamp the selection in case the matched list shrunk (e.g. after
    /// a refetch returns a shorter list, or after the filter changed).
    pub fn clamp_selection(&mut self, n: usize) {
        if self.list.selected >= n {
            self.list.selected = n.saturating_sub(1);
        }
    }
}

/// Kick off a background ESV fetch for the active tenant.
///
/// - `force = false`: only fetches when there's no cached entry yet
///   (initial load on startup / tenant switch).
/// - `force = true`: always fetches, even if a `Loaded` entry exists.
///   The stale data stays visible until the new fetch completes; failed
///   refetches don't clobber the cached value (see the `EsvListed`
///   handler in `app::handle_event`).
///
/// A no-op when (a) the app is locked (still on the unlock screen — the
/// agent would return `Locked` and we'd just surface noise), (b) there's
/// no active tenant, or (c) a fetch for this tenant is already in flight.
/// Refetches keep the previous list and apply-state visible until both
/// tenant calls return through the event loop.
pub fn refresh(app: &mut App, force: bool) {
    let Some(name) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    refresh_tenant(app, &name, force);
}

/// Like [`refresh`] but for a specific tenant by name — used by async
/// completion handlers so a result that lands after the user switched
/// tenants still refreshes the tenant it actually mutated, not whichever
/// one happens to be active now.
pub fn refresh_tenant(app: &mut App, name: &str, force: bool) {
    if !app.is_unlocked() {
        return;
    }
    let name = name.to_string();
    if app.esv.refreshing.contains(&name) {
        return;
    }
    if !force && app.esv.list.data.contains_key(&name) {
        return;
    }

    // Only show the Loading spinner when there's nothing cached yet;
    // refetches keep the previous Loaded entry visible.
    if !app.esv.list.data.contains_key(&name) {
        app.esv.list.data.insert(name.clone(), LoadState::Loading);
    }
    app.esv.refreshing.insert(name.clone());
    app.esv.last_poll = Instant::now();

    let tx = app.events.tx.clone();
    let tenant_name = name.clone();
    tokio::spawn(async move {
        let (variables, pending_variables, secrets, pending_secrets, startup) = tokio::join!(
            crate::aic::esv::list_variables(&tenant_name),
            crate::aic::esv::list_pending_variables(&tenant_name),
            crate::aic::esv::list_secrets(&tenant_name),
            crate::aic::esv::list_pending_secrets(&tenant_name),
            crate::aic::esv::startup_status(&tenant_name),
        );
        let outcome = RefreshOutcome {
            variables: variables.map_err(|e| e.to_string()),
            pending_variables: pending_variables.map_err(|e| e.to_string()),
            secrets: secrets.map_err(|e| e.to_string()),
            pending_secrets: pending_secrets.map_err(|e| e.to_string()),
            startup: startup.map_err(|e| e.to_string()),
        };
        let _ = tx.send(AppEvent::EsvListed {
            tenant: name,
            outcome,
        });
    });
}

/// Open the restart-confirm popup if there's anything to apply, the
/// background saves have caught up, and a restart isn't already in
/// flight. Each negative case gets its own info toast so the user can
/// see why the keystroke was a no-op.
pub fn request_restart(app: &mut App) {
    let Some(tenant_name) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    if is_applying(app, &tenant_name) {
        app.push_toast(crate::event::ToastKind::Info, "Restart already in progress");
        return;
    }
    if queued_count(app, &tenant_name) > 0 {
        // Purple banner already tells the user a save is in flight;
        // swallow ^S silently rather than stacking a contradicting toast.
        return;
    }
    if !can_request_restart(app, &tenant_name) {
        app.push_toast(crate::event::ToastKind::Info, "No pending changes to apply");
        return;
    }
    app.input_mode = InputMode::EsvRestartConfirm;
}

pub fn request_delete(app: &mut App) {
    let Some(plan) = build_delete_plan(app) else {
        return;
    };
    app.esv.pending_delete = Some(plan);
    app.input_mode = InputMode::EsvDeleteConfirm;
}

fn build_delete_plan(app: &mut App) -> Option<DeletePlan> {
    let tenant = app.active_tenant()?;
    let tenant_name = tenant.name.clone();
    let matches = app.esv.matches(Some(&tenant_name));
    let m = matches.get(app.esv.list.selected)?;
    if m.deleted {
        app.push_toast(
            ToastKind::Info,
            "Variable is already deleted; press ^Z to undo",
        );
        return None;
    }
    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), m.id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", m.id),
        );
        return None;
    }
    let Some(LoadState::Loaded(items)) = app.esv.list.data.get(&tenant_name) else {
        return None;
    };
    let original = items.get(m.idx?).cloned()?;
    Some(DeletePlan {
        tenant_name,
        id: m.id.clone(),
        original,
    })
}

/// Dispatched from the y/n delete popup. `y` may still route through the
/// shared production confirmation before executing the delete.
pub fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(plan) = app.esv.pending_delete.take() else {
                app.input_mode = InputMode::Normal;
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::EsvDelete(plan));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                execute_delete_plan(app, plan, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.esv.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

/// Dispatched from the y/n popup. `y` triggers the restart, `n`/`Esc`
/// closes the popup.
pub fn handle_restart_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            trigger_restart(app);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

fn trigger_restart(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::EsvRestart { tenant_name });
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    trigger_restart_confirmed(app, tenant_name, false);
}

pub fn trigger_restart_confirmed(app: &mut App, tenant_name: String, confirmed_prod: bool) {
    // Flip the banner to its "applying" state immediately so the user
    // sees their click registered. It stays there until `/environment/startup`
    // and `/environment/variables` together prove a new state.
    let pending = pending_count(app, &tenant_name);
    set_apply_state(app, &tenant_name, ApplyState::Restarting(pending));
    app.input_mode = InputMode::Normal;
    app.push_toast(
        crate::event::ToastKind::Info,
        "Restart triggered — runtime will pick up changes in a few minutes",
    );
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::trigger_restart(&tenant_name, confirmed_prod)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::EsvRestartResult {
            tenant: tenant_name,
            result,
        });
    });
}

/// Apply the async restart-trigger result. Success → toast; on error
/// surface the message and immediately roll back the "applying" banner
/// state — there's no in-flight restart to wait for.
pub fn apply_restart_result(
    app: &mut App,
    tenant: String,
    result: Result<serde_json::Value, String>,
) {
    match result {
        Ok(_) => {}
        Err(e) => {
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(
                crate::event::ToastKind::Error,
                format!("Restart failed: {e}"),
            );
        }
    }
}

/// EsvSearch keys: chars/backspace/cursor → editor; ↑/↓/PgUp/PgDn keep
/// scrolling the results list while the user is still typing; Esc clears
/// the filter; Enter commits and returns to Normal mode.
pub fn handle_search_key(app: &mut App, key: KeyEvent) {
    // The search box edits whichever half's query is showing.
    if app.esv.view == EsvView::Secrets {
        match key.code {
            KeyCode::Esc => {
                app.secret.list.query.clear();
                app.secret.list.selected = 0;
                app.secret.list.scroll = 0;
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Up => crate::keymap::move_selection(app, -1),
            KeyCode::Down => crate::keymap::move_selection(app, 1),
            KeyCode::PageUp => crate::keymap::move_selection(app, -10),
            KeyCode::PageDown => crate::keymap::move_selection(app, 10),
            _ => {
                let before = app.secret.list.query.value().to_string();
                if app.secret.list.query.handle_key(&key) && app.secret.list.query.value() != before {
                    app.secret.list.selected = 0;
                    app.secret.list.scroll = 0;
                }
            }
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            app.esv.reset_view();
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => {
            crate::keymap::move_selection(app, -1);
            return;
        }
        KeyCode::Down => {
            crate::keymap::move_selection(app, 1);
            return;
        }
        KeyCode::PageUp => {
            crate::keymap::move_selection(app, -10);
            return;
        }
        KeyCode::PageDown => {
            crate::keymap::move_selection(app, 10);
            return;
        }
        _ => {}
    }
    let before = app.esv.list.query.value().to_string();
    if app.esv.list.query.handle_key(&key) && app.esv.list.query.value() != before {
        app.esv.list.selected = 0;
        app.esv.list.scroll = 0;
    }
}

/// Apply an `EsvListed` event to the tab. Caller is `app::handle_event`.
pub fn apply_refresh(app: &mut App, tenant: String, outcome: RefreshOutcome) {
    let is_active = app.active_tenant().is_some_and(|t| t.name == tenant);
    app.esv.refreshing.remove(&tenant);
    let pending_for_merge = outcome
        .pending_variables
        .as_ref()
        .ok()
        .cloned()
        .unwrap_or_default();
    let variables_refreshed = match outcome.variables {
        Ok(mut vs) => {
            // Re-merge any entries we recently saved but the polled list
            // hasn't picked up yet (AIC's variable-list endpoint is
            // eventually consistent — a brand-new variable can lag by a
            // few seconds). Drop expired write pins while we're here.
            app.esv
                .recent_writes
                .retain(|_, (saved_at, _)| saved_at.elapsed() < RECENT_WRITE_TTL);
            // Drop expired delete tombstones so the red `!` ghost rows clear
            // once the delete has had time to settle.
            app.esv
                .recent_deletes
                .retain(|_, tomb| tomb.deleted_at.elapsed() < DELETE_TOMBSTONE_TTL);
            for ((t, recent_id), (_, body)) in app.esv.recent_writes.iter() {
                if t != &tenant {
                    continue;
                }
                if !vs.iter().any(|v| id_of(v) == recent_id) {
                    vs.push(body.clone());
                }
            }
            for pending in &pending_for_merge {
                let pending_id = id_of(pending);
                if !vs.iter().any(|v| id_of(v) == pending_id) {
                    vs.push(pending.clone());
                }
            }
            // Negative pin: AIC's list endpoint is eventually consistent, so a
            // just-deleted variable can still come back for a few polls. While
            // its tombstone is alive, suppress it from the live list so the row
            // stays "deleted" instead of flickering back to a normal entry.
            let suppressed: HashSet<String> = app
                .esv
                .recent_deletes
                .keys()
                .filter(|(t, _)| t == &tenant)
                .map(|(_, id)| id.clone())
                .collect();
            vs.retain(|v| !suppressed.contains(id_of(v)));
            app.esv.list.data.insert(tenant.clone(), LoadState::Loaded(vs));
            if is_active {
                let n = app
                    .esv
                    .matches(app.active_tenant().map(|t| t.name.as_str()))
                    .len();
                app.esv.clamp_selection(n);
            }
            true
        }
        Err(e) => {
            // Don't clobber a previously-cached list with a background-
            // refresh failure — keep showing the stale data and just log.
            if matches!(app.esv.list.data.get(&tenant), Some(LoadState::Loaded(_))) {
                tracing::warn!("ESV refresh failed for {tenant}: {e}");
            } else {
                app.esv.list.data.insert(tenant.clone(), LoadState::Failed(e));
            }
            false
        }
    };
    let pending_refreshed = match outcome.pending_variables {
        Ok(vs) => {
            app.esv.list.pending_ids.insert(
                tenant.clone(),
                vs.iter().map(|v| id_of(v).to_string()).collect(),
            );
            true
        }
        Err(e) => {
            tracing::warn!("ESV pending-variable refresh failed for {tenant}: {e}");
            false
        }
    };

    // Hand the secret half of the poll to the secrets screen. Whether the
    // pending-secret fetch succeeded gates "authoritative" below, since the
    // pending count now folds in secrets.
    let secret_pending_refreshed = outcome.pending_secrets.is_ok();
    crate::screens::secret::apply_refresh(
        app,
        &tenant,
        &outcome.secrets,
        &outcome.pending_secrets,
    );

    match outcome.startup {
        Ok(StartupStatus::Restarting) => {
            set_apply_state(
                app,
                &tenant,
                ApplyState::Restarting(pending_count(app, &tenant)),
            );
        }
        Ok(StartupStatus::Ready)
            if variables_refreshed && pending_refreshed && secret_pending_refreshed =>
        {
            set_apply_state(
                app,
                &tenant,
                ApplyState::from_authoritative(StartupStatus::Ready, pending_count(app, &tenant)),
            );
        }
        Ok(StartupStatus::Ready) => {
            // Startup alone can prove "restarting", but it cannot prove
            // "no changes" without a fresh variable list. Keep the cached
            // apply state until both tenant reads have succeeded together.
        }
        Err(e) => {
            tracing::warn!("ESV startup-status refresh failed for {tenant}: {e}");
        }
    }
}

fn set_apply_state(app: &mut App, tenant: &str, state: ApplyState) {
    match state {
        ApplyState::Restarting(_) => {
            app.esv
                .restart_started_at
                .entry(tenant.to_string())
                .or_insert_with(Instant::now);
        }
        ApplyState::NoChanges | ApplyState::Unapplied(_) => {
            app.esv.restart_started_at.remove(tenant);
        }
    }
    app.esv.apply_states.insert(tenant.to_string(), state);
}

fn refresh_apply_state_from_cache(app: &mut App, tenant: &str) {
    let pending = pending_count(app, tenant);
    let state = match app.esv.apply_states.get(tenant).copied() {
        Some(ApplyState::Restarting(_)) => ApplyState::Restarting(pending),
        _ if pending > 0 => ApplyState::Unapplied(pending),
        _ => ApplyState::NoChanges,
    };
    set_apply_state(app, tenant, state);
}

/// Open the edit form for the currently-selected list row. Snapshots the
/// variable so we have something to diff against on save, decodes the
/// base64 value, and switches input mode.
pub fn start_edit(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let matches = app.esv.matches(Some(&tenant_name));
    let Some(m) = matches.get(app.esv.list.selected) else {
        return;
    };
    if m.deleted {
        app.push_toast(ToastKind::Info, "Deleted variable; press ^Z to restore it");
        return;
    }
    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), m.id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Save already in progress: {}", m.id),
        );
        return;
    }
    let Some(LoadState::Loaded(items)) = app.esv.list.data.get(&tenant_name) else {
        return;
    };
    let Some(idx) = m.idx else { return };
    let Some(v) = items.get(idx).cloned() else {
        return;
    };

    let description = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let expr_type = ExpressionType::parse(
        v.get("expressionType")
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    let value_b64 = v.get("valueBase64").and_then(|x| x.as_str()).unwrap_or("");
    // Try to render the value as UTF-8 text. Binary values fall back to
    // the base64 string itself — they can still be edited (the save path
    // re-encodes whatever we display), they just won't look pretty.
    let value_str = match B64.decode(value_b64) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
            tracing::debug!(id = %m.id, "value is not UTF-8: {e}");
            value_b64.to_string()
        }),
        Err(_) => value_b64.to_string(),
    };

    app.esv.editing = Some(EditState {
        id: m.id.clone(),
        original: v,
        creating: false,
        id_input: TextField::single_line("_id"),
        description: TextField::single_line("Description").with_initial(description),
        expr_type,
        value: TextField::textarea("Value").with_initial(value_str),
        focused: EditField::Description,
        error: None,
    });
    app.input_mode = InputMode::EsvEdit;
}

/// Open the create-new-variable form. Same fields as edit, except `_id`
/// is now an editable field at the top and the save path is a plain PUT
/// (no DELETE step, no conflict refetch — server creates if absent).
pub fn start_create(app: &mut App) {
    if !app.is_unlocked() {
        return;
    }
    if app.active_tenant().is_none() {
        return;
    }
    app.esv.editing = Some(EditState {
        id: String::new(),
        original: serde_json::Value::Null,
        creating: true,
        // The `esv-` prefix is required by AIC, so lock it in: the user
        // types only the suffix and can't delete the prefix.
        id_input: TextField::single_line("_id").with_locked_prefix("esv-"),
        description: TextField::single_line("Description"),
        expr_type: ExpressionType::String,
        value: TextField::textarea("Value"),
        focused: EditField::Id,
        error: None,
    });
    app.input_mode = InputMode::EsvEdit;
}

/// Discard the in-flight edit and return to preview mode.
pub fn cancel_edit(app: &mut App) {
    app.esv.editing = None;
    app.input_mode = InputMode::Normal;
}

pub fn handle_edit_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(edit) = app.esv.editing.as_mut() else {
        return Ok(());
    };
    let creating = edit.creating;
    // Keys that the form owns regardless of which field is focused.
    match key.code {
        KeyCode::Esc => {
            cancel_edit(app);
            return Ok(());
        }
        KeyCode::Tab => {
            edit.focused = edit.focused.next(creating);
            return Ok(());
        }
        KeyCode::BackTab => {
            edit.focused = edit.focused.prev(creating);
            return Ok(());
        }
        KeyCode::Enter => {
            match edit.focused {
                EditField::Save => commit_save(app),
                EditField::Value => edit.value.push_newline(),
                // Enter on a non-textarea field advances focus.
                _ => edit.focused = edit.focused.next(creating),
            }
            return Ok(());
        }
        // ←/→ cycle the chip on the Type row; on any other field they
        // fall through to the TextField's cursor nav below.
        KeyCode::Left if edit.focused == EditField::Type => {
            edit.expr_type = edit.expr_type.cycle(-1);
            return Ok(());
        }
        KeyCode::Right if edit.focused == EditField::Type => {
            edit.expr_type = edit.expr_type.cycle(1);
            return Ok(());
        }
        _ => {}
    }

    // Everything else is per-field text editing — cursor moves, char
    // inserts, backspace, delete-forward.
    match edit.focused {
        EditField::Id if creating => {
            edit.id_input.handle_key(&key);
        }
        EditField::Description => {
            edit.description.handle_key(&key);
        }
        EditField::Value => {
            edit.value.handle_key(&key);
        }
        _ => {}
    }
    Ok(())
}

fn commit_save(app: &mut App) {
    let Some(plan) = build_save_plan(app) else {
        return;
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::EsvSave(plan));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_save_plan(app, plan, false);
}

fn build_save_plan(app: &mut App) -> Option<SavePlan> {
    let tenant_name = app.active_tenant().map(|t| t.name.clone())?;
    let edit = app.esv.editing.as_mut()?;

    if edit.creating {
        let id = edit.id_input.value.trim().to_string();
        // The `esv-` prefix is locked in the field, so the only id problem the
        // user can still hit is leaving the suffix empty.
        if id == "esv-" || id.is_empty() {
            edit.error = Some("Give the variable a name after 'esv-'".into());
            return None;
        }
        if !id.starts_with("esv-") {
            edit.error = Some("_id must start with 'esv-'".into());
            return None;
        }
        edit.id = id;
    }

    // A variable value must be non-empty: base64 of "" is "", which AIC
    // rejects (and a rejected create leaves a confusing local-only row).
    // A single space is a valid, non-empty value.
    if edit.value.value.is_empty() {
        edit.error = Some("Value cannot be empty (a single space is allowed)".into());
        return None;
    }

    // Pre-flight validation. Catches obvious type/value mismatches before
    // we apply optimistically and ship a request that would just bounce.
    if let Err(msg) = edit.expr_type.validate(&edit.value.value) {
        edit.error = Some(msg);
        return None;
    }

    let id = edit.id.clone();
    let description = edit.description.value.clone();
    let expr_type = edit.expr_type.as_str().to_string();
    let value_str = edit.value.value.clone();
    let value_b64 = B64.encode(value_str.as_bytes());
    let creating = edit.creating;
    let was_creating = edit.creating;
    let original_for_conflict = if creating {
        None
    } else {
        Some(edit.original.clone())
    };

    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), id.clone()))
    {
        edit.error = Some("Save already in progress for this variable".into());
        return None;
    }

    // Build the optimistic body the local list will show until the
    // server's echo lands. Server-managed fields are inherited from the
    // snapshot when editing, and stubbed for creates.
    let mut optimistic = if creating {
        serde_json::json!({})
    } else {
        edit.original.clone()
    };
    optimistic["_id"] = serde_json::Value::String(id.clone());
    optimistic["description"] = serde_json::Value::String(description.clone());
    optimistic["expressionType"] = serde_json::Value::String(expr_type.clone());
    optimistic["valueBase64"] = serde_json::Value::String(value_b64.clone());
    // We don't know the new lastChangeDate yet; stamp a placeholder so
    // it doesn't look like the previous edit was just now.
    optimistic["lastChangeDate"] = serde_json::Value::String("(saving…)".into());
    // The runtime hasn't picked it up yet — restart is pending until the
    // user triggers one. Holds for both edits and creates.
    optimistic["loaded"] = serde_json::Value::Bool(false);

    Some(SavePlan {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original: original_for_conflict,
        optimistic,
        was_creating,
    })
}

pub fn execute_save_plan(app: &mut App, plan: SavePlan, confirmed_prod: bool) {
    let SavePlan {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
        optimistic,
        was_creating,
    } = plan;

    if let Err(e) = record_save_undo(
        app,
        &tenant_name,
        &id,
        original.as_ref(),
        &optimistic,
        was_creating,
    ) {
        app.push_toast(
            ToastKind::Error,
            format!("Save cancelled: failed to record undo — {e}"),
        );
        return;
    }

    // Apply locally + pin across polls.
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant_name) {
        if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
            *slot = optimistic.clone();
        } else {
            items.push(optimistic.clone());
        }
    }
    app.esv.recent_writes.insert(
        (tenant_name.clone(), id.clone()),
        (Instant::now(), optimistic),
    );
    app.esv
        .recent_deletes
        .remove(&(tenant_name.clone(), id.clone()));
    // Mark the background PUT as in flight so the banner can flip
    // purple and ^S is gated until it returns.
    app.esv
        .in_flight_writes
        .insert((tenant_name.clone(), id.clone()));

    // Close the form. The actual save runs in the background; result
    // events arrive via `apply_save_result` and either silently refresh
    // the entry with the server's echo or toast an error.
    app.esv.editing = None;
    app.input_mode = InputMode::Normal;
    // Jump to the new row if we just created one.
    if was_creating {
        let matches = app.esv.matches(Some(&tenant_name));
        if let Some(pos) = matches.iter().position(|m| m.id == id) {
            app.esv.list.selected = pos;
        }
    }

    let request = SaveRequest {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
    };
    let event_tenant = request.tenant_name.clone();
    let event_id = request.id.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = save_variable(request, confirmed_prod).await;
        let _ = tx.send(AppEvent::EsvSaveResult {
            tenant: event_tenant,
            id: event_id,
            result,
        });
    });
}

fn record_save_undo(
    app: &mut App,
    tenant_name: &str,
    id: &str,
    original: Option<&serde_json::Value>,
    optimistic: &serde_json::Value,
    was_creating: bool,
) -> crate::Result<UndoId> {
    let entry = if was_creating {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Delete created variable {id}"),
            Sensitivity::PublicMetadata,
            Capability::Undoable,
            Some(UndoOp::EsvVariableDelete {
                tenant: tenant_name.to_string(),
                id: id.to_string(),
                recorded_body: optimistic.clone(),
            }),
            ConflictCheck::ContentEqualsAfter {
                body: optimistic.clone(),
            },
        )
    } else if let Some(original) = original {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Revert {id} to previous value"),
            Sensitivity::PublicMetadata,
            Capability::Undoable,
            Some(UndoOp::EsvVariableUpdateTo {
                tenant: tenant_name.to_string(),
                id: id.to_string(),
                body: original.clone(),
            }),
            ConflictCheck::ContentEqualsAfter {
                body: optimistic.clone(),
            },
        )
    } else {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Changed {id}"),
            Sensitivity::PublicMetadata,
            Capability::Irreversible,
            None,
            ConflictCheck::None,
        )
    };
    app.undo.record(entry)
}

pub fn execute_delete_plan(app: &mut App, plan: DeletePlan, confirmed_prod: bool) {
    let DeletePlan {
        tenant_name,
        id,
        original,
    } = plan;

    if let Err(e) = record_delete_undo(app, &tenant_name, &id, &original) {
        app.push_toast(
            ToastKind::Error,
            format!("Delete cancelled: failed to record undo — {e}"),
        );
        app.input_mode = InputMode::Normal;
        return;
    }

    let mut remaining = None;
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant_name) {
        items.retain(|v| id_of(v) != id);
        remaining = Some(items.len());
    }
    if let Some(n) = remaining {
        app.esv.clamp_selection(n);
    }
    app.esv
        .recent_writes
        .remove(&(tenant_name.clone(), id.clone()));
    app.esv.recent_deletes.insert(
        (tenant_name.clone(), id.clone()),
        DeleteTombstone {
            deleted_at: Instant::now(),
            body: original.clone(),
        },
    );
    select_id(app, &tenant_name, &id);
    app.esv
        .failed_writes
        .remove(&(tenant_name.clone(), id.clone()));
    app.esv
        .in_flight_writes
        .insert((tenant_name.clone(), id.clone()));
    app.esv
        .in_flight_deletes
        .insert((tenant_name.clone(), id.clone()), original.clone());
    app.input_mode = InputMode::Normal;

    let request = DeleteRequest {
        tenant_name,
        id,
        original,
    };
    let event_tenant = request.tenant_name.clone();
    let event_id = request.id.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = delete_variable_request(request, confirmed_prod).await;
        let _ = tx.send(AppEvent::EsvDeleteResult {
            tenant: event_tenant,
            id: event_id,
            result,
        });
    });
}

fn record_delete_undo(
    app: &mut App,
    tenant_name: &str,
    id: &str,
    original: &serde_json::Value,
) -> crate::Result<UndoId> {
    app.undo.record(UndoEntry::pending(
        tenant_name.to_string(),
        "esv",
        format!("Restore deleted variable {id}"),
        Sensitivity::PublicMetadata,
        Capability::Undoable,
        Some(UndoOp::EsvVariableRestore {
            tenant: tenant_name.to_string(),
            body: original.clone(),
        }),
        ConflictCheck::ResourceAbsent,
    ))
}

/// Outcome of a background save attempt. Carries the server's echo on
/// success so the local cache can absorb server-managed fields
/// (`lastChangeDate`, `lastChangedBy`).
#[derive(Debug)]
pub struct SaveOutcome {
    pub body: serde_json::Value,
    /// True when we opened the form as an "edit" but the variable was
    /// absent on AIC (a prior create failed), so the save became a create.
    pub created: bool,
}

#[derive(Debug)]
pub struct DeleteOutcome;

#[derive(Debug)]
pub struct UndoOutcome {
    description: String,
    applied: UndoApplied,
}

#[derive(Debug)]
enum UndoApplied {
    Upsert {
        id: String,
        body: serde_json::Value,
    },
    Delete {
        id: String,
        body: Option<serde_json::Value>,
    },
    /// A secret was deleted as the undo of its create. The secrets list owns
    /// the local-state update (variables and secrets are cached separately).
    SecretRemoved {
        id: String,
    },
    /// A secret's description was reverted. Just re-poll the secrets list.
    SecretDescriptionSet,
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let Some(summary) = app.undo.latest_pending(&tenant_name) else {
        app.push_toast(ToastKind::Info, "Nothing to undo for this tenant");
        return;
    };

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::EsvUndo(summary.id));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_undo(app, summary.id, false);
    }
}

pub fn execute_undo(app: &mut App, undo_id: UndoId, confirmed_prod: bool) {
    let entry = match app.undo.load(undo_id) {
        Ok(entry) => entry,
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Undo failed: {e}"));
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

    let event_tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::EsvUndoResult {
            undo_id,
            tenant: event_tenant,
            result,
        });
    });
}

async fn save_variable(request: SaveRequest, confirmed_prod: bool) -> Result<SaveOutcome, String> {
    let SaveRequest {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
    } = request;

    // Conflict check (against the snapshot we opened), the type-change
    // DELETE-then-PUT quirk, and create-on-absent all live in the shared
    // helper so the CLI takes exactly the same path.
    let saved = crate::aic::esv::save_variable(
        &tenant_name,
        &id,
        &description,
        &expr_type,
        &value_b64,
        confirmed_prod,
        original.as_ref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(SaveOutcome {
        body: saved.body,
        created: saved.created,
    })
}

async fn delete_variable_request(
    request: DeleteRequest,
    confirmed_prod: bool,
) -> Result<DeleteOutcome, String> {
    let DeleteRequest {
        tenant_name,
        id,
        original,
    } = request;

    let current = crate::aic::esv::get_variable(&tenant_name, &id)
        .await
        .map_err(|e| format!("conflict check: {e}"))?;
    if !crate::aic::esv::content_equal(&current, &original) {
        return Err("remote value changed since you selected it; refresh and retry".into());
    }
    crate::aic::esv::delete_variable(&tenant_name, &id, confirmed_prod)
        .await
        .map_err(|e| e.to_string())?;
    Ok(DeleteOutcome)
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> Result<UndoOutcome, UndoFailure> {
    let op = entry
        .op
        .clone()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no operation".into()))?;
    check_undo_conflict(&op, &entry.conflict_check).await?;

    match op {
        UndoOp::EsvVariableRestore { tenant, body } => {
            let id = body_id(&body)?;
            let saved = upsert_variable_body(&tenant, &body, confirmed_prod).await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Upsert { id, body: saved },
            })
        }
        UndoOp::EsvVariableUpdateTo { tenant, id, body } => {
            let saved = upsert_variable_body(&tenant, &body, confirmed_prod).await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Upsert { id, body: saved },
            })
        }
        UndoOp::EsvVariableDelete {
            tenant,
            id,
            recorded_body,
        } => {
            crate::aic::esv::delete_variable(&tenant, &id, confirmed_prod)
                .await
                .map_err(|e| UndoFailure::Failed(e.to_string()))?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Delete {
                    id,
                    body: Some(recorded_body),
                },
            })
        }
        UndoOp::SecretDelete {
            tenant,
            id,
            active_version,
        } => {
            crate::screens::secret::undo_delete(&tenant, &id, &active_version, confirmed_prod)
                .await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::SecretRemoved { id },
            })
        }
        UndoOp::SecretSetDescription {
            tenant,
            id,
            previous,
            expected,
        } => {
            crate::screens::secret::undo_set_description(
                &tenant,
                &id,
                &previous,
                &expected,
                confirmed_prod,
            )
            .await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::SecretDescriptionSet,
            })
        }
    }
}

async fn check_undo_conflict(op: &UndoOp, check: &ConflictCheck) -> Result<(), UndoFailure> {
    match check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => {
            let tenant = op.tenant();
            let id = op
                .resource_id()
                .ok_or_else(|| UndoFailure::Failed("undo operation has no resource id".into()))?;
            let current = crate::aic::esv::get_variable(tenant, id)
                .await
                .map_err(|e| UndoFailure::Conflict(format!("current value unavailable: {e}")))?;
            if crate::aic::esv::content_equal(&current, body) {
                Ok(())
            } else {
                Err(UndoFailure::Conflict(
                    "remote value changed since the original write".into(),
                ))
            }
        }
        ConflictCheck::ResourceAbsent => {
            let tenant = op.tenant();
            let id = op
                .resource_id()
                .ok_or_else(|| UndoFailure::Failed("undo operation has no resource id".into()))?;
            match crate::aic::esv::get_variable(tenant, id).await {
                Ok(_) => Err(UndoFailure::Conflict(format!(
                    "{id} already exists; refusing to restore over it"
                ))),
                Err(e) if is_not_found(&e) => Ok(()),
                Err(e) => Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
            }
        }
        ConflictCheck::None => Ok(()),
    }
}

async fn upsert_variable_body(
    tenant: &str,
    body: &serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value, UndoFailure> {
    let id = body_id(body)?;
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expression_type = body
        .get("expressionType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UndoFailure::Failed(format!("{id} has no expressionType")))?;
    let value_base64 = body
        .get("valueBase64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UndoFailure::Failed(format!("{id} has no valueBase64")))?;

    let delete_first = match crate::aic::esv::get_variable(tenant, &id).await {
        Ok(current) => {
            current
                .get("expressionType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                != expression_type
        }
        Err(e) if is_not_found(&e) => false,
        Err(e) => return Err(UndoFailure::Failed(format!("preflight fetch failed: {e}"))),
    };
    if delete_first {
        crate::aic::esv::delete_variable(tenant, &id, confirmed_prod)
            .await
            .map_err(|e| UndoFailure::Failed(format!("type change delete failed: {e}")))?;
    }

    crate::aic::esv::update_variable(
        tenant,
        &id,
        description,
        expression_type,
        value_base64,
        confirmed_prod,
    )
    .await
    .map_err(|e| UndoFailure::Failed(e.to_string()))
}

fn body_id(body: &serde_json::Value) -> Result<String, UndoFailure> {
    body.get("_id")
        .and_then(|v| v.as_str())
        .map(|id| id.to_string())
        .ok_or_else(|| UndoFailure::Failed("undo body has no _id".into()))
}

fn is_not_found(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Api { status: 404, .. })
}

/// Background save finished. The edit form was already closed and the
/// list already shows the optimistic body — this just replaces the
/// optimistic placeholder with the server's echo (so `lastChangeDate`
/// and friends become real) or surfaces an error toast.
pub fn apply_save_result(
    app: &mut App,
    tenant: String,
    id: String,
    result: Result<SaveOutcome, String>,
) {
    // The background PUT has returned — clear the in-flight marker so
    // the "queued" banner can drop and ^S unlocks. Done regardless of
    // success or failure; failures are tracked separately in
    // `failed_writes`.
    app.esv
        .in_flight_writes
        .remove(&(tenant.clone(), id.clone()));
    match result {
        Ok(SaveOutcome { body, created }) => {
            if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                    *slot = body.clone();
                }
            }
            // Refresh the pin so the new server-echoed body survives
            // the next poll's eventual-consistency window.
            app.esv
                .recent_writes
                .insert((tenant.clone(), id.clone()), (Instant::now(), body));
            app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
            // Clear any prior failure marker — the save went through.
            app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
            refresh_apply_state_from_cache(app, &tenant);
            let msg = if created {
                format!("{id} was missing on AIC — created it. Press ^Z to undo.")
            } else {
                "Saved ESV. Press ^Z to undo.".to_string()
            };
            app.push_toast(ToastKind::Success, msg);
        }
        Err(e) => {
            // Keep the optimistic body in `recent_writes` so the user
            // doesn't lose their attempted edit, and flag the row so the
            // list highlights it red.
            app.esv.failed_writes.insert((tenant, id.clone()));
            app.push_toast(ToastKind::Error, format!("Save failed: {id} — {e}"));
        }
    }
}

pub fn apply_delete_result(
    app: &mut App,
    tenant: String,
    id: String,
    result: Result<DeleteOutcome, String>,
) {
    app.esv
        .in_flight_writes
        .remove(&(tenant.clone(), id.clone()));
    let original = app
        .esv
        .in_flight_deletes
        .remove(&(tenant.clone(), id.clone()));

    match result {
        Ok(DeleteOutcome) => {
            app.esv.recent_writes.remove(&(tenant.clone(), id.clone()));
            app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(
                ToastKind::Success,
                format!("Deleted {id}. Press ^Z to undo."),
            );
        }
        Err(e) => {
            app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
            if let Some(original) = original {
                if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                    if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                        *slot = original.clone();
                    } else {
                        items.push(original.clone());
                    }
                }
            }
            app.push_toast(ToastKind::Error, format!("Delete failed: {id} — {e}"));
        }
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
            applied,
        }) => {
            if let Err(e) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {e}"),
                );
            }
            match applied {
                UndoApplied::Upsert { id, body } => {
                    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                        if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                            *slot = body.clone();
                        } else {
                            items.push(body.clone());
                        }
                    }
                    app.esv
                        .recent_writes
                        .insert((tenant.clone(), id.clone()), (Instant::now(), body));
                    app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
                    app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
                    select_id(app, &tenant, &id);
                }
                UndoApplied::Delete { id, body } => {
                    let mut remaining = None;
                    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                        items.retain(|v| id_of(v) != id);
                        remaining = Some(items.len());
                    }
                    if let Some(n) = remaining {
                        app.esv.clamp_selection(n);
                    }
                    app.esv.recent_writes.remove(&(tenant.clone(), id.clone()));
                    if let Some(body) = body {
                        app.esv.recent_deletes.insert(
                            (tenant.clone(), id.clone()),
                            DeleteTombstone {
                                deleted_at: Instant::now(),
                                body,
                            },
                        );
                    }
                    app.esv.failed_writes.remove(&(tenant.clone(), id));
                }
                UndoApplied::SecretRemoved { id } => {
                    // Drop the secret locally and re-poll so the list + pending
                    // state reflect the removal.
                    if let Some(LoadState::Loaded(items)) = app.secret.list.data.get_mut(&tenant) {
                        items.retain(|v| id_of(v) != id);
                    }
                    refresh(app, true);
                }
                UndoApplied::SecretDescriptionSet => {
                    // The description lives only on the server object; re-poll
                    // to pull the reverted value back into the cache.
                    refresh(app, true);
                }
            }
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(ToastKind::Success, format!("Undone: {description}"));
        }
        Err(UndoFailure::Conflict(message)) => {
            app.push_toast(ToastKind::Warning, format!("Undo conflict: {message}"));
        }
        Err(UndoFailure::Failed(message)) => {
            if let Err(e) = app.undo.mark_applied(undo_id, EntryStatus::AppliedFailure) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo failure log update failed: {e}"),
                );
            }
            app.push_toast(ToastKind::Error, format!("Undo failed: {message}"));
        }
    }
}

fn select_id(app: &mut App, tenant: &str, id: &str) {
    let matches = app.esv.matches(Some(tenant));
    if let Some(pos) = matches.iter().position(|m| m.id == id) {
        app.esv.list.selected = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_apply_state_prefers_startup_restarting() {
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Restarting, 0),
            ApplyState::Restarting(0)
        );
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Restarting, 3),
            ApplyState::Restarting(3)
        );
    }

    #[test]
    fn authoritative_apply_state_uses_pending_when_ready() {
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Ready, 0),
            ApplyState::NoChanges
        );
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Ready, 2),
            ApplyState::Unapplied(2)
        );
    }
}
