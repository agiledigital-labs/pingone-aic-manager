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

/// Unlock via a yubikey wrap: ask the device for the HMAC of its enrolled
/// (credential, salt) pair (one touch), use that HMAC as the KEK that
/// unwraps the DEK, then decrypt `keys.enc`.
pub fn unlock_with_yubikey(
    wrap: &wraps::Wrap,
) -> Result<(crypto::Dek, HashMap<String, serde_json::Value>)> {
    let (credential_id_b64, hmac_salt_b64, nonce_b64, ct_b64) = match wrap {
        wraps::Wrap::Yubikey {
            credential_id,
            hmac_salt,
            nonce,
            ciphertext,
            ..
        } => (credential_id, hmac_salt, nonce, ciphertext),
        _ => {
            return Err(crate::Error::Crypto(
                "unlock_with_yubikey called with a non-yubikey wrap".into(),
            ));
        }
    };
    let credential_id = wraps::b64_decode(credential_id_b64)?;
    let hmac_salt: [u8; crate::yubikey::HMAC_SALT_LEN] = wraps::b64_decode(hmac_salt_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("yubikey wrap: hmac_salt length".into()))?;
    let nonce: [u8; 12] = wraps::b64_decode(nonce_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("yubikey wrap: nonce length".into()))?;
    let ct = wraps::b64_decode(ct_b64)?;

    let hmac = crate::yubikey::derive_hmac(&credential_id, &hmac_salt)?;
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Settings {
    pub encrypt_keys: bool,
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
        // FIDO2 credential id for any enrolled yubikeys — opaque but
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

    /// Current working directory as a stable string key for the keychain.
    pub fn project_key() -> String {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string())
    }
}
