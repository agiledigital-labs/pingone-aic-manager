pub mod crypto;
pub mod tenant;
pub mod wraps;

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;
pub use tenant::{Tenant, TenantTheme};

/// Decrypt the DEK with the user's password (via the password wrap recorded
/// in `wraps.toml`), then decrypt `keys.enc` with the DEK and parse the JWK
/// map. Returns `(dek, jwks)`. An empty map is yielded when `keys.enc`
/// doesn't exist yet — that's the "just after first-run setup" state.
pub fn unlock_with_password(
    password: &str,
) -> Result<(crypto::Dek, HashMap<String, serde_json::Value>)> {
    let wraps_file = wraps::WrapsFile::load()?
        .ok_or_else(|| crate::Error::Crypto("no wraps.toml on disk".into()))?;
    let pw_wrap = wraps_file
        .password_wrap()
        .ok_or_else(|| crate::Error::Crypto("no password wrap on file".into()))?;
    let (salt_b64, nonce_b64, ct_b64) = match pw_wrap {
        wraps::Wrap::Password { salt, nonce, ciphertext } => (salt, nonce, ciphertext),
        _ => unreachable!(),
    };
    let salt: [u8; 16] = wraps::b64_decode(salt_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("password wrap: salt length".into()))?;
    let nonce: [u8; 12] = wraps::b64_decode(nonce_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("password wrap: nonce length".into()))?;
    let ct = wraps::b64_decode(ct_b64)?;
    let dek = crypto::unwrap_dek_with_password(password, &salt, &nonce, &ct)?;
    let jwks = decrypt_keys_file(&dek)?;
    Ok((dek, jwks))
}

pub fn decrypt_keys_file(
    dek: &crypto::Dek,
) -> Result<HashMap<String, serde_json::Value>> {
    match ProjectConfig::load_keys_enc()? {
        Some(data) => {
            let plaintext = crypto::decrypt_data(&data, dek)?;
            Ok(serde_json::from_slice(&plaintext)?)
        }
        None => Ok(HashMap::new()),
    }
}

/// Encrypt + persist the JWK map using the in-memory DEK.
pub fn save_jwk_map(
    map: &HashMap<String, serde_json::Value>,
    dek: &crypto::Dek,
) -> Result<()> {
    let bytes = serde_json::to_vec(map)?;
    let enc = crypto::encrypt_data(&bytes, dek)?;
    ProjectConfig::save_keys_enc(&enc)?;
    Ok(())
}

/// Transition from "no encryption" to "encrypted": read `keys.plain`,
/// encrypt with `dek`, write `keys.enc`, delete `keys.plain`, and flip
/// `settings.toml` to `encrypt_keys = true`. Called by the Settings screen
/// when the user adds the first auth factor while encryption is disabled.
///
/// `wraps.toml` is not touched here — callers must have already saved the
/// wrap that protects this DEK before calling this function, otherwise
/// `keys.enc` would be unreadable on next launch.
pub fn enable_encryption(dek: &crypto::Dek) -> Result<()> {
    // First-run installs never wrote keys.plain (no factors were added before
    // now), so substitute an empty JSON map. Decrypting empty bytes would
    // fail the `from_slice` round-trip in `decrypt_keys_file`.
    let plain = match ProjectConfig::load_keys_plain()? {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => b"{}".to_vec(),
    };
    let enc = crypto::encrypt_data(&plain, dek)?;
    ProjectConfig::save_keys_enc(&enc)?;
    let _ = fs::remove_file(ProjectConfig::keys_plain_path());
    let mut settings = Settings::load()?.unwrap_or_default();
    settings.encrypt_keys = true;
    settings.save()?;
    Ok(())
}

/// Transition from "encrypted" to "no encryption": decrypt `keys.enc` with
/// `dek`, write the cleartext to `keys.plain` (mode 600), then delete
/// `keys.enc` and `wraps.toml`, and flip `settings.toml` to
/// `encrypt_keys = false`. Called by the last-factor guard in the Settings
/// screen.
pub fn disable_encryption(dek: &crypto::Dek) -> Result<()> {
    let plain = decrypt_keys_file(dek)?;
    let bytes = serde_json::to_vec(&plain)?;
    ProjectConfig::save_keys_plain(&bytes)?;
    let _ = fs::remove_file(ProjectConfig::keys_path());
    let _ = fs::remove_file(ProjectConfig::wraps_path());
    let mut settings = Settings::load()?.unwrap_or_default();
    settings.encrypt_keys = false;
    settings.save()?;
    Ok(())
}

/// Unlock via any enrolled security key in `wraps_file`. Builds an allowList
/// from every enrolled credential id and makes a single `getAssertion` call
/// (one PIN check, one tap) — the device returns the credential it actually
/// holds, and we use that to pick the right wrap to unwrap.
///
/// Errors if `wraps_file` has security-key wraps but no `security_key_hmac_salt`
/// (old schema — user needs to re-enrol).
pub fn unlock_with_security_key(
    wraps_file: &wraps::WrapsFile,
    pin: Option<&str>,
) -> Result<(crypto::Dek, HashMap<String, serde_json::Value>)> {
    let salt_b64 = wraps_file
        .security_key_hmac_salt
        .as_ref()
        .ok_or_else(|| {
            crate::Error::Crypto(
                "wraps.toml has security keys enrolled but no security_key_hmac_salt — \
                 the salt was moved file-level; please remove and re-enrol your keys"
                    .into(),
            )
        })?;
    let hmac_salt: [u8; crate::security_key::HMAC_SALT_LEN] = wraps::b64_decode(salt_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("security_key_hmac_salt length".into()))?;

    let security_wraps: Vec<&wraps::Wrap> = wraps_file.security_key_wraps().collect();
    if security_wraps.is_empty() {
        return Err(crate::Error::Crypto(
            "no security key wraps enrolled".into(),
        ));
    }
    let credential_ids: Vec<Vec<u8>> = security_wraps
        .iter()
        .map(|w| match w {
            wraps::Wrap::SecurityKey { credential_id, .. } => wraps::b64_decode(credential_id),
            _ => unreachable!("filtered above"),
        })
        .collect::<Result<_>>()?;

    let (matched_id, hmac) = crate::security_key::unlock_any(&credential_ids, &hmac_salt, pin)?;

    // Find the wrap whose credential_id matches what the device returned.
    let matched_wrap = security_wraps
        .iter()
        .find(|w| match w {
            wraps::Wrap::SecurityKey { credential_id, .. } => {
                wraps::b64_decode(credential_id).map(|b| b == matched_id).unwrap_or(false)
            }
            _ => false,
        })
        .ok_or_else(|| {
            crate::Error::Crypto(
                "security key returned a credential id that doesn't match any enrolled wrap"
                    .into(),
            )
        })?;
    let (nonce_b64, ct_b64) = match matched_wrap {
        wraps::Wrap::SecurityKey { nonce, ciphertext, .. } => (nonce, ciphertext),
        _ => unreachable!("filtered above"),
    };
    let nonce: [u8; 12] = wraps::b64_decode(nonce_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("security key wrap: nonce length".into()))?;
    let ct = wraps::b64_decode(ct_b64)?;
    let dek = crypto::unwrap_dek_with_kek(&hmac, &nonce, &ct)?;
    let jwks = decrypt_keys_file(&dek)?;
    Ok((dek, jwks))
}

/// Path to the per-project "currently-selected tenant" pointer used by the CLI.
pub fn current_context_path() -> PathBuf {
    ProjectConfig::dir().join("current-context")
}

pub fn read_current_context() -> Result<Option<String>> {
    let path = current_context_path();
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(path)?;
    Ok(Some(s.trim().to_string()))
}

pub fn write_current_context(name: &str) -> Result<()> {
    fs::create_dir_all(ProjectConfig::dir())?;
    fs::write(current_context_path(), name)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: String,
    pub default_tenant: String,
    #[serde(rename = "tenant", default)]
    pub tenants: Vec<Tenant>,
}

/// First-run user choice: encrypt tenant credentials at rest with a master
/// password (Argon2id + AES-256-GCM in `keys.enc`) or store them as a plain,
/// gitignored, mode-600 file (`keys.plain`). Recorded once in
/// `.aic-edit/settings.toml` so the choice persists across launches.
///
/// `agent_idle_timeout_secs` is read by the daemon at startup to decide how
/// long the cached DEK lives in memory. Omit (or set to `None`) for the
/// 1-hour default. No UI yet — edit the file by hand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Settings {
    pub encrypt_keys: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_idle_timeout_secs: Option<u64>,
}

impl Settings {
    pub fn path() -> std::path::PathBuf {
        ProjectConfig::dir().join("settings.toml")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(toml::from_str(&fs::read_to_string(&path)?)?))
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(ProjectConfig::dir())?;
        fs::write(Self::path(), toml::to_string_pretty(self)?)?;
        ProjectConfig::write_gitignore()?;
        Ok(())
    }
}

impl ProjectConfig {
    pub fn dir() -> PathBuf {
        PathBuf::from(".aic-edit")
    }

    pub fn keys_path() -> PathBuf {
        Self::dir().join("keys.enc")
    }

    pub fn wraps_path() -> PathBuf {
        Self::dir().join("wraps.toml")
    }

    pub fn keys_plain_path() -> PathBuf {
        Self::dir().join("keys.plain")
    }

    pub fn config_path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let contents = fs::read_to_string(&path)?;
        let config: ProjectConfig = toml::from_str(&contents)?;
        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)?;
        fs::write(Self::config_path(), contents)?;
        Self::write_gitignore()?;
        Ok(())
    }

    pub fn write_gitignore() -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let path = Self::dir().join(".gitignore");
        // wraps.toml contains the (encrypted) DEK envelope, including the
        // FIDO2 credential id for any enrolled security_keys — opaque but
        // device-specific, so we never check it in.
        let content = "keys.enc\nkeys.plain\nwraps.toml\nlocal-config/\n*.log\n";
        fs::write(path, content)?;
        Ok(())
    }

    /// Save the encrypted JWK map (Argon2id + AES-256-GCM) at mode 600.
    pub fn save_keys_enc(data: &[u8]) -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let path = Self::keys_path();
        fs::write(&path, data)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn load_keys_enc() -> Result<Option<Vec<u8>>> {
        let path = Self::keys_path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }

    /// Save unencrypted JWK map (mode 600). Used when the user opts out of
    /// master-password protection.
    pub fn save_keys_plain(data: &[u8]) -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let path = Self::keys_plain_path();
        fs::write(&path, data)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn load_keys_plain() -> Result<Option<Vec<u8>>> {
        let path = Self::keys_plain_path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }

}
