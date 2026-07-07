//! Tenant onboarding: verified bootstrap patterns plus direct sandbox import
//! from the development environment.
//!
//! Layout — one file per flow, each owning its whole lifecycle:
//! - [`cookie`], [`userpass`], [`paste`], and [`log_only`] each own their form
//!   struct, key handler, bootstrap task(s), and completion handler. To edit a
//!   flow, edit only its file.
//! - [`screen`] is menu + dispatch: the nested `Mode`/`Event`/`PendingConfirm`/
//!   `ProdAction` enums, `apply_event`, the `handle_key` dispatch (one arm per
//!   flow), the menu key handler, the overwrite-confirm handler, the cross-flow
//!   auth-progress/error handlers, and the tiny envrc import.
//! - [`common`] holds cross-flow glue: the ordered `MENU` slice that drives
//!   selection + numbering (no tuple-index math), tenant persistence, the
//!   shared service-account completion handler, and small form/confirm helpers.
//! - [`bootstrap`] drives delegated OAuth2, RSA key generation, service-account
//!   creation, and the session→bearer→mint log-key plumbing. That mint flow is
//!   also called by `aic logs key create`, so keep it provider-agnostic here.
//! - [`view`] renders the menu, forms, and duplicate-name confirmation.
//!
//! To add a flow: new file (form + keys + bootstrap + completion), one entry in
//! `common::MENU`, and one arm each in the `Mode`, `Event`, `PendingConfirm`,
//! and `handle_key`/`apply_event` dispatch in `screen`.
//!
//! Onboarding persists tenant metadata through [`crate::config`] and private
//! JWKs through [`crate::app::App`], uses the shared production-write guard,
//! and refreshes ESVs after a tenant is added. Form widgets and modal chrome
//! remain shared UI infrastructure; the per-pattern form state is internal to
//! this vertical, removing the former AIC-layer dependency on UI code.
//!
//! API ground truth: `docs/api/00-auth.md` and
//! `docs/api/99-quirks-and-open-questions.md` Q11/Q12. A browser-handoff PKCE
//! flow cannot bootstrap root-realm platform admins because AIC rejects
//! loopback redirects and blocks root-realm OAuth2 client management, so the
//! verified cookie, username/password, and existing-service-account patterns
//! are used instead.

pub mod bootstrap;
pub mod common;
pub mod cookie;
pub mod log_only;
pub mod paste;
pub mod screen;
pub mod userpass;
pub mod view;

/// Which onboarding pattern the user chose. Index into the menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardPath {
    Cookie,
    UserPass,
    Paste,
    Envrc,
    LogOnly,
}

/// Strip scheme + path + trailing slash from a user-supplied domain field.
/// Other tools (e.g. frodo) expect a URL with `/am` on the end; pingone-aic-manager always
/// asks for just the hostname so the user doesn't have to guess.
pub fn normalise_domain(input: &str) -> String {
    let s = input.trim();
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = match s.find('/') {
        Some(i) => &s[..i],
        None => s,
    };
    s.trim_end_matches('/').to_string()
}

pub fn domain_to_base_url(domain: &str) -> String {
    format!("https://{}", normalise_domain(domain))
}

pub fn validate_domain(input: &str) -> std::result::Result<String, String> {
    let d = normalise_domain(input);
    if d.is_empty() {
        return Err("Tenant hostname is required".into());
    }
    if !d.contains('.') {
        return Err("Tenant hostname looks wrong (no dot)".into());
    }
    Ok(d)
}
