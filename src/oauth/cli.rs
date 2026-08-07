//! `aic oauth` parser and command implementation.

use std::io::ErrorKind;
use std::path::{Path, PathBuf, is_separator};

use base64::Engine as _;
use clap::{Args, Subcommand};
use rand::RngCore;
use serde_json::Value;

use crate::cli::{print_json, print_table, prod_hint, read_password_line, realm_arg, tenant_for};
use crate::config::ProjectConfig;
use crate::oauth::{api, spec};
use crate::{Error, Result};

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Seed the template from a JSON object (including `aic oauth pull` output).
    #[arg(long, value_name = "FILE")]
    from: Option<PathBuf>,
    /// coreOAuth2ClientConfig.clientName (one array value).
    #[arg(long)]
    name: Option<String>,
    /// advancedOAuth2ClientConfig.descriptions (one array value).
    #[arg(long)]
    description: Option<String>,
    /// coreOAuth2ClientConfig.clientType. Defaults to Confidential unless
    /// --from supplies a value.
    #[arg(long, value_name = "TYPE")]
    client_type: Option<String>,
    /// Read coreOAuth2ClientConfig.userpassword as one line from stdin. The
    /// write-only value reads back as null and cannot be recovered.
    #[arg(long, conflicts_with = "generate_secret")]
    secret_stdin: bool,
    /// Generate a 256-bit userpassword and print it once after success. The
    /// write-only value reads back as null and cannot be recovered.
    #[arg(long)]
    generate_secret: bool,
    /// coreOAuth2ClientConfig.scopes (repeatable).
    #[arg(long)]
    scope: Vec<String>,
    /// coreOAuth2ClientConfig.defaultScopes (repeatable).
    #[arg(long)]
    default_scope: Vec<String>,
    /// coreOAuth2ClientConfig.redirectionUris (repeatable).
    #[arg(long)]
    redirect_uri: Vec<String>,
    /// advancedOAuth2ClientConfig.grantTypes (repeatable; live-schema validated).
    #[arg(long)]
    grant: Vec<String>,
    /// advancedOAuth2ClientConfig.responseTypes (repeatable).
    #[arg(long)]
    response_type: Vec<String>,
    /// advancedOAuth2ClientConfig.tokenEndpointAuthMethod (live-schema validated).
    #[arg(long, value_name = "METHOD")]
    token_endpoint_auth_method: Option<String>,
    /// advancedOAuth2ClientConfig.subjectType (live-schema validated).
    #[arg(long, value_name = "TYPE")]
    subject_type: Option<String>,
    /// Set advancedOAuth2ClientConfig.isConsentImplied to true.
    #[arg(long)]
    implied_consent: bool,
    /// coreOAuth2ClientConfig.accessTokenLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    access_token_lifetime: Option<u64>,
    /// coreOAuth2ClientConfig.refreshTokenLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    refresh_token_lifetime: Option<u64>,
    /// coreOAuth2ClientConfig.authorizationCodeLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    authorization_code_lifetime: Option<u64>,
    /// Replace an existing client with the same id.
    #[arg(long)]
    force: bool,
    #[arg(long)]
    realm: Option<String>,
    #[arg(long)]
    tenant: Option<String>,
    /// Confirm creation or replacement on a production-themed tenant.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
pub struct GrantChangeArgs {
    /// OAuth2 client id.
    id: String,
    /// Grant types to add or remove.
    #[arg(required = true, num_args = 1..)]
    grant: Vec<String>,
    #[arg(long)]
    realm: Option<String>,
    #[arg(long)]
    tenant: Option<String>,
    /// Confirm the write on a production-themed tenant.
    #[arg(long)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum GrantCommand {
    /// List the grant types enabled on an OAuth2 client.
    List {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Add one or more grant types to an OAuth2 client.
    Add(GrantChangeArgs),
    /// Remove one or more grant types from an OAuth2 client.
    Remove(GrantChangeArgs),
}

#[derive(Subcommand, Debug)]
pub enum OauthCommand {
    /// List OAuth2 client ids in a realm.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long, help = "Print client ids as JSON")]
        json: bool,
    },
    /// Create an OAuth2 client from the tenant's live template.
    Create {
        /// OAuth2 client id.
        id: String,
        #[command(flatten)]
        options: Box<CreateArgs>,
    },
    /// List or change an OAuth2 client's grant types.
    Grant {
        #[command(subcommand)]
        command: GrantCommand,
    },
    /// Pull an OAuth2 client into the workspace as JSON.
    Pull {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Push a workspace OAuth2 client JSON file back to AIC.
    Push {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Delete an OAuth2 client from AIC. Requires --force.
    Delete {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
}

fn validate_client_id(id: &str) -> Result<()> {
    if id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "oauth client id {id:?} contains a path separator"
        )));
    }
    Ok(())
}

fn export_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(format!("{id}.json")))
}

fn snapshot_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(".snapshots")
        .join(format!("{id}.json")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushBlockReason {
    MissingSnapshot,
    RemoteDrift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushDecision {
    Push,
    Blocked(PushBlockReason),
    NothingToDo,
}

fn push_decision(
    local: &Value,
    remote: &Value,
    snapshot: Option<&Value>,
    force: bool,
) -> PushDecision {
    if api::content_equal(local, remote) {
        return PushDecision::NothingToDo;
    }

    let Some(snapshot) = snapshot else {
        return if force {
            PushDecision::Push
        } else {
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        };
    };

    if api::content_equal(remote, snapshot) || force {
        PushDecision::Push
    } else {
        PushDecision::Blocked(PushBlockReason::RemoteDrift)
    }
}

fn push_block_message(id: &str, reason: &PushBlockReason) -> String {
    match reason {
        PushBlockReason::MissingSnapshot => format!(
            "no snapshot for oauth client {id:?}; run `aic oauth pull {id}` first or pass --force"
        ),
        PushBlockReason::RemoteDrift => format!(
            "remote oauth client {id} changed since you last pulled; re-pull or pass --force"
        ),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_snapshot(tenant: &str, realm: &str, id: &str, client: &Value) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    write_bytes(&path, &serde_json::to_vec_pretty(client)?)
}

fn parse_client_value(value: Value, label: &str) -> Result<Value> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(Error::Config(format!(
            "oauth client JSON {label} is not an object"
        )))
    }
}

fn read_seed(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|error| {
        Error::Config(format!(
            "read oauth client seed {}: {error}",
            path.display()
        ))
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client seed {}: {error}",
            path.display()
        ))
    })?;
    parse_client_value(value, &path.display().to_string())
}

fn read_export(path: &Path, id: &str) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            Error::Config(format!(
                "local oauth client export missing: {}; run `aic oauth pull {id}` first",
                path.display()
            ))
        } else {
            Error::Config(format!(
                "read oauth client export {}: {error}",
                path.display()
            ))
        }
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client export {}: {error}",
            path.display()
        ))
    })?;
    parse_client_value(value, &path.display().to_string())
}

fn read_snapshot(path: &Path) -> Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Config(format!(
                "read oauth client snapshot {}: {error}",
                path.display()
            )));
        }
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client snapshot {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(parse_client_value(
        value,
        &path.display().to_string(),
    )?))
}

fn remove_snapshot_if_present(tenant: &str, realm: &str, id: &str) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Config(format!(
            "remove oauth client snapshot {}: {error}",
            path.display()
        ))),
    }
}

fn api_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

fn ensure_create_allowed(exists: bool, force: bool, id: &str) -> Result<()> {
    if exists && !force {
        return Err(Error::Config(format!(
            "oauth client {id:?} already exists; pass --force to replace it"
        )));
    }
    Ok(())
}

fn generate_client_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn schema_for_validation(result: Result<Value>) -> Option<Value> {
    match result {
        Ok(schema) => Some(schema),
        Err(error) => {
            eprintln!(
                "warning: could not fetch oauth client schema; deferring enum validation to AIC: {error}"
            );
            None
        }
    }
}

const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

async fn run_grant_change(args: GrantChangeArgs, operation: spec::GrantOperation) -> Result<()> {
    let tenant = tenant_for(args.tenant)?;
    let realm = realm_arg("oauth", args.realm)?;
    validate_client_id(&args.id)?;

    let (client, schema) = tokio::join!(
        api::read_client(&tenant, &realm, &args.id),
        api::client_schema(&tenant, &realm, args.yes),
    );
    let client = client?;
    let schema = schema_for_validation(schema);
    let adding_jwt_bearer = operation == spec::GrantOperation::Add
        && args.grant.iter().any(|grant| grant == JWT_BEARER_GRANT)
        && !spec::grant_types(&client)
            .map_err(Error::Config)?
            .iter()
            .any(|grant| grant == JWT_BEARER_GRANT);
    let update = spec::update_grants(&client, &args.grant, operation).map_err(Error::Config)?;
    spec::validate_grant_types(&update.body, schema.as_ref()).map_err(Error::Config)?;

    if !update.changed {
        println!("oauth client {} grants already match; no change", args.id);
        return Ok(());
    }

    if adding_jwt_bearer {
        eprintln!(
            "warning: jwt-bearer lets a Trusted JWT Issuer with empty allowedSubjects mint tokens as any user in realm {realm}; AIC has no per-client issuer restriction"
        );
    }

    prod_hint(api::upsert_client(&tenant, &realm, &args.id, update.body, args.yes).await)?;
    let verb = match operation {
        spec::GrantOperation::Add => "added to",
        spec::GrantOperation::Remove => "removed from",
    };
    println!("grant types {verb} oauth client {}", args.id);
    Ok(())
}

pub async fn run(cmd: OauthCommand) -> Result<()> {
    match cmd {
        OauthCommand::List {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let clients = api::list_clients(&tenant, &realm).await?;
            if json {
                print_json(&clients)?;
            } else {
                let rows = clients
                    .iter()
                    .map(|id| vec![id.clone()])
                    .collect::<Vec<_>>();
                print_table(&["CLIENT_ID"], &rows);
            }
            eprintln!("{} oauth clients", clients.len());
            Ok(())
        }
        OauthCommand::Create { id, options } => {
            let tenant = tenant_for(options.tenant.clone())?;
            let realm = realm_arg("oauth", options.realm.clone())?;
            validate_client_id(&id)?;

            let exists = match api::read_client(&tenant, &realm, &id).await {
                Ok(_) => true,
                Err(error) if api_not_found(&error) => false,
                Err(error) => return Err(error),
            };
            ensure_create_allowed(exists, options.force, &id)?;

            let seed = options.from.as_deref().map(read_seed).transpose()?;
            let (template, schema) = tokio::join!(
                api::client_template(&tenant, &realm, options.yes),
                api::client_schema(&tenant, &realm, options.yes),
            );
            let template = prod_hint(template)?;
            let schema = schema_for_validation(schema);

            let generated_secret = options.generate_secret.then(generate_client_secret);
            let secret = if options.secret_stdin {
                Some(read_password_line(std::io::stdin().lock())?)
            } else {
                generated_secret.clone()
            };
            let create_spec = spec::CreateClientSpec {
                name: options.name,
                description: options.description,
                client_type: options.client_type,
                secret,
                scopes: options.scope,
                default_scopes: options.default_scope,
                redirect_uris: options.redirect_uri,
                grants: options.grant,
                response_types: options.response_type,
                token_endpoint_auth_method: options.token_endpoint_auth_method,
                subject_type: options.subject_type,
                implied_consent: options.implied_consent.then_some(true),
                access_token_lifetime: options.access_token_lifetime,
                refresh_token_lifetime: options.refresh_token_lifetime,
                authorization_code_lifetime: options.authorization_code_lifetime,
            };
            let body =
                spec::build_create_body(template, seed, &create_spec).map_err(Error::Config)?;
            spec::validate_enumerated_fields(&body, schema.as_ref()).map_err(Error::Config)?;

            prod_hint(api::create_client(&tenant, &realm, &id, body, options.yes).await)?;
            if let Some(secret) = generated_secret {
                println!("client secret: {secret}");
            }
            let verb = if exists { "replaced" } else { "created" };
            println!("{verb} oauth client {id}");
            Ok(())
        }
        OauthCommand::Grant { command } => match command {
            GrantCommand::List { id, realm, tenant } => {
                let tenant = tenant_for(tenant)?;
                let realm = realm_arg("oauth", realm)?;
                validate_client_id(&id)?;
                let client = api::read_client(&tenant, &realm, &id).await?;
                let grants = spec::grant_types(&client).map_err(Error::Config)?;
                let rows = grants
                    .iter()
                    .map(|grant| vec![grant.clone()])
                    .collect::<Vec<_>>();
                print_table(&["GRANT_TYPE"], &rows);
                eprintln!("{} grant types", grants.len());
                Ok(())
            }
            GrantCommand::Add(args) => run_grant_change(args, spec::GrantOperation::Add).await,
            GrantCommand::Remove(args) => {
                run_grant_change(args, spec::GrantOperation::Remove).await
            }
        },
        OauthCommand::Pull { id, realm, tenant } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let client = api::read_client(&tenant, &realm, &id).await?;
            let bytes = serde_json::to_vec_pretty(&client)?;
            write_bytes(&path, &bytes)?;
            write_bytes(&snapshot, &bytes)?;
            println!("pulled oauth client {id} -> {}", path.display());
            Ok(())
        }
        OauthCommand::Push {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let local = read_export(&path, &id)?;
            let remote = match api::read_client(&tenant, &realm, &id).await {
                Ok(client) => Some(client),
                Err(error) if api_not_found(&error) => None,
                Err(error) => return Err(error),
            };

            let Some(remote) = remote else {
                api::upsert_client(&tenant, &realm, &id, local, false).await?;
                let refreshed = api::read_client(&tenant, &realm, &id).await?;
                write_snapshot(&tenant, &realm, &id, &refreshed)?;
                println!("created oauth client {id}");
                return Ok(());
            };

            let snapshot_value = read_snapshot(&snapshot)?;
            match push_decision(&local, &remote, snapshot_value.as_ref(), force) {
                PushDecision::NothingToDo => {
                    write_snapshot(&tenant, &realm, &id, &remote)?;
                    println!("oauth client {id} already matches remote -> {tenant}/{realm}");
                    Ok(())
                }
                PushDecision::Blocked(reason) => {
                    Err(Error::Config(push_block_message(&id, &reason)))
                }
                PushDecision::Push => {
                    api::upsert_client(&tenant, &realm, &id, local, false).await?;
                    let refreshed = api::read_client(&tenant, &realm, &id).await?;
                    write_snapshot(&tenant, &realm, &id, &refreshed)?;
                    println!("pushed oauth client {id}");
                    Ok(())
                }
            }
        }
        OauthCommand::Delete {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            if !force {
                eprintln!(
                    "would delete oauth client {id} from {tenant}/{realm}; pass --force to delete it"
                );
                return Err(Error::Config("oauth client delete requires --force".into()));
            }
            api::delete_client(&tenant, &realm, &id).await?;
            remove_snapshot_if_present(&tenant, &realm, &id)?;
            println!("deleted oauth client {id}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn oauth_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(realm_arg("oauth", None).unwrap(), "alpha");
        assert_eq!(realm_arg("oauth", Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn oauth_realm_rejects_other_realms() {
        let error = realm_arg("oauth", Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn export_path_rejects_ids_with_path_separators() {
        let error = export_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn export_path_uses_the_oauth_workspace_tree() {
        assert_eq!(
            export_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/service_C1.json")
        );
    }

    #[test]
    fn snapshot_path_rejects_ids_with_path_separators() {
        let error = snapshot_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn snapshot_path_uses_snapshots_sibling_directory() {
        assert_eq!(
            snapshot_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/.snapshots/service_C1.json")
        );
    }

    #[test]
    fn create_refuses_an_existing_client_without_force() {
        let error = ensure_create_allowed(true, false, "existing-client").unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert!(error.to_string().contains("--force"));
        assert!(ensure_create_allowed(true, true, "existing-client").is_ok());
        assert!(ensure_create_allowed(false, false, "new-client").is_ok());
    }

    #[test]
    fn create_flags_parse_without_an_argv_secret_value() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic",
            "oauth",
            "create",
            "test-client",
            "--from",
            "source.json",
            "--name",
            "Test client",
            "--scope",
            "openid",
            "--scope",
            "profile",
            "--secret-stdin",
            "--token-endpoint-auth-method",
            "client_secret_post",
            "--access-token-lifetime",
            "0",
            "--force",
            "--yes",
        ])
        .unwrap();

        let Some(crate::cli::Command::Oauth {
            command: OauthCommand::Create { id, options },
        }) = cli.command
        else {
            panic!("expected oauth create");
        };
        assert_eq!(id, "test-client");
        assert_eq!(options.from, Some(PathBuf::from("source.json")));
        assert_eq!(options.name.as_deref(), Some("Test client"));
        assert_eq!(options.scope, ["openid", "profile"]);
        assert!(options.secret_stdin);
        assert_eq!(
            options.token_endpoint_auth_method.as_deref(),
            Some("client_secret_post")
        );
        assert_eq!(options.access_token_lifetime, Some(0));
        assert!(options.force);
        assert!(options.yes);

        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "oauth",
                "create",
                "test-client",
                "--secret",
                "visible-in-argv"
            ])
            .is_err()
        );
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "oauth",
                "create",
                "test-client",
                "--secret-stdin",
                "--generate-secret"
            ])
            .is_err()
        );
    }

    #[test]
    fn grant_commands_parse_repeatable_grants_and_production_confirmation() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic",
            "oauth",
            "grant",
            "add",
            "existing-client",
            "client_credentials",
            "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "--realm",
            "bravo",
            "--tenant",
            "sandbox",
            "--yes",
        ])
        .unwrap();

        let Some(crate::cli::Command::Oauth {
            command:
                OauthCommand::Grant {
                    command: GrantCommand::Add(args),
                },
        }) = cli.command
        else {
            panic!("expected oauth grant add");
        };
        assert_eq!(args.id, "existing-client");
        assert_eq!(
            args.grant,
            [
                "client_credentials",
                "urn:ietf:params:oauth:grant-type:jwt-bearer"
            ]
        );
        assert_eq!(args.realm.as_deref(), Some("bravo"));
        assert_eq!(args.tenant.as_deref(), Some("sandbox"));
        assert!(args.yes);

        assert!(
            crate::cli::Cli::try_parse_from(["aic", "oauth", "grant", "remove", "existing-client"])
                .is_err()
        );
    }

    #[test]
    fn generated_secret_is_256_bits_of_url_safe_random_data() {
        let secret = generate_client_secret();
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(secret)
            .unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn failed_schema_fetch_becomes_validation_fallback() {
        let schema = schema_for_validation(Err(Error::Config("schema unavailable".into())));
        let body = json!({
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["tenant-future-grant"]
            }
        });

        assert!(schema.is_none());
        assert!(spec::validate_enumerated_fields(&body, schema.as_ref()).is_ok());
    }

    fn client_with(name: &str, secret: &str, rev: &str) -> Value {
        json!({
            "_id": "client-a",
            "_rev": rev,
            "coreOAuth2ClientConfig": {
                "clientName": {"inherited": false, "value": [name]},
                "userpassword": secret
            }
        })
    }

    #[test]
    fn push_decision_returns_nothing_to_do_when_local_matches_remote() {
        let local = client_with("same", "secret", "local-rev");
        let remote = client_with("same", "secret", "remote-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::NothingToDo
        );
    }

    #[test]
    fn push_decision_pushes_when_remote_matches_snapshot_and_local_differs() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_when_remote_drifted_without_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Blocked(PushBlockReason::RemoteDrift)
        );
    }

    #[test]
    fn push_decision_allows_remote_drift_with_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), true),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_missing_snapshot_without_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        );
    }

    #[test]
    fn push_decision_allows_missing_snapshot_with_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, true),
            PushDecision::Push
        );
    }
}
