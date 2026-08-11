//! `aic access` parser and guarded `config/access` command implementation.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::access::ops::{self, Changes};
use crate::access::spec::{self, RuleEdit, RuleSpec, RuleView, TouchedIndices, WarningScope};
use crate::access::{api, spec::Findings};
use crate::cli::{
    WriteOk, ensure_prod_confirmed, print_json, print_table, prompting_disabled, tenant_for,
};
use crate::config::ProjectConfig;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum AccessCommand {
    /// List access rules and the whole-document write precondition digest.
    List {
        #[command(flatten)]
        options: AccessListArgs,
    },
    /// Show one rule by index, or every rule matching a displayed digest.
    Show {
        index_or_digest: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print or save the raw config/access document for hand editing.
    Get {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Append a rule. Because rules are OR-ed, this can only grant access.
    Add {
        #[arg(long)]
        pattern: String,
        #[arg(long)]
        roles: String,
        #[arg(long)]
        methods: String,
        #[arg(long)]
        actions: Option<String>,
        #[arg(long)]
        custom_authz: Option<String>,
        #[arg(long)]
        exclude_patterns: Option<String>,
        #[command(flatten)]
        write: AccessWriteArgs,
    },
    /// Change only the supplied fields of one indexed rule.
    Edit {
        index: usize,
        #[arg(long)]
        pattern: Option<String>,
        #[arg(long)]
        roles: Option<String>,
        #[arg(long)]
        methods: Option<String>,
        #[arg(long, conflicts_with = "clear_actions")]
        actions: Option<String>,
        #[arg(long, conflicts_with = "clear_custom_authz")]
        custom_authz: Option<String>,
        #[arg(long, conflicts_with = "clear_exclude_patterns")]
        exclude_patterns: Option<String>,
        #[arg(long, conflicts_with = "actions")]
        clear_actions: bool,
        #[arg(long, conflicts_with = "custom_authz")]
        clear_custom_authz: bool,
        #[arg(long, conflicts_with = "exclude_patterns")]
        clear_exclude_patterns: bool,
        #[command(flatten)]
        write: AccessWriteArgs,
    },
    /// Remove one or more rules by their original indices.
    Rm {
        #[arg(required = true, num_args = 1..)]
        index: Vec<usize>,
        #[command(flatten)]
        write: AccessWriteArgs,
    },
    /// Replace config/access from a hand-edited document or backup.
    Apply {
        file: PathBuf,
        #[command(flatten)]
        write: AccessWriteArgs,
    },
}

#[derive(Args, Debug)]
pub struct AccessListArgs {
    #[arg(long)]
    tenant: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    role: Option<String>,
    #[arg(long)]
    pattern: Option<String>,
    #[arg(long)]
    method: Option<String>,
    #[arg(long)]
    duplicates: bool,
    /// Spell out every document-wide warning instead of counting them.
    ///
    /// The sandbox's own 65 rules produce 28 warnings — 22 `customAuthz`, six
    /// duplicates — so printing them by default buries the table a reader came
    /// for, and trains them to skip the line that matters.
    #[arg(long)]
    warnings: bool,
}

#[derive(Args, Debug)]
pub struct AccessWriteArgs {
    #[arg(long)]
    if_digest: Option<String>,
    #[arg(long)]
    yes: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    no_backup: bool,
    #[arg(long)]
    tenant: Option<String>,
}

enum Amendment {
    Add(RuleSpec),
    Edit { index: usize, edit: RuleEdit },
    Remove(Vec<usize>),
    Apply(PathBuf),
}

pub async fn run(command: AccessCommand) -> Result<()> {
    match command {
        AccessCommand::List { options } => list(options).await,
        AccessCommand::Show {
            index_or_digest,
            tenant,
            json,
        } => show(&index_or_digest, tenant, json).await,
        AccessCommand::Get { tenant, out } => get(tenant, out).await,
        AccessCommand::Add {
            pattern,
            roles,
            methods,
            actions,
            custom_authz,
            exclude_patterns,
            write: options,
        } => {
            write(
                Amendment::Add(RuleSpec {
                    pattern,
                    roles,
                    methods,
                    actions,
                    custom_authz,
                    exclude_patterns,
                }),
                options,
            )
            .await
        }
        AccessCommand::Edit {
            index,
            pattern,
            roles,
            methods,
            actions,
            custom_authz,
            exclude_patterns,
            clear_actions,
            clear_custom_authz,
            clear_exclude_patterns,
            write: options,
        } => {
            write(
                Amendment::Edit {
                    index,
                    edit: RuleEdit {
                        pattern,
                        roles,
                        methods,
                        actions,
                        custom_authz,
                        exclude_patterns,
                        clear_actions,
                        clear_custom_authz,
                        clear_exclude_patterns,
                    },
                },
                options,
            )
            .await
        }
        AccessCommand::Rm {
            index,
            write: options,
        } => write(Amendment::Remove(index), options).await,
        AccessCommand::Apply {
            file,
            write: options,
        } => write(Amendment::Apply(file), options).await,
    }
}

async fn list(options: AccessListArgs) -> Result<()> {
    let AccessListArgs {
        tenant: tenant_arg,
        json: json_output,
        role,
        pattern,
        method,
        duplicates,
        warnings,
    } = options;
    let duplicates_only = duplicates;
    let tenant = tenant_for(tenant_arg)?;
    let document = api::get_access(&tenant).await?;
    let roles = api::role_index(&tenant).await.ok();
    let findings = spec::validate_document(&document, roles.as_ref(), WarningScope::All);
    let warning_count = findings.warnings.len();
    report_errors_only(findings, warnings)?;
    let rules = ops::rules(&document)?;
    let entries = rule_entries(rules)
        .into_iter()
        .filter(|entry| {
            role.as_deref()
                .is_none_or(|role| comma_field_contains(entry.rule, "roles", role))
                && pattern.as_deref().is_none_or(|pattern| {
                    entry.rule.get("pattern").and_then(Value::as_str) == Some(pattern)
                })
                && method
                    .as_deref()
                    .is_none_or(|method| comma_field_contains(entry.rule, "methods", method))
                && (!duplicates_only || entry.duplicate)
        })
        .collect::<Vec<_>>();

    if json_output {
        print_json(&ListOutput {
            digest: spec::digest(&document),
            rules: entries,
        })
    } else {
        println!("document digest: {}", spec::digest(&document));
        print_rule_table(&entries);
        if warning_count > 0 && !warnings {
            eprintln!(
                "{warning_count} warning(s) about existing rules; re-run with --warnings to read them"
            );
        }
        Ok(())
    }
}

async fn show(address: &str, tenant_arg: Option<String>, json_output: bool) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let document = api::get_access(&tenant).await?;
    let rules = ops::rules(&document)?;
    let entries = spec::resolve_rule_address(rules, address)?
        .into_iter()
        .map(|index| RuleEntry::new(index, &rules[index], duplicate_at(rules, index)))
        .collect::<Vec<_>>();

    if json_output {
        print_json(&entries)
    } else {
        for (position, entry) in entries.iter().enumerate() {
            if position > 0 {
                println!();
            }
            println!("#{}  {}", entry.index, entry.digest);
            print_json(entry.rule)?;
        }
        Ok(())
    }
}

async fn get(tenant_arg: Option<String>, out: Option<PathBuf>) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let document = api::get_access(&tenant).await?;
    let Some(path) = out else {
        return print_json(&document);
    };
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    fs::write(&path, bytes)?;
    println!("wrote config/access to {}", path.display());
    Ok(())
}

async fn write(amendment: Amendment, options: AccessWriteArgs) -> Result<()> {
    let tenant = tenant_for(options.tenant)?;
    let permission = ensure_prod_confirmed(&tenant, options.yes)?;
    ensure_confirmation_available(options.yes, options.dry_run, prompting_disabled())?;
    print_disjunction_warning(&amendment);

    let before = api::get_access(&tenant).await?;
    let backup = if options.dry_run || options.no_backup {
        None
    } else {
        let path = backup_document(&tenant, &before, Utc::now())?;
        println!("backup: {}", path.display());
        Some(path)
    };

    spec::check_digest(options.if_digest.as_deref(), &before)?;
    let (after, touched) = apply_amendment(&before, amendment)?;
    let roles = api::role_index(&tenant).await.ok();
    let warning_scope = touched
        .as_ref()
        .map_or(WarningScope::All, WarningScope::Touched);
    report_findings(spec::validate_document(
        &after,
        roles.as_ref(),
        warning_scope,
    ))?;

    let summary = ops::changes(&before, &after, touched.as_ref());
    println!("{}", render_changes(&summary));
    if summary.changed.is_empty() {
        println!("config/access is unchanged");
        return Ok(());
    }
    if options.dry_run {
        return Ok(());
    }
    if !options.yes && !confirm_write(&tenant)? {
        return Err(Error::Config("config/access was not changed".into()));
    }

    write_confirmed(&permission, after)
        .await
        .map_err(|error| confirmed_write_error(error, backup.as_deref()))?;
    println!("updated config/access on tenant {tenant}");
    Ok(())
}

fn apply_amendment(
    before: &Value,
    amendment: Amendment,
) -> Result<(Value, Option<TouchedIndices>)> {
    match amendment {
        Amendment::Add(rule) => {
            ops::append(before, rule).map(|result| (result.document, Some(result.touched)))
        }
        Amendment::Edit { index, edit } => ops::replace_at(before, index, edit)
            .map(|result| (result.document, Some(result.touched))),
        Amendment::Remove(indices) => {
            ops::remove_at(before, &indices).map(|result| (result.document, Some(result.touched)))
        }
        Amendment::Apply(path) => Ok((serde_json::from_slice(&fs::read(path)?)?, None)),
    }
}

fn print_disjunction_warning(amendment: &Amendment) {
    match amendment {
        Amendment::Add(_) => {
            eprintln!("Rules are OR-ed: this can only grant access, never restrict it.");
        }
        Amendment::Edit { .. } | Amendment::Remove(_) => eprintln!(
            "Rules are OR-ed, so narrowing or removing a rule is the only way to revoke access — this can lock operators out."
        ),
        Amendment::Apply(_) => {}
    }
}

fn report_findings(findings: Findings) -> Result<()> {
    report_errors_only(findings, true)
}

/// Errors always speak; warnings only when `spell_out_warnings`.
///
/// A read verb over a document it did not author has to count rather than
/// enumerate — see [`AccessListArgs::warnings`]. Write verbs pass `true`, because
/// their warnings are already scoped to the rules they touched.
fn report_errors_only(findings: Findings, spell_out_warnings: bool) -> Result<()> {
    if spell_out_warnings {
        for warning in &findings.warnings {
            eprintln!("warning: {}", render_finding(warning));
        }
    }
    if findings.errors.is_empty() {
        return Ok(());
    }
    let messages = findings
        .errors
        .into_iter()
        .map(|finding| format!("- {}", render_finding(&finding)))
        .collect::<Vec<_>>()
        .join("\n");
    Err(Error::Config(format!(
        "config/access validation failed:\n{messages}"
    )))
}

fn render_finding(finding: &spec::Finding) -> String {
    finding.index.map_or_else(
        || finding.message.clone(),
        |index| format!("rule #{index}: {}", finding.message),
    )
}

fn ensure_confirmation_available(yes: bool, dry_run: bool, disabled: bool) -> Result<()> {
    if !yes && !dry_run && disabled {
        return Err(Error::Config(
            "config/access confirmation disabled by --no-prompt; pass --yes to write".into(),
        ));
    }
    Ok(())
}

fn confirm_write(tenant: &str) -> Result<bool> {
    use inquire::{Confirm, error::InquireError};
    match Confirm::new(&format!(
        "Write these config/access changes to tenant {tenant:?}?"
    ))
    .with_default(false)
    .prompt()
    {
        Ok(answer) => Ok(answer),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(false),
        Err(InquireError::NotTTY) => Err(Error::Config(
            "config/access changes require confirmation; pass --yes when no terminal is available"
                .into(),
        )),
        Err(error) => Err(Error::Config(format!(
            "confirm config/access changes: {error}"
        ))),
    }
}

async fn write_confirmed(
    permission: &WriteOk<'_>,
    after: Value,
) -> std::result::Result<(), api::ConfirmedWriteError> {
    api::put_access_confirmed(permission.tenant, after, permission.confirmed_prod).await
}

fn confirmed_write_error(error: api::ConfirmedWriteError, backup: Option<&Path>) -> Error {
    match error {
        api::ConfirmedWriteError::NotWritten(error) => error,
        api::ConfirmedWriteError::AcceptedButUnconfirmed(message) => {
            let recovery = backup.map_or_else(
                || "no backup was created (--no-backup)".to_string(),
                |path| format!("restore from {} with `aic access apply`", path.display()),
            );
            Error::Config(format!("{message}; {recovery}"))
        }
    }
}

fn render_changes(changes: &Changes) -> String {
    let mut lines = Vec::new();
    if changes.positions_approximate && !changes.changed.is_empty() {
        lines.push("  apply diff uses approximate positions; rules are matched by content".into());
    }
    for change in &changes.changed {
        if let Some(before) = &change.before {
            lines.push(format!("- #{} {}", change.index, compact_json(before)));
        }
        if let Some(after) = &change.after {
            lines.push(format!("+ #{} {}", change.index, compact_json(after)));
        }
    }
    lines.push(format!("  {} rules unchanged", changes.unchanged));
    lines.join("\n")
}

fn compact_json(value: &Value) -> String {
    // Serializing an already-constructed serde_json::Value cannot fail.
    serde_json::to_string(value).expect("serialize serde_json::Value")
}

fn backup_document(tenant: &str, document: &Value, now: DateTime<Utc>) -> Result<PathBuf> {
    ProjectConfig::write_gitignore()?;
    let path = ProjectConfig::dir()
        .join("backups")
        .join(backup_filename(tenant, now));
    write_backup(&path, document)?;
    Ok(path)
}

fn backup_filename(tenant: &str, now: DateTime<Utc>) -> String {
    let tenant = tenant
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("access-{tenant}-{}.json", now.format("%Y%m%dT%H%M%SZ"))
}

fn write_backup(path: &Path, document: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, document)?;
    file.write_all(b"\n")?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[derive(Serialize)]
struct ListOutput<'a> {
    digest: String,
    rules: Vec<RuleEntry<'a>>,
}

#[derive(Serialize)]
struct RuleEntry<'a> {
    index: usize,
    digest: String,
    duplicate: bool,
    pattern: String,
    methods: String,
    roles: String,
    rule: &'a Value,
}

impl<'a> RuleEntry<'a> {
    fn new(index: usize, rule: &'a Value, duplicate: bool) -> Self {
        let view = RuleView::from_value(rule);
        Self {
            index,
            digest: spec::short_digest(rule),
            duplicate,
            pattern: view.pattern,
            methods: view.methods,
            roles: view.roles,
            rule,
        }
    }
}

fn rule_entries(rules: &[Value]) -> Vec<RuleEntry<'_>> {
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| RuleEntry::new(index, rule, duplicate_at(rules, index)))
        .collect()
}

fn duplicate_at(rules: &[Value], index: usize) -> bool {
    rules
        .iter()
        .enumerate()
        .any(|(other, rule)| other != index && rule == &rules[index])
}

fn comma_field_contains(rule: &Value, field: &str, needle: &str) -> bool {
    rule.get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| value.split(',').map(str::trim).any(|item| item == needle))
}

fn print_rule_table(entries: &[RuleEntry<'_>]) {
    let rows = entries
        .iter()
        .map(|entry| {
            vec![
                entry.index.to_string(),
                entry.digest.clone(),
                entry.pattern.clone(),
                entry.methods.clone(),
                entry.roles.clone(),
                if entry.duplicate { "dup" } else { "" }.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        &["#", "DIGEST", "PATTERN", "METHODS", "ROLES", "DUP"],
        &rows,
    );
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    #[test]
    fn no_prompt_without_yes_is_refused() {
        let error = ensure_confirmation_available(false, false, true).unwrap_err();
        assert!(
            matches!(error, Error::Config(message) if message.contains("--no-prompt") && message.contains("--yes"))
        );
        assert!(ensure_confirmation_available(false, true, true).is_ok());
        assert!(ensure_confirmation_available(true, false, true).is_ok());
    }

    #[test]
    fn out_of_range_index_names_the_valid_range() {
        let error = apply_amendment(
            &crate::access::six_rule_fixture(),
            Amendment::Edit {
                index: 6,
                edit: RuleEdit::default(),
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Config(message) if message.contains("0..=5")));
    }

    #[test]
    fn change_summary_renders_an_edit_add_and_removal() {
        let changes = Changes {
            changed: vec![
                ops::RuleChange {
                    index: 1,
                    before: Some(json!({"methods": "read"})),
                    after: Some(json!({"methods": "read,query"})),
                },
                ops::RuleChange {
                    index: 2,
                    before: None,
                    after: Some(json!({"pattern": "endpoint/new"})),
                },
                ops::RuleChange {
                    index: 3,
                    before: Some(json!({"pattern": "endpoint/old"})),
                    after: None,
                },
            ],
            unchanged: 7,
            positions_approximate: false,
        };

        assert_eq!(
            render_changes(&changes),
            "- #1 {\"methods\":\"read\"}\n+ #1 {\"methods\":\"read,query\"}\n+ #2 {\"pattern\":\"endpoint/new\"}\n- #3 {\"pattern\":\"endpoint/old\"}\n  7 rules unchanged"
        );
    }

    #[test]
    fn backup_guidance_is_only_added_after_an_accepted_write() {
        let path = Path::new(".aic/backups/access-sandbox.json");
        let not_written = confirmed_write_error(
            api::ConfirmedWriteError::NotWritten(Error::Config("PUT failed".into())),
            Some(path),
        );
        assert!(matches!(not_written, Error::Config(message) if message == "PUT failed"));

        let unconfirmed = confirmed_write_error(
            api::ConfirmedWriteError::AcceptedButUnconfirmed("read-back differed".into()),
            Some(path),
        );
        assert!(
            matches!(unconfirmed, Error::Config(message) if message.contains("read-back differed") && message.contains("access-sandbox.json"))
        );
    }

    #[test]
    fn backup_filename_is_utc_and_writer_sets_mode_0600() {
        let now = Utc.with_ymd_and_hms(2026, 8, 11, 4, 5, 6).unwrap();
        let filename = backup_filename("sandbox", now);
        assert_eq!(filename, "access-sandbox-20260811T040506Z.json");

        let dir = std::env::temp_dir().join(format!("aic-access-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join(filename);
        write_backup(&path, &json!({"_id": "access", "configs": []})).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, json!({"_id": "access", "configs": []}));
        fs::remove_dir_all(dir).unwrap();
    }
}
