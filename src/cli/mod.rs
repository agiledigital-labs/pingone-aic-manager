//! `aic` CLI subcommands.
//!
//! Two layers:
//!   * agent lifecycle + auth (`agent`, `login`, `logout`, `stop`, `status`,
//!     `ctx`, `whoami`) — talks directly to the daemon over its socket.
//!   * resource commands (`esv list/get/set/delete/apply`, `esv secret …`,
//!     future `script`, `oauth2`, ...) — go through `aic::api` / `aic::esv`,
//!     which are shared with the TUI. Mutations take `--yes` to confirm a
//!     write to a production-themed tenant (mirrors the TUI's prod guard). Do
//!     NOT add tenant-scoped HTTP via reqwest in here; everything tenant-
//!     facing belongs in `aic/` so both surfaces stay in sync.
//!
//! **Onboarding (creating a new tenant + service account) is TUI-only.**
//! The required flows mix browser cookies, interactive TOTP, and RSA
//! keygen; we haven't tried to script them. Run the TUI once per tenant.

use clap::{Parser, Subcommand};

use crate::agent::{self, AgentClient, Request, Response};
use crate::auth;
use crate::config::crypto::Dek;
use crate::config::wraps::WrapsFile;
use crate::config::{self, ProjectConfig};
use crate::{Error, Result};

#[derive(Parser, Debug)]
#[command(
    name = "aic",
    version,
    about = "AIC tenant TUI + CLI",
    long_about = None,
    disable_help_subcommand = true,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the agent (holds DEK + tokens in memory, hands out bearer tokens).
    /// By default runs attached to the current terminal — Ctrl-C to stop, logs
    /// to stderr. The TUI auto-spawns a detached copy via `--detach`.
    Agent {
        /// Spawn a detached child that runs the daemon loop, and exit. Stdio
        /// gets redirected to .aic-edit/agent.log; setsid() puts the child in
        /// its own session so a terminal HUP doesn't kill it.
        #[arg(long)]
        detach: bool,
        /// Idle-lock timeout in seconds (default 3600, or whatever
        /// settings.toml specifies).
        #[arg(long)]
        idle_timeout: Option<u64>,
    },
    /// Unlock the keystore in the agent. Prompts for the master password.
    Login,
    /// Clear the agent's in-memory JWKs + tokens (but leave the agent running).
    Logout,
    /// Stop the agent process.
    Stop,
    /// Show agent state, current context, and cached-token expirations.
    Status,
    /// Manage the active tenant context for this project.
    Ctx {
        #[command(subcommand)]
        command: CtxCommand,
    },
    /// Mint and print a token for the current context (or `--tenant <name>`).
    Whoami {
        #[arg(long)]
        tenant: Option<String>,
    },
    /// ESV operations (variables, secrets).
    Esv {
        #[command(subcommand)]
        command: EsvCommand,
    },
    /// Script workspace sync (AM scripts + IDM endpoints).
    Script {
        #[command(subcommand)]
        command: ScriptCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum EsvCommand {
    /// List ESV variables. Outputs the `result` array as JSON.
    List {
        /// Override the current context for this call.
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Get a single variable as JSON.
    Get {
        id: String,
        #[arg(long)]
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
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete a variable.
    Delete {
        id: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Apply pending changes by restarting the tenant runtime.
    Apply {
        #[arg(long)]
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

#[derive(Subcommand, Debug)]
pub enum SecretCommand {
    /// List secrets (metadata only — values are write-only).
    List {
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Get a single secret's metadata as JSON.
    Get {
        id: String,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Create a secret (PUT is create-only; change values via add-version).
    ///
    /// The value is read (in priority order) from `--value-file`,
    /// `--value-stdin`, or an interactive no-echo prompt. `--value` exists for
    /// scripting but leaks the secret into shell history / process listings —
    /// prefer the file or stdin form.
    Create {
        id: String,
        /// Secret value inline (DISCOURAGED — visible in `ps`/shell history).
        #[arg(long)]
        value: Option<String>,
        /// Read the value from a file (a single trailing newline is stripped).
        #[arg(long)]
        value_file: Option<std::path::PathBuf>,
        /// Read the value from stdin (a single trailing newline is stripped).
        #[arg(long)]
        value_stdin: bool,
        /// generic | pem | base64hmac | base64aes.
        #[arg(long, default_value = "generic")]
        encoding: String,
        /// Validate the value as JSON (generic encoding only).
        #[arg(long)]
        json: bool,
        /// Don't expose as `&{esv.id}` placeholder (loads immediately, no restart).
        #[arg(long)]
        no_placeholders: bool,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Set a secret's description.
    SetDescription {
        id: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// List a secret's versions (newest first).
    Versions {
        id: String,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Add a new version (becomes the active version). Value is encoded with
    /// the secret's existing encoding. Value source as for `create` (prefer
    /// `--value-file` / `--value-stdin` over `--value`).
    AddVersion {
        id: String,
        /// Secret value inline (DISCOURAGED — visible in `ps`/shell history).
        #[arg(long)]
        value: Option<String>,
        /// Read the value from a file (a single trailing newline is stripped).
        #[arg(long)]
        value_file: Option<std::path::PathBuf>,
        /// Read the value from stdin (a single trailing newline is stripped).
        #[arg(long)]
        value_stdin: bool,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Enable a version.
    Enable {
        id: String,
        version: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Disable a version (the latest version can't be disabled).
    Disable {
        id: String,
        version: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Destroy a version — irreversible.
    Destroy {
        id: String,
        version: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Delete a secret and all its versions — irreversible.
    Delete {
        id: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScriptCommand {
    /// Scaffold / refresh the local workspace tree (types, tsconfig, eslint).
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// List scripts on the tenant (not the local workspace).
    List {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        /// Limit to one kind: `am` or `idm` (default: both).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Pull script(s) into the workspace. Without a name, pulls all of `--kind`.
    Pull {
        /// Script name (omit to pull all of the kind).
        name: Option<String>,
        /// `am` (scripts) or `idm` (endpoints).
        #[arg(long, default_value = "am")]
        kind: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        /// Overwrite local edits without backing them up first.
        #[arg(long)]
        force: bool,
    },
    /// Push a local edit back to the tenant (requires a prior pull).
    Push {
        name: String,
        #[arg(long, default_value = "am")]
        kind: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        /// Push past a remote-drift conflict (overwrites remote).
        #[arg(long)]
        force: bool,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Show the sync state of every synced script.
    Status {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Show the 3-way diff (last-synced / remote / local) for one script.
    Diff {
        name: String,
        #[arg(long, default_value = "am")]
        kind: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceCommand {
    /// Create the workspace tree + type definitions for a tenant + realm.
    Init {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
    },
    /// Refresh managed type/config files to the latest bundled version.
    Update {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        realm: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CtxCommand {
    /// List tenants defined in `.aic-edit/config.toml`.
    List,
    /// Print the current context.
    Current,
    /// Switch to a different tenant.
    Use { tenant: String },
}

pub async fn run(cli: Cli) -> Result<()> {
    bootstrap_project_root();
    match cli.command {
        Some(Command::Agent { detach, idle_timeout }) => {
            run_agent(detach, idle_timeout).await
        }
        Some(Command::Login) => login().await,
        Some(Command::Logout) => logout().await,
        Some(Command::Stop) => stop().await,
        Some(Command::Status) => status().await,
        Some(Command::Ctx { command }) => ctx(command).await,
        Some(Command::Whoami { tenant }) => whoami(tenant).await,
        Some(Command::Esv { command }) => esv(command).await,
        Some(Command::Script { command }) => script(command).await,
        None => unreachable!("dispatch handled at top level"),
    }
}

/// Locate the project root (the dir containing `.aic-edit/`) by walking up
/// from the current directory, record any tenant/realm implied by a
/// `workspace/<tenant>/<realm>/` working directory, then chdir to the root so
/// every project-relative path (config, keystore, agent socket) resolves the
/// same no matter which subdirectory the command was invoked from.
fn bootstrap_project_root() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if let Some(root) = config::find_project_root(&cwd) {
        config::set_workspace_context(config::detect_workspace_context(&root, &cwd));
        let _ = std::env::set_current_dir(&root);
    }
}

async fn run_agent(detach: bool, idle_timeout: Option<u64>) -> Result<()> {
    if detach {
        // User asked us to spawn a detached child and exit. The spawn path
        // (agent::client::spawn_detached_agent) handles stdio redirection and
        // setsid; the child then re-enters this function with detach=false
        // and runs the loop.
        return spawn_detached_then_exit();
    }
    // Precedence: CLI flag > settings.toml > 3600s default. Settings is
    // best-effort — if it can't be read we just fall through to the default
    // rather than refusing to start.
    let from_settings = config::Settings::load()
        .ok()
        .flatten()
        .and_then(|s| s.agent_idle_timeout_secs);
    let opts = agent::daemon::DaemonOptions {
        idle_timeout_secs: idle_timeout.or(from_settings).unwrap_or(3600),
    };
    eprintln!("{}", agent::daemon::describe_paths());
    agent::daemon::run(opts).await
}

fn spawn_detached_then_exit() -> Result<()> {
    // Reuse the CLI's spawn path by going through connect_or_spawn — that
    // gives us the "wait for socket" handshake too.
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| Error::Config(format!("tokio runtime: {e}")))?;
    rt.block_on(async {
        let _ = AgentClient::connect_or_spawn().await?;
        eprintln!("agent running at {}", agent::socket_path().display());
        Ok(())
    })
}

async fn login() -> Result<()> {
    if ProjectConfig::load()?.is_none() {
        return Err(Error::Config(
            "no .aic-edit/config.toml here — onboard a tenant in the TUI first".into(),
        ));
    }

    // Plain mode (user opted out of encryption): no DEK to derive, no
    // prompt. Just tell the agent to load keys.plain and exit.
    let plain_mode = matches!(
        config::Settings::load()?,
        Some(config::Settings { encrypt_keys: false, .. })
    );
    if plain_mode {
        if ProjectConfig::load_keys_plain()?.is_none() {
            return Err(Error::Config(
                "no .aic-edit/keys.plain — onboard a tenant in the TUI first".into(),
            ));
        }
        auth::unlock_plain_agent().await?;
        println!("unlocked (plain mode)");
        return print_status_block().await;
    }

    if ProjectConfig::load_keys_enc()?.is_none() {
        return Err(Error::Config(
            "no .aic-edit/keys.enc — set up an auth factor in the TUI first".into(),
        ));
    }
    let wraps_file = WrapsFile::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/wraps.toml — set up an auth factor in the TUI first".into()))?;

    let dek = match pick_method(&wraps_file)? {
        MethodChoice::Password => unlock_with_password_prompt().await?,
        MethodChoice::SecurityKey => unlock_with_security_key_prompt(&wraps_file).await?,
    };

    auth::put_dek_to_agent(&dek).await?;
    println!("unlocked");
    print_status_block().await
}

enum MethodChoice {
    Password,
    SecurityKey,
}

/// Pick an auth method based on what's enrolled. If only one *method* is
/// enrolled (password OR any number of security keys), use it without
/// prompting. If both are enrolled, ask. We don't list individual security
/// keys — the device tells us which credential matches when the user taps,
/// so it's fine to enumerate enrolled wraps internally and just present
/// "Security key" to the user.
fn pick_method(wraps_file: &WrapsFile) -> Result<MethodChoice> {
    let has_password = wraps_file.has_password();
    let has_security_key = wraps_file.has_security_key();

    match (has_password, has_security_key) {
        (false, false) => Err(Error::Config(
            "no auth factors enrolled in wraps.toml".into(),
        )),
        (true, false) => Ok(MethodChoice::Password),
        (false, true) => Ok(MethodChoice::SecurityKey),
        (true, true) => {
            println!("Authentication methods:");
            println!("  1) Master password");
            println!("  2) Security key");
            print!("Choose [1]: ");
            use std::io::Write;
            std::io::stdout()
                .flush()
                .map_err(|e| Error::Config(format!("flush: {e}")))?;
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| Error::Config(format!("read: {e}")))?;
            match line.trim() {
                "" | "1" => Ok(MethodChoice::Password),
                "2" => Ok(MethodChoice::SecurityKey),
                other => Err(Error::Config(format!("invalid choice: {other}"))),
            }
        }
    }
}

async fn unlock_with_password_prompt() -> Result<Dek> {
    let password = rpassword::prompt_password("Master password: ")
        .map_err(|e| Error::Config(format!("read password: {e}")))?;
    Ok(auth::unlock_password(password).await?.dek)
}

async fn unlock_with_security_key_prompt(wraps_file: &WrapsFile) -> Result<Dek> {
    let pin = rpassword::prompt_password("Security key PIN: ")
        .map_err(|e| Error::Config(format!("read pin: {e}")))?;
    eprintln!("{}", crate::security_key::TAP_MESSAGE);
    Ok(auth::unlock_security_key(wraps_file.clone(), pin).await?.dek)
}

async fn logout() -> Result<()> {
    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::Lock).await? {
        Response::Ok => {
            println!("locked");
            Ok(())
        }
        Response::Error { message } => Err(Error::Config(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

async fn stop() -> Result<()> {
    let sock = agent::socket_path();
    if !sock.exists() {
        println!("no agent running");
        return Ok(());
    }
    let client = AgentClient::connect(&sock).await?;
    match client.send(&Request::Shutdown).await? {
        Response::Ok => {
            println!("agent stopping");
            Ok(())
        }
        Response::Error { message } => Err(Error::Config(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

async fn status() -> Result<()> {
    let sock = agent::socket_path();
    if !sock.exists() {
        println!("agent: not running");
        if let Some(cur) = config::read_current_context()? {
            println!("context: {cur}");
        }
        return Ok(());
    }
    print_status_block().await
}

async fn print_status_block() -> Result<()> {
    let client = AgentClient::connect(agent::socket_path()).await?;
    let resp = client.send(&Request::Status).await?;
    let info = match resp {
        Response::Status(s) => s,
        Response::Error { message } => return Err(Error::Config(message)),
        other => return Err(Error::Config(format!("unexpected reply: {other:?}"))),
    };

    let now = chrono::Utc::now().timestamp();
    println!("agent:    running (pid {})", read_pid_or_zero());
    println!("project:  {}", info.project_dir);
    println!("unlocked: {}", info.unlocked);
    if info.unlocked {
        println!(
            "idle:     {}s remaining of {}s",
            info.idle_remaining_secs, info.idle_timeout_secs
        );
    }
    if let Some(cur) = config::read_current_context()? {
        println!("context:  {cur}");
    } else {
        println!("context:  <none>");
    }
    if !info.tenants.is_empty() {
        println!("tenants:  {}", info.tenants.join(", "));
    }
    for t in &info.cached_tokens {
        let ttl = t.expires_at - now;
        if ttl > 0 {
            println!("  token {} → {}s remaining", t.tenant, ttl);
        } else {
            println!("  token {} → expired", t.tenant);
        }
    }
    Ok(())
}

fn read_pid_or_zero() -> u32 {
    std::fs::read_to_string(agent::pid_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

async fn ctx(cmd: CtxCommand) -> Result<()> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    let current = config::read_current_context()?;
    match cmd {
        CtxCommand::List => {
            for t in &cfg.tenants {
                let marker = if Some(&t.name) == current.as_ref() {
                    "* "
                } else {
                    "  "
                };
                println!(
                    "{marker}{}  ({}, {})",
                    t.name,
                    t.theme.label(),
                    t.base_url
                );
            }
        }
        CtxCommand::Current => match current {
            Some(c) => println!("{c}"),
            None => {
                if !cfg.default_tenant.is_empty() {
                    println!("{} (default)", cfg.default_tenant);
                } else {
                    println!("<none>");
                }
            }
        },
        CtxCommand::Use { tenant } => {
            if !cfg.tenants.iter().any(|t| t.name == tenant) {
                return Err(Error::Config(format!(
                    "no tenant named '{tenant}' in config"
                )));
            }
            config::write_current_context(&tenant)?;
            println!("context switched to {tenant}");
        }
    }
    Ok(())
}

async fn whoami(tenant_arg: Option<String>) -> Result<()> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    let tenant = resolve_tenant(tenant_arg, &cfg)?;

    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::GetToken { tenant: tenant.clone() }).await? {
        Response::Token { access_token, expires_at } => {
            let ttl = expires_at - chrono::Utc::now().timestamp();
            println!("tenant:  {tenant}");
            println!("expires: in {ttl}s (unix {expires_at})");
            println!("token:   {}", redact(&access_token));
            Ok(())
        }
        Response::Locked => Err(Error::Auth("agent locked; run `aic login`".into())),
        Response::Error { message } => Err(Error::Auth(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

async fn esv(cmd: EsvCommand) -> Result<()> {
    use crate::aic::esv;
    match cmd {
        EsvCommand::List { tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::list_variables(&t).await?)
        }
        EsvCommand::Get { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::get_variable(&t, &id).await?)
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
                esv::save_variable(&t, &id, &description, &expr_type, &value_b64, yes, None).await,
            )?;
            let verb = if saved.created { "created" } else { "saved" };
            let extra = if saved.type_deleted { " (type changed — recreated)" } else { "" };
            println!("variable {id} {verb}{extra}");
            Ok(())
        }
        EsvCommand::Delete { id, tenant, yes } => {
            let t = tenant_for(tenant)?;
            prod_hint(esv::delete_variable(&t, &id, yes).await)?;
            println!("variable {id} deleted");
            Ok(())
        }
        EsvCommand::Apply { tenant, yes } => {
            let t = tenant_for(tenant)?;
            print_json(&prod_hint(esv::trigger_restart(&t, yes).await)?)
        }
        EsvCommand::Secret { command } => secret(command).await,
    }
}

async fn secret(cmd: SecretCommand) -> Result<()> {
    use crate::aic::esv;
    match cmd {
        SecretCommand::List { tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::list_secrets(&t).await?)
        }
        SecretCommand::Get { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::get_secret(&t, &id).await?)
        }
        SecretCommand::Create {
            id,
            value,
            value_file,
            value_stdin,
            encoding,
            json,
            no_placeholders,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            let value = resolve_secret_value(value, value_file, value_stdin, "Secret value: ")?;
            let value_b64 =
                esv::encode_secret_value(&encoding, &value, json).map_err(Error::Config)?;
            prod_hint(
                esv::create_secret(&t, &id, &encoding, !no_placeholders, &value_b64, &description, yes)
                    .await,
            )?;
            println!("secret {id} created");
            Ok(())
        }
        SecretCommand::SetDescription {
            id,
            description,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            prod_hint(esv::set_secret_description(&t, &id, &description, yes).await)?;
            println!("secret {id} description updated");
            Ok(())
        }
        SecretCommand::Versions { id, tenant } => {
            let t = tenant_for(tenant)?;
            print_json(&esv::list_secret_versions(&t, &id).await?)
        }
        SecretCommand::AddVersion {
            id,
            value,
            value_file,
            value_stdin,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            let value = resolve_secret_value(value, value_file, value_stdin, "New secret value: ")?;
            // A secret's encoding is fixed at create; re-use it for the new
            // version so the value is encoded the same way.
            let encoding = esv::get_secret(&t, &id)
                .await?
                .get("encoding")
                .and_then(|x| x.as_str())
                .unwrap_or("generic")
                .to_string();
            let value_b64 =
                esv::encode_secret_value(&encoding, &value, false).map_err(Error::Config)?;
            let created = prod_hint(esv::create_secret_version(&t, &id, &value_b64, yes).await)?;
            let v = created.get("version").map(json_scalar).unwrap_or_default();
            println!("secret {id}: added version {v}");
            Ok(())
        }
        SecretCommand::Enable {
            id,
            version,
            tenant,
            yes,
        } => set_version_status(&id, &version, "ENABLED", tenant, yes).await,
        SecretCommand::Disable {
            id,
            version,
            tenant,
            yes,
        } => set_version_status(&id, &version, "DISABLED", tenant, yes).await,
        SecretCommand::Destroy {
            id,
            version,
            tenant,
            yes,
        } => {
            let t = tenant_for(tenant)?;
            if !confirm_irreversible(
                &format!("Destroy version {version} of secret {id} on {t}."),
                yes,
            )? {
                println!("aborted");
                return Ok(());
            }
            prod_hint(esv::destroy_secret_version(&t, &id, &version, yes).await)?;
            println!("secret {id} version {version} destroyed");
            Ok(())
        }
        SecretCommand::Delete { id, tenant, yes } => {
            let t = tenant_for(tenant)?;
            if !confirm_irreversible(
                &format!("Delete secret {id} and all its versions on {t}."),
                yes,
            )? {
                println!("aborted");
                return Ok(());
            }
            prod_hint(esv::delete_secret(&t, &id, yes).await)?;
            println!("secret {id} deleted");
            Ok(())
        }
    }
}

async fn script(cmd: ScriptCommand) -> Result<()> {
    use crate::aic::script::{sync, workspace};

    match cmd {
        ScriptCommand::Workspace { command } => match command {
            WorkspaceCommand::Init { tenant, realm } => {
                let t = tenant_for(tenant)?;
                let realm = realm_for(realm)?;
                let r = workspace::init(&t, &realm)?;
                println!(
                    "workspace ready at {} ({} files written, templates v{})",
                    r.tree.display(),
                    r.written.len(),
                    workspace::TEMPLATES_VERSION
                );
                Ok(())
            }
            WorkspaceCommand::Update { tenant, realm } => {
                let t = tenant_for(tenant)?;
                let realm = realm_for(realm)?;
                let r = workspace::update(&t, &realm)?;
                println!(
                    "templates refreshed to v{} ({} files written) at {}",
                    workspace::TEMPLATES_VERSION,
                    r.written.len(),
                    r.tree.display()
                );
                Ok(())
            }
        },
        ScriptCommand::List { tenant, realm, kind } => {
            let t = tenant_for(tenant)?;
            let realm = realm_for(realm)?;
            let kinds = kinds_for(kind.as_deref())?;
            let mut all = Vec::new();
            for k in kinds {
                all.extend(k.list(&t, &realm).await?);
            }
            print_json(&all)
        }
        ScriptCommand::Pull { name, kind, tenant, realm, force } => {
            let t = tenant_for(tenant)?;
            let realm = realm_for(realm)?;
            let k = kind_for(&kind)?;
            // Be friendly: scaffold the workspace on first use so the pulled
            // sources land next to their type definitions.
            if workspace::applied_version(&t, &realm)? == 0 {
                let r = workspace::init(&t, &realm)?;
                println!("initialised workspace at {}", r.tree.display());
            }
            let selector = match name {
                Some(n) => sync::Selector::Name(n),
                None => sync::Selector::All,
            };
            let outcomes = sync::pull(&t, &realm, k, &selector, force).await?;
            if outcomes.is_empty() {
                println!("no {} scripts to pull", k.as_str());
            }
            for o in &outcomes {
                let what = match &o.status {
                    sync::PullStatus::Created => "pulled (new)".to_string(),
                    sync::PullStatus::Updated => "pulled (updated)".to_string(),
                    sync::PullStatus::Unchanged => "unchanged".to_string(),
                    sync::PullStatus::LocalBackedUp(p) => {
                        format!("pulled; local edits backed up to {}", p.display())
                    }
                };
                println!("  {} {}: {}", o.kind.as_str(), o.name, what);
            }
            workspace_update_hint(&t, &realm)?;
            Ok(())
        }
        ScriptCommand::Push { name, kind, tenant, realm, force, yes } => {
            let t = tenant_for(tenant)?;
            let realm = realm_for(realm)?;
            let k = kind_for(&kind)?;
            match prod_hint(sync::push(&t, &realm, k, &name, force, yes).await)? {
                sync::PushOutcome::Pushed => println!("pushed {} {name}", k.as_str()),
                sync::PushOutcome::Unchanged => println!("{name}: no local changes to push"),
                sync::PushOutcome::AlreadyInSync => {
                    println!("{name}: remote already matched local; snapshot refreshed")
                }
                sync::PushOutcome::Conflict(tw) => {
                    print_conflict(&name, &tw);
                    return Err(Error::Config(
                        "remote changed since last sync — resolve, or re-run with --force".into(),
                    ));
                }
            }
            workspace_update_hint(&t, &realm)?;
            Ok(())
        }
        ScriptCommand::Status { tenant, realm, kind } => {
            let t = tenant_for(tenant)?;
            let realm = realm_for(realm)?;
            let only = match kind.as_deref() {
                Some(s) => Some(kind_for(s)?),
                None => None,
            };
            let entries = sync::status(&t, &realm, only).await?;
            if entries.is_empty() {
                println!("nothing synced yet — `aic script pull …` first");
            }
            for e in &entries {
                let label = match e.state {
                    sync::ScriptState::InSync => "in sync",
                    sync::ScriptState::LocallyModified => "modified locally",
                    sync::ScriptState::RemotelyModified => "modified on remote",
                    sync::ScriptState::BothModified => "CONFLICT (both changed)",
                    sync::ScriptState::LocalMissing => "local file missing",
                };
                println!("  {:<4} {:<40} {}", e.kind.as_str(), e.name, label);
            }
            Ok(())
        }
        ScriptCommand::Diff { name, kind, tenant, realm } => {
            let t = tenant_for(tenant)?;
            let realm = realm_for(realm)?;
            let k = kind_for(&kind)?;
            let tw = sync::diff(&t, &realm, k, &name).await?;
            print_conflict(&name, &tw);
            Ok(())
        }
    }
}

/// Parse a `--kind` value into a single [`Kind`].
fn kind_for(s: &str) -> Result<crate::aic::script::Kind> {
    crate::aic::script::Kind::parse(s)
        .ok_or_else(|| Error::Config(format!("unknown --kind {s:?} (use `am` or `idm`)")))
}

/// Parse an optional `--kind`: `None` means all kinds.
fn kinds_for(s: Option<&str>) -> Result<Vec<crate::aic::script::Kind>> {
    match s {
        Some(s) => Ok(vec![kind_for(s)?]),
        None => Ok(crate::aic::script::Kind::all().to_vec()),
    }
}

/// Resolve + validate the realm: explicit `--realm` wins, else a
/// `workspace/<tenant>/<realm>` working directory implies it, else `alpha`.
/// AIC only has `alpha` + `bravo` (CLAUDE.md §4).
fn realm_for(realm: Option<String>) -> Result<String> {
    let realm = realm
        .or_else(|| config::workspace_context().realm)
        .unwrap_or_else(|| "alpha".to_string());
    match realm.as_str() {
        "alpha" | "bravo" => Ok(realm),
        other => Err(Error::Config(format!(
            "invalid realm {other:?} — AIC only has `alpha` and `bravo`"
        ))),
    }
}

/// Print a "templates out of date" nudge if the workspace predates the bundled
/// template version (mirrors p1-sync's update prompt).
fn workspace_update_hint(tenant: &str, realm: &str) -> Result<()> {
    use crate::aic::script::workspace;
    let applied = workspace::applied_version(tenant, realm)?;
    if applied != 0 && applied < workspace::TEMPLATES_VERSION {
        println!(
            "note: workspace templates v{applied} → v{} available — run `aic script workspace update`",
            workspace::TEMPLATES_VERSION
        );
    }
    Ok(())
}

fn print_conflict(name: &str, tw: &crate::aic::script::sync::ThreeWay) {
    println!("=== {name}: last-synced ===\n{}", tw.last_synced);
    println!("=== {name}: remote ===\n{}", tw.remote);
    println!("=== {name}: local ===\n{}", tw.local);
}

async fn set_version_status(
    id: &str,
    version: &str,
    status: &str,
    tenant: Option<String>,
    yes: bool,
) -> Result<()> {
    let t = tenant_for(tenant)?;
    prod_hint(crate::aic::esv::change_version_status(&t, id, version, status, yes).await)?;
    println!("secret {id} version {version} → {status}");
    Ok(())
}

/// Resolve the tenant for a resource command (flag → current context →
/// default), loading the project config.
fn tenant_for(tenant_arg: Option<String>) -> Result<String> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    resolve_tenant(tenant_arg, &cfg)
}

/// Turn the agent's prod-confirm refusal into an actionable CLI message.
fn prod_hint<T>(r: Result<T>) -> Result<T> {
    match r {
        Err(Error::ProdConfirmRequired) => Err(Error::Config(
            "tenant is production — re-run with --yes to confirm the write".into(),
        )),
        other => other,
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Resolve a secret value from (in priority order) `--value`, `--value-file`,
/// `--value-stdin`, or an interactive no-echo prompt. Keeping the secret out of
/// argv is the default; `--value` stays for scripting but is discouraged.
fn resolve_secret_value(
    value: Option<String>,
    value_file: Option<std::path::PathBuf>,
    value_stdin: bool,
    prompt: &str,
) -> Result<String> {
    let sources = value.is_some() as u8 + value_file.is_some() as u8 + value_stdin as u8;
    if sources > 1 {
        return Err(Error::Config(
            "provide only one of --value / --value-file / --value-stdin".into(),
        ));
    }
    // Strip a single trailing newline so `echo`/editor-added newlines don't end
    // up in the secret.
    let strip = |mut s: String| {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    };
    let v = if let Some(v) = value {
        v
    } else if let Some(path) = value_file {
        strip(std::fs::read_to_string(&path).map_err(|e| {
            Error::Config(format!("read --value-file {}: {e}", path.display()))
        })?)
    } else if value_stdin {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Error::Config(format!("read stdin: {e}")))?;
        strip(s)
    } else {
        rpassword::prompt_password(prompt)
            .map_err(|e| Error::Config(format!("read value: {e}")))?
    };
    if v.is_empty() {
        return Err(Error::Config("value cannot be empty".into()));
    }
    Ok(v)
}

/// Confirm an irreversible write regardless of tenant theme. `--yes` (which
/// also greenlights prod writes) skips the prompt; otherwise we require a typed
/// `yes` on stdin so a destroy/delete can't run on an accidental keystroke.
fn confirm_irreversible(action: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    use std::io::Write;
    eprint!("{action} This cannot be undone. Type 'yes' to confirm: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Config(format!("read confirmation: {e}")))?;
    Ok(line.trim() == "yes")
}

/// Render a JSON scalar without quotes (the API returns a secret version as
/// either a string or a number depending on context).
fn json_scalar(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        v.to_string()
    }
}

/// Resolve a tenant name from a CLI flag, falling back to the on-disk
/// current context, then the config's default tenant. Errors if none is set.
fn resolve_tenant(arg: Option<String>, cfg: &ProjectConfig) -> Result<String> {
    if let Some(t) = arg {
        return Ok(t);
    }
    // A `workspace/<tenant>/<realm>` working directory implies the tenant —
    // but only honour it if it's a tenant we actually know, so a stale
    // directory can't silently retarget writes.
    if let Some(t) = config::workspace_context().tenant {
        if cfg.tenants.iter().any(|x| x.name == t) {
            return Ok(t);
        }
    }
    if let Some(c) = config::read_current_context()? {
        return Ok(c);
    }
    if !cfg.default_tenant.is_empty() {
        return Ok(cfg.default_tenant.clone());
    }
    Err(Error::Config(
        "no current context — run `aic ctx use <tenant>` first".into(),
    ))
}

fn redact(token: &str) -> String {
    let n = token.len();
    if n <= 16 {
        return "*".repeat(n);
    }
    format!("{}…{}  ({} chars)", &token[..8], &token[n - 4..], n)
}
