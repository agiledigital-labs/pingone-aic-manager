use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::tui::widgets::LineEditor;

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Value),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ManagedMatch {
    /// Index into the summaries returned by `managed::api::summarize`.
    pub idx: usize,
    pub name: String,
    pub properties: usize,
    pub hooks_inline: usize,
    pub hooks_file: usize,
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
}

impl State {
    pub fn new() -> Self {
        Self::default()
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

    pub fn matches(&self, tenant: Option<&str>) -> Vec<ManagedMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(doc)) = self.data.get(tenant) else {
            return Vec::new();
        };
        let Ok(summaries) = crate::managed::api::summarize(doc) else {
            return Vec::new();
        };

        if self.query.is_empty() {
            let mut matches: Vec<ManagedMatch> = summaries
                .iter()
                .enumerate()
                .map(|(idx, summary)| ManagedMatch {
                    idx,
                    name: summary.name.clone(),
                    properties: summary.properties,
                    hooks_inline: summary.hooks_inline.len(),
                    hooks_file: summary.hooks_file.len(),
                    score: 0,
                    positions: Vec::new(),
                })
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
        for (idx, summary) in summaries.iter().enumerate() {
            positions.clear();
            let haystack = Utf32Str::new(&summary.name, &mut buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                matches.push(ManagedMatch {
                    idx,
                    name: summary.name.clone(),
                    properties: summary.properties,
                    hooks_inline: summary.hooks_inline.len(),
                    hooks_file: summary.hooks_file.len(),
                    score,
                    positions: positions.clone(),
                });
            }
        }
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        matches
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state_with_objects(objects: Value) -> State {
        let mut state = State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(objects));
        state
    }

    #[test]
    fn empty_query_sorts_objects_alphabetically() {
        let state = state_with_objects(json!({"objects": [
            {"name": "bravo_user", "schema": {"properties": {}}},
            {"name": "alpha_role", "schema": {"properties": {}}},
            {"name": "alpha_user", "schema": {"properties": {}}}
        ]}));

        let names: Vec<_> = state
            .matches(Some("sandbox"))
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(names, ["alpha_role", "alpha_user", "bravo_user"]);
    }

    #[test]
    fn query_fuzzy_filters_by_object_name() {
        let mut state = state_with_objects(json!({"objects": [
            {"name": "alpha_application", "schema": {"properties": {}}},
            {"name": "alpha_role", "schema": {"properties": {}}},
            {"name": "bravo_user", "schema": {"properties": {}}}
        ]}));
        state.query.set("arole");

        let matches = state.matches(Some("sandbox"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "alpha_role");
        assert!(!matches[0].positions.is_empty());
    }
}
