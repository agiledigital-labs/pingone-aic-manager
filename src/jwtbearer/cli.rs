//! `aic jwt-bearer` parser and command implementation.

use std::path::PathBuf;

use clap::Subcommand;
use serde_json::Value;

use crate::Result;
use crate::cli::{print_json, realm_arg, tenant_config_for};
use crate::config::{self, operator::NetworkAccess};
use crate::jwtbearer::ops;

#[derive(Subcommand, Debug)]
pub enum JwtBearerCommand {
    /// Ensure the default realm issuer and this install's key exist.
    Setup {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Manage named Trusted JWT Issuers.
    Issuer {
        #[command(subcommand)]
        command: IssuerCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum IssuerCommand {
    /// Create a named issuer from a public JWKS file.
    Create {
        id: String,
        #[arg(long)]
        issuer: String,
        #[arg(long, value_name = "FILE")]
        jwks_from: PathBuf,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Show one issuer, or all issuers in the realm when no id is supplied.
    Show {
        id: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

pub async fn run(command: JwtBearerCommand) -> Result<()> {
    match command {
        JwtBearerCommand::Setup { realm, tenant } => {
            let realm = realm_arg("jwt-bearer", realm)?;
            let tenant = tenant_config_for(tenant)?;
            let settings = config::Settings::load()?.unwrap_or_default();
            let operator =
                config::operator::resolve(&settings, Some(&tenant), NetworkAccess::Skip).await;
            let kid = ops::setup(&tenant, &realm, &operator).await?;
            println!(
                "configured Trusted JWT issuer {} in realm {} for tenant {} (kid {})",
                ops::DEFAULT_ISSUER_ID,
                realm,
                tenant.name,
                kid
            );
            Ok(())
        }
        JwtBearerCommand::Issuer { command } => run_issuer(command).await,
    }
}

async fn run_issuer(command: IssuerCommand) -> Result<()> {
    match command {
        IssuerCommand::Create {
            id,
            issuer,
            jwks_from,
            realm,
            tenant,
        } => {
            let realm = realm_arg("jwt-bearer", realm)?;
            let tenant = tenant_config_for(tenant)?;
            ops::create_issuer(&tenant, &realm, &id, &issuer, &jwks_from).await?;
            println!("created Trusted JWT issuer {id} in realm {}", realm);
            Ok(())
        }
        IssuerCommand::Show { id, realm, tenant } => {
            let realm = realm_arg("jwt-bearer", realm)?;
            let tenant = tenant_config_for(tenant)?;
            let value: Value = match id {
                Some(id) => crate::jwtbearer::api::read_issuer(&tenant.name, &realm, &id).await?,
                None => crate::jwtbearer::api::list_issuers(&tenant.name, &realm).await?,
            };
            print_json(&value)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::realm_arg;

    #[test]
    fn realm_arg_defaults_to_alpha_and_only_aic_realms_are_allowed() {
        assert_eq!(realm_arg("jwt-bearer", None).unwrap(), "alpha");
        assert_eq!(
            realm_arg("jwt-bearer", Some("bravo".into())).unwrap(),
            "bravo"
        );
        assert!(realm_arg("jwt-bearer", Some("root".into())).is_err());
    }
}
