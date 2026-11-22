use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PrivateKeyParts;
use rsa::{BigUint, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::tenant::Tenant;
use crate::http::RequestBuilderExt;
use crate::{Error, Result};

/// Nominal issued lifetime, used only for proactive refresh heuristics.
pub const ISSUED_TOKEN_TTL: i64 = 898;

/// Enough life for a request we are about to make ourselves. `bearer()`'s
/// token is used within milliseconds, so the only thing this has to cover is
/// the round trip.
pub const MIN_TTL_FOR_OUR_OWN_REQUEST: i64 = 60;

pub struct TokenCache {
    token: Option<String>,
    expires_at: i64,
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            token: None,
            expires_at: 0,
        }
    }

    /// The cached token, if it still has at least `min_ttl` seconds to live.
    ///
    /// The floor is the caller's, not the cache's: a request about to be sent
    /// needs only enough life to arrive, while a token handed to an external
    /// process is out of our hands the moment we print it and has to outlive
    /// whatever that process does with it.
    pub fn get_with_min_ttl(&self, min_ttl: i64) -> Option<(&str, i64)> {
        let now = unix_now();
        if self.expires_at > now && self.expires_at >= now + min_ttl {
            self.token.as_deref().map(|token| (token, self.expires_at))
        } else {
            None
        }
    }

    pub fn get_valid(&self) -> Option<&str> {
        self.get_with_min_ttl(MIN_TTL_FOR_OUR_OWN_REQUEST)
            .map(|(token, _)| token)
    }

    pub fn store(&mut self, token: String, expires_at: i64) {
        self.token = Some(token);
        self.expires_at = expires_at;
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Serialize, Deserialize)]
struct JwtClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    jti: String,
}

/// Rebuild the RSA key a private JWK describes. `p`/`q` are used when present
/// and recovered from `n`/`e`/`d` when not.
pub(crate) fn jwk_to_private_key(jwk: &serde_json::Value) -> Result<RsaPrivateKey> {
    let n_b64 = jwk["n"]
        .as_str()
        .ok_or_else(|| Error::Auth("JWK missing 'n'".into()))?;
    let e_b64 = jwk["e"]
        .as_str()
        .ok_or_else(|| Error::Auth("JWK missing 'e'".into()))?;
    let d_b64 = jwk["d"]
        .as_str()
        .ok_or_else(|| Error::Auth("JWK missing 'd'".into()))?;

    let n = BigUint::from_bytes_be(&B64URL.decode(n_b64)?);
    let e = BigUint::from_bytes_be(&B64URL.decode(e_b64)?);
    let d = BigUint::from_bytes_be(&B64URL.decode(d_b64)?);

    let private_key = if let (Some(p_b64), Some(q_b64)) = (jwk["p"].as_str(), jwk["q"].as_str()) {
        let p = BigUint::from_bytes_be(&B64URL.decode(p_b64)?);
        let q = BigUint::from_bytes_be(&B64URL.decode(q_b64)?);
        RsaPrivateKey::from_components(n, e, d, vec![p, q])
            .map_err(|e| Error::Rsa(e.to_string()))?
    } else {
        RsaPrivateKey::from_components(n, e, d, vec![]).map_err(|e| Error::Rsa(e.to_string()))?
    };
    Ok(private_key)
}

/// The RFC 7518 §6.3.2 private members after `d`: both primes and the three
/// CRT values. JOSE consumers may refuse a private JWK without them — Node's
/// `crypto.createPrivateKey` rejects one outright as "Invalid JWK RSA key" — so
/// every private JWK `aic` hands out carries all five.
pub(crate) fn rsa_crt_members(key: &RsaPrivateKey) -> Result<[(&'static str, String); 5]> {
    let missing = || Error::Rsa("RSA key has no CRT parameters".into());
    let [p, q] = key.primes() else {
        return Err(Error::Rsa("RSA key is not a two-prime key".into()));
    };
    let encode = |value: &BigUint| B64URL.encode(value.to_bytes_be());
    Ok([
        ("p", encode(p)),
        ("q", encode(q)),
        ("dp", encode(key.dp().ok_or_else(missing)?)),
        ("dq", encode(key.dq().ok_or_else(missing)?)),
        ("qi", encode(&key.crt_coefficient().ok_or_else(missing)?)),
    ])
}

pub(crate) fn jwk_to_encoding_key(jwk: &serde_json::Value) -> Result<EncodingKey> {
    let private_key = jwk_to_private_key(jwk)?;

    // jsonwebtoken's `rust_crypto` feature expects PKCS#1 DER, not PKCS#8.
    // PKCS#8 starts with `SEQUENCE { INTEGER version, SEQUENCE AlgorithmId, ... }`,
    // and the inner SEQUENCE at byte 5 trips its PKCS#1 parser.
    let der = private_key
        .to_pkcs1_der()
        .map_err(|e| Error::Rsa(e.to_string()))?;
    Ok(EncodingKey::from_rsa_der(der.as_bytes()))
}

/// A token the tenant just issued. `scopes` is the `scope` field of that
/// response, not the list we asked for.
#[derive(Debug)]
pub struct MintedToken {
    pub access_token: String,
    pub expires_at: i64,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

/// Refuse a floor the freshly minted token cannot meet, using the lifetime
/// reported by the tenant rather than a nominal default.
pub fn check_minted_min_ttl(expires_in: i64, min_ttl: i64) -> Result<()> {
    if expires_in < min_ttl {
        return Err(Error::Auth(format!(
            "fresh token lifetime is {expires_in}s, below requested --min-ttl {min_ttl}s"
        )));
    }
    Ok(())
}

/// Mint a service-account access token using RS256 JWT assertion.
pub async fn mint_token(
    client: &reqwest::Client,
    tenant: &Tenant,
    jwk: &serde_json::Value,
) -> Result<MintedToken> {
    let sa_id = tenant.sa_id.as_ref().ok_or_else(|| {
        Error::Auth(format!(
            "tenant {} is log-only (no service account); cannot mint a bearer token",
            tenant.name
        ))
    })?;
    let now = unix_now();
    let aud = format!("{}/am/oauth2/access_token", tenant.base_url);

    let claims = JwtClaims {
        iss: sa_id.clone(),
        sub: sa_id.clone(),
        aud: aud.clone(),
        iat: now,
        exp: now + 300,
        jti: Uuid::new_v4().to_string(),
    };

    let mut header = Header::new(Algorithm::RS256);
    // Only set `kid` when the JWK actually carries one. Setting kid to the
    // SA UUID as a fallback breaks external SAs whose registered kid is
    // something the AIC console picked (we don't know what it is). Omitting
    // kid entirely lets the server try every key registered for the SA —
    // matches the verify-endpoint.sh reference implementation.
    header.kid = jwk["kid"].as_str().map(|s| s.to_string());

    let encoding_key = jwk_to_encoding_key(jwk)?;
    let assertion = jsonwebtoken::encode(&header, &claims, &encoding_key)
        .map_err(|e| Error::Auth(e.to_string()))?;

    let scope = tenant.scopes.join(" ");
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
        .append_pair("client_id", "service-account")
        .append_pair("assertion", &assertion)
        .append_pair("scope", &scope)
        .finish();

    let resp = client
        .post(&aud)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .aic_transaction_id()
        .body(body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Api {
            status: status.as_u16(),
            body,
        });
    }

    let json: serde_json::Value = resp.json().await?;
    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| Error::Auth("no access_token in response".into()))?
        .to_string();
    let expires_in = json["expires_in"]
        .as_i64()
        .ok_or_else(|| Error::Auth("no expires_in in token response".into()))?;
    let expires_at = now + expires_in;
    let scopes = parse_issued_scopes(&json)?;

    Ok(MintedToken {
        access_token,
        expires_at,
        expires_in,
        scopes,
    })
}

/// The `scope` field of a token response. A string is the grant's form; an
/// array is what `/tokeninfo` returns. Anything else is refused rather than
/// treated as the scopes we requested.
pub fn parse_issued_scopes(body: &serde_json::Value) -> Result<Vec<String>> {
    match body.get("scope") {
        Some(serde_json::Value::String(text)) => {
            Ok(text.split_whitespace().map(str::to_string).collect())
        }
        Some(serde_json::Value::Array(items)) => {
            let mut scopes = Vec::with_capacity(items.len());
            for scope in items {
                let Some(scope) = scope.as_str() else {
                    return Err(Error::Auth("token scope has a non-string entry".into()));
                };
                scopes.push(scope.to_string());
            }
            Ok(scopes)
        }
        _ => Err(Error::Auth("token response has no scope".into())),
    }
}

/// A tenant whose token request carries `requested` and nothing else.
///
/// Every name has to already be in `tenant.scopes` (the list this install
/// asks for). The grant refuses a scope the service account does not hold
/// with `400 invalid_scope` and issues no token, so an unknown name is
/// rejected here before that call. Duplicates are rejected too.
pub fn downscoped_tenant(tenant: &Tenant, requested: &[String]) -> Result<Tenant> {
    if requested.is_empty() {
        return Err(Error::Config(
            "whoami --scope needs at least one scope".into(),
        ));
    }
    let mut narrowed = Vec::with_capacity(requested.len());
    let mut unknown = Vec::new();
    for scope in requested {
        if narrowed.iter().any(|seen| seen == scope) {
            return Err(Error::Config(format!(
                "scope {scope} was given to whoami --scope more than once"
            )));
        }
        if !tenant.scopes.iter().any(|have| have == scope) {
            unknown.push(scope.as_str());
        }
        narrowed.push(scope.clone());
    }
    if !unknown.is_empty() {
        return Err(Error::Config(format!(
            "whoami --scope only narrows scopes this install already requests. {} {} not \
             among them (`aic ctx scopes`). A scope the service account does not hold fails \
             the whole token with 400 invalid_scope and is not dropped",
            unknown.join(", "),
            if unknown.len() == 1 { "is" } else { "are" },
        )));
    }
    let mut tenant = tenant.clone();
    tenant.scopes = narrowed;
    Ok(tenant)
}

/// Extract the public JWK fields from a private JWK.
pub fn public_jwk(private_jwk: &serde_json::Value) -> serde_json::Value {
    let mut pub_jwk = serde_json::json!({
        "kty": private_jwk["kty"],
        "use": "sig",
        "alg": "RS256",
        "n":   private_jwk["n"],
        "e":   private_jwk["e"],
    });
    if let Some(kid) = private_jwk["kid"].as_str() {
        pub_jwk["kid"] = serde_json::Value::String(kid.to_string());
    }
    pub_jwk
}

#[cfg(test)]
mod tests {
    use super::{MIN_TTL_FOR_OUR_OWN_REQUEST, TokenCache, unix_now};

    #[test]
    fn minted_lifetime_is_the_min_ttl_ceiling() {
        assert!(super::check_minted_min_ttl(899, 899).is_ok());
        let error = super::check_minted_min_ttl(899, 900).unwrap_err();
        assert!(error.to_string().contains("lifetime is 899s"));
    }

    /// The floor is the caller's. A token with ~8 minutes left is fine for a
    /// request we are about to send and NOT fine for one we hand to a script
    /// that will hold it — an implementation that ignores `min_ttl` passes the
    /// first of these and fails the second.
    #[test]
    fn a_higher_floor_rejects_a_token_a_lower_floor_accepts() {
        let mut cache = TokenCache::new();
        let expires_at = unix_now() + 500;
        cache.store("t".into(), expires_at);

        assert_eq!(
            cache.get_with_min_ttl(MIN_TTL_FOR_OUR_OWN_REQUEST),
            Some(("t", expires_at))
        );
        assert_eq!(cache.get_with_min_ttl(840), None);
    }

    #[test]
    fn a_token_past_its_floor_is_withheld_even_though_it_has_not_expired() {
        let mut cache = TokenCache::new();
        cache.store("t".into(), unix_now() + 30);

        // Still valid on the wire for another 30s, and still refused: the
        // point of the floor is to not hand out a token that dies mid-use.
        assert!(cache.expires_at() > unix_now());
        assert_eq!(cache.get_valid(), None);
    }

    #[test]
    fn cache_reuses_exactly_at_a_positive_ttl_floor_but_not_when_expired_at_zero() {
        let now = unix_now();
        let mut cache = TokenCache::new();
        cache.store("equal".into(), now + 120);
        assert_eq!(cache.get_with_min_ttl(120), Some(("equal", now + 120)));

        cache.store("expired".into(), now - 1);
        assert_eq!(cache.get_with_min_ttl(0), None);
    }

    use super::mint_token;
    use crate::Error;
    use crate::config::{Tenant, TenantTheme};

    #[tokio::test]
    async fn log_only_tenant_is_rejected_before_jwt_construction() {
        let tenant = Tenant {
            name: "logs".into(),
            base_url: "https://example.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
            provenance: crate::config::Provenance::default(),
        };

        let error = mint_token(&reqwest::Client::new(), &tenant, &serde_json::Value::Null)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::Auth(message)
                if message
                    == "tenant logs is log-only (no service account); cannot mint a bearer token"
        ));
    }

    #[test]
    fn issued_scopes_follow_the_token_response_and_not_the_request() {
        let string = super::parse_issued_scopes(&serde_json::json!({
            "scope": "fr:am:* fr:idm:*"
        }))
        .unwrap();
        assert_eq!(string, ["fr:am:*", "fr:idm:*"]);
        let list = super::parse_issued_scopes(&serde_json::json!({
            "scope": ["fr:idm:*"]
        }))
        .unwrap();
        assert_eq!(list, ["fr:idm:*"]);
        assert!(super::parse_issued_scopes(&serde_json::json!({})).is_err());
        assert!(super::parse_issued_scopes(&serde_json::json!({"scope": 1})).is_err());
    }

    #[test]
    fn downscoped_tenant_keeps_only_requested_scopes_that_are_already_configured() {
        let tenant = Tenant {
            name: "sandbox".into(),
            base_url: "https://tenant.example.com".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some("sa".into()),
            scopes: vec!["fr:am:*".into(), "fr:idm:*".into(), "fr:idc:esv:*".into()],
            provenance: crate::config::Provenance::default(),
        };
        let narrowed = super::downscoped_tenant(&tenant, &["fr:idm:*".into()]).unwrap();
        assert_eq!(narrowed.scopes, ["fr:idm:*"]);
        assert_eq!(tenant.scopes.len(), 3);

        let unknown =
            super::downscoped_tenant(&tenant, &["fr:am:*".into(), "fr:zzemu:r3n:*".into()])
                .unwrap_err();
        assert!(unknown.to_string().contains("fr:zzemu:r3n:*"));
        assert!(unknown.to_string().contains("invalid_scope"));
        assert!(super::downscoped_tenant(&tenant, &[]).is_err());
        assert!(super::downscoped_tenant(&tenant, &["fr:am:*".into(), "fr:am:*".into()]).is_err());
    }
}
