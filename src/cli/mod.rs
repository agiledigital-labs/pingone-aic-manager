//! `aic` CLI subcommands.
//!
//! Two layers:
//!   * agent lifecycle + auth (`agent`, `login`, `logout`, `stop`, `status`,
//!     `ctx`, `whoami`) — talks directly to the daemon over its socket.
//!   * resource commands (`esv list/get/set/delete/apply`, `esv secret …`,
//!     future `script`, `oauth2`, ...) — go through feature API modules,
//!     which are shared with the TUI. Mutations take `--yes` to confirm a
//!     write to a production-themed tenant (mirrors the TUI's prod guard). Do
//!     NOT add tenant-scoped HTTP via reqwest in here; everything tenant-
//!     facing belongs in `aic/` so both surfaces stay in sync.
//!
//! **Onboarding (creating a new tenant + service account) is TUI-only.**
//! The required flows mix browser cookies, interactive TOTP, and RSA
//! keygen; we haven't tried to script them. Run the TUI once per tenant.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::agent::{self, AgentClient, Request, Response};
use crate::config::crypto::Dek;
use crate::config::wraps::WrapsFile;
use crate::config::{self, ProjectConfig};
use crate::vault::auth;
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
        #[arg(long, help = "Tenant to target")]
        tenant: Option<String>,
        #[arg(long, help = "Print only the full bearer token for scripting")]
        token: bool,
    },
    /// ESV operations (variables, secrets).
    Esv {
        #[command(subcommand)]
        command: crate::esv::cli::EsvCommand,
    },
    /// IDM managed-object schema inspection (hooks sync via `aic script`).
    Managed {
        #[command(subcommand)]
        command: crate::managed::cli::ManagedCommand,
    },
    /// Local IDM managed-object record store and query commands.
    Idm {
        #[command(subcommand)]
        command: crate::idmstore::cli::IdmCommand,
    },
    /// Audit/debug log fetch and API-key management.
    Logs {
        #[command(subcommand)]
        command: crate::logs::cli::LogsCommand,
    },
    /// Journey (authentication tree) inspection and export.
    Journey {
        #[command(subcommand)]
        command: crate::journey::cli::JourneyCommand,
    },
    /// OAuth2 client inspection and export.
    Oauth {
        #[command(subcommand)]
        command: crate::oauth::cli::OauthCommand,
    },
    /// Secret mappings from AM labels to ESV secrets.
    Secretmap {
        #[command(subcommand)]
        command: crate::secretmap::cli::SecretmapCommand,
    },
    /// Script workspace sync (AM scripts + IDM endpoints).
    Script {
        #[command(subcommand)]
        command: crate::scripts::cli::ScriptCommand,
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
        Some(Command::Agent {
            detach,
            idle_timeout,
        }) => run_agent(detach, idle_timeout).await,
        Some(Command::Login) => login().await,
        Some(Command::Logout) => logout().await,
        Some(Command::Stop) => stop().await,
        Some(Command::Status) => status().await,
        Some(Command::Ctx { command }) => ctx(command).await,
        Some(Command::Whoami { tenant, token }) => whoami(tenant, token).await,
        Some(Command::Esv { command }) => crate::esv::cli::run(command).await,
        Some(Command::Managed { command }) => crate::managed::cli::run(command).await,
        Some(Command::Idm { command }) => crate::idmstore::cli::run(command).await,
        Some(Command::Logs { command }) => crate::logs::cli::run(command).await,
        Some(Command::Journey { command }) => crate::journey::cli::run(command).await,
        Some(Command::Oauth { command }) => crate::oauth::cli::run(command).await,
        Some(Command::Secretmap { command }) => crate::secretmap::cli::run(command).await,
        Some(Command::Script { command }) => crate::scripts::cli::run(command).await,
        None => unreachable!("dispatch handled at top level"),
    }
}

/// Locate the project root (the dir containing `.aic-edit/`) by walking up
/// from the current directory, record any tenant/realm implied by a
/// `workspace/<tenant>/<realm>/` working directory, then chdir to the root so
/// every project-relative path (config, keystore, agent socket) resolves the
/// same no matter which subdirectory the command was invoked from.
pub fn bootstrap_project_root() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if let Some(root) = config::find_project_root(&cwd) {
        config::set_workspace_context(config::detect_workspace_context(&root, &cwd));
        let _ = std::env::set_current_dir(&root);
    }
}

/// Parse argv, but first bake the resolved tenant in as the `default_value` of
/// every `--tenant` flag — so `-h` shows the concrete default that will be used
/// and an omitted flag adopts it. Call [`bootstrap_project_root`] first so the
/// workspace context + cwd are set. (Realm/kind are no longer flags — scripts
/// are addressed by `<namespace>/<name>`.)
pub fn parse_with_defaults() -> Cli {
    let cmd = inject_tenant_default(Cli::command(), resolved_tenant().as_deref());
    match Cli::from_arg_matches(&cmd.get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    }
}

/// The tenant to surface as the `--tenant` default: a workspace-dir tenant
/// (only if configured), else the current `aic ctx`, else `default_tenant`,
/// else `None`. Mirrors [`resolve_tenant`].
fn resolved_tenant() -> Option<String> {
    let ctx = config::workspace_context();
    let cfg = ProjectConfig::load().ok().flatten();
    if let (Some(t), Some(cfg)) = (&ctx.tenant, &cfg) {
        if cfg.tenants.iter().any(|x| &x.name == t) {
            return Some(t.clone());
        }
    }
    if let Ok(Some(c)) = config::read_current_context() {
        return Some(c);
    }
    cfg.as_ref()
        .filter(|c| !c.default_tenant.is_empty())
        .map(|c| c.default_tenant.clone())
}

/// Recursively set `default_value` on every `--tenant` arg in the command tree.
fn inject_tenant_default(mut cmd: clap::Command, tenant: Option<&str>) -> clap::Command {
    // clap stores `default_value` as a `'static` borrow, so leak this
    // program-lifetime string (resolved once at startup) to satisfy it.
    if let Some(t) = tenant {
        if cmd.get_arguments().any(|a| a.get_id() == "tenant") {
            let v: &'static str = Box::leak(t.to_string().into_boxed_str());
            cmd = cmd.mut_arg("tenant", |a| a.default_value(v));
        }
    }
    let subs: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for name in subs {
        cmd = cmd.mut_subcommand(&name, |s| inject_tenant_default(s, tenant));
    }
    cmd
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
    let rt =
        tokio::runtime::Runtime::new().map_err(|e| Error::Config(format!("tokio runtime: {e}")))?;
    rt.block_on(async {
        let _ = AgentClient::connect_or_spawn().await?;
        eprintln!("agent running at {}", agent::socket_path().display());
        Ok(())
    })
}

async fn login() -> Result<()> {
    ensure_agent_unlocked().await?;
    print_status_block().await
}

pub(crate) async fn ensure_agent_unlocked() -> Result<()> {
    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::Status).await? {
        Response::Status(info) if info.unlocked => return Ok(()),
        Response::Status(_) => {}
        Response::Error { message } => return Err(Error::Config(message)),
        other => return Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }

    if ProjectConfig::load()?.is_none() {
        return Err(Error::Config(
            "no .aic-edit/config.toml here — onboard a tenant in the TUI first".into(),
        ));
    }

    // Plain mode (user opted out of encryption): no DEK to derive, no
    // prompt. Just tell the agent to load keys.plain and exit.
    let plain_mode = matches!(
        config::Settings::load()?,
        Some(config::Settings {
            encrypt_keys: false,
            ..
        })
    );
    if plain_mode {
        if ProjectConfig::load_keys_plain()?.is_none() {
            return Err(Error::Config(
                "no .aic-edit/keys.plain — onboard a tenant in the TUI first".into(),
            ));
        }
        auth::unlock_plain_agent().await?;
        println!("unlocked (plain mode)");
        return Ok(());
    }

    if ProjectConfig::load_keys_enc()?.is_none() {
        return Err(Error::Config(
            "no .aic-edit/keys.enc — set up an auth factor in the TUI first".into(),
        ));
    }
    let wraps_file = WrapsFile::load()?.ok_or_else(|| {
        Error::Config("no .aic-edit/wraps.toml — set up an auth factor in the TUI first".into())
    })?;

    let dek = match pick_method(&wraps_file)? {
        MethodChoice::Password => unlock_with_password_prompt().await?,
        MethodChoice::SecurityKey => unlock_with_security_key_prompt(&wraps_file).await?,
    };

    auth::put_dek_to_agent(&dek).await?;
    drop(dek);
    println!("unlocked");
    Ok(())
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
    eprintln!("{}", crate::vault::security_key::TAP_MESSAGE);
    Ok(auth::unlock_security_key(wraps_file.clone(), pin)
        .await?
        .dek)
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
                println!("{marker}{}  ({}, {})", t.name, t.theme.label(), t.base_url);
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

async fn whoami(tenant_arg: Option<String>, token_only: bool) -> Result<()> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    let tenant = resolve_tenant(tenant_arg, &cfg)?;

    let client = AgentClient::connect_or_spawn().await?;
    match client
        .send(&Request::GetToken {
            tenant: tenant.clone(),
        })
        .await?
    {
        Response::Token {
            access_token,
            expires_at,
        } => {
            let ttl = expires_at - chrono::Utc::now().timestamp();
            if token_only {
                println!("{access_token}");
            } else {
                println!("tenant:  {tenant}");
                println!("expires: in {ttl}s (unix {expires_at})");
                println!("token:   {}", redact(&access_token));
            }
            Ok(())
        }
        Response::Locked => Err(Error::Auth("agent locked; run `aic login`".into())),
        Response::Error { message } => Err(Error::Auth(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

/// Resolve the tenant for a resource command (flag → current context →
/// default), loading the project config.
pub(crate) fn tenant_for(tenant_arg: Option<String>) -> Result<String> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    resolve_tenant(tenant_arg, &cfg)
}

/// Resolve the tenant for a resource command and return the configured record.
pub(crate) fn tenant_config_for(tenant_arg: Option<String>) -> Result<crate::config::Tenant> {
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml here".into()))?;
    let name = resolve_tenant(tenant_arg, &cfg)?;
    cfg.tenants
        .into_iter()
        .find(|tenant| tenant.name == name)
        .ok_or_else(|| Error::Config(format!("no tenant named '{name}' in config")))
}

/// Turn the agent's prod-confirm refusal into an actionable CLI message.
pub(crate) fn prod_hint<T>(r: Result<T>) -> Result<T> {
    match r {
        Err(Error::ProdConfirmRequired) => Err(Error::Config(
            "tenant is production — re-run with --yes to confirm the write".into(),
        )),
        other => other,
    }
}

pub(crate) fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
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
