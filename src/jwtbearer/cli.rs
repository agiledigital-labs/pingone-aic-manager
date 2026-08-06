//! `aic jwt-bearer` parser and command implementation.

use std::path::PathBuf;

use std::io::IsTerminal;

use chrono::Utc;
use clap::{Args, Subcommand};
use serde_json::Value;

use crate::Result;
use crate::agent::AgentClient;
use crate::cli::{print_json, read_password_line, realm_arg, tenant_config_for};
use crate::config::{self, operator::NetworkAccess};
use crate::jwtbearer::{self, ops, spec};

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

#[derive(Args, Debug)]
#[command(group(
    clap::ArgGroup::new("subject")
        .required(true)
        .multiple(false)
))]
pub struct AuthOptions {
    /// User UUID to put in the assertion subject.
    #[arg(long, group = "subject")]
    pub as_id: Option<String>,
    /// Username to resolve to a user UUID before signing.
    #[arg(long, group = "subject")]
    pub as_username: Option<String>,
    /// OAuth2 client to authenticate.
    #[arg(long)]
    pub client_id: String,
    /// Read the client secret from one line of stdin.
    #[arg(long)]
    pub client_secret_stdin: bool,
    /// Requested OAuth2 scope. Repeat for multiple scopes.
    #[arg(long)]
    pub scope: Vec<String>,
    #[arg(long)]
    pub realm: Option<String>,
    #[arg(long, help = "Tenant to target")]
    pub tenant: Option<String>,
    /// Print only the bare access token.
    #[arg(long)]
    pub token: bool,
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

pub async fn run_auth(options: AuthOptions) -> Result<()> {
    let realm = realm_arg("auth", options.realm)?;
    let tenant = tenant_config_for(options.tenant)?;
    spec::ensure_not_production(tenant.theme)?;

    let record = jwtbearer::get_key(AgentClient::connect_or_spawn().await?, &tenant.name)
        .await?
        .ok_or_else(|| {
            crate::Error::Config(format!(
                "no Trusted JWT private key stored for tenant {}; run aic jwt-bearer setup",
                tenant.name
            ))
        })?;

    let (subject, username) = match (options.as_id, options.as_username) {
        (Some(id), None) => (id, None),
        (None, Some(username)) => {
            let response =
                crate::jwtbearer::api::lookup_username(&tenant.name, &realm, &username).await?;
            let id = spec::user_id_from_lookup(&username, &response)?;
            (id, Some(username))
        }
        _ => unreachable!("clap enforces exactly one subject argument"),
    };

    let discovery = crate::jwtbearer::api::discovery(&tenant.name, &realm).await?;
    let audience = required_string(&discovery, "issuer")?;
    let client_secret = read_client_secret(options.client_secret_stdin)?;
    let now = Utc::now().timestamp();
    let assertion = spec::sign_user_assertion(
        ops::DEFAULT_ISSUER,
        &subject,
        audience,
        now,
        &record.kid,
        &record.private_jwk,
    )?;

    let form = spec::token_request(
        &options.client_id,
        client_secret.as_deref(),
        &assertion,
        &options.scope,
    );

    let response = crate::jwtbearer::api::mint_user_token(&tenant.name, &realm, &form)
        .await
        .map_err(spec::map_token_error)?;
    let access_token = response
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| crate::Error::Auth("AM response did not contain access_token".into()))?;
    let expires_in = response
        .get("expires_in")
        .and_then(Value::as_i64)
        .ok_or_else(|| crate::Error::Auth("AM response did not contain expires_in".into()))?;
    let granted_scope = response
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let expires_at = now + expires_in;

    if options.token {
        println!("{access_token}");
    } else {
        match username {
            Some(username) => println!("user:    {subject} ({username})"),
            None => println!("user:    {subject}"),
        }
        println!("client:  {}", options.client_id);
        println!("scope:   {granted_scope}");
        println!("expires: in {expires_in}s (unix {expires_at})");
        println!("kid:     {}", record.kid);
        println!("token:   {}", crate::cli::redact(access_token));
    }
    Ok(())
}

fn required_string<'a>(object: &'a Value, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::Error::Config(format!("discovery document is missing {field}")))
}

fn read_client_secret(from_stdin: bool) -> Result<Option<String>> {
    if from_stdin {
        return Ok(Some(read_password_line(std::io::stdin().lock())?));
    }
    if crate::cli::prompting_disabled()
        || !std::io::stderr().is_terminal()
        || std::fs::File::open("/dev/tty").is_err()
    {
        return Err(crate::Error::Config(
            "client secret is required; supply --client-secret-stdin".into(),
        ));
    }
    rpassword::prompt_password("Client secret: ")
        .map(Some)
        .map_err(|error| crate::Error::Config(format!("read client secret from /dev/tty: {error}")))
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
    use clap::Parser;

    use crate::cli::{Cli, Command, realm_arg};

    #[test]
    fn realm_arg_defaults_to_alpha_and_only_aic_realms_are_allowed() {
        assert_eq!(realm_arg("jwt-bearer", None).unwrap(), "alpha");
        assert_eq!(
            realm_arg("jwt-bearer", Some("bravo".into())).unwrap(),
            "bravo"
        );
        assert!(realm_arg("jwt-bearer", Some("root".into())).is_err());
    }

    #[test]
    fn auth_requires_exactly_one_subject_form() {
        let id = Cli::try_parse_from(["aic", "auth", "--as-id", "u", "--client-id", "c"]).unwrap();
        assert!(matches!(id.command, Some(Command::Auth { .. })));

        assert!(Cli::try_parse_from(["aic", "auth", "--client-id", "c"]).is_err());
        assert!(
            Cli::try_parse_from([
                "aic",
                "auth",
                "--as-id",
                "u",
                "--as-username",
                "name",
                "--client-id",
                "c"
            ])
            .is_err()
        );
    }
}
