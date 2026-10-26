//! The target-tenant banner, driven through the real binary.
//!
//! Unit tests cover `capture_invocation` and `resolve_against` separately;
//! only a subprocess shows that `parse_with_defaults` stores the capture and
//! `run` prints from it. Delete the `INVOCATION_TENANT.set` in
//! `parse_with_defaults` and every assertion here fails, because no banner is
//! printed at all.
//!
//! `saml metadata export` is the vehicle because it takes `--tenant`, needs no
//! vault or agent, and reaches the network only after the banner: its base
//! URL is a closed local port, so it fails fast once the banner is out.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

/// Two tenants, `default_tenant = sandbox`, and a `workspace/dev/` directory.
fn project() -> PathBuf {
    let root = std::env::temp_dir().join(format!("aic-banner-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".aic")).expect("create project root");
    std::fs::create_dir_all(root.join("workspace/dev")).expect("create workspace dir");
    let tenant = |name: &str| {
        format!(
            "[[tenant]]\nname = \"{name}\"\nbase_url = \"http://127.0.0.1:9\"\n\
             theme = \"sandbox\"\nscopes = []\n\n"
        )
    };
    std::fs::write(
        root.join(".aic/config.toml"),
        format!(
            "project = \"test\"\ndefault_tenant = \"sandbox\"\n\n{}{}",
            tenant("dev"),
            tenant("sandbox")
        ),
    )
    .expect("write config");
    root.canonicalize().expect("canonical root")
}

/// The first stderr line of an export run from `cwd`.
fn banner(cwd: &Path, extra: &[&str]) -> String {
    let output = Command::new(bin())
        .current_dir(cwd)
        .env_remove("AIC_PROJECT")
        .args(["--no-prompt", "saml", "metadata", "export"])
        .arg("https://sp-a.example.com")
        .args(["--out", cwd.join("entity.xml").to_str().expect("utf-8")])
        .args(extra)
        .output()
        .expect("run aic");
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_banner_names_the_tenant_and_the_rule_that_chose_it() {
    let root = project();
    let dev = root.join("workspace/dev");

    assert_eq!(
        banner(&root, &[]),
        "target tenant: sandbox (default_tenant in .aic/config.toml)"
    );
    assert_eq!(
        banner(&dev, &[]),
        "target tenant: dev (current directory is in workspace/dev/)"
    );
    // Same value as the injected default; only the value source differs.
    assert_eq!(
        banner(&dev, &["--tenant", "dev"]),
        "target tenant: dev (--tenant)"
    );
    assert_eq!(
        banner(&dev, &["--tenant", "sandbox"]),
        "target tenant: sandbox (--tenant)"
    );

    // The stored context outranks default_tenant but not the workspace.
    std::fs::write(root.join(".aic/current-context"), "dev").expect("store context");
    assert_eq!(
        banner(&root, &[]),
        "target tenant: dev (stored context, set by `aic ctx use`)"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
