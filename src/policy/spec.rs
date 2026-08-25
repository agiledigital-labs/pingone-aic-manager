//! TUI-free input types and pure transforms for AM policies.
//!
//! Two jobs, both of which exist because the API is quiet about failure:
//!
//! - **Content normalisation** for the pull/push snapshot rule (CLAUDE.md §5).
//!   Policies have no `_rev`; policy sets and resource types carry one plus
//!   four audit fields, none of which is part of what an operator authored.
//! - **[`diagnose`]**, which turns `actions: {}` — "no policy applied", the same
//!   answer for a resource that missed, a subject that failed and a condition
//!   that failed — into a list of the reasons that actually fit the request.

use std::collections::BTreeSet;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value};

/// Fields AM writes and an operator never authors. Stripped before any
/// content comparison and before any body is sent back.
const SERVER_MANAGED: &[&str] = &[
    "_rev",
    "createdBy",
    "creationDate",
    "lastModifiedBy",
    "lastModifiedDate",
    "editable",
];

/// The authored content of an entitlement object: everything AM did not write
/// itself. Comparing these is the revert-detection rule from CLAUDE.md §5.
pub fn content(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let kept = object
        .iter()
        .filter(|(key, _)| !SERVER_MANAGED.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    Value::Object(kept)
}

pub fn content_equal(a: &Value, b: &Value) -> bool {
    content(a) == content(b)
}

// ------------------------------------------------------- URL wildcards
//
// Measured against the live tenant on 2026-08-25 and tabulated in
// `docs/api/21-am-policies.md`. The vendor docs describe `*` the other way
// round, so this implementation follows the observations:
//
//   *     zero or more characters, crossing `/` but never `?`
//   -*-   zero or more characters within one path segment
//   case  insensitive throughout, path included
//   ?     splits resource from query; a pattern with no `?` matches only a
//         resource with no query string
//   port  a missing port is defaulted from the scheme before comparison

fn default_port(scheme: &str) -> Option<&'static str> {
    match scheme {
        "http" => Some("80"),
        "https" => Some("443"),
        _ => None,
    }
}

/// Insert the scheme's default port when the resource omits one, the way AM
/// does before matching. Returns the input unchanged when there is no default
/// to apply — an unknown scheme, or a string that is not a URL at all.
pub fn normalise_resource(resource: &str) -> String {
    let lower = resource.to_lowercase();
    let Some((scheme, rest)) = lower.split_once("://") else {
        return lower;
    };
    let Some(port) = default_port(scheme) else {
        return lower;
    };
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    if authority.contains(':') {
        return lower;
    }
    format!("{scheme}://{authority}:{port}{tail}")
}

/// Match one measured-semantics glob segment-wise. `single_level` is true
/// while expanding a `-*-`, which may not cross `/`.
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    // Iterative backtracking: at most one open `*` needs remembering, and
    // `-*-` is bounded by the next `/` so it never needs a resume point.
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None, 0usize);

    while t < text.len() {
        if pattern[p..].starts_with(b"-*-") {
            // Single-level: try the shortest expansion, extend up to the
            // next `/` on failure. Recursion keeps this readable and the
            // depth is the number of `-*-` in one pattern.
            let limit = text[t..]
                .iter()
                .position(|byte| *byte == b'/')
                .map_or(text.len(), |offset| t + offset);
            return (t..=limit).any(|split| glob_match(&pattern[p + 3..], &text[split..]));
        }
        if p < pattern.len() && pattern[p] == b'*' {
            star_p = Some(p);
            p += 1;
            star_t = t;
            continue;
        }
        if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
            continue;
        }
        // `*` never crosses a `?`, so a backtrack that would swallow one is
        // not available.
        match star_p {
            Some(star) if text[star_t] != b'?' => {
                p = star + 1;
                star_t += 1;
                t = star_t;
            }
            _ => return false,
        }
    }

    while p < pattern.len() {
        if pattern[p..].starts_with(b"-*-") {
            p += 3;
        } else if pattern[p] == b'*' {
            p += 1;
        } else {
            return false;
        }
    }
    true
}

/// Does `resource` match `pattern` under AM's URL comparator?
///
/// Conservative by construction: it is used only to explain a deny, so a
/// pattern it fails to understand should produce no advice rather than wrong
/// advice.
pub fn resource_matches(pattern: &str, resource: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let resource = normalise_resource(resource);
    let (pattern_path, pattern_query) = split_query(&pattern);
    let (resource_path, resource_query) = split_query(&resource);

    match (pattern_query, resource_query) {
        (None, Some(_)) | (Some(_), None) => false,
        (None, None) => glob_match(pattern_path.as_bytes(), resource_path.as_bytes()),
        (Some(pattern_query), Some(resource_query)) => {
            glob_match(pattern_path.as_bytes(), resource_path.as_bytes())
                && glob_match(pattern_query.as_bytes(), resource_query.as_bytes())
        }
    }
}

fn split_query(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((head, tail)) => (head, Some(tail)),
        None => (url, None),
    }
}

// -------------------------------------------------------- subject claims

/// Decode a JWT payload **without verifying anything**.
///
/// That is exactly what the PDP itself does with `subject.jwt`, and it is only
/// used here to explain a decision: "the policy wants `demoRoles` to contain
/// `payments.admin`; the token you presented carries `[orders.reader]`". Never
/// make an authorization decision from this.
pub fn unverified_claims(token: &str) -> Option<Map<String, Value>> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .as_object()
        .cloned()
}

/// Does a claim value satisfy a `JwtClaim` leaf? AM matches inside an array
/// claim as well as against a scalar — verified 2026-08-25, and the whole
/// reason the demo can read `scope` out of the token.
fn claim_satisfies(claim: &Value, wanted: &str) -> bool {
    match claim {
        Value::Array(items) => items.iter().any(|item| scalar_eq(item, wanted)),
        scalar => scalar_eq(scalar, wanted),
    }
}

/// `claimValue` is always a string on the wire, so a numeric or boolean claim
/// is compared by how it renders.
fn scalar_eq(value: &Value, wanted: &str) -> bool {
    match value {
        Value::String(text) => text == wanted,
        Value::Number(number) => number.to_string() == wanted,
        Value::Bool(flag) => flag.to_string() == wanted,
        _ => false,
    }
}

/// Every `JwtClaim` leaf in a subject tree, as (claimName, claimValue).
fn jwt_claim_leaves(node: Option<&Value>, out: &mut Vec<(String, String)>) {
    let Some(node) = node else { return };
    if let Some(items) = node.as_array() {
        for item in items {
            jwt_claim_leaves(Some(item), out);
        }
        return;
    }
    let Some(object) = node.as_object() else {
        return;
    };
    if object.get("type").and_then(Value::as_str) == Some("JwtClaim")
        && let (Some(name), Some(value)) = (
            object.get("claimName").and_then(Value::as_str),
            object.get("claimValue").and_then(Value::as_str),
        )
    {
        out.push((name.to_owned(), value.to_owned()));
    }
    for key in ["subjects", "conditions", "subject", "condition"] {
        jwt_claim_leaves(object.get(key), out);
    }
}

// ---------------------------------------------------------- diagnosis

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    /// No `subject` in the body: AM evaluates as the caller, and a
    /// service-account bearer satisfies `AuthenticatedUsers`.
    Caller,
    Jwt,
    SsoToken,
    Claims,
}

/// Everything `diagnose` needs, gathered by the caller so the reasoning stays
/// pure and testable.
pub struct Decision<'a> {
    pub resource: &'a str,
    /// The response row's `actions`. Empty means "no policy applied".
    pub actions: &'a Map<String, Value>,
    /// Actions the operator named with `--action`, if any.
    pub wanted: &'a [String],
    /// The set's resource types, as returned by the API.
    pub resource_types: &'a [Value],
    /// Every policy in the set.
    pub policies: &'a [Value],
    pub subject_kind: SubjectKind,
    /// Keys present in the evaluation `environment`.
    pub environment_keys: &'a BTreeSet<String>,
    /// The presented token's claims, decoded but **not verified**. `None`
    /// unless the subject is a JWT.
    pub subject_claims: Option<&'a Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub text: String,
}

fn hint(text: impl Into<String>) -> Hint {
    Hint { text: text.into() }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Every `type` appearing anywhere in a recursive subject or condition tree.
fn tree_types(node: Option<&Value>, out: &mut BTreeSet<String>) {
    let Some(node) = node else { return };
    if let Some(items) = node.as_array() {
        for item in items {
            tree_types(Some(item), out);
        }
        return;
    }
    let Some(object) = node.as_object() else {
        return;
    };
    if let Some(kind) = object.get("type").and_then(Value::as_str) {
        out.insert(kind.to_owned());
    }
    for key in ["subjects", "conditions", "subject", "condition"] {
        tree_types(object.get(key), out);
    }
}

fn policy_name(policy: &Value) -> String {
    policy
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_owned()
}

fn policy_matches_resource(policy: &Value, resource: &str) -> bool {
    strings(policy.get("resources"))
        .iter()
        .any(|pattern| resource_matches(pattern, resource))
}

fn policy_is_active(policy: &Value) -> bool {
    policy
        .get("active")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Reconstruct why a decision looks the way it does.
///
/// Returns hints in the order an operator should read them: the resource first,
/// because a resource that matched nothing makes every later question moot.
pub fn diagnose(decision: &Decision<'_>) -> Vec<Hint> {
    let mut hints = Vec::new();
    let resource = decision.resource;

    let declared_actions = decision
        .resource_types
        .iter()
        .filter_map(|rt| rt.get("actions").and_then(Value::as_object))
        .flat_map(|actions| actions.keys().cloned())
        .collect::<BTreeSet<_>>();
    for wanted in decision.wanted {
        if !declared_actions.contains(wanted) {
            hints.push(hint(format!(
                "action {wanted:?} is not declared by any resource type in this set \
                 (declared: {}) — a policy granting it can never apply",
                joined(&declared_actions)
            )));
        }
    }

    let satisfied = decision.wanted.is_empty() && !decision.actions.is_empty()
        || (!decision.wanted.is_empty()
            && decision
                .wanted
                .iter()
                .all(|action| decision.actions.contains_key(action)));
    if satisfied {
        return hints;
    }

    let patterns = decision
        .resource_types
        .iter()
        .flat_map(|rt| strings(rt.get("patterns")))
        .collect::<Vec<_>>();
    if !patterns
        .iter()
        .any(|pattern| resource_matches(pattern, resource))
    {
        if resource.contains('?') && !patterns.iter().any(|pattern| pattern.contains('?')) {
            hints.push(hint(format!(
                "resource {resource:?} carries a query string and no pattern in this set has a \
                 `?` — AM matches the query as part of the resource, so strip it before \
                 evaluating (or add `?*` patterns)"
            )));
        }
        match resource.split_once("://") {
            None => hints.push(hint(format!(
                "resource {resource:?} is not a URL; this set compares resources as URLs, so \
                 write them scheme://host:port/path"
            ))),
            Some((scheme, rest)) => {
                let authority = rest.split(['/', '?']).next().unwrap_or_default();
                if !authority.contains(':') && default_port(&scheme.to_lowercase()).is_none() {
                    hints.push(hint(format!(
                        "resource {resource:?} names no port and AM has no default for the \
                         {scheme:?} scheme, so it cannot match a `scheme://host:port/...` \
                         pattern — write the port explicitly"
                    )));
                }
            }
        }
        hints.push(hint(format!(
            "resource {resource:?} matches none of the set's resource-type patterns ({}) — \
             remember `*` crosses `/` and `-*-` does not",
            joined_slice(&patterns)
        )));
        return hints;
    }

    let candidates = decision
        .policies
        .iter()
        .filter(|policy| policy_matches_resource(policy, resource))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        hints.push(hint(format!(
            "the resource is inside the set's resource types but no policy in the set lists a \
             matching resource pattern; {} polic{} in the set",
            decision.policies.len(),
            if decision.policies.len() == 1 {
                "y"
            } else {
                "ies"
            }
        )));
        return hints;
    }

    for policy in &candidates {
        if !policy_is_active(policy) {
            hints.push(hint(format!(
                "policy {} matches this resource but is inactive",
                policy_name(policy)
            )));
        }
    }

    let mut subject_types = BTreeSet::new();
    let mut condition_types = BTreeSet::new();
    for policy in &candidates {
        tree_types(policy.get("subject"), &mut subject_types);
        tree_types(policy.get("condition"), &mut condition_types);
    }

    if decision.subject_kind == SubjectKind::Jwt && subject_types.contains("AuthenticatedUsers") {
        hints.push(hint(
            "a matching policy uses the `AuthenticatedUsers` subject, which a `jwt` subject \
             never satisfies — a JWT is not a session. Use `JwtClaim`.",
        ));
    }
    if decision.subject_kind == SubjectKind::Claims {
        hints.push(hint(
            "a `claims` subject satisfies neither `AuthenticatedUsers` nor `JwtClaim`; it is \
             effectively anonymous. Pass the token with --subject-jwt instead.",
        ));
    }
    if condition_types.contains("OAuth2Scope") && !decision.environment_keys.contains("scope") {
        hints.push(hint(
            "a matching policy has an `OAuth2Scope` condition, which reads `environment.scope` \
             (singular) — and nothing supplied it. Pass --env scope=<...>.",
        ));
    }

    // The most useful line in the whole command: which JwtClaim leaf the
    // presented token fails, and what it carries instead.
    if let Some(claims) = decision.subject_claims {
        for policy in &candidates {
            let mut leaves = Vec::new();
            jwt_claim_leaves(policy.get("subject"), &mut leaves);
            for (name, wanted) in leaves {
                let present = claims.get(&name);
                if present.is_some_and(|claim| claim_satisfies(claim, &wanted)) {
                    continue;
                }
                let held = match present {
                    None => "the token has no such claim".to_string(),
                    Some(value) => format!("the token has {value}"),
                };
                hints.push(hint(format!(
                    "policy {} wants claim {name}={wanted:?}; {held}",
                    policy_name(policy)
                )));
            }
        }
    }

    let names = candidates
        .iter()
        .map(|policy| policy_name(policy))
        .collect::<Vec<_>>();
    hints.push(hint(format!(
        "the resource matched polic{} {} — so the subject or a condition is what failed \
         (subjects: {}; conditions: {})",
        if names.len() == 1 { "y" } else { "ies" },
        names.join(", "),
        joined(&subject_types),
        if condition_types.is_empty() {
            "none".to_string()
        } else {
            joined(&condition_types)
        }
    )));

    hints
}

fn joined(items: &BTreeSet<String>) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

fn joined_slice(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------ content

    #[test]
    fn content_strips_only_what_am_wrote() {
        let policy = json!({
            "name": "P",
            "active": true,
            "_rev": "17",
            "createdBy": "id=x",
            "creationDate": "2026-08-25T08:43:19.497Z",
            "lastModifiedBy": "id=x",
            "lastModifiedDate": "2026-08-25T08:43:19.497Z",
            "editable": true,
        });
        assert_eq!(content(&policy), json!({"name": "P", "active": true}));
    }

    #[test]
    fn a_policy_that_only_differs_by_its_audit_fields_is_unchanged() {
        let a = json!({"name": "P", "resources": ["https://x:443/a"], "lastModifiedDate": "1"});
        let b = json!({"name": "P", "resources": ["https://x:443/a"], "lastModifiedDate": "2"});
        assert!(content_equal(&a, &b));
        let c = json!({"name": "P", "resources": ["https://x:443/b"], "lastModifiedDate": "1"});
        assert!(!content_equal(&a, &c));
    }

    // ------------------------------------------------------------ globbing
    //
    // Every case here is a row from the live probe recorded in
    // `docs/api/21-am-policies.md`. If AM's behaviour changes, these are the
    // tests that should be re-measured rather than adjusted.

    #[test]
    fn star_crosses_a_slash_and_a_single_level_wildcard_does_not() {
        assert!(resource_matches("https://*:*/g/*", "https://x:443/g/one"));
        assert!(resource_matches(
            "https://*:*/g/*",
            "https://x:443/g/one/two"
        ));
        assert!(resource_matches("https://*:*/h/-*-", "https://x:443/h/one"));
        assert!(!resource_matches(
            "https://*:*/h/-*-",
            "https://x:443/h/one/two"
        ));
    }

    #[test]
    fn a_mid_pattern_star_crosses_a_slash_too() {
        assert!(resource_matches(
            "https://*:*/i/*/z",
            "https://x:443/i/one/z"
        ));
        assert!(resource_matches(
            "https://*:*/i/*/z",
            "https://x:443/i/one/two/z"
        ));
    }

    #[test]
    fn star_matches_zero_characters_but_not_the_literal_slash_before_it() {
        assert!(resource_matches("https://*:*/g/*", "https://x:443/g/"));
        assert!(!resource_matches("https://*:*/g/*", "https://x:443/g"));
        assert!(!resource_matches("https://*:*/t/*/", "https://x:443/t/one"));
        assert!(resource_matches("https://*:*/t/*/", "https://x:443/t/one/"));
    }

    #[test]
    fn matching_is_case_insensitive_including_the_path() {
        assert!(resource_matches(
            "https://*:*/lit/One",
            "https://x:443/LIT/one"
        ));
    }

    #[test]
    fn a_query_string_is_part_of_the_resource() {
        // The discriminating pair: a path wildcard does *not* swallow `?b=1`,
        // which is the trap a PEP falls into by passing the raw request URL.
        assert!(!resource_matches(
            "https://*:*/g/*",
            "https://x:443/g/one?b=1"
        ));
        assert!(resource_matches(
            "https://*:*/q/a?*",
            "https://x:443/q/a?b=1"
        ));
        assert!(!resource_matches("https://*:*/q/a?*", "https://x:443/q/a"));
    }

    #[test]
    fn a_missing_port_is_defaulted_by_scheme_and_the_scheme_itself_is_literal() {
        assert!(resource_matches("https://*:*/g/*", "https://x/g/one"));
        assert!(!resource_matches("https://*:*/g/*", "http://x:80/g/one"));
        // An unknown scheme has no default to apply, so it stays unmatched —
        // which is what `shop://orders/123` did against `shop://orders/*`.
        assert!(!resource_matches(
            "shop://*:*/orders/*",
            "shop://orders/123"
        ));
    }

    // ----------------------------------------------------------- diagnosis

    fn shop_types() -> Vec<Value> {
        vec![json!({
            "name": "Shop",
            "patterns": ["https://*:*/orders/*"],
            "actions": {"read": true, "approve": false},
        })]
    }

    fn decision<'a>(
        resource: &'a str,
        actions: &'a Map<String, Value>,
        wanted: &'a [String],
        types: &'a [Value],
        policies: &'a [Value],
        kind: SubjectKind,
        env: &'a BTreeSet<String>,
    ) -> Decision<'a> {
        Decision {
            resource,
            actions,
            wanted,
            resource_types: types,
            policies,
            subject_kind: kind,
            environment_keys: env,
            subject_claims: None,
        }
    }

    fn texts(hints: &[Hint]) -> String {
        hints
            .iter()
            .map(|hint| hint.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_granted_decision_produces_no_hints() {
        let actions = json!({"read": true}).as_object().unwrap().clone();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(hints.is_empty(), "{}", texts(&hints));
    }

    #[test]
    fn a_query_string_is_called_out_before_the_generic_no_match() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![];
        let hints = diagnose(&decision(
            "https://x:443/orders/1?expand=lines",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(texts(&hints).contains("query string"), "{}", texts(&hints));
    }

    #[test]
    fn a_portless_resource_on_a_scheme_am_cannot_default_is_called_out() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![];
        let hints = diagnose(&decision(
            "shop://orders/123",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(texts(&hints).contains("names no port"), "{}", texts(&hints));

        // The control: https has a default, so a portless https resource must
        // NOT get that hint — AM matches it fine.
        let hints = diagnose(&decision(
            "https://x/payments/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(
            !texts(&hints).contains("names no port"),
            "{}",
            texts(&hints)
        );
    }

    #[test]
    fn a_matching_resource_with_no_policy_says_so_rather_than_blaming_the_subject() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Payments",
            "resources": ["https://*:*/payments/*"],
            "subject": {"type": "JwtClaim", "claimName": "x", "claimValue": "y"},
        })];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        let text = texts(&hints);
        assert!(
            text.contains("no policy in the set lists a matching"),
            "{text}"
        );
        assert!(!text.contains("subject or a condition"), "{text}");
    }

    #[test]
    fn a_jwt_subject_against_an_authenticated_users_policy_is_named() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        // Nested inside an AND, because the real trees are recursive and a
        // top-level-only check would miss exactly this shape.
        let policies = vec![json!({
            "name": "Orders",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "AND", "subjects": [
                {"type": "AuthenticatedUsers"},
                {"type": "JwtClaim", "claimName": "scope", "claimValue": "orders.read"},
            ]},
        })];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        let text = texts(&hints);
        assert!(text.contains("AuthenticatedUsers"), "{text}");
        assert!(text.contains("never satisfies"), "{text}");
    }

    #[test]
    fn the_same_policy_under_a_caller_subject_does_not_get_that_hint() {
        // The discriminating control for the test above: the hint must key on
        // the subject form, not merely on the policy mentioning the type.
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Orders",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "AuthenticatedUsers"},
        })];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Caller,
            &env,
        ));
        assert!(
            !texts(&hints).contains("never satisfies"),
            "{}",
            texts(&hints)
        );
    }

    #[test]
    fn an_oauth2_scope_condition_with_no_environment_scope_is_named() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Orders",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "JwtClaim", "claimName": "scope", "claimValue": "orders.read"},
            "condition": {"type": "OAuth2Scope", "requiredScopes": ["orders.read"]},
        })];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(
            texts(&hints).contains("environment.scope"),
            "{}",
            texts(&hints)
        );

        let mut supplied = BTreeSet::new();
        supplied.insert("scope".to_string());
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &supplied,
        ));
        assert!(
            !texts(&hints).contains("environment.scope"),
            "{}",
            texts(&hints)
        );
    }

    #[test]
    fn an_undeclared_action_is_reported_even_when_something_else_was_granted() {
        let actions = json!({"read": true}).as_object().unwrap().clone();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![];
        let wanted = vec!["refund".to_string()];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &wanted,
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(
            texts(&hints).contains("not declared by any resource type"),
            "{}",
            texts(&hints)
        );
    }

    #[test]
    fn the_failing_jwt_claim_is_named_along_with_what_the_token_carries() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Approve",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "AND", "subjects": [
                {"type": "JwtClaim", "claimName": "demoRoles", "claimValue": "orders.approver"},
                {"type": "JwtClaim", "claimName": "scope", "claimValue": "orders.approve"},
            ]},
        })];
        let claims = json!({
            "demoRoles": ["orders.reader"],
            "scope": ["orders.read"],
        })
        .as_object()
        .unwrap()
        .clone();
        let mut decision = decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        );
        decision.subject_claims = Some(&claims);
        let text = texts(&diagnose(&decision));
        assert!(
            text.contains(r#"wants claim demoRoles="orders.approver""#),
            "{text}"
        );
        assert!(
            text.contains(r#"wants claim scope="orders.approve""#),
            "{text}"
        );
        assert!(text.contains("orders.reader"), "{text}");
    }

    #[test]
    fn a_claim_the_token_does_satisfy_is_not_reported() {
        // The discriminating control: AM matches inside an array claim, so a
        // token holding the value among others must not be flagged. Without
        // this the diff would report every multi-valued claim as a failure.
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Approve",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "AND", "subjects": [
                {"type": "JwtClaim", "claimName": "demoRoles", "claimValue": "orders.approver"},
                {"type": "JwtClaim", "claimName": "scope", "claimValue": "orders.approve"},
            ]},
        })];
        let claims = json!({
            "demoRoles": ["orders.reader", "orders.approver"],
            "scope": ["orders.approve"],
        })
        .as_object()
        .unwrap()
        .clone();
        let mut decision = decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        );
        decision.subject_claims = Some(&claims);
        let text = texts(&diagnose(&decision));
        assert!(!text.contains("wants claim"), "{text}");
    }

    #[test]
    fn a_missing_claim_reads_differently_from_a_wrong_one() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Approve",
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "JwtClaim", "claimName": "demoRoles", "claimValue": "x"},
        })];
        let claims = json!({"scope": ["orders.read"]})
            .as_object()
            .unwrap()
            .clone();
        let mut decision = decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        );
        decision.subject_claims = Some(&claims);
        assert!(
            texts(&diagnose(&decision)).contains("no such claim"),
            "{}",
            texts(&diagnose(&decision))
        );
    }

    #[test]
    fn an_inactive_policy_is_called_out() {
        let actions = Map::new();
        let env = BTreeSet::new();
        let types = shop_types();
        let policies = vec![json!({
            "name": "Orders",
            "active": false,
            "resources": ["https://*:*/orders/*"],
            "subject": {"type": "JwtClaim", "claimName": "scope", "claimValue": "orders.read"},
        })];
        let hints = diagnose(&decision(
            "https://x:443/orders/1",
            &actions,
            &[],
            &types,
            &policies,
            SubjectKind::Jwt,
            &env,
        ));
        assert!(texts(&hints).contains("is inactive"), "{}", texts(&hints));
    }
}
