use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::{BigUint, RsaPrivateKey};
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::tenant::Tenant;
use crate::{Error, Result};

pub struct TokenCache {
    token: Option<String>,
    expires_at: i64,
}

impl TokenCache {
    pub fn new() -> Self {
        Self { token: None, expires_at: 0 }
    }

    pub fn get_valid(&self) -> Option<&str> {
        let now = unix_now();
        if self.expires_at > now + 60 {
            self.token.as_deref()
        } else {
            None
        }
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

fn jwk_to_encoding_key(jwk: &serde_json::Value) -> Result<EncodingKey> {
    let n_b64 = jwk["n"].as_str().ok_or_else(|| Error::Auth("JWK missing 'n'".into()))?;
    let e_b64 = jwk["e"].as_str().ok_or_else(|| Error::Auth("JWK missing 'e'".into()))?;
    let d_b64 = jwk["d"].as_str().ok_or_else(|| Error::Auth("JWK missing 'd'".into()))?;

    let n = BigUint::from_bytes_be(&B64URL.decode(n_b64)?);
    let e = BigUint::from_bytes_be(&B64URL.decode(e_b64)?);
    let d = BigUint::from_bytes_be(&B64URL.decode(d_b64)?);

    let private_key = if let (Some(p_b64), Some(q_b64)) = (jwk["p"].as_str(), jwk["q"].as_str()) {
        let p = BigUint::from_bytes_be(&B64URL.decode(p_b64)?);
        let q = BigUint::from_bytes_be(&B64URL.decode(q_b64)?);
        RsaPrivateKey::from_components(n, e, d, vec![p, q])
            .map_err(|e| Error::Rsa(e.to_string()))?
    } else {
        RsaPrivateKey::from_components(n, e, d, vec![])
            .map_err(|e| Error::Rsa(e.to_string()))?
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
    let now = unix_now();
    let aud = format!("{}/am/oauth2/access_token", tenant.base_url);

    let claims = JwtClaims {
        iss: tenant.sa_id.clone(),
        sub: tenant.sa_id.clone(),
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
        return Err(Error::Api { status: status.as_u16(), body });
    }

    let json: serde_json::Value = resp.json().await?;
    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| Error::Auth("no access_token in response".into()))?
        .to_string();
    let expires_in = json["expires_in"].as_i64().unwrap_or(898);
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
