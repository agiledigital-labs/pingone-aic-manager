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

    // Identities, not counts. Three certificates of the right *number* is
    // what a scanner produces from this document whether it fingerprinted the
    // enveloped signature's key and dropped a role's, hashed the base64 text
    // instead of the DER, or attributed a key to the wrong role — and the
    // role is what a rotation will address a key by.
    const ENTRA_CERT: &str = "5f50984266ddca8c23154e9c6549ddf6c38f5b3b6f32d9315e8108000cebf33c";
    assert_eq!(
        doc["certs"],
        serde_json::json!([
            { "descriptor": "RoleDescriptor", "key_use": "signing", "key_name": null, "sha256": ENTRA_CERT },
            { "descriptor": "RoleDescriptor", "key_use": "signing", "key_name": null, "sha256": ENTRA_CERT },
            { "descriptor": "IDPSSODescriptor", "key_use": "signing", "key_name": null, "sha256": ENTRA_CERT },
        ])
    );

    // The removal report is the only record that a document was changed, so
    // it is asserted as the operator reads it: which element, on which line,
    // and for which of the two reasons — the signature and the WS-Federation
    // roles have different remedies.
    assert_eq!(
        doc["would_remove"],
        serde_json::json!([
            {
                "element": "ds:Signature",
                "local_name": "Signature",
                "xsi_type": null,
                "line": 16,
                "reason": "enveloped-signature",
            },
            {
                "element": "RoleDescriptor",
                "local_name": "RoleDescriptor",
                "xsi_type": "fed:SecurityTokenServiceType",
                "line": 36,
                "reason": "unsupported-role",
            },
            {
                "element": "RoleDescriptor",
                "local_name": "RoleDescriptor",
                "xsi_type": "fed:ApplicationServiceType",
                "line": 60,
                "reason": "unsupported-role",
            },
        ])
    );
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
