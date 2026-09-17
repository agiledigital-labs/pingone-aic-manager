//! `aic saml` parser and command implementation.
//!
//! `metadata inspect|sanitise` are offline file transforms; `list`, `show` and
//! `cot list|show` read the `realm-config` JSON collections; `create-hosted`
//! and `delete` write; and `metadata export` fetches standard metadata from a
//! JSP that takes **no authentication**, so it works against a locked daemon.
//! Import and cert rotation are later slices.
//!
//! [`needs_tenant_auth`] is where that three-way split is recorded, and it is
//! the one thing a new verb must classify itself in.
//!
//! Two behaviours here exist only because AM will not provide them:
//!
//! - **`create-hosted` imposes `entityId` and `metaAlias`.** AM requires
//!   neither — `{}` is a 201 with a UUID name, and a role block without
//!   `services.metaAlias` is a 500 that names no field. Both checks are in
//!   [`spec::build_hosted_create`], before any tenant contact.
//! - **`delete` reads the circle-of-trust collection first.** An entity
//!   `DELETE` silently rewrites every CoT that listed the entity, and nothing
//!   in the delete response says so.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::cli::force::OperationForce;
use crate::cli::{
    ensure_prod_confirmed, print_json, print_table, realm_arg, tenant_config_for, tenant_for,
};
use crate::saml::api;
use crate::saml::metadata::{self, SanitiseOpts};
use crate::saml::spec::{self, ExportOutcome, HostedCreate, Located, Location, Role};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum SamlCommand {
    /// List the realm's SAML 2.0 entity providers.
    List {
        /// Only entities in this collection.
        #[arg(long, value_enum)]
        location: Option<Location>,
        /// Only entities holding this role. An entity may hold both.
        #[arg(long, value_enum)]
        role: Option<Role>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show one entity provider's full configuration.
    Show {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// Skip the lookup and read this collection directly.
        #[arg(long, value_enum)]
        location: Option<Location>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// SAML 2.0 metadata: offline transforms, and export from the tenant.
    Metadata {
        #[command(subcommand)]
        command: MetadataCommand,
    },
    /// Circles of trust, as the CoT documents record them.
    Cot {
        #[command(subcommand)]
        command: CotCommand,
    },
    /// Create a hosted entity provider with one role.
    ///
    /// AM requires neither an entity id nor a metaAlias and fails opaquely
    /// without the second; both are mandatory here.
    CreateHosted {
        /// The entity ID to create. Required: AM would mint a UUID instead.
        entity_id: String,
        /// Which role block to create. An entity may later hold both.
        #[arg(long, value_enum)]
        role: Role,
        /// The role's routing key, `/<realm>/<name>`.
        #[arg(long, value_name = "ALIAS")]
        meta_alias: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete an entity provider — and, with it, its circle-of-trust entries.
    Delete {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        /// Skip the lookup and delete from this collection directly.
        #[arg(long, value_enum)]
        location: Option<Location>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        force: OperationForce,
    },
}

#[derive(Subcommand, Debug)]
pub enum CotCommand {
    /// List the realm's circles of trust.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show one circle of trust.
    Show {
        /// The CoT name. Plain, not base64url — unlike an entity id.
        name: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MetadataCommand {
    /// Describe a metadata document: entity id, roles, endpoints, certs, and
    /// what sanitise would remove.
    Inspect {
        /// Path to a SAML 2.0 `EntityDescriptor` XML file.
        file: PathBuf,
    },
    /// Strip what an AM import cannot take, splicing the original buffer.
    Sanitise {
        /// Path to a SAML 2.0 `EntityDescriptor` XML file.
        file: PathBuf,
        /// Write the spliced XML here. Default: stdout.
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Keep an enveloped signature that no longer matches the content.
        #[arg(long)]
        keep_signature: bool,
    },
    /// Fetch an entity's standard metadata XML from the tenant.
    ///
    /// The export endpoint is unauthenticated, so this needs no unlock.
    Export {
        /// The entity ID, exactly as the tenant stores it.
        entity_id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Write the XML here. Default: stdout.
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
    },
}

/// Whether the root pre-flight must unlock the agent for this command.
///
/// Three classes, and the `metadata` group straddles two of them, which is why
/// the inner match is exhaustive rather than a wildcard:
///
/// - `inspect` / `sanitise` are local file rewrites;
/// - `metadata export` reaches the tenant but over an **unauthenticated** JSP
///   (verified: identical bodies with and without a bearer,
///   `docs/api/06-saml.md`), so requiring an unlock would demand a credential
///   the request never sends;
/// - `list` / `show` are ordinary bearer-authenticated AM reads.
///
/// Putting the whole `Saml` group on either side would be wrong in one
/// direction or the other. A new verb must add its own arm here.
pub fn needs_tenant_auth(command: &SamlCommand) -> bool {
    match command {
        SamlCommand::List { .. }
        | SamlCommand::Show { .. }
        | SamlCommand::Cot { .. }
        | SamlCommand::CreateHosted { .. }
        | SamlCommand::Delete { .. } => true,
        SamlCommand::Metadata { command } => match command {
            MetadataCommand::Inspect { .. }
            | MetadataCommand::Sanitise { .. }
            | MetadataCommand::Export { .. } => false,
        },
    }
}

pub async fn run(command: SamlCommand) -> Result<()> {
    match command {
        SamlCommand::List {
            location,
            role,
            realm,
            tenant,
            json,
        } => list(tenant, realm, location, role, json).await,
        SamlCommand::Show {
            entity_id,
            location,
            realm,
            tenant,
            json,
        } => show(tenant, realm, &entity_id, location, json).await,
        SamlCommand::Metadata { command } => run_metadata(command).await,
        SamlCommand::Cot { command } => run_cot(command).await,
        SamlCommand::CreateHosted {
            entity_id,
            role,
            meta_alias,
            realm,
            tenant,
            yes,
        } => {
            create_hosted(
                tenant,
                realm,
                HostedCreate {
                    entity_id,
                    role,
                    meta_alias,
                },
                yes,
            )
            .await
        }
        SamlCommand::Delete {
            entity_id,
            location,
            realm,
            tenant,
            yes,
            force,
        } => delete(tenant, realm, &entity_id, location, yes, force).await,
    }
}

async fn run_cot(command: CotCommand) -> Result<()> {
    match command {
        CotCommand::List {
            realm,
            tenant,
            json,
        } => cot_list(tenant, realm, json).await,
        CotCommand::Show {
            name,
            realm,
            tenant,
            json,
        } => cot_show(tenant, realm, &name, json).await,
    }
}

/// List the realm's circles of trust.
///
/// The caveat is part of the **human** rendering and goes to stdout with it,
/// the way [`spec::cot_show_lines`] ends its block — including when the realm
/// has none, which is the reading most likely to be taken as proof that
/// nothing trusts anything. `--json` puts it on stderr instead, so the JSON
/// stream stays clean for a caller that is parsing it.
async fn cot_list(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let documents = api::list_cots(&tenant, &realm).await?;

    if json {
        eprintln!("{}", spec::COT_MEMBERSHIP_CAVEAT);
        return print_json(&documents);
    }
    let cots = spec::sort_cots(spec::cots(&documents)?);
    if cots.is_empty() {
        println!("no circles of trust in realm {realm}");
    } else {
        print_table(&spec::COT_LIST_HEADERS, &spec::cot_rows(&cots));
    }
    println!();
    println!("{}", spec::COT_MEMBERSHIP_CAVEAT);
    Ok(())
}

async fn cot_show(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    name: &str,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let document = api::read_cot(&tenant, &realm, name).await?;
    if json {
        eprintln!("{}", spec::COT_MEMBERSHIP_CAVEAT);
        return print_json(&document);
    }
    for line in spec::cot_show_lines(&spec::cot(&document)?) {
        println!("{line}");
    }
    Ok(())
}

/// Create a hosted entity provider.
///
/// The entity-id collision check is a guard, not a measurement: what AM does
/// with `_action=create` against an id that already exists was never probed,
/// and the two outcomes it could have — a 500, or a silent replacement of a
/// configured entity — are both worse than a named refusal here.
async fn create_hosted(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    request: HostedCreate,
    yes: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let body = spec::build_hosted_create(&realm, &request)?;

    let stubs = api::list(&tenant, &realm).await?;
    let existing = match spec::locate(&request.entity_id, &stubs) {
        Located::NotFound => None,
        Located::In(location) => Some(location.to_string()),
        Located::Ambiguous(locations) => Some(
            locations
                .iter()
                .map(|location| location.as_str())
                .collect::<Vec<_>>()
                .join(" and "),
        ),
    };
    if let Some(where_it_is) = existing {
        let id = request.entity_id.trim();
        return Err(Error::Config(format!(
            "SAML entity provider {id:?} already exists in realm {realm} as \
             {where_it_is}; `aic saml show {id} --realm {realm}` shows it"
        )));
    }

    let created = api::create_hosted(&tenant, &realm, body, ok.confirmed_prod).await?;
    // The 201 body is a stub, not the created document. `spec::created_lines`
    // is what keeps the report honest about that: the id is AM's, the role and
    // the alias are ours, and nothing here was read back.
    for line in spec::created_lines(
        created.get("entityId").and_then(serde_json::Value::as_str),
        &request,
        &tenant,
        &realm,
    ) {
        println!("{line}");
    }
    Ok(())
}

/// Delete an entity provider, naming the circle-of-trust cascade first.
///
/// Without `--force` this performs the two reads and prints nothing but the
/// cascade, which makes the refusal path the preview — no separate `--dry-run`
/// flag, and no permission token that a preview could carry by accident.
async fn delete(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    yes: bool,
    force: OperationForce,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let location = match location {
        Some(location) => location,
        None => resolve_location(&tenant, &realm, entity_id).await?,
    };

    let cots = spec::cots(&api::list_cots(&tenant, &realm).await?)?;
    let affected = spec::cots_naming(entity_id, &cots);
    for line in spec::cascade_lines(entity_id, &realm, &affected) {
        eprintln!("{line}");
    }

    spec::delete_ok(force.operation(), entity_id, location, &tenant, &realm)?;

    api::delete_entity(&tenant, &realm, location, entity_id, ok.confirmed_prod).await?;
    println!("deleted SAML entity provider {entity_id} ({location}) from {tenant}/{realm}");

    if affected.is_empty() {
        return Ok(());
    }
    // Read the cascade back. AM performs it, we do not, and the delete
    // response says nothing about it — so printing `affected` here would be
    // reporting what we expected instead of what happened. A failed re-read
    // costs the report, never the delete, which has already landed.
    match api::list_cots(&tenant, &realm).await {
        Ok(documents) => {
            let after = spec::cots_naming(entity_id, &spec::cots(&documents)?);
            for line in spec::cascade_outcome_lines(entity_id, &affected, &after) {
                println!("{line}");
            }
        }
        Err(error) => eprintln!(
            "warning: the entity was deleted, but re-reading the circles of trust failed, \
             so the cascade is unconfirmed: {error}"
        ),
    }
    Ok(())
}

async fn list(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    location: Option<Location>,
    role: Option<Role>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let stubs = spec::select(api::list(&tenant, &realm).await?, location, role);

    if json {
        return print_json(&stubs);
    }
    if stubs.is_empty() {
        // Name the realm. The project default is `alpha` and a tenant's SAML
        // entities commonly live in `bravo`, so a bare "no entities" reads as
        // "this tenant has no SAML" when it means "not in the realm you did
        // not choose".
        println!(
            "no SAML entity providers in realm {realm}{}",
            filters(location, role)
        );
        return Ok(());
    }
    print_table(&spec::LIST_HEADERS, &spec::list_rows(&stubs));
    Ok(())
}

/// The filter clause for the empty-list message, so it says what was searched
/// for as well as where.
fn filters(location: Option<Location>, role: Option<Role>) -> String {
    let mut clauses = Vec::new();
    if let Some(location) = location {
        clauses.push(format!("location {location}"));
    }
    if let Some(role) = role {
        clauses.push(format!("role {}", role.wire()));
    }
    if clauses.is_empty() {
        String::new()
    } else {
        format!(" matching {}", clauses.join(" and "))
    }
}

async fn show(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    location: Option<Location>,
    json: bool,
) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let location = match location {
        Some(location) => location,
        None => resolve_location(&tenant, &realm, entity_id).await?,
    };
    let entity = api::read(&tenant, &realm, location, entity_id).await?;
    if json {
        return print_json(&entity);
    }
    for line in spec::summarise(&entity, location).lines() {
        println!("{line}");
    }
    Ok(())
}

/// One cheap list call, so a right entity ID in the wrong collection is a
/// named error instead of the 404 the read would otherwise answer with.
async fn resolve_location(tenant: &str, realm: &str, entity_id: &str) -> Result<Location> {
    let stubs = api::list(tenant, realm).await?;
    match spec::locate(entity_id, &stubs) {
        Located::In(location) => Ok(location),
        Located::NotFound => Err(crate::Error::Config(format!(
            "no SAML entity provider {entity_id:?} in realm {realm}; \
             `aic saml list --realm {realm}` shows what is there"
        ))),
        Located::Ambiguous(locations) => Err(crate::Error::Config(format!(
            "SAML entity provider {entity_id:?} exists in realm {realm} as {}; \
             pass --location to choose",
            locations
                .iter()
                .map(|location| location.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        ))),
    }
}

async fn run_metadata(command: MetadataCommand) -> Result<()> {
    match command {
        MetadataCommand::Export {
            entity_id,
            realm,
            tenant,
            out,
        } => export(tenant, realm, &entity_id, out.as_deref()).await,
        MetadataCommand::Inspect { file } => print_json(&inspect_file(&file)?),
        MetadataCommand::Sanitise {
            file,
            out,
            keep_signature,
        } => {
            let xml = read_xml(&file)?;
            let result = metadata::sanitise(&xml, SanitiseOpts { keep_signature })?;
            write_xml(out.as_deref(), &result.bytes)?;
            for removal in &result.removed {
                eprintln!("{removal}");
            }
            if result.stale_signature {
                eprintln!(
                    "warning: enveloped signature kept over content that changed; it will not verify"
                );
            }
            Ok(())
        }
    }
}

/// Fetch standard metadata and refuse to write anything that is not metadata.
///
/// The endpoint answers **200 for both outcomes**, so the classification is
/// the whole of the safety here: without it, `--out entity.xml` cheerfully
/// saves the string `ERROR : No metadata for entity …` under an `.xml` name
/// and the failure surfaces later, somewhere else.
async fn export(
    tenant_arg: Option<String>,
    realm_arg_value: Option<String>,
    entity_id: &str,
    out: Option<&Path>,
) -> Result<()> {
    let tenant = tenant_config_for(tenant_arg)?;
    let realm = realm_arg("saml", realm_arg_value)?;
    let body = api::export_metadata(&tenant, &realm, entity_id).await?;
    match spec::classify_export(body.as_bytes()) {
        ExportOutcome::Metadata => write_xml(out, body.as_bytes()),
        ExportOutcome::TenantError(message) => Err(crate::Error::Config(format!(
            "tenant {} refused to export {entity_id:?} from realm {realm}: {message}",
            tenant.name
        ))),
        ExportOutcome::Unrecognised(excerpt) => Err(crate::Error::Config(format!(
            "tenant {} answered the metadata export for {entity_id:?} in realm {realm} \
             with something that is not SAML metadata: {excerpt}",
            tenant.name
        ))),
    }
}

fn inspect_file(path: &Path) -> Result<metadata::MetadataDoc> {
    Ok(metadata::inspect(&read_xml(path)?)?)
}

fn read_xml(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| {
        crate::Error::Config(format!("read SAML metadata {}: {error}", path.display()))
    })
}

fn write_xml(output: Option<&Path>, bytes: &[u8]) -> Result<()> {
    match output {
        Some(path) => std::fs::write(path, bytes).map_err(|error| {
            crate::Error::Config(format!("write SAML metadata {}: {error}", path.display()))
        }),
        None => {
            let mut stdout = std::io::stdout();
            stdout.write_all(bytes)?;
            stdout.flush()?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    const ENTRA: &[u8] = include_bytes!("fixtures/entra-federationmetadata.xml");
    const ENTRA_SANITISED: &[u8] =
        include_bytes!("fixtures/entra-federationmetadata.sanitised.xml");

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-saml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[tokio::test]
    async fn sanitise_command_writes_the_committed_entra_pair() {
        let dir = temp_dir();
        let input = dir.join("in.xml");
        let output = dir.join("out.xml");
        std::fs::write(&input, ENTRA).expect("write input");

        run(SamlCommand::Metadata {
            command: MetadataCommand::Sanitise {
                file: input,
                out: Some(output.clone()),
                keep_signature: false,
            },
        })
        .await
        .expect("sanitise");

        assert_eq!(
            std::fs::read(&output).expect("read output"),
            ENTRA_SANITISED
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn inspect_command_reads_the_same_file_the_library_does() {
        let dir = temp_dir();
        let input = dir.join("in.xml");
        std::fs::write(&input, ENTRA).expect("write input");

        run(SamlCommand::Metadata {
            command: MetadataCommand::Inspect {
                file: input.clone(),
            },
        })
        .await
        .expect("inspect");

        let doc = inspect_file(&input).expect("parse");
        assert_eq!(
            doc.entity_id,
            "https://sts.windows.net/00000000-0000-0000-0000-000000000000/"
        );
        assert_eq!(doc.roles, vec![metadata::Role::IdentityProvider]);
        assert_eq!(doc.would_remove.len(), 3);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn keep_signature_reaches_the_library_opt() {
        let dir = temp_dir();
        let input = dir.join("in.xml");
        let output = dir.join("out.xml");
        std::fs::write(&input, ENTRA).expect("write input");

        run(SamlCommand::Metadata {
            command: MetadataCommand::Sanitise {
                file: input,
                out: Some(output.clone()),
                keep_signature: true,
            },
        })
        .await
        .expect("sanitise");

        let written = std::fs::read(&output).expect("read output");
        assert_ne!(written, ENTRA_SANITISED);
        assert!(
            written
                .windows(b"<ds:Signature>".len())
                .any(|window| window == b"<ds:Signature>"),
            "keep_signature did not reach SanitiseOpts"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn read_verbs_parse_with_their_filters() {
        use crate::cli::{Cli, Command};

        let Some(Command::Saml {
            command:
                SamlCommand::List {
                    location,
                    role,
                    realm,
                    json,
                    ..
                },
        }) = Cli::try_parse_from([
            "aic",
            "saml",
            "list",
            "--location",
            "remote",
            "--role",
            "idp",
            "--realm",
            "bravo",
            "--json",
        ])
        .unwrap()
        .command
        else {
            panic!("expected saml list");
        };
        assert_eq!(location, Some(Location::Remote));
        assert_eq!(role, Some(Role::Idp));
        assert_eq!(realm.as_deref(), Some("bravo"));
        assert!(json);

        // Both filters are optional, and omitting `--location` is what asks
        // the CLI to infer it.
        let Some(Command::Saml {
            command:
                SamlCommand::Show {
                    entity_id,
                    location,
                    ..
                },
        }) = Cli::try_parse_from(["aic", "saml", "show", "https://sp-a.example.com/"])
            .unwrap()
            .command
        else {
            panic!("expected saml show");
        };
        assert_eq!(entity_id, "https://sp-a.example.com/");
        assert_eq!(location, None);

        let Some(Command::Saml {
            command:
                SamlCommand::Metadata {
                    command: MetadataCommand::Export { out, realm, .. },
                },
        }) = Cli::try_parse_from([
            "aic",
            "saml",
            "metadata",
            "export",
            "https://sp-a.example.com",
            "--realm",
            "bravo",
            "--out",
            "entity.xml",
        ])
        .unwrap()
        .command
        else {
            panic!("expected saml metadata export");
        };
        // `--out`, matching `jwt-bearer key export` and `access get`. `--output`
        // must not be accepted, or the two spellings drift apart.
        assert_eq!(out.as_deref(), Some(Path::new("entity.xml")));
        assert_eq!(realm.as_deref(), Some("bravo"));
        assert!(
            Cli::try_parse_from([
                "aic",
                "saml",
                "metadata",
                "export",
                "https://sp-a.example.com",
                "--output",
                "entity.xml",
            ])
            .is_err()
        );
    }

    /// The empty-list line names the realm it searched, because the project
    /// default is `alpha` and a tenant's SAML entities commonly live in
    /// `bravo` — "no entities" otherwise reads as "this tenant has none".
    #[test]
    fn the_empty_list_message_says_what_was_searched_for() {
        assert_eq!(filters(None, None), "");
        assert_eq!(
            filters(Some(Location::Hosted), None),
            " matching location hosted"
        );
        assert_eq!(
            filters(Some(Location::Remote), Some(Role::Sp)),
            " matching location remote and role serviceProvider"
        );
    }

    #[test]
    fn metadata_commands_parse_as_nested_subcommands() {
        use crate::cli::{Cli, Command};

        let inspect =
            Cli::try_parse_from(["aic", "saml", "metadata", "inspect", "in.xml"]).unwrap();
        assert!(matches!(
            inspect.command,
            Some(Command::Saml {
                command: SamlCommand::Metadata {
                    command: MetadataCommand::Inspect { .. }
                }
            })
        ));

        let sanitise = Cli::try_parse_from([
            "aic",
            "saml",
            "metadata",
            "sanitise",
            "in.xml",
            "--out",
            "out.xml",
            "--keep-signature",
        ])
        .unwrap();
        let Some(Command::Saml {
            command:
                SamlCommand::Metadata {
                    command:
                        MetadataCommand::Sanitise {
                            out,
                            keep_signature,
                            ..
                        },
                },
        }) = sanitise.command
        else {
            panic!("expected saml metadata sanitise");
        };
        assert_eq!(out.as_deref(), Some(Path::new("out.xml")));
        assert!(keep_signature);
    }
    fn saml_command(argv: &[&str]) -> SamlCommand {
        use crate::cli::{Cli, Command};

        match Cli::try_parse_from(argv)
            .unwrap_or_else(|error| panic!("{argv:?}: {error}"))
            .command
        {
            Some(Command::Saml { command }) => command,
            other => panic!("{argv:?} did not parse as a saml command: {other:?}"),
        }
    }

    /// The classification that decides whether the root pre-flight unlocks the
    /// agent. Getting it wrong in one direction demands a credential the
    /// request never sends; in the other, it sends a tenant write at a locked
    /// daemon. Every verb is listed, so a new one is a visibly missing row.
    #[test]
    fn needs_tenant_auth_follows_what_each_verb_actually_sends() {
        let cases: [(bool, &[&str]); 9] = [
            (true, &["aic", "saml", "list"]),
            (true, &["aic", "saml", "show", "https://sp-a.example.com"]),
            (true, &["aic", "saml", "cot", "list"]),
            (true, &["aic", "saml", "cot", "show", "client-b"]),
            (
                true,
                &[
                    "aic",
                    "saml",
                    "create-hosted",
                    "https://sp-b.example.com",
                    "--role",
                    "sp",
                    "--meta-alias",
                    "/bravo/client-b-sp",
                ],
            ),
            (
                true,
                &[
                    "aic",
                    "saml",
                    "delete",
                    "https://sp-b.example.com",
                    "--force",
                ],
            ),
            // The odd one out, and the reason this function is not a constant:
            // the export JSP takes no authentication at all.
            (
                false,
                &[
                    "aic",
                    "saml",
                    "metadata",
                    "export",
                    "https://sp-a.example.com",
                ],
            ),
            (false, &["aic", "saml", "metadata", "inspect", "in.xml"]),
            (false, &["aic", "saml", "metadata", "sanitise", "in.xml"]),
        ];

        for (expected, argv) in cases {
            assert_eq!(needs_tenant_auth(&saml_command(argv)), expected, "{argv:?}");
        }
    }

    #[test]
    fn cot_verbs_parse_with_the_plain_name() {
        let SamlCommand::Cot {
            command: CotCommand::Show {
                name, realm, json, ..
            },
        } = saml_command(&["aic", "saml", "cot", "show", "client-b", "--realm", "bravo"])
        else {
            panic!("expected saml cot show");
        };
        assert_eq!(name, "client-b");
        assert_eq!(realm.as_deref(), Some("bravo"));
        assert!(!json);

        let SamlCommand::Cot {
            command: CotCommand::List { json, .. },
        } = saml_command(&["aic", "saml", "cot", "list", "--json"])
        else {
            panic!("expected saml cot list");
        };
        assert!(json);
    }

    /// The two fields AM does not require. `--meta-alias` absent is a parse
    /// error rather than a 500 from the tenant; `--meta-alias ""` reaches
    /// `spec::build_hosted_create`, which is the half a parser cannot cover.
    #[test]
    fn create_hosted_requires_the_role_and_the_meta_alias() {
        use crate::cli::Cli;

        let SamlCommand::CreateHosted {
            entity_id,
            role,
            meta_alias,
            yes,
            ..
        } = saml_command(&[
            "aic",
            "saml",
            "create-hosted",
            "https://sp-b.example.com",
            "--role",
            "idp",
            "--meta-alias",
            "/bravo/b-idp",
            "--yes",
        ])
        else {
            panic!("expected saml create-hosted");
        };
        assert_eq!(entity_id, "https://sp-b.example.com");
        assert_eq!(role, Role::Idp);
        assert_eq!(meta_alias, "/bravo/b-idp");
        assert!(yes);

        for argv in [
            // no --meta-alias
            vec![
                "aic",
                "saml",
                "create-hosted",
                "https://sp-b.example.com",
                "--role",
                "sp",
            ],
            // no --role
            vec![
                "aic",
                "saml",
                "create-hosted",
                "https://sp-b.example.com",
                "--meta-alias",
                "/bravo/x",
            ],
            // no entity id
            vec![
                "aic",
                "saml",
                "create-hosted",
                "--role",
                "sp",
                "--meta-alias",
                "/bravo/x",
            ],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_err(),
                "{argv:?} must not parse"
            );
        }
    }

    /// `delete` declares exactly one guard, so an undeclared one is a parse
    /// error rather than a flag that is silently ignored.
    #[test]
    fn delete_declares_only_the_operation_guard() {
        use crate::cli::Cli;

        for (argv, forced) in [
            (
                vec!["aic", "saml", "delete", "https://sp-b.example.com"],
                false,
            ),
            (
                vec![
                    "aic",
                    "saml",
                    "delete",
                    "https://sp-b.example.com",
                    "--force",
                ],
                true,
            ),
            (
                vec![
                    "aic",
                    "saml",
                    "delete",
                    "https://sp-b.example.com",
                    "--force=operation",
                ],
                true,
            ),
        ] {
            let SamlCommand::Delete { force, .. } = saml_command(&argv) else {
                panic!("expected saml delete");
            };
            assert_eq!(force.operation(), forced, "{argv:?}");
        }

        for guard in ["backup", "syntax-check", "nonsense"] {
            assert!(
                Cli::try_parse_from([
                    "aic",
                    "saml",
                    "delete",
                    "https://sp-b.example.com",
                    &format!("--force={guard}"),
                ])
                .is_err(),
                "--force={guard} is not a guard this command supports"
            );
        }
    }

    #[test]
    fn delete_takes_an_explicit_location_and_infers_it_otherwise() {
        let SamlCommand::Delete { location, .. } = saml_command(&[
            "aic",
            "saml",
            "delete",
            "https://sp-b.example.com",
            "--force",
        ]) else {
            panic!("expected saml delete");
        };
        assert_eq!(
            location, None,
            "omitting --location is what asks for a lookup"
        );

        let SamlCommand::Delete { location, .. } = saml_command(&[
            "aic",
            "saml",
            "delete",
            "https://sp-b.example.com",
            "--location",
            "remote",
            "--force",
        ]) else {
            panic!("expected saml delete");
        };
        assert_eq!(location, Some(Location::Remote));
    }
}
