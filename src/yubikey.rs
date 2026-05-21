//! FIDO2 hmac-secret integration for the yubikey unlock path.
//!
//! The yubikey isn't asked to decrypt the data directly. Instead we use it
//! as a deterministic HMAC oracle:
//!
//! 1. **Enrol** — make a new FIDO2 credential on the device with the
//!    `hmac-secret` extension enabled. The device returns a `credential_id`
//!    we save in `wraps.toml`. No PIN is requested (touch-only).
//! 2. **Get HMAC** — send (`credential_id`, 32-byte salt) to the device.
//!    User taps. Device returns a 32-byte HMAC — deterministic for that
//!    (credential, salt) pair, undisclosable to the host any other way.
//!
//! We use the HMAC output as the KEK that wraps the DEK in `wraps.toml`.
//! This matches the pattern age-plugin-yubikey and systemd-cryptenroll use.
//!
//! Talking to the device is fully synchronous; callers should run these
//! functions inside `tokio::task::spawn_blocking`.

use ctap_hid_fido2::{
    fidokey::{
        AssertionExtension as Aext, CredentialExtension as Cext, FidoKeyHid,
        GetAssertionArgsBuilder, MakeCredentialArgsBuilder,
    },
    verifier, Cfg, FidoKeyHidFactory,
};
use rand::RngCore;

use crate::{Error, Result};

/// Stable RP ID used for every aic-edit credential. The yubikey scopes its
/// HMAC outputs by (credential_id, rp_id, salt), so this just needs to be
/// constant across enrol + use.
pub const RP_ID: &str = "aic-edit";

pub const HMAC_SALT_LEN: usize = 32;
pub const HMAC_OUT_LEN: usize = 32;

/// What a successful enrolment hands back to the caller.
pub struct Enrolment {
    /// FIDO2 credential id returned by the device. Goes into wraps.toml.
    pub credential_id: Vec<u8>,
    /// The salt we chose at enrolment time. Stored alongside the credential
    /// id so subsequent unlocks can reproduce the HMAC.
    pub hmac_salt: [u8; HMAC_SALT_LEN],
    /// The HMAC output for (credential_id, hmac_salt). Used immediately to
    /// wrap the DEK; not persisted.
    pub hmac: [u8; HMAC_OUT_LEN],
}

fn open_device() -> Result<FidoKeyHid> {
    FidoKeyHidFactory::create(&Cfg::init())
        .map_err(|e| Error::Crypto(format!("no yubikey detected: {e}")))
}

/// Enrol a new yubikey credential with hmac-secret enabled and immediately
/// derive its first HMAC. Requires one touch.
pub fn enroll() -> Result<Enrolment> {
    let device = open_device()?;

    // 1. Make a fresh credential.
    let challenge = verifier::create_challenge();
    let make_args = MakeCredentialArgsBuilder::new(RP_ID, &challenge)
        .without_pin_and_uv()
        .extensions(&[Cext::HmacSecret(Some(true))])
        .build();
    let attestation = device
        .make_credential_with_args(&make_args)
        .map_err(|e| Error::Crypto(format!("yubikey make_credential failed: {e}")))?;
    let credential_id = attestation.credential_descriptor.id.clone();

    // 2. Pick a salt and ask for the first HMAC.
    let mut hmac_salt = [0u8; HMAC_SALT_LEN];
    rand::thread_rng().fill_bytes(&mut hmac_salt);
    let hmac = derive_hmac_with_device(&device, &credential_id, &hmac_salt)?;

    Ok(Enrolment {
        credential_id,
        hmac_salt,
        hmac,
    })
}

/// Derive the HMAC for an already-enrolled (credential_id, salt) pair.
/// Requires one touch.
pub fn derive_hmac(credential_id: &[u8], hmac_salt: &[u8; HMAC_SALT_LEN]) -> Result<[u8; HMAC_OUT_LEN]> {
    let device = open_device()?;
    derive_hmac_with_device(&device, credential_id, hmac_salt)
}

fn derive_hmac_with_device(
    device: &FidoKeyHid,
    credential_id: &[u8],
    hmac_salt: &[u8; HMAC_SALT_LEN],
) -> Result<[u8; HMAC_OUT_LEN]> {
    let challenge = verifier::create_challenge();
    let get_args = GetAssertionArgsBuilder::new(RP_ID, &challenge)
        .without_pin_and_uv()
        .credential_id(credential_id)
        .extensions(&[Aext::HmacSecret(Some(*hmac_salt))])
        .build();
    let assertions = device
        .get_assertion_with_args(&get_args)
        .map_err(|e| Error::Crypto(format!("yubikey get_assertion failed: {e}")))?;
    let assertion = assertions
        .first()
        .ok_or_else(|| Error::Crypto("yubikey returned no assertions".into()))?;
    let hmac = assertion
        .extensions
        .iter()
        .find_map(|ext| match ext {
            Aext::HmacSecret(Some(v)) => Some(*v),
            _ => None,
        })
        .ok_or_else(|| Error::Crypto("yubikey assertion had no hmac-secret extension output".into()))?;
    Ok(hmac)
}

/// Check whether a FIDO2 authenticator is currently connected. We just try
/// to open it — `FidoKeyHidFactory::create` errors when no device is found.
pub fn device_present() -> bool {
    open_device().is_ok()
}
