//! Independent byte oracle produced on 2026-11-14 with Node v24.21.0 and
//! @forgerock/fr-config-manager 1.9.0 (the cached published package).
//! The committed generate.cjs mocks ONLY restGet, returning the input object;
//! exportManagedObjects, hook extraction, deepSort and saveJsonToFile are real.
//! Reproduce (package-root is the installed @forgerock/fr-config-manager):
//! node src/fcm/fixtures/generate.cjs <package-root> \
//!   src/fcm/fixtures/managed-input.json /tmp/fcm-oracle
//! Copy the resulting managed-objects/custom_widget/* into fixtures/expected/.
//! Node drains the package's asynchronous fs.writeFile before process exit.

use super::*;

#[test]
fn managed_bytes_equal_the_real_package_writer() {
    let raw: Value = serde_json::from_slice(include_bytes!("fixtures/managed-input.json")).unwrap();
    let files = managed_object_files(&raw).unwrap();
    let expected: [(&str, &[u8]); 2] = [
        (
            "managed-objects/custom_widget/custom_widget.onCreate.js",
            include_bytes!("fixtures/expected/custom_widget.onCreate.js"),
        ),
        (
            "managed-objects/custom_widget/custom_widget.json",
            include_bytes!("fixtures/expected/custom_widget.json"),
        ),
    ];
    assert_eq!(files.len(), expected.len());
    for (path, bytes) in expected {
        let file = files
            .iter()
            .find(|file| file.relative == Path::new(path))
            .unwrap();
        assert_eq!(file.bytes, bytes, "package byte mismatch at {path}");
    }
}
