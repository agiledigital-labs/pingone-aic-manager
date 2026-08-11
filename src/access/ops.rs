//! Pure, tenant-free transforms over the raw `config/access` document.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::access::spec::{RuleEdit, RuleSpec, TouchedIndices};
use crate::{Error, Result};

/// One changed rule at its document index.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleChange {
    pub index: usize,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// Rule-level changes and the number of rules that remained byte-identical.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub changed: Vec<RuleChange>,
    pub unchanged: usize,
    /// True when an apply diff could only recover approximate source positions.
    pub positions_approximate: bool,
}

/// A transformed document and the exact original/result indices it touched.
#[derive(Debug, Clone, PartialEq)]
pub struct Transformed {
    pub document: Value,
    pub touched: TouchedIndices,
}

/// The `configs` array of an access document.
pub fn rules(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("configs")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/access shape: {doc}"),
        })
}

/// Append one grant without rebuilding or reordering existing rules.
pub fn append(doc: &Value, spec: RuleSpec) -> Result<Transformed> {
    let index = rules(doc)?.len();
    let mut amended = doc.clone();
    rules_mut(&mut amended)?.push(new_rule_value(spec));
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::appended(index),
    })
}

/// Mutate only supplied fields on the rule at `index`.
pub fn replace_at(doc: &Value, index: usize, edit: RuleEdit) -> Result<Transformed> {
    let len = rules(doc)?.len();
    ensure_index(index, len)?;
    reject_conflicting_optional_edits(&edit)?;

    let mut amended = doc.clone();
    let rule = rules_mut(&mut amended)?
        .get_mut(index)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error::Config(format!("access rule at index {index} is not an object")))?;

    replace_when_some(rule, "pattern", edit.pattern);
    replace_when_some(rule, "roles", edit.roles);
    replace_when_some(rule, "methods", edit.methods);
    apply_optional(rule, "actions", edit.actions, edit.clear_actions);
    apply_optional(
        rule,
        "customAuthz",
        edit.custom_authz,
        edit.clear_custom_authz,
    );
    apply_optional(
        rule,
        "excludePatterns",
        edit.exclude_patterns,
        edit.clear_exclude_patterns,
    );
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::replaced(index),
    })
}

/// Remove the original indices as one operation, without index-shift mistakes.
pub fn remove_at(doc: &Value, indices: &[usize]) -> Result<Transformed> {
    let len = rules(doc)?.len();
    for &index in indices {
        ensure_index(index, len)?;
    }

    let mut amended = doc.clone();
    let rules = rules_mut(&mut amended)?;
    let unique = indices.iter().copied().collect::<BTreeSet<_>>();
    for &index in unique.iter().rev() {
        rules.remove(index);
    }
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::removed(unique),
    })
}

/// Compare rule arrays, using exact transform indices when available.
///
/// `touched = None` is the apply path: exact-equal rules are matched as a
/// multiset and the remaining additions/removals carry approximate positions.
pub fn changes(before: &Value, after: &Value, touched: Option<&TouchedIndices>) -> Changes {
    let before = before
        .get("configs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let after = after
        .get("configs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    match touched {
        Some(touched) => touched_changes(before, after, touched),
        None => multiset_changes(before, after),
    }
}

fn touched_changes(before: &[Value], after: &[Value], touched: &TouchedIndices) -> Changes {
    let mut summary = Changes::default();
    for &index in touched.before().intersection(touched.after()) {
        let old = before.get(index);
        let new = after.get(index);
        if old != new {
            summary.changed.push(RuleChange {
                index,
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    for &index in touched.before().difference(touched.after()) {
        summary.changed.push(RuleChange {
            index,
            before: before.get(index).cloned(),
            after: None,
        });
    }
    for &index in touched.after().difference(touched.before()) {
        summary.changed.push(RuleChange {
            index,
            before: None,
            after: after.get(index).cloned(),
        });
    }
    let paired_changes = touched
        .before()
        .intersection(touched.after())
        .filter(|&&index| before.get(index) != after.get(index))
        .count();
    summary.unchanged = before.len().min(after.len()).saturating_sub(paired_changes);
    summary
}

fn multiset_changes(before: &[Value], after: &[Value]) -> Changes {
    let mut used_before = vec![false; before.len()];
    let mut added = Vec::new();
    let mut unchanged = 0;

    for (index, rule) in after.iter().enumerate() {
        if let Some((matched, _)) = before
            .iter()
            .enumerate()
            .find(|(candidate, old)| !used_before[*candidate] && *old == rule)
        {
            used_before[matched] = true;
            unchanged += 1;
        } else {
            added.push(RuleChange {
                index,
                before: None,
                after: Some(rule.clone()),
            });
        }
    }

    let mut changed = before
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_before[*index])
        .map(|(index, rule)| RuleChange {
            index,
            before: Some(rule.clone()),
            after: None,
        })
        .collect::<Vec<_>>();
    changed.extend(added);
    Changes {
        changed,
        unchanged,
        positions_approximate: true,
    }
}

fn rules_mut(doc: &mut Value) -> Result<&mut Vec<Value>> {
    if doc.get("configs").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/access shape: {doc}"),
        });
    }
    Ok(doc
        .get_mut("configs")
        .and_then(Value::as_array_mut)
        .expect("checked configs array above"))
}

fn ensure_index(index: usize, len: usize) -> Result<()> {
    if index < len {
        return Ok(());
    }
    let valid = if len == 0 {
        "no indices are valid because `configs` is empty".to_string()
    } else {
        format!("the valid range is 0..={}", len - 1)
    };
    Err(Error::Config(format!(
        "access rule index {index} is out of range; {valid}"
    )))
}

fn reject_conflicting_optional_edits(edit: &RuleEdit) -> Result<()> {
    for (key, value, clear) in [
        ("actions", &edit.actions, edit.clear_actions),
        ("customAuthz", &edit.custom_authz, edit.clear_custom_authz),
        (
            "excludePatterns",
            &edit.exclude_patterns,
            edit.clear_exclude_patterns,
        ),
    ] {
        if value.is_some() && clear {
            return Err(Error::Config(format!(
                "cannot set and clear access rule field {key:?} in the same edit"
            )));
        }
    }
    Ok(())
}

fn replace_when_some(rule: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        rule.insert(key.to_string(), Value::String(value));
    }
}

fn apply_optional(rule: &mut Map<String, Value>, key: &str, value: Option<String>, clear: bool) {
    if clear {
        rule.remove(key);
    } else {
        replace_when_some(rule, key, value);
    }
}

fn new_rule_value(spec: RuleSpec) -> Value {
    let mut rule = Map::new();
    rule.insert("pattern".into(), Value::String(spec.pattern));
    rule.insert("roles".into(), Value::String(spec.roles));
    rule.insert("methods".into(), Value::String(spec.methods));
    replace_when_some(&mut rule, "actions", spec.actions);
    replace_when_some(&mut rule, "customAuthz", spec.custom_authz);
    replace_when_some(&mut rule, "excludePatterns", spec.exclude_patterns);
    Value::Object(rule)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::access::spec::{digest, short_digest};

    use super::*;

    fn new_rule() -> RuleSpec {
        RuleSpec {
            pattern: "endpoint/new/*".into(),
            roles: "internal/role/new-reader".into(),
            methods: "read".into(),
            actions: None,
            custom_authz: None,
            exclude_patterns: None,
        }
    }

    #[test]
    fn append_preserves_order_and_unknown_keys() {
        let before = crate::access::six_rule_fixture();
        let transformed = append(&before, new_rule()).unwrap();
        let after = transformed.document;

        assert_eq!(&rules(&after).unwrap()[..6], rules(&before).unwrap());
        assert_eq!(after["unknownTopLevelKey"], before["unknownTopLevelKey"]);
        assert_eq!(
            after["configs"][0]["unknownRuleKey"],
            before["configs"][0]["unknownRuleKey"]
        );
        assert!(after["configs"][6].get("actions").is_none());
    }

    #[test]
    fn replace_changes_only_supplied_keys() {
        let before = crate::access::six_rule_fixture();
        let after = replace_at(
            &before,
            0,
            RuleEdit {
                methods: Some("read,query,update".into()),
                ..RuleEdit::default()
            },
        )
        .unwrap()
        .document;

        assert_eq!(after["configs"][0]["methods"], json!("read,query,update"));
        assert!(after["configs"][0].get("actions").is_none());
        assert_eq!(
            after["configs"][0]["unknownRuleKey"],
            before["configs"][0]["unknownRuleKey"]
        );
        assert_eq!(&rules(&after).unwrap()[1..], &rules(&before).unwrap()[1..]);
    }

    #[test]
    fn removing_one_duplicate_removes_exactly_one_rule() {
        let before = crate::access::six_rule_fixture();
        let duplicate = before["configs"][4].clone();
        let after = remove_at(&before, &[4]).unwrap().document;

        assert_eq!(rules(&after).unwrap().len(), 5);
        assert_eq!(
            rules(&after)
                .unwrap()
                .iter()
                .filter(|rule| *rule == &duplicate)
                .count(),
            1
        );
        assert_eq!(&rules(&after).unwrap()[..4], &rules(&before).unwrap()[..4]);
    }

    #[test]
    fn removing_multiple_indices_does_not_shift_later_targets() {
        let before = crate::access::six_rule_fixture();
        let after = remove_at(&before, &[1, 4]).unwrap().document;

        assert_eq!(
            rules(&after).unwrap(),
            &vec![
                before["configs"][0].clone(),
                before["configs"][2].clone(),
                before["configs"][3].clone(),
                before["configs"][5].clone(),
            ]
        );
    }

    #[test]
    fn document_digest_changes_on_edit() {
        let before = crate::access::six_rule_fixture();
        let after = replace_at(
            &before,
            2,
            RuleEdit {
                methods: Some("read,query".into()),
                ..RuleEdit::default()
            },
        )
        .unwrap()
        .document;

        assert_ne!(digest(&before), digest(&after));
        assert_eq!(short_digest(&before).len(), 8);
    }

    #[test]
    fn exact_change_summaries_are_table_driven() {
        let before = crate::access::six_rule_fixture();
        let cases = [
            (
                "append",
                append(&before, new_rule()).unwrap(),
                6,
                false,
                true,
                6,
            ),
            (
                "remove non-duplicate",
                remove_at(&before, &[2]).unwrap(),
                2,
                true,
                false,
                5,
            ),
            (
                "remove addressed duplicate",
                remove_at(&before, &[5]).unwrap(),
                5,
                true,
                false,
                5,
            ),
        ];

        for (name, transformed, index, has_before, has_after, unchanged) in cases {
            let summary = changes(&before, &transformed.document, Some(&transformed.touched));
            assert_eq!(summary.changed.len(), 1, "{name}: {summary:?}");
            assert_eq!(summary.changed[0].index, index, "{name}");
            assert_eq!(summary.changed[0].before.is_some(), has_before, "{name}");
            assert_eq!(summary.changed[0].after.is_some(), has_after, "{name}");
            assert_eq!(summary.unchanged, unchanged, "{name}");
            assert!(!summary.positions_approximate, "{name}");
        }
    }

    #[test]
    fn apply_changes_match_a_multiset_instead_of_pairing_positions() {
        let before = crate::access::six_rule_fixture();
        let mut after = before.clone();
        after["configs"].as_array_mut().unwrap().remove(1);
        after["configs"].as_array_mut().unwrap().push(json!({
            "pattern": "endpoint/new/*",
            "roles": "*",
            "methods": "read"
        }));

        let summary = changes(&before, &after, None);
        assert_eq!(summary.unchanged, 5);
        assert_eq!(summary.changed.len(), 2);
        assert!(
            summary
                .changed
                .iter()
                .any(|change| change.before == Some(before["configs"][1].clone())
                    && change.after.is_none())
        );
        assert!(
            summary.changed.iter().any(|change| change.before.is_none()
                && change.after == Some(after["configs"][5].clone()))
        );
        assert!(summary.positions_approximate);
    }

    #[test]
    fn out_of_range_index_names_valid_range() {
        let error =
            replace_at(&crate::access::six_rule_fixture(), 6, RuleEdit::default()).unwrap_err();
        assert!(
            matches!(error, Error::Config(ref message) if message.contains("0..=5")),
            "{error}"
        );
    }
}
