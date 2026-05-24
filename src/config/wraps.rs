//! `.aic-edit/wraps.toml` — the file that records how the data-encryption key
//! (DEK) is wrapped for each enrolled unlock method. The TOML schema is
//! human-readable on purpose; the cryptographic material is base64 strings.
//!
//! ```toml
//! version = 1
//! security_key_hmac_salt = "<base64 — 32 bytes, shared across security-key wraps>"
//!
//! [[wrap]]
//! method = "password"
//! salt = "<base64 — 16-byte Argon2 salt>"
//! nonce = "<base64 — 12-byte AES-GCM nonce>"
//! ciphertext = "<base64 — wrapped DEK + tag>"
//!
//! [[wrap]]
//! method = "security_key"
//! label = "Security key 5C NFC"        # optional, user-supplied
//! credential_id = "<base64>"           # FIDO2 credential id
//! rp_id = "aic-edit"
//! nonce = "<base64 — 12 bytes>"
//! ciphertext = "<base64 — wrapped DEK + tag>"
//! ```
//!
//! The hmac-secret salt is stored once at the top of the file rather than
//! per-wrap so an unlock can be done in a single `getAssertion` call with
//! an `allowList` of every enrolled credential — the device returns the
//! one it actually holds. Per-credential salts would force us to either
//! iterate (extra taps) or call twice (discover then derive).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::config::ProjectConfig;
use crate::{Error, Result};

const FILE_VERSION: u32 = 1;

/// One unlock method's record. Tagged on the `method` field so the TOML stays
/// readable and so new methods don't break old files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum Wrap {
    Password {
        salt: String,
        nonce: String,
        ciphertext: String,
    },
    /// Explicit serde tag: the default `rename_all = "lowercase"` would emit
    /// `method = "securitykey"`; the snake-cased form reads better in
    /// `wraps.toml`.
    #[serde(rename = "security_key")]
    SecurityKey {
        #[serde(default)]
        label: Option<String>,
        credential_id: String,
        rp_id: String,
        nonce: String,
        ciphertext: String,
    },
}

impl Wrap {
    pub fn kind(&self) -> WrapKind {
        match self {
            Wrap::Password { .. } => WrapKind::Password,
            Wrap::SecurityKey { .. } => WrapKind::SecurityKey,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Wrap::Password { .. } => "Master password".into(),
            Wrap::SecurityKey { label, .. } => label
                .clone()
                .unwrap_or_else(|| "Security key".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Password,
    SecurityKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapsFile {
    pub version: u32,
    /// Base64 of the 32-byte salt fed to the hmac-secret extension on every
    /// security-key unlock. Set once when the first security key is enrolled
    /// and reused for every subsequent enrolment. `None` when no security key
    /// has ever been enrolled (or after all have been removed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_key_hmac_salt: Option<String>,
    #[serde(rename = "wrap", default)]
    pub wraps: Vec<Wrap>,
}

impl Default for WrapsFile {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            security_key_hmac_salt: None,
            wraps: Vec::new(),
        }
    }
}

impl WrapsFile {
    pub fn load() -> Result<Option<Self>> {
        let path = ProjectConfig::wraps_path();
        if !path.exists() {
            return Ok(None);
        }
        let body = fs::read_to_string(&path)?;
        let parsed: Self = toml::from_str(&body)?;
        Ok(Some(parsed))
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(ProjectConfig::dir())?;
        let body = toml::to_string_pretty(self)?;
        fs::write(ProjectConfig::wraps_path(), body)?;
        ProjectConfig::write_gitignore()?;
        Ok(())
    }

    pub fn password_wrap(&self) -> Option<&Wrap> {
        self.wraps
            .iter()
            .find(|w| matches!(w, Wrap::Password { .. }))
    }

    pub fn security_key_wraps(&self) -> impl Iterator<Item = &Wrap> {
        self.wraps
            .iter()
            .filter(|w| matches!(w, Wrap::SecurityKey { .. }))
    }

    pub fn has_security_key(&self) -> bool {
        self.security_key_wraps().next().is_some()
    }

    pub fn has_password(&self) -> bool {
        self.password_wrap().is_some()
    }

    /// Replace the password wrap (or append if none exists yet).
    pub fn upsert_password(&mut self, wrap: Wrap) {
        debug_assert!(matches!(wrap, Wrap::Password { .. }));
        if let Some(idx) = self
            .wraps
            .iter()
            .position(|w| matches!(w, Wrap::Password { .. }))
        {
            self.wraps[idx] = wrap;
        } else {
            self.wraps.push(wrap);
        }
    }

    pub fn push_security_key(&mut self, wrap: Wrap) {
        debug_assert!(matches!(wrap, Wrap::SecurityKey { .. }));
        self.wraps.push(wrap);
    }

    /// Return the shared hmac-secret salt for security keys, generating one
    /// if no security key has ever been enrolled. The salt is stored in
    /// `self.security_key_hmac_salt` (as base64) — the caller is responsible
    /// for `.save()`ing the file after a successful enrolment.
    pub fn get_or_create_security_key_salt(&mut self) -> [u8; 32] {
        if let Some(b64) = &self.security_key_hmac_salt {
            if let Ok(bytes) = b64_decode(b64) {
                if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
                    return arr;
                }
            }
        }
        use rand::RngCore;
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        self.security_key_hmac_salt = Some(b64_encode(&salt));
        salt
    }

    /// If no security-key wraps remain, also drop the shared salt — the
    /// next enrolment generates a fresh one. Call after removing a wrap.
    pub fn clear_security_key_salt_if_unused(&mut self) {
        if !self.has_security_key() {
            self.security_key_hmac_salt = None;
        }
    }
}

/// Small base64 helpers so call-sites don't depend on the engine directly.
pub fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(Error::from)
}
