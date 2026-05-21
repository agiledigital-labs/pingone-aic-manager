//! Envelope encryption used by aic-edit at rest.
//!
//! ## Why two layers
//!
//! aic-edit needs more than one way to unlock the on-disk credentials — a
//! master password and (optionally) one or more Yubikeys, possibly more
//! methods later. Encrypting the JWK map directly with the password would
//! mean re-encrypting the whole blob whenever a key is added, removed, or
//! rotated, and it would mean each unlock method has to derive the same key.
//!
//! Instead we use a single random 32-byte **DEK** (data encryption key) for
//! the data, and **wrap** the DEK with whatever unlock methods are enrolled.
//! Each wrap is independent — adding/removing a Yubikey only touches the
//! wraps file, never `keys.enc`.
//!
//! ```text
//!   ┌──────────────┐                       ┌──────────────┐
//!   │ keys.enc     │  AES-256-GCM (DEK)    │ wraps.toml   │
//!   │ JWK map      │ ◄──────────decrypt──┐ │ [[wrap]]     │
//!   └──────────────┘                     │ │  password    │
//!                                        │ │ [[wrap]]     │
//!                                  DEK  ◄┘ │  yubikey     │
//!                                          └──────────────┘
//! ```

use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{Error, Result};

/// File magic for the data file (`keys.enc`). The version byte changes when
/// the layout changes.
const DATA_MAGIC: &[u8; 4] = b"AICE";
const DATA_VERSION: u8 = 2;
const DATA_HEADER_LEN: usize = 4 + 1 + 12; // magic + version + nonce

pub const DEK_LEN: usize = 32;

/// A 32-byte data encryption key. Zeroed on drop. Held in memory between
/// unlock and exit.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Dek(pub [u8; DEK_LEN]);

impl Dek {
    pub fn random() -> Self {
        let mut bytes = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; DEK_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; DEK_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for Dek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dek").field("len", &DEK_LEN).finish()
    }
}

// ---- Data layer ----

/// Encrypt the JWK map plaintext with the DEK. Output layout:
/// `magic(4) | version(1) | nonce(12) | ciphertext+tag(…)`.
pub fn encrypt_data(plaintext: &[u8], dek: &Dek) -> Result<Vec<u8>> {
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(dek.as_bytes())
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::Crypto(e.to_string()))?;

    let mut out = Vec::with_capacity(DATA_HEADER_LEN + ciphertext.len());
    out.extend_from_slice(DATA_MAGIC);
    out.push(DATA_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_data(data: &[u8], dek: &Dek) -> Result<Vec<u8>> {
    if data.len() < DATA_HEADER_LEN + 16 {
        return Err(Error::Crypto("keys.enc is too short".into()));
    }
    if &data[0..4] != DATA_MAGIC {
        return Err(Error::Crypto("keys.enc has bad magic bytes".into()));
    }
    if data[4] != DATA_VERSION {
        return Err(Error::Crypto(format!(
            "keys.enc has unknown version {} (expected {DATA_VERSION})",
            data[4]
        )));
    }
    let nonce_bytes: [u8; 12] = data[5..17].try_into().unwrap();
    let ciphertext = &data[17..];
    let cipher = Aes256Gcm::new_from_slice(dek.as_bytes())
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::Crypto("keys.enc decryption failed".into()))
}

// ---- KEK derivation + DEK wrapping ----

/// Wrap a DEK with a key derived from the user's master password (Argon2id).
/// Returns (salt, nonce, ciphertext).
pub fn wrap_dek_with_password(
    dek: &Dek,
    password: &str,
) -> Result<([u8; 16], [u8; 12], Vec<u8>)> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let kek = derive_password_kek(password, &salt)?;
    let (nonce, ct) = wrap_dek_with_kek(dek, &kek)?;
    Ok((salt, nonce, ct))
}

pub fn unwrap_dek_with_password(
    password: &str,
    salt: &[u8; 16],
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Dek> {
    let kek = derive_password_kek(password, salt)?;
    unwrap_dek_with_kek(&kek, nonce, ciphertext)
}

/// Wrap a DEK with a raw 32-byte KEK (e.g. an HMAC output from a Yubikey).
/// Returns (nonce, ciphertext).
pub fn wrap_dek_with_kek(dek: &Dek, kek: &[u8; 32]) -> Result<([u8; 12], Vec<u8>)> {
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(kek).map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, dek.as_bytes().as_ref())
        .map_err(|e| Error::Crypto(e.to_string()))?;
    Ok((nonce_bytes, ct))
}

pub fn unwrap_dek_with_kek(
    kek: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Dek> {
    let cipher = Aes256Gcm::new_from_slice(kek).map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(nonce);
    let mut plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::Crypto("DEK unwrap failed (wrong key?)".into()))?;
    if plaintext.len() != DEK_LEN {
        plaintext.zeroize();
        return Err(Error::Crypto(format!(
            "wrapped DEK has wrong length {}, expected {}",
            plaintext.len(),
            DEK_LEN
        )));
    }
    let mut bytes = [0u8; DEK_LEN];
    bytes.copy_from_slice(&plaintext);
    plaintext.zeroize();
    Ok(Dek::from_bytes(bytes))
}

fn derive_password_kek(password: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    let params = Params::new(65536, 3, 4, Some(32)).map_err(|e| Error::Crypto(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut kek = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut kek)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(kek)
}
