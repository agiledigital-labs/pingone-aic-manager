//! `aic saml` parser and command implementation.
//!
//! Read-only. `metadata inspect|sanitise` are offline file transforms;
//! `list`/`show` read the `realm-config/saml2` JSON collection; and
//! `metadata export` fetches standard metadata from a JSP that takes **no
//! authentication**, so it too works against a locked daemon. Import, create,
//! delete and cert rotation are later slices.
//!
//! [`needs_tenant_auth`] is where that three-way split is recorded, and it is
//! the one thing a new verb must classify itself in.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::Result;
use crate::cli::{print_json, print_table, realm_arg, tenant_config_for, tenant_for};
use crate::saml::api;
use crate::saml::metadata::{self, SanitiseOpts};
use crate::saml::spec::{self, ExportOutcome, Located, Location, Role};

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
        SamlCommand::List { .. } | SamlCommand::Show { .. } => true,
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
    }
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
}
