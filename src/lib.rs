pub mod agent;
pub mod aic;
pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod esv;
pub mod idmstore;
pub mod journey;
pub mod jwtbearer;
pub mod logging;
pub mod logs;
pub mod managed;
pub mod mappings;
pub mod oauth;
pub mod onboard;
pub mod roles;
pub mod scripts;
pub mod secretmap;
pub mod secrets;
pub mod tui;
pub mod undo;
pub mod vault;

pub use error::{Error, Result};

/// Repo hygiene rules that are cheaper to assert than to remember.
///
/// These are tests rather than lints because they are about *this* codebase's
/// history: each one exists because the thing it forbids already happened once.
#[cfg(test)]
mod repo_hygiene {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Calls that mint a fresh asymmetric keypair. RSA key generation is a
    /// primality search — seconds, and variable, because it depends on how many
    /// candidates get rejected. One of these reached the default test path in
    /// 2026-08 and took the workspace suite from 0.33s to 8.31s, in exchange for
    /// no coverage at all: the test was asserting the *shape* of a key record,
    /// which a stub JSON object proves just as well.
    ///
    /// Signing and verifying with an existing key are sub-millisecond and are
    /// not the concern here. Neither is Ed25519/ECDSA keygen, which is just
    /// picking a random scalar. This is specifically about RSA generation.
    const KEYGEN_CALLS: &[&str] = &["generate_rsa_jwk(", "RsaPrivateKey::new("];

    /// Put this on the offending line when a test genuinely needs real key
    /// material — and gate that test behind `#[ignore]` so it stays out of the
    /// default run.
    const ALLOW_MARKER: &str = "slow-keygen-ok";

    /// **This check is necessary but not sufficient**, and the gap is worth
    /// knowing about. It only sees keygen called *directly* from a test module.
    /// The 2026-08 instance it was written for called a production helper that
    /// generated a key one level down, so a grep like this would have missed it.
    /// A per-test wall-clock budget is the guard that catches the transitive
    /// case; this one catches the obvious case for free.
    #[test]
    fn no_direct_key_generation_under_cfg_test() {
        let mut offences = Vec::new();

        // Anchor on the manifest dir, not the CWD: other tests in this suite
        // chdir the whole process (`src/cli/mod.rs`, `src/agent/daemon.rs`), so
        // a relative "src" resolves or not depending on what else is running.
        let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

        for path in rust_sources(&src_root) {
            // A guard must never match its own definition. `KEYGEN_CALLS` above
            // literally contains the strings being searched for, so scanning
            // this file reports the check itself as the offence — the same
            // self-matching trap as a `pgrep -f` whose pattern appears in its
            // own command line.
            if path.ends_with("lib.rs") {
                continue;
            }
            let source = fs::read_to_string(&path).expect("read source file");
            // Test modules sit at the bottom of a file by convention here, so
            // everything after the first `#[cfg(test)]` is test-only code.
            let Some(test_start) = source.find("#[cfg(test)]") else {
                continue;
            };
            let offset_line = source[..test_start].lines().count();

            for (index, line) in source[test_start..].lines().enumerate() {
                if line.contains(ALLOW_MARKER) {
                    continue;
                }
                // Skip the definition itself, which is production code that
                // happens to live in a file with a test module.
                if line.contains("fn generate_rsa_jwk") {
                    continue;
                }
                if KEYGEN_CALLS.iter().any(|call| line.contains(call)) {
                    offences.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        offset_line + index + 1,
                        line.trim()
                    ));
                }
            }
        }

        assert!(
            offences.is_empty(),
            "RSA key generation in the default test path:\n  {}\n\n\
             Assert against a stub JWK instead. If the test genuinely needs real \
             key material, mark it #[ignore] and add `{ALLOW_MARKER}` to the \
             offending line.",
            offences.join("\n  ")
        );
    }

    fn rust_sources(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let entries = fs::read_dir(dir).expect("read source dir");
        for entry in entries {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                found.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
        found
    }
}
