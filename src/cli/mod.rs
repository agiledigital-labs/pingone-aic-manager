//! `aic-edit` CLI subcommands. Plumbing only — agent lifecycle, context
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
    name = "aic-edit",
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
    /// Run the background agent (held in memory, hands out bearer tokens).
    Agent {
        /// Stay attached to the current terminal; log to stderr.
        #[arg(long)]
        foreground: bool,
        /// Idle-lock timeout in seconds (default 3600).
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
        Some(Command::Agent { foreground, idle_timeout }) => {
            run_agent(foreground, idle_timeout).await
        }
        Some(Command::Login) => login().await,
        Some(Command::Logout) => logout().await,
        Some(Command::Stop) => stop().await,
        Some(Command::Status) => status().await,
        Some(Command::Ctx { command }) => ctx(command).await,
        Some(Command::Whoami { tenant }) => whoami(tenant).await,
        None => unreachable!("dispatch handled at top level"),
    }
}

async fn run_agent(foreground: bool, idle_timeout: Option<u64>) -> Result<()> {
    if !foreground {
        // Re-exec ourselves with --foreground after detaching. Keeps the
        // detach logic in one place (in agent::client::spawn_detached_agent),
        // and lets `aic-edit agent` (no flag) work as a one-liner from a
        // shell.
        return spawn_detached_then_exit();
    }
    let opts = agent::daemon::DaemonOptions {
        idle_timeout_secs: idle_timeout.unwrap_or(3600),
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
    let tenant = match tenant_arg {
        Some(t) => t,
        None => config::read_current_context()?
            .or_else(|| {
                if cfg.default_tenant.is_empty() {
                    None
                } else {
                    Some(cfg.default_tenant.clone())
                }
            })
            .ok_or_else(|| {
                Error::Config(
                    "no current context — run `aic-edit ctx use <tenant>` first".into(),
                )
            })?,
    };

    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::GetToken { tenant: tenant.clone() }).await? {
        Response::Token { access_token, expires_at } => {
            let ttl = expires_at - chrono::Utc::now().timestamp();
            println!("tenant:  {tenant}");
            println!("expires: in {ttl}s (unix {expires_at})");
            println!("token:   {}", redact(&access_token));
            Ok(())
        }
        Response::Error { message } => Err(Error::Auth(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

fn redact(token: &str) -> String {
    let n = token.len();
    if n <= 16 {
        return "*".repeat(n);
    }
    format!("{}…{}  ({} chars)", &token[..8], &token[n - 4..], n)
}
