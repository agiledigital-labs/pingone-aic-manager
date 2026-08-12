//! Tenant-free access-rule input types, parsing, validation, and digests.

use std::collections::{BTreeSet, HashSet};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub const KNOWN_METHODS: [&str; 9] = [
    "read", "query", "create", "update", "delete", "patch", "action", "script", "*",
];

/// Role strings known from internal-role ids and authentication mappings.
///
/// Populated indices come from [`crate::access::api::role_index`]. The explicit
/// empty constructor is for callers that intentionally have no tenant data;
/// callers that failed to fetch tenant data should pass `None` to validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleIndex(HashSet<String>);

impl RoleIndex {
    pub(super) fn from_roles(roles: impl IntoIterator<Item = String>) -> Self {
        Self(roles.into_iter().collect())
    }

    pub(super) fn extend(&mut self, roles: impl IntoIterator<Item = String>) {
        self.0.extend(roles);
    }

    fn contains(&self, role: &str) -> bool {
        self.0.contains(role)
    }
}

/// Original and resulting rule indices affected by one transform.
///
/// Removals have only `before` indices and appends only `after` indices. This
/// keeps shifted survivors from being mistaken for touched rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TouchedIndices {
    before: BTreeSet<usize>,
    after: BTreeSet<usize>,
}

impl TouchedIndices {
    pub(super) fn appended(index: usize) -> Self {
        Self {
            before: BTreeSet::new(),
            after: BTreeSet::from([index]),
        }
    }

    pub(super) fn replaced(index: usize) -> Self {
        Self {
            before: BTreeSet::from([index]),
            after: BTreeSet::from([index]),
        }
    }

    pub(super) fn removed(indices: BTreeSet<usize>) -> Self {
        Self {
            before: indices,
            after: BTreeSet::new(),
        }
    }

    pub(super) fn from_sets(before: BTreeSet<usize>, after: BTreeSet<usize>) -> Self {
        Self { before, after }
    }

    pub fn before(&self) -> &BTreeSet<usize> {
        &self.before
    }

    pub fn after(&self) -> &BTreeSet<usize> {
        &self.after
    }
}

/// Which rules should emit advisory findings during document validation.
#[derive(Debug, Clone, Copy)]
pub enum WarningScope<'a> {
    All,
    Touched(&'a TouchedIndices),
}

impl WarningScope<'_> {
    fn includes(self, index: usize) -> bool {
        match self {
            Self::All => true,
            Self::Touched(touched) => touched.after.contains(&index),
        }
    }
}

/// Fields used to construct one new access rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    pub pattern: String,
    pub roles: String,
    pub methods: String,
    pub actions: Option<String>,
    pub custom_authz: Option<String>,
    pub exclude_patterns: Option<String>,
}

/// Partial changes to an existing rule. An absent value leaves the key alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleEdit {
    pub pattern: Option<String>,
    pub roles: Option<String>,
    pub methods: Option<String>,
    pub actions: Option<String>,
    pub custom_authz: Option<String>,
    pub exclude_patterns: Option<String>,
    pub clear_actions: bool,
    pub clear_custom_authz: bool,
    pub clear_exclude_patterns: bool,
}

/// One tenant-free amendment to the complete access document.
///
/// `Apply` remains here even though the TUI never constructs it: the CLI's
/// file workflow still needs the same pure amendment decision and change
/// summary as the interactive add/edit/remove paths.
#[derive(Debug, Clone, PartialEq)]
pub enum Amendment {
    Add(RuleSpec),
    Edit { index: usize, edit: RuleEdit },
    Remove(Vec<usize>),
    Apply(Value),
}

/// Read-only projection of a raw rule for display and validation.
///
/// This type deliberately has no conversion back into [`Value`]. Transforms
/// must mutate the raw rule so that absent and unknown keys survive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleView {
    pub pattern: String,
    pub roles: String,
    pub methods: String,
    pub actions: Option<String>,
    pub custom_authz: Option<String>,
    pub exclude_patterns: Option<String>,
}

impl RuleView {
    pub fn from_value(rule: &Value) -> Self {
        Self {
            pattern: string_field(rule, "pattern")
                .unwrap_or_default()
                .to_string(),
            roles: string_field(rule, "roles").unwrap_or_default().to_string(),
            methods: string_field(rule, "methods")
                .unwrap_or_default()
                .to_string(),
            actions: string_field(rule, "actions").map(str::to_owned),
            custom_authz: string_field(rule, "customAuthz").map(str::to_owned),
            exclude_patterns: string_field(rule, "excludePatterns").map(str::to_owned),
        }
    }
}

/// Shared read projection of one access rule.
///
/// The raw [`Value`] deliberately stays with each caller: the CLI borrows it
/// for stable `--json` output, while the TUI owns it for the detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSummary {
    pub index: usize,
    pub digest: String,
    pub duplicate: bool,
    pub pattern: String,
    pub methods: String,
    pub roles: String,
    pub actions: Option<String>,
    pub custom_authz: Option<String>,
    pub exclude_patterns: Option<String>,
}

impl RuleSummary {
    fn new(index: usize, rule: &Value, duplicate: bool) -> Self {
        let view = RuleView::from_value(rule);
        Self {
            index,
            digest: short_digest(rule),
            duplicate,
            pattern: view.pattern,
            methods: view.methods,
            roles: view.roles,
            actions: view.actions,
            custom_authz: view.custom_authz,
            exclude_patterns: view.exclude_patterns,
        }
    }
}

/// Project all rules and mark every member of each duplicate group.
pub fn rule_summaries(rules: &[Value]) -> Vec<RuleSummary> {
    let duplicates = crate::access::ops::duplicate_flags(rules);
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| RuleSummary::new(index, rule, duplicates[index]))
        .collect()
}

/// One structural validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub index: Option<usize>,
    pub message: String,
}

/// Fatal and advisory structural validation diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

impl Findings {
    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(Finding {
            index: None,
            message: message.into(),
        });
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(Finding {
            index: None,
            message: message.into(),
        });
    }

    fn warning_at(&mut self, index: usize, message: impl Into<String>) {
        self.warnings.push(Finding {
            index: Some(index),
            message: message.into(),
        });
    }

    fn extend_rule(&mut self, mut other: Self, index: usize, include_warnings: bool) {
        for finding in &mut other.errors {
            finding.index = Some(index);
        }
        self.errors.extend(other.errors);
        if include_warnings {
            for finding in &mut other.warnings {
                finding.index = Some(index);
            }
            self.warnings.extend(other.warnings);
        }
    }
}

/// Validate one rule without performing tenant I/O.
pub fn validate_rule(rule: &Value, known_roles: Option<&RoleIndex>) -> Findings {
    let view = RuleView::from_value(rule);
    let mut findings = Findings::default();

    if view.pattern.trim().is_empty() {
        findings.error("access rule pattern cannot be empty");
    }
    if view.methods.trim().is_empty() {
        findings.error("access rule methods cannot be empty");
    }

    for role in view.roles.split(',').map(str::trim) {
        if !role_form(role) {
            findings.error(invalid_role_message(role));
        } else if role != "*" && known_roles.is_some_and(|known| !known.contains(role)) {
            findings.warning(format!(
                "role reference {role:?} is absent from internal roles and config/authentication mappings"
            ));
        }
    }

    if !view.methods.trim().is_empty() {
        for method in view.methods.split(',').map(str::trim) {
            if !KNOWN_METHODS.contains(&method) {
                findings.warning(format!(
                    "unrecognised access method {method:?}; IDM publishes no method enum"
                ));
            }
        }
    }

    if rule.get("customAuthz").is_some() {
        findings.warning(
            "customAuthz can only deny; it cannot widen access beyond pattern and methods",
        );
    }

    findings
}

/// Validate a complete document, including apply-only shape and duplicate checks.
///
/// Errors always cover the complete document. Warnings cover only `scope`.
pub fn validate_document(
    doc: &Value,
    known_roles: Option<&RoleIndex>,
    scope: WarningScope<'_>,
) -> Findings {
    let mut findings = Findings::default();
    let Some(object) = doc.as_object() else {
        findings.error("config/access document must be a JSON object");
        return findings;
    };

    for key in object.keys() {
        if key != "_id" && key != "configs" {
            findings.warning(format!("unrecognised top-level config/access key {key:?}"));
        }
    }

    let Some(rules) = object.get("configs").and_then(Value::as_array) else {
        findings.error("config/access document must contain a `configs` array of objects");
        return findings;
    };
    if rules.iter().any(|rule| !rule.is_object()) {
        findings.error("config/access document must contain a `configs` array of objects");
        return findings;
    }

    let duplicates = crate::access::ops::duplicate_flags(rules);
    for (index, rule) in rules.iter().enumerate() {
        let include_warnings = scope.includes(index);
        findings.extend_rule(validate_rule(rule, known_roles), index, include_warnings);
        let duplicate_warning = duplicates[index]
            && (!matches!(scope, WarningScope::All) || rules[..index].contains(rule));
        if include_warnings && duplicate_warning {
            findings.warning_at(
                index,
                "access rule is byte-identical to another rule; duplicates are legal",
            );
        }
    }
    findings
}

/// Resolve an index or displayed/full digest to every matching rule index.
pub fn resolve_rule_address(rules: &[Value], address: &str) -> Result<BTreeSet<usize>> {
    if let Ok(index) = address.parse::<usize>() {
        if index < rules.len() {
            return Ok(BTreeSet::from([index]));
        }
    }

    let matches = rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            (short_digest(rule).eq_ignore_ascii_case(address)
                || digest(rule).eq_ignore_ascii_case(address))
            .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    if matches.is_empty() {
        Err(Error::Config(format!(
            "no access rule has digest {address:?}; use an 8-character digest from `aic access list`"
        )))
    } else {
        Ok(matches)
    }
}

/// Check a caller-supplied whole-document digest precondition.
pub fn check_digest(expected: Option<&str>, document: &Value) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = digest(document);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "config/access changed since document digest {expected}; the live digest is {actual}; nothing was written"
        )))
    }
}

/// SHA-256 of the deterministic JSON representation, encoded as lowercase hex.
pub fn digest(value: &Value) -> String {
    let mut json = String::new();
    write_canonical_json(value, &mut json);
    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    hex
}

fn write_canonical_json(value: &Value, output: &mut String) {
    match value {
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_json(value, output);
            }
            output.push(']');
        }
        Value::Object(object) => {
            output.push('{');
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key).expect("serialize serde_json object key"),
                );
                output.push(':');
                write_canonical_json(&object[key], output);
            }
            output.push('}');
        }
        primitive => output
            .push_str(&serde_json::to_string(primitive).expect("serialize serde_json primitive")),
    }
}

/// Eight-character digest used when displaying a rule.
pub fn short_digest(value: &Value) -> String {
    digest(value)[..8].to_string()
}

fn string_field<'a>(rule: &'a Value, key: &str) -> Option<&'a str> {
    rule.get(key).and_then(Value::as_str)
}

fn role_form(role: &str) -> bool {
    role == "*"
        || role
            .strip_prefix("internal/role/")
            .is_some_and(|id| !id.is_empty())
}

pub fn comma_list_contains(rule: &Value, field: &str, needle: &str) -> bool {
    string_field(rule, field)
        .is_some_and(|value| value.split(',').map(str::trim).any(|item| item == needle))
}

pub(super) fn ensure_index(index: usize, len: usize) -> Result<()> {
    if index < len {
        return Ok(());
    }
    let range = if len == 0 {
        "no indices are valid because `configs` is empty".to_string()
    } else {
        format!("the valid range is 0..={}", len - 1)
    };
    Err(Error::Config(format!(
        "access rule index {index} is out of range; {range}"
    )))
}

fn invalid_role_message(role: &str) -> String {
    format!(
        "invalid role entry {role:?}; expected `*` or `internal/role/<id>`. A bare value is probably a role name, and a name never matches; use its role id"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use crate::access::ops;

    use super::*;

    fn known_roles() -> RoleIndex {
        RoleIndex::from_roles(
            ["internal/role/x", "internal/role/a", "internal/role/b"]
                .into_iter()
                .map(str::to_string),
        )
    }

    #[test]
    fn shared_projection_preserves_values_for_cli_and_tui_presentations() {
        // Reintroducing either caller's former projection, or replacing an
        // absent value with a display default, makes these fields fail.
        let fixture = crate::access::six_rule_fixture();
        let summaries = rule_summaries(fixture["configs"].as_array().unwrap());

        assert_eq!(summaries[0].index, 0);
        assert_eq!(summaries[0].digest, "ab516c84");
        assert_eq!(summaries[0].pattern, "managed/alpha_user/*");
        assert_eq!(summaries[0].methods, "read,query");
        assert_eq!(summaries[0].roles, "internal/role/user-reader");
        assert_eq!(summaries[0].actions, None);
        assert_eq!(summaries[1].actions.as_deref(), Some("*"));
        assert_eq!(summaries[1].custom_authz.as_deref(), Some("ownDataOnly()"));
        assert!(summaries[4].duplicate);
        assert!(summaries[5].duplicate);
    }

    #[test]
    fn role_forms_are_validated_as_a_table() {
        for (roles, valid) in [
            ("*", true),
            ("internal/role/x", true),
            ("internal/role/a,internal/role/b", true),
            ("x", false),
            ("", false),
        ] {
            let rule = json!({"pattern": "managed/x", "roles": roles, "methods": "read"});
            let roles = known_roles();
            let findings = validate_rule(&rule, Some(&roles));
            assert_eq!(
                findings.errors.is_empty(),
                valid,
                "unexpected result for {roles:?}: {findings:?}"
            );
            if !valid {
                let message = &findings.errors[0].message;
                assert!(message.contains("role name"), "{message}");
                assert!(message.contains("name never matches"), "{message}");
            }
        }
    }

    #[test]
    fn methods_are_advisory_and_table_driven() {
        for (methods, warns) in [("read", false), ("frobnicate", true)] {
            let rule = json!({"pattern": "managed/x", "roles": "*", "methods": methods});
            let empty = RoleIndex::from_roles(std::iter::empty());
            let findings = validate_rule(&rule, Some(&empty));
            assert!(findings.errors.is_empty(), "{findings:?}");
            assert_eq!(
                findings.warnings.is_empty(),
                !warns,
                "unexpected result for {methods:?}: {findings:?}"
            );
        }
    }

    #[test]
    fn optional_actions_edits_preserve_absence_and_every_other_key() {
        let fixture = crate::access::six_rule_fixture();
        let cases = [
            ("absent unchanged", 0, RuleEdit::default(), None),
            (
                "absent set empty",
                0,
                RuleEdit {
                    actions: Some(String::new()),
                    ..RuleEdit::default()
                },
                Some(""),
            ),
            ("present unchanged", 1, RuleEdit::default(), Some("*")),
            (
                "present set empty",
                1,
                RuleEdit {
                    actions: Some(String::new()),
                    ..RuleEdit::default()
                },
                Some(""),
            ),
            (
                "present cleared",
                1,
                RuleEdit {
                    clear_actions: true,
                    ..RuleEdit::default()
                },
                None,
            ),
        ];

        for (name, index, edit, expected_actions) in cases {
            let before_rule = fixture["configs"][index].as_object().unwrap();
            let transformed = ops::replace_at(&fixture, index, edit).unwrap();
            let after_rule = transformed.document["configs"][index].as_object().unwrap();
            assert_eq!(
                after_rule.get("actions").and_then(Value::as_str),
                expected_actions,
                "{name}"
            );

            let mut before_without_actions = before_rule.clone();
            let mut after_without_actions = after_rule.clone();
            before_without_actions.remove("actions");
            after_without_actions.remove("actions");
            assert_eq!(before_without_actions, after_without_actions, "{name}");
        }

        for (name, index, field, edit) in [
            (
                "clear customAuthz",
                1,
                "customAuthz",
                RuleEdit {
                    clear_custom_authz: true,
                    ..RuleEdit::default()
                },
            ),
            (
                "clear excludePatterns",
                2,
                "excludePatterns",
                RuleEdit {
                    clear_exclude_patterns: true,
                    ..RuleEdit::default()
                },
            ),
        ] {
            let before_rule = fixture["configs"][index].as_object().unwrap();
            let transformed = ops::replace_at(&fixture, index, edit).unwrap();
            let after_rule = transformed.document["configs"][index].as_object().unwrap();
            assert!(!after_rule.contains_key(field), "{name}");

            let mut before_without_field = before_rule.clone();
            let mut after_without_field = after_rule.clone();
            before_without_field.remove(field);
            after_without_field.remove(field);
            assert_eq!(before_without_field, after_without_field, "{name}");
        }

        for (name, edit) in [
            (
                "actions",
                RuleEdit {
                    actions: Some("*".into()),
                    clear_actions: true,
                    ..RuleEdit::default()
                },
            ),
            (
                "customAuthz",
                RuleEdit {
                    custom_authz: Some("false".into()),
                    clear_custom_authz: true,
                    ..RuleEdit::default()
                },
            ),
            (
                "excludePatterns",
                RuleEdit {
                    exclude_patterns: Some("managed/private/*".into()),
                    clear_exclude_patterns: true,
                    ..RuleEdit::default()
                },
            ),
        ] {
            let error = ops::replace_at(&fixture, 1, edit).unwrap_err();
            assert!(
                matches!(error, Error::Config(ref message) if message.contains("set and clear") && message.contains(name)),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn unknown_role_and_custom_authz_are_warnings() {
        let rule = json!({
            "pattern": "managed/x",
            "roles": "internal/role/missing",
            "methods": "read",
            "customAuthz": "false"
        });
        let empty = RoleIndex::from_roles(std::iter::empty());
        let findings = validate_rule(&rule, Some(&empty));

        assert!(findings.errors.is_empty());
        assert_eq!(findings.warnings.len(), 2);

        let without_index = validate_rule(&rule, None);
        assert_eq!(without_index.warnings.len(), 1);
        assert!(without_index.warnings[0].message.contains("customAuthz"));
    }

    #[test]
    fn document_shape_errors_are_whole_document_and_table_driven() {
        for (name, document) in [
            ("not object", json!([])),
            ("configs missing", json!({"_id": "access"})),
            ("configs not array", json!({"configs": {}})),
            ("rule not object", json!({"configs": [{}, 1]})),
        ] {
            let findings = validate_document(&document, None, WarningScope::All);
            assert_eq!(findings.errors.len(), 1, "{name}: {findings:?}");
            assert!(
                findings.errors[0]
                    .message
                    .contains(if name == "not object" {
                        "JSON object"
                    } else {
                        "`configs` array of objects"
                    }),
                "{name}: {findings:?}"
            );
        }

        let fixture = crate::access::six_rule_fixture();
        let findings = validate_document(&fixture, None, WarningScope::All);
        assert!(findings.warnings.iter().any(
            |finding| finding.index.is_none() && finding.message.contains("unknownTopLevelKey")
        ));
        assert!(
            findings
                .warnings
                .iter()
                .any(|finding| finding.index == Some(5)
                    && finding.message.contains("byte-identical"))
        );
    }

    #[test]
    fn touched_warnings_are_scoped_but_errors_remain_document_wide() {
        let fixture = crate::access::six_rule_fixture();
        let transformed = ops::replace_at(
            &fixture,
            0,
            RuleEdit {
                methods: Some("read".into()),
                ..RuleEdit::default()
            },
        )
        .unwrap();
        let findings = validate_document(
            &transformed.document,
            None,
            WarningScope::Touched(&transformed.touched),
        );
        assert_eq!(findings.warnings.len(), 1, "{findings:?}");
        assert!(findings.warnings[0].index.is_none());

        let mut malformed = transformed.document;
        malformed["configs"][2]["pattern"] = json!("");
        let findings = validate_document(
            &malformed,
            None,
            WarningScope::Touched(&transformed.touched),
        );
        assert!(
            findings
                .errors
                .iter()
                .any(|finding| finding.index == Some(2) && finding.message.contains("pattern"))
        );
        assert_eq!(findings.warnings.len(), 1, "{findings:?}");
    }

    #[test]
    fn rule_addresses_and_document_preconditions_are_shared_helpers() {
        let fixture = crate::access::six_rule_fixture();
        let rules = fixture["configs"].as_array().unwrap();
        assert_eq!(
            resolve_rule_address(rules, "2").unwrap(),
            BTreeSet::from([2])
        );

        let duplicate_digest = short_digest(&rules[4]);
        assert_eq!(
            resolve_rule_address(rules, &duplicate_digest).unwrap(),
            BTreeSet::from([4, 5])
        );

        let expected = digest(&fixture);
        assert!(check_digest(Some(&expected.to_uppercase()), &fixture).is_ok());
        let error = check_digest(Some("deadbeef"), &fixture).unwrap_err();
        assert!(
            matches!(error, Error::Config(message) if message.contains("deadbeef") && message.contains(&expected))
        );
    }

    #[test]
    fn numeric_out_of_range_addresses_can_resolve_as_digests() {
        let numeric_rule = (0_u64..)
            .map(|seed| json!({"seed": seed}))
            .find(|rule| short_digest(rule).bytes().all(|byte| byte.is_ascii_digit()))
            .expect("find an all-numeric short digest");
        let address = short_digest(&numeric_rule);
        let rules = vec![numeric_rule];

        assert_eq!(
            resolve_rule_address(&rules, &address).unwrap(),
            BTreeSet::from([0])
        );
    }

    #[test]
    fn comma_list_matching_is_shared_and_exact() {
        let rule = json!({"roles": "internal/role/a, internal/role/b"});
        assert!(comma_list_contains(&rule, "roles", "internal/role/b"));
        assert!(!comma_list_contains(&rule, "roles", "role/b"));
    }

    #[test]
    fn digest_canonicalises_different_object_insertion_orders() {
        let mut first = Map::new();
        first.insert("pattern".into(), json!("managed/x"));
        first.insert("roles".into(), json!("*"));
        first.insert("methods".into(), json!("read"));

        let mut second = Map::new();
        second.insert("methods".into(), json!("read"));
        second.insert("roles".into(), json!("*"));
        second.insert("pattern".into(), json!("managed/x"));

        assert_eq!(
            digest(&Value::Object(first)),
            digest(&Value::Object(second))
        );
    }
}
