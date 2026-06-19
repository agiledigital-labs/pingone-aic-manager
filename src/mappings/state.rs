//! Mappings TUI state: per-tenant sync-mapping list cache plus search and
//! selection state.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::mappings::api::{MappingSummary, ReconStatus};
use crate::tui::widgets::LineEditor;

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Vec<MappingSummary>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct MappingMatch {
    /// Index into the loaded, sorted mapping summary list.
    pub idx: usize,
    pub name: String,
    pub display: String,
    pub source: String,
    pub target: String,
    pub inline_script_count: usize,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ReconView {
    pub last: ReconStatus,
}

#[derive(Debug)]
pub struct State {
    pub data: HashMap<String, LoadState>,
    pub refreshing: HashSet<String>,
    pub recon: HashMap<(String, String), ReconView>,
    pub in_flight_recon: HashSet<(String, String)>,
    pub in_flight_pull: HashSet<(String, String)>,
    pub last_poll: Instant,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            recon: HashMap::new(),
            in_flight_recon: HashSet::new(),
            in_flight_pull: HashSet::new(),
            last_poll: Instant::now(),
            query: LineEditor::new(),
            selected: 0,
            scroll: 0,
        }
    }

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

    pub fn select(&mut self, idx: usize) {
        self.selected = idx;
    }

    pub fn recon_for(&self, tenant: &str, mapping: &str) -> Option<&ReconStatus> {
        self.recon
            .get(&(tenant.to_string(), mapping.to_string()))
            .map(|view| &view.last)
    }

    pub fn selected_mapping(&self, tenant: &str) -> Option<&MappingSummary> {
        let matches = self.matches(Some(tenant));
        let selected = self.selected.min(matches.len().saturating_sub(1));
        let idx = matches.get(selected)?.idx;
        match self.data.get(tenant) {
            Some(LoadState::Loaded(mappings)) => mappings.get(idx),
            _ => None,
        }
    }

    pub fn matches(&self, tenant: Option<&str>) -> Vec<MappingMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(mappings)) = self.data.get(tenant) else {
            return Vec::new();
        };

        if self.query.is_empty() {
            let mut matches: Vec<MappingMatch> = mappings
                .iter()
                .enumerate()
                .map(|(idx, mapping)| match_from_summary(idx, mapping, 0, Vec::new()))
                .collect();
            matches.sort_by(|a, b| a.name.cmp(&b.name));
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
        for (idx, mapping) in mappings.iter().enumerate() {
            positions.clear();
            let haystack = Utf32Str::new(&mapping.name, &mut buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                matches.push(match_from_summary(idx, mapping, score, positions.clone()));
            }
        }
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        matches
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn match_from_summary(
    idx: usize,
    mapping: &MappingSummary,
    score: u32,
    positions: Vec<u32>,
) -> MappingMatch {
    MappingMatch {
        idx,
        name: mapping.name.clone(),
        display: format!("{} -> {}", mapping.source, mapping.target),
        source: mapping.source.clone(),
        target: mapping.target.clone(),
        inline_script_count: mapping.inline_script_count,
        score,
        positions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        name: &str,
        source: &str,
        target: &str,
        inline_script_count: usize,
    ) -> MappingSummary {
        MappingSummary {
            name: name.into(),
            source: source.into(),
            target: target.into(),
            inline_script_count,
        }
    }

    fn state_with_mappings(mappings: Vec<MappingSummary>) -> State {
        let mut state = State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(mappings));
        state
    }

    #[test]
    fn empty_query_sorts_mappings_alphabetically() {
        let state = state_with_mappings(vec![
            summary("z_map", "managed/z", "managed/out", 1),
            summary("a_map", "managed/a", "managed/out", 0),
            summary("middle", "managed/m", "managed/out", 2),
        ]);

        let names: Vec<_> = state
            .matches(Some("sandbox"))
            .into_iter()
            .map(|item| item.name)
            .collect();

        assert_eq!(names, ["a_map", "middle", "z_map"]);
    }

    #[test]
    fn search_filter_matches_case_insensitively() {
        let mut state = state_with_mappings(vec![
            summary(
                "ManagedUser_from_Ldap",
                "system/ldap/account",
                "managed/user",
                3,
            ),
            summary("orders", "managed/order", "managed/order_archive", 0),
        ]);
        state.query.set("ldap");

        let matches = state.matches(Some("sandbox"));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "ManagedUser_from_Ldap");
        assert_eq!(matches[0].display, "system/ldap/account -> managed/user");
        assert_eq!(matches[0].inline_script_count, 3);
        assert!(!matches[0].positions.is_empty());
    }

    #[test]
    fn selection_clamps_to_filtered_bounds() {
        let mut state = state_with_mappings(vec![
            summary("one", "managed/a", "managed/b", 0),
            summary("two", "managed/a", "managed/c", 0),
        ]);
        state.selected = 10;

        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 1);

        state.query.set("missing");
        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 0);
    }

    #[test]
    fn recon_and_in_flight_are_keyed_by_tenant_and_mapping() {
        let mut state = State::new();
        state.recon.insert(
            ("sandbox".into(), "map".into()),
            ReconView {
                last: recon_status("map", "ACTIVE"),
            },
        );
        state.recon.insert(
            ("prod".into(), "map".into()),
            ReconView {
                last: recon_status("map", "SUCCESS"),
            },
        );
        state
            .in_flight_recon
            .insert(("sandbox".into(), "map".into()));
        state
            .in_flight_pull
            .insert(("sandbox".into(), "other".into()));

        assert_eq!(
            state
                .recon_for("sandbox", "map")
                .map(|status| status.state.as_str()),
            Some("ACTIVE")
        );
        assert_eq!(
            state
                .recon_for("prod", "map")
                .map(|status| status.state.as_str()),
            Some("SUCCESS")
        );
        assert!(state.recon_for("sandbox", "other").is_none());
        assert!(
            state
                .in_flight_recon
                .contains(&("sandbox".into(), "map".into()))
        );
        assert!(
            !state
                .in_flight_recon
                .contains(&("prod".into(), "map".into()))
        );
        assert!(
            state
                .in_flight_pull
                .contains(&("sandbox".into(), "other".into()))
        );
    }

    fn recon_status(mapping: &str, state: &str) -> ReconStatus {
        ReconStatus {
            id: format!("{mapping}-{state}"),
            mapping: mapping.into(),
            state: state.into(),
            stage: "stage".into(),
            stage_description: "stage description".into(),
            created: 0,
            updated: 0,
            deleted: 0,
            processed: 0,
            ended: None,
            duration: None,
        }
    }
}
