//! `security_key_probe` — a single-purpose CLI for poking the FIDO2 hmac-secret
//! flow without dragging the rest of aic-edit along.
//!
//! Build:
//!     nix-shell --run "cargo build --example security_key_probe"
//!
//! Usage:
//!     cargo run --example security_key_probe -- list
//!     cargo run --example security_key_probe -- info     [<device-index>]
//!     cargo run --example security_key_probe -- enroll   [<device-index>]
//!     cargo run --example security_key_probe -- assert <cred_id_b64> <salt_b64> [<device-index>]
//!
//! `<device-index>` defaults to 0; values come from `list`.
//! Set `SECURITY_KEY_PIN=…` before the command to skip the interactive prompt.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ctap_hid_fido2::{
    fidokey::{
        get_info::InfoOption, AssertionExtension as Aext, CredentialExtension as Cext,
        FidoKeyHid, GetAssertionArgsBuilder, MakeCredentialArgsBuilder,
    },
    get_fidokey_devices, util, verifier, Cfg, FidoKeyHidFactory, HidInfo,
};

const RP_ID: &str = "aic-edit-probe";

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "list".into());
    let rest: Vec<String> = args.collect();

    let result = match cmd.as_str() {
        "list" => list_devices(),
        "info" => device_info(rest.first().and_then(|s| s.parse().ok())),
        "enroll" => enroll(rest.first().and_then(|s| s.parse().ok())),
        "assert" => assert_hmac(rest),
        other => Err(format!(
            "unknown command {other:?} — try list | info | enroll | assert"
        )),
    };

    if let Err(e) = result {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

// ---- list ----

fn list_devices() -> Result<(), String> {
    let devices = get_fidokey_devices();
    if devices.is_empty() {
        println!("No FIDO HID devices found.");
        println!("(check `lsusb` shows the key, and that you're in the right user group for hidraw)");
        return Ok(());
    }
    println!("Found {} FIDO HID device(s):", devices.len());
    for (i, d) in devices.iter().enumerate() {
        print_device(i, d);
    }
    Ok(())
}

fn print_device(i: usize, d: &HidInfo) {
    println!(
        "  [{i}]  vid=0x{:04x} pid=0x{:04x}  {}",
        d.vid, d.pid, d.product_string
    );
    println!("       {}", d.info);
}

/// Open the FIDO device at index `idx` from `get_fidokey_devices()`. Defaults
/// to the first one when `idx` is None.
fn open_device(idx: Option<usize>) -> Result<FidoKeyHid, String> {
    let devices = get_fidokey_devices();
    if devices.is_empty() {
        return Err("no FIDO HID devices found (plug your key in?)".into());
    }
    let chosen = match idx {
        Some(i) if i >= devices.len() => {
            return Err(format!(
                "device index {i} out of range — `list` shows {} device(s)",
                devices.len()
            ));
        }
        Some(i) => i,
        None => 0,
    };
    eprintln!(
        "Opening device [{chosen}]: vid=0x{:04x} pid=0x{:04x}  {}",
        devices[chosen].vid, devices[chosen].pid, devices[chosen].product_string
    );
    FidoKeyHidFactory::create_by_params(&[devices[chosen].param.clone()], &Cfg::init())
        .map_err(|e| format!("open device: {e}"))
}

// ---- info ----

fn device_info(idx: Option<usize>) -> Result<(), String> {
    let device = open_device(idx)?;
    let info = device.get_info().map_err(|e| format!("get_info: {e}"))?;

    println!("Versions:    {:?}", info.versions);
    println!("AAGUID:      {}", util::to_hex_str(&info.aaguid));
    println!("Extensions:  {:?}", info.extensions);
    println!("Transports:  {:?}", info.transports);
    println!("PIN-UV auth protocols: {:?}", info.pin_uv_auth_protocols);
    println!("Min PIN length: {}", info.min_pin_length);
    println!("Force PIN change: {}", info.force_pin_change);
    println!("Firmware version: 0x{:08x}", info.firmware_version);
    println!();
    println!("Options:");
    for (name, val) in &info.options {
        println!("  {name:30}  {val}");
    }
    println!();

    println!("Key claims to support `hmac-secret`?  {}",
        if info.extensions.iter().any(|e| e == "hmac-secret") {
            "YES"
        } else {
            "NO"
        });
    println!(
        "Client PIN set?  {}",
        match device.enable_info_option(&InfoOption::ClientPin) {
            Ok(Some(true)) => "YES",
            Ok(Some(false)) => "NO (FIDO2 supported but no PIN set)",
            Ok(None) => "N/A (no clientPin option advertised)",
            Err(_) => "unknown",
        }
    );
    Ok(())
}

// ---- enroll ----

fn enroll(idx: Option<usize>) -> Result<(), String> {
    let device = open_device(idx)?;
    let pin = read_pin_optional();

    println!("Making a credential on RP {RP_ID}");
    println!("(Touch your security key now…)");

    let challenge = verifier::create_challenge();
    let mut builder = MakeCredentialArgsBuilder::new(RP_ID, &challenge)
        .extensions(&[Cext::HmacSecret(Some(true))]);
    match pin.as_deref() {
        Some(p) if !p.is_empty() => {
            builder = builder.pin(p);
        }
        _ => {
            builder = builder.without_pin_and_uv();
        }
    }
    let attestation = device
        .make_credential_with_args(&builder.build())
        .map_err(|e| format!("make_credential: {e}"))?;
    let credential_id = attestation.credential_descriptor.id.clone();
    println!("OK — credential_id = {}", B64.encode(&credential_id));

    let hmac_supported = attestation
        .extensions
        .iter()
        .any(|e| matches!(e, Cext::HmacSecret(Some(true))));
    if !hmac_supported {
        return Err(
            "device returned attestation, but hmac-secret extension is not enabled on it"
                .into(),
        );
    }

    println!();
    println!("Now generating an HMAC with a fresh salt (touch again)…");
    let mut salt = [0u8; 32];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut salt);

    let challenge = verifier::create_challenge();
    let mut a_builder = GetAssertionArgsBuilder::new(RP_ID, &challenge)
        .credential_id(&credential_id)
        .extensions(&[Aext::HmacSecret(Some(salt))]);
    if let Some(p) = pin.as_deref() {
        if !p.is_empty() {
            a_builder = a_builder.pin(p);
        }
    }
    let assertions = device
        .get_assertion_with_args(&a_builder.build())
        .map_err(|e| format!("get_assertion: {e}"))?;
    let hmac = assertions
        .first()
        .and_then(|a| {
            a.extensions.iter().find_map(|e| match e {
                Aext::HmacSecret(Some(v)) => Some(*v),
                _ => None,
            })
        })
        .ok_or_else(|| "device returned no hmac-secret output".to_string())?;

    println!("salt   = {}", B64.encode(salt));
    println!("hmac   = {}", B64.encode(hmac));
    println!();
    println!("Save the credential_id + salt; re-run with:");
    println!(
        "    cargo run --example security_key_probe -- assert {} {}",
        B64.encode(&credential_id),
        B64.encode(salt)
    );
    Ok(())
}

// ---- assert ----

fn assert_hmac(args: Vec<String>) -> Result<(), String> {
    let cred_b64 = args
        .first()
        .ok_or("usage: assert <credential_id_b64> <salt_b64> [<device-index>]")?;
    let salt_b64 = args
        .get(1)
        .ok_or("usage: assert <credential_id_b64> <salt_b64> [<device-index>]")?;
    let idx = args.get(2).and_then(|s| s.parse().ok());

    let credential_id = B64.decode(cred_b64).map_err(|e| format!("cred b64: {e}"))?;
    let salt_bytes = B64.decode(salt_b64).map_err(|e| format!("salt b64: {e}"))?;
    let salt: [u8; 32] = salt_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "salt must be 32 bytes".to_string())?;

    let device = open_device(idx)?;
    let pin = read_pin_optional();

    println!("Touch your security key to generate the HMAC…");
    let challenge = verifier::create_challenge();
    let mut builder = GetAssertionArgsBuilder::new(RP_ID, &challenge)
        .credential_id(&credential_id)
        .extensions(&[Aext::HmacSecret(Some(salt))]);
    if let Some(p) = pin.as_deref() {
        if !p.is_empty() {
            builder = builder.pin(p);
        }
    }
    let assertions = device
        .get_assertion_with_args(&builder.build())
        .map_err(|e| format!("get_assertion: {e}"))?;
    let hmac = assertions
        .first()
        .and_then(|a| {
            a.extensions.iter().find_map(|e| match e {
                Aext::HmacSecret(Some(v)) => Some(*v),
                _ => None,
            })
        })
        .ok_or_else(|| "device returned no hmac-secret output".to_string())?;

    println!("hmac   = {}", B64.encode(hmac));
    Ok(())
}

// ---- PIN entry ----

fn read_pin_optional() -> Option<String> {
    // 1. env var wins (lets you script without typing).
    if let Ok(pin) = std::env::var("SECURITY_KEY_PIN") {
        if !pin.is_empty() {
            return Some(pin);
        }
    }
    // 2. prompt only if attached to a tty.
    if !atty_stdin() {
        return None;
    }
    eprint!("FIDO2 PIN (blank to skip): ");
    use std::io::Write;
    let _ = std::io::stderr().flush();
    match rpassword::read_password() {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

fn atty_stdin() -> bool {
    // Avoid pulling in the `atty` crate — a libc isatty is enough.
    extern crate libc;
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}
