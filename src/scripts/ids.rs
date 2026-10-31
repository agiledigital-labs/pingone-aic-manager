//! `aic script ids <DIR>`: whether a configuration repository and the tenant
//! agree on which UUID each AM script name has.
//!
//! A repository that pushes AM scripts by id (fr-config-manager's
//! `realms/<realm>/scripts/scripts-config/<uuid>.json`) and a tenant that holds
//! the same name under another id do not conflict in any way a content diff
//! can see: the push writes a second script, or fails on the duplicate name,
//! and nothing says the ids were the problem. This reads only the tenant's
//! list, never a body.
//!
//! Tenant-free: `cli.rs` supplies the listing.

use super::RemoteRef;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A script-shaped object found in a repository file: an object carrying
/// string `_id`, `name` and `context`, which is how AM exports a script and
/// how nothing else in a config tree is shaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub file: PathBuf,
    /// From a `realms/<realm>/` path component; `None` when the path has none.
    pub realm: Option<String>,
    pub id: String,
    pub name: String,
}

/// Every `*.json` under `dir`, sorted. Symlinks are not followed — a read-only
/// walk could, but there is no reason to leave the tree it was pointed at —
/// and `.git` / `node_modules` are skipped.
pub fn json_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            let skip = matches!(
                entry.file_name().to_str(),
                Some(".git") | Some("node_modules")
            );
            if kind.is_dir() && !skip {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The realm a repository path names: the component after `realms`, skipping
/// a `root` that precedes a sub-realm (`realms/root/realms/alpha`).
pub fn realm_of(path: &Path) -> Option<String> {
    let parts: Vec<&str> = path.iter().filter_map(|part| part.to_str()).collect();
    let mut realm = None;
    for pair in parts.windows(2) {
        if pair[0] == "realms" && pair[1] != "root" {
            realm = Some(pair[1].to_string());
        }
    }
    realm
}

/// Every script-shaped object in one parsed file, at any depth.
pub fn declared_in(file: &Path, value: &Value) -> Vec<Declared> {
    let realm = realm_of(file);
    let mut out = Vec::new();
    collect(value, &mut |id, name| {
        out.push(Declared {
            file: file.to_path_buf(),
            realm: realm.clone(),
            id: id.to_string(),
            name: name.to_string(),
        })
    });
    out
}

fn collect(value: &Value, found: &mut impl FnMut(&str, &str)) {
    match value {
        Value::Object(map) => {
            let field = |key: &str| map.get(key).and_then(Value::as_str);
            if let (Some(id), Some(name), Some(_)) = (field("_id"), field("name"), field("context"))
            {
                found(id, name);
                return;
            }
            map.values().for_each(|child| collect(child, found));
        }
        Value::Array(items) => items.iter().for_each(|child| collect(child, found)),
        _ => {}
    }
}

/// One repository script compared with the tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Verdict {
    /// Same name, same id.
    Match,
    /// The tenant has the name under another id — the case this command
    /// exists for.
    IdDiffers { tenant_id: String },
    /// The tenant has the id under another name: a rename on one side.
    NameDiffers { tenant_name: String },
    /// Neither the name nor the id is on the tenant; a push would create it.
    Absent,
    /// The repository id is not a UUID, which AM's own ids always are.
    NotUuid,
}

impl Verdict {
    pub fn is_problem(&self) -> bool {
        !matches!(self, Verdict::Match | Verdict::Absent)
    }
}

/// Judge each declared script against its realm's tenant listing. A script
/// whose realm the path does not name takes `default_realm`.
pub fn judge<'a>(
    declared: &'a [Declared],
    default_realm: &str,
    tenant: &BTreeMap<String, Vec<RemoteRef>>,
) -> Vec<(&'a Declared, String, Verdict)> {
    declared
        .iter()
        .map(|script| {
            let realm = script
                .realm
                .clone()
                .unwrap_or_else(|| default_realm.to_string());
            let listed = tenant.get(&realm).map(Vec::as_slice).unwrap_or_default();
            let verdict = verdict(script, listed);
            (script, realm, verdict)
        })
        .collect()
}

fn verdict(script: &Declared, listed: &[RemoteRef]) -> Verdict {
    if uuid::Uuid::parse_str(&script.id).is_err() {
        return Verdict::NotUuid;
    }
    if let Some(by_name) = listed.iter().find(|r| r.name == script.name) {
        return if by_name.id == script.id {
            Verdict::Match
        } else {
            Verdict::IdDiffers {
                tenant_id: by_name.id.clone(),
            }
        };
    }
    match listed.iter().find(|r| r.id == script.id) {
        Some(by_id) => Verdict::NameDiffers {
            tenant_name: by_id.name.clone(),
        },
        None => Verdict::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::Kind;
    use serde_json::json;

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";

    fn listed(id: &str, name: &str) -> RemoteRef {
        RemoteRef {
            kind: Kind::Am,
            id: id.into(),
            name: name.into(),
            context: Some("LIBRARY".into()),
            is_default: false,
            evaluator_version: None,
        }
    }

    #[test]
    fn realm_comes_from_the_path() {
        assert_eq!(
            realm_of(Path::new("cfg/realms/alpha/scripts/scripts-config/x.json")),
            Some("alpha".into())
        );
        assert_eq!(
            realm_of(Path::new("am/realms/root/realms/bravo/x.json")),
            Some("bravo".into())
        );
        assert_eq!(realm_of(Path::new("scripts/x.json")), None);
    }

    #[test]
    fn only_script_shaped_objects_are_found() {
        let file = Path::new("realms/alpha/journeys/j.json");
        let value = json!({
            "nodes": {"n": {"_id": "n", "name": "not a script"}},
            "scripts": [{"_id": A, "name": "Lib", "context": "LIBRARY",
                         "inner": {"_id": B, "name": "x", "context": "y"}}],
        });
        let found = declared_in(file, &value);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].id, A);
        assert_eq!(found[0].realm.as_deref(), Some("alpha"));
    }

    /// The discriminating case is the same name under another id, which a
    /// name-only or an id-only lookup each judge as fine.
    #[test]
    fn each_kind_of_disagreement_is_named() {
        let declared = |id: &str, name: &str| Declared {
            file: PathBuf::from("x.json"),
            realm: None,
            id: id.into(),
            name: name.into(),
        };
        let tenant = BTreeMap::from([(
            "alpha".to_string(),
            vec![listed(A, "Lib"), listed(B, "Other")],
        )]);
        let scripts = [
            declared(A, "Lib"),
            declared(B, "Lib"),
            declared(A, "Renamed"),
            declared("33333333-3333-4333-8333-333333333333", "New"),
            declared("lib-script", "Lib"),
        ];
        let verdicts: Vec<Verdict> = judge(&scripts, "alpha", &tenant)
            .into_iter()
            .map(|(_, _, verdict)| verdict)
            .collect();
        assert_eq!(
            verdicts,
            vec![
                Verdict::Match,
                Verdict::IdDiffers {
                    tenant_id: A.into()
                },
                Verdict::NameDiffers {
                    tenant_name: "Lib".into()
                },
                Verdict::Absent,
                Verdict::NotUuid,
            ]
        );
        assert_eq!(
            verdicts.iter().filter(|v| v.is_problem()).count(),
            3,
            "match and absent are not problems"
        );
    }
}
