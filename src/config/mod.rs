pub mod crypto;
pub mod tenant;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;
pub use tenant::{Tenant, TenantTheme};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: String,
    pub default_tenant: String,
    #[serde(rename = "tenant", default)]
    pub tenants: Vec<Tenant>,
}

impl ProjectConfig {
    pub fn dir() -> PathBuf {
        PathBuf::from(".aic-edit")
    }

    pub fn keys_path() -> PathBuf {
        Self::dir().join("keys.enc")
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
        let content = "keys.enc\nlocal-config/\n*.log\n";
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

    /// Current working directory as a stable string key for the keychain.
    pub fn project_key() -> String {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string())
    }
}
