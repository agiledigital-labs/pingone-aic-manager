//! Tenant-free relationship storage planning for `relationship map` and new
//! `relationship set` pairs. `repair` alone applies these document changes.
//! Slice 2 can reuse the pair judgement, allocator and atomic pair transform.
//! Ground truth: docs/api/10-managed-objects.md, conversion measured 2026-11-01.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use super::verify::{self, PairReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Generic,
    Mapped,
    Mixed,
}

/// Storage judgement is separate from schema health; `verify` supplies the
/// latter. A lone forward reference is fully mapped without a reverse.
pub fn mode(pair: &PairReport) -> Mode {
    match (
        &pair.forward.mapping,
        pair.reverse.as_ref().map(|end| &end.mapping),
    ) {
        (None, None | Some(None)) => Mode::Generic,
        (Some(_), None | Some(Some(_))) => Mode::Mapped,
        _ => Mode::Mixed,
    }
}

/// One declared, reciprocal pair with exactly one declared managed target.
/// Multi-target mappings and external targets have no measured write shape.
pub fn pair(managed: &Value, repo: &Value, object: &str, key: &str) -> Result<PairReport, String> {
    if verify::properties(managed, object).is_none() {
        return Err(format!(
            "object '{object}' is not declared in config/managed"
        ));
    }
    let mut pairs = verify::pairs(managed, repo, object, key);
    if pairs.len() != 1 {
        return Err(format!(
            "{object}.{key} must name exactly one managed target"
        ));
    }
    let pair = pairs.remove(0);
    if pair.forward.targets.len() != 1 || !pair.unchecked.is_empty() {
        return Err(format!(
            "{object}.{key} must name exactly one managed target"
        ));
    }
    let target = pair.forward.targets[0]
        .strip_prefix("managed/")
        .ok_or_else(|| format!("{object}.{key} does not target a managed object"))?;
    if verify::properties(managed, target).is_none() {
        return Err(format!(
            "target object '{target}' is not declared in config/managed"
        ));
    }
    if pair
        .reverse
        .as_ref()
        .is_some_and(|end| end.object == object && end.key == key)
    {
        return Err(format!(
            "{object}.{key} cannot be its own reverse: use two distinct keys"
        ));
    }
    if !pair.problems.is_empty() {
        return Err(pair
            .problems
            .iter()
            .map(verify::Problem::message)
            .collect::<Vec<_>>()
            .join("; "));
    }
    Ok(pair)
}

/// Columns belong to the forward object. Count every mention in every
/// property's mapping, including explicitMapping and stale/scalar entries.
pub fn allocate(repo: &Value, object: &str) -> Result<u8, String> {
    fn mentions(value: &Value, column: &str) -> bool {
        match value {
            Value::String(value) => value == column,
            Value::Array(values) => values.iter().any(|value| mentions(value, column)),
            Value::Object(values) => values.values().any(|value| mentions(value, column)),
            _ => false,
        }
    }
    let mut used: BTreeMap<u8, BTreeSet<String>> = BTreeMap::new();
    if let Some(sections) = repo.get("resourceMapping").and_then(Value::as_object) {
        for (section, entries) in sections {
            if let Some(properties) = entries
                .get(format!("managed/{object}"))
                .and_then(|entry| entry.get("properties"))
                .and_then(Value::as_object)
            {
                for (key, entry) in properties {
                    for number in 1..=10 {
                        if mentions(entry, &format!("fr-idm-reference-{number}")) {
                            used.entry(number)
                                .or_default()
                                .insert(format!("{object}.{key} ({section})"));
                        }
                    }
                }
            }
        }
    }
    let next = used.keys().next_back().copied().unwrap_or(0) + 1;
    if next <= 10 {
        return Ok(next);
    }
    if let Some(free) = (1..=10).find(|number| !used.contains_key(number)) {
        return Ok(free);
    }
    Err(format!(
        "no free reference column on {object}; all ten are held: {}. Remove unused relationships and their mappings before retrying",
        used.iter()
            .map(|(number, holders)| format!(
                "fr-idm-reference-{number}: {}",
                holders.iter().cloned().collect::<Vec<_>>().join(", ")
            ))
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub object: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Addition {
    pub pair: PairReport,
    pub column: String,
    pub entries: Vec<Entry>,
    pub added_types: Vec<String>,
    #[serde(skip)]
    pub after: Value,
}

/// All additions are made on a clone. On failure no partial document escapes,
/// including when preparing the reverse object's entry fails.
pub fn plan(managed: &Value, repo: &Value, object: &str, key: &str) -> Result<Addition, String> {
    let pair = pair(managed, repo, object, key)?;
    match mode(&pair) {
        Mode::Mapped => {
            return Err(format!(
                "{object}.{key} is already mapped; use relationship verify to check it"
            ));
        }
        Mode::Mixed => {
            return Err(format!(
                "{object}.{key} is mixed; repair the conflicting declarations/mappings before mapping"
            ));
        }
        Mode::Generic => {}
    }
    let column = format!("fr-idm-reference-{}", allocate(repo, object)?);
    let mut entries = vec![Entry {
        object: object.to_string(),
        key: key.to_string(),
        value: json!({"type": "reference", "ldapAttribute": column,
            "primaryKey": "uid", "resourcePath": pair.forward.targets[0],
            "isMultiValued": pair.forward.array}),
    }];
    if let Some(reverse) = &pair.reverse {
        entries.push(Entry {
            object: reverse.object.clone(),
            key: reverse.key.clone(),
            value: json!({"type": "reverseReference", "resourcePath": format!("managed/{object}"),
                "propertyName": key, "isMultiValued": reverse.array}),
        });
    }
    let mut after = repo.clone();
    let generic = after
        .pointer_mut("/resourceMapping/genericMapping")
        .and_then(Value::as_object_mut)
        .ok_or(
            "config/repo.ds has no genericMapping object; inspect the document before retrying",
        )?;
    let mut added_types = Vec::new();
    for entry in &entries {
        if !verify::sections_mapping(repo, &entry.object, &entry.key).is_empty() {
            return Err(format!(
                "{}.{} already has a repo.ds entry; inspect it before retrying",
                entry.object, entry.key
            ));
        }
        let path = format!("managed/{}", entry.object);
        if !generic.contains_key(&path) {
            generic.insert(path.clone(), json!({
                "dnTemplate": format!("ou={},ou=managed,dc=openidm,dc=example,dc=com", entry.object),
                "objectClasses": ["uidObject", "fr-idm-generic-obj"],
                "jsonAttribute": "fr-idm-json", "jsonQueryEqualityMatchingRule": "caseIgnoreJsonQueryMatch",
                "properties": {}
            }));
            added_types.push(entry.object.clone());
        }
        let properties = generic
            .get_mut(&path)
            .and_then(|entry| entry.get_mut("properties"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                format!(
                    "{path}'s genericMapping has no properties object; inspect it before retrying"
                )
            })?;
        properties.insert(entry.key.clone(), entry.value.clone());
    }
    Ok(Addition {
        pair,
        column,
        entries,
        added_types,
        after,
    })
}

/// Exact union of edge IDs across both ends, so a one-sided link also blocks
/// conversion. Missing edge IDs refuse rather than inventing an exact count.
pub fn link_count(ends: &[(&str, &[Value])]) -> Result<usize, String> {
    let mut ids = BTreeSet::new();
    for (key, records) in ends {
        for record in *records {
            for link in verify::links_of(record, key)? {
                let id = link.edge_id.ok_or_else(|| {
                    format!(
                        "{}.{} has a link without an edge id; cannot count safely",
                        link.from_id, key
                    )
                })?;
                ids.insert(id);
            }
        }
    }
    Ok(ids.len())
}

pub fn require_empty(count: usize) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    Err(format!(
        "pair has {count} link(s); mapping would hide them. Converting a linked pair requires capture/restore (slice 2, not yet supported). Keep this pair generic"
    ))
}

/// The CLI's real write sequence. Editing never changes storage mode.
/// Managed confirmation precedes even planning/writing the mapping.
pub async fn set_with(
    creating: bool,
    managed: impl AsyncFnOnce() -> crate::Result<()>,
    mapping: impl AsyncFnOnce() -> crate::Result<()>,
) -> crate::Result<()> {
    managed().await?;
    if creating {
        mapping().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn managed() -> Value {
        json!({"objects": [
            {"name": "a", "schema": {"properties": {"link": {
                "type": "array", "items": {"type": "relationship",
                "resourceCollection": [{"path": "managed/b"}],
                "reverseRelationship": true, "reversePropertyName": "back"}}}}},
            {"name": "b", "schema": {"properties": {"back": {
                "type": "relationship", "resourceCollection": [{"path": "managed/a"}],
                "reverseRelationship": true, "reversePropertyName": "link"}}}}
        ]})
    }

    fn repo() -> Value {
        json!({"resourceMapping": {"genericMapping": {"managed/*": {"properties": {}}}}})
    }

    fn occupied(numbers: &[u8]) -> Value {
        let mut repo = repo();
        repo["resourceMapping"]["genericMapping"]["managed/a"] = json!({"properties": {}});
        for number in numbers {
            repo["resourceMapping"]["genericMapping"]["managed/a"]["properties"]
                [format!("p{number}")] =
                json!({"ldapAttribute": format!("fr-idm-reference-{number}")});
        }
        repo
    }

    #[test]
    fn allocation_prefers_next_then_lowest_free_and_names_all_holders() {
        assert_eq!(
            allocate(&occupied(&(1..=7).collect::<Vec<_>>()), "a"),
            Ok(8)
        );
        assert_eq!(allocate(&occupied(&[1, 2, 3, 10]), "a"), Ok(4));
        let full = occupied(&(1..=10).collect::<Vec<_>>());
        let error = allocate(&full, "a").unwrap_err();
        for number in 1..=10 {
            assert!(error.contains(&format!("a.p{number} (")));
        }
        assert_eq!(allocate(&full, "b"), Ok(1));
    }

    #[test]
    fn allocation_checks_explicit_and_nested_non_relationship_mappings() {
        let mut repo = occupied(&[1, 2]);
        repo["resourceMapping"]["explicitMapping"] = json!({"managed/a": {"properties": {
            "scalar": {"nested": ["fr-idm-reference-7"]}
        }}});
        assert_eq!(allocate(&repo, "a"), Ok(8));
    }

    #[test]
    fn per_type_entries_and_both_mappings_share_one_document() {
        let doc = managed();
        let mut repo = repo();
        let existing = json!({"custom": true, "properties": {"name": {"type": "string"}}});
        repo["resourceMapping"]["genericMapping"]["managed/a"] = existing.clone();
        let result = plan(&doc, &repo, "a", "link").unwrap();
        assert_eq!(result.added_types, ["b"]);
        let generic = &result.after["resourceMapping"]["genericMapping"];
        assert_eq!(generic["managed/a"]["custom"], true);
        assert_eq!(
            generic["managed/a"]["properties"]["name"],
            existing["properties"]["name"]
        );
        assert_eq!(
            generic["managed/b"]["dnTemplate"],
            "ou=b,ou=managed,dc=openidm,dc=example,dc=com"
        );
        assert_eq!(
            generic["managed/b"]["objectClasses"],
            json!(["uidObject", "fr-idm-generic-obj"])
        );
        assert_eq!(generic["managed/b"]["jsonAttribute"], "fr-idm-json");
        assert_eq!(
            generic["managed/b"]["jsonQueryEqualityMatchingRule"],
            "caseIgnoreJsonQueryMatch"
        );
        assert_eq!(
            generic["managed/a"]["properties"]["link"]["isMultiValued"],
            true
        );
        assert_eq!(
            generic["managed/b"]["properties"]["back"]["isMultiValued"],
            false
        );
        assert_eq!(
            mode(&pair(&doc, &result.after, "a", "link").unwrap()),
            Mode::Mapped
        );
        assert_eq!(
            repo["resourceMapping"]["genericMapping"]["managed/a"],
            existing
        );
    }

    #[test]
    fn scalar_forward_array_reverse_and_one_way_follow_schema() {
        let mut doc = managed();
        let array = doc["objects"][0]["schema"]["properties"]["link"].take();
        doc["objects"][0]["schema"]["properties"]["link"] = array["items"].clone();
        let scalar = doc["objects"][1]["schema"]["properties"]["back"].take();
        doc["objects"][1]["schema"]["properties"]["back"] =
            json!({"type": "array", "items": scalar});
        let result = plan(&doc, &repo(), "a", "link").unwrap();
        assert_eq!(result.entries[0].value["isMultiValued"], false);
        assert_eq!(result.entries[1].value["isMultiValued"], true);
        doc["objects"][0]["schema"]["properties"]["link"]["reverseRelationship"] = json!(false);
        doc["objects"][0]["schema"]["properties"]["link"]
            .as_object_mut()
            .unwrap()
            .remove("reversePropertyName");
        let result = plan(&doc, &repo(), "a", "link").unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].value["type"], "reference");
        assert_eq!(result.added_types, ["a"]);
    }

    #[test]
    fn reverse_failure_cannot_return_a_half_mapped_document() {
        let mut repo = repo();
        repo["resourceMapping"]["genericMapping"]["managed/b"] = json!({"properties": null});
        let before = repo.clone();
        assert!(
            plan(&managed(), &repo, "a", "link")
                .unwrap_err()
                .contains("properties object")
        );
        assert_eq!(repo, before);
        assert_eq!(
            mode(&pair(&managed(), &repo, "a", "link").unwrap()),
            Mode::Generic
        );
    }

    #[test]
    fn mapped_mixed_and_undeclared_are_refused() {
        let doc = managed();
        let mapped = plan(&doc, &repo(), "a", "link").unwrap().after;
        assert!(
            plan(&doc, &mapped, "a", "link")
                .unwrap_err()
                .contains("already mapped")
        );
        let mut mixed = mapped;
        mixed["resourceMapping"]["genericMapping"]["managed/b"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("back");
        assert_eq!(
            mode(&verify::pairs(&doc, &mixed, "a", "link")[0]),
            Mode::Mixed
        );
        assert!(plan(&doc, &mixed, "a", "link").is_err());
        assert!(
            plan(&doc, &repo(), "missing", "link")
                .unwrap_err()
                .contains("not declared")
        );
        let mut absent = doc.clone();
        absent["objects"].as_array_mut().unwrap().pop();
        assert!(
            plan(&absent, &repo(), "a", "link")
                .unwrap_err()
                .contains("not declared")
        );
    }

    #[test]
    fn same_object_pair_uses_one_column_and_two_distinct_keys() {
        let mut doc = managed();
        let back = doc["objects"][1]["schema"]["properties"]["back"].clone();
        doc["objects"][0]["schema"]["properties"]["back"] = back;
        doc["objects"][0]["schema"]["properties"]["link"]["items"]["resourceCollection"][0]["path"] =
            json!("managed/a");
        let result = plan(&doc, &repo(), "a", "link").unwrap();
        assert_eq!(result.added_types, ["a"]);
        assert_eq!(result.entries.len(), 2);
        assert_eq!(allocate(&result.after, "a"), Ok(2));
    }

    #[test]
    fn link_union_counts_one_sided_edges_once_and_refuses_linked_pairs() {
        let link = |id, edge| {
            json!({"_id": id, "link": [{"_refResourceCollection": "managed/b",
            "_refResourceId": "target", "_refProperties": {"_id": edge}}]})
        };
        let forward = vec![link("one", "edge1")];
        let reverse = vec![link("two", "edge1"), link("three", "edge2")];
        let count = link_count(&[("link", &forward), ("link", &reverse)]).unwrap();
        assert_eq!(count, 2);
        let error = require_empty(count).unwrap_err();
        assert!(error.contains("2 link(s)"));
        assert!(error.contains("hide"));
        assert!(require_empty(0).is_ok());
        assert!(link_count(&[("link", &[json!({"_id": "x", "link": "bad"})])]).is_err());
    }

    #[tokio::test]
    async fn set_creates_in_order_edits_skip_mapping_and_managed_failure_stops() {
        for creating in [true, false] {
            let calls = RefCell::new(Vec::new());
            set_with(
                creating,
                async || {
                    calls.borrow_mut().push("managed");
                    Ok(())
                },
                async || {
                    calls.borrow_mut().push("mapping");
                    Ok(())
                },
            )
            .await
            .unwrap();
            assert_eq!(
                *calls.borrow(),
                if creating {
                    vec!["managed", "mapping"]
                } else {
                    vec!["managed"]
                }
            );
        }
        let called = RefCell::new(false);
        assert!(
            set_with(
                true,
                async || Err(crate::Error::Config("failed".into())),
                async || {
                    *called.borrow_mut() = true;
                    Ok(())
                }
            )
            .await
            .is_err()
        );
        assert!(!*called.borrow());
    }
}
