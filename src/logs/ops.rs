//! Shared CLI orchestration for log fetches.

use reqwest::Client;

use crate::Result;
use crate::agent::AgentClient;
use crate::cli::tenant_config_for;
use crate::config::LogKeyPair;

pub struct FetchContext {
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
        client,
        base_url: tenant.base_url,
        key,
    })
}
