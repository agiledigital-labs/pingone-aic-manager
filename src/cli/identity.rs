//! Tenant identity beyond the context name: the custom domains a tenant
//! answers on.
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
    fn host_strips_scheme_and_path() {
        assert_eq!(host("https://tenant.example.com"), "tenant.example.com");
        assert_eq!(
            host("https://tenant.example.com/am?x"),
            "tenant.example.com"
        );
        assert_eq!(host("tenant.example.com"), "tenant.example.com");
    }
}
