pub mod api;
pub mod auth;
pub mod esv;
pub mod onboard;

use std::sync::{Arc, Mutex};

use auth::TokenCache;

use crate::config::tenant::{Tenant, TenantTheme};
use crate::{Error, Result};

/// Internal-only: the in-process AIC HTTP client used by the agent daemon.
/// Frontends (TUI + CLI) **must** go through `aic::api` / `aic::esv` so
/// every tenant call lands on the daemon's token cache + connection pool;
/// `pub(crate)` is the type-level guard rail.
#[derive(Clone)]
pub(crate) struct AicClient {
    pub tenant: Tenant,
    http: reqwest::Client,
    pub token_cache: Arc<Mutex<TokenCache>>,
    /// The private JWK used to mint tokens.
    jwk: serde_json::Value,
}

impl AicClient {
    pub fn new(tenant: Tenant, jwk: serde_json::Value) -> Self {
        let http = reqwest::Client::builder()
            .build()
            .expect("failed to build reqwest client");
        Self {
            tenant,
            http,
            token_cache: Arc::new(Mutex::new(TokenCache::new())),
            jwk,
        }
    }

    pub async fn bearer(&self) -> Result<String> {
        // Check cache first
        {
            let cache = self.token_cache.lock().unwrap();
            if let Some(t) = cache.get_valid() {
                return Ok(t.to_string());
            }
        }
        // Mint a fresh token
        let (token, expires_at) = auth::mint_token(&self.http, &self.tenant, &self.jwk).await?;
        {
            let mut cache = self.token_cache.lock().unwrap();
            cache.store(token.clone(), expires_at);
        }
        Ok(token)
    }

    pub async fn get(&self, path: &str) -> Result<serde_json::Value> {
        let token = self.bearer().await?;
        let url = format!("{}{path}", self.tenant.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-API-Version", "resource=1.0")
            .header("Accept", "application/json")
            .send()
            .await?;
        self.check_response(resp).await
    }

    /// Write method — checks prod confirmation for prod-themed tenants.
    pub async fn write(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            return Err(Error::ProdConfirmRequired);
        }
        let token = self.bearer().await?;
        let url = format!("{}{path}", self.tenant.base_url);
        let resp = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-API-Version", "resource=1.0")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        self.check_response(resp).await
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<serde_json::Value> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp.json().await?)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(Error::Api {
                status: status.as_u16(),
                body,
            })
        }
    }

}
