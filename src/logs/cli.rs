//! `aic logs` parser and command implementation.

use std::future::Future;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use clap::Subcommand;
use inquire::{Password, PasswordDisplayMode, Text, error::InquireError};
use serde::Serialize;
use serde_json::Value;

use crate::agent::AgentClient;
use crate::cli::{WriteOk, ensure_prod_confirmed, print_table, tenant_for};
use crate::config::{CredentialSource, ProjectConfig};
#[cfg(feature = "logs-store")]
use crate::logs::db::store_path;
use crate::logs::{api, ops};
#[cfg(feature = "logs-store")]
use crate::logs::{db, journey};
use crate::onboard::bootstrap::{mint_log_key_via_session, no_redirect_client};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum LogsCommand {
    /// Manage the tenant's log API key pair.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// List available log source ids.
    Sources {
        #[arg(long, help = "Print the source list as JSON")]
        json: bool,
        #[arg(
            long,
            value_name = "PATH",
            help = "Write the JSON source list to a file"
        )]
        output: Option<PathBuf>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Fetch all events for a transaction id.
    Tx {
        transaction_id: String,
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(long, value_name = "PATH", help = "Write the JSON result to a file")]
        output: Option<PathBuf>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Poll until the trailing AM-ACCESS-OUTCOME event arrives.
        #[arg(long)]
        wait: bool,
        /// Seconds to poll with --wait (ignored otherwise). Still writes
        /// whatever has arrived if the outcome never shows up.
        #[arg(long, value_name = "SECS", default_value_t = 60)]
        timeout: u64,
    },
    /// Fetch events in an ISO-8601 time range.
    Range {
        begin: String,
        end: String,
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(long, help = "Optional CREST _queryFilter")]
        query: Option<String>,
        #[arg(long, value_name = "PATH", help = "Write the JSON result to a file")]
        output: Option<PathBuf>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Run a CREST filter. Defaults to the most recent 24 hours.
    Query {
        filter: String,
        #[arg(long, help = "Range start; defaults to 24h before end")]
        begin: Option<String>,
        #[arg(long, help = "Range end; defaults to now")]
        end: Option<String>,
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(long, value_name = "PATH", help = "Write the JSON result to a file")]
        output: Option<PathBuf>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Find events whose payload contains a substring.
    Grep {
        pattern: String,
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(
            long,
            value_name = "DURATION",
            conflicts_with_all = ["begin", "end"],
            help = "Recent window (s/m/h suffix; default: 15m)"
        )]
        since: Option<String>,
        #[arg(
            long,
            value_name = "ISO-8601",
            requires = "end",
            conflicts_with = "since",
            help = "Fixed range start (requires --end)"
        )]
        begin: Option<String>,
        #[arg(
            long,
            value_name = "ISO-8601",
            requires = "begin",
            conflicts_with = "since",
            help = "Fixed range end (requires --begin)"
        )]
        end: Option<String>,
        #[arg(long, value_name = "PATH", help = "Write the JSON result to a file")]
        output: Option<PathBuf>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Follow new events until Ctrl-C (JSON Lines on stdout).
    Tail {
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(long, value_name = "TEXT", help = "Filter by payload substring")]
        pattern: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    // Without the feature these three would simply not exist, and clap would
    // answer `unrecognized subcommand 'search'` — which reads as "no such
    // command" when the truth is "not in this build". docs/CLI.md documents
    // them, and released binaries are built without the feature, so that error
    // is the one a reader is most likely to hit.
    #[cfg(not(feature = "logs-store"))]
    /// Search the local synced log store. Needs the `logs-store` build feature.
    Search {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    #[cfg(not(feature = "logs-store"))]
    /// Roll up and prune the local store. Needs the `logs-store` build feature.
    Compact {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    #[cfg(not(feature = "logs-store"))]
    /// Sync logs into the local store. Needs the `logs-store` build feature.
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    #[cfg(feature = "logs-store")]
    /// Search the local synced log store (offline; reads the DuckDB file).
    Search {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, value_name = "ID", help = "Filter by transactionId")]
        tx: Option<String>,
        #[arg(long, value_name = "SOURCE", help = "Filter by exact source id")]
        source: Option<String>,
        #[arg(long, value_name = "NAME", help = "Filter by eventName")]
        event: Option<String>,
        #[arg(long, value_name = "ID", help = "Filter by userId")]
        user: Option<String>,
        #[arg(long, value_name = "LEVEL", help = "Filter by level (INFO/WARN/ERROR)")]
        level: Option<String>,
        #[arg(long, help = "Range start (ISO-8601)")]
        begin: Option<String>,
        #[arg(long, help = "Range end (ISO-8601)")]
        end: Option<String>,
        #[arg(long, value_name = "TEXT", help = "Substring match within the payload")]
        contains: Option<String>,
        #[arg(long, default_value_t = 1000, help = "Max rows to return")]
        limit: usize,
        #[arg(long, help = "Print only the match count, not the events")]
        count: bool,
        #[arg(
            long,
            value_name = "PATH",
            help = "Write the JSON result to a file (ignored with --count)"
        )]
        output: Option<PathBuf>,
    },
    #[cfg(feature = "logs-store")]
    /// Roll up am-authentication into the journey model and prune old raw events.
    Compact {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(
            long,
            default_value_t = 3,
            help = "Keep raw log_events younger than N months"
        )]
        retain_months: i64,
    },
    #[cfg(feature = "logs-store")]
    /// Incrementally sync logs into the local DuckDB store.
    Sync {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, value_name = "CSV", help = "Comma-separated log sources")]
        source: Option<String>,
        #[arg(long, help = "Override the incremental cursor with an ISO-8601 start")]
        since: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeyCommand {
    /// Store or replace a log API key pair.
    Set {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Log API key id")]
        id: Option<String>,
    },
    /// Mint a new log API key for an existing tenant via an admin session.
    Create {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(
            long,
            help = "AM session cookie name (random-hex). Prompted if omitted."
        )]
        cookie_name: Option<String>,
        /// Confirm key creation on a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Show whether a log API key pair is stored.
    Show {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Remove the stored log API key pair.
    Rm {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

pub async fn run(cmd: LogsCommand) -> Result<()> {
    match cmd {
        LogsCommand::Key { command } => run_key(command).await,
        LogsCommand::Sources {
            json,
            output,
            tenant,
        } => {
            let context = ops::fetch_context(tenant).await?;
            let sources = api::sources(&context.client, &context.base_url, &context.key).await?;
            if let Some(path) = output {
                write_json(&sources, Some(&path))
            } else if json {
                write_json(&sources, None)
            } else {
                let rows = sources
                    .into_iter()
                    .map(|source| vec![source])
                    .collect::<Vec<_>>();
                print_table(&["SOURCE"], &rows);
                Ok(())
            }
        }
        LogsCommand::Tx {
            transaction_id,
            source,
            output,
            tenant,
            wait,
            timeout,
        } => {
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            let events = api::fetch_transaction(
                &context.client,
                &context.base_url,
                &context.key,
                &transaction_id,
                &sources,
            )
            .await?;
            let events = maybe_wait_for_outcome(
                &transaction_id,
                events,
                wait,
                std::time::Duration::from_secs(timeout),
                || {
                    api::fetch_transaction(
                        &context.client,
                        &context.base_url,
                        &context.key,
                        &transaction_id,
                        &sources,
                    )
                },
            )
            .await?;
            write_json(&events, output.as_deref())
        }
        LogsCommand::Range {
            begin,
            end,
            source,
            query,
            output,
            tenant,
        } => {
            let begin = parse_time(&begin, "begin")?;
            let end = parse_time(&end, "end")?;
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            let result = api::fetch_range(
                &context.client,
                &context.base_url,
                &context.key,
                begin,
                end,
                &sources,
                query.as_deref(),
            )
            .await?;
            write_json(&result, output.as_deref())
        }
        LogsCommand::Query {
            filter,
            begin,
            end,
            source,
            output,
            tenant,
        } => {
            if filter.trim().is_empty() {
                return Err(Error::Config("log query filter cannot be empty".into()));
            }
            let (begin, end) = query_range(begin.as_deref(), end.as_deref(), Utc::now())?;
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            let result = api::fetch_range(
                &context.client,
                &context.base_url,
                &context.key,
                begin,
                end,
                &sources,
                Some(&filter),
            )
            .await?;
            write_json(&result, output.as_deref())
        }
        LogsCommand::Grep {
            pattern,
            source,
            since,
            begin,
            end,
            output,
            tenant,
        } => {
            require_pattern(&pattern)?;
            let (begin, end) = grep_range(
                since.as_deref(),
                begin.as_deref(),
                end.as_deref(),
                Utc::now(),
            )?;
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            let writer: Box<dyn Write + Send> = match output {
                Some(path) => Box::new(BufWriter::new(std::fs::File::create(path)?)),
                None => Box::new(BufWriter::new(std::io::stdout())),
            };
            let mut output = JsonArrayWriter::new(writer);
            let fetch_result = {
                let mut on_page = |mut page: Vec<Value>| -> Result<()> {
                    sort_events_by_timestamp(&mut page);
                    for event in page {
                        if payload_matches(&event, &pattern) {
                            output.write(&event)?;
                        }
                    }
                    Ok(())
                };
                api::fetch_range_streamed(
                    &context.client,
                    &context.base_url,
                    &context.key,
                    begin,
                    end,
                    &sources,
                    None,
                    &mut on_page,
                )
                .await
            };
            let finish_result = output.finish();
            fetch_result.and(finish_result)
        }
        LogsCommand::Tail {
            source,
            pattern,
            tenant,
        } => {
            if let Some(pattern) = pattern.as_deref() {
                require_pattern(pattern)?;
            }
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            tail(&context, &sources, pattern.as_deref()).await
        }
        #[cfg(not(feature = "logs-store"))]
        LogsCommand::Search { .. } | LogsCommand::Compact { .. } | LogsCommand::Sync { .. } => {
            Err(crate::Error::Config(
                "this build has no local log store: rebuild with \
             `cargo build --release --features logs-store`. Released binaries \
             are built without it; `aic logs tx|range|query` work in every build"
                    .into(),
            ))
        }
        #[cfg(feature = "logs-store")]
        LogsCommand::Search {
            tenant,
            tx,
            source,
            event,
            user,
            level,
            begin,
            end,
            contains,
            limit,
            count,
            output,
        } => {
            let tenant = tenant_for(tenant)?;
            let path = store_path(&tenant);
            if !path.exists() {
                return Err(Error::Config(format!(
                    "no local log store for tenant '{tenant}'; run `aic logs sync` first"
                )));
            }
            let begin = begin
                .as_deref()
                .map(|value| parse_time(value, "begin"))
                .transpose()?;
            let end = end
                .as_deref()
                .map(|value| parse_time(value, "end"))
                .transpose()?;
            if let (Some(begin), Some(end)) = (begin, end) {
                if end <= begin {
                    return Err(Error::Config(
                        "log search end must be after begin".to_string(),
                    ));
                }
            }
            let params = db::SearchParams {
                transaction_id: tx,
                source,
                event_name: event,
                user_id: user,
                level,
                begin,
                end,
                contains,
                limit,
            };
            let conn = db::open(&path)?;
            if count {
                let n = db::count_matching(&conn, &params)?;
                println!("{n}");
            } else {
                let events = db::search(&conn, &params)?;
                write_json(&events, output.as_deref())?;
            }
            Ok(())
        }
        #[cfg(feature = "logs-store")]
        LogsCommand::Compact {
            tenant,
            retain_months,
        } => {
            let report = journey::compact_tenant(tenant, retain_months).await?;
            println!(
                "rolled up {} attempts across {} journeys; pruned {} raw events",
                report.attempts_upserted, report.journeys, report.events_pruned
            );
            Ok(())
        }
        #[cfg(feature = "logs-store")]
        LogsCommand::Sync {
            tenant,
            source,
            since,
        } => {
            let sources = parse_sync_sources(source.as_deref())?;
            let since = since
                .as_deref()
                .map(|value| parse_time(value, "since"))
                .transpose()?;
            let reports = ops::sync_tenant(tenant, &sources, since).await?;
            let mut total_fetched = 0;
            let mut total_filtered = 0;
            let mut total_inserted = 0;
            for report in reports {
                println!(
                    "{}: fetched {}, filtered {}, new {}",
                    report.source, report.fetched, report.filtered, report.inserted
                );
                total_fetched += report.fetched;
                total_filtered += report.filtered;
                total_inserted += report.inserted;
            }
            println!(
                "total: fetched {total_fetched}, filtered {total_filtered}, new {total_inserted}"
            );
            Ok(())
        }
    }
}

async fn run_key(cmd: KeyCommand) -> Result<()> {
    match cmd {
        KeyCommand::Set { tenant, id } => {
            let tenant = tenant_for(tenant)?;
            let api_key_id = match id {
                Some(id) => id,
                None => {
                    require_prompt("log API key id")?;
                    prompt(Text::new("Log API key id").prompt(), "log API key id")?
                }
            };
            let api_key_id = api_key_id.trim().to_string();
            if api_key_id.is_empty() {
                return Err(Error::Config("log API key id cannot be empty".into()));
            }

            require_prompt("log API key secret")?;
            let api_key_secret = prompt(
                Password::new("Log API key secret")
                    .with_display_mode(PasswordDisplayMode::Hidden)
                    .without_confirmation()
                    .prompt(),
                "log API key secret",
            )?;
            if api_key_secret.is_empty() {
                return Err(Error::Config("log API key secret cannot be empty".into()));
            }

            let agent = AgentClient::connect_or_spawn().await?;
            let pair = crate::logs::LogKeyPair {
                api_key_id: api_key_id.clone(),
                api_key_secret,
            };
            crate::logs::put_log_key(agent, &tenant, &pair).await?;
            persist_log_key_provenance(&tenant, CredentialSource::External)?;
            println!("stored log API key {api_key_id} for tenant {tenant}");
            verify_stored_key(&tenant).await;
            Ok(())
        }
        KeyCommand::Create {
            tenant,
            cookie_name,
            yes,
        } => {
            let (tenant, base_url) = configured_tenant_base_url(tenant)?;
            let ok = ensure_prod_confirmed(&tenant, yes)?;
            let cookie_name = match cookie_name {
                Some(cookie_name) => cookie_name,
                None => {
                    require_prompt("AM session cookie name")?;
                    prompt(
                        Text::new("AM session cookie name").prompt(),
                        "AM session cookie name",
                    )?
                }
            };
            let cookie_name = cookie_name.trim().to_string();
            if cookie_name.is_empty() {
                return Err(Error::Config(
                    "AM session cookie name cannot be empty".into(),
                ));
            }

            require_prompt("AM session cookie value")?;
            let cookie_value = prompt(
                Password::new("AM session cookie value")
                    .with_display_mode(PasswordDisplayMode::Hidden)
                    .without_confirmation()
                    .prompt(),
                "AM session cookie value",
            )?;
            if cookie_value.is_empty() {
                return Err(Error::Config(
                    "AM session cookie value cannot be empty".into(),
                ));
            }

            let client = no_redirect_client()?;
            let minted = after_prod_confirmation(
                ok,
                &tenant,
                mint_log_key_via_session(
                    &client,
                    &base_url,
                    Some(&cookie_name),
                    &cookie_value,
                    &tenant,
                    None,
                ),
            )
            .await?;
            let name = minted.credential_name;
            let pair = minted.key;
            let api_key_id = pair.api_key_id.clone();

            let agent = AgentClient::connect_or_spawn().await?;
            crate::logs::put_log_key(agent, &tenant, &pair).await?;
            persist_log_key_provenance(&tenant, CredentialSource::Created)?;
            println!("created log key {name} ({api_key_id}) for tenant {tenant}");
            verify_stored_key(&tenant).await;
            Ok(())
        }
        KeyCommand::Show { tenant } => {
            let tenant = tenant_for(tenant)?;
            let agent = AgentClient::connect_or_spawn().await?;
            let pair = crate::logs::get_log_key(agent, &tenant).await?;
            println!("tenant: {tenant}");
            println!("api_key_id: {}", pair.api_key_id);
            println!("secret: set (hidden)");
            Ok(())
        }
        KeyCommand::Rm { tenant } => {
            let tenant = tenant_for(tenant)?;
            let agent = AgentClient::connect_or_spawn().await?;
            crate::logs::remove_log_key(agent, &tenant).await?;
            println!("removed log API key for tenant {tenant}");
            Ok(())
        }
    }
}

/// Keep the direct admin-session mutation behind the same typed permission as
/// daemon-backed writes. A rejected gate never polls `operation`.
async fn after_prod_confirmation<T, F>(
    permission: WriteOk<'_>,
    tenant: &str,
    operation: F,
) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    if permission.tenant != tenant {
        return Err(Error::Config(
            "production confirmation belongs to a different tenant".into(),
        ));
    }
    operation.await
}

fn persist_log_key_provenance(tenant: &str, source: CredentialSource) -> Result<()> {
    let mut cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let Some(entry) = cfg.tenants.iter_mut().find(|entry| entry.name == tenant) else {
        return Err(Error::Config(format!(
            "no tenant named '{tenant}' in config"
        )));
    };
    entry.provenance.log_key = Some(source);
    cfg.save()
}

fn configured_tenant_base_url(tenant_arg: Option<String>) -> Result<(String, String)> {
    let tenant = tenant_for(tenant_arg)?;
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let base_url = cfg
        .tenants
        .iter()
        .find(|configured| configured.name == tenant)
        .map(|configured| configured.base_url.clone())
        .ok_or_else(|| {
            Error::Config(format!(
                "no tenant named '{tenant}' in config; onboard it first"
            ))
        })?;
    Ok((tenant, base_url))
}

async fn verify_stored_key(tenant: &str) {
    let verification = async {
        let context = ops::fetch_context(Some(tenant.to_string())).await?;
        api::sources(&context.client, &context.base_url, &context.key).await
    }
    .await;
    match verification {
        Ok(_) => {
            println!("✓ key verified");
        }
        Err(error) => {
            eprintln!("⚠ key stored but verification FAILED for tenant {tenant}: {error}");
            eprintln!(
                "  The key id/secret may be wrong, or the tenant base URL may be unreachable."
            );
        }
    }
}

fn parse_sources(value: Option<&str>) -> Result<Vec<String>> {
    parse_sources_with_default(value, &ops::DEFAULT_SOURCES)
}

#[cfg(feature = "logs-store")]
fn parse_sync_sources(value: Option<&str>) -> Result<Vec<String>> {
    parse_sources_with_default(value, &ops::DEFAULT_SYNC_SOURCES)
}

fn parse_sources_with_default(value: Option<&str>, default: &[&str]) -> Result<Vec<String>> {
    let sources: Vec<String> = value.map_or_else(
        || default.iter().map(|source| source.to_string()).collect(),
        |csv| {
            csv.split(',')
                .map(str::trim)
                .filter(|source| !source.is_empty())
                .map(str::to_string)
                .collect()
        },
    );
    if sources.is_empty() {
        return Err(Error::Config(
            "--source must contain at least one source id".into(),
        ));
    }
    Ok(sources)
}

fn parse_time(value: &str, field: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            Error::Config(format!(
                "invalid {field} timestamp {value:?}; expected ISO-8601 such as \
                 2026-06-24T12:00:00Z"
            ))
        })
}

fn query_range(
    begin: Option<&str>,
    end: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let end = end.map(|value| parse_time(value, "end")).transpose()?;
    let begin = begin.map(|value| parse_time(value, "begin")).transpose()?;
    let end = end.unwrap_or(now);
    let begin = begin.unwrap_or(end - Duration::hours(24));
    if end <= begin {
        return Err(Error::Config(
            "log query end must be after begin".to_string(),
        ));
    }
    Ok((begin, end))
}

const DEFAULT_GREP_SINCE: &str = "15m";
const TAIL_INITIAL_LOOKBACK_SECONDS: i64 = 15;

fn parse_recent_duration(value: &str) -> Result<Duration> {
    let value = value.trim();
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    let amount = amount.parse::<i64>().map_err(|_| {
        Error::Config(format!(
            "invalid duration {value:?}; expected a positive integer with an s, m, or h suffix"
        ))
    })?;
    if amount <= 0 {
        return Err(Error::Config(format!(
            "invalid duration {value:?}; duration must be greater than zero"
        )));
    }
    let seconds = match unit {
        "s" => Some(amount),
        "m" => amount.checked_mul(60),
        "h" => amount.checked_mul(60 * 60),
        _ => None,
    }
    .ok_or_else(|| {
        Error::Config(format!(
            "invalid duration {value:?}; expected a positive integer with an s, m, or h suffix"
        ))
    })?;
    Duration::try_seconds(seconds)
        .ok_or_else(|| Error::Config(format!("invalid duration {value:?}; duration is too large")))
}

fn grep_range(
    since: Option<&str>,
    begin: Option<&str>,
    end: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    match (begin, end) {
        (Some(begin), Some(end)) => {
            let begin = parse_time(begin, "begin")?;
            let end = parse_time(end, "end")?;
            if end <= begin {
                return Err(Error::Config(
                    "log grep end must be after begin".to_string(),
                ));
            }
            Ok((begin, end))
        }
        (None, None) => {
            let duration = parse_recent_duration(since.unwrap_or(DEFAULT_GREP_SINCE))?;
            let begin = now.checked_sub_signed(duration).ok_or_else(|| {
                Error::Config("log grep duration is outside the supported timestamp range".into())
            })?;
            Ok((begin, now))
        }
        _ => Err(Error::Config(
            "log grep fixed ranges require both --begin and --end".to_string(),
        )),
    }
}

fn require_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty() {
        Err(Error::Config("log payload pattern cannot be empty".into()))
    } else {
        Ok(())
    }
}

fn normalized_payload_text(event: &Value) -> String {
    match event.get("payload") {
        Some(Value::String(payload)) => payload.clone(),
        Some(payload) => serde_json::to_string(payload).expect("serialize serde_json::Value"),
        None => String::new(),
    }
}

fn payload_matches(event: &Value, pattern: &str) -> bool {
    normalized_payload_text(event).contains(pattern)
}

fn event_timestamp(event: &Value) -> Option<DateTime<Utc>> {
    event
        .get("timestamp")?
        .as_str()?
        .parse::<DateTime<Utc>>()
        .ok()
}

fn sort_events_by_timestamp(events: &mut [Value]) {
    events.sort_by(
        |left, right| match (event_timestamp(left), event_timestamp(right)) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
    );
}

struct JsonArrayWriter<W> {
    writer: W,
    wrote_event: bool,
}

impl<W: Write> JsonArrayWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            wrote_event: false,
        }
    }

    fn write(&mut self, event: &Value) -> Result<()> {
        if self.wrote_event {
            self.writer.write_all(b",\n")?;
        } else {
            self.writer.write_all(b"[\n")?;
            self.wrote_event = true;
        }
        serde_json::to_writer(&mut self.writer, event)?;
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        if self.wrote_event {
            self.writer.write_all(b"\n]\n")?;
        } else {
            self.writer.write_all(b"[]\n")?;
        }
        self.writer.flush()?;
        Ok(())
    }
}

#[derive(Debug)]
struct TailCursor {
    next_begin: DateTime<Utc>,
}

impl TailCursor {
    fn new(next_begin: DateTime<Utc>) -> Self {
        Self { next_begin }
    }

    fn next_window(&mut self, end: DateTime<Utc>) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
        if end <= self.next_begin {
            return Err(Error::Config(
                "system clock did not advance while following logs".to_string(),
            ));
        }
        let begin = self.next_begin;
        self.next_begin = end;
        Ok((begin, end))
    }
}

async fn tail(
    context: &ops::FetchContext,
    sources: &[String],
    pattern: Option<&str>,
) -> Result<()> {
    let mut cursor = TailCursor::new(Utc::now() - Duration::seconds(TAIL_INITIAL_LOOKBACK_SECONDS));
    let mut stdout = BufWriter::new(std::io::stdout());
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    eprintln!(
        "following logs for tenant {} (initial {}s window; Ctrl-C to stop)",
        context.tenant, TAIL_INITIAL_LOOKBACK_SECONDS
    );

    loop {
        let (begin, end) = cursor.next_window(Utc::now())?;
        let mut seen = 0usize;
        let mut matched = 0usize;
        {
            let mut on_page = |mut page: Vec<Value>| -> Result<()> {
                seen += page.len();
                sort_events_by_timestamp(&mut page);
                for event in page {
                    if pattern.is_none_or(|pattern| payload_matches(&event, pattern)) {
                        serde_json::to_writer(&mut stdout, &event)?;
                        stdout.write_all(b"\n")?;
                        matched += 1;
                    }
                }
                stdout.flush()?;
                Ok(())
            };
            let fetch = api::fetch_range_streamed(
                &context.client,
                &context.base_url,
                &context.key,
                begin,
                end,
                sources,
                None,
                &mut on_page,
            );
            tokio::select! {
                signal = &mut interrupted => {
                    signal.map_err(Error::Io)?;
                    eprintln!("stopped following logs");
                    return Ok(());
                }
                result = fetch => result?,
            }
        }

        if seen == 0 {
            eprintln!("no new events");
        } else if matched == 0 {
            eprintln!("{seen} new events; none matched the payload pattern");
        }
    }
}

const ACCESS_OUTCOME: &str = "AM-ACCESS-OUTCOME";

fn payload_event_name(event: &Value) -> Option<&str> {
    event.get("payload")?.get("eventName")?.as_str()
}

fn has_access_outcome(events: &[Value]) -> bool {
    events
        .iter()
        .any(|event| payload_event_name(event) == Some(ACCESS_OUTCOME))
}

fn incomplete_tx_note(transaction_id: &str, waited_secs: Option<u64>) -> String {
    match waited_secs {
        Some(secs) => format!(
            "note: no {ACCESS_OUTCOME} event yet for transaction {transaction_id} after waiting {secs}s — logs can lag tens of seconds behind the request; retry, or pass --wait"
        ),
        None => format!(
            "note: no {ACCESS_OUTCOME} event yet for transaction {transaction_id} — logs can lag tens of seconds behind the request; retry, or pass --wait"
        ),
    }
}

/// After the first fetch, warn if `AM-ACCESS-OUTCOME` is missing, or (with
/// `--wait`) poll the same query until it appears or `timeout` elapses.
/// Timeout still returns the last payload. Polls go through
/// `api::fetch_transaction`, which is already rate-limited.
async fn maybe_wait_for_outcome<F, Fut>(
    transaction_id: &str,
    mut events: Vec<Value>,
    wait: bool,
    timeout: std::time::Duration,
    mut fetch: F,
) -> Result<Vec<Value>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<Value>>>,
{
    if has_access_outcome(&events) {
        return Ok(events);
    }
    if !wait {
        eprintln!("{}", incomplete_tx_note(transaction_id, None));
        return Ok(events);
    }

    let started = Instant::now();
    while started.elapsed() < timeout {
        eprintln!(
            "waiting for {ACCESS_OUTCOME}… ({}s elapsed, {} events so far)",
            started.elapsed().as_secs(),
            events.len()
        );
        events = fetch().await?;
        if has_access_outcome(&events) {
            return Ok(events);
        }
    }
    eprintln!(
        "{}",
        incomplete_tx_note(transaction_id, Some(started.elapsed().as_secs()))
    );
    Ok(events)
}

fn write_json<T: Serialize>(value: &T, output: Option<&Path>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if let Some(path) = output {
        std::fs::write(path, bytes)?;
    } else {
        std::io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}

fn prompt<T>(result: std::result::Result<T, InquireError>, field: &str) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            Err(Error::Config("log API key input canceled".into()))
        }
        Err(InquireError::NotTTY) => Err(Error::Config(format!(
            "no terminal available to prompt for {field}"
        ))),
        Err(error) => Err(Error::Config(format!("prompt for {field}: {error}"))),
    }
}

fn require_prompt(field: &str) -> Result<()> {
    if crate::cli::prompting_disabled() {
        Err(Error::Config(format!(
            "interactive prompt for {field} disabled by --no-prompt"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use clap::Parser;

    use super::*;

    #[test]
    fn key_create_requires_and_forwards_production_consent() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic",
            "logs",
            "key",
            "create",
            "--cookie-name",
            "example-cookie",
            "--yes",
        ])
        .unwrap();
        let Some(crate::cli::Command::Logs {
            command:
                LogsCommand::Key {
                    command: KeyCommand::Create { yes, .. },
                },
        }) = cli.command
        else {
            panic!("expected logs key create");
        };

        let ok =
            crate::cli::prod_write_ok(crate::config::TenantTheme::Production, "prod", yes).unwrap();
        assert!(ok.confirmed_prod);
        assert!(
            crate::cli::prod_write_ok(crate::config::TenantTheme::Production, "prod", false,)
                .is_err()
        );
    }

    #[tokio::test]
    async fn confirmed_log_key_creation_reaches_the_remote_mint() {
        let called = std::cell::Cell::new(false);
        assert!(
            crate::cli::prod_write_ok(crate::config::TenantTheme::Production, "prod", false)
                .is_err()
        );
        assert!(!called.get());

        let permission =
            crate::cli::prod_write_ok(crate::config::TenantTheme::Production, "prod", true)
                .unwrap();
        let result = after_prod_confirmation(permission, "prod", async {
            called.set(true);
            Ok::<_, Error>("minted")
        })
        .await
        .unwrap();

        assert_eq!(result, "minted");
        assert!(called.get());
    }

    #[test]
    fn default_sources_are_the_two_everything_rollups() {
        assert_eq!(
            parse_sources(None).unwrap(),
            vec!["am-everything".to_string(), "idm-everything".to_string()]
        );
    }

    #[cfg(feature = "logs-store")]
    #[test]
    fn sync_default_sources_are_curated_for_signal() {
        assert_eq!(
            parse_sync_sources(None).unwrap(),
            vec![
                "am-authentication".to_string(),
                "am-access".to_string(),
                "am-activity".to_string(),
                "idm-activity".to_string(),
                "idm-config".to_string(),
                "idm-access".to_string(),
            ]
        );
    }

    #[test]
    fn source_override_trims_and_joins_cleanly() {
        let sources = parse_sources(Some("am-access, idm-core")).unwrap();
        assert_eq!(api::source_param(&sources).unwrap(), "am-access,idm-core");
    }

    #[cfg(feature = "logs-store")]
    #[test]
    fn sync_source_override_accepts_core_and_everything_sources() {
        let sources = parse_sync_sources(Some("idm-core, am-everything")).unwrap();
        assert_eq!(
            api::source_param(&sources).unwrap(),
            "idm-core,am-everything"
        );
    }

    #[test]
    fn query_defaults_to_the_previous_twenty_four_hours() {
        let now = Utc.with_ymd_and_hms(2026, 6, 24, 12, 0, 0).unwrap();
        assert_eq!(
            query_range(None, None, now).unwrap(),
            (now - Duration::hours(24), now)
        );
    }

    #[test]
    fn recent_durations_accept_seconds_minutes_and_hours() {
        assert_eq!(parse_recent_duration("30s").unwrap(), Duration::seconds(30));
        assert_eq!(parse_recent_duration("5m").unwrap(), Duration::minutes(5));
        assert_eq!(parse_recent_duration("1h").unwrap(), Duration::hours(1));
    }

    #[test]
    fn recent_durations_reject_zero_missing_and_unknown_units() {
        for invalid in ["0s", "5", "1d", "soon", "", "9223372036854775807s"] {
            assert!(
                parse_recent_duration(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn grep_defaults_to_the_previous_fifteen_minutes() {
        let now = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        assert_eq!(
            grep_range(None, None, None, now).unwrap(),
            (now - Duration::minutes(15), now)
        );
        assert_eq!(
            grep_range(Some("30s"), None, None, now).unwrap(),
            (now - Duration::seconds(30), now)
        );
    }

    #[test]
    fn grep_fixed_range_requires_both_bounds_and_orders_them() {
        let now = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        assert!(grep_range(None, Some("2026-09-10T11:00:00Z"), None, now).is_err());
        assert!(
            grep_range(
                None,
                Some("2026-09-10T12:00:00Z"),
                Some("2026-09-10T11:00:00Z"),
                now,
            )
            .is_err()
        );
        assert_eq!(
            grep_range(
                None,
                Some("2026-09-10T10:00:00Z"),
                Some("2026-09-10T11:00:00Z"),
                now,
            )
            .unwrap(),
            (
                Utc.with_ymd_and_hms(2026, 9, 10, 10, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 9, 10, 11, 0, 0).unwrap(),
            )
        );
    }

    #[test]
    fn payload_matching_normalizes_strings_and_objects() {
        let raw = serde_json::json!({"payload": "raw needle text"});
        let structured = serde_json::json!({"payload": {"message": "object needle text"}});
        let absent = serde_json::json!({"source": "am-core"});

        assert_eq!(normalized_payload_text(&raw), "raw needle text");
        assert_eq!(
            normalized_payload_text(&structured),
            r#"{"message":"object needle text"}"#
        );
        assert!(payload_matches(&raw, "needle"));
        assert!(payload_matches(&structured, "needle"));
        assert!(!payload_matches(&absent, "needle"));
    }

    #[test]
    fn events_are_sorted_by_top_level_timestamp_with_malformed_values_last() {
        let mut events = vec![
            serde_json::json!({"timestamp": "2026-09-10T12:00:02Z", "n": 2}),
            serde_json::json!({"source": "missing-timestamp"}),
            serde_json::json!({"timestamp": "2026-09-10T12:00:01Z", "n": 1}),
        ];

        sort_events_by_timestamp(&mut events);

        assert_eq!(events[0]["n"], 1);
        assert_eq!(events[1]["n"], 2);
        assert_eq!(events[2]["source"], "missing-timestamp");
    }

    #[test]
    fn consecutive_tail_windows_share_exactly_one_boundary() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        let t1 = t0 + Duration::seconds(1);
        let t2 = t1 + Duration::seconds(2);
        let mut cursor = TailCursor::new(t0);

        assert_eq!(cursor.next_window(t1).unwrap(), (t0, t1));
        assert_eq!(cursor.next_window(t2).unwrap(), (t1, t2));
    }

    #[test]
    fn json_array_writer_streams_valid_empty_and_populated_arrays() {
        let mut populated = Vec::new();
        {
            let mut writer = JsonArrayWriter::new(&mut populated);
            writer.write(&serde_json::json!({"n": 1})).unwrap();
            writer.write(&serde_json::json!({"n": 2})).unwrap();
            writer.finish().unwrap();
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&populated).unwrap(),
            serde_json::json!([{"n": 1}, {"n": 2}])
        );

        let mut empty = Vec::new();
        JsonArrayWriter::new(&mut empty).finish().unwrap();
        assert_eq!(empty, b"[]\n");
    }

    #[test]
    fn grep_and_tail_flags_parse_in_default_builds() {
        let grep = crate::cli::Cli::try_parse_from([
            "aic", "logs", "grep", "failure", "--since", "5m", "--source", "am-core",
        ])
        .unwrap();
        match grep.command {
            Some(crate::cli::Command::Logs {
                command: LogsCommand::Grep { pattern, since, .. },
            }) => {
                assert_eq!(pattern, "failure");
                assert_eq!(since.as_deref(), Some("5m"));
            }
            other => panic!("unexpected {other:?}"),
        }

        let tail =
            crate::cli::Cli::try_parse_from(["aic", "logs", "tail", "--pattern", "Exception"])
                .unwrap();
        assert!(matches!(
            tail.command,
            Some(crate::cli::Command::Logs {
                command: LogsCommand::Tail { pattern: Some(pattern), .. },
            }) if pattern == "Exception"
        ));
    }

    #[test]
    fn grep_since_and_fixed_range_flags_conflict() {
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "logs",
                "grep",
                "failure",
                "--since",
                "5m",
                "--begin",
                "2026-09-10T10:00:00Z",
                "--end",
                "2026-09-10T11:00:00Z",
            ])
            .is_err()
        );
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "logs",
                "grep",
                "failure",
                "--begin",
                "2026-09-10T10:00:00Z",
            ])
            .is_err()
        );
    }

    fn event_with_name(name: &str) -> Value {
        serde_json::json!({ "payload": { "eventName": name } })
    }

    #[test]
    fn access_outcome_is_the_completeness_marker() {
        assert!(!has_access_outcome(&[]));
        assert!(!has_access_outcome(&[event_with_name("AM-ACCESS-ATTEMPT")]));
        assert!(!has_access_outcome(&[serde_json::json!({
            "payload": "raw string, no eventName"
        })]));
        assert!(!has_access_outcome(&[serde_json::json!({
            "eventName": "AM-ACCESS-OUTCOME"
        })]));
        assert!(has_access_outcome(&[
            event_with_name("AM-ACCESS-ATTEMPT"),
            event_with_name("AM-ACCESS-OUTCOME"),
        ]));
    }

    #[test]
    fn incomplete_note_names_the_transaction_and_optional_wait() {
        assert_eq!(
            incomplete_tx_note("abc-1", None),
            "note: no AM-ACCESS-OUTCOME event yet for transaction abc-1 — logs can lag tens of seconds behind the request; retry, or pass --wait"
        );
        assert_eq!(
            incomplete_tx_note("abc-1", Some(90)),
            "note: no AM-ACCESS-OUTCOME event yet for transaction abc-1 after waiting 90s — logs can lag tens of seconds behind the request; retry, or pass --wait"
        );
    }

    #[test]
    fn tx_wait_parses_with_default_timeout() {
        let cli = crate::cli::Cli::try_parse_from(["aic", "logs", "tx", "abc", "--wait"]).unwrap();
        match cli.command {
            Some(crate::cli::Command::Logs {
                command:
                    LogsCommand::Tx {
                        transaction_id,
                        wait,
                        timeout,
                        ..
                    },
            }) => {
                assert_eq!(transaction_id, "abc");
                assert!(wait);
                assert_eq!(timeout, 60);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tx_timeout_is_accepted_without_wait() {
        let cli = crate::cli::Cli::try_parse_from(["aic", "logs", "tx", "abc", "--timeout", "15"])
            .unwrap();
        match cli.command {
            Some(crate::cli::Command::Logs {
                command: LogsCommand::Tx { wait, timeout, .. },
            }) => {
                assert!(!wait);
                assert_eq!(timeout, 15);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn range_and_query_reject_wait() {
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "logs",
                "range",
                "2026-01-01T00:00:00Z",
                "2026-01-01T01:00:00Z",
                "--wait",
            ])
            .is_err()
        );
        assert!(
            crate::cli::Cli::try_parse_from(["aic", "logs", "query", "true", "--wait"]).is_err()
        );
    }

    #[tokio::test]
    async fn wait_does_not_poll_when_outcome_already_present() {
        let first = vec![event_with_name("AM-ACCESS-OUTCOME")];
        let result = maybe_wait_for_outcome(
            "tx-1",
            first.clone(),
            true,
            std::time::Duration::from_secs(60),
            || async { panic!("must not poll once AM-ACCESS-OUTCOME is present") },
        )
        .await
        .unwrap();
        assert_eq!(result, first);
    }

    #[tokio::test]
    async fn missing_outcome_without_wait_does_not_poll() {
        let first = vec![event_with_name("AM-ACCESS-ATTEMPT")];
        let result = maybe_wait_for_outcome(
            "tx-1",
            first.clone(),
            false,
            std::time::Duration::from_secs(60),
            || async { panic!("must not poll without --wait") },
        )
        .await
        .unwrap();
        assert_eq!(result, first);
        assert!(!has_access_outcome(&result));
    }

    #[tokio::test]
    async fn wait_stops_when_outcome_arrives() {
        let first = vec![event_with_name("AM-ACCESS-ATTEMPT")];
        let mut calls = 0u32;
        let result = maybe_wait_for_outcome(
            "tx-1",
            first,
            true,
            std::time::Duration::from_secs(5),
            || {
                calls += 1;
                let n = calls;
                async move {
                    if n >= 2 {
                        Ok(vec![
                            event_with_name("AM-ACCESS-ATTEMPT"),
                            event_with_name("AM-ACCESS-OUTCOME"),
                        ])
                    } else {
                        Ok(vec![event_with_name("AM-ACCESS-ATTEMPT")])
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(calls, 2);
        assert!(has_access_outcome(&result));
    }

    #[tokio::test]
    async fn wait_timeout_returns_partial_events() {
        let first = vec![event_with_name("AM-ACCESS-ATTEMPT")];
        let result = maybe_wait_for_outcome(
            "tx-missing",
            first.clone(),
            true,
            std::time::Duration::ZERO,
            || async { panic!("should not poll when timeout is already elapsed") },
        )
        .await
        .unwrap();
        assert_eq!(result, first);
        assert!(!has_access_outcome(&result));
    }
}
