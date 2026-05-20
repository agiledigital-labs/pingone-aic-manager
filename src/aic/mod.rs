pub mod auth;
pub mod onboard;
pub mod svcacct;

use std::sync::{Arc, Mutex};

use auth::TokenCache;

use crate::config::tenant::{Tenant, TenantTheme};
use crate::{Error, Result};

pub struct AicClient {
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

    async fn bearer(&self) -> Result<String> {
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

    pub async fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        self.write(reqwest::Method::POST, path, body, false).await
    }

    /// Write method — checks prod confirmation for prod-themed tenants.
    pub async fn write(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Prod && !confirmed_prod {
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

    pub async fn put(&self, path: &str, body: serde_json::Value, confirmed_prod: bool) -> Result<serde_json::Value> {
        self.write(reqwest::Method::PUT, path, body, confirmed_prod).await
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<serde_json::Value> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp.json().await?)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(Error::Api { status: status.as_u16(), body })
        }
    }

    /// Mint a token in a background task and report the result via AppEvent.
    pub fn spawn_mint_token(
        tenant: Tenant,
        jwk: serde_json::Value,
        token_cache: Arc<Mutex<TokenCache>>,
        tx: tokio::sync::mpsc::UnboundedSender<crate::event::AppEvent>,
    ) {
        use crate::event::AppEvent;
        let http = reqwest::Client::new();
        let name = tenant.name.clone();
        tokio::spawn(async move {
            match auth::mint_token(&http, &tenant, &jwk).await {
                Ok((token, expires_at)) => {
                    {
                        let mut cache = token_cache.lock().unwrap();
                        cache.store(token, expires_at);
                    }
                    let _ = tx.send(AppEvent::TokenMinted { tenant: name, expires_at });
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!(tenant = %name, error = %msg, "token mint failed");
                    let _ = tx.send(AppEvent::TokenError { tenant: name, error: msg });
                }
            }
        });
    }
}
