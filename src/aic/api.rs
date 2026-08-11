//! Surface-agnostic AIC HTTP helpers. **The TUI and CLI both call into here**
//! — neither one builds an `AicClient` directly. The bearer-mint, token
//! cache, prod-confirm guard, and HTTP connection pool all live in the
//! agent process; this module is the thin client glue that wraps a single
//! request/response in the `agent::Request::ApiCall` envelope.
//!
//! Add a new resource (scripts, OAuth2, journeys) by adding a module
//! alongside this one (e.g. `esv::api`) that uses these primitives — do
//! NOT thread a parallel HTTP path through `AicClient::*` in either
//! frontend.
//!
//! # Needing a new *combination* of options? Build an [`ApiCall`].
//!
//! The free functions below are the common shapes, and each one is a
//! one-liner over [`ApiCall`]. They are not a menu to be extended: every
//! transport concern that arrived as a new positional parameter (api
//! version, then `If-Match`) had to be threaded through all of them, so a
//! caller wanting, say, a `PATCH` with an `If-Match` should write
//!
//! ```ignore
//! ApiCall::new(tenant, "PATCH", path)
//!     .body(body)
//!     .confirmed_prod(yes)
//!     .if_match(revision)
//!     .send()
//!     .await
//! ```
//!
//! rather than add a twelfth wrapper.
//!
//! [`crate::config::operator`] also uses [`get_versioned`] for its optional,
//! best-effort service-account-name lookup; keeping that read here means
//! operator resolution never creates a second bearer or transport path.

use crate::agent::{AgentClient, Request, Response};
use crate::{Error, Result};

/// The content type the daemon uses — together with a `POST` method — to
/// route a call down the form transport, which deliberately attaches no
/// service-account bearer. See [`ApiCall::form_body`].
const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";

pub async fn get(tenant: &str, path: &str) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "GET", path).send().await
}

pub async fn put(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "PUT", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .send()
        .await
}

pub async fn post(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "POST", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .send()
        .await
}

/// `POST` a form-encoded body through the daemon-owned HTTP connection pool.
/// The form transport does not attach the service-account bearer because OAuth2
/// token endpoints authenticate from the form body.
pub async fn post_form(tenant: &str, path: &str, body: &str) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "POST", path)
        .form_body(body)
        .send()
        .await
}

/// `POST` with an explicit `Accept-API-Version`.
pub async fn post_versioned(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "POST", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .api_version(api_version)
        .send()
        .await
}

pub async fn patch(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "PATCH", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .send()
        .await
}

pub async fn delete(tenant: &str, path: &str, confirmed_prod: bool) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "DELETE", path)
        .confirmed_prod(confirmed_prod)
        .send()
        .await
}

/// `GET` with an explicit `Accept-API-Version` (AM scripts need
/// `protocol=2.0,resource=1.0`; IDM config endpoints set their own).
pub async fn get_versioned(
    tenant: &str,
    path: &str,
    api_version: &str,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "GET", path)
        .api_version(api_version)
        .send()
        .await
}

/// `PUT` with an explicit `Accept-API-Version`.
pub async fn put_versioned(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "PUT", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .api_version(api_version)
        .send()
        .await
}

/// `PUT` with explicit API version and optimistic-concurrency revision.
pub async fn put_versioned_if_match(
    tenant: &str,
    path: &str,
    body: serde_json::Value,
    confirmed_prod: bool,
    api_version: &str,
    revision: &str,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "PUT", path)
        .body(body)
        .confirmed_prod(confirmed_prod)
        .api_version(api_version)
        .if_match(revision)
        .send()
        .await
}

/// `DELETE` with an explicit `Accept-API-Version`.
pub async fn delete_versioned(
    tenant: &str,
    path: &str,
    confirmed_prod: bool,
    api_version: &str,
) -> Result<serde_json::Value> {
    ApiCall::new(tenant, "DELETE", path)
        .confirmed_prod(confirmed_prod)
        .api_version(api_version)
        .send()
        .await
}

/// One tenant-scoped AIC call, assembled option by option.
///
/// Everything past the method and path defaults to "absent", which is what
/// most calls want: no body, not prod-confirmed, default
/// `Accept-API-Version`, no `If-Match`, JSON transport. Setters are
/// chainable and each one names a single transport concern, so adding a
/// concern later costs one setter instead of one positional parameter at
/// every call site.
pub struct ApiCall<'a> {
    tenant: &'a str,
    method: &'a str,
    path: &'a str,
    body: Option<serde_json::Value>,
    confirmed_prod: bool,
    content_type: Option<&'a str>,
    api_version: Option<&'a str>,
    if_match: Option<&'a str>,
}

impl<'a> ApiCall<'a> {
    pub fn new(tenant: &'a str, method: &'a str, path: &'a str) -> Self {
        Self {
            tenant,
            method,
            path,
            body: None,
            confirmed_prod: false,
            content_type: None,
            api_version: None,
            if_match: None,
        }
    }

    /// Send `body` as JSON.
    pub fn body(mut self, body: serde_json::Value) -> Self {
        self.body = Some(body);
        self.content_type = None;
        self
    }

    /// Send an already-encoded `application/x-www-form-urlencoded` body.
    ///
    /// This is not merely a different `Content-Type`: the daemon keys its
    /// no-bearer form transport off `POST` *plus* this content type
    /// (`agent::daemon::do_api_call`), so a call built this way authenticates
    /// from the body alone. That is what OAuth2 token endpoints want, and
    /// sending the service-account bearer there would hand a more powerful
    /// tenant credential to an endpoint that neither needs nor accepts it.
    pub fn form_body(mut self, body: &str) -> Self {
        self.body = Some(serde_json::Value::String(body.to_string()));
        self.content_type = Some(FORM_CONTENT_TYPE);
        self
    }

    /// Greenlight a write against a prod-themed tenant. The caller asks the
    /// user first (modal in the TUI, `--yes` in the CLI).
    pub fn confirmed_prod(mut self, yes: bool) -> Self {
        self.confirmed_prod = yes;
        self
    }

    /// Override the `Accept-API-Version` header (default `resource=1.0`).
    pub fn api_version(mut self, api_version: &'a str) -> Self {
        self.api_version = Some(api_version);
        self
    }

    /// Attach an optimistic-concurrency precondition. Only for API families
    /// verified to honour conditional writes — see `CLAUDE.md` §5.
    pub fn if_match(mut self, revision: &'a str) -> Self {
        self.if_match = Some(revision);
        self
    }

    /// The wire envelope this call will be sent as. Split out from
    /// [`Self::send`] so tests can pin the wrapper-to-envelope mapping
    /// without a live agent.
    fn envelope(self) -> Request {
        Request::ApiCall {
            tenant: self.tenant.to_string(),
            method: self.method.to_string(),
            path: self.path.to_string(),
            body: self.body,
            confirmed_prod: self.confirmed_prod,
            content_type: self.content_type.map(str::to_owned),
            api_version: self.api_version.map(str::to_owned),
            if_match: self.if_match.map(str::to_owned),
        }
    }

    pub async fn send(self) -> Result<serde_json::Value> {
        let agent = AgentClient::connect_or_spawn().await?;
        let resp = agent.send(&self.envelope()).await?;
        match resp {
            Response::Json { value } => Ok(value),
            Response::Locked => Err(Error::Auth(
                "agent is locked — run `aic session login` (CLI) or unlock the TUI".into(),
            )),
            Response::ProdConfirmRequired => Err(Error::ProdConfirmRequired),
            Response::ApiError { status, body } => Err(Error::Api { status, body }),
            Response::Error { message } => Err(Error::Config(message)),
            other => Err(Error::Config(format!("unexpected agent reply: {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The envelope fields, flattened into something comparable. `Request`
    /// deliberately derives no `PartialEq`, and the whole point of these
    /// tests is field-by-field equality against a literal expectation.
    #[derive(Debug, PartialEq)]
    struct Envelope {
        tenant: String,
        method: String,
        path: String,
        body: Option<serde_json::Value>,
        confirmed_prod: bool,
        content_type: Option<String>,
        api_version: Option<String>,
        if_match: Option<String>,
    }

    impl Envelope {
        fn of(call: ApiCall<'_>) -> Self {
            match call.envelope() {
                Request::ApiCall {
                    tenant,
                    method,
                    path,
                    body,
                    confirmed_prod,
                    content_type,
                    api_version,
                    if_match,
                } => Self {
                    tenant,
                    method,
                    path,
                    body,
                    confirmed_prod,
                    content_type,
                    api_version,
                    if_match,
                },
                other => panic!("expected an api_call envelope, got {other:?}"),
            }
        }

        /// Every field absent — what a bare `GET` should produce, and the
        /// baseline each wrapper's expectation is written as a diff against.
        fn bare(method: &str) -> Self {
            Self {
                tenant: TENANT.into(),
                method: method.into(),
                path: PATH.into(),
                body: None,
                confirmed_prod: false,
                content_type: None,
                api_version: None,
                if_match: None,
            }
        }
    }

    const TENANT: &str = "sandbox";
    const PATH: &str = "/openidm/managed/alpha_user/x";

    fn body() -> serde_json::Value {
        serde_json::json!({"givenName": "Ada"})
    }

    /// A defaulted `ApiCall` must opt into nothing. Each of these fields is
    /// a transport concern that changes what the daemon does — a stray
    /// `content_type` reroutes the call onto the bearer-less form transport,
    /// a stray `if_match` turns an unconditional write conditional — so
    /// "absent unless asked for" is the property worth pinning.
    #[test]
    fn a_new_call_opts_into_nothing() {
        assert_eq!(
            Envelope::of(ApiCall::new(TENANT, "GET", PATH)),
            Envelope::bare("GET")
        );
    }

    /// The wrappers are the compatibility surface: their signatures are
    /// fixed, so the only way they can break is by mapping onto a different
    /// envelope than they used to. One case per wrapper, each asserting the
    /// whole envelope rather than the field it happens to set — a wrapper
    /// that leaks an *extra* option is exactly the regression a
    /// per-field assertion would miss.
    #[test]
    fn each_wrapper_maps_onto_its_envelope() {
        let cases: Vec<(&str, ApiCall<'_>, Envelope)> = vec![
            (
                "get",
                ApiCall::new(TENANT, "GET", PATH),
                Envelope::bare("GET"),
            ),
            (
                "put",
                ApiCall::new(TENANT, "PUT", PATH)
                    .body(body())
                    .confirmed_prod(true),
                Envelope {
                    body: Some(body()),
                    confirmed_prod: true,
                    ..Envelope::bare("PUT")
                },
            ),
            (
                "post",
                ApiCall::new(TENANT, "POST", PATH)
                    .body(body())
                    .confirmed_prod(false),
                Envelope {
                    body: Some(body()),
                    ..Envelope::bare("POST")
                },
            ),
            (
                "post_form",
                ApiCall::new(TENANT, "POST", PATH)
                    .form_body("grant_type=example&scope=fr%3Aidm%3A*"),
                Envelope {
                    body: Some(serde_json::Value::String(
                        "grant_type=example&scope=fr%3Aidm%3A*".into(),
                    )),
                    content_type: Some("application/x-www-form-urlencoded".into()),
                    ..Envelope::bare("POST")
                },
            ),
            (
                "post_versioned",
                ApiCall::new(TENANT, "POST", PATH)
                    .body(body())
                    .confirmed_prod(true)
                    .api_version("protocol=2.0,resource=1.0"),
                Envelope {
                    body: Some(body()),
                    confirmed_prod: true,
                    api_version: Some("protocol=2.0,resource=1.0".into()),
                    ..Envelope::bare("POST")
                },
            ),
            (
                "patch",
                ApiCall::new(TENANT, "PATCH", PATH)
                    .body(body())
                    .confirmed_prod(true),
                Envelope {
                    body: Some(body()),
                    confirmed_prod: true,
                    ..Envelope::bare("PATCH")
                },
            ),
            (
                "delete",
                ApiCall::new(TENANT, "DELETE", PATH).confirmed_prod(true),
                Envelope {
                    confirmed_prod: true,
                    ..Envelope::bare("DELETE")
                },
            ),
            (
                "get_versioned",
                ApiCall::new(TENANT, "GET", PATH).api_version("resource=2.0"),
                Envelope {
                    api_version: Some("resource=2.0".into()),
                    ..Envelope::bare("GET")
                },
            ),
            (
                "put_versioned",
                ApiCall::new(TENANT, "PUT", PATH)
                    .body(body())
                    .confirmed_prod(true)
                    .api_version("resource=2.0"),
                Envelope {
                    body: Some(body()),
                    confirmed_prod: true,
                    api_version: Some("resource=2.0".into()),
                    ..Envelope::bare("PUT")
                },
            ),
            (
                "put_versioned_if_match",
                ApiCall::new(TENANT, "PUT", PATH)
                    .body(body())
                    .confirmed_prod(true)
                    .api_version("resource=2.0")
                    .if_match("00000000-1"),
                Envelope {
                    body: Some(body()),
                    confirmed_prod: true,
                    api_version: Some("resource=2.0".into()),
                    if_match: Some("00000000-1".into()),
                    ..Envelope::bare("PUT")
                },
            ),
            (
                "delete_versioned",
                ApiCall::new(TENANT, "DELETE", PATH)
                    .confirmed_prod(true)
                    .api_version("resource=2.0"),
                Envelope {
                    confirmed_prod: true,
                    api_version: Some("resource=2.0".into()),
                    ..Envelope::bare("DELETE")
                },
            ),
        ];

        for (wrapper, call, expected) in cases {
            assert_eq!(Envelope::of(call), expected, "{wrapper} envelope drifted");
        }
    }

    /// The daemon routes onto its bearer-less form transport on `POST` *plus*
    /// this exact content type (`agent::daemon::do_api_call`). Both halves of
    /// that discriminator have to come out of `form_body`, and — because the
    /// daemon re-reads the body with `as_str()` — the body has to be a JSON
    /// string rather than an object.
    #[test]
    fn form_body_produces_the_daemons_no_bearer_discriminator() {
        let envelope = Envelope::of(
            ApiCall::new(TENANT, "POST", "/am/oauth2/access_token")
                .form_body("grant_type=client_credentials"),
        );

        assert_eq!(envelope.method, "POST");
        assert_eq!(
            envelope.content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            envelope.body.as_ref().and_then(serde_json::Value::as_str),
            Some("grant_type=client_credentials")
        );
    }

    /// A JSON body must not inherit a form content type from a call that was
    /// built as a form first. Order-independence matters because the setters
    /// are chainable in any order and the mistake is silent: the daemon would
    /// keep routing to the form transport and drop the bearer.
    #[test]
    fn a_json_body_clears_an_earlier_form_content_type() {
        let envelope = Envelope::of(
            ApiCall::new(TENANT, "POST", PATH)
                .form_body("grant_type=example")
                .body(body()),
        );

        assert_eq!(envelope.content_type, None);
        assert_eq!(envelope.body, Some(body()));
    }
}
