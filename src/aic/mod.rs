pub mod api;
pub mod auth;
use std::sync::{Arc, Mutex};

use auth::TokenCache;

use crate::config::tenant::{Tenant, TenantTheme};
use crate::{Error, Result};

/// Internal-only: the in-process AIC HTTP client used by the agent daemon.
/// Frontends (TUI + CLI) **must** go through `aic::api` / feature API modules so
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

    pub async fn get(&self, path: &str, api_version: Option<&str>) -> Result<serde_json::Value> {
        let token = self.bearer().await?;
        let url = self.url(path);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-API-Version", api_version.unwrap_or("resource=1.0"))
            .header("Accept", "application/json")
            .send()
            .await?;
        self.check_response(resp).await
    }

    /// Write method — checks prod confirmation for prod-themed tenants.
    /// `api_version` overrides the `Accept-API-Version` header (default
    /// `resource=1.0`); AM scripts pass `protocol=2.0,resource=1.0`.
    pub async fn write(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
        confirmed_prod: bool,
        api_version: Option<&str>,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            return Err(Error::ProdConfirmRequired);
        }
        let token = self.bearer().await?;
        let url = self.url(path);
        let resp = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-API-Version", api_version.unwrap_or("resource=1.0"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        self.check_response(resp).await
    }

    pub async fn write_form(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &str,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            return Err(Error::ProdConfirmRequired);
        }
        let resp = self.form_request(method, path, body).send().await?;
        self.check_response(resp).await
    }

    fn form_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &str,
    ) -> reqwest::RequestBuilder {
        // OAuth2 token exchanges authenticate with the form's client
        // credentials and assertion. Sending the service-account bearer here
        // would expose a tenant credential to an endpoint that does not need it.
        let url = self.url(path);
        self.http
            .request(method, &url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(body.to_string())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.tenant.base_url)
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<serde_json::Value> {
        let status = resp.status();
        if status.is_success() {
            // Some AIC write actions return `200` with an **empty body** —
            // verified for secret `setDescription` (2026-05-31), which the docs
            // wrongly claimed echoes the object. Treat an empty success body as
            // JSON null instead of failing to decode.
            let bytes = resp.bytes().await?;
            if bytes.is_empty() {
                return Ok(serde_json::Value::Null);
            }
            Ok(serde_json::from_slice(&bytes)?)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(Error::Api {
                status: status.as_u16(),
                body,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(base_url: String) -> Tenant {
        Tenant {
            name: "sandbox".into(),
            base_url,
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
        }
    }

    #[test]
    fn url_always_prefixes_the_tenant_base_url() {
        let client = AicClient::new(
            tenant("https://tenant.example".into()),
            serde_json::Value::Null,
        );

        // The property that matters is confinement, not the exact string an
        // absolute path degrades into: whatever a caller passes, the request
        // must still be addressed at this tenant. The daemon holds decrypted
        // keys, so a path that could name its own host would let a tenant
        // credential be sent anywhere.
        let mangled = client.url("https://attacker.example/token");
        assert!(mangled.starts_with("https://tenant.example"));
        assert_eq!(
            url::Url::parse(&mangled).unwrap().host_str(),
            Some("tenant.examplehttps")
        );

        assert_eq!(client.url("/am/json/x"), "https://tenant.example/am/json/x");
    }

    #[test]
    fn form_request_sends_no_bearer_or_api_version_header() {
        let client = AicClient::new(
            tenant("https://tenant.example".into()),
            serde_json::Value::Null,
        );
        let request = client
            .form_request(reqwest::Method::POST, "/token", "grant_type=example")
            .build()
            .unwrap();

        assert!(request.headers().get("authorization").is_none());
        assert!(request.headers().get("accept-api-version").is_none());
    }
}
