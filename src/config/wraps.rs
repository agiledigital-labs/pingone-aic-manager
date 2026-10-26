//! `.aic/wraps.toml` — the file that records how the data-encryption key
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
//! rp_id = "aic"
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

use crate::config::{ProjectPaths, project_paths};
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
            Wrap::SecurityKey { label, .. } => {
                label.clone().unwrap_or_else(|| "Security key".into())
            }
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
        project_paths().load_wraps()
    }

    /// Write this file as-is. Only for `enable_encryption`'s `save_wrap`,
    /// which runs under the exclusive vault lock before any file is
    /// encrypted under the wrap's key; every later edit goes through
    /// [`WrapsFile::update`].
    pub fn save_for_transition(&self) -> Result<()> {
        project_paths().save_wraps(self)
    }

    /// Edit the encrypted vault's `wraps.toml` as a transaction: under the
    /// exclusive vault lock, only while the vault is encrypted under `dek`,
    /// and with `change` applied to the file **as it is on disk** — not to
    /// this session's copy, which may predate another session's edit or a
    /// whole disable-and-re-enable. Saving a stale copy whole would drop a
    /// factor added elsewhere, or replace a newer key's wraps with ones for
    /// a key no file is encrypted under. Returns the file as written.
    pub fn update(
        dek: &crate::config::crypto::Dek,
        change: impl FnOnce(&mut WrapsFile) -> Result<()>,
    ) -> Result<WrapsFile> {
        project_paths().update_wraps(dek, change)
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
        if let Some(b64) = &self.security_key_hmac_salt
            && let Ok(bytes) = b64_decode(b64)
            && let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            return arr;
        }
        use rand::RngCore;
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        self.security_key_hmac_salt = Some(b64_encode(&salt));
        salt
    }

    /// Add `wrap`, enrolled by a session whose file held hmac salt `salt`.
    /// A password replaces the password wrap. A security key derived its
    /// wrapping key from `salt`, so it is refused if the file now holds a
    /// different one — it would never unlock.
    pub fn add_factor(&mut self, wrap: Wrap, salt: Option<&str>) -> Result<()> {
        match wrap {
            Wrap::Password { .. } => self.upsert_password(wrap),
            Wrap::SecurityKey { .. } => {
                let salt = salt.ok_or_else(|| {
                    Error::Crypto("security key enrolled without an hmac salt".into())
                })?;
                match self.security_key_hmac_salt.as_deref() {
                    Some(current) if current != salt => {
                        return Err(Error::Crypto(
                            "another aic changed the security-key salt; enrol again".into(),
                        ));
                    }
                    Some(_) => {}
                    None => self.security_key_hmac_salt = Some(salt.to_string()),
                }
                self.push_security_key(wrap);
            }
        }
        Ok(())
    }

    /// Remove the factor `wrap` names (the password, or a security key by
    /// credential id), refused if it would leave no factor — the guard the
    /// screen applied was to its own copy, which another session may have
    /// already shrunk. Gone already is not an error.
    pub fn remove_factor(&mut self, wrap: &Wrap) -> Result<()> {
        let remaining = self.wraps.iter().filter(|w| !same_factor(w, wrap)).count();
        if remaining == 0 {
            return Err(Error::Crypto(
                "that is the vault's last factor now; disable encryption instead".into(),
            ));
        }
        self.wraps.retain(|w| !same_factor(w, wrap));
        self.clear_security_key_salt_if_unused();
        Ok(())
    }

    /// Relabel the security key with `credential_id`.
    pub fn rename_factor(&mut self, credential_id: &str, new_label: &str) -> Result<()> {
        match self.wraps.iter_mut().find(
            |w| matches!(w, Wrap::SecurityKey { credential_id: id, .. } if id == credential_id),
        ) {
            Some(Wrap::SecurityKey { label, .. }) => {
                *label = Some(new_label.to_string());
                Ok(())
            }
            _ => Err(Error::Crypto(
                "that security key is no longer enrolled".into(),
            )),
        }
    }

    /// If no security-key wraps remain, also drop the shared salt — the
    /// next enrolment generates a fresh one. Call after removing a wrap.
    pub fn clear_security_key_salt_if_unused(&mut self) {
        if !self.has_security_key() {
            self.security_key_hmac_salt = None;
        }
    }
}

/// The same enrolled factor: there is one password, and a security key is
/// its credential.
fn same_factor(a: &Wrap, b: &Wrap) -> bool {
    match (a, b) {
        (Wrap::Password { .. }, Wrap::Password { .. }) => true,
        (
            Wrap::SecurityKey {
                credential_id: x, ..
            },
            Wrap::SecurityKey {
                credential_id: y, ..
            },
        ) => x == y,
        _ => false,
    }
}

impl ProjectPaths {
    pub fn load_wraps(&self) -> Result<Option<WrapsFile>> {
        let path = self.wraps_path();
        if !path.exists() {
            return Ok(None);
        }
        let body = fs::read_to_string(&path)?;
        let parsed = toml::from_str(&body)?;
        Ok(Some(parsed))
    }

    /// Replaced, never truncated in place, at mode 600: wraps.toml contains
    /// password-Argon2 salts + security-key credential ids — material an
    /// attacker with read access can use to brute-force or impersonate.
    /// Match the keys.enc / keys.plain protections.
    pub fn save_wraps(&self, wraps: &WrapsFile) -> Result<()> {
        let body = toml::to_string_pretty(wraps)?;
        super::save_private_file(&self.wraps_path(), body.as_bytes())?;
        self.write_gitignore()?;
        Ok(())
    }

    pub(crate) fn update_wraps(
        &self,
        dek: &crate::config::crypto::Dek,
        change: impl FnOnce(&mut WrapsFile) -> Result<()>,
    ) -> Result<WrapsFile> {
        let _vault = self.lock_vault(super::Hold::Exclusive)?;
        self.vault_mode_is(Some(dek))?;
        let mut wraps = self.load_wraps()?.ok_or_else(|| {
            Error::Crypto("the vault is encrypted but wraps.toml is missing".into())
        })?;
        change(&mut wraps)?;
        self.save_wraps(&wraps)?;
        Ok(wraps)
    }
}

/// Small base64 helpers so call-sites don't depend on the engine directly.
pub fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(Error::from)
}
