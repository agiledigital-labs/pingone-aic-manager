//! `aic access` parser and guarded `config/access` command implementation.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::access::ops::{self, Changes};
use crate::access::spec::{
    self, Amendment, RuleEdit, RuleSpec, RuleSummary, TouchedIndices, WarningScope,
};
use crate::access::{api, spec::Findings};
use crate::cli::{
    WriteOk, confirm_destructive, ensure_prod_confirmed, print_json, prompt_available, tenant_for,
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

#[derive(Debug)]
struct Plan {
    backup: bool,
    after: Value,
    touched: TouchedIndices,
    summary: Changes,
    needs_confirm: bool,
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
        } => {
            write(
                Amendment::Apply(serde_json::from_slice(&fs::read(file)?)?),
                options,
            )
            .await
        }
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
    let roles = resolve_roles(&tenant).await;
    let findings = spec::validate_document(&document, roles.as_ref(), WarningScope::All);
    let warning_count = findings.warnings.len();
    report_read_findings(&findings, warnings)?;
    let rules = ops::rules(&document)?;
    let summaries = spec::rule_summaries(rules);
    let selected = summaries
        .iter()
        .zip(rules)
        .filter(|(summary, rule)| {
            role.as_deref()
                .is_none_or(|role| spec::comma_list_contains(rule, "roles", role))
                && pattern.as_deref().is_none_or(|pattern| {
                    rule.get("pattern").and_then(Value::as_str) == Some(pattern)
                })
                && method
                    .as_deref()
                    .is_none_or(|method| spec::comma_list_contains(rule, "methods", method))
                && (!duplicates_only || summary.duplicate)
        })
        .collect::<Vec<_>>();

    if json_output {
        let entries = selected
            .iter()
            .map(|(summary, rule)| RuleEntry::new(summary, rule))
            .collect();
        print_json(&ListOutput {
            digest: spec::digest(&document),
            rules: entries,
        })
    } else {
        println!("document digest: {}", spec::digest(&document));
        if !selected.is_empty() {
            println!();
            print!(
                "{}",
                render_rule_blocks(selected.iter().map(|(summary, rule)| (*summary, *rule)),)
            );
            println!();
            println!("Full rule bodies: aic access show <address> or aic access list --json");
        } else {
            println!("No rules matched the supplied filters.");
        }
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
    let summaries = spec::rule_summaries(rules);
    let entries = spec::resolve_rule_address(rules, address)?
        .into_iter()
        .map(|index| RuleEntry::new(&summaries[index], &rules[index]))
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
    ProjectConfig::write_gitignore()?;
    crate::access::ops::write_private_file(&path, &bytes, false)?;
    println!("wrote config/access to {}", path.display());
    Ok(())
}

async fn write(amendment: Amendment, options: AccessWriteArgs) -> Result<()> {
    let tenant = tenant_for(options.tenant.clone())?;
    let permission = if options.dry_run {
        None
    } else {
        Some(ensure_prod_confirmed(&tenant, options.yes)?)
    };
    ensure_confirmation_available(&options)?;

    let before = api::get_access(&tenant).await?;
    let plan = plan(&before, amendment, &options)?;
    let backup = if plan.backup {
        let path = ops::backup_document(&tenant, &before, Utc::now()).map_err(|error| {
            Error::Config(format!(
                "{error}; pass --no-backup to proceed without a backup"
            ))
        })?;
        println!("backup: {}", path.display());
        Some(path)
    } else {
        None
    };

    spec::check_digest(options.if_digest.as_deref(), &before)?;
    let roles = resolve_roles(&tenant).await;
    report_write_findings(
        spec::validate_document(
            &plan.after,
            roles.as_ref(),
            WarningScope::Touched(&plan.touched),
        ),
        &plan.touched,
    )?;

    println!("{}", render_changes(&plan.summary));
    print_disjunction_warning(&plan.summary);
    if plan.summary.changed.is_empty() {
        println!("config/access is unchanged");
        return Ok(());
    }
    let Some(permission) = permission else {
        debug_assert!(options.dry_run);
        return Ok(());
    };
    if plan.needs_confirm
        && !confirm_destructive(
            "config/access changes",
            &format!("Write these config/access changes to tenant {tenant:?}?"),
            "--yes",
        )?
    {
        return Err(Error::Config("config/access was not changed".into()));
    }

    write_confirmed(permission, plan.after)
        .await
        .map_err(|error| confirmed_write_error(error, backup.as_deref()))?;
    println!("updated config/access on tenant {tenant}");
    Ok(())
}

fn plan(before: &Value, amendment: Amendment, options: &AccessWriteArgs) -> Result<Plan> {
    let amended = ops::amend(before, amendment)?;
    let needs_confirm = !amended.summary.changed.is_empty() && !options.dry_run && !options.yes;
    Ok(Plan {
        backup: !options.dry_run && !options.no_backup,
        after: amended.after,
        touched: amended.touched,
        summary: amended.summary,
        needs_confirm,
    })
}

async fn resolve_roles(tenant: &str) -> Option<spec::RoleIndex> {
    match api::role_index(tenant).await {
        Ok(roles) => Some(roles),
        Err(error) => {
            eprintln!(
                "warning: could not resolve roles ({error}); role references were not checked"
            );
            None
        }
    }
}

fn print_disjunction_warning(summary: &Changes) {
    if summary.changed.iter().any(|change| change.before.is_some()) {
        eprintln!(
            "Rules are OR-ed, so narrowing or removing a rule is the only way to revoke access — this can lock operators out."
        );
    } else if !summary.changed.is_empty() {
        eprintln!("Rules are OR-ed: this can only grant access, never restrict it.");
    }
}

fn report_write_findings(findings: Findings, touched: &TouchedIndices) -> Result<()> {
    for warning in &findings.warnings {
        eprintln!("warning: {}", render_finding(warning));
    }
    if findings.errors.is_empty() {
        return Ok(());
    }
    let has_untouched_rule_error = findings.errors.iter().any(|finding| {
        finding
            .index
            .is_some_and(|index| !touched.after().contains(&index))
    });
    let messages = findings
        .errors
        .into_iter()
        .map(|finding| format!("- {}", render_finding(&finding)))
        .collect::<Vec<_>>()
        .join("\n");
    let escape = if has_untouched_rule_error {
        "\nUntouched invalid rules are still fatal. Run `aic access list` for their indices and digests, then repair them with `aic access edit`, remove them with `aic access rm`, or replace the document with `aic access apply`."
    } else {
        ""
    };
    Err(Error::Config(format!(
        "config/access validation failed:\n{messages}{escape}"
    )))
}

/// Reads report malformed foreign rules but remain usable for recovery.
fn report_read_findings(findings: &Findings, spell_out_warnings: bool) -> Result<()> {
    if spell_out_warnings {
        for warning in &findings.warnings {
            eprintln!("warning: {}", render_finding(warning));
        }
    }
    for error in &findings.errors {
        eprintln!("error: {}", render_finding(error));
    }
    if findings
        .errors
        .iter()
        .any(|finding| finding.index.is_none())
    {
        return Err(Error::Config(
            "config/access document shape is invalid; rules cannot be rendered safely".into(),
        ));
    }
    Ok(())
}

fn render_finding(finding: &spec::Finding) -> String {
    finding.index.map_or_else(
        || finding.message.clone(),
        |index| format!("rule #{index}: {}", finding.message),
    )
}

fn ensure_confirmation_available(options: &AccessWriteArgs) -> Result<()> {
    if !options.yes && !options.dry_run && !prompt_available() {
        return Err(Error::Config(
            "config/access changes require confirmation; pass --yes when no terminal is available"
                .into(),
        ));
    }
    Ok(())
}

async fn write_confirmed(
    permission: WriteOk<'_>,
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
    // A move has no `changed` entries by design — nothing is granted or
    // withdrawn — so it would otherwise render as "N rules unchanged" and
    // nothing else.
    for moved in &changes.moved {
        lines.push(format!(
            "~ #{} -> #{} (order only; rules are OR-ed)",
            moved.from, moved.to
        ));
    }
    lines.push(format!("  {} rules unchanged", changes.unchanged));
    lines.join("\n")
}

fn compact_json(value: &Value) -> String {
    // Serializing an already-constructed serde_json::Value cannot fail.
    serde_json::to_string(value).expect("serialize serde_json::Value")
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
    fn new(summary: &RuleSummary, rule: &'a Value) -> Self {
        Self {
            index: summary.index,
            digest: summary.digest.clone(),
            duplicate: summary.duplicate,
            pattern: summary.pattern.clone(),
            methods: summary.methods.clone(),
            roles: summary.roles.clone(),
            rule,
        }
    }
}

const CUSTOM_AUTHZ_LIST_CLIP: usize = 100;

fn render_rule_blocks<'a>(rules: impl IntoIterator<Item = (&'a RuleSummary, &'a Value)>) -> String {
    let mut output = String::new();
    for (position, (summary, rule)) in rules.into_iter().enumerate() {
        if position > 0 {
            output.push('\n');
        }
        let duplicate = if summary.duplicate { "   dup" } else { "" };
        let _ = writeln!(
            output,
            "#{}   {}{}",
            summary.index, summary.digest, duplicate
        );
        if rule.get("pattern").is_some() {
            push_rule_field(&mut output, "pattern", &summary.pattern);
        }
        if rule.get("roles").is_some() {
            push_rule_field(&mut output, "roles", &summary.roles);
        }
        if rule.get("methods").is_some() {
            push_rule_field(&mut output, "methods", &summary.methods);
        }
        if let Some(actions) = &summary.actions
            && rule.get("actions").is_some()
        {
            push_rule_field(&mut output, "actions", actions);
        }
        if let Some(custom_authz) = &summary.custom_authz
            && rule.get("customAuthz").is_some()
        {
            push_rule_field(
                &mut output,
                "customAuthz",
                &crate::cli::clip(custom_authz, CUSTOM_AUTHZ_LIST_CLIP),
            );
        }
        if let Some(exclude_patterns) = &summary.exclude_patterns
            && rule.get("excludePatterns").is_some()
        {
            push_rule_field(&mut output, "excludePatterns", exclude_patterns);
        }
    }
    output
}

fn push_rule_field(output: &mut String, label: &str, value: &str) {
    if value.is_empty() {
        let _ = writeln!(output, "  {label}");
    } else {
        let _ = writeln!(output, "  {label:<16} {value}");
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use serde_json::json;

    use super::*;

    fn options(yes: bool, dry_run: bool, no_backup: bool) -> AccessWriteArgs {
        AccessWriteArgs {
            if_digest: None,
            yes,
            dry_run,
            no_backup,
            tenant: None,
        }
    }

    #[test]
    fn list_json_rule_keys_remain_backward_compatible() {
        // Adding shared-summary fields through serde flattening, or renaming
        // the thin wrapper's legacy keys, makes this compatibility set fail.
        let fixture = crate::access::six_rule_fixture();
        let rules = fixture["configs"].as_array().unwrap();
        let summaries = spec::rule_summaries(rules);
        let value = serde_json::to_value(RuleEntry::new(&summaries[0], &rules[0])).unwrap();
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            keys,
            [
                "digest",
                "duplicate",
                "index",
                "methods",
                "pattern",
                "roles",
                "rule"
            ]
        );
        assert_eq!(value["roles"], "internal/role/user-reader");
    }

    #[test]
    fn list_blocks_distinguish_absent_and_present_empty_fields() {
        // Defaulting an absent actions key, or suppressing a present empty
        // value, makes the corresponding line-presence assertion fail.
        let fixture = crate::access::six_rule_fixture();
        let rules = fixture["configs"].as_array().unwrap();
        let summaries = spec::rule_summaries(rules);

        let absent = render_rule_blocks([(&summaries[0], &rules[0])]);
        assert!(!absent.lines().any(|line| line.trim() == "actions"));

        let present_empty_rule = json!({
            "pattern": "managed/user/*",
            "roles": "internal/role/user-reader",
            "methods": "read",
            "actions": ""
        });
        let present_empty_summary = spec::rule_summaries(std::slice::from_ref(&present_empty_rule))
            .into_iter()
            .next()
            .unwrap();
        let present_empty = render_rule_blocks([(&present_empty_summary, &present_empty_rule)]);
        assert!(present_empty.lines().any(|line| line == "  actions"));

        let missing_pattern = json!({"roles": "*", "methods": "read"});
        let missing_pattern_summary = spec::rule_summaries(std::slice::from_ref(&missing_pattern))
            .into_iter()
            .next()
            .unwrap();
        let missing = render_rule_blocks([(&missing_pattern_summary, &missing_pattern)]);
        assert!(!missing.lines().any(|line| line.trim() == "pattern"));
    }

    #[test]
    fn list_blocks_keep_full_role_paths_and_render_all_optional_values() {
        // Stripping roles in the shared projection, or overlooking an optional
        // summary field in the block renderer, makes this output fail.
        let fixture = crate::access::six_rule_fixture();
        let rules = fixture["configs"].as_array().unwrap();
        let summaries = spec::rule_summaries(rules);
        let rendered = render_rule_blocks([(&summaries[1], &rules[1]), (&summaries[2], &rules[2])]);

        assert!(rendered.contains("roles            internal/role/user-owner"));
        assert!(rendered.contains("actions          *"));
        assert!(rendered.contains("customAuthz      ownDataOnly()"));
        assert!(rendered.contains("excludePatterns  endpoint/report/private/*"));
        assert_eq!(rendered.matches("\n\n#").count(), 1);
    }

    #[test]
    fn list_blocks_clip_custom_authz_with_the_shared_cli_helper() {
        // Printing raw multiline scripts or introducing a local truncation
        // policy makes the collapsed, ellipsized line fail.
        let summary = RuleSummary {
            index: 0,
            digest: "01234567".into(),
            duplicate: false,
            pattern: "endpoint/x".into(),
            methods: "read".into(),
            roles: "*".into(),
            actions: None,
            custom_authz: Some(format!("first line\n{}", "x".repeat(100))),
            exclude_patterns: None,
        };

        let rule = json!({
            "pattern": "endpoint/x",
            "methods": "read",
            "roles": "*",
            "customAuthz": summary.custom_authz.clone().unwrap()
        });
        let rendered = render_rule_blocks([(&summary, &rule)]);
        assert!(rendered.contains("customAuthz      first line "));
        assert!(rendered.contains("...\n"));
        assert!(!rendered.contains("first line\n"));
    }

    #[test]
    fn confirmation_gate_is_wired_to_the_full_prompt_predicate() {
        let source = include_str!("cli.rs");
        assert!(
            source.contains("!options.yes && !options.dry_run && !prompt_available()"),
            "the access pre-fetch gate must use prompt_available()"
        );
        assert!(
            !source.contains(&["prompting", "_disabled"].concat()),
            "the weaker --no-prompt-only predicate must not be wired into access"
        );
    }

    #[test]
    fn out_of_range_index_names_the_valid_range() {
        let error = plan(
            &crate::access::six_rule_fixture(),
            Amendment::Edit {
                index: 6,
                edit: RuleEdit::default(),
            },
            &options(true, false, false),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Config(message) if message.contains("0..=5")));
    }

    #[test]
    fn plans_make_backup_confirmation_and_write_decisions_explicit() {
        let before = crate::access::six_rule_fixture();
        for (name, amendment, options, backup, changed, needs_confirm) in [
            (
                "dry run",
                Amendment::Remove(vec![1]),
                options(false, true, false),
                false,
                true,
                false,
            ),
            (
                "interactive write",
                Amendment::Remove(vec![1]),
                options(false, false, false),
                true,
                true,
                true,
            ),
            (
                "yes write without backup",
                Amendment::Remove(vec![1]),
                options(true, false, true),
                false,
                true,
                false,
            ),
            (
                "unchanged apply",
                Amendment::Apply(before.clone()),
                options(true, false, false),
                true,
                false,
                false,
            ),
        ] {
            let plan = plan(&before, amendment, &options).unwrap();
            assert_eq!(plan.backup, backup, "{name}");
            assert_eq!(!plan.summary.changed.is_empty(), changed, "{name}");
            assert_eq!(plan.needs_confirm, needs_confirm, "{name}");
        }
    }

    #[test]
    fn apply_normalises_or_rejects_the_document_id() {
        let before = crate::access::six_rule_fixture();
        let mut missing = before.clone();
        missing.as_object_mut().unwrap().remove("_id");
        let planned = plan(
            &before,
            Amendment::Apply(missing),
            &options(true, true, false),
        )
        .unwrap();
        assert_eq!(planned.after["_id"], "access");

        let mut wrong = before.clone();
        wrong["_id"] = json!("authentication");
        let error = plan(
            &before,
            Amendment::Apply(wrong),
            &options(true, true, false),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Config(message) if message.contains("must be \"access\"")));
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
            touched: TouchedIndices::default(),
            positions_approximate: false,
            moved: Vec::new(),
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
    fn get_output_writer_sets_mode_0600() {
        // Removing the mode from write_private_file makes this expose a saved
        // raw access document with the process umask's broader permissions.
        let filename = "access-sandbox.json";
        let dir = std::env::temp_dir().join(format!("aic-access-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join(filename);
        let mut bytes =
            serde_json::to_vec_pretty(&json!({"_id": "access", "configs": []})).unwrap();
        bytes.push(b'\n');
        crate::access::ops::write_private_file(&path, &bytes, true).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, json!({"_id": "access", "configs": []}));
        fs::remove_dir_all(dir).unwrap();
    }
}
