use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;

use crate::{Error, Result};

const MAGIC: &[u8; 4] = b"AICE";
const VERSION: u8 = 1;
// Layout: magic(4) | version(1) | salt(16) | nonce(12) | ciphertext+tag(...)
const HEADER_LEN: usize = 4 + 1 + 16 + 12;

fn derive_key(password: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    let params = Params::new(65536, 3, 4, Some(32))
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(key)
}

pub fn encrypt(plaintext: &[u8], password: &str) -> Result<Vec<u8>> {
    let mut rng = rand::thread_rng();

    let mut salt = [0u8; 16];
    rng.fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::Crypto(e.to_string()))?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt(data: &[u8], password: &str) -> Result<Vec<u8>> {
    if data.len() < HEADER_LEN + 16 {
        return Err(Error::Crypto("file too short".into()));
    }
    if &data[0..4] != MAGIC {
        return Err(Error::Crypto("bad magic bytes".into()));
    }
    if data[4] != VERSION {
        return Err(Error::Crypto(format!("unknown version {}", data[4])));
    }

    let salt: [u8; 16] = data[5..21].try_into().unwrap();
    let nonce_bytes: [u8; 12] = data[21..33].try_into().unwrap();
    let ciphertext = &data[33..];

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::Crypto("decryption failed — wrong password?".into()))
}

/// Encrypt raw key bytes (used to store derived key in the file alongside JWKs)
pub fn encrypt_key_bytes(key_bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    encrypt(key_bytes, password)
}

pub fn decrypt_key_bytes(data: &[u8], password: &str) -> Result<Vec<u8>> {
    decrypt(data, password)
}
