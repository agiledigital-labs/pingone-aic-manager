//! `aic logs` parser and command implementation.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use clap::Subcommand;
use inquire::{Password, PasswordDisplayMode, Text, error::InquireError};
use serde::Serialize;

use crate::agent::AgentClient;
use crate::cli::tenant_for;
use crate::logs::{api, ops};
use crate::{Error, Result};

const DEFAULT_SOURCES: [&str; 2] = ["am-everything", "idm-everything"];

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
            agent
                .put_log_key(&tenant, api_key_id.clone(), api_key_secret)
                .await?;
            println!("stored log API key {api_key_id} for tenant {tenant}");

            let verification = async {
                let context = ops::fetch_context(Some(tenant.clone())).await?;
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
            Ok(())
        }
        KeyCommand::Show { tenant } => {
            let tenant = tenant_for(tenant)?;
            crate::cli::ensure_agent_unlocked().await?;
            let agent = AgentClient::connect_or_spawn().await?;
            let pair = agent.get_log_key(&tenant).await?;
            println!("tenant: {tenant}");
            println!("api_key_id: {}", pair.api_key_id);
            println!("secret: set (hidden)");
            Ok(())
        }
        KeyCommand::Rm { tenant } => {
            let tenant = tenant_for(tenant)?;
            crate::cli::ensure_agent_unlocked().await?;
            let agent = AgentClient::connect_or_spawn().await?;
            agent.remove_log_key(&tenant).await?;
            println!("removed log API key for tenant {tenant}");
            Ok(())
        }
    }
}

fn parse_sources(value: Option<&str>) -> Result<Vec<String>> {
    let sources: Vec<String> = value.map_or_else(
        || {
            DEFAULT_SOURCES
                .iter()
                .map(|source| source.to_string())
                .collect()
        },
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
    fn source_override_trims_and_joins_cleanly() {
        let sources = parse_sources(Some("am-access, idm-core")).unwrap();
        assert_eq!(api::source_param(&sources).unwrap(), "am-access,idm-core");
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
