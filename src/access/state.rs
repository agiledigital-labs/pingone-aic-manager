//! Access-tab state: per-tenant raw rule documents, search, and selection.

use std::cell::Cell;
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
    pub summary: spec::RuleSummary,
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
        let rows = spec::rule_summaries(rules)
            .into_iter()
            .zip(rules)
            .map(|(summary, rule)| RuleRow {
                summary,
                raw: rule.clone(),
            })
            .collect();
        Ok(Self {
            digest: spec::digest(&value),
            rows,
        })
    }
}

impl RuleRow {
    fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {}",
            self.summary.index,
            self.summary.digest,
            self.summary.pattern,
            self.summary.methods,
            self.summary.roles,
            self.summary.actions.as_deref().unwrap_or_default(),
            self.summary.custom_authz.as_deref().unwrap_or_default(),
            self.summary.exclude_patterns.as_deref().unwrap_or_default(),
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
    pub detail_scroll: usize,
    detail_scroll_limit: Cell<usize>,
}

impl State {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            query: LineEditor::new(),
            selected: 0,
            scroll: 0,
            detail_scroll: 0,
            detail_scroll_limit: Cell::new(0),
        }
    }

    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.reset_detail_scroll();
    }

    pub fn clamp_selection(&mut self, count: usize) {
        self.select(self.selected, count);
    }

    pub fn move_selection(&mut self, count: usize, delta: isize) {
        if count == 0 {
            self.select(0, count);
            return;
        }
        let selected = (self.selected as isize + delta).clamp(0, count as isize - 1) as usize;
        self.select(selected, count);
    }

    pub fn select(&mut self, index: usize, count: usize) {
        let selected = index.min(count.saturating_sub(1));
        if self.selected != selected {
            self.selected = selected;
            self.reset_detail_scroll();
        }
    }

    pub fn scroll_detail(&mut self, delta: isize) {
        let limit = self.detail_scroll_limit.get();
        let current = self.detail_scroll.min(limit);
        let requested = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as usize)
        };
        self.detail_scroll = requested.min(limit);
    }

    pub fn clamp_detail_scroll(&self, rendered_height: usize, viewport_height: usize) -> usize {
        let limit = crate::tui::list_chrome::clamp_detail_scroll(
            usize::MAX,
            rendered_height,
            viewport_height,
        );
        self.detail_scroll_limit.set(limit);
        self.detail_scroll.min(limit)
    }

    pub fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
        self.detail_scroll_limit.set(0);
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
                .then_with(|| a.row.summary.index.cmp(&b.row.summary.index))
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

        assert_eq!(document.rows[0].summary.actions, None);
        assert!(document.rows[0].raw.get("actions").is_none());
        assert_eq!(document.rows[1].summary.actions.as_deref(), Some("*"));
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

        assert!(document.rows[..7].iter().all(|row| row.summary.duplicate));
        assert!(!document.rows[7].summary.duplicate);
    }

    #[test]
    fn absent_actions_are_not_searchable_but_present_actions_are() {
        // Restoring the `<absent>` display sentinel to `search_text`, or
        // omitting present action values, makes one of these searches fail.
        let mut state = State::new();
        let row = |actions: Option<String>| RuleRow {
            summary: spec::RuleSummary {
                index: 0,
                digest: "zz".into(),
                duplicate: false,
                pattern: "x".into(),
                methods: "read".into(),
                roles: "*".into(),
                actions: actions.clone(),
                custom_authz: None,
                exclude_patterns: None,
            },
            raw: json!({"pattern": "x", "roles": "*", "methods": "read"}),
        };
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(Document {
                digest: "document".into(),
                rows: vec![row(None)],
            }),
        );

        state.query.set("absent");
        assert!(state.matches(Some("sandbox")).is_empty());

        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(Document {
                digest: "document".into(),
                rows: vec![row(Some("approve-report".into()))],
            }),
        );
        state.query.set("approve-report");
        let matches = state.matches(Some("sandbox"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].row.summary.index, 0);
    }

    #[test]
    fn marker_literals_and_glyphs_are_not_search_terms() {
        // Adding presentation markers to the value-field haystack makes at
        // least one of these otherwise absent queries match the duplicate.
        let mut state = State::new();
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(Document {
                digest: "0".into(),
                rows: vec![RuleRow {
                    summary: spec::RuleSummary {
                        index: 0,
                        digest: "0".into(),
                        duplicate: true,
                        pattern: "x".into(),
                        methods: "q".into(),
                        roles: "*".into(),
                        actions: None,
                        custom_authz: None,
                        exclude_patterns: None,
                    },
                    raw: json!({"pattern": "x", "roles": "*", "methods": "q"}),
                }],
            }),
        );

        for marker in ["yes", "dup", "A", "D", "AD"] {
            state.query.set(marker);
            assert!(
                state.matches(Some("sandbox")).is_empty(),
                "presentation marker {marker:?} became searchable"
            );
        }
    }

    #[test]
    fn roles_are_searchable_by_full_path_or_tui_form() {
        // Removing the full value from the haystack, or relying only on exact
        // comma-list matching, makes the stripped-form query fail.
        let mut state = State::new();
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(Document::from_value(crate::access::six_rule_fixture()).unwrap()),
        );

        for query in ["internal/role/user-reader", "user-reader"] {
            state.query.set(query);
            let matches = state.matches(Some("sandbox"));
            assert_eq!(matches.len(), 1, "query {query:?}");
            assert_eq!(matches[0].row.summary.index, 0, "query {query:?}");
        }
    }

    #[test]
    fn detail_scroll_clamps_to_the_last_rendered_height_and_resets() {
        // Dropping the state-side limit lets repeated scroll-down actions
        // accumulate beyond the five rendered rows available for scrolling.
        let mut state = State::new();
        assert_eq!(state.clamp_detail_scroll(15, 10), 0);
        state.detail_scroll = 50;
        state.scroll_detail(-10);
        assert_eq!(state.detail_scroll, 0);
        for _ in 0..5 {
            state.scroll_detail(10);
        }
        assert_eq!(state.detail_scroll, 5);

        state.scroll_detail(-10);
        assert_eq!(state.detail_scroll, 0);
        state.scroll_detail(10);
        state.select(1, 3);
        assert_eq!(state.detail_scroll, 0);
    }
}
