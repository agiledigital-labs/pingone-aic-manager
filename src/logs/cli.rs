//! `aic logs` parser and command implementation.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use clap::Subcommand;
use inquire::{Password, PasswordDisplayMode, Text, error::InquireError};
use serde::Serialize;

use crate::agent::AgentClient;
use crate::cli::tenant_for;
use crate::config::ProjectConfig;
use crate::logs::{api, db, ops, state};
use crate::onboard::bootstrap::{
    create_log_api_key, credential_name, no_redirect_client, resolve_admin_username,
    session_to_bearer,
};
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
                for source in sources {
                    println!("{source}");
                }
                Ok(())
            }
        }
        LogsCommand::Tx {
            transaction_id,
            source,
            output,
            tenant,
        } => {
            let sources = parse_sources(source.as_deref())?;
            let context = ops::fetch_context(tenant).await?;
            let result = api::fetch_transaction(
                &context.client,
                &context.base_url,
                &context.key,
                &transaction_id,
                &sources,
            )
            .await?;
            write_json(&result, output.as_deref())
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
            let path = state::store_path(&tenant);
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
        LogsCommand::Compact {
            tenant,
            retain_months,
        } => {
            let report = ops::compact_tenant(tenant, retain_months).await?;
            println!(
                "rolled up {} attempts across {} journeys; pruned {} raw events",
                report.attempts_upserted, report.journeys, report.events_pruned
            );
            Ok(())
        }
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
            crate::cli::ensure_agent_unlocked().await?;
            let api_key_id = match id {
                Some(id) => id,
                None => prompt(Text::new("Log API key id").prompt(), "log API key id")?,
            };
            let api_key_id = api_key_id.trim().to_string();
            if api_key_id.is_empty() {
                return Err(Error::Config("log API key id cannot be empty".into()));
            }

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
            println!("stored log API key {api_key_id} for tenant {tenant}");
            verify_stored_key(&tenant).await;
            Ok(())
        }
        KeyCommand::Create {
            tenant,
            cookie_name,
        } => {
            let (tenant, base_url) = configured_tenant_base_url(tenant)?;
            crate::cli::ensure_agent_unlocked().await?;

            let cookie_name = match cookie_name {
                Some(cookie_name) => cookie_name,
                None => prompt(
                    Text::new("AM session cookie name").prompt(),
                    "AM session cookie name",
                )?,
            };
            let cookie_name = cookie_name.trim().to_string();
            if cookie_name.is_empty() {
                return Err(Error::Config(
                    "AM session cookie name cannot be empty".into(),
                ));
            }

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
            let bearer = session_to_bearer(&client, &base_url, &cookie_name, &cookie_value).await?;
            let username = resolve_admin_username(&client, &base_url, &bearer).await;
            let name = credential_name(username.as_deref(), &tenant);
            let pair = create_log_api_key(&client, &base_url, &bearer, &name).await?;
            let api_key_id = pair.api_key_id.clone();

            let agent = AgentClient::connect_or_spawn().await?;
            crate::logs::put_log_key(agent, &tenant, &pair).await?;
            println!("created log key {name} ({api_key_id}) for tenant {tenant}");
            verify_stored_key(&tenant).await;
            Ok(())
        }
        KeyCommand::Show { tenant } => {
            let tenant = tenant_for(tenant)?;
            crate::cli::ensure_agent_unlocked().await?;
            let agent = AgentClient::connect_or_spawn().await?;
            let pair = crate::logs::get_log_key(agent, &tenant).await?;
            println!("tenant: {tenant}");
            println!("api_key_id: {}", pair.api_key_id);
            println!("secret: set (hidden)");
            Ok(())
        }
        KeyCommand::Rm { tenant } => {
            let tenant = tenant_for(tenant)?;
            crate::cli::ensure_agent_unlocked().await?;
            let agent = AgentClient::connect_or_spawn().await?;
            crate::logs::remove_log_key(agent, &tenant).await?;
            println!("removed log API key for tenant {tenant}");
            Ok(())
        }
    }
}

fn configured_tenant_base_url(tenant_arg: Option<String>) -> Result<(String, String)> {
    let tenant = tenant_for(tenant_arg)?;
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn default_sources_are_the_two_everything_rollups() {
        assert_eq!(
            parse_sources(None).unwrap(),
            vec!["am-everything".to_string(), "idm-everything".to_string()]
        );
    }

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
}
