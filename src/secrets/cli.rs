//! `aic esv secret` parser and command implementation.

use clap::Subcommand;

use crate::cli::{print_json, prod_hint, tenant_for};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum SecretCommand {
    /// List secrets (metadata only — values are write-only).
    List {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Get a single secret's metadata as JSON.
    Get {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Create a secret (PUT is create-only; change values via add-version).
    ///
    /// The value is read (in priority order) from `--value-file`,
    /// `--value-stdin`, or an interactive no-echo prompt. `--value` exists for
    /// scripting but leaks the secret into shell history / process listings —
    /// prefer the file or stdin form.
    Create {
        id: String,
        /// Secret value inline (DISCOURAGED — visible in `ps`/shell history).
        #[arg(long)]
        value: Option<String>,
        /// Read the value from a file (a single trailing newline is stripped).
        #[arg(long)]
        value_file: Option<std::path::PathBuf>,
        /// Read the value from stdin (a single trailing newline is stripped).
        #[arg(long)]
        value_stdin: bool,
        /// generic | pem | base64hmac | base64aes.
        #[arg(long, default_value = "generic")]
        encoding: String,
        /// Validate the value as JSON (generic encoding only).
        #[arg(long)]
        json: bool,
        /// Don't expose as `&{esv.id}` placeholder (loads immediately, no restart).
        #[arg(long)]
        no_placeholders: bool,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Set a secret's description.
    SetDescription {
        id: String,
        #[arg(long)]
        description: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// List a secret's versions (newest first).
    Versions {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Add a new version (becomes the active version). Value is encoded with
    /// the secret's existing encoding. Value source as for `create` (prefer
    /// `--value-file` / `--value-stdin` over `--value`).
    AddVersion {
        id: String,
        /// Secret value inline (DISCOURAGED — visible in `ps`/shell history).
        #[arg(long)]
        value: Option<String>,
        /// Read the value from a file (a single trailing newline is stripped).
        #[arg(long)]
        value_file: Option<std::path::PathBuf>,
        /// Read the value from stdin (a single trailing newline is stripped).
        #[arg(long)]
        value_stdin: bool,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Enable a version.
    Enable {
        id: String,
        version: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Disable a version (the latest version can't be disabled).
    Disable {
        id: String,
        version: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Destroy a version — irreversible.
    Destroy {
        id: String,
        version: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Delete a secret and all its versions — irreversible.
    Delete {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(cmd: SecretCommand) -> Result<()> {
    use crate::esv::api as esv;
    match cmd {
        SecretCommand::List { tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::list_secrets(&t).await?)
        }
        SecretCommand::Get { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::get_secret(&t, &id).await?)
        }
        SecretCommand::Create {
            id,
            value,
            value_file,
            value_stdin,
            encoding,
            json,
            no_placeholders,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            let value = resolve_secret_value(value, value_file, value_stdin, "Secret value: ")?;
            let value_b64 =
                esv::encode_secret_value(&encoding, &value, json).map_err(Error::Config)?;
            prod_hint(
                esv::create_secret(
                    &t,
                    &id,
                    &encoding,
                    !no_placeholders,
                    &value_b64,
                    &description,
                    yes,
                )
                .await,
            )?;
            println!("secret {id} created");
            Ok(())
        }
        SecretCommand::SetDescription {
            id,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            prod_hint(esv::set_secret_description(&t, &id, &description, yes).await)?;
            println!("secret {id} description updated");
            Ok(())
        }
        SecretCommand::Versions { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::list_secret_versions(&t, &id).await?)
        }
        SecretCommand::AddVersion {
            id,
            value,
            value_file,
            value_stdin,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            let value = resolve_secret_value(value, value_file, value_stdin, "New secret value: ")?;
            let encoding = esv::get_secret(&t, &id)
                .await?
                .get("encoding")
                .and_then(|x| x.as_str())
                .unwrap_or("generic")
                .to_string();
            let value_b64 =
                esv::encode_secret_value(&encoding, &value, false).map_err(Error::Config)?;
            let created = prod_hint(esv::create_secret_version(&t, &id, &value_b64, yes).await)?;
            let v = created.get("version").map(json_scalar).unwrap_or_default();
            println!("secret {id}: added version {v}");
            Ok(())
        }
        SecretCommand::Enable {
            id,
            version,
            tenant,
            yes,
        } => set_version_status(&id, &version, "ENABLED", tenant, yes).await,
        SecretCommand::Disable {
            id,
            version,
            tenant,
            yes,
        } => set_version_status(&id, &version, "DISABLED", tenant, yes).await,
        SecretCommand::Destroy {
            id,
            version,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            if !confirm_irreversible(
                &format!("Destroy version {version} of secret {id} on {t}."),
                yes,
            )? {
                println!("aborted");
                return Ok(());
            }
            prod_hint(esv::destroy_secret_version(&t, &id, &version, yes).await)?;
            println!("secret {id} version {version} destroyed");
            Ok(())
        }
        SecretCommand::Delete { id, tenant, yes } => {
            let t = tenant_for(tenant)?;
            if !confirm_irreversible(
                &format!("Delete secret {id} and all its versions on {t}."),
                yes,
            )? {
                println!("aborted");
                return Ok(());
            }
            prod_hint(esv::delete_secret(&t, &id, yes).await)?;
            println!("secret {id} deleted");
            Ok(())
        }
    }
}

async fn set_version_status(
    id: &str,
    version: &str,
    status: &str,
    tenant: Option<String>,
    yes: bool,
) -> Result<()> {
    let t = tenant_for(tenant)?;
    prod_hint(crate::esv::api::change_version_status(&t, id, version, status, yes).await)?;
    println!("secret {id} version {version} → {status}");
    Ok(())
}

fn resolve_secret_value(
    value: Option<String>,
    value_file: Option<std::path::PathBuf>,
    value_stdin: bool,
    prompt: &str,
) -> Result<String> {
    let sources = value.is_some() as u8 + value_file.is_some() as u8 + value_stdin as u8;
    if sources > 1 {
        return Err(Error::Config(
            "provide only one of --value / --value-file / --value-stdin".into(),
        ));
    }
    let strip = |mut s: String| {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    };
    let value = if let Some(value) = value {
        value
    } else if let Some(path) = value_file {
        strip(
            std::fs::read_to_string(&path)
                .map_err(|e| Error::Config(format!("read --value-file {}: {e}", path.display())))?,
        )
    } else if value_stdin {
        use std::io::Read;
        let mut value = String::new();
        std::io::stdin()
            .read_to_string(&mut value)
            .map_err(|e| Error::Config(format!("read stdin: {e}")))?;
        strip(value)
    } else {
        rpassword::prompt_password(prompt).map_err(|e| Error::Config(format!("read value: {e}")))?
    };
    if value.is_empty() {
        return Err(Error::Config("value cannot be empty".into()));
    }
    Ok(value)
}

fn confirm_irreversible(action: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    use std::io::Write;
    eprint!("{action} This cannot be undone. Type 'yes' to confirm: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Config(format!("read confirmation: {e}")))?;
    Ok(line.trim() == "yes")
}

fn json_scalar(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        v.to_string()
    }
}
