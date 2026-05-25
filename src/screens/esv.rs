//! ESV tab — list, fuzzy search, preview. The state struct lives on `App`
//! as a single field (`app.esv`); handlers are free functions that take
//! `&mut App` so the dispatch table in `app.rs` stays one-liner per arm.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};
use crate::event::AppEvent;
use crate::ui::widgets::LineEditor;

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
}

impl Default for State {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            last_poll: Instant::now(),
            query: LineEditor::new(),
            selected: 0,
            scroll: 0,
        }
    }
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
            let haystack = Utf32Str::new(id, &mut buf);
            positions.clear();
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                out.push(Match {
                    idx: i,
                    id: id.to_string(),
                    score,
                    positions: positions.clone(),
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
        _ => false,
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
        Ok(vs) => {
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
