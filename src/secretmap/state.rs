//! Secret-mapping TUI state: alpha-realm mapping list, fuzzy search, and the
//! ESV-secret alias picker used by the edit flow.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::secretmap::{api, labels};
use crate::tui::widgets::LineEditor;

/// Secret mappings are scoped to alpha for the first TUI tab.
/// TODO bravo: add a realm-aware mapping browser if this tab grows realm UI.
pub const REALM: &str = "alpha";

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Vec<api::Mapping>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct MappingMatch {
    /// Index into the loaded, sorted mapping list.
    pub idx: usize,
    pub secret_id: String,
    pub alias: Option<String>,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct AliasMatch {
    pub id: String,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct LabelMatch {
    pub id: String,
    pub description: String,
    pub score: u32,
    pub positions: Vec<u32>,
}

#[derive(Debug)]
pub struct PickLabelState {
    pub tenant: String,
    pub realm: String,
    pub query: LineEditor,
    pub selected: usize,
    pub error: Option<String>,
}

impl PickLabelState {
    pub fn new(tenant: String) -> Self {
        Self {
            tenant,
            realm: REALM.to_string(),
            query: LineEditor::new(),
            selected: 0,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct EditAliasState {
    pub tenant: String,
    pub realm: String,
    pub secret_id: String,
    pub prior_alias: Option<String>,
    pub snapshot: Value,
    pub query: LineEditor,
    pub selected: usize,
    pub error: Option<String>,
}

impl EditAliasState {
    pub fn new(tenant: String, mapping: api::Mapping) -> Self {
        let snapshot = mapping_snapshot(&mapping.secret_id, mapping.alias.as_deref());
        Self {
            tenant,
            realm: REALM.to_string(),
            secret_id: mapping.secret_id,
            prior_alias: mapping.alias,
            snapshot,
            query: LineEditor::new(),
            selected: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeleteMappingState {
    pub tenant: String,
    pub realm: String,
    pub secret_id: String,
    pub prior_alias: String,
    pub snapshot: Value,
}

#[derive(Debug, Default)]
pub struct State {
    pub data: HashMap<String, LoadState>,
    pub refreshing: HashSet<String>,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
    pub detail_scroll: usize,
    pub picking_label: Option<PickLabelState>,
    pub editing: Option<EditAliasState>,
    pub pending_delete: Option<DeleteMappingState>,
    pub esv_secret_ids: HashMap<String, Vec<String>>,
    pub esv_secret_loading: HashSet<String>,
    pub esv_secret_failed: HashMap<String, String>,
    pub valid_secret_ids: HashMap<String, Vec<String>>,
    pub valid_secret_loading: HashSet<String>,
    pub valid_secret_failed: HashMap<String, String>,
    pub in_flight_writes: HashSet<(String, String)>,
    pub failed_writes: HashSet<(String, String)>,
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
        self.picking_label = None;
        self.editing = None;
        self.pending_delete = None;
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

    pub fn selected_mapping(&self, tenant: Option<&str>) -> Option<api::Mapping> {
        let tenant = tenant?;
        let matches = self.matches(Some(tenant));
        let item = matches.get(self.selected)?;
        let Some(LoadState::Loaded(mappings)) = self.data.get(tenant) else {
            return None;
        };
        mappings.get(item.idx).cloned()
    }

    pub fn invalidate_esv_cache(&mut self, tenant: &str) {
        self.esv_secret_ids.remove(tenant);
        self.esv_secret_failed.remove(tenant);
        self.esv_secret_loading.remove(tenant);
    }

    pub fn invalidate_valid_label_cache(&mut self, tenant: &str) {
        self.valid_secret_ids.remove(tenant);
        self.valid_secret_failed.remove(tenant);
        self.valid_secret_loading.remove(tenant);
    }

    pub fn clamp_picker_selection(&mut self, n: usize) {
        if let Some(edit) = self.editing.as_mut() {
            if edit.selected >= n {
                edit.selected = n.saturating_sub(1);
            }
        }
    }

    pub fn clamp_label_selection(&mut self, n: usize) {
        if let Some(pick) = self.picking_label.as_mut() {
            if pick.selected >= n {
                pick.selected = n.saturating_sub(1);
            }
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
                .map(|(idx, mapping)| MappingMatch {
                    idx,
                    secret_id: mapping.secret_id.clone(),
                    alias: mapping.alias.clone(),
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
            matches.sort_by(|a, b| a.secret_id.cmp(&b.secret_id));
            return matches;
        }

        fuzzy_mapping_matches(mappings, self.query.value())
    }

    pub fn alias_matches(&self, tenant: Option<&str>) -> Vec<AliasMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(ids) = self.esv_secret_ids.get(tenant) else {
            return Vec::new();
        };
        let query = self
            .editing
            .as_ref()
            .map(|edit| edit.query.value())
            .unwrap_or_default();
        fuzzy_alias_matches(ids, query)
    }

    pub fn label_matches(&self, tenant: Option<&str>) -> Vec<LabelMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(ids) = self.valid_secret_ids.get(tenant) else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(mappings)) = self.data.get(tenant) else {
            return Vec::new();
        };
        let candidates = unmapped_secret_ids(ids, mappings);
        let query = self
            .picking_label
            .as_ref()
            .map(|pick| pick.query.value())
            .unwrap_or_default();
        fuzzy_label_matches(&candidates, query)
    }
}

pub fn mapping_snapshot(secret_id: &str, alias: Option<&str>) -> Value {
    let aliases = alias
        .map(|alias| vec![Value::String(alias.to_string())])
        .unwrap_or_default();
    json!({
        "_id": secret_id,
        "secretId": secret_id,
        "aliases": aliases
    })
}

pub fn fuzzy_alias_matches(ids: &[String], query: &str) -> Vec<AliasMatch> {
    if query.is_empty() {
        let mut matches: Vec<AliasMatch> = ids
            .iter()
            .map(|id| AliasMatch {
                id: id.clone(),
                score: 0,
                positions: Vec::new(),
            })
            .collect();
        matches.sort_by(|a, b| a.id.cmp(&b.id));
        return matches;
    }

    let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(query);
    let mut matches = Vec::new();
    for id in ids {
        if let Some((score, positions)) = matcher.match_indices(id) {
            matches.push(AliasMatch {
                id: id.clone(),
                score,
                positions,
            });
        }
    }
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    matches
}

pub fn unmapped_secret_ids(valid: &[String], mappings: &[api::Mapping]) -> Vec<String> {
    let mapped: HashSet<&str> = mappings
        .iter()
        .map(|mapping| mapping.secret_id.as_str())
        .collect();
    let mut ids: Vec<String> = valid
        .iter()
        .filter(|id| !mapped.contains(id.as_str()))
        .cloned()
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

pub fn fuzzy_label_matches(ids: &[String], query: &str) -> Vec<LabelMatch> {
    if query.is_empty() {
        let mut matches: Vec<LabelMatch> = ids
            .iter()
            .map(|id| LabelMatch {
                id: id.clone(),
                description: labels::describe(id),
                score: 0,
                positions: Vec::new(),
            })
            .collect();
        matches.sort_by(|a, b| a.id.cmp(&b.id));
        return matches;
    }

    let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(query);
    let mut matches = Vec::new();
    for id in ids {
        let description = labels::describe(id);
        let haystack_text = format!("{id} {description}");
        if let Some((score, positions)) = matcher.match_indices(&haystack_text) {
            matches.push(LabelMatch {
                id: id.clone(),
                description,
                score,
                positions,
            });
        }
    }
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    matches
}

fn fuzzy_mapping_matches(mappings: &[api::Mapping], query: &str) -> Vec<MappingMatch> {
    let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(query);
    let mut matches = Vec::new();
    for (idx, mapping) in mappings.iter().enumerate() {
        let haystack_text = match mapping.alias.as_deref() {
            Some(alias) => format!("{} {}", mapping.secret_id, alias),
            None => mapping.secret_id.clone(),
        };
        if let Some((score, positions)) = matcher.match_indices(&haystack_text) {
            matches.push(MappingMatch {
                idx,
                secret_id: mapping.secret_id.clone(),
                alias: mapping.alias.clone(),
                score,
                positions,
            });
        }
    }
    matches.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.secret_id.cmp(&b.secret_id))
    });
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_mappings(mappings: Vec<api::Mapping>) -> State {
        let mut state = State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(mappings));
        state
    }

    #[test]
    fn search_filter_matches_case_insensitively() {
        let mut state = state_with_mappings(vec![
            api::Mapping {
                secret_id: "am.applications.oauth2.client.Service_App.secret".into(),
                alias: Some("esv-service".into()),
            },
            api::Mapping {
                secret_id: "am.services.saml2.metadata.signing.RSA".into(),
                alias: Some("esv-saml".into()),
            },
        ]);
        state.query.set("service_app");

        let matches = state.matches(Some("sandbox"));

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].secret_id,
            "am.applications.oauth2.client.Service_App.secret"
        );
        assert!(!matches[0].positions.is_empty());
    }

    #[test]
    fn selection_clamps_to_filtered_bounds() {
        let mut state = state_with_mappings(vec![
            api::Mapping {
                secret_id: "am.a".into(),
                alias: None,
            },
            api::Mapping {
                secret_id: "am.b".into(),
                alias: None,
            },
        ]);
        state.selected = 10;

        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 1);

        state.query.set("missing");
        state.clamp_selection(state.matches(Some("sandbox")).len());

        assert_eq!(state.selected, 0);
    }

    #[test]
    fn reset_view_clears_filter_selection_picker_and_scroll() {
        let mut state = state_with_mappings(vec![api::Mapping {
            secret_id: "am.a".into(),
            alias: Some("esv-a".into()),
        }]);
        state.query.set("am");
        state.selected = 1;
        state.scroll = 3;
        state.detail_scroll = 9;
        state.picking_label = Some(PickLabelState::new("sandbox".into()));
        state.editing = Some(EditAliasState::new(
            "sandbox".into(),
            api::Mapping {
                secret_id: "am.a".into(),
                alias: Some("esv-a".into()),
            },
        ));
        state.pending_delete = Some(DeleteMappingState {
            tenant: "sandbox".into(),
            realm: REALM.into(),
            secret_id: "am.a".into(),
            prior_alias: "esv-a".into(),
            snapshot: mapping_snapshot("am.a", Some("esv-a")),
        });

        state.reset_view();

        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll, 0);
        assert_eq!(state.detail_scroll, 0);
        assert!(state.picking_label.is_none());
        assert!(state.editing.is_none());
        assert!(state.pending_delete.is_none());
    }

    #[test]
    fn picker_fuzzy_filter_returns_expected_matches_and_order() {
        let ids = vec![
            "esv-saml-signing-key".to_string(),
            "esv-oauth-jwt-key".to_string(),
            "esv-oauth-client-secret".to_string(),
        ];

        let matches = fuzzy_alias_matches(&ids, "oauth");
        let ids: Vec<_> = matches.into_iter().map(|item| item.id).collect();

        assert_eq!(ids, ["esv-oauth-client-secret", "esv-oauth-jwt-key"]);
    }

    #[test]
    fn unmapped_secret_ids_subtracts_current_mapping_list() {
        let valid = vec!["am.a".to_string(), "am.b".to_string(), "am.c".to_string()];
        let mappings = vec![
            api::Mapping {
                secret_id: "am.b".into(),
                alias: Some("esv-b".into()),
            },
            api::Mapping {
                secret_id: "am.c".into(),
                alias: None,
            },
        ];

        assert_eq!(unmapped_secret_ids(&valid, &mappings), ["am.a"]);
    }

    #[test]
    fn label_fuzzy_filter_matches_description_and_orders_results() {
        let ids = vec![
            "am.services.saml2.metadata.signing.RSA".to_string(),
            "am.applications.oauth2.client.pega.secret".to_string(),
            "am.applications.agents.ig.secret".to_string(),
        ];

        let matches = fuzzy_label_matches(&ids, "client secret");
        let ids: Vec<_> = matches.into_iter().map(|item| item.id).collect();

        assert_eq!(ids[0], "am.applications.oauth2.client.pega.secret");
        assert!(!ids.contains(&"am.services.saml2.metadata.signing.RSA".to_string()));
    }
}
