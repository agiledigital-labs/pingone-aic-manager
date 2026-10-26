//! `aic idm get <path>`: a read-only GET of any `/openidm/` path, with cursor
//! paging on request.
//!
//! GET-only, not side-effect-free: a GET on `/openidm/endpoint/<name>` runs
//! that endpoint's script, so the `/openidm/` prefix bounds *where* the call
//! goes, never what the tenant does when it gets there.
//!
//! Paging follows `pagedResultsCookie`, verified on managed-object queries
//! (`docs/api/10-managed-objects.md`, "Paging"); other collections are walked
//! the same way and a repeated cookie stops the walk.

use url::form_urlencoded::{Serializer, parse};

use crate::{Error, Result};

/// The page size `--all-pages` asks for when the path names none. Without
/// one IDM returns the whole result set in one response and no cookie.
pub const DEFAULT_PAGE_SIZE: &str = "1000";

/// A validated request: `/openidm/<segments>` plus the query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawGet {
    path: String,
    query: Vec<(String, String)>,
}

impl RawGet {
    /// Validate `input` and fold in `--fields` / `--all-pages`.
    ///
    /// Accepts `/openidm/x`, `openidm/x` or `x`. Refuses anything that could
    /// leave `/openidm/` once the server normalises it — a scheme, a dot
    /// segment, an encoded slash — rather than trusting the prefix of the
    /// string as typed.
    pub fn parse(input: &str, fields: Option<&str>, all_pages: bool) -> Result<Self> {
        let refuse = |why: &str| Err(Error::Config(format!("refusing path {input:?}: {why}")));
        let input = input.trim();
        if input.contains("://") || input.starts_with("//") {
            return refuse("give a path under /openidm/, not a URL");
        }
        if input.contains('#') {
            return refuse("fragments are not sent to the server");
        }
        let (path, query) = input.split_once('?').unwrap_or((input, ""));
        let path = path.strip_prefix('/').unwrap_or(path);
        let path = path.strip_prefix("openidm/").unwrap_or(path);
        if path.is_empty() || path == "openidm" {
            return refuse("name a resource under /openidm/");
        }
        for segment in path.split('/') {
            let decoded = percent_decode(segment);
            if segment.is_empty() {
                return refuse("empty path segment");
            }
            if decoded == "." || decoded == ".." {
                return refuse("dot segments could leave /openidm/");
            }
            if decoded.contains(['/', '\\']) {
                return refuse("encoded slashes could leave /openidm/");
            }
        }
        let mut query: Vec<(String, String)> = parse(query.as_bytes()).into_owned().collect();
        let has = |query: &[(String, String)], key: &str| query.iter().any(|(k, _)| k == key);
        if let Some(fields) = fields {
            if has(&query, "_fields") {
                return refuse("_fields is in the path and --fields was given; use one");
            }
            query.push(("_fields".into(), fields.into()));
        }
        if all_pages {
            for key in ["_pagedResultsCookie", "_pagedResultsOffset"] {
                if has(&query, key) {
                    return refuse(&format!("--all-pages manages paging; drop {key}"));
                }
            }
            if !has(&query, "_pageSize") {
                query.push(("_pageSize".into(), DEFAULT_PAGE_SIZE.into()));
            }
        }
        Ok(Self {
            path: format!("/openidm/{path}"),
            query,
        })
    }

    /// The request path for one page; `cookie` continues a cursor walk.
    pub fn page_path(&self, cookie: Option<&str>) -> String {
        let mut query = Serializer::new(String::new());
        for (key, value) in &self.query {
            query.append_pair(key, value);
        }
        if let Some(cookie) = cookie {
            query.append_pair("_pagedResultsCookie", cookie);
        }
        let query = query.finish();
        if query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{query}", self.path)
        }
    }
}

fn percent_decode(segment: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", segment.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(input: &str) -> String {
        RawGet::parse(input, None, false).unwrap().page_path(None)
    }

    #[test]
    fn accepts_the_three_spellings_of_an_openidm_path() {
        for input in [
            "/openidm/managed/alpha_user",
            "openidm/managed/alpha_user",
            "managed/alpha_user",
        ] {
            assert_eq!(path(input), "/openidm/managed/alpha_user");
        }
        assert_eq!(
            path("config/managed?_fields=objects/name"),
            "/openidm/config/managed?_fields=objects%2Fname"
        );
    }

    #[test]
    fn refuses_anything_that_could_leave_openidm() {
        for input in [
            "https://tenant.example.com/openidm/config",
            "//tenant.example.com/openidm/config",
            "/openidm/../am/json/realms/root",
            "/openidm/config/%2e%2e/%2E%2E/am",
            "/openidm/config/a%2fb",
            "/openidm/config/a%5Cb",
            "/openidm/config//managed",
            "/openidm/",
            "/openidm",
            "config#frag",
        ] {
            assert!(
                RawGet::parse(input, None, false).is_err(),
                "{input} must be refused"
            );
        }
    }

    #[test]
    fn all_pages_owns_the_cursor_and_defaults_the_page_size() {
        let get = RawGet::parse("managed/alpha_user?_queryFilter=true", Some("_id"), true).unwrap();
        assert_eq!(
            get.page_path(Some("abc/=")),
            "/openidm/managed/alpha_user?_queryFilter=true&_fields=_id&_pageSize=1000&_pagedResultsCookie=abc%2F%3D"
        );
        let sized = RawGet::parse("managed/x?_queryFilter=true&_pageSize=5", None, true).unwrap();
        assert_eq!(
            sized.page_path(None),
            "/openidm/managed/x?_queryFilter=true&_pageSize=5"
        );
        for input in [
            "managed/x?_queryFilter=true&_pagedResultsCookie=c",
            "managed/x?_queryFilter=true&_pagedResultsOffset=10",
        ] {
            assert!(RawGet::parse(input, None, true).is_err(), "{input}");
            assert!(RawGet::parse(input, None, false).is_ok(), "{input}");
        }
        assert!(RawGet::parse("managed/x?_fields=a", Some("b"), false).is_err());
    }
}
