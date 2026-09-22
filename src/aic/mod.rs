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
    /// The same transport with redirects switched off, for the one endpoint
    /// that sends no credential — see [`Self::get_text_unauthenticated`].
    http_no_redirect: reqwest::Client,
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
        let builder =
            || crate::http::client_builder().connect_timeout(std::time::Duration::from_secs(10));
        let http = builder().build().expect("failed to build reqwest client");
        // Confinement to the tenant is the point of the unauthenticated
        // transport, and `url()` only confines the request we *send*. reqwest
        // follows up to ten redirects by default, so a 302 could hand the
        // SAML metadata classifier a valid-looking descriptor fetched from
        // somewhere else entirely, which it would then write to --out. A
        // redirect off this path is a tenant misconfiguration or an
        // interception; either way the answer is to stop, not to follow.
        let http_no_redirect = builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest client");
        Self {
            tenant,
            http,
            http_no_redirect,
            token_cache: Arc::new(Mutex::new(TokenCache::new())),
            jwk,
        }
    }

    pub async fn bearer(&self) -> Result<String> {
        self.bearer_with_min_ttl(auth::MIN_TTL_FOR_OUR_OWN_REQUEST)
            .await
    }

    /// A bearer with at least `min_ttl` seconds left, minting a fresh one if
    /// the cached token is shorter-lived than that. A floor above the token's
    /// full lifetime mints on every call, which is a waste rather than an
    /// error — the tenant's TTL is the real ceiling and no caller can raise it.
    pub async fn bearer_with_min_ttl(&self, min_ttl: i64) -> Result<String> {
        // Check cache first
        {
            let cache = self.token_cache.lock().unwrap();
            if let Some(t) = cache.get_with_min_ttl(min_ttl) {
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

    /// `GET` a non-JSON body with **no** `Authorization` header.
    ///
    /// The one endpoint this exists for is AM's SAML metadata-export JSP,
    /// which is unauthenticated (verified: the body is byte-identical with and
    /// without a bearer, `docs/api/06-saml.md`) and answers `text/xml`. Both
    /// halves are the reason it is not `get`: that path attaches the
    /// service-account bearer and decodes the body as JSON.
    ///
    /// Sending no credential is the point, not a shortcut. An endpoint that
    /// does not need the service-account bearer must not receive it, and this
    /// method has no way to attach one.
    ///
    /// Confinement takes two things, and used to have one. [`Self::url`]
    /// prefixes the tenant base URL unconditionally, so the request we
    /// *send* cannot leave the tenant host whatever `path` says — and then
    /// reqwest's default policy would have followed up to ten redirects off
    /// it. The response body here is classified and written to a file, so a
    /// 302 was enough to save another host's document under the entity's
    /// name. This transport does not follow redirects at all: a 3xx arrives
    /// as the non-success status it is.
    ///
    /// A non-2xx status is still an error — but note that for the JSP a
    /// *failed* export is a 200, so the caller must classify the body too.
    pub async fn get_text_unauthenticated(&self, path: &str) -> Result<String> {
        let resp = self
            .unauthenticated_request(reqwest::Method::GET, path)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if status.is_success() {
            Ok(body)
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                body,
            })
        }
    }

    fn unauthenticated_request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        self.http_no_redirect.request(method, self.url(path))
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
        self.ensure_write_allowed(confirmed_prod)?;
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
        self.ensure_write_allowed(confirmed_prod)?;
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

    fn ensure_write_allowed(&self, confirmed_prod: bool) -> Result<()> {
        if self.tenant.theme == TenantTheme::Production && !confirmed_prod {
            Err(Error::ProdConfirmRequired)
        } else {
            Ok(())
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
    fn production_transport_requires_the_request_confirmation_bit() {
        let mut production = tenant("https://tenant.example".into());
        production.theme = TenantTheme::Production;
        let client = AicClient::new(production, serde_json::Value::Null);

        assert!(matches!(
            client.ensure_write_allowed(false),
            Err(Error::ProdConfirmRequired)
        ));
        assert!(client.ensure_write_allowed(true).is_ok());
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
    fn unauthenticated_request_sends_no_credential_and_is_addressed_at_the_tenant() {
        let client = AicClient::new(
            tenant("https://tenant.example".into()),
            serde_json::Value::Null,
        );
        let request = client
            .unauthenticated_request(reqwest::Method::GET, "/am/saml2/jsp/exportmetadata.jsp?x=1")
            .build()
            .unwrap();

        // The point of the method: the metadata JSP needs no bearer, so it
        // must not be handed one.
        assert!(request.headers().get("authorization").is_none());
        // Addressed at, not confined to. This is a property of the request we
        // build; where the *response* can come from is the redirect policy,
        // and the test below is the one that proves it.
        assert_eq!(
            request.url().as_str(),
            "https://tenant.example/am/saml2/jsp/exportmetadata.jsp?x=1"
        );
    }

    /// Following a redirect is the one way an unauthenticated GET can leave
    /// the tenant, and the SAML metadata classifier writes what comes back to
    /// a file under the entity's name. No credential is exposed either way —
    /// the hazard is the *body*, not the request.
    #[tokio::test]
    async fn an_unauthenticated_get_does_not_follow_a_redirect_off_the_tenant() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;

        // The host the redirect points at. It serves a perfectly valid
        // descriptor, which is exactly the hazard: followed, that document
        // would be classified as a successful export and saved.
        let elsewhere = TcpListener::bind("127.0.0.1:0").expect("bind the other host");
        let elsewhere_port = elsewhere.local_addr().expect("its port").port();
        let (contacted, was_contacted) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = elsewhere.accept() {
                let _ = contacted.send(());
                let body = "<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\"/>";
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\
                         Connection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });

        let redirector = TcpListener::bind("127.0.0.1:0").expect("bind the tenant");
        let base = format!(
            "http://127.0.0.1:{}",
            redirector.local_addr().expect("its port").port()
        );
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = redirector.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{elsewhere_port}/moved\r\n\
                     Content-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            );
        });

        let client = AicClient::new(tenant(base), serde_json::Value::Null);
        let result = client
            .get_text_unauthenticated("/am/saml2/jsp/exportmetadata.jsp?x=1")
            .await;

        // The redirect is reported as the failure it is, rather than followed
        // and then indistinguishable from a real export.
        assert!(
            matches!(result, Err(Error::Api { status: 302, .. })),
            "{result:?}"
        );
        assert!(
            was_contacted.try_recv().is_err(),
            "the redirect target was contacted; its document could have been saved"
        );
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
