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
        // Only RFC 3986 path characters, as typed: a tab, space, backslash or
        // non-ASCII character is something the URL parser rewrites (a tab is
        // dropped, a backslash becomes a separator), so the path checked here
        // would not be the path sent.
        if let Some(bad) = path.chars().find(|c| !is_path_char(*c)) {
            return refuse(&format!(
                "{bad:?} is not a URL path character; percent-encode it"
            ));
        }
        for segment in path.split('/') {
            if segment.is_empty() {
                return refuse("empty path segment");
            }
            let Some(decoded) = percent_decode(segment) else {
                return refuse("malformed percent-encoding");
            };
            if decoded == "." || decoded == ".." {
                return refuse("dot segments could leave /openidm/");
            }
            if decoded.contains(['/', '\\']) {
                return refuse("encoded slashes could leave /openidm/");
            }
            if decoded.contains('%') {
                return refuse("double percent-encoding");
            }
            if decoded.chars().any(char::is_control) {
                return refuse("encoded control characters");
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
        let request = Self {
            path: format!("/openidm/{path}"),
            query,
        };
        // The check that matters: the transport joins the path onto the base
        // URL and parses it, so the path it ends up sending must be exactly
        // the one validated above.
        if !request.sent_unchanged() {
            return refuse("the URL parser would rewrite it");
        }
        Ok(request)
    }

    fn sent_unchanged(&self) -> bool {
        url::Url::parse(&format!("https://tenant.invalid{}", self.page_path(None)))
            .is_ok_and(|sent| sent.path() == self.path)
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

/// RFC 3986 `pchar` plus the segment separator.
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~!$&'()*+,;=:@%/".contains(c)
}

/// Decodes one path segment's `%XX` escapes. `+` is a literal plus in a path,
/// and `&` is an ordinary character — not a query separator. `None` when an
/// escape is malformed or the bytes are not UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
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
    fn each_layer_refuses_the_escapes_on_its_own() {
        for input in ["/openidm/.\t./am", "x&\\..\\..\\am"] {
            let error = RawGet::parse(input, None, false).unwrap_err().to_string();
            assert!(
                error.contains("not a URL path character"),
                "{input}: {error}"
            );
        }
        for path in ["/openidm/.\t./am", "/openidm/x&\\..\\..\\am"] {
            let request = RawGet {
                path: path.into(),
                query: Vec::new(),
            };
            assert!(
                !request.sent_unchanged(),
                "{path:?} is rewritten in transit"
            );
        }
        assert!(
            RawGet::parse("managed/x&y", None, false)
                .unwrap()
                .sent_unchanged()
        );
    }

    #[test]
    fn path_characters_a_query_would_split_on_are_ordinary_here() {
        assert_eq!(path("managed/x&y+z"), "/openidm/managed/x&y+z");
        assert_eq!(path("managed/a%20b"), "/openidm/managed/a%20b");
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
            // Both reviewers' escapes: `&` once hid the rest of a segment from
            // a query-string decoder, and the URL parser drops a tab and reads
            // a backslash as a separator.
            "/openidm/x&y=\\..\\..\\am/json/realms/root",
            "x&\\..\\..\\am/json/realms/root",
            "/openidm/.\t./am/json/realms/root",
            "/openidm/config/a b",
            "/openidm/config/\u{ff0e}\u{ff0e}",
            "/openidm/config/%252e%252e",
            "/openidm/config/%2",
            "/openidm/config/%zz",
            "/openidm/config/%09",
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
