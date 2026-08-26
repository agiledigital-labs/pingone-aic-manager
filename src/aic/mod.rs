pub mod api;
pub mod auth;
use std::sync::{Arc, Mutex};

use auth::TokenCache;

use crate::config::tenant::{Tenant, TenantTheme};
use crate::{Error, Result};

/// Internal-only: the in-process AIC HTTP client used by the agent daemon.
/// Frontends (TUI + CLI) normally go through `aic::api` / feature API modules so
/// tenant administration calls land on the daemon's token cache + connection
/// pool. The narrow exception is the bearer-free user-token exchange in
/// `jwtbearer::api`: it constructs this transport with no JWK so a client's own
/// Basic credential can be sent without making the service-account bearer
/// available to the request path. `pub(crate)` remains the type-level guard
/// rail against use outside the crate.
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
        // A connect timeout, because callers are not free to cancel: `script
        // watch` treats a tenant write plus its snapshot update as one
        // uncancellable step — dropping the future cannot retract a PUT the
        // server already accepted — so the transport is the only thing that can
        // bound a hang, and reqwest defaults to waiting forever.
        //
        // NOT a response timeout. This client is shared with calls whose
        // legitimate duration the caller chooses: `aic sync recon --wait
        // --timeout 10m` is one synchronous POST holding the connection open
        // for `waitForCompletion=true`, and any global cap would abort it. The
        // fix is a per-request timeout, which needs `ApiCallRequest` to carry
        // one and every `AicClient` verb to forward it; until then a hung
        // *response* is still unbounded, and the connect timeout only catches
        // the common case of a host that never answers.
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
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
        if_match: Option<&str>,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            return Err(Error::ProdConfirmRequired);
        }
        let token = self.bearer().await?;
        let resp = self
            .json_request(method, path, body, &token, api_version, if_match)
            .send()
            .await?;
        self.check_response(resp).await
    }

    fn json_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
        token: &str,
        api_version: Option<&str>,
        if_match: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let request = self
            .http
            .request(method, self.url(path))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-API-Version", api_version.unwrap_or("resource=1.0"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body);
        match if_match {
            Some(revision) => request.header("If-Match", revision),
            None => request,
        }
    }

    pub async fn write_form(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &str,
        confirmed_prod: bool,
    ) -> Result<serde_json::Value> {
        self.write_form_with_authorization(method, path, body, confirmed_prod, None)
            .await
    }

    /// Send a form body with an optional caller-owned Authorization value.
    ///
    /// This never consults the service-account token cache. The only accepted
    /// credential is the value explicitly supplied by the token-exchange
    /// caller, such as an OAuth2 client's Basic credential.
    pub async fn write_form_with_authorization(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &str,
        confirmed_prod: bool,
        authorization: Option<&str>,
    ) -> Result<serde_json::Value> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            return Err(Error::ProdConfirmRequired);
        }
        let resp = self
            .form_request(method, path, body, authorization)
            .send()
            .await?;
        self.check_response(resp).await
    }

    fn form_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &str,
        authorization: Option<&str>,
    ) -> reqwest::RequestBuilder {
        // OAuth2 token exchanges may authenticate with a client-owned
        // credential passed explicitly here. Sending the service-account
        // bearer would expose a more powerful tenant credential to an endpoint
        // that neither needs nor accepts it.
        let url = self.url(path);
        let request = self
            .http
            .request(method, &url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(body.to_string());
        match authorization {
            Some(authorization) => request.header("Authorization", authorization),
            None => request,
        }
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
            provenance: crate::config::Provenance::default(),
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
            .form_request(reqwest::Method::POST, "/token", "grant_type=example", None)
            .build()
            .unwrap();

        assert!(request.headers().get("authorization").is_none());
        assert!(request.headers().get("accept-api-version").is_none());
    }

    #[test]
    fn form_request_carries_explicit_basic_without_a_bearer() {
        let client = AicClient::new(
            tenant("https://tenant.example".into()),
            serde_json::Value::Null,
        );
        let request = client
            .form_request(
                reqwest::Method::POST,
                "/token",
                "grant_type=example",
                Some("Basic Y2xpZW50OnNlY3JldA=="),
            )
            .build()
            .unwrap();

        assert_eq!(
            request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Basic Y2xpZW50OnNlY3JldA=="
        );
        assert!(
            !request.headers()["authorization"]
                .to_str()
                .unwrap()
                .starts_with("Bearer ")
        );
        assert!(request.headers().get("accept-api-version").is_none());
    }

    #[test]
    fn json_request_only_sends_if_match_when_supplied() {
        let client = AicClient::new(
            tenant("https://tenant.example".into()),
            serde_json::Value::Null,
        );
        let plain = client
            .json_request(
                reqwest::Method::PUT,
                "/resource",
                serde_json::json!({}),
                "token",
                Some("resource=1.0"),
                None,
            )
            .build()
            .unwrap();
        let conditional = client
            .json_request(
                reqwest::Method::PUT,
                "/resource",
                serde_json::json!({}),
                "token",
                Some("resource=1.0"),
                Some("revision-1"),
            )
            .build()
            .unwrap();

        assert!(plain.headers().get("if-match").is_none());
        assert_eq!(conditional.headers()["if-match"], "revision-1");
    }
}
