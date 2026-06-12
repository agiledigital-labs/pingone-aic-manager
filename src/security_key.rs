//! FIDO2 hmac-secret integration for the security key unlock path.
//!
//! The security key isn't asked to decrypt the data directly. Instead we use it
//! as a deterministic HMAC oracle:
//!
//! 1. **Enrol** — make a new FIDO2 credential on the device with the
//!    `hmac-secret` extension enabled. The device returns a `credential_id`
//!    we save in `wraps.toml`.
//! 2. **Get HMAC** — send (`credential_id`, 32-byte salt) to the device.
//!    User taps. Device returns a 32-byte HMAC — deterministic for that
//!    (credential, salt) pair, undisclosable to the host any other way.
//!
//! We use the HMAC output as the KEK that wraps the DEK in `wraps.toml`.
//! This matches the pattern age-plugin-yubikey and systemd-cryptenroll use.
//!
//! Security keys (and most FIDO2 authenticators) require a `pinUvAuthToken` to
//! be established before they'll honour `hmac-secret`. That means a PIN is
//! mandatory at enrol AND at every unlock — the CTAP2 spec offers no
//! "touch-only" path for this extension. We thread `pin: Option<&str>`
//! through so that callers that don't need a PIN (e.g. dev probes against
//! authenticators with no PIN set) can still skip it.
//!
//! Talking to the device is fully synchronous; callers should run these
//! functions inside `tokio::task::spawn_blocking`.

use crate::{Error, Result};
use ctap_hid_fido2::{
    Cfg, FidoKeyHidFactory,
    fidokey::{
        AssertionExtension as Aext, CredentialExtension as Cext, FidoKeyHid,
        GetAssertionArgsBuilder, MakeCredentialArgsBuilder,
    },
    verifier,
};

/// Stable RP ID used for every aic-edit credential. The security key scopes its
/// HMAC outputs by (credential_id, rp_id, salt), so this just needs to be
/// constant across enrol + use.
pub const RP_ID: &str = "aic-edit";

pub const HMAC_SALT_LEN: usize = 32;
pub const HMAC_OUT_LEN: usize = 32;

/// Status string shown under the PIN field while a tap is pending. Lives
/// here (rather than next to the generic `secret_field` widget) because the
/// concept is specific to the security-key flow.
pub const TAP_MESSAGE: &str = "🔑  Tap your security key to unlock…";

/// What a successful enrolment hands back to the caller.
pub struct Enrolment {
    /// FIDO2 credential id returned by the device. Goes into wraps.toml.
    pub credential_id: Vec<u8>,
    /// The HMAC output for the credential against the file-level salt the
    /// caller supplied. Used immediately to wrap the DEK; not persisted.
    pub hmac: [u8; HMAC_OUT_LEN],
}

fn open_device() -> Result<FidoKeyHid> {
    // `enable_keep_alive_msg` defaults to true in ctap-hid-fido2, which makes
    // it `println!("- Touch the sensor on the authenticator")` directly to
    // stdout every ~1s while waiting for a tap. That trashes the TUI; turn
    // it off — we surface tap prompts through the UI ourselves.
    let mut cfg = Cfg::init();
    cfg.enable_keep_alive_msg = false;
    FidoKeyHidFactory::create(&cfg).map_err(|e| {
        let raw = e.to_string();
        if raw.contains("FIDO device not found") {
            Error::Crypto("no security key detected — plug in a FIDO2 device and try again".into())
        } else {
            Error::Crypto(format!("could not open security key: {raw}"))
        }
    })
}

/// Ensure the connected device speaks CTAP2 and advertises the `hmac-secret`
/// extension. CTAP1-only tokens (older U2F devices, Yubico Gnubby) fail
/// `get_info` with `INVALID_COMMAND`; CTAP2 tokens without `hmac-secret`
/// answer get_info just fine but lack the extension. We turn both into a
/// concrete error the UI can show before the user wastes a tap.
fn require_hmac_secret(device: &FidoKeyHid) -> Result<()> {
    match device.get_info() {
        Ok(info) => {
            if info.extensions.iter().any(|e| e == "hmac-secret") {
                Ok(())
            } else {
                Err(Error::Crypto(
                    "this security key doesn't support the hmac-secret extension. \
                     aic-edit needs a FIDO2/CTAP2 key with hmac-secret (e.g. \
                     Yubikey 5, SoloKey, or Nitrokey 3)."
                        .into(),
                ))
            }
        }
        Err(e) => {
            let raw = e.to_string();
            if raw.contains("CTAP1_ERR_INVALID_COMMAND") || raw.contains("0x01") {
                Err(Error::Crypto(
                    "this security key only speaks U2F (CTAP1). aic-edit needs a \
                     FIDO2/CTAP2 key with the hmac-secret extension (e.g. Yubikey 5, \
                     SoloKey, or Nitrokey 3)."
                        .into(),
                ))
            } else {
                Err(Error::Crypto(format!(
                    "couldn't read security key capabilities: {raw}"
                )))
            }
        }
    }
}

/// Enrol a new security key credential with hmac-secret enabled and
/// immediately derive its HMAC against `hmac_salt` — the caller's file-
/// level salt that's shared across every security-key wrap in the same
/// project. Two touches required (make_credential + first get_assertion).
pub fn enroll(pin: Option<&str>, hmac_salt: &[u8; HMAC_SALT_LEN]) -> Result<Enrolment> {
    let device = open_device()?;
    require_hmac_secret(&device)?;

    // 1. Make a fresh credential.
    let challenge = verifier::create_challenge();
    let mut make_builder = MakeCredentialArgsBuilder::new(RP_ID, &challenge)
        .extensions(&[Cext::HmacSecret(Some(true))]);
    make_builder = match pin {
        Some(p) if !p.is_empty() => make_builder.pin(p),
        _ => make_builder.without_pin_and_uv(),
    };
    let attestation = device
        .make_credential_with_args(&make_builder.build())
        .map_err(|e| Error::Crypto(format!("security key make_credential failed: {e}")))?;
    let credential_id = attestation.credential_descriptor.id.clone();

    // 2. Ask for the first HMAC against the shared salt.
    let (_matched, hmac) = unlock_with_device(
        &device,
        std::slice::from_ref(&credential_id),
        hmac_salt,
        pin,
    )?;

    Ok(Enrolment {
        credential_id,
        hmac,
    })
}

/// Single `getAssertion` against an allowList of every enrolled credential
/// — the device returns the one it actually holds. Lets us identify the
/// matched wrap and derive its HMAC in one PIN check + one tap, regardless
/// of how many keys are enrolled.
///
/// Returns `(matched_credential_id, hmac)`. The credential id is the raw
/// bytes the device echoed back; the caller matches it against `wraps.toml`
/// to find which wrap to unwrap.
pub fn unlock_any(
    credential_ids: &[Vec<u8>],
    hmac_salt: &[u8; HMAC_SALT_LEN],
    pin: Option<&str>,
) -> Result<(Vec<u8>, [u8; HMAC_OUT_LEN])> {
    if credential_ids.is_empty() {
        return Err(Error::Crypto("no security key credentials enrolled".into()));
    }
    let device = open_device()?;
    unlock_with_device(&device, credential_ids, hmac_salt, pin)
}

fn unlock_with_device(
    device: &FidoKeyHid,
    credential_ids: &[Vec<u8>],
    hmac_salt: &[u8; HMAC_SALT_LEN],
    pin: Option<&str>,
) -> Result<(Vec<u8>, [u8; HMAC_OUT_LEN])> {
    let challenge = verifier::create_challenge();
    let mut get_builder = GetAssertionArgsBuilder::new(RP_ID, &challenge)
        .extensions(&[Aext::HmacSecret(Some(*hmac_salt))]);
    for cid in credential_ids {
        get_builder = get_builder.add_credential_id(cid);
    }
    get_builder = match pin {
        Some(p) if !p.is_empty() => get_builder.pin(p),
        _ => get_builder.without_pin_and_uv(),
    };
    let assertions = device
        .get_assertion_with_args(&get_builder.build())
        .map_err(|e| Error::Crypto(format!("security key get_assertion failed: {e}")))?;
    let assertion = assertions
        .first()
        .ok_or_else(|| Error::Crypto("security key returned no assertions".into()))?;
    let hmac = assertion
        .extensions
        .iter()
        .find_map(|ext| match ext {
            Aext::HmacSecret(Some(v)) => Some(*v),
            _ => None,
        })
        .ok_or_else(|| {
            Error::Crypto("security key assertion had no hmac-secret extension output".into())
        })?;
    Ok((assertion.credential_id.clone(), hmac))
}

/// Check whether a FIDO2 authenticator is currently connected. We just try
/// to open it — `FidoKeyHidFactory::create` errors when no device is found.
pub fn device_present() -> bool {
    open_device().is_ok()
}
