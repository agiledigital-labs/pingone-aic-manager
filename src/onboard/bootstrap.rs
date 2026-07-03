//! Shared bootstrap helpers: from an AM session cookie (tokenId), drive the
//! idmAdminClient PKCE flow to obtain a delegated Bearer, mint an RSA keypair,
//! create a service account, and return the resulting (tenant, private JWK).
//!
//! Both Pattern 1 (paste cookie) and Pattern 2 (in-app u/p) end up here once
//! they have a session cookie value. The same session→bearer→resolve username→
//! credential name→log-key mint flow is also reused by the logs vertical.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use rand::RngCore;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, traits::PrivateKeyParts};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

const CLIENT_ID: &str = "idmAdminClient";
const SCOPE: &str = "openid fr:idm:*";

/// Final scopes the SA we create will be granted (and which its own tokens carry).
pub const SA_SCOPES: &[&str] = &[
    "fr:idm:*",
    "fr:am:*",
    "fr:idc:esv:*",
    "fr:idc:cookie-domain:*",
];

pub fn credential_name(username: Option<&str>, tenant_name: &str) -> String {
    format!("aicx-{}", username.unwrap_or(tenant_name))
}

/// Result of minting a log API key from an admin session.
pub struct MintedLogKey {
    pub credential_name: String,
    pub key: crate::logs::LogKeyPair,
}

#[derive(Deserialize)]
struct ServerInfo {
    #[serde(rename = "cookieName")]
    cookie_name: String,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
}

#[derive(Deserialize)]
struct UserInfoResp {
    sub: String,
}

#[derive(Deserialize)]
struct TeamMemberResp {
    #[serde(rename = "userName")]
    user_name: Option<String>,
    mail: Option<String>,
}

#[derive(Deserialize)]
struct CreateSaResp {
    #[serde(rename = "_id")]
    id: String,
}

#[derive(Deserialize)]
struct CreateLogApiKeyResp {
    api_key_id: String,
    api_key_secret: String,
}

/// Discover the tenant-specific AM session cookie name.
pub async fn discover_cookie_name(http: &reqwest::Client, base_url: &str) -> Result<String> {
    let url = format!("{base_url}/am/json/serverinfo/*");
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Api {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let info: ServerInfo = resp.json().await?;
    Ok(info.cookie_name)
}

fn gen_pkce() -> (String, String) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = B64URL.encode(bytes);
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Exchange the AM session for an idmAdminClient access token via PKCE.
/// The HTTP client must NOT follow redirects — the code arrives in a 302 Location.
pub async fn session_to_bearer(
    http_no_redirect: &reqwest::Client,
    base_url: &str,
    cookie_name: &str,
    session_value: &str,
) -> Result<String> {
    let redirect_uri = format!("{base_url}/platform/appAuthHelperRedirect.html");
    let (verifier, challenge) = gen_pkce();

    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", "aic-edit")
        .finish();
    let auth_url = format!("{base_url}/am/oauth2/realms/root/authorize?{query}");
    let resp = http_no_redirect
        .get(&auth_url)
        .header(
            "Cookie",
            format!("{cookie_name}={session_value}; amlbcookie=01"),
        )
        .send()
        .await?;

    let status = resp.status();
    if status.as_u16() != 302 {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Auth(format!(
            "authorize expected 302, got {status}: {body}"
        )));
    }
    let location = resp
        .headers()
        .get("location")
        .ok_or_else(|| Error::Auth("authorize response had no Location header".into()))?
        .to_str()
        .map_err(|e| Error::Auth(format!("bad Location header: {e}")))?
        .to_string();

    let code = extract_query_param(&location, "code").ok_or_else(|| {
        let err = extract_query_param(&location, "error_description")
            .or_else(|| extract_query_param(&location, "error"))
            .unwrap_or_else(|| location.clone());
        Error::Auth(format!("no code in authorize redirect: {err}"))
    })?;

    let token_url = format!("{base_url}/am/oauth2/realms/root/access_token");
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_verifier", &verifier)
        .finish();

    let resp = http_no_redirect
        .post(&token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Api {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let tok: TokenResp = resp.json().await?;
    Ok(tok.access_token)
}

/// Best-effort resolution of the human admin represented by a bootstrap bearer.
pub async fn resolve_admin_username(
    http: &reqwest::Client,
    base_url: &str,
    bearer: &str,
) -> Option<String> {
    let userinfo_url = format!("{base_url}/am/oauth2/realms/root/userinfo");
    let resp = http
        .get(&userinfo_url)
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let userinfo: UserInfoResp = resp.json().await.ok()?;
    let sub = userinfo.sub.trim();
    if sub.is_empty() {
        return None;
    }

    let teammember_url = format!("{base_url}/openidm/managed/teammember/{sub}");
    let resp = http
        .get(&teammember_url)
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let teammember: TeamMemberResp = resp.json().await.ok()?;
    teammember
        .user_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| {
            teammember
                .mail
                .as_deref()
                .map(str::trim)
                .filter(|mail| !mail.is_empty())
        })
        .map(str::to_owned)
}

/// Generate a 2048-bit RSA keypair as a private JWK (with kid).
pub fn generate_rsa_jwk(kid: &str) -> Result<serde_json::Value> {
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, 2048).map_err(|e| Error::Rsa(e.to_string()))?;

    let n = B64URL.encode(key.n().to_bytes_be());
    let e = B64URL.encode(key.e().to_bytes_be());
    let d = B64URL.encode(key.d().to_bytes_be());

    let mut jwk = serde_json::json!({
        "kty": "RSA",
        "alg": "RS256",
        "use": "sig",
        "kid": kid,
        "n": n,
        "e": e,
        "d": d,
    });
    if let Some(primes) = key.primes().get(0..2) {
        jwk["p"] = serde_json::Value::String(B64URL.encode(primes[0].to_bytes_be()));
        jwk["q"] = serde_json::Value::String(B64URL.encode(primes[1].to_bytes_be()));
    }
    Ok(jwk)
}

/// Create a service account via /openidm/managed/svcacct using the bootstrap Bearer.
/// Returns the new SA's UUID.
pub async fn create_service_account(
    http: &reqwest::Client,
    base_url: &str,
    bearer: &str,
    name: &str,
    description: &str,
    public_jwk: &serde_json::Value,
) -> Result<String> {
    let jwks_str = serde_json::to_string(&serde_json::json!({ "keys": [public_jwk] }))?;
    let body = serde_json::json!({
        "name": name,
        "description": description,
        "scopes": SA_SCOPES,
        "accountStatus": "Active",
        "jwks": jwks_str,
    });
    let url = format!("{base_url}/openidm/managed/svcacct?_action=create");
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Api {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let created: CreateSaResp = resp.json().await?;
    Ok(created.id)
}

/// Create a log API key using the bootstrap admin-user bearer.
pub async fn create_log_api_key(
    http: &reqwest::Client,
    base_url: &str,
    bearer: &str,
    name: &str,
) -> Result<crate::logs::LogKeyPair> {
    let url = format!("{base_url}/keys?_action=create");
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Api {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let created: CreateLogApiKeyResp = resp.json().await?;
    Ok(crate::logs::LogKeyPair {
        api_key_id: created.api_key_id,
        api_key_secret: created.api_key_secret,
    })
}

/// Shared log-key mint flow used by logs CLI and onboarding.
pub async fn mint_log_key_from_bearer(
    http: &reqwest::Client,
    base_url: &str,
    tenant_name: &str,
    bearer: &str,
    username_hint: Option<&str>,
) -> Result<MintedLogKey> {
    let username = resolve_admin_username(http, base_url, bearer).await;
    let credential_name = credential_name(username.as_deref().or(username_hint), tenant_name);
    let key = create_log_api_key(http, base_url, bearer, &credential_name).await?;
    Ok(MintedLogKey {
        credential_name,
        key,
    })
}

/// Shared log-key mint flow used by logs CLI and onboarding when only a
/// session cookie is available. `cookie_name` may be supplied by the caller or
/// discovered from serverinfo; `username_hint` is used only when admin-user
/// resolution fails.
pub async fn mint_log_key_via_session(
    http: &reqwest::Client,
    base_url: &str,
    cookie_name: Option<&str>,
    session_value: &str,
    tenant_name: &str,
    username_hint: Option<&str>,
) -> Result<MintedLogKey> {
    let cookie_name = match cookie_name {
        Some(cookie_name) => cookie_name.to_string(),
        None => discover_cookie_name(http, base_url).await?,
    };
    let bearer = session_to_bearer(http, base_url, &cookie_name, session_value).await?;
    mint_log_key_from_bearer(http, base_url, tenant_name, &bearer, username_hint).await
}

/// Build a reqwest client that does NOT follow redirects (so authorize 302 is observable).
pub fn no_redirect_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

fn extract_query_param(url_or_query: &str, key: &str) -> Option<String> {
    let q = match url_or_query.split_once('?') {
        Some((_, q)) => q,
        None => url_or_query,
    };
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_name_uses_username_or_tenant_fallback() {
        assert_eq!(
            credential_name(Some("admin@example.com"), "development"),
            "aicx-admin@example.com"
        );
        assert_eq!(credential_name(None, "development"), "aicx-development");
    }
}
