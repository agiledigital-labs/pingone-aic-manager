//! `aic journey` parser and command implementation (read-only).

use std::path::{PathBuf, is_separator};

use clap::Subcommand;

use crate::cli::tenant_for;
use crate::config::ProjectConfig;
use crate::journey::api;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum JourneyCommand {
    /// List journey (auth tree) names in a realm.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Pull a journey (tree + all its nodes) into the workspace as JSON.
    Pull {
        /// Journey (authentication tree) name.
        name: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
}

fn journey_realm(realm: Option<String>) -> Result<String> {
    let realm = realm.unwrap_or_else(|| "alpha".to_string());
    if realm == "alpha" || realm == "bravo" {
        Ok(realm)
    } else {
        Err(Error::Config(format!(
            "invalid journey realm {realm:?}; use alpha or bravo"
        )))
    }
}

fn export_path(tenant: &str, realm: &str, name: &str) -> Result<PathBuf> {
    if name.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "journey name {name:?} contains a path separator"
        )));
    }
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("journeys")
        .join(realm)
        .join(format!("{name}.json")))
}

pub async fn run(cmd: JourneyCommand) -> Result<()> {
    match cmd {
        JourneyCommand::List { realm, tenant } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            for name in api::list_trees(&tenant, &realm).await? {
                println!("{name}");
            }
            Ok(())
        }
        JourneyCommand::Pull {
            name,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let path = export_path(&tenant, &realm, &name)?;
            let export = api::pull(&tenant, &realm, &name).await?;
            let node_count = export.nodes.len();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, serde_json::to_vec_pretty(&export)?)?;
            println!(
                "pulled journey {name:?} ({node_count} nodes) -> {}",
                path.display()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journey_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(journey_realm(None).unwrap(), "alpha");
        assert_eq!(journey_realm(Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn journey_realm_rejects_other_realms() {
        let error = journey_realm(Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn export_path_rejects_names_with_path_separators() {
        let error = export_path("sandbox", "alpha", "folder/GetIP").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn export_path_uses_the_journey_workspace_tree() {
        assert_eq!(
            export_path("sandbox", "bravo", "GetIP").unwrap(),
            PathBuf::from("workspace/sandbox/journeys/bravo/GetIP.json")
        );
    }
}
