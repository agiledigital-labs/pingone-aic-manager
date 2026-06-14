//! OAuth2 TUI state: alpha-realm client list, search/filter selection, and
//! lazily fetched read-only detail cache.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::tui::widgets::LineEditor;

/// OAuth TUI is scoped to alpha for this first read-only tab.
/// TODO: add bravo support if the app grows a realm-aware OAuth browser.
pub const REALM: &str = "alpha";

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Vec<String>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ClientMatch {
    /// Index into the loaded, sorted client-id list.
    pub idx: usize,
    pub id: String,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct State {
    pub data: HashMap<String, LoadState>,
    pub refreshing: HashSet<String>,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
    pub detail_scroll: usize,
    pub detail_cache: HashMap<String, Value>,
    pub detail_loading: HashSet<String>,
    pub detail_failed: HashMap<String, String>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.detail_scroll = 0;
    }

    pub fn clamp_selection(&mut self, n: usize) {
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    pub fn select(&mut self, idx: usize) {
        if self.selected != idx {
            self.selected = idx;
            self.detail_scroll = 0;
        }
    }

    pub fn detail_key(tenant: &str, id: &str) -> String {
        format!("{tenant}/{REALM}/{id}")
    }

    pub fn selected_id(&self, tenant: Option<&str>) -> Option<String> {
        let tenant = tenant?;
        let matches = self.matches(Some(tenant));
        matches.get(self.selected).map(|item| item.id.clone())
    }

    pub fn clear_tenant_details(&mut self, tenant: &str) {
        let prefix = format!("{tenant}/{REALM}/");
        self.detail_cache.retain(|key, _| !key.starts_with(&prefix));
        self.detail_loading.retain(|key| !key.starts_with(&prefix));
        self.detail_failed
            .retain(|key, _| !key.starts_with(&prefix));
        self.detail_scroll = 0;
    }

    pub fn matches(&self, tenant: Option<&str>) -> Vec<ClientMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(ids)) = self.data.get(tenant) else {
            return Vec::new();
        };

        if self.query.is_empty() {
            let mut matches: Vec<ClientMatch> = ids
                .iter()
                .enumerate()
                .map(|(idx, id)| ClientMatch {
                    idx,
                    id: id.clone(),
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
            matches.sort_by(|a, b| a.id.cmp(&b.id));
            return matches;
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
        let mut buf = Vec::new();
        let mut positions = Vec::new();
        let mut matches = Vec::new();
        for (idx, id) in ids.iter().enumerate() {
            positions.clear();
            let haystack = Utf32Str::new(id, &mut buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                matches.push(ClientMatch {
                    idx,
                    id: id.clone(),
                    score,
                    positions: positions.clone(),
                });
            }
        }
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_clients(ids: &[&str]) -> State {
        let mut state = State::new();
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(ids.iter().map(|id| (*id).to_string()).collect()),
        );
        state
    }

    #[test]
    fn empty_query_sorts_clients_alphabetically() {
        let state = state_with_clients(&["zeta", "alpha_service", "bravo"]);

        let ids: Vec<_> = state
            .matches(Some("sandbox"))
            .into_iter()
            .map(|item| item.id)
            .collect();

        assert_eq!(ids, ["alpha_service", "bravo", "zeta"]);
    }

    #[test]
    fn search_filter_matches_case_insensitively() {
        let mut state = state_with_clients(&["Service_App", "admin-console", "web"]);
        state.query.set("service");

        let matches = state.matches(Some("sandbox"));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "Service_App");
        assert!(!matches[0].positions.is_empty());
    }

    #[test]
    fn selection_clamps_to_filtered_bounds() {
        let mut state = state_with_clients(&["one", "two"]);
        state.selected = 10;

        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 1);

        state.query.set("missing");
        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 0);
    }

    #[test]
    fn reset_view_clears_filter_selection_and_scroll() {
        let mut state = state_with_clients(&["one"]);
        state.query.set("o");
        state.selected = 1;
        state.scroll = 3;
        state.detail_scroll = 8;

        state.reset_view();

        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll, 0);
        assert_eq!(state.detail_scroll, 0);
    }
}
