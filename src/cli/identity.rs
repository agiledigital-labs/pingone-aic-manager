//! Tenant identity beyond the context name: the custom domains a tenant
//! answers on, and the `whoami --token --host` check that a URL is this
//! tenant's.
//!
//! Two contexts can both be called "sandbox" and both have a base URL that
//! says so; the domain the operator actually types into a browser is what
//! tells them apart. `GET /environment/custom-domains/{realm}` is where AIC
//! keeps it. Measured 2026-10-31 on three tenants: the route exists and
//! answers `403 "The access token does not have sufficient scope"` without
//! `fr:idc:custom-domain:*`, which no context here holds, so the **success
//! shape is unmeasured**. It is read as `{"domains": [..]}`, the shape
//! `/environment/cookie-domains` was measured to return; anything else is
//! reported as unreadable, never as "none". AM realm `aliases` were empty on
//! all three tenants and are not consulted.

use serde_json::Value;

use crate::{Error, Result};

/// The realms a custom domain can be attached to.
const REALMS: [&str; 2] = ["alpha", "bravo"];

/// What a tenant's custom domains read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Domains {
    /// Every realm read; possibly empty.
    Listed(Vec<String>),
    /// The token lacks the scope. Says nothing about whether domains exist.
    NoScope,
    /// Any other failure, with the reason.
    Unreadable(String),
}

impl Domains {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Listed(domains) if domains.is_empty() => "none".into(),
            Self::Listed(domains) => domains.join(", "),
            Self::NoScope => {
                "unreadable — the token lacks fr:idc:custom-domain:* (grant it to the \
                              service account, then `aic ctx scopes --add fr:idc:custom-domain:*`)"
                    .into()
            }
            Self::Unreadable(reason) => format!("unreadable — {reason}"),
        }
    }

    /// The short form for a table cell.
    pub(crate) fn cell(&self) -> String {
        match self {
            Self::Listed(domains) if domains.is_empty() => "-".into(),
            Self::Listed(domains) => domains.join(","),
            Self::NoScope => "? (no scope)".into(),
            Self::Unreadable(_) => "? (error)".into(),
        }
    }
}

/// Fold the per-realm reads into one answer. Any realm that could not be read
/// makes the whole answer unreadable: a partial list would read as complete.
pub(crate) fn fold(reads: Vec<(&str, Result<Value>)>) -> Domains {
    let mut domains = Vec::new();
    for (realm, read) in reads {
        let body = match read {
            Ok(body) => body,
            Err(Error::Api { status: 403, body }) if body.contains("sufficient scope") => {
                return Domains::NoScope;
            }
            Err(error) => return Domains::Unreadable(format!("{realm}: {error}")),
        };
        let Some(listed) = body.get("domains").and_then(Value::as_array) else {
            return Domains::Unreadable(format!("{realm}: unexpected response shape"));
        };
        for domain in listed {
            let Some(domain) = domain.as_str() else {
                return Domains::Unreadable(format!("{realm}: a domain that is not a string"));
            };
            if !domains.iter().any(|seen| seen == domain) {
                domains.push(domain.to_string());
            }
        }
    }
    Domains::Listed(domains)
}

pub(crate) async fn custom_domains(tenant: &str) -> Domains {
    let mut reads = Vec::new();
    for realm in REALMS {
        let path = format!("/environment/custom-domains/{realm}");
        reads.push((realm, crate::aic::api::get(tenant, &path).await));
    }
    fold(reads)
}

/// The host of a base URL, for printing beside a tenant name.
pub(crate) fn host(base_url: &str) -> &str {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// The host a URL (or a bare host) names, normalised for comparison: no
/// scheme, userinfo, port, path or trailing dot, and lower case.
pub(crate) fn normalise_host(url_or_host: &str) -> String {
    let authority = host(url_or_host.trim());
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let without_port = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    };
    without_port.trim_end_matches('.').to_ascii_lowercase()
}

/// The one context whose base URL names `requested`'s host; `None` when no
/// context does, or when several share it.
pub(crate) fn context_for_host<'a>(
    contexts: &[(&'a str, &str)],
    requested: &str,
) -> Option<&'a str> {
    let wanted = normalise_host(requested);
    let mut owners = contexts
        .iter()
        .filter(|(_, url)| normalise_host(url) == wanted)
        .map(|(name, _)| *name);
    match (owners.next(), owners.next()) {
        (Some(name), None) => Some(name),
        _ => None,
    }
}

/// Refuse unless `requested` names `tenant`'s own host or one of its custom
/// domains. The domains are read only when the base host does not match, so
/// the usual case costs no request; an unreadable list refuses, because it
/// cannot show the host is this tenant's.
pub(crate) async fn check_host<F, Fut>(
    tenant: &str,
    base_url: &str,
    requested: &str,
    contexts: &[(&str, &str)],
    domains: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Domains>,
{
    let wanted = normalise_host(requested);
    if wanted.is_empty() {
        return Err(Error::Config(format!("--host {requested:?} names no host")));
    }
    let own = normalise_host(base_url);
    if wanted == own {
        return Ok(());
    }
    // Naming the context that does own the host turns the refusal into the fix.
    let remedy = context_for_host(contexts, &wanted).map_or_else(
        || {
            "pick the context that owns it with `--tenant <name>` (`aic ctx list` \
                 shows each context's host)"
                .to_string()
        },
        |name| format!("it is context `{name}`'s host: use `--tenant {name}`"),
    );
    let refusal = |why: String| {
        Error::Config(format!(
            "--host {wanted} is not context `{tenant}`'s host ({own}), and {why}. No \
             token was printed; {remedy}"
        ))
    };
    match domains().await {
        Domains::Listed(listed) => {
            if listed.iter().any(|domain| normalise_host(domain) == wanted) {
                Ok(())
            } else if listed.is_empty() {
                Err(refusal("the tenant has no custom domains".into()))
            } else {
                Err(refusal(format!(
                    "not one of its custom domains ({})",
                    listed.join(", ")
                )))
            }
        }
        other => Err(refusal(format!(
            "its custom domains could not be read to check: {}",
            other.describe()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope_refusal() -> Result<Value> {
        Err(Error::Api {
            status: 403,
            body: r#"{"code":403,"message":"The access token does not have sufficient scope"}"#
                .into(),
        })
    }

    #[test]
    fn both_realms_listed_merge_without_duplicates() {
        let read = fold(vec![
            (
                "alpha",
                Ok(json!({ "domains": ["a.example.com", "b.example.com"] })),
            ),
            ("bravo", Ok(json!({ "domains": ["b.example.com"] }))),
        ]);
        assert_eq!(
            read,
            Domains::Listed(vec!["a.example.com".into(), "b.example.com".into()])
        );
        assert_eq!(read.cell(), "a.example.com,b.example.com");
    }

    #[test]
    fn empty_lists_are_none_not_unknown() {
        let read = fold(vec![
            ("alpha", Ok(json!({ "domains": [] }))),
            ("bravo", Ok(json!({ "domains": [] }))),
        ]);
        assert_eq!(read.describe(), "none");
    }

    #[test]
    fn a_scope_refusal_is_not_none() {
        // The measured case: every context here answers this way.
        let read = fold(vec![("alpha", scope_refusal()), ("bravo", scope_refusal())]);
        assert_eq!(read, Domains::NoScope);
        assert!(read.describe().contains("fr:idc:custom-domain:*"));
        // Another 403 is not the scope case.
        let other = fold(vec![(
            "alpha",
            Err(Error::Api {
                status: 403,
                body: "forbidden".into(),
            }),
        )]);
        assert!(matches!(other, Domains::Unreadable(_)));
    }

    #[test]
    fn one_unreadable_realm_spoils_the_list() {
        let read = fold(vec![
            ("alpha", Ok(json!({ "domains": ["a.example.com"] }))),
            ("bravo", Ok(json!({ "unexpected": true }))),
        ]);
        assert!(matches!(read, Domains::Unreadable(ref reason) if reason.starts_with("bravo")));
    }

    #[test]
    fn normalise_host_reduces_every_spelling_to_the_host() {
        for spelling in [
            "https://tenant.example.com/am/json/realms/root?x=1",
            "tenant.example.com",
            "TENANT.example.com.",
            "https://tenant.example.com:443/openidm",
            "https://someone@tenant.example.com/",
            " https://tenant.example.com ",
        ] {
            assert_eq!(normalise_host(spelling), "tenant.example.com", "{spelling}");
        }
        // userinfo that looks like a host must not win over the real one.
        assert_eq!(
            normalise_host("https://tenant.example.com@other.example.com/"),
            "other.example.com"
        );
    }

    fn no_domains_needed() -> std::future::Ready<Domains> {
        panic!("custom domains must not be read when the base host matches")
    }

    #[tokio::test]
    async fn the_tenant_host_passes_without_reading_domains() {
        check_host(
            "dev",
            "https://tenant.example.com",
            "https://tenant.example.com/openidm/managed/alpha_user",
            &[],
            no_domains_needed,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn another_tenants_host_is_refused() {
        let error = check_host(
            "dev",
            "https://tenant.example.com",
            "https://other.example.com/am",
            &[
                ("dev", "https://tenant.example.com"),
                ("uat", "https://Other.example.com/"),
            ],
            || std::future::ready(Domains::Listed(vec![])),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("other.example.com"), "{error}");
        assert!(error.contains("`dev`"), "{error}");
        assert!(error.contains("No token was printed"), "{error}");
        assert!(error.contains("use `--tenant uat`"), "{error}");
    }

    #[tokio::test]
    async fn a_custom_domain_of_the_tenant_passes() {
        check_host(
            "dev",
            "https://tenant.example.com",
            "https://login.example.com/am",
            &[],
            || std::future::ready(Domains::Listed(vec!["login.example.com".into()])),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn unreadable_domains_refuse_rather_than_pass() {
        let error = check_host(
            "dev",
            "https://tenant.example.com",
            "https://login.example.com/am",
            &[],
            || std::future::ready(Domains::NoScope),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("could not be read to check"), "{error}");
        assert!(error.contains("`aic ctx list`"), "{error}");
        assert!(error.contains("fr:idc:custom-domain:*"), "{error}");
    }

    #[tokio::test]
    async fn an_empty_host_is_refused() {
        assert!(
            check_host(
                "dev",
                "https://tenant.example.com",
                "https://",
                &[],
                no_domains_needed
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn the_url_picks_its_one_context() {
        let contexts = [
            ("dev", "https://dev.example.com"),
            ("uat", "https://UAT.example.com/"),
            ("uat-copy", "https://shared.example.com"),
            ("uat-twin", "https://shared.example.com"),
        ];
        assert_eq!(
            context_for_host(&contexts, "https://uat.example.com/openidm/x"),
            Some("uat")
        );
        assert_eq!(context_for_host(&contexts, "login.example.com"), None);
        // Two contexts on one host: ambiguous, so the caller must name one.
        assert_eq!(context_for_host(&contexts, "shared.example.com"), None);
    }

    #[test]
    fn host_strips_scheme_and_path() {
        assert_eq!(host("https://tenant.example.com"), "tenant.example.com");
        assert_eq!(
            host("https://tenant.example.com/am?x"),
            "tenant.example.com"
        );
        assert_eq!(host("tenant.example.com"), "tenant.example.com");
    }
}
