use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::{BigUint, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::tenant::Tenant;
use crate::{Error, Result};

/// What AIC issues, when it does not say otherwise (`docs/api/00-auth.md`).
/// It is a ceiling on every TTL floor below: no caller can ask for more life
/// than the tenant hands out.
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
    pub fn get_with_min_ttl(&self, min_ttl: i64) -> Option<&str> {
        if self.expires_at > unix_now() + min_ttl {
            self.token.as_deref()
        } else {
            None
        }
    }

    pub fn get_valid(&self) -> Option<&str> {
        self.get_with_min_ttl(MIN_TTL_FOR_OUR_OWN_REQUEST)
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

pub(crate) fn jwk_to_encoding_key(jwk: &serde_json::Value) -> Result<EncodingKey> {
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

    // jsonwebtoken's `rust_crypto` feature expects PKCS#1 DER, not PKCS#8.
    // PKCS#8 starts with `SEQUENCE { INTEGER version, SEQUENCE AlgorithmId, ... }`,
    // and the inner SEQUENCE at byte 5 trips its PKCS#1 parser.
    let der = private_key
        .to_pkcs1_der()
        .map_err(|e| Error::Rsa(e.to_string()))?;
    Ok(EncodingKey::from_rsa_der(der.as_bytes()))
}

/// Mint a service-account access token using RS256 JWT assertion.
/// Returns (access_token, expires_at_unix_seconds).
pub async fn mint_token(
    client: &reqwest::Client,
    tenant: &Tenant,
    jwk: &serde_json::Value,
) -> Result<(String, i64)> {
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
    let expires_in = json["expires_in"].as_i64().unwrap_or(ISSUED_TOKEN_TTL);
    let expires_at = now + expires_in;

    Ok((access_token, expires_at))
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

    /// The floor is the caller's. A token with ~8 minutes left is fine for a
    /// request we are about to send and NOT fine for one we hand to a script
    /// that will hold it — an implementation that ignores `min_ttl` passes the
    /// first of these and fails the second.
    #[test]
    fn a_higher_floor_rejects_a_token_a_lower_floor_accepts() {
        let mut cache = TokenCache::new();
        cache.store("t".into(), unix_now() + 500);

        assert_eq!(
            cache.get_with_min_ttl(MIN_TTL_FOR_OUR_OWN_REQUEST),
            Some("t")
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
}
