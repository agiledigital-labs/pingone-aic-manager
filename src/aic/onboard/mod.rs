pub mod bootstrap;
pub mod cookie;
pub mod paste;
pub mod userpass;

/// Which onboarding pattern the user chose. Index into the menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardPath {
    Cookie,
    UserPass,
    Paste,
    Envrc,
}

/// Strip scheme + path + trailing slash from a user-supplied domain field.
/// Other tools (e.g. frodo) expect a URL with `/am` on the end; aic-edit always
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
