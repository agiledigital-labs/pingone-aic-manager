//! CLI wiring for `aic saml metadata`. These go through the binary so a
//! `run` arm that returns `Ok(())` without printing cannot stay green.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

fn entra_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/saml/fixtures/entra-federationmetadata.xml")
}

fn entra_sanitised() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/saml/fixtures/entra-federationmetadata.sanitised.xml")
}

#[test]
fn inspect_stdout_is_the_library_json() {
    let output = Command::new(bin())
        .args([
            "saml",
            "metadata",
            "inspect",
            entra_fixture().to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("run inspect");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("inspect prints JSON");
    assert_eq!(
        doc["entity_id"],
        "https://sts.windows.net/00000000-0000-0000-0000-000000000000/"
    );
    assert_eq!(doc["roles"], serde_json::json!(["identityProvider"]));
    assert_eq!(doc["certs"].as_array().map(Vec::len), Some(3));
    assert_eq!(doc["would_remove"].as_array().map(Vec::len), Some(3));
}

#[test]
fn sanitise_stdout_matches_the_committed_pair() {
    let output = Command::new(bin())
        .args([
            "saml",
            "metadata",
            "sanitise",
            entra_fixture().to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("run sanitise");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = std::fs::read(entra_sanitised()).expect("fixture");
    assert_eq!(output.stdout, expected);
}
