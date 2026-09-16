//! `aic saml` parser and command implementation.
//!
//! Offline only for this slice: inspect and sanitise metadata XML on disk.
//! Nothing here talks to a tenant, so the group must keep working against a
//! locked daemon. Import, export, list, and cert rotation are later slices.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::Result;
use crate::cli::print_json;
use crate::saml::metadata::{self, SanitiseOpts};

#[derive(Subcommand, Debug)]
pub enum SamlCommand {
    /// Offline SAML 2.0 metadata transforms (no tenant, no network).
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
}

/// Whether the root pre-flight must unlock the agent for this command.
///
/// Every verb in this slice is a local file rewrite. When import/export land,
/// this match is what forces an explicit classification — putting the whole
/// `Saml` group on the `true` side would lock a file rewrite behind an unlock.
pub fn needs_tenant_auth(command: &SamlCommand) -> bool {
    match command {
        SamlCommand::Metadata { .. } => false,
    }
}

pub async fn run(command: SamlCommand) -> Result<()> {
    match command {
        SamlCommand::Metadata { command } => run_metadata(command),
    }
}

fn run_metadata(command: MetadataCommand) -> Result<()> {
    match command {
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
