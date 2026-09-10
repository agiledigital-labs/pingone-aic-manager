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

#[derive(Debug)]
pub enum ProdAction {
    Push {
        tenant: String,
        kind: Kind,
        realm: String,
        name: String,
        full: String,
    },
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Push {
            tenant,
            kind,
            realm,
            name,
            full,
        } => execute_push(app, tenant, kind, realm, name, full, true),
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Normal
}

pub fn describe_prod_action(action: &ProdAction) -> Option<String> {
    match action {
        ProdAction::Push { full, .. } => Some(format!("push script {full}")),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    PullConfirm,
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
        outcome: OpOutcome,
    },
    PullPrepared {
        tenant: String,
        full: String,
        label: String,
        result: std::result::Result<sync::PullPlan, String>,
    },
}

/// How a finished pull/push is surfaced.
///
/// A refusal is its own arm rather than an `Err(String)` because the two want
/// different surfaces: a transient event is a toast, and a refusal is
/// actionable state that has to survive the toast's few seconds
/// (`docs/DESIGN.md`, "Issue surfacing"). It is also why the shared sync
/// engine no longer prints: presentation is per-surface, and an `eprintln!`
/// from a background task lands on the alternate screen.
#[derive(Debug)]
pub enum OpOutcome {
    /// Toast this, and clear any standing issue for the script.
    Ok(String),
    /// The syntax gate refused the write; nothing was written. `source`
    /// identifies the bytes that were checked, carried from the push rather
    /// than re-read here — the operator may have saved a fix while the request
    /// was in flight, and a refusal pinned to that fix would claim the tenant
    /// rejected source it never saw.
    Refused {
        refusal: script::syntax::Refusal,
        source: sync::SourceId,
    },
    /// The op itself failed (transport, locked agent, …).
    Failed(String),
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Listed { tenant, result } => apply_refresh(app, tenant, result),
        Event::OpResult {
            tenant,
            full,
            label,
            outcome,
        } => apply_op_result(app, tenant, full, label, outcome),
        Event::PullPrepared {
            tenant,
            full,
            label,
            result,
        } => apply_pull_prepared(app, tenant, full, label, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::PullConfirm => handle_pull_confirm_key(app, key),
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            ("Type", "edit search query"),
            ("Backspace", "delete character"),
            ("Enter", "keep filter and return to list"),
            ("Esc", "clear filter and return to list"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
        Mode::PullConfirm => Some(vec![
            ("y", "overwrite every listed local edit with backups"),
            ("n/Enter/Esc", "cancel the whole pull (default)"),
            ("F1", "show keybinds"),
        ]),
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
    /// Writes the syntax gate refused, keyed by (tenant, full-name). Kept here
    /// rather than left to a toast because it is actionable and stays true
    /// until the operator does something about it; the view renders it as an
    /// inline strip. Cleared by a successful op on the same script, or by the
    /// next refresh once the local source has changed.
    pub refused: HashMap<(String, String), Refused>,
    pub pending_pull: Option<PendingPull>,
}

#[derive(Debug)]
pub struct PendingPull {
    tenant: String,
    full: String,
    label: String,
    plan: sync::PullPlan,
}

pub fn pending_pull_refs(app: &App) -> Vec<String> {
    app.scripts
        .pending_pull
        .as_ref()
        .map(|pending| pending.plan.protected_refs())
        .unwrap_or_default()
}

/// A refusal held for display, with the source it was about.
#[derive(Debug, Clone)]
pub struct Refused {
    /// Why, in one clause.
    pub summary: String,
    /// Per-error lines, coordinates included.
    pub detail: Vec<String>,
    /// The source that was refused, as reported by the push itself. The strip
    /// is stale once the workspace no longer holds these bytes — that is the
    /// "until the source changes" half of the rule, and it is evaluated on
    /// refresh because that is when the tab re-reads the workspace.
    pub source: sync::SourceId,
}

/// Whether a held refusal still describes what is in the workspace.
///
/// A missing local file counts as changed: there is nothing left for the
/// refusal to be about, and a strip against a file that is gone cannot be
/// acted on.
fn refusal_is_current(held: &Refused, on_disk: Option<sync::SourceId>) -> bool {
    on_disk == Some(held.source)
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
            refused: HashMap::new(),
            pending_pull: None,
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
        if let Some(pending) = self.pending_pull.take() {
            self.in_flight.remove(&(pending.tenant, pending.full));
        }
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

        let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(self.query.value());
        let mut out: Vec<Match> = Vec::new();
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
            if let Some((score, positions)) = matcher.match_indices(&haystack_text) {
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

pub fn row_count(app: &App) -> usize {
    app.scripts
        .matches(app.active_tenant().map(|t| t.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.scripts.selected
}

pub fn set_selection(app: &mut App, idx: usize) {
    app.scripts.selected = idx;
}

pub fn filter_active(app: &App) -> bool {
    !app.scripts.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.scripts.reset_view();
}

pub fn primary(_app: &mut App) {}

pub fn delete(_app: &mut App) {}

pub fn new_item(_app: &mut App) {}

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
            // Drop a held refusal whose source has since changed (or whose
            // script has gone), so the strip cannot outlive the edit that
            // answers it.
            app.scripts.refused.retain(|(t, full), held| {
                if *t != tenant {
                    return true;
                }
                match items.iter().find(|c| State::full_of(c) == *full) {
                    Some(c) => refusal_is_current(held, sync::local_source_id(&tenant, c)),
                    None => false,
                }
            });
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

/// Preflight the selected pull before changing the workspace.
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
        let result = sync::prepare_pull(
            &tenant,
            vec![sync::PullTarget {
                realm,
                kind,
                selector: Selector::Name(name),
            }],
        )
        .await
        .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Scripts(Event::PullPrepared {
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
        let targets = script::Namespace::all()
            .into_iter()
            .map(|ns| sync::PullTarget {
                realm: ns.realm_arg().to_string(),
                kind: ns.kind,
                selector: Selector::All,
            })
            .collect();
        let result = sync::prepare_pull(&tenant, targets)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Scripts(Event::PullPrepared {
            tenant,
            full,
            label,
            result,
        }));
    });
}

fn apply_pull_prepared(
    app: &mut App,
    tenant: String,
    full: String,
    label: String,
    result: std::result::Result<sync::PullPlan, String>,
) {
    if app
        .active_tenant()
        .is_none_or(|active| active.name != tenant)
    {
        app.scripts.in_flight.remove(&(tenant, full));
        return;
    }
    let plan = match result {
        Ok(plan) => plan,
        Err(error) => {
            apply_op_result(app, tenant, full, label, OpOutcome::Failed(error));
            return;
        }
    };
    if plan.protected_refs().is_empty() {
        apply_pull_plan(app, tenant, full, label, plan);
    } else {
        app.scripts.pending_pull = Some(PendingPull {
            tenant,
            full,
            label,
            plan,
        });
        app.input_mode = InputMode::Scripts(Mode::PullConfirm);
    }
}

fn apply_pull_plan(
    app: &mut App,
    tenant: String,
    full: String,
    label: String,
    plan: sync::PullPlan,
) {
    let count = plan.len();
    let outcome = match plan.install(false) {
        Ok(outcomes) => {
            let backed_up = outcomes
                .iter()
                .filter(|outcome| matches!(outcome.status, sync::PullStatus::LocalBackedUp(_)))
                .count();
            let suffix = if backed_up == 0 {
                String::new()
            } else {
                format!(" ({backed_up} local source(s) backed up)")
            };
            OpOutcome::Ok(format!("pulled {count} script(s){suffix}"))
        }
        Err(error) => OpOutcome::Failed(error.to_string()),
    };
    apply_op_result(app, tenant, full, label, outcome);
}

fn handle_pull_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(pending) = app.scripts.pending_pull.take() else {
                app.input_mode = InputMode::Normal;
                return;
            };
            app.input_mode = InputMode::Normal;
            apply_pull_plan(
                app,
                pending.tenant,
                pending.full,
                pending.label,
                pending.plan,
            );
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter | KeyCode::Esc => {
            if let Some(pending) = app.scripts.pending_pull.take() {
                app.scripts
                    .in_flight
                    .remove(&(pending.tenant, pending.full));
            }
            app.input_mode = InputMode::Normal;
            app.push_toast(ToastKind::Info, "Pull cancelled; local changes kept");
        }
        _ => {}
    }
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
        app.prod_confirm.pending = Some(PendingProdAction::Scripts(ProdAction::Push {
            tenant,
            kind: c.kind,
            realm,
            name: c.name.clone(),
            full,
        }));
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
        let outcome = push_op_outcome(
            &full,
            sync::push(
                &tenant,
                &realm,
                kind,
                &name,
                false,
                confirmed_prod,
                // No opt-out in the TUI: the CLI's `--force=syntax-check` exists for
                // scripted use, and there is no keybind worth spending on writing
                // source the tenant has just said it cannot parse.
                sync::SyntaxGate::Check,
            )
            .await,
        );
        let _ = tx.send(AppEvent::Scripts(Event::OpResult {
            tenant,
            full: full_for_event,
            label,
            outcome,
        }));
    });
}

fn push_op_outcome(full: &str, result: crate::Result<PushOutcome>) -> OpOutcome {
    match result {
        Err(e) => OpOutcome::Failed(e.to_string()),
        Ok(PushOutcome::Pushed) => OpOutcome::Ok(format!("pushed {full}")),
        Ok(PushOutcome::NotConfirmed(reason)) => OpOutcome::Failed(reason.message()),
        Ok(PushOutcome::Unchanged) => OpOutcome::Ok(format!("{full}: no local changes")),
        Ok(PushOutcome::AlreadyInSync) => OpOutcome::Ok(format!("{full}: already in sync")),
        Ok(PushOutcome::Conflict(_)) => OpOutcome::Failed(format!(
            "{full}: remote changed since last pull — resolve with `aic script diff {full}`"
        )),
        // Held as an inline issue, not spent on one toast line: the coordinate
        // and reason are what the operator needs while fixing the source.
        Ok(PushOutcome::Refused { refusal, source }) => OpOutcome::Refused { refusal, source },
    }
}

/// Apply a finished pull/push. Clears the in-flight marker, toasts the
/// outcome, and refreshes the list so local-state markers update.
fn apply_op_result(app: &mut App, tenant: String, full: String, label: String, outcome: OpOutcome) {
    let key = (tenant.clone(), full.clone());
    app.scripts.in_flight.remove(&key);
    match outcome {
        OpOutcome::Ok(msg) => {
            // Any successful op on this script answers a standing refusal.
            app.scripts.refused.remove(&key);
            app.push_toast(ToastKind::Success, msg);
        }
        OpOutcome::Failed(e) => app.push_toast(ToastKind::Error, format!("{label} failed: {e}")),
        OpOutcome::Refused { refusal, source } => {
            app.push_toast(ToastKind::Error, format!("{full}: {}", refusal.headline()));
            app.scripts.refused.insert(
                key,
                Refused {
                    summary: refusal.summary(),
                    detail: refusal.detail(),
                    source,
                },
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::syntax::{Refusal, SyntaxError};
    use crossterm::event::KeyModifiers;

    fn held(source: sync::SourceId) -> Refused {
        let refusal = Refusal::Rejected(vec![SyntaxError {
            line: Some(3),
            column: Some(15),
            message: "missing ) in parenthetical".into(),
        }]);
        Refused {
            summary: refusal.summary(),
            detail: refusal.detail(),
            source,
        }
    }

    /// The strip stands until the source changes — and "the source" is the
    /// bytes the tenant was asked about, which is why the push reports them
    /// instead of the tab re-reading the file afterwards. Re-reading loses the
    /// discriminating case below: the operator saves a fix while the request
    /// is in flight, and the refusal would then be pinned to source the tenant
    /// never saw.
    #[test]
    fn a_held_refusal_survives_only_while_the_source_it_judged_is_on_disk() {
        let broken = sync::source_id(b"var x = (1;");
        let fixed = sync::source_id(b"var x = 1;");
        let held = held(broken);

        assert!(refusal_is_current(&held, Some(broken)));
        assert!(
            !refusal_is_current(&held, Some(fixed)),
            "an edit must clear the strip"
        );
        assert!(
            !refusal_is_current(&held, None),
            "a file that is gone leaves nothing for the strip to be about"
        );
    }

    #[test]
    fn an_unconfirmed_push_is_a_failed_tui_operation() {
        let outcome = push_op_outcome(
            "alpha/example",
            Ok(PushOutcome::NotConfirmed(
                sync::ConfirmationFailure::Mismatch,
            )),
        );
        assert!(
            matches!(outcome, OpOutcome::Failed(message) if message.contains("tenant state uncertain"))
        );
    }

    #[test]
    fn pull_confirmation_defaults_to_cancelling_the_whole_plan() {
        let mut app = App::for_test(Vec::new(), crate::app::View::Scripts);
        app.scripts
            .in_flight
            .insert(("sandbox".into(), "all".into()));
        app.scripts.pending_pull = Some(PendingPull {
            tenant: "sandbox".into(),
            full: "all".into(),
            label: "pull all".into(),
            plan: sync::PullPlan::protected_for_test(&["map.onCreate"]),
        });
        app.input_mode = InputMode::Scripts(Mode::PullConfirm);

        handle_pull_confirm_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.scripts.pending_pull.is_none());
        assert!(app.scripts.in_flight.is_empty());
    }
}
