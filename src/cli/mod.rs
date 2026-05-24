//! `aic` CLI subcommands. Plumbing only — agent lifecycle, context
//! selection, and a `whoami` that proves a token can be minted via the agent.
//!
//! Resource commands (`get esv`, `apply`, etc.) will land alongside their
//! feature implementations.

use clap::{Parser, Subcommand};

use crate::agent::{self, AgentClient, Request, Response};
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
}

#[derive(Subcommand, Debug)]
pub enum EsvCommand {
    /// List ESV variables. Outputs the `result` array as JSON.
    List {
        /// Override the current context for this call.
        #[arg(long)]
        tenant: Option<String>,
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
        None => unreachable!("dispatch handled at top level"),
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
    if ProjectConfig::load_keys_enc()?.is_none() {
        return Err(Error::Config(
            "no .aic-edit/keys.enc — set a master password in the TUI first".into(),
        ));
    }

    let password = rpassword::prompt_password("Master password: ")
        .map_err(|e| Error::Config(format!("read password: {e}")))?;

    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::Unlock { password }).await? {
        Response::Ok => {
            println!("unlocked");
            print_status_block().await
        }
        Response::Error { message } => Err(Error::Auth(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
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
    match cmd {
        EsvCommand::List { tenant } => esv_list(tenant).await,
    }
}

async fn esv_list(tenant_arg: Option<String>) -> Result<()> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    let tenant_name = resolve_tenant(tenant_arg, &cfg)?;

    let body = api_get(&tenant_name, "/environment/variables").await?;
    let result = body
        .get("result")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Ask the daemon to GET an AIC path on our behalf. The daemon reuses its
/// tenant HTTP connection (TLS handshake amortised across CLI invocations)
/// and caches the body keyed by `(tenant, path)` so subsequent calls can
/// short-circuit on a 304 from AIC.
async fn api_get(tenant: &str, path: &str) -> Result<serde_json::Value> {
    let agent = AgentClient::connect_or_spawn().await?;
    match agent
        .send(&Request::ApiGet {
            tenant: tenant.to_string(),
            path: path.to_string(),
        })
        .await?
    {
        Response::Json { value } => Ok(value),
        Response::Locked => Err(Error::Auth("agent locked; run `aic login`".into())),
        Response::Error { message } => Err(Error::Api { status: 0, body: message }),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

/// Resolve a tenant name from a CLI flag, falling back to the on-disk
/// current context, then the config's default tenant. Errors if none is set.
fn resolve_tenant(arg: Option<String>, cfg: &ProjectConfig) -> Result<String> {
    if let Some(t) = arg {
        return Ok(t);
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
