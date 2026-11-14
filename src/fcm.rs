//! One script or one managed object, written the way `fr-config-pull` writes it.
//!
//! The layout is the one in `@forgerock/fr-config-manager` 1.9.0
//! (`fr-config-pull` `scripts.js` / `managed.js`, `saveJsonToFile` and
//! `safeFileName` in `fr-config-common`). JSON is `JSON.stringify` of a
//! deep-sorted value with a two-space indent and no trailing newline.
//! `serde_json::Value` stores objects in a `BTreeMap`, and `to_vec_pretty`
//! emits that shape for the keys and numbers these documents carry.
//!
//! `safe_file_name` is `sanitize-filename` 1.6.5 — the version `^1.6.3`
//! resolves to — called as
//! `sanitize(name, { replacement: (character) => encodeURIComponent(character) })`.
//! That wrapper runs a second pass with an empty replacement, which is what
//! removes a `*` the encoder leaves alone and a trailing dot the encoder
//! leaves as a dot.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::{Error, Result};

const SCRIPT_HOOKS: [&str; 3] = ["onStore", "onRetrieve", "onValidate"];

/// One file the export would place under the configuration directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub relative: PathBuf,
    pub bytes: Vec<u8>,
}

/// Where a forced overwrite keeps the previous bytes.
#[derive(Debug, Clone)]
pub struct Install<'a> {
    pub config_dir: &'a Path,
    pub files: &'a [PlannedFile],
    pub dry_run: bool,
    pub force: bool,
    /// Backup filename stem: `script` or `managed`.
    pub backup_kind: &'a str,
    /// Backup filename stem: the realm, or `managed` for an object.
    pub backup_realm: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub files: Vec<FileOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileOutcome {
    pub path: String,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Wrote,
    Unchanged,
    BackedUp,
    Skipped,
    WouldWrite,
    WouldLeave,
    WouldBackUp,
    WouldRefuse,
    Refused,
}

impl Action {
    fn line(self, path: &str, backup: Option<&str>) -> String {
        match self {
            Self::Wrote => format!("wrote {path}"),
            Self::Unchanged => format!("unchanged {path}"),
            Self::BackedUp => format!(
                "wrote {path}; previous file backed up to {}",
                backup.unwrap_or("(missing backup path)")
            ),
            Self::Skipped => format!("not written {path}"),
            Self::WouldWrite => format!("would write {path}"),
            Self::WouldLeave => format!("would leave {path} unchanged"),
            Self::WouldBackUp => format!("would back up and write {path}"),
            Self::WouldRefuse => format!("would refuse {path} (differs; re-run with --force)"),
            Self::Refused => format!("refused {path} (differs; re-run with --force)"),
        }
    }
}

pub fn lines(report: &Report) -> Vec<String> {
    report
        .files
        .iter()
        .map(|file| file.action.line(&file.path, file.backup.as_deref()))
        .collect()
}

/// Print the report and fail when a file was refused or a write stopped early.
pub fn present(report: &Report, json: bool) -> Result<()> {
    if json {
        crate::cli::print_json(report)?;
    } else {
        for line in lines(report) {
            println!("{line}");
        }
    }
    match &report.error {
        Some(error) => Err(Error::Config(error.clone())),
        None => Ok(()),
    }
}

/// AM script files for one realm, from the raw script resource `am::fetch` returns.
pub fn script_files(realm: &str, raw: &Value) -> Result<Vec<PlannedFile>> {
    require_component(realm, "realm")?;
    let object = raw
        .as_object()
        .ok_or_else(|| Error::Config("AM script resource is not an object".into()))?;
    let language = object.get("language").and_then(Value::as_str).unwrap_or("");
    if language != "JAVASCRIPT" {
        return Err(Error::Config(format!(
            "fr-config-pull scripts writes only language JAVASCRIPT; this script is {language:?}"
        )));
    }
    let name = field(object, "name")?;
    let context = field(object, "context")?;
    let id = field(object, "_id")?;
    require_component(context, "script context")?;
    require_component(id, "script id")?;
    if !raw.get("script").is_some_and(Value::is_string) {
        return Err(Error::Config(
            "AM script `script` is not a base64 string, which is the form fr-config-pull writes"
                .into(),
        ));
    }
    let source = crate::scripts::am::decode_source(raw)?;
    let js_name = format!("{}.js", safe_file_name(name));
    require_component(&js_name, "script filename")?;
    let file_ref = format!("scripts-content/{context}/{js_name}");
    let mut config = raw.clone();
    config
        .as_object_mut()
        .expect("checked object above")
        .insert("script".into(), serde_json::json!({ "file": file_ref }));
    let root = PathBuf::from("realms").join(realm).join("scripts");
    Ok(vec![
        PlannedFile {
            relative: root.join("scripts-content").join(context).join(js_name),
            bytes: source,
        },
        PlannedFile {
            relative: root.join("scripts-config").join(format!("{id}.json")),
            bytes: json_bytes(&config)?,
        },
    ])
}

/// One managed object, split the way `fr-config-pull managed-objects` splits it
/// when `--custom-relationships` is not passed. That flag is a second read
/// (`config/repo.ds` and per-property schema); the default pull does not do it,
/// and neither does this.
pub fn managed_object_files(object: &Value) -> Result<Vec<PlannedFile>> {
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("managed object has no name".into()))?;
    require_component(name, "managed object name")?;
    if object.pointer("/schema/properties").is_none() {
        return Err(Error::Config(format!(
            "managed object {name:?} has no schema.properties"
        )));
    }
    let mut object = object.clone();
    let dir = PathBuf::from("managed-objects").join(name);
    let mut files = Vec::new();

    let keys: Vec<String> = object
        .as_object()
        .expect("managed object is an object")
        .keys()
        .cloned()
        .collect();
    for key in keys {
        if let Some(source) = take_inline_script(object.get_mut(&key).expect("key just listed")) {
            let filename = hook_filename(name, &format!("{name}.{key}.js"))?;
            object[&key]["file"] = Value::String(filename.clone());
            files.push(PlannedFile {
                relative: dir.join(filename),
                bytes: source,
            });
        }
    }

    if object.get("actions").is_some() {
        if !object["actions"].is_object() {
            return Err(Error::Config(format!(
                "managed object {name:?} actions is not an object"
            )));
        }
        let action_keys: Vec<String> = object["actions"]
            .as_object()
            .expect("checked")
            .keys()
            .cloned()
            .collect();
        for key in action_keys {
            if let Some(source) = take_inline_script(&mut object["actions"][&key]) {
                let filename = hook_filename(name, &format!("{name}.actions.{key}.js"))?;
                object["actions"][&key]["file"] = Value::String(filename.clone());
                files.push(PlannedFile {
                    relative: dir.join(filename),
                    bytes: source,
                });
            }
        }
    }

    let prop_keys: Vec<String> = object["schema"]["properties"]
        .as_object()
        .ok_or_else(|| {
            Error::Config(format!(
                "managed object {name:?} schema.properties is not an object"
            ))
        })?
        .keys()
        .cloned()
        .collect();
    for prop in prop_keys {
        for hook in SCRIPT_HOOKS {
            let Some(slot) = object["schema"]["properties"][&prop].get_mut(hook) else {
                continue;
            };
            if let Some(source) = take_inline_script(slot) {
                let filename = hook_filename(name, &format!("{name}.{prop}.{hook}.js"))?;
                object["schema"]["properties"][&prop][hook]["file"] =
                    Value::String(filename.clone());
                files.push(PlannedFile {
                    relative: dir.join(filename),
                    bytes: source,
                });
            }
        }
    }

    files.push(PlannedFile {
        relative: dir.join(format!("{name}.json")),
        bytes: json_bytes(&object)?,
    });
    Ok(files)
}

/// The realm an `aic script export` ref names. IDM kinds are not this layout.
pub fn am_export_realm(ns: &crate::scripts::Namespace) -> Result<&str> {
    match (&ns.kind, ns.realm.as_deref()) {
        (crate::scripts::Kind::Am, Some(realm)) => Ok(realm),
        _ => Err(Error::Config(
            "script export writes the fr-config-pull scripts layout, which is one AM realm script; \
             IDM endpoints, schedules, hooks and sync scripts are not in that tree"
                .into(),
        )),
    }
}

pub fn install(req: &Install<'_>) -> Result<Report> {
    let mut planned = Vec::new();
    for file in req.files {
        let path = contained(req.config_dir, &file.relative)?;
        let existing = read_existing(&path)?;
        planned.push(Planned {
            relative: slash(&file.relative),
            path,
            bytes: file.bytes.clone(),
            existing,
        });
    }

    let preview = req.dry_run || blocks_write(&planned, req.force);
    if preview {
        return Ok(preview_report(req, &planned));
    }

    let mut files = Vec::new();
    let mut stopped = None;
    for (index, item) in planned.iter().enumerate() {
        match write_one(req, item) {
            Ok(outcome) => files.push(outcome),
            Err(error) => {
                stopped = Some(error);
                files.push(FileOutcome {
                    path: item.relative.clone(),
                    action: Action::Refused,
                    backup: None,
                });
                for rest in planned.iter().skip(index + 1) {
                    files.push(FileOutcome {
                        path: rest.relative.clone(),
                        action: Action::Skipped,
                        backup: None,
                    });
                }
                break;
            }
        }
    }
    if let Some(error) = stopped.take() {
        stopped = Some(note_landed(&files, error));
    }
    Ok(Report {
        dry_run: false,
        files,
        error: stopped,
    })
}

struct Planned {
    relative: String,
    path: PathBuf,
    bytes: Vec<u8>,
    existing: Existing,
}

enum Existing {
    Absent,
    File(Vec<u8>),
    Symlink,
}

fn blocks_write(planned: &[Planned], force: bool) -> bool {
    planned.iter().any(|item| match &item.existing {
        Existing::Symlink => true,
        Existing::File(have) => !force && have != &item.bytes,
        Existing::Absent => false,
    })
}

fn preview_report(req: &Install<'_>, planned: &[Planned]) -> Report {
    let mut refused = Vec::new();
    let files = planned
        .iter()
        .map(|item| {
            let (action, why_symlink) = match &item.existing {
                Existing::Absent => (Action::WouldWrite, false),
                Existing::File(have) if have == &item.bytes => (Action::WouldLeave, false),
                Existing::File(_) if req.force => (Action::WouldBackUp, false),
                Existing::File(_) => (Action::WouldRefuse, false),
                Existing::Symlink => (Action::WouldRefuse, true),
            };
            if matches!(action, Action::WouldRefuse) {
                refused.push((item.relative.clone(), why_symlink));
            }
            FileOutcome {
                path: item.relative.clone(),
                action,
                backup: None,
            }
        })
        .collect();
    let error = if refused.is_empty() {
        None
    } else {
        Some(refusal_text(&refused))
    };
    Report {
        dry_run: req.dry_run,
        files,
        error,
    }
}

fn refusal_text(refused: &[(String, bool)]) -> String {
    let paths = refused
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if refused.iter().any(|(_, symlink)| *symlink) {
        format!("refusing to replace symlink {paths}; nothing was written")
    } else {
        let verb = if refused.len() == 1 {
            "differs"
        } else {
            "differ"
        };
        format!(
            "{paths} {verb} from the export; nothing was written. \
             Re-run with --force to back up and overwrite"
        )
    }
}

fn write_one(req: &Install<'_>, item: &Planned) -> std::result::Result<FileOutcome, String> {
    let existing = read_existing(&item.path).map_err(|error| error.to_string())?;
    match existing {
        Existing::Symlink => Err(format!(
            "refusing to replace symlink {}; nothing further was written",
            item.relative
        )),
        Existing::File(have) if have == item.bytes => Ok(FileOutcome {
            path: item.relative.clone(),
            action: Action::Unchanged,
            backup: None,
        }),
        Existing::File(have) if req.force => {
            let backup =
                backup_file(req, &item.relative, &have).map_err(|error| error.to_string())?;
            write_bytes(&item.path, &item.bytes)
                .map_err(|error| format!("{error}. The previous bytes are in {backup}"))?;
            Ok(FileOutcome {
                path: item.relative.clone(),
                action: Action::BackedUp,
                backup: Some(backup),
            })
        }
        Existing::File(_) => Err(format!(
            "{} differs from the export; nothing further was written. \
             Re-run with --force to back up the existing file and overwrite it",
            item.relative
        )),
        Existing::Absent => {
            write_bytes(&item.path, &item.bytes).map_err(|error| error.to_string())?;
            Ok(FileOutcome {
                path: item.relative.clone(),
                action: Action::Wrote,
                backup: None,
            })
        }
    }
}

fn note_landed(files: &[FileOutcome], error: String) -> String {
    let landed: Vec<_> = files
        .iter()
        .filter(|file| matches!(file.action, Action::Wrote | Action::BackedUp))
        .map(|file| file.path.as_str())
        .collect();
    if landed.is_empty() {
        error
    } else {
        format!("{error}. Already written: {}", landed.join(", "))
    }
}

fn backup_file(req: &Install<'_>, relative: &str, bytes: &[u8]) -> Result<String> {
    let dir = req.config_dir.join(".aic-backups");
    let extension = Path::new(relative)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin");
    let path = crate::backup::create_in(
        &dir,
        req.backup_kind,
        req.backup_realm,
        relative,
        extension,
        bytes,
    )?;
    Ok(slash(
        path.strip_prefix(req.config_dir).unwrap_or(path.as_path()),
    ))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn read_existing(path: &Path) -> Result<Existing> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Existing::Absent),
        Err(error) => Err(Error::Config(format!("read {}: {error}", path.display()))),
        Ok(meta) if meta.file_type().is_symlink() => Ok(Existing::Symlink),
        Ok(meta) if meta.is_dir() => Err(Error::Config(format!(
            "{} is a directory; an export file cannot replace it",
            path.display()
        ))),
        Ok(_) => Ok(Existing::File(std::fs::read(path)?)),
    }
}

fn contained(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::Config(format!(
            "export path {} escapes the config directory",
            relative.display()
        )));
    }
    Ok(root.join(relative))
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn json_bytes(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(value)?)
}

fn field<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("AM script has no {key}")))
}

fn require_component(value: &str, what: &str) -> Result<()> {
    if component_ok(value) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{what} {value:?} is not a single path component"
        )))
    }
}

fn component_ok(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
}

fn hook_filename(object: &str, filename: &str) -> Result<String> {
    require_component(
        filename,
        &format!("managed object {object} script filename"),
    )?;
    Ok(filename.to_string())
}

/// Inline `text/javascript` with a non-empty string `source`, which is the
/// predicate `managed.js` uses (`value.type === "text/javascript" && value.source`).
/// An empty string is falsy there, so it stays in the JSON.
fn take_inline_script(value: &mut Value) -> Option<Vec<u8>> {
    let object = value.as_object_mut()?;
    if object.get("type").and_then(Value::as_str) != Some("text/javascript") {
        return None;
    }
    let source = object.get("source").and_then(Value::as_str)?;
    if source.is_empty() {
        return None;
    }
    let bytes = source.as_bytes().to_vec();
    object.remove("source");
    Some(bytes)
}

/// Filename stem `fr-config-pull` derives for a script name.
pub fn safe_file_name(input: &str) -> String {
    let once = sanitize_pass(input, true);
    sanitize_pass(&once, false)
}

fn sanitize_pass(input: &str, encode: bool) -> String {
    let mut sanitized = replace_chars(input, is_illegal, encode);
    sanitized = replace_chars(&sanitized, is_control, encode);
    sanitized = replace_whole_if(&sanitized, is_reserved_dots, encode);
    sanitized = replace_whole_if(&sanitized, is_windows_reserved, encode);
    sanitized = replace_trailing(&sanitized, encode);
    sanitized = truncate_utf8(&sanitized, 255);
    sanitized = replace_trailing(&sanitized, false);
    replace_whole_if(&sanitized, is_windows_reserved, false)
}

fn replace_chars(input: &str, pred: impl Fn(char) -> bool, encode: bool) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if pred(ch) {
            push_replacement(&mut out, &ch.to_string(), encode);
        } else {
            out.push(ch);
        }
    }
    out
}

fn replace_whole_if(input: &str, pred: impl Fn(&str) -> bool, encode: bool) -> String {
    if pred(input) {
        let mut out = String::new();
        push_replacement(&mut out, input, encode);
        out
    } else {
        input.to_string()
    }
}

fn replace_trailing(input: &str, encode: bool) -> String {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut last = chars.len();
    while last > 0 && matches!(chars[last - 1].1, '.' | ' ') {
        last -= 1;
    }
    if last == chars.len() {
        return input.to_string();
    }
    let split = if last == 0 { 0 } else { chars[last].0 };
    let mut out = input[..split].to_string();
    push_replacement(&mut out, &input[split..], encode);
    out
}

fn push_replacement(out: &mut String, matched: &str, encode: bool) {
    if encode {
        out.push_str(&encode_uri_component(matched));
    }
}

fn is_illegal(ch: char) -> bool {
    matches!(ch, '/' | '?' | '<' | '>' | '\\' | ':' | '*' | '|' | '"')
}

fn is_control(ch: char) -> bool {
    let code = ch as u32;
    code <= 0x1f || (0x80..=0x9f).contains(&code)
}

fn is_reserved_dots(input: &str) -> bool {
    !input.is_empty() && input.chars().all(|ch| ch == '.')
}

fn is_windows_reserved(input: &str) -> bool {
    let lower: String = input.chars().flat_map(|ch| ch.to_lowercase()).collect();
    let (base, _) = lower.split_once('.').unwrap_or((&lower, ""));
    matches!(base, "con" | "prn" | "aux" | "nul")
        || is_numbered_device(base, "com")
        || is_numbered_device(base, "lpt")
}

fn is_numbered_device(base: &str, prefix: &str) -> bool {
    let Some(rest) = base.strip_prefix(prefix) else {
        return false;
    };
    rest.chars().count() == 1
        && rest.chars().next().is_some_and(|ch| {
            ch.is_ascii_digit() || matches!(ch, '\u{00b9}' | '\u{00b2}' | '\u{00b3}')
        })
}

fn encode_uri_component(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in input.bytes() {
        if matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'!'
                | b'~'
                | b'*'
                | b'\''
                | b'('
                | b')'
        ) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

fn truncate_utf8(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let mut bytes = 0;
    let mut end = 0;
    for (index, ch) in input.char_indices() {
        let size = ch.len_utf8();
        if bytes + size > max {
            return input[..end].to_string();
        }
        bytes += size;
        end = index + size;
        if bytes == max {
            return input[..end].to_string();
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use serde_json::json;

    fn script_raw(source: &str) -> Value {
        json!({
            "_id": "00000000-0000-4000-8000-000000000001",
            "context": "LIBRARY",
            "default": false,
            "evaluatorVersion": "2.0",
            "language": "JAVASCRIPT",
            "name": "custom_widget",
            "script": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, source.as_bytes())
        })
    }

    #[test]
    fn script_layout_matches_the_pull_tool_shape() {
        let files = script_files("alpha", &script_raw("var x = 1;\n")).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0].relative,
            PathBuf::from("realms/alpha/scripts/scripts-content/LIBRARY/custom_widget.js")
        );
        assert_eq!(files[0].bytes, b"var x = 1;\n");
        assert_eq!(
            files[1].relative,
            PathBuf::from(
                "realms/alpha/scripts/scripts-config/00000000-0000-4000-8000-000000000001.json"
            )
        );
        assert!(!files[1].bytes.ends_with(b"\n"), "no trailing newline");
        let parsed: Value = serde_json::from_slice(&files[1].bytes).unwrap();
        assert_eq!(
            parsed["script"]["file"],
            "scripts-content/LIBRARY/custom_widget.js"
        );
        assert!(parsed["script"].get("source").is_none());
        assert_eq!(parsed["name"], "custom_widget");
        assert_eq!(parsed["language"], "JAVASCRIPT");
        let expected = "\
{
  \"_id\": \"00000000-0000-4000-8000-000000000001\",
  \"context\": \"LIBRARY\",
  \"default\": false,
  \"evaluatorVersion\": \"2.0\",
  \"language\": \"JAVASCRIPT\",
  \"name\": \"custom_widget\",
  \"script\": {
    \"file\": \"scripts-content/LIBRARY/custom_widget.js\"
  }
}";
        assert_eq!(std::str::from_utf8(&files[1].bytes).unwrap(), expected);
    }

    /// The fields a created AM script carries, with the id and principal
    /// replaced. Captured 2026-11-14 and checked byte-for-byte against
    /// `fr-config-pull scripts`.
    #[test]
    fn script_layout_keeps_null_and_audit_fields() {
        let mut raw = script_raw("// custom_widget\n");
        let principal = "id=00000000-0000-4000-8000-000000000002,ou=user,ou=am-config";
        raw["createdBy"] = json!(principal);
        raw["creationDate"] = json!(1_700_000_000_000_i64);
        raw["description"] = Value::Null;
        raw["lastModifiedBy"] = json!(principal);
        raw["lastModifiedDate"] = json!(1_700_000_000_000_i64);
        let files = script_files("alpha", &raw).unwrap();
        assert_eq!(files[0].bytes, b"// custom_widget\n");
        let expected = "\
{
  \"_id\": \"00000000-0000-4000-8000-000000000001\",
  \"context\": \"LIBRARY\",
  \"createdBy\": \"id=00000000-0000-4000-8000-000000000002,ou=user,ou=am-config\",
  \"creationDate\": 1700000000000,
  \"default\": false,
  \"description\": null,
  \"evaluatorVersion\": \"2.0\",
  \"language\": \"JAVASCRIPT\",
  \"lastModifiedBy\": \"id=00000000-0000-4000-8000-000000000002,ou=user,ou=am-config\",
  \"lastModifiedDate\": 1700000000000,
  \"name\": \"custom_widget\",
  \"script\": {
    \"file\": \"scripts-content/LIBRARY/custom_widget.js\"
  }
}";
        assert_eq!(std::str::from_utf8(&files[1].bytes).unwrap(), expected);
    }

    #[test]
    fn script_source_without_a_trailing_newline_is_not_given_one() {
        let files = script_files("alpha", &script_raw("var x = 1;")).unwrap();
        assert_eq!(files[0].bytes, b"var x = 1;");
    }

    #[test]
    fn a_name_with_a_slash_is_encoded_into_one_filename() {
        let mut raw = script_raw("var x = 1;");
        raw["name"] = json!("a/b");
        let files = script_files("alpha", &raw).unwrap();
        assert_eq!(
            files[0].relative,
            PathBuf::from("realms/alpha/scripts/scripts-content/LIBRARY/a%2Fb.js")
        );
        let parsed: Value = serde_json::from_slice(&files[1].bytes).unwrap();
        assert_eq!(parsed["script"]["file"], "scripts-content/LIBRARY/a%2Fb.js");
    }

    #[test]
    fn non_javascript_and_a_non_string_body_are_refused() {
        let mut raw = script_raw("var x = 1;");
        raw["language"] = json!("GROOVY");
        assert!(script_files("alpha", &raw).is_err());
        raw["language"] = json!("JAVASCRIPT");
        raw["script"] = json!(["var x = 1;"]);
        assert!(script_files("alpha", &raw).is_err());
    }

    #[test]
    fn managed_layout_splits_only_the_hooks_the_pull_tool_splits() {
        let object = json!({
            "actions": {
                "doThing": {"source": "act();\n", "type": "text/javascript"}
            },
            "name": "custom_widget",
            "onCreate": {"source": "", "type": "text/javascript"},
            "onDelete": {"source": "nope", "type": "text/plain"},
            "onUpdate": {"globals": {"keep": true}, "source": "var a = 1;\n", "type": "text/javascript"},
            "schema": {
                "order": ["note"],
                "properties": {
                    "note": {
                        "onUpdate": {"source": "stay;\n", "type": "text/javascript"},
                        "onValidate": {"source": "true;\n", "type": "text/javascript"},
                        "title": "Note",
                        "type": "string"
                    }
                },
                "type": "object"
            }
        });
        let files = managed_object_files(&object).unwrap();
        let paths: Vec<_> = files
            .iter()
            .map(|file| file.relative.display().to_string())
            .collect();
        assert_eq!(
            paths,
            vec![
                "managed-objects/custom_widget/custom_widget.onUpdate.js",
                "managed-objects/custom_widget/custom_widget.actions.doThing.js",
                "managed-objects/custom_widget/custom_widget.note.onValidate.js",
                "managed-objects/custom_widget/custom_widget.json",
            ]
        );
        assert_eq!(files[0].bytes, b"var a = 1;\n");
        assert_eq!(files[1].bytes, b"act();\n");
        assert_eq!(files[2].bytes, b"true;\n");
        assert!(!files[3].bytes.ends_with(b"\n"));
        let expected = "\
{
  \"actions\": {
    \"doThing\": {
      \"file\": \"custom_widget.actions.doThing.js\",
      \"type\": \"text/javascript\"
    }
  },
  \"name\": \"custom_widget\",
  \"onCreate\": {
    \"source\": \"\",
    \"type\": \"text/javascript\"
  },
  \"onDelete\": {
    \"source\": \"nope\",
    \"type\": \"text/plain\"
  },
  \"onUpdate\": {
    \"file\": \"custom_widget.onUpdate.js\",
    \"globals\": {
      \"keep\": true
    },
    \"type\": \"text/javascript\"
  },
  \"schema\": {
    \"order\": [
      \"note\"
    ],
    \"properties\": {
      \"note\": {
        \"onUpdate\": {
          \"source\": \"stay;\\n\",
          \"type\": \"text/javascript\"
        },
        \"onValidate\": {
          \"file\": \"custom_widget.note.onValidate.js\",
          \"type\": \"text/javascript\"
        },
        \"title\": \"Note\",
        \"type\": \"string\"
      }
    },
    \"type\": \"object\"
  }
}";
        assert_eq!(std::str::from_utf8(&files[3].bytes).unwrap(), expected);
    }

    /// A managed object created with one lifecycle hook, renamed. Captured
    /// 2026-11-14. `fr-config-pull managed-objects` stops on an earlier object
    /// that has no `schema.properties`; the package's writer, given this
    /// object alone, matches these bytes.
    #[test]
    fn managed_layout_matches_a_created_object() {
        let object = json!({
            "name": "custom_widget",
            "onCreate": {
                "source": "// onCreate for custom_widget\n",
                "type": "text/javascript"
            },
            "schema": {
                "order": [],
                "properties": {},
                "required": [],
                "title": "custom_widget",
                "type": "object"
            }
        });
        let files = managed_object_files(&object).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0].relative,
            PathBuf::from("managed-objects/custom_widget/custom_widget.onCreate.js")
        );
        assert_eq!(files[0].bytes, b"// onCreate for custom_widget\n");
        let expected = "\
{
  \"name\": \"custom_widget\",
  \"onCreate\": {
    \"file\": \"custom_widget.onCreate.js\",
    \"type\": \"text/javascript\"
  },
  \"schema\": {
    \"order\": [],
    \"properties\": {},
    \"required\": [],
    \"title\": \"custom_widget\",
    \"type\": \"object\"
  }
}";
        assert_eq!(std::str::from_utf8(&files[1].bytes).unwrap(), expected);
    }

    #[test]
    fn safe_file_name_matches_sanitize_filename_as_the_pull_tool_calls_it() {
        let cases = [
            ("custom_widget", "custom_widget"),
            ("a/b", "a%2Fb"),
            ("a*b", "ab"),
            ("foo.", "foo"),
            ("foo ", "foo%20"),
            ("con", ""),
            ("COM1", ""),
            ("*", ""),
            ("lpt².txt", "lpt%C2%B2.txt"),
            ("..", ""),
            ("a b", "a b"),
            ("quote\"", "quote%22"),
            ("a:b", "a%3Ab"),
        ];
        for (input, expected) in cases {
            assert_eq!(safe_file_name(input), expected, "{input:?}");
        }
    }

    #[test]
    fn export_refuses_a_differing_file_unless_forced_and_then_backs_it_up() {
        let dir = std::env::temp_dir().join(format!("aic-fcm-{}", uuid::Uuid::new_v4()));
        let files = vec![
            PlannedFile {
                relative: PathBuf::from("managed-objects/custom_widget/custom_widget.json"),
                bytes: b"{}\n".to_vec(),
            },
            PlannedFile {
                relative: PathBuf::from("managed-objects/custom_widget/custom_widget.onUpdate.js"),
                bytes: b"new\n".to_vec(),
            },
        ];
        let js = dir.join(&files[1].relative);
        std::fs::create_dir_all(js.parent().unwrap()).unwrap();
        std::fs::write(&js, b"old\n").unwrap();

        let blocked = install(&Install {
            config_dir: &dir,
            files: &files,
            dry_run: false,
            force: false,
            backup_kind: "managed",
            backup_realm: "managed",
        })
        .unwrap();
        assert!(blocked.error.is_some(), "{blocked:?}");
        assert!(!dir.join(&files[0].relative).exists());
        assert_eq!(std::fs::read(&js).unwrap(), b"old\n");
        assert!(
            blocked
                .files
                .iter()
                .any(|file| file.action == Action::WouldRefuse || file.action == Action::Refused)
        );

        let dry = install(&Install {
            config_dir: &dir,
            files: &files,
            dry_run: true,
            force: false,
            backup_kind: "managed",
            backup_realm: "managed",
        })
        .unwrap();
        assert!(dry.dry_run && dry.error.is_some());
        assert_eq!(std::fs::read(&js).unwrap(), b"old\n");
        assert!(lines(&dry).iter().any(|line| line.starts_with("would ")));

        let forced = install(&Install {
            config_dir: &dir,
            files: &files,
            dry_run: false,
            force: true,
            backup_kind: "managed",
            backup_realm: "managed",
        })
        .unwrap();
        assert!(forced.error.is_none(), "{forced:?}");
        assert_eq!(
            std::fs::read(dir.join(&files[0].relative)).unwrap(),
            b"{}\n"
        );
        assert_eq!(std::fs::read(&js).unwrap(), b"new\n");
        let backup = forced
            .files
            .iter()
            .find(|file| file.action == Action::BackedUp)
            .unwrap();
        assert_eq!(
            std::fs::read(dir.join(backup.backup.as_deref().unwrap())).unwrap(),
            b"old\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_equal_file_is_left_unchanged_and_a_symlink_is_refused() {
        let dir = std::env::temp_dir().join(format!("aic-fcm-{}", uuid::Uuid::new_v4()));
        let files = vec![PlannedFile {
            relative: PathBuf::from(
                "realms/alpha/scripts/scripts-content/LIBRARY/custom_widget.js",
            ),
            bytes: b"same".to_vec(),
        }];
        let path = dir.join(&files[0].relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"same").unwrap();
        let same = install(&Install {
            config_dir: &dir,
            files: &files,
            dry_run: false,
            force: false,
            backup_kind: "script",
            backup_realm: "alpha",
        })
        .unwrap();
        assert!(same.error.is_none());
        assert_eq!(same.files[0].action, Action::Unchanged);
        assert!(!dir.join(".aic-backups").exists());

        let link = dir.join("link.js");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let linked = vec![PlannedFile {
            relative: PathBuf::from("link.js"),
            bytes: b"other".to_vec(),
        }];
        let refused = install(&Install {
            config_dir: &dir,
            files: &linked,
            dry_run: false,
            force: true,
            backup_kind: "script",
            backup_realm: "alpha",
        })
        .unwrap();
        assert!(refused.error.as_deref().unwrap().contains("symlink"));
        assert_eq!(std::fs::read(&path).unwrap(), b"same");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_commands_parse_with_the_sibling_flags() {
        let script = crate::cli::Cli::try_parse_from([
            "aic",
            "script",
            "export",
            "alpha/custom_widget",
            "--config-dir",
            "cfg",
            "--dry-run",
            "--json",
            "--force",
        ])
        .unwrap();
        assert!(matches!(
            script.command,
            Some(crate::cli::Command::Script {
                command: crate::scripts::cli::ScriptCommand::Export { dry_run, json, force, .. }
            }) if dry_run && json && force.operation()
        ));
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "script",
                "export",
                "alpha/custom_widget",
                "--config-dir",
                "cfg",
                "--force=syntax-check",
            ])
            .is_err()
        );

        let managed = crate::cli::Cli::try_parse_from([
            "aic",
            "managed",
            "export",
            "custom_widget",
            "--config-dir",
            "cfg",
            "--dry-run",
        ])
        .unwrap();
        assert!(matches!(
            managed.command,
            Some(crate::cli::Command::Managed {
                command: crate::managed::cli::ManagedCommand::Export { dry_run, .. }
            }) if dry_run
        ));
    }

    #[test]
    fn only_an_am_ref_is_an_export() {
        let alpha = crate::scripts::Namespace::parse("alpha").unwrap();
        assert_eq!(am_export_realm(&alpha).unwrap(), "alpha");
        let endpoint = crate::scripts::Namespace::parse("endpoint").unwrap();
        assert!(am_export_realm(&endpoint).is_err());
    }
}
