//! `.aic-edit/wraps.toml` — the file that records how the data-encryption key
//! (DEK) is wrapped for each enrolled unlock method. The TOML schema is
//! human-readable on purpose; the cryptographic material is base64 strings.
//!
//! ```toml
//! version = 1
//!
//! [[wrap]]
//! method = "password"
//! salt = "<base64 — 16-byte Argon2 salt>"
//! nonce = "<base64 — 12-byte AES-GCM nonce>"
//! ciphertext = "<base64 — wrapped DEK + tag>"
//!
//! [[wrap]]
//! method = "yubikey"
//! label = "Yubikey 5C NFC"           # optional, user-supplied
//! credential_id = "<base64>"          # FIDO2 credential id
//! rp_id = "aic-edit"
//! hmac_salt = "<base64 — 32 bytes>"   # input to hmac-secret
//! nonce = "<base64 — 12 bytes>"
//! ciphertext = "<base64 — wrapped DEK + tag>"
//! ```

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
    Yubikey {
        #[serde(default)]
        label: Option<String>,
        credential_id: String,
        rp_id: String,
        hmac_salt: String,
        nonce: String,
        ciphertext: String,
    },
}

impl Wrap {
    pub fn kind(&self) -> WrapKind {
        match self {
            Wrap::Password { .. } => WrapKind::Password,
            Wrap::Yubikey { .. } => WrapKind::Yubikey,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Wrap::Password { .. } => "Master password".into(),
            Wrap::Yubikey { label, .. } => label
                .clone()
                .unwrap_or_else(|| "Yubikey".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Password,
    Yubikey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapsFile {
    pub version: u32,
    #[serde(rename = "wrap", default)]
    pub wraps: Vec<Wrap>,
}

impl Default for WrapsFile {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
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

    pub fn yubikey_wraps(&self) -> impl Iterator<Item = &Wrap> {
        self.wraps
            .iter()
            .filter(|w| matches!(w, Wrap::Yubikey { .. }))
    }

    pub fn has_yubikey(&self) -> bool {
        self.yubikey_wraps().next().is_some()
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

    pub fn push_yubikey(&mut self, wrap: Wrap) {
        debug_assert!(matches!(wrap, Wrap::Yubikey { .. }));
        self.wraps.push(wrap);
    }
}

/// Small base64 helpers so call-sites don't depend on the engine directly.
pub fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(Error::from)
}
