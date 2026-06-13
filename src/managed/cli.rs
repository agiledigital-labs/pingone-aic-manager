//! `aic managed` parser and command implementation (read-only slice).
//!
//! Hook *editing* is `aic script` territory (`aic script pull
//! managed/alpha_user.onCreate`); this command inspects the schema and tells
//! you which hooks are syncable. Schema property editing is the planned
//! follow-up (see PLAN.md).

use clap::Subcommand;

use crate::Result;
use crate::cli::{print_json, tenant_for};
use crate::managed::api;

#[derive(Subcommand, Debug)]
pub enum ManagedCommand {
    /// List managed object types with property and hook counts.
    List {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Print one managed object's full definition as JSON.
    Get {
        /// Object name, e.g. alpha_user.
        name: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
}

pub async fn run(cmd: ManagedCommand) -> Result<()> {
    match cmd {
        ManagedCommand::List { tenant } => {
            let t = tenant_for(tenant)?;
            let doc = api::get_managed(&t).await?;
            let summaries = api::summarize(&doc)?;
            for s in &summaries {
                let mut hooks: Vec<String> = s.hooks_inline.clone();
                hooks.extend(
                    s.hooks_file
                        .iter()
                        .map(|h| format!("{h} (file, read-only)")),
                );
                let hooks = if hooks.is_empty() {
                    "-".to_string()
                } else {
                    hooks.join(", ")
                };
                println!(
                    "{:<24} {:>3} properties   hooks: {}",
                    s.name, s.properties, hooks
                );
            }
            if summaries.is_empty() {
                println!("(no managed objects)");
            } else {
                println!(
                    "\nhook scripts sync via: aic script pull managed/<object>.<hook>  (see aic script list managed)"
                );
            }
            Ok(())
        }
        ManagedCommand::Get { name, tenant } => {
            let t = tenant_for(tenant)?;
            let doc = api::get_managed(&t).await?;
            print_json(api::object_named(&doc, &name)?)
        }
    }
}
