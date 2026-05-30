//! Shared per-tenant list state for the ESVs tab's two halves (variables and
//! secrets), and a model for future list-shaped tabs (Scripts, OAuth2, …).
//!
//! Holds only the mechanics common to every tenant-scoped list: the cached
//! per-tenant data, the pending-id set, the fuzzy/substring query, and the
//! cursor/scroll. Screen-specific state (edit forms, tombstones, versions,
//! in-flight bookkeeping) stays on the owning screen's `State`.

use std::collections::{HashMap, HashSet};

use crate::screens::esv::LoadState;
use crate::ui::widgets::LineEditor;

#[derive(Debug, Default)]
pub struct TenantListState {
    /// Per-tenant cached list, keyed by tenant name.
    pub data: HashMap<String, LoadState>,
    /// Ids the tenant reports as pending (need a restart/apply), keyed by tenant.
    pub pending_ids: HashMap<String, HashSet<String>>,
    /// Search query (empty = show everything).
    pub query: LineEditor,
    /// Cursor index into the filtered list (clamped to len-1).
    pub selected: usize,
    /// First visible row — drives windowed rendering.
    pub scroll: usize,
}

impl TenantListState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-clamp the selection after the visible list shrank.
    pub fn clamp_selection(&mut self, n: usize) {
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    /// Drop view state (filter + cursor) — e.g. on tenant switch.
    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    /// The loaded items for `tenant`, if the cache holds a `Loaded` entry.
    pub fn loaded(&self, tenant: &str) -> Option<&Vec<serde_json::Value>> {
        match self.data.get(tenant) {
            Some(LoadState::Loaded(items)) => Some(items),
            _ => None,
        }
    }
}
