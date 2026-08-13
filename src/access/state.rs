//! Access-tab state: per-tenant raw rule documents, search, and selection.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::access::{ops, spec};
use crate::tui::list_chrome::DetailScroll;
use crate::tui::widgets::{LineEditor, TextField};

#[derive(Debug)]
pub enum LoadState {
    Loading,
    Loaded(Document),
    Failed(String),
}

#[derive(Debug)]
pub enum RoleIndexState {
    Loading,
    Loaded(spec::RoleIndex),
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
    /// Untouched whole document used as the write precondition and undo body.
    pub value: Value,
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
            value,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFocus {
    Pattern,
    Roles,
    Methods,
    Actions,
    CustomAuthz,
    ExcludePatterns,
    Save,
}

impl FormFocus {
    const ORDER: [Self; 7] = [
        Self::Pattern,
        Self::Roles,
        Self::Methods,
        Self::Actions,
        Self::CustomAuthz,
        Self::ExcludePatterns,
        Self::Save,
    ];

    pub fn next(self) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        Self::ORDER[(index + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        Self::ORDER[(index + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    pub fn optional(self) -> bool {
        matches!(
            self,
            Self::Actions | Self::CustomAuthz | Self::ExcludePatterns
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalEdit {
    Unchanged,
    Set,
    Clear,
}

impl OptionalEdit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unchanged => "keep",
            Self::Set => "set",
            Self::Clear => "clear",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptionalField {
    pub input: TextField,
    pub edit: Option<OptionalEdit>,
}

impl OptionalField {
    fn create(label: &str) -> Self {
        Self {
            input: TextField::single_line(label),
            edit: None,
        }
    }

    fn edit(label: &str, value: Option<&str>) -> Self {
        Self {
            input: TextField::single_line(label).with_initial(value.unwrap_or_default()),
            edit: Some(OptionalEdit::Unchanged),
        }
    }

    pub fn set_clear(&mut self) {
        self.edit = Some(OptionalEdit::Clear);
    }

    pub fn set_unchanged(&mut self) {
        self.edit = Some(OptionalEdit::Unchanged);
    }

    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        let before = self.input.value.clone();
        let handled = self.input.handle_key(key);
        if handled && self.input.value != before && self.edit.is_some() {
            self.edit = Some(OptionalEdit::Set);
        }
        handled
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Create,
    Edit { index: usize },
}

#[derive(Debug, Clone)]
pub struct RuleFormState {
    pub tenant: String,
    pub original_document: Value,
    pub original_digest: String,
    pub original_rule_digest: Option<String>,
    pub kind: FormKind,
    pub pattern: TextField,
    pattern_seed: String,
    pub roles: TextField,
    roles_seed: String,
    pub methods: TextField,
    methods_seed: String,
    pub actions: OptionalField,
    pub custom_authz: OptionalField,
    pub exclude_patterns: OptionalField,
    pub focused: FormFocus,
    confirming: bool,
    pub error: Option<String>,
    pub known_roles: Option<spec::RoleIndex>,
    pub role_check_note: Option<String>,
    pub review_warnings: Vec<String>,
}

impl RuleFormState {
    pub fn create(tenant: String, document: &Document) -> Self {
        Self {
            tenant,
            original_document: document.value.clone(),
            original_digest: document.digest.clone(),
            original_rule_digest: None,
            kind: FormKind::Create,
            pattern: TextField::single_line("Pattern"),
            pattern_seed: String::new(),
            roles: TextField::single_line("Roles (full internal/role/<id> paths)"),
            roles_seed: String::new(),
            methods: TextField::single_line("Methods"),
            methods_seed: String::new(),
            actions: OptionalField::create("Actions (optional)"),
            custom_authz: OptionalField::create("customAuthz (optional)"),
            exclude_patterns: OptionalField::create("excludePatterns (optional)"),
            focused: FormFocus::Pattern,
            confirming: false,
            error: None,
            known_roles: None,
            role_check_note: None,
            review_warnings: Vec::new(),
        }
    }

    pub fn edit(tenant: String, document: &Document, row: &RuleRow) -> Self {
        Self {
            tenant,
            original_document: document.value.clone(),
            original_digest: document.digest.clone(),
            original_rule_digest: Some(spec::digest(&row.raw)),
            kind: FormKind::Edit {
                index: row.summary.index,
            },
            pattern: TextField::single_line("Pattern").with_initial(&row.summary.pattern),
            pattern_seed: row.summary.pattern.clone(),
            roles: TextField::single_line("Roles (full internal/role/<id> paths)")
                .with_initial(&row.summary.roles),
            roles_seed: row.summary.roles.clone(),
            methods: TextField::single_line("Methods").with_initial(&row.summary.methods),
            methods_seed: row.summary.methods.clone(),
            actions: OptionalField::edit("Actions (optional)", row.summary.actions.as_deref()),
            custom_authz: OptionalField::edit(
                "customAuthz (optional)",
                row.summary.custom_authz.as_deref(),
            ),
            exclude_patterns: OptionalField::edit(
                "excludePatterns (optional)",
                row.summary.exclude_patterns.as_deref(),
            ),
            focused: FormFocus::Pattern,
            confirming: false,
            error: None,
            known_roles: None,
            role_check_note: None,
            review_warnings: Vec::new(),
        }
    }

    pub fn set_role_validation(
        &mut self,
        known_roles: Option<spec::RoleIndex>,
        note: Option<String>,
    ) {
        self.known_roles = known_roles;
        self.role_check_note = note;
    }

    pub fn amendment(&self) -> spec::Amendment {
        match self.kind {
            // Create treats an empty optional input as absent.
            FormKind::Create => spec::Amendment::Add(spec::RuleSpec {
                pattern: self.pattern.value.clone(),
                roles: self.roles.value.clone(),
                methods: self.methods.value.clone(),
                actions: nonempty(&self.actions.input.value),
                custom_authz: nonempty(&self.custom_authz.input.value),
                exclude_patterns: nonempty(&self.exclude_patterns.input.value),
            }),
            // Edit preserves an explicitly set empty string as a present value.
            FormKind::Edit { index } => spec::Amendment::Edit {
                index,
                edit: spec::RuleEdit {
                    pattern: changed(&self.pattern, &self.pattern_seed),
                    roles: changed(&self.roles, &self.roles_seed),
                    methods: changed(&self.methods, &self.methods_seed),
                    actions: optional_value(&self.actions),
                    custom_authz: optional_value(&self.custom_authz),
                    exclude_patterns: optional_value(&self.exclude_patterns),
                    clear_actions: self.actions.edit == Some(OptionalEdit::Clear),
                    clear_custom_authz: self.custom_authz.edit == Some(OptionalEdit::Clear),
                    clear_exclude_patterns: self.exclude_patterns.edit == Some(OptionalEdit::Clear),
                },
            },
        }
    }

    pub fn optional_mut(&mut self) -> Option<&mut OptionalField> {
        match self.focused {
            FormFocus::Actions => Some(&mut self.actions),
            FormFocus::CustomAuthz => Some(&mut self.custom_authz),
            FormFocus::ExcludePatterns => Some(&mut self.exclude_patterns),
            _ => None,
        }
    }

    pub fn review(&mut self) {
        self.confirming = true;
    }

    pub fn unreview(&mut self) {
        self.confirming = false;
    }

    pub fn confirming(&self) -> bool {
        self.confirming
    }
}

fn changed(field: &TextField, seed: &str) -> Option<String> {
    (field.value != seed).then(|| field.value.clone())
}

fn optional_value(field: &OptionalField) -> Option<String> {
    (field.edit == Some(OptionalEdit::Set)).then(|| field.input.value.clone())
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

#[derive(Debug, Clone)]
pub struct DeleteState {
    pub tenant: String,
    pub original_document: Value,
    pub original_digest: String,
    pub index: usize,
    pub rule_digest: String,
    confirmed: bool,
}

impl DeleteState {
    pub fn new(tenant: String, document: &Document, row: &RuleRow) -> Self {
        Self {
            tenant,
            original_document: document.value.clone(),
            original_digest: document.digest.clone(),
            index: row.summary.index,
            rule_digest: row.summary.digest.clone(),
            confirmed: false,
        }
    }

    pub fn confirm(&mut self) {
        self.confirmed = true;
    }

    pub fn confirmed(&self) -> bool {
        self.confirmed
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
    pub role_indices: HashMap<String, RoleIndexState>,
    pub role_refreshing: HashSet<String>,
    pub query: LineEditor,
    pub selected: usize,
    pub scroll: usize,
    pub detail_scroll: DetailScroll,
    pub form: Option<RuleFormState>,
    pub pending_delete: Option<DeleteState>,
    pub in_flight_writes: HashSet<String>,
}

impl State {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            refreshing: HashSet::new(),
            role_indices: HashMap::new(),
            role_refreshing: HashSet::new(),
            query: LineEditor::new(),
            selected: 0,
            scroll: 0,
            detail_scroll: DetailScroll::default(),
            form: None,
            pending_delete: None,
            in_flight_writes: HashSet::new(),
        }
    }

    pub fn reset_view(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.detail_scroll.reset();
        self.form = None;
        self.pending_delete = None;
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
            self.detail_scroll.reset();
        }
    }

    pub fn document(&self, tenant: &str) -> Option<&Document> {
        match self.data.get(tenant) {
            Some(LoadState::Loaded(document)) => Some(document),
            _ => None,
        }
    }

    pub fn role_validation(&self, tenant: &str) -> (Option<spec::RoleIndex>, Option<String>) {
        match self.role_indices.get(tenant) {
            Some(RoleIndexState::Loaded(index)) => (Some(index.clone()), None),
            Some(RoleIndexState::Failed(error)) => (
                None,
                Some(format!(
                    "Role references were not checked because the role index could not be loaded: {error}"
                )),
            ),
            Some(RoleIndexState::Loading) | None => (
                None,
                Some(
                    "Role references were not checked because the role index is still loading"
                        .into(),
                ),
            ),
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

    type FormOptional = for<'a> fn(&'a mut RuleFormState) -> &'a mut OptionalField;
    type EditOptional = for<'a> fn(&'a spec::RuleEdit) -> (Option<&'a str>, bool);

    struct OptionalAccessors {
        name: &'static str,
        form: FormOptional,
        edit: EditOptional,
    }

    const OPTIONAL_ACCESSORS: [OptionalAccessors; 3] = [
        OptionalAccessors {
            name: "actions",
            form: |form| &mut form.actions,
            edit: |edit| (edit.actions.as_deref(), edit.clear_actions),
        },
        OptionalAccessors {
            name: "customAuthz",
            form: |form| &mut form.custom_authz,
            edit: |edit| (edit.custom_authz.as_deref(), edit.clear_custom_authz),
        },
        OptionalAccessors {
            name: "excludePatterns",
            form: |form| &mut form.exclude_patterns,
            edit: |edit| {
                (
                    edit.exclude_patterns.as_deref(),
                    edit.clear_exclude_patterns,
                )
            },
        },
    ];

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
                value: json!({"_id": "access", "configs": []}),
            }),
        );

        state.query.set("absent");
        assert!(state.matches(Some("sandbox")).is_empty());

        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(Document {
                digest: "document".into(),
                rows: vec![row(Some("approve-report".into()))],
                value: json!({"_id": "access", "configs": []}),
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
        // Digest and customAuthz are legitimate search fields, so keep their
        // fixture values marker-free or this test stops isolating presentation.
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
                value: json!({"_id": "access", "configs": []}),
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
    fn edit_form_maps_optional_fields_to_keep_set_and_clear() {
        // Miswiring any RuleFormState optional field to its RuleEdit value or
        // clear flag makes that accessor's table row fail.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let row = document.rows[1].clone();

        for accessors in OPTIONAL_ACCESSORS {
            for (name, change, input, expected_value, expected_clear) in [
                ("untouched", OptionalEdit::Unchanged, "", None, false),
                (
                    "set value",
                    OptionalEdit::Set,
                    "changed",
                    Some("changed"),
                    false,
                ),
                ("set empty", OptionalEdit::Set, "", Some(""), false),
                ("clear", OptionalEdit::Clear, "", None, true),
            ] {
                let mut form = RuleFormState::edit("sandbox".into(), &document, &row);
                let optional = (accessors.form)(&mut form);
                optional.edit = Some(change);
                if change == OptionalEdit::Set {
                    optional.input.set(input);
                }
                let spec::Amendment::Edit { edit, .. } = form.amendment() else {
                    panic!("edit form returned a non-edit amendment");
                };
                let (value, clear) = (accessors.edit)(&edit);
                assert_eq!(value, expected_value, "{} {name}", accessors.name);
                assert_eq!(clear, expected_clear, "{} {name}", accessors.name);
                assert_eq!(
                    edit.pattern, None,
                    "{} {name}: untouched pattern",
                    accessors.name
                );
                assert_eq!(
                    edit.roles, None,
                    "{} {name}: untouched role path",
                    accessors.name
                );
            }
        }
    }

    #[test]
    fn reset_view_discards_tenant_bound_write_drafts() {
        // Removing either reset assignment leaves the old tenant's form or
        // delete confirmation available after State::reset_view.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let mut state = State::new();
        state.form = Some(RuleFormState::create("tenant-a".into(), &document));
        state.pending_delete = Some(DeleteState::new(
            "tenant-a".into(),
            &document,
            &document.rows[0],
        ));

        state.reset_view();

        assert!(state.form.is_none());
        assert!(state.pending_delete.is_none());
    }

    #[test]
    fn failed_role_index_becomes_an_explicit_unchecked_note() {
        // Treating a fetch failure as an empty successful index, or silently
        // as None, makes the review lose this operator-facing caveat.
        let mut state = State::new();
        state.role_indices.insert(
            "sandbox".into(),
            RoleIndexState::Failed("authentication read failed".into()),
        );

        let (index, note) = state.role_validation("sandbox");
        assert!(index.is_none());
        assert!(
            note.is_some_and(|note| note.contains("Role references were not checked")
                && note.contains("authentication read failed"))
        );
    }
}
