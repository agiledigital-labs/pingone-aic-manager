use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use crate::Result;

pub fn store_key(project_path: &str, key: &[u8]) -> Result<()> {
    let entry = keyring::Entry::new("aic-edit", project_path)
        .map_err(|e| crate::Error::Keychain(e.to_string()))?;
    let encoded = B64.encode(key);
    entry
        .set_password(&encoded)
        .map_err(|e| crate::Error::Keychain(e.to_string()))?;
    Ok(())
}

pub fn load_key(project_path: &str) -> Result<Option<Vec<u8>>> {
    let entry = keyring::Entry::new("aic-edit", project_path)
        .map_err(|e| crate::Error::Keychain(e.to_string()))?;
    match entry.get_password() {
        Ok(encoded) => {
            let bytes = B64.decode(encoded)?;
            Ok(Some(bytes))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(crate::Error::Keychain(e.to_string())),
    }
}

pub fn delete_key(project_path: &str) -> Result<()> {
    let entry = keyring::Entry::new("aic-edit", project_path)
        .map_err(|e| crate::Error::Keychain(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(crate::Error::Keychain(e.to_string())),
    }
}
