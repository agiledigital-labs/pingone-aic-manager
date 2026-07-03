//! Scripts tab — browse every script the sync engine knows about (AM scripts,
//! IDM endpoints/schedules, managed-object hooks, and sync-mapping scripts),
//! with per-script local sync state, and pull/push the selected one. The list
//! is kind-agnostic: it enumerates `Namespace::all()`, so a new `Kind` shows up
//! here automatically. Editing happens in the workspace (`aic script` CLI /
//! your editor); this tab is a browser with pull/push actions, mirroring the
//! read-side of the ESV tab.
//!
//! The state struct lives on `App` as `app.scripts`; handlers are free
//! functions taking `&mut App` so the keymap dispatch stays one line per arm.
//! This module also owns the feature's nested [`Mode`] and [`Event`] enums.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::scripts::sync::{self, Candidate, LocalState, PushOutcome, Selector};
use crate::scripts::{self as script, Kind};
use crate::tui::widgets::LineEditor;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
}

#[derive(Debug)]
pub enum Event {
    Listed {
        tenant: String,
        result: std::result::Result<Vec<Candidate>, String>,
    },
    OpResult {
        tenant: String,
        full: String,
        label: String,
        result: std::result::Result<String, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Listed { tenant, result } => apply_refresh(app, tenant, result),
        Event::OpResult {
            tenant,
            full,
            label,
            result,
        } => apply_op_result(app, tenant, full, label, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
    }
}

/// Per-tenant load state for the script candidate list.
#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Vec<Candidate>),
    Failed(String),
}

/// One rendered row: a candidate plus its fuzzy-match metadata.
#[derive(Debug, Clone)]
pub struct Match {
    /// Index into the tenant's loaded candidate list.
    pub idx: usize,
    pub full: String,
    pub score: u32,
    pub positions: Vec<u32>,
    pub local: LocalState,
    pub is_default: bool,
}

#[derive(Debug)]
pub struct State {
    /// Per-tenant candidate cache, keyed by tenant name.
    pub data: HashMap<String, LoadState>,
    /// Tenants whose candidate list is currently being fetched.
    pub refreshing: HashSet<String>,
    /// When the last poll-refresh ran (drives the tick cadence).
    pub last_poll: Instant,
    /// Fuzzy search query (empty = show everything).
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
    /// (tenant, full-name) for pull/push ops in flight — gates re-spawns.
    pub in_flight: HashSet<(String, String)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            last_poll: Instant::now(),
            query: LineEditor::default(),
            selected: 0,
            scroll: 0,
            in_flight: HashSet::new(),
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop view state (filter + cursor) — called on tenant switch.
    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn clamp_selection(&mut self, n: usize) {
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    /// The full-name (`<namespace>/<name>`) of a candidate, e.g.
    /// `alpha/Show Result`, `endpoint/test`.
    fn full_of(c: &Candidate) -> String {
        script::full_name(c.kind, c.realm.as_deref(), &c.name)
    }

    /// Filter + sort the tenant's candidates. Empty query → alphabetical by
    /// full-name; otherwise fuzzy-scored (descending). Returns `Match` rows
    /// carrying the index back into the loaded list.
    pub fn matches(&self, tenant: Option<&str>) -> Vec<Match> {
        let Some(name) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(items)) = self.data.get(name) else {
            return Vec::new();
        };

        if self.query.is_empty() {
            let mut out: Vec<Match> = items
                .iter()
                .enumerate()
                .map(|(i, c)| Match {
                    idx: i,
                    full: Self::full_of(c),
                    score: 0,
                    positions: Vec::new(),
                    local: c.local,
                    is_default: c.is_default,
                })
                .collect();
            out.sort_by(|a, b| a.full.cmp(&b.full));
            return out;
        }

        use nucleo_matcher::{
            Config, Matcher, Utf32Str,
            pattern::{AtomKind, CaseMatching, Normalization, Pattern},
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
        for (i, c) in items.iter().enumerate() {
            let full = Self::full_of(c);
            // Synthetic tags so `/!` and `/-` filter by local state, like the
            // ESV tab's `/!pending`. Stripped from the highlight positions.
            let mut haystack_text = full.clone();
            match c.local {
                LocalState::Modified => haystack_text.push_str(" !modified"),
                LocalState::Missing => haystack_text.push_str(" -notpulled"),
                LocalState::Clean => {}
            }
            let full_chars = full.chars().count();
            let haystack = Utf32Str::new(&haystack_text, &mut buf);
            positions.clear();
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                let display_positions: Vec<u32> = positions
                    .iter()
                    .copied()
                    .filter(|p| (*p as usize) < full_chars)
                    .collect();
                out.push(Match {
                    idx: i,
                    full,
                    score,
                    positions: display_positions,
                    local: c.local,
                    is_default: c.is_default,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.full.cmp(&b.full)));
        out
    }
}

/// Kick off a background candidate refresh for the active tenant. `force`
/// re-fetches even when a cached list exists (the stale list stays visible).
pub fn refresh(app: &mut App, force: bool) {
    let Some(name) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }
    if app.scripts.refreshing.contains(&name) {
        return;
    }
    if !force && app.scripts.data.contains_key(&name) {
        return;
    }
    if !app.scripts.data.contains_key(&name) {
        app.scripts.data.insert(name.clone(), LoadState::Loading);
    }
    app.scripts.refreshing.insert(name.clone());
    app.scripts.last_poll = Instant::now();

    let tx = app.events.tx.clone();
    let tenant = name.clone();
    tokio::spawn(async move {
        let result = sync::pull_candidates(&tenant)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Scripts(Event::Listed {
            tenant: name,
            result,
        }));
    });
}

/// Apply a completed background refresh. A failure keeps the
/// previously-cached list (just logs); a first-load failure shows the error.
fn apply_refresh(
    app: &mut App,
    tenant: String,
    result: std::result::Result<Vec<Candidate>, String>,
) {
    app.scripts.refreshing.remove(&tenant);
    let is_active = app.active_tenant().is_some_and(|t| t.name == tenant);
    match result {
        Ok(items) => {
            app.scripts
                .data
                .insert(tenant.clone(), LoadState::Loaded(items));
            if is_active {
                let n = app.scripts.matches(Some(&tenant)).len();
                app.scripts.clamp_selection(n);
            }
        }
        Err(e) => {
            if matches!(app.scripts.data.get(&tenant), Some(LoadState::Loaded(_))) {
                tracing::warn!("scripts refresh failed for {tenant}: {e}");
            } else {
                app.scripts
                    .data
                    .insert(tenant.clone(), LoadState::Failed(e));
            }
        }
    }
}

/// The candidate under the cursor, cloned. `None` when nothing is loaded /
/// selected.
fn selected_candidate(app: &App) -> Option<Candidate> {
    let tenant = app.active_tenant()?.name.clone();
    let matches = app.scripts.matches(Some(&tenant));
    let m = matches.get(app.scripts.selected)?;
    match app.scripts.data.get(&tenant) {
        Some(LoadState::Loaded(items)) => items.get(m.idx).cloned(),
        _ => None,
    }
}

/// ESV-style search keys for the scripts list.
fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.scripts.reset_view();
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => return crate::app::keymap::move_selection(app, -1),
        KeyCode::Down => return crate::app::keymap::move_selection(app, 1),
        KeyCode::PageUp => return crate::app::keymap::move_selection(app, -10),
        KeyCode::PageDown => return crate::app::keymap::move_selection(app, 10),
        _ => {}
    }
    let before = app.scripts.query.value().to_string();
    if app.scripts.query.handle_key(&key) && app.scripts.query.value() != before {
        app.scripts.selected = 0;
        app.scripts.scroll = 0;
    }
}

// ---------------------------------------------------------------------------
// Pull / push actions
// ---------------------------------------------------------------------------

/// Pull the selected script into the workspace (overwrites local edits; a
/// backup is taken first, same as the CLI's non-`--force` pull).
pub fn pull_selected(app: &mut App) {
    let Some(c) = selected_candidate(app) else {
        return;
    };
    let tenant = match app.active_tenant() {
        Some(t) => t.name.clone(),
        None => return,
    };
    let full = script::full_name(c.kind, c.realm.as_deref(), &c.name);
    if !crate::scripts::workspace::ensure_workspace_ready(app, &tenant) {
        return;
    }
    if !begin_op(app, &tenant, &full) {
        return;
    }
    let realm = c.realm.clone().unwrap_or_default();
    let (kind, name) = (c.kind, c.name.clone());
    let tx = app.events.tx.clone();
    let label = format!("pull {full}");
    tokio::spawn(async move {
        let result = sync::pull(&tenant, &realm, kind, &Selector::Name(name), false)
            .await
            .map(|outs| match outs.first() {
                Some(o) => format!("{full}: {}", pull_status(&o.status)),
                None => format!("{full}: nothing to pull"),
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Scripts(Event::OpResult {
            tenant,
            full: full.clone(),
            label,
            result,
        }));
    });
}

/// Pull every script across all namespaces (both realms + endpoints +
/// schedules) into the workspace.
pub fn pull_all(app: &mut App) {
    let tenant = match app.active_tenant() {
        Some(t) => t.name.clone(),
        None => return,
    };
    if !app.is_unlocked() {
        return;
    }
    let full = "all".to_string();
    if !crate::scripts::workspace::ensure_workspace_ready(app, &tenant) {
        return;
    }
    if !begin_op(app, &tenant, &full) {
        return;
    }
    app.push_toast(ToastKind::Info, "Pulling all scripts…");
    let tx = app.events.tx.clone();
    let label = "pull all".to_string();
    tokio::spawn(async move {
        let mut pulled = 0usize;
        let mut errors = 0usize;
        for ns in script::Namespace::all() {
            match sync::pull(&tenant, ns.realm_arg(), ns.kind, &Selector::All, false).await {
                Ok(outs) => pulled += outs.len(),
                Err(e) => {
                    errors += 1;
                    tracing::warn!("pull all: {} failed: {e}", ns.label());
                }
            }
        }
        let result = if errors == 0 {
            Ok(format!("pulled {pulled} scripts"))
        } else {
            Ok(format!(
                "pulled {pulled} scripts ({errors} namespace(s) failed)"
            ))
        };
        let _ = tx.send(AppEvent::Scripts(Event::OpResult {
            tenant,
            full,
            label,
            result,
        }));
    });
}

/// Push the selected script's local edits back to the tenant. Routes through
/// the production-write confirm for prod tenants. Content-checked: a drifted
/// remote is reported (resolve via the CLI's `diff`/`--force`).
pub fn push_selected(app: &mut App) {
    let Some(c) = selected_candidate(app) else {
        return;
    };
    let tenant = match app.active_tenant() {
        Some(t) => t.name.clone(),
        None => return,
    };
    let full = script::full_name(c.kind, c.realm.as_deref(), &c.name);
    if c.local == LocalState::Missing {
        app.push_toast(
            ToastKind::Info,
            format!("{full}: not pulled yet — press p to pull"),
        );
        return;
    }
    let realm = c.realm.clone().unwrap_or_default();
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::ScriptPush {
            tenant,
            kind: c.kind,
            realm,
            name: c.name.clone(),
            full,
        });
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_push(app, tenant, c.kind, realm, c.name.clone(), full, false);
}

/// Spawn the actual push. Shared by the non-prod path and the prod-confirm
/// dispatcher (`confirmed_prod = true`).
pub fn execute_push(
    app: &mut App,
    tenant: String,
    kind: Kind,
    realm: String,
    name: String,
    full: String,
    confirmed_prod: bool,
) {
    if !begin_op(app, &tenant, &full) {
        return;
    }
    let tx = app.events.tx.clone();
    let label = format!("push {full}");
    let full_for_event = full.clone();
    tokio::spawn(async move {
        let result = sync::push(&tenant, &realm, kind, &name, false, confirmed_prod)
            .await
            .map_err(|e| e.to_string())
            .and_then(|outcome| match outcome {
                PushOutcome::Pushed => Ok(format!("pushed {full}")),
                PushOutcome::Unchanged => Ok(format!("{full}: no local changes")),
                PushOutcome::AlreadyInSync => Ok(format!("{full}: already in sync")),
                PushOutcome::Conflict(_) => Err(format!(
                    "{full}: remote changed since last pull — resolve with `aic script diff {full}`"
                )),
            });
        let _ = tx.send(AppEvent::Scripts(Event::OpResult {
            tenant,
            full: full_for_event,
            label,
            result,
        }));
    });
}

/// Apply a finished pull/push. Clears the in-flight marker, toasts the
/// outcome, and refreshes the list so local-state markers update.
fn apply_op_result(
    app: &mut App,
    tenant: String,
    full: String,
    label: String,
    result: std::result::Result<String, String>,
) {
    app.scripts.in_flight.remove(&(tenant.clone(), full));
    match result {
        Ok(msg) => app.push_toast(ToastKind::Success, msg),
        Err(e) => app.push_toast(ToastKind::Error, format!("{label} failed: {e}")),
    }
    // Refresh only the tenant we touched, even if the user has since switched.
    refresh_named(app, &tenant);
}

/// Force-refresh a specific tenant by name (used by op completions).
fn refresh_named(app: &mut App, tenant: &str) {
    if app.active_tenant().is_some_and(|t| t.name == tenant) {
        refresh(app, true);
    } else {
        // Drop the cache so the next visit re-fetches.
        invalidate_tenant(app, tenant);
    }
}

/// Drop the cached script list for `tenant`.
/// Called by other verticals after they mutate script-backed workspace state
/// so the Scripts tab re-fetches next time it is shown.
pub fn invalidate_tenant(app: &mut App, tenant: &str) {
    app.scripts.data.remove(tenant);
}

/// Mark an op in flight; returns false (and toasts) if one is already running
/// for this (tenant, target) or the app is locked.
fn begin_op(app: &mut App, tenant: &str, full: &str) -> bool {
    if !app.is_unlocked() {
        return false;
    }
    let key = (tenant.to_string(), full.to_string());
    if app.scripts.in_flight.contains(&key) {
        app.push_toast(ToastKind::Info, format!("{full}: already in progress"));
        return false;
    }
    app.scripts.in_flight.insert(key);
    true
}

fn pull_status(status: &sync::PullStatus) -> &'static str {
    match status {
        sync::PullStatus::Created => "pulled (new)",
        sync::PullStatus::Updated => "updated",
        sync::PullStatus::Unchanged => "already up to date",
        sync::PullStatus::LocalBackedUp(_) => "pulled (local backed up)",
    }
}
