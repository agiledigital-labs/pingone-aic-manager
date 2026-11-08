//! Tenant-free relationship storage planning for `relationship map` and new
//! `relationship set` pairs. `repair` alone applies these document changes.
//! `conversion` reuses pair judgement and section-aware entries for capture;
//! mapping creation still allocates under genericMapping.
//! Ground truth: docs/api/10-managed-objects.md, conversion measured 2026-11-01.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
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
    if !verify::properties(managed, object)
        .and_then(|props| props.get(key))
        .is_some_and(super::state::is_relationship_property)
    {
        return Err(format!(
            "{object}.{key} is not a declared relationship; declare it with relationship set first"
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
    if let Some(reverse) = &pair.reverse
        && (reverse.reverse_key.as_deref() != Some(key)
            || reverse.targets != [format!("managed/{object}")])
    {
        return Err(format!(
            "{}.{} must declare a reciprocal relationship only to {object}.{key}; fix the schema before mapping",
            reverse.object, reverse.key
        ));
    }
    if mode(&pair) == Mode::Mixed {
        return Err(format!(
            "mixed pair {object}.{key}: {}; inspect declarations and mappings before retrying",
            pair.problems
                .iter()
                .map(verify::Problem::message)
                .collect::<Vec<_>>()
                .join("; ")
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub section: String,
    pub object: String,
    pub key: String,
    pub value: Value,
}

pub fn generic_section() -> String {
    "genericMapping".into()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Addition {
    pub pair: PairReport,
    pub column: String,
    pub entries: Vec<Entry>,
    pub added_types: BTreeMap<String, Value>,
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
        section: generic_section(),
        object: object.to_string(),
        key: key.to_string(),
        value: json!({"type": "reference", "ldapAttribute": column,
            "primaryKey": "uid", "resourcePath": pair.forward.targets[0],
            "isMultiValued": pair.forward.array}),
    }];
    if let Some(reverse) = &pair.reverse {
        entries.push(Entry {
            section: generic_section(),
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
    let mut added_types = BTreeMap::new();
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
            added_types.insert(entry.object.clone(), generic[&path].clone());
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
        "pair has {count} link(s); mapping would hide them. Pause link writers and use relationship map to capture and restore it safely"
    ))
}

/// A newly assigned column must expose no data. Leave it installed on a
/// finding: preserve evidence and avoid an unguarded compensating PUT.
pub fn no_orphans(object: &str, key: &str, column: &str, records: &[String]) -> Result<(), String> {
    if records.is_empty() {
        return Ok(());
    }
    Err(format!(
        "ORPHANED DATA: mapping {object}.{key} exposes existing values in {column} \
         (record sample: {records:?}). Mapping remains installed; nothing was cleaned. \
         Keep link writers paused and inspect the column and repo.ds backup before using it"
    ))
}

/// The warning owed after a write that gave an object its first per-type
/// `genericMapping`. A record created through that object before IDM activates
/// the new mapping lands in the old generic store and becomes unreadable once
/// it does; reverting the mapping exposes it again. Seen on a client tenant
/// (2026-11-03); how long activation takes is unmeasured, so this warns rather
/// than waits.
pub fn activation_warning(addition: &Addition) -> Option<String> {
    if addition.added_types.is_empty() {
        return None;
    }
    let objects: Vec<String> = addition
        .added_types
        .keys()
        .map(|object| format!("managed/{object}"))
        .collect();
    Some(format!(
        "warning: {} now {} its own config/repo.ds mapping. Until IDM activates it, a record \
         created there can land in the old generic store and become unreadable once it does. \
         How long that takes is unmeasured: wait before creating records, then read one back",
        objects.join(" and "),
        if objects.len() == 1 { "has" } else { "have" }
    ))
}

/// The CLI's real write sequence. Editing never changes storage mode.
/// Managed confirmation precedes even planning/writing the mapping.
pub async fn set_with<T>(
    creating: bool,
    managed: impl AsyncFnOnce() -> crate::Result<()>,
    mapping: impl AsyncFnOnce() -> crate::Result<T>,
) -> crate::Result<Option<T>> {
    managed().await?;
    if creating {
        return mapping().await.map(Some);
    }
    Ok(None)
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
    fn activation_warning_only_for_a_first_per_type_mapping() {
        let generic = plan(&managed(), &repo(), "a", "link").unwrap();
        assert_eq!(generic.added_types.len(), 2);
        let warning = activation_warning(&generic).unwrap();
        assert!(
            warning.contains("managed/a and managed/b now have"),
            "{warning}"
        );

        let mut existing = repo();
        existing["resourceMapping"]["genericMapping"]["managed/a"] = json!({"properties": {}});
        existing["resourceMapping"]["genericMapping"]["managed/b"] = json!({"properties": {}});
        let typed = plan(&managed(), &existing, "a", "link").unwrap();
        assert_eq!(activation_warning(&typed), None);
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
        assert_eq!(
            result
                .added_types
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["b"]
        );
        assert_eq!(
            result.entries[0].value,
            json!({"type": "reference", "ldapAttribute": "fr-idm-reference-1", "primaryKey": "uid", "resourcePath": "managed/b", "isMultiValued": true})
        );
        assert_eq!(
            result.entries[1].value,
            json!({"type": "reverseReference", "resourcePath": "managed/a", "propertyName": "link", "isMultiValued": false})
        );
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
        assert_eq!(
            result
                .added_types
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["a"]
        );
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
        assert_eq!(
            result
                .added_types
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["a"]
        );
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
    #[test]
    fn malformed_or_nonreciprocal_and_self_same_key_pairs_are_refused() {
        let mut doc = managed();
        doc["objects"][1]["schema"]["properties"]["back"]["reverseRelationship"] = json!(false);
        doc["objects"][1]["schema"]["properties"]["back"]
            .as_object_mut()
            .unwrap()
            .remove("reversePropertyName");
        assert!(
            plan(&doc, &repo(), "a", "link")
                .unwrap_err()
                .contains("reciprocal")
        );
        doc["objects"][0]["schema"]["properties"]["link"]["items"]["resourceCollection"][0]["path"] =
            json!("managed/a");
        doc["objects"][0]["schema"]["properties"]["link"]["items"]["reversePropertyName"] =
            json!("link");
        assert!(
            plan(&doc, &repo(), "a", "link")
                .unwrap_err()
                .contains("own reverse")
        );
        assert!(
            plan(&doc, &repo(), "a", "missing")
                .unwrap_err()
                .contains("not a declared relationship")
        );
    }

    #[test]
    fn an_existing_type_entry_is_unchanged_except_for_the_new_property() {
        let doc = managed();
        let before = occupied(&[1, 2, 3]);
        let mut result = plan(&doc, &before, "a", "link").unwrap().after;
        result["resourceMapping"]["genericMapping"]["managed/a"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("link");
        assert_eq!(
            result["resourceMapping"]["genericMapping"]["managed/a"],
            before["resourceMapping"]["genericMapping"]["managed/a"]
        );
    }
    #[test]
    fn orphaned_values_fail_loudly_with_the_column_and_record() {
        assert!(no_orphans("a", "link", "fr-idm-reference-4", &[]).is_ok());
        let error =
            no_orphans("a", "link", "fr-idm-reference-4", &["record-1".into()]).unwrap_err();
        for detail in [
            "ORPHANED DATA",
            "a.link",
            "fr-idm-reference-4",
            "record-1",
            "remains installed",
            "nothing was cleaned",
        ] {
            assert!(error.contains(detail), "{error}");
        }
    }
}
