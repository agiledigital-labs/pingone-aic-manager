//! Access-tab state: per-tenant raw rule documents, search, and selection.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::access::{ops, spec};
use crate::tui::widgets::LineEditor;

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Document),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RuleRow {
    pub index: usize,
    pub digest: String,
    pub duplicate: bool,
    pub pattern: String,
    pub methods: String,
    pub roles: String,
    /// `None` means the raw rule omitted the key; it is never defaulted.
    pub actions: Option<String>,
    /// The untouched rule is the detail pane's source of truth, including
    /// unknown keys that [`spec::RuleView`] deliberately does not project.
    pub raw: Value,
}

#[derive(Debug)]
pub struct Document {
    pub digest: String,
    pub rows: Vec<RuleRow>,
}

impl Document {
    pub fn from_value(value: Value) -> crate::Result<Self> {
        let rules = ops::rules(&value)?;
        let duplicates = ops::duplicate_flags(rules);
        let rows = rules
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleRow::new(index, rule, duplicates[index]))
            .collect();
        Ok(Self {
            digest: spec::digest(&value),
            rows,
        })
    }
}

impl RuleRow {
    fn new(index: usize, rule: &Value, duplicate: bool) -> Self {
        let view = spec::RuleView::from_value(rule);
        Self {
            index,
            digest: spec::short_digest(rule),
            duplicate,
            pattern: view.pattern,
            methods: view.methods,
            roles: view.roles,
            actions: view.actions,
            raw: rule.clone(),
        }
    }

    fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.index,
            self.digest,
            self.pattern,
            self.methods,
            self.roles,
            self.actions.as_deref().unwrap_or("<absent>")
        )
    }
}

#[derive(Debug, Clone)]
pub struct RuleMatch {
    pub row: RuleRow,
    score: u32,
}

#[derive(Debug)]
pub struct State {
    pub data: HashMap<String, LoadState>,
    pub refreshing: HashSet<String>,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
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

    pub fn clamp_selection(&mut self, count: usize) {
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    pub fn move_selection(&mut self, count: usize, delta: isize) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, count as isize - 1) as usize;
    }

    pub fn select(&mut self, index: usize, count: usize) {
        self.selected = index.min(count.saturating_sub(1));
    }

    pub fn document(&self, tenant: &str) -> Option<&Document> {
        match self.data.get(tenant) {
            Some(LoadState::Loaded(document)) => Some(document),
            _ => None,
        }
    }

    pub fn matches(&self, tenant: Option<&str>) -> Vec<RuleMatch> {
        let Some(tenant) = tenant else {
            return Vec::new();
        };
        let Some(document) = self.document(tenant) else {
            return Vec::new();
        };
        if self.query.is_empty() {
            return document
                .rows
                .iter()
                .cloned()
                .map(|row| RuleMatch { row, score: 0 })
                .collect();
        }

        let query = self.query.value();
        let mut matcher = crate::tui::fuzzy::FuzzyMatcher::new(query);
        let mut matches = document
            .rows
            .iter()
            .filter_map(|row| {
                let exact_list = spec::comma_list_contains(&row.raw, "methods", query)
                    || spec::comma_list_contains(&row.raw, "roles", query);
                let fuzzy_score = matcher
                    .match_indices(&row.search_text())
                    .map(|(score, _)| score);
                (exact_list || fuzzy_score.is_some()).then(|| RuleMatch {
                    row: row.clone(),
                    score: fuzzy_score.unwrap_or_default() + u32::from(exact_list) * 1_000,
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.row.index.cmp(&b.row.index))
        });
        matches
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn projection_preserves_absent_actions() {
        // Replacing the Option projection with a display default such as `*`
        // makes this fail and would misrepresent the six live omitted keys.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();

        assert_eq!(document.rows[0].actions, None);
        assert!(document.rows[0].raw.get("actions").is_none());
        assert_eq!(document.rows[1].actions.as_deref(), Some("*"));
    }

    #[test]
    fn selection_movement_clamps_for_populated_and_empty_lists() {
        // Removing either boundary clamp, or leaving a stale selection on an
        // empty result, makes the corresponding table row fail.
        for (name, initial, count, delta, expected) in [
            ("above top", 0, 3, -1, 0),
            ("at top", 1, 3, -10, 0),
            ("inside", 1, 3, 1, 2),
            ("below bottom", 2, 3, 1, 2),
            ("at bottom", 0, 3, 10, 2),
            ("empty", 7, 0, 1, 0),
        ] {
            let mut state = State::new();
            state.selected = initial;
            state.move_selection(count, delta);
            assert_eq!(state.selected, expected, "{name}");
        }
    }

    #[test]
    fn projection_marks_every_member_of_a_seven_rule_duplicate_block() {
        // Marking only duplicates after their first occurrence makes index 0
        // fail; marking only the first pair makes indices 2 through 6 fail.
        let duplicate = json!({
            "pattern": "endpoint/duplicate/*",
            "roles": "internal/role/duplicate-reader",
            "methods": "read"
        });
        let mut rules = vec![duplicate; 7];
        rules.push(json!({"pattern": "unique", "roles": "*", "methods": "read"}));
        let document = Document::from_value(json!({"_id": "access", "configs": rules})).unwrap();

        assert!(document.rows[..7].iter().all(|row| row.duplicate));
        assert!(!document.rows[7].duplicate);
    }
}
