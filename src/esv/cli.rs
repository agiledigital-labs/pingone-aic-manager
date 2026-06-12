//! `aic esv` parser and ESV-variable command implementation.

use clap::Subcommand;

use crate::Result;
use crate::cli::{print_json, prod_hint, tenant_for};
use crate::secrets::cli::SecretCommand;

#[derive(Subcommand, Debug)]
pub enum EsvCommand {
    /// List ESV variables. Outputs the `result` array as JSON.
    List {
        /// Override the current context for this call.
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Get a single variable as JSON.
    Get {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
    },
    /// Create or update a variable.
    Set {
        id: String,
        /// Plain value (stored base64-encoded as `valueBase64`).
        #[arg(long)]
        value: String,
        /// expressionType: string, int, bool, list, object, array, keyvaluelist.
        #[arg(long = "type", default_value = "string")]
        expr_type: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete a variable.
    Delete {
        id: String,
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Apply pending changes by restarting the tenant runtime.
    Apply {
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Secret operations (versioned, write-only values).
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
}

pub async fn run(cmd: EsvCommand) -> Result<()> {
    use crate::esv::api;
    match cmd {
        EsvCommand::List { tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&api::list_variables(&t).await?)
        }
        EsvCommand::Get { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&api::get_variable(&t, &id).await?)
        }
        EsvCommand::Set {
            id,
            value,
            expr_type,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            use base64::Engine as _;
            let value_b64 = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
            // Shared with the TUI: handles the AIC quirk that an existing
            // variable's type can't change in place (DELETE-then-PUT).
            let saved = prod_hint(
                api::save_variable(&t, &id, &description, &expr_type, &value_b64, yes, None).await,
            )?;
            let verb = if saved.created { "created" } else { "saved" };
            let extra = if saved.type_deleted {
                " (type changed — recreated)"
            } else {
                ""
            };
            println!("variable {id} {verb}{extra}");
            Ok(())
        }
        EsvCommand::Delete { id, tenant, yes } => {
            let t = tenant_for(tenant)?;
            prod_hint(api::delete_variable(&t, &id, yes).await)?;
            println!("variable {id} deleted");
            Ok(())
        }
        EsvCommand::Apply { tenant, yes } => {
            let t = tenant_for(tenant)?;
            print_json(&prod_hint(api::trigger_restart(&t, yes).await)?)
        }
        EsvCommand::Secret { command } => crate::secrets::cli::run(command).await,
    }
}
