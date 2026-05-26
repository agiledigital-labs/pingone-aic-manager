//! ESV tab — list, fuzzy search, preview. The state struct lives on `App`
//! as a single field (`app.esv`); handlers are free functions that take
//! `&mut App` so the dispatch table in `app.rs` stays one-liner per arm.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode};
use crate::event::{AppEvent, ToastKind};
use crate::ui::widgets::{LineEditor, TextField};

/// Per-tenant ESV load state. `app.esv.data` maps tenant name → this.
#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Vec<serde_json::Value>),
    Failed(String),
}

/// One row in the rendered ESV list. The UI consumes `Vec<Match>` from
/// [`State::matches`]; sorted-by-score with match positions in `_id` for
/// per-char highlight.
#[derive(Debug, Clone)]
pub struct Match {
    pub idx: usize,
    pub id: String,
    pub score: u32,
    pub positions: Vec<u32>,
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
pub fn pending_count(app: &App, tenant: &str) -> usize {
    match app.esv.data.get(tenant) {
        Some(LoadState::Loaded(items)) => items.iter().filter(|v| is_pending(v)).count(),
        _ => 0,
    }
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
            ExpressionType::Object => match serde_json::from_str::<serde_json::Value>(value.trim()) {
                Ok(serde_json::Value::Object(_)) => Ok(()),
                Ok(_) => Err("Value must be a JSON object (e.g. {\"k\":\"v\"})".into()),
                Err(e) => Err(format!("Value must be valid JSON: {e}")),
            },
            ExpressionType::Array => match serde_json::from_str::<serde_json::Value>(value.trim()) {
                Ok(serde_json::Value::Array(_)) => Ok(()),
                Ok(_) => Err("Value must be a JSON array (e.g. [1,2,3])".into()),
                Err(e) => Err(format!("Value must be valid JSON: {e}")),
            },
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

#[derive(Debug)]
pub struct State {
    /// Cached ESV lists, keyed by tenant name. Populated lazily on first
    /// visit; refreshed on a 30s tick.
    pub data: HashMap<String, LoadState>,
    /// Tenants whose ESV refetch is currently in flight — guards against
    /// duplicate spawns when the user re-enters the tab or the poll fires.
    pub refreshing: HashSet<String>,
    /// When the last poll-refresh ran. Drives the 30s cadence in `tick`.
    pub last_poll: Instant,

    /// Fuzzy search query + cursor. Empty = show everything. Updated while
    /// in `InputMode::EsvSearch`; persists after Enter so the filter stays
    /// applied. Cleared on tenant switch + on Esc-from-search.
    pub query: LineEditor,
    /// Index into the filtered list. Always clamped to len-1 (or 0 when
    /// the filter eliminates everything).
    pub selected: usize,
    /// First visible row — drives windowed rendering.
    pub scroll: usize,
    /// In-flight edit. `None` = preview pane (read-only display). `Some`
    /// = the right pane is the editable form. See `start_edit` / `cancel_edit`.
    pub editing: Option<EditState>,

    /// Variables we just saved that may not yet appear in a polled list
    /// (AIC's `/environment/variables` is eventually consistent — a brand-
    /// new variable can take a few seconds to show up). Keyed by
    /// `(tenant, id)`; each value is `(saved_at, body)`. On every
    /// `apply_listed` we re-merge any entries still within
    /// `RECENT_WRITE_TTL` so the local view never loses them.
    pub recent_writes: HashMap<(String, String), (Instant, serde_json::Value)>,

    /// (tenant, id) for any optimistic save whose background request
    /// failed. The local cache still holds the user's attempted body
    /// (via `recent_writes`); the list renders these rows in red so the
    /// user can re-open and retry. Cleared on a subsequent successful
    /// save for the same id.
    pub failed_writes: HashSet<(String, String)>,

    /// Tenants we've just triggered a restart on, keyed by name. The
    /// banner flips from "to apply" to "applying" while an entry is
    /// present. Cleared when a polled list reports `pending_count == 0`
    /// (the runtime caught up) or after `RESTART_TIMEOUT` as a safety
    /// net for failures we didn't otherwise observe.
    pub restart_in_progress: HashMap<String, Instant>,

    /// (tenant, id) for every optimistic save whose background PUT
    /// hasn't returned yet. Drives the "queued" banner colour and gates
    /// the `^S` apply keybind — we don't want to restart while a write
    /// is in flight. Cleared by `apply_save_result` regardless of
    /// success / failure.
    pub in_flight_writes: HashSet<(String, String)>,
}

/// How long the "applying" banner state stays sticky in the absence of
/// a polled list confirming completion. Tenant restarts are documented
/// to take "a few minutes"; ten minutes is a comfortable upper bound.
pub const RESTART_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// How long we keep a freshly-saved variable pinned to the local cache
/// after a save. Long enough to cover AIC's indexing lag on creates;
/// short enough that genuinely-deleted-elsewhere variables eventually
/// drop off after the next poll confirms their absence.
pub const RECENT_WRITE_TTL: std::time::Duration = std::time::Duration::from_secs(120);

impl Default for State {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            last_poll: Instant::now(),
            query: LineEditor::new(),
            selected: 0,
            scroll: 0,
            editing: None,
            recent_writes: HashMap::new(),
            failed_writes: HashSet::new(),
            restart_in_progress: HashMap::new(),
            in_flight_writes: HashSet::new(),
        }
    }
}

/// Banner display mode — picks the colour and the wording. Computed
/// fresh on every draw from the three sources of truth: restart-in-
/// progress, in-flight saves, and `loaded=false` counts.
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
    app.esv
        .in_flight_writes
        .iter()
        .filter(|(t, _)| t == tenant)
        .count()
}

/// Pick which banner (if any) to display. Precedence: applying > queued
/// > to-apply. The applying state wins even when queued > 0 because a
/// restart in progress is the more important signal.
pub fn banner_state(app: &App, tenant: &str) -> BannerState {
    if is_applying(app, tenant) {
        return BannerState::Applying(pending_count(app, tenant));
    }
    let q = queued_count(app, tenant);
    if q > 0 {
        return BannerState::Queued(q);
    }
    let p = pending_count(app, tenant);
    if p > 0 {
        return BannerState::ToApply(p);
    }
    BannerState::None
}

/// True when the "applying" banner state should be shown — i.e. the user
/// triggered a restart recently and it hasn't been confirmed complete
/// (by a polled list returning zero pending changes) or timed out.
pub fn is_applying(app: &App, tenant: &str) -> bool {
    app.esv
        .restart_in_progress
        .get(tenant)
        .map(|started_at| started_at.elapsed() < RESTART_TIMEOUT)
        .unwrap_or(false)
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop view state (filter + selection). Called on tenant switch.
    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Apply the fuzzy filter to a tenant's loaded ESV list. Returns
    /// matches sorted by score (descending), with match positions for
    /// per-char highlighting. Empty when the tenant isn't `Loaded`.
    pub fn matches(&self, tenant: Option<&str>) -> Vec<Match> {
        let Some(name) = tenant else { return Vec::new() };
        let Some(LoadState::Loaded(items)) = self.data.get(name) else {
            return Vec::new();
        };
        if self.query.is_empty() {
            let mut indexed: Vec<Match> = items
                .iter()
                .enumerate()
                .map(|(i, v)| Match {
                    idx: i,
                    id: id_of(v).to_string(),
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
            indexed.sort_by(|a, b| a.id.cmp(&b.id));
            return indexed;
        }
        use nucleo_matcher::{
            pattern::{AtomKind, CaseMatching, Normalization, Pattern},
            Config, Matcher, Utf32Str,
        };
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::new(
            self.query.value(),
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut out: Vec<Match> = Vec::new();
        let mut buf = Vec::new();
        let mut positions: Vec<u32> = Vec::new();
        for (i, v) in items.iter().enumerate() {
            let id = id_of(v);
            // Build the fuzzy haystack from `id` plus synthetic tags so
            // `/!`, `/!pending`, `/!failed` filter to those rows. The
            // tags are stripped from the highlight positions so the id
            // renders without spurious tag chars.
            let pending = is_pending(v);
            let failed = self
                .failed_writes
                .contains(&(name.to_string(), id.to_string()));
            let mut haystack_text = id.to_string();
            if pending {
                haystack_text.push_str(" !pending");
            }
            if failed {
                haystack_text.push_str(" !failed");
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
                    idx: i,
                    id: id.to_string(),
                    score,
                    positions: display_positions,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// Re-clamp the selection in case the matched list shrunk (e.g. after
    /// a refetch returns a shorter list, or after the filter changed).
    pub fn clamp_selection(&mut self, n: usize) {
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
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
pub fn refresh(app: &mut App, force: bool) {
    if !app.is_unlocked() {
        return;
    }
    let Some(tenant) = app.active_tenant() else { return };
    let name = tenant.name.clone();
    if app.esv.refreshing.contains(&name) {
        return;
    }
    if !force && app.esv.data.contains_key(&name) {
        return;
    }

    // Only show the Loading spinner when there's nothing cached yet;
    // refetches keep the previous Loaded entry visible.
    if !app.esv.data.contains_key(&name) {
        app.esv.data.insert(name.clone(), LoadState::Loading);
    }
    app.esv.refreshing.insert(name.clone());
    app.esv.last_poll = Instant::now();

    let tx = app.events.tx.clone();
    let tenant_name = name.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::list_variables(&tenant_name)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::EsvListed { tenant: name, result });
    });
}

/// ESV-tab keys while in Normal mode. Returns `true` if the key was
/// consumed (skip the global key table) and `false` to fall through.
pub fn handle_normal_key(app: &mut App, key: KeyEvent) -> bool {
    let n = app.esv.matches(app.active_tenant().map(|t| t.name.as_str())).len();
    match key.code {
        KeyCode::Char('/') => {
            app.input_mode = InputMode::EsvSearch;
            true
        }
        KeyCode::Esc if !app.esv.query.is_empty() => {
            app.esv.reset_view();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if n > 0 && app.esv.selected + 1 < n {
                app.esv.selected += 1;
            }
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.esv.selected > 0 {
                app.esv.selected -= 1;
            }
            true
        }
        KeyCode::PageDown => {
            app.esv.selected = (app.esv.selected + 10).min(n.saturating_sub(1));
            true
        }
        KeyCode::PageUp => {
            app.esv.selected = app.esv.selected.saturating_sub(10);
            true
        }
        KeyCode::Char('g') => {
            app.esv.selected = 0;
            true
        }
        KeyCode::Char('G') => {
            app.esv.selected = n.saturating_sub(1);
            true
        }
        KeyCode::Enter if n > 0 => {
            start_edit(app);
            true
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            start_create(app);
            true
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            request_restart(app);
            true
        }
        _ => false,
    }
}

/// Open the restart-confirm popup if there's anything to apply, the
/// background saves have caught up, and a restart isn't already in
/// flight. Each negative case gets its own info toast so the user can
/// see why the keystroke was a no-op.
fn request_restart(app: &mut App) {
    let Some(tenant_name) = app.active_tenant().map(|t| t.name.clone()) else { return };
    if is_applying(app, &tenant_name) {
        app.push_toast(
            crate::event::ToastKind::Info,
            "Restart already in progress",
        );
        return;
    }
    if queued_count(app, &tenant_name) > 0 {
        // Purple banner already tells the user a save is in flight;
        // swallow ^S silently rather than stacking a contradicting toast.
        return;
    }
    if pending_count(app, &tenant_name) == 0 {
        app.push_toast(crate::event::ToastKind::Info, "No pending changes to apply");
        return;
    }
    app.input_mode = InputMode::EsvRestartConfirm;
}

/// Dispatched from the y/n popup. `y` triggers the restart, `n`/`Esc`
/// closes the popup.
pub fn handle_restart_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            trigger_restart(app);
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

fn trigger_restart(app: &mut App) {
    let Some(tenant_name) = app.active_tenant().map(|t| t.name.clone()) else { return };
    // Flip the banner to its "applying" state immediately so the user
    // sees their click registered. Cleared on the next polled list
    // that reports zero pending (`apply_listed`) or after the safety
    // timeout in `RESTART_TIMEOUT`.
    app.esv
        .restart_in_progress
        .insert(tenant_name.clone(), Instant::now());
    app.push_toast(
        crate::event::ToastKind::Info,
        "Restart triggered — runtime will pick up changes in a few minutes",
    );
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::aic::esv::trigger_restart(&tenant_name, false)
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
            app.esv.restart_in_progress.remove(&tenant);
            app.push_toast(crate::event::ToastKind::Error, format!("Restart failed: {e}"));
        }
    }
}

/// EsvSearch keys: chars/backspace/cursor → editor; ↑/↓/PgUp/PgDn keep
/// scrolling the results list while the user is still typing; Esc clears
/// the filter; Enter commits and returns to Normal mode.
pub fn handle_search_key(app: &mut App, key: KeyEvent) {
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
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
            handle_normal_key(app, key);
            return;
        }
        _ => {}
    }
    let before = app.esv.query.value().to_string();
    if app.esv.query.handle_key(&key) && app.esv.query.value() != before {
        app.esv.selected = 0;
        app.esv.scroll = 0;
    }
}

/// Apply an `EsvListed` event to the tab. Caller is `app::handle_event`.
pub fn apply_listed(
    app: &mut App,
    tenant: String,
    result: std::result::Result<Vec<serde_json::Value>, String>,
) {
    let is_active = app.active_tenant().is_some_and(|t| t.name == tenant);
    app.esv.refreshing.remove(&tenant);
    match result {
        Ok(mut vs) => {
            // Re-merge any entries we recently saved but the polled list
            // hasn't picked up yet (AIC's variable-list endpoint is
            // eventually consistent — a brand-new variable can lag by a
            // few seconds). Drop expired entries while we're here so
            // they don't pin removed variables forever.
            app.esv
                .recent_writes
                .retain(|_, (saved_at, _)| saved_at.elapsed() < RECENT_WRITE_TTL);
            for ((t, recent_id), (_, body)) in app.esv.recent_writes.iter() {
                if t != &tenant {
                    continue;
                }
                if !vs.iter().any(|v| id_of(v) == recent_id) {
                    vs.push(body.clone());
                }
            }
            // If the runtime has caught up (no more `loaded: false`
            // entries) and we'd recently triggered a restart, clear the
            // sticky "applying" state so the banner drops off.
            let still_pending = vs.iter().any(is_pending);
            if !still_pending {
                app.esv.restart_in_progress.remove(&tenant);
            }
            app.esv.data.insert(tenant, LoadState::Loaded(vs));
            if is_active {
                let n = app.esv.matches(app.active_tenant().map(|t| t.name.as_str())).len();
                app.esv.clamp_selection(n);
            }
        }
        Err(e) => {
            // Don't clobber a previously-cached list with a background-
            // refresh failure — keep showing the stale data and just log.
            if matches!(app.esv.data.get(&tenant), Some(LoadState::Loaded(_))) {
                tracing::warn!("ESV refresh failed for {tenant}: {e}");
            } else {
                app.esv.data.insert(tenant, LoadState::Failed(e));
            }
        }
    }
}

/// Open the edit form for the currently-selected list row. Snapshots the
/// variable so we have something to diff against on save, decodes the
/// base64 value, and switches input mode.
pub fn start_edit(app: &mut App) {
    let Some(tenant) = app.active_tenant() else { return };
    let tenant_name = tenant.name.clone();
    let matches = app.esv.matches(Some(&tenant_name));
    let Some(m) = matches.get(app.esv.selected) else { return };
    let Some(LoadState::Loaded(items)) = app.esv.data.get(&tenant_name) else { return };
    let Some(v) = items.get(m.idx).cloned() else { return };

    let description = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let expr_type = ExpressionType::parse(
        v.get("expressionType").and_then(|x| x.as_str()).unwrap_or(""),
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
        // Pre-seed the conventional `esv-` prefix so the user doesn't
        // have to remember it. They can backspace if they want a
        // different shape.
        id_input: TextField::single_line("_id").with_initial("esv-"),
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
    let Some(edit) = app.esv.editing.as_mut() else { return Ok(()) };
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
    let Some(tenant_name) = app.active_tenant().map(|t| t.name.clone()) else { return };
    let Some(edit) = app.esv.editing.as_mut() else { return };

    if edit.creating {
        let id = edit.id_input.value.trim().to_string();
        if id.is_empty() {
            edit.error = Some("_id cannot be empty".into());
            return;
        }
        if !id.starts_with("esv-") {
            edit.error = Some("_id must start with 'esv-'".into());
            return;
        }
        edit.id = id;
    }

    // Pre-flight validation. Catches obvious type/value mismatches before
    // we apply optimistically and ship a request that would just bounce.
    if let Err(msg) = edit.expr_type.validate(&edit.value.value) {
        edit.error = Some(msg);
        return;
    }

    let id = edit.id.clone();
    let description = edit.description.value.clone();
    let expr_type = edit.expr_type.as_str().to_string();
    let original_type = edit
        .original
        .get("expressionType")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let value_str = edit.value.value.clone();
    let value_b64 = B64.encode(value_str.as_bytes());
    let creating = edit.creating;
    let was_creating = edit.creating;
    let type_changed = !creating && original_type != expr_type;

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

    // Apply locally + pin across polls.
    if let Some(LoadState::Loaded(items)) = app.esv.data.get_mut(&tenant_name) {
        if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
            *slot = optimistic.clone();
        } else {
            items.push(optimistic.clone());
        }
    }
    app.esv
        .recent_writes
        .insert((tenant_name.clone(), id.clone()), (Instant::now(), optimistic));
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
            app.esv.selected = pos;
        }
    }

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            save_variable(&tenant_name, &id, &description, &expr_type, &value_b64, type_changed)
                .await;
        let _ = tx.send(AppEvent::EsvSaveResult {
            tenant: tenant_name,
            id,
            result,
        });
    });
}

/// Outcome of a background save attempt. Carries the server's echo on
/// success so the local cache can absorb server-managed fields
/// (`lastChangeDate`, `lastChangedBy`).
#[derive(Debug)]
pub struct SaveOutcome {
    pub body: serde_json::Value,
}

async fn save_variable(
    tenant: &str,
    id: &str,
    description: &str,
    expr_type: &str,
    value_b64: &str,
    type_changed: bool,
) -> Result<SaveOutcome, String> {
    if type_changed {
        // AIC rejects in-place type changes on existing variables, but
        // accepts DELETE followed by an immediate PUT with the new type
        // — verified on the sandbox 2026-05-26, no restart between.
        crate::aic::esv::delete_variable(tenant, id, false)
            .await
            .map_err(|e| format!("type change: delete failed: {e}"))?;
    }
    let saved = crate::aic::esv::update_variable(tenant, id, description, expr_type, value_b64, false)
        .await
        .map_err(|e| {
            if type_changed {
                format!(
                    "type change: delete succeeded but recreate failed ({e}). \
                     Variable is currently absent on AIC; re-save to recreate."
                )
            } else {
                e.to_string()
            }
        })?;
    Ok(SaveOutcome { body: saved })
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
    app.esv.in_flight_writes.remove(&(tenant.clone(), id.clone()));
    match result {
        Ok(SaveOutcome { body }) => {
            if let Some(LoadState::Loaded(items)) = app.esv.data.get_mut(&tenant) {
                if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                    *slot = body.clone();
                }
            }
            // Refresh the pin so the new server-echoed body survives
            // the next poll's eventual-consistency window.
            app.esv
                .recent_writes
                .insert((tenant.clone(), id.clone()), (Instant::now(), body));
            // Clear any prior failure marker — the save went through.
            app.esv.failed_writes.remove(&(tenant, id));
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
