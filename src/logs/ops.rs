//! Shared CLI orchestration for log fetches and local sync.

use std::fs;

use chrono::{DateTime, Duration, Utc};
use reqwest::Client;

use crate::Result;
use crate::agent::AgentClient;
use crate::cli::tenant_config_for;
use crate::config::LogKeyPair;
use crate::logs::{api, db, state};

pub(crate) const DEFAULT_SOURCES: [&str; 2] = ["am-everything", "idm-everything"];
pub(crate) const DEFAULT_SYNC_SOURCES: [&str; 6] = [
    "am-authentication",
    "am-access",
    "am-activity",
    "idm-activity",
    "idm-config",
    "idm-access",
];
const SYNC_OVERLAP: Duration = Duration::minutes(5);
const RETENTION: Duration = Duration::days(30);

pub struct FetchContext {
    pub tenant: String,
    pub client: Client,
    pub base_url: String,
    pub key: LogKeyPair,
}

pub async fn fetch_context(tenant: Option<String>) -> Result<FetchContext> {
    let tenant = tenant_config_for(tenant)?;
    let agent = AgentClient::connect_or_spawn().await?;
    let key = agent.get_log_key(&tenant.name).await?;
    let client = Client::builder().build()?;
    Ok(FetchContext {
        tenant: tenant.name,
        client,
        base_url: tenant.base_url,
        key,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSyncReport {
    pub source: String,
    pub fetched: usize,
    pub filtered: usize,
    pub inserted: usize,
}

pub async fn sync_tenant(
    tenant: Option<String>,
    sources: &[String],
    since: Option<DateTime<Utc>>,
) -> Result<Vec<SourceSyncReport>> {
    let context = fetch_context(tenant).await?;
    fs::create_dir_all(state::store_dir())?;
    let mut conn = db::open(state::store_path(&context.tenant))?;
    let now = Utc::now();
    let sources = if sources.is_empty() {
        DEFAULT_SYNC_SOURCES
            .iter()
            .map(|source| source.to_string())
            .collect()
    } else {
        sources.to_vec()
    };
    let mut reports = Vec::with_capacity(sources.len());

    for source in sources {
        let start = match since {
            Some(since) => since,
            None => db::get_sync_state(&conn, &source)?
                .map(|last_end| last_end - SYNC_OVERLAP)
                .unwrap_or(now - RETENTION),
        };
        let mut fetched = 0;
        let mut filtered = 0;
        let mut inserted = 0;
        {
            let mut on_page = |mut page: Vec<serde_json::Value>| -> Result<()> {
                fetched += page.len();
                let unfiltered = page.len();
                page.retain(|event| !is_core_noise(event));
                filtered += unfiltered - page.len();
                inserted += db::insert_events(&mut conn, &page)?;
                Ok(())
            };
            api::fetch_range_streamed(
                &context.client,
                &context.base_url,
                &context.key,
                start,
                now,
                std::slice::from_ref(&source),
                None,
                &mut on_page,
            )
            .await?;
        }
        db::set_sync_state(&conn, &source, now)?;
        reports.push(SourceSyncReport {
            source,
            fetched,
            filtered,
            inserted,
        });
    }

    Ok(reports)
}

pub(crate) fn is_core_noise(event: &serde_json::Value) -> bool {
    let Some(source) = event.get("source").and_then(serde_json::Value::as_str) else {
        return false;
    };
    if !source.ends_with("-core") {
        return false;
    }

    let Some(payload) = event.get("payload").and_then(serde_json::Value::as_str) else {
        return false;
    };
    !(payload.contains("WARN") || payload.contains("ERROR"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn core_string_without_warning_or_error_is_noise() {
        assert!(is_core_noise(&json!({
            "source": "idm-core",
            "payload": "FINE org.apache.felix.hc.core.impl"
        })));
    }

    #[test]
    fn core_string_error_is_kept() {
        assert!(!is_core_noise(&json!({
            "source": "idm-core",
            "payload": "ERROR org.forgerock.openidm"
        })));
    }

    #[test]
    fn core_object_payload_is_kept() {
        assert!(!is_core_noise(&json!({
            "source": "am-core",
            "payload": {
                "level": "FINE",
                "message": "structured payload"
            }
        })));
    }

    #[test]
    fn non_core_string_payload_is_kept() {
        assert!(!is_core_noise(&json!({
            "source": "idm-access",
            "payload": "FINE access trace"
        })));
    }
}
