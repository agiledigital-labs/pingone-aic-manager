//! `aic managed` parser and command implementation (read-only slice).
//!
//! Hook *editing* is `aic script` territory (`aic script pull
//! managed/alpha_user.onCreate`); this command inspects the schema and tells
//! you which hooks are syncable. Schema property editing is the planned
//! follow-up (see PLAN.md).

use clap::Subcommand;
use serde::Serialize;

use crate::Result;
use crate::cli::{print_json, print_table, tenant_for};
use crate::managed::api;

#[derive(Subcommand, Debug)]
pub enum ManagedCommand {
    /// List managed object types with property and hook counts.
    List {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print managed object summaries as JSON")]
        json: bool,
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
        ManagedCommand::List { tenant, json } => {
            let t = tenant_for(tenant)?;
            let doc = api::get_managed(&t).await?;
            let summaries = api::summarize(&doc)?;
            if json {
                let output = summaries.iter().map(summary_output).collect::<Vec<_>>();
                print_json(&output)?;
            } else {
                let rows = summaries
                    .iter()
                    .map(|s| {
                        vec![
                            s.name.clone(),
                            s.properties.to_string(),
                            join_or_dash(&s.hooks_inline),
                            join_or_dash(&s.hooks_file),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(
                    &["OBJECT", "PROPERTIES", "SYNCABLE_HOOKS", "FILE_HOOKS"],
                    &rows,
                );
            }
            if summaries.is_empty() {
                eprintln!("no managed objects");
            } else {
                eprintln!(
                    "hook scripts sync via: aic script pull managed/<object>.<hook>  (see aic script list managed)"
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

#[derive(Serialize)]
struct ManagedSummaryOutput {
    name: String,
    properties: usize,
    hooks_inline: Vec<String>,
    hooks_file: Vec<String>,
}

fn summary_output(summary: &api::ObjectSummary) -> ManagedSummaryOutput {
    ManagedSummaryOutput {
        name: summary.name.clone(),
        properties: summary.properties,
        hooks_inline: summary.hooks_inline.clone(),
        hooks_file: summary.hooks_file.clone(),
    }
}

fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}
