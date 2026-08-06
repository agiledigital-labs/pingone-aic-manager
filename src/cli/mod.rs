//! `aic` CLI subcommands.
//!
//! Two layers:
//!   * agent lifecycle + auth (`agent`, `session`, `ctx`, `whoami`) — talks
//!     directly to the daemon over its socket.
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

use std::io::{BufRead, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use serde::Serialize;

use crate::agent::duration::{format_duration, parse_duration};
use crate::agent::{self, AgentClient, Request, Response};
use crate::config::crypto::Dek;
use crate::config::operator::{self, NameSource, NetworkAccess, ResolvedOperator};
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
    /// Disable all interactive prompts and fail when input is required.
    #[arg(long, global = true)]
    pub no_prompt: bool,

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
        /// gets redirected to .aic/agent.log; setsid() puts the child in
        /// its own session so a terminal HUP doesn't kill it.
        #[arg(long)]
        detach: bool,
        /// Idle-lock timeout in seconds (default 3600, or whatever
        /// settings.toml specifies).
        #[arg(long)]
        idle_timeout: Option<u64>,
    },
    /// Unlock the keystore in the agent. Prompts for the master password.
    #[command(hide = true)]
    Login {
        /// Idle-lock timeout (for example, 1h20m).
        #[arg(long)]
        timeout: Option<String>,
        /// Read one password line from stdin instead of prompting on the TTY.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Clear the agent's in-memory JWKs + tokens (but leave the agent running).
    #[command(hide = true)]
    Logout,
    /// Stop the agent process.
    #[command(hide = true)]
    Stop,
    /// Show agent state, current context, and cached-token expirations.
    #[command(hide = true)]
    Status,
    /// Manage the unlocked CLI session and agent lifecycle.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Manage the active tenant context for this project.
    Ctx {
        #[command(subcommand)]
        command: CtxCommand,
    },
    /// Inspect and change local project settings.
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
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
    /// IDM sync-queue diagnostics and reconciliation control.
    Sync {
        #[command(subcommand)]
        command: crate::mappings::cli::SyncCommand,
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
    /// Scaffold / refresh the local script workspace tree.
    Workspace {
        #[command(subcommand)]
        command: crate::scripts::cli::WorkspaceCommand,
    },
    /// Script workspace sync (AM scripts + IDM endpoints).
    Script {
        #[command(subcommand)]
        command: crate::scripts::cli::ScriptCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum CtxCommand {
    /// List tenants defined in `.aic/config.toml`.
    List {
        #[arg(long, help = "Print tenants as JSON")]
        json: bool,
    },
    /// Print the current context.
    Current,
    /// Switch to a different tenant.
    Use { tenant: String },
}

#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    /// Unlock the keystore in the agent. Prompts for the master password.
    Login {
        /// Idle-lock timeout (for example, 1h20m).
        #[arg(long)]
        timeout: Option<String>,
        /// Read one password line from stdin instead of prompting on the TTY.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Clear the agent's in-memory JWKs + tokens (but leave the agent running).
    Logout,
    /// Stop the agent process.
    Stop,
    /// Show agent state, current context, and cached-token expirations.
    Status,
}

#[derive(Subcommand, Debug)]
pub enum SettingsCommand {
    /// List every user-configurable setting and whether its value is derived.
    List,
    /// Print one effective setting value.
    Get { key: String },
    /// Persist one setting value.
    Set { key: String, value: String },
}

static NO_PROMPT: AtomicBool = AtomicBool::new(false);

pub(crate) fn prompting_disabled() -> bool {
    NO_PROMPT.load(Ordering::Relaxed)
}

impl Command {
    fn needs_tenant_auth(&self) -> bool {
        match self {
            Self::Whoami { .. }
            | Self::Esv { .. }
            | Self::Managed { .. }
            | Self::Idm { .. }
            | Self::Sync { .. }
            | Self::Logs { .. }
            | Self::Journey { .. }
            | Self::Oauth { .. }
            | Self::Secretmap { .. }
            | Self::Workspace { .. }
            | Self::Script { .. } => true,
            Self::Agent { .. }
            | Self::Login { .. }
            | Self::Logout
            | Self::Stop
            | Self::Status
            | Self::Session { .. }
            | Self::Ctx { .. }
            | Self::Settings { .. } => false,
        }
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    NO_PROMPT.store(cli.no_prompt, Ordering::Relaxed);
    if cli.command.as_ref().is_some_and(Command::needs_tenant_auth) {
        ensure_agent_unlocked(false).await?;
        prepare_operator().await?;
    }
    match cli.command {
        Some(Command::Agent {
            detach,
            idle_timeout,
        }) => run_agent(detach, idle_timeout).await,
        Some(Command::Login {
            timeout,
            password_stdin,
        }) => login(timeout, password_stdin).await,
        Some(Command::Logout) => logout().await,
        Some(Command::Stop) => stop().await,
        Some(Command::Status) => status().await,
        Some(Command::Session { command }) => session(command).await,
        Some(Command::Ctx { command }) => ctx(command).await,
        Some(Command::Settings { command }) => settings(command).await,
        Some(Command::Whoami { tenant, token }) => whoami(tenant, token).await,
        Some(Command::Esv { command }) => crate::esv::cli::run(command).await,
        Some(Command::Managed { command }) => crate::managed::cli::run(command).await,
        Some(Command::Idm { command }) => crate::idmstore::cli::run(command).await,
        Some(Command::Sync { command }) => crate::mappings::cli::run(command).await,
        Some(Command::Logs { command }) => crate::logs::cli::run(command).await,
        Some(Command::Journey { command }) => crate::journey::cli::run(command).await,
        Some(Command::Oauth { command }) => crate::oauth::cli::run(command).await,
        Some(Command::Secretmap { command }) => crate::secretmap::cli::run(command).await,
        Some(Command::Workspace { command }) => crate::scripts::cli::run_workspace(command).await,
        Some(Command::Script { command }) => crate::scripts::cli::run(command).await,
        None => unreachable!("dispatch handled at top level"),
    }
}

async fn prepare_operator() -> Result<()> {
    let settings = config::Settings::load()?.unwrap_or_default();
    // Either the name is already established, or there is no terminal to
    // establish it on. Bail before doing any work: commands that need a name
    // resolve their own fallback, and this pre-flight runs ahead of *every*
    // tenant-auth command, so anything it does non-interactively is a cost an
    // AI agent pays on each invocation and never benefits from.
    let can_prompt = prompt_available();
    if operator_action(settings.operator.name.is_some(), can_prompt, None) == OperatorAction::Skip {
        return Ok(());
    }

    // Committed to prompting, so the service-account guess is worth fetching:
    // it is what pre-fills the default the user can accept with Enter.
    let project = ProjectConfig::load().ok().flatten();
    let tenant = project.as_ref().and_then(|project| {
        let name = resolve_tenant(None, project).ok()?;
        project.tenants.iter().find(|tenant| tenant.name == name)
    });
    let resolved = operator::resolve(&settings, tenant, NetworkAccess::Allow).await;

    let guess = (resolved.source == NameSource::ServiceAccount).then_some(resolved.name.as_str());
    let action = operator_action(false, can_prompt, guess);
    let answer = match timed_blocking_prompt(move || prompt_for_operator_name(action)).await {
        Ok(Some(answer)) => answer,
        Ok(None) => return Ok(()),
        Err(error) => {
            tracing::warn!(%error, "operator prompt failed; using fallback for this run");
            return Ok(());
        }
    };
    if let Err(error) = operator::set_name(answer) {
        tracing::warn!(%error, "operator setting save failed; using fallback for this run");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorAction {
    Skip,
    PromptWithDefault(String),
    PromptRequired,
}

fn operator_action(name_set: bool, can_prompt: bool, guess: Option<&str>) -> OperatorAction {
    if name_set || !can_prompt {
        return OperatorAction::Skip;
    }
    match guess.map(str::trim).filter(|guess| !guess.is_empty()) {
        Some(guess) => OperatorAction::PromptWithDefault(guess.to_string()),
        None => OperatorAction::PromptRequired,
    }
}

fn prompt_for_operator_name(action: OperatorAction) -> std::io::Result<Option<String>> {
    use std::io::Write;

    let (label, default) = match action {
        OperatorAction::Skip => return Ok(None),
        OperatorAction::PromptWithDefault(default) => {
            (format!("Operator name [{default}]: "), Some(default))
        }
        OperatorAction::PromptRequired => ("Operator name: ".to_string(), None),
    };

    // Read from the terminal device, not stdin. `timed_blocking_prompt`'s
    // timeout abandons the task but cannot cancel a thread parked in
    // `read_line`, so a `StdinLock` taken here would be held for the life of
    // the process — silently hanging every later stdin reader
    // (`--password-stdin`, the `inquire` pickers behind a bare `script pull`).
    // `prompt_available` has already proved /dev/tty opens.
    let mut tty = std::io::BufReader::new(std::fs::File::open("/dev/tty")?);
    loop {
        eprint!("{label}");
        std::io::stderr().flush()?;
        let mut input = String::new();
        if tty.read_line(&mut input)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        let answer = input.trim();
        if !answer.is_empty() {
            return Ok(Some(answer.to_string()));
        }
        // Empty input accepts an offered default; with nothing to fall back on
        // there is no answer yet, so ask again.
        match &default {
            Some(default) => return Ok(Some(default.clone())),
            None => eprintln!("Operator name cannot be empty."),
        }
    }
}

async fn session(command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Login {
            timeout,
            password_stdin,
        } => login(timeout, password_stdin).await,
        SessionCommand::Logout => logout().await,
        SessionCommand::Stop => stop().await,
        SessionCommand::Status => status().await,
    }
}

const DEFAULT_AGENT_IDLE_TIMEOUT_SECS: u64 = 3600;

async fn settings(command: SettingsCommand) -> Result<()> {
    let mut settings = config::Settings::load()?.unwrap_or_default();
    match command {
        SettingsCommand::List => {
            let resolved = operator::resolve(&settings, None, NetworkAccess::Skip).await;
            print_table(
                &["KEY", "VALUE", "DEFAULTED"],
                &[
                    vec![
                        "operator.name".into(),
                        resolved.name,
                        yes_no(settings.operator.name.is_none()).into(),
                    ],
                    vec![
                        "operator.host".into(),
                        resolved.host,
                        yes_no(settings.operator.host.is_none()).into(),
                    ],
                    vec![
                        "agent-idle-timeout-secs".into(),
                        settings
                            .agent_idle_timeout_secs
                            .unwrap_or(DEFAULT_AGENT_IDLE_TIMEOUT_SECS)
                            .to_string(),
                        yes_no(settings.agent_idle_timeout_secs.is_none()).into(),
                    ],
                ],
            );
            Ok(())
        }
        SettingsCommand::Get { key } => {
            let resolved = operator::resolve(&settings, None, NetworkAccess::Skip).await;
            println!("{}", setting_value(&key, &settings, &resolved)?);
            Ok(())
        }
        SettingsCommand::Set { key, value } => {
            apply_setting(&mut settings, &key, &value)?;
            settings.save()?;
            let resolved = operator::resolve(&settings, None, NetworkAccess::Skip).await;
            println!("{key} = {}", setting_value(&key, &settings, &resolved)?);
            Ok(())
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn setting_value(
    key: &str,
    settings: &config::Settings,
    resolved: &ResolvedOperator,
) -> Result<String> {
    match key {
        "operator.name" => Ok(resolved.name.clone()),
        "operator.host" => Ok(resolved.host.clone()),
        "agent-idle-timeout-secs" => Ok(settings
            .agent_idle_timeout_secs
            .unwrap_or(DEFAULT_AGENT_IDLE_TIMEOUT_SECS)
            .to_string()),
        _ => Err(unknown_setting(key)),
    }
}

fn apply_setting(settings: &mut config::Settings, key: &str, value: &str) -> Result<()> {
    match key {
        "operator.name" => {
            settings.operator.name = Some(config::Operator::validated_name(value)?);
            Ok(())
        }
        "operator.host" => {
            settings.operator.host = Some(config::Operator::validated_host(value)?);
            Ok(())
        }
        "agent-idle-timeout-secs" => {
            settings.agent_idle_timeout_secs = Some(value.parse::<u64>().map_err(|error| {
                Error::Config(format!(
                    "agent-idle-timeout-secs must be an unsigned integer: {error}"
                ))
            })?);
            Ok(())
        }
        "encrypt_keys" | "encrypt-keys" => Err(Error::Config(
            "encrypt_keys cannot be changed with `aic settings set`; use the TUI Auth Settings \
             screen so the vault files are migrated safely"
                .into(),
        )),
        "version" => Err(Error::Config(
            "version is managed by aic and cannot be set".into(),
        )),
        _ => Err(unknown_setting(key)),
    }
}

fn unknown_setting(key: &str) -> Error {
    Error::Config(format!(
        "unknown setting '{key}'; expected operator.name, operator.host, or \
         agent-idle-timeout-secs"
    ))
}

/// Locate the project root (the dir containing `.aic/`) by walking up
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
        Ok(mut cli) => {
            apply_no_prompt_env(&mut cli, std::env::var_os("AIC_NO_PROMPT").as_deref());
            cli
        }
        Err(e) => e.exit(),
    }
}

fn no_prompt_from_env(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "1")
}

fn apply_no_prompt_env(cli: &mut Cli, value: Option<&std::ffi::OsStr>) {
    cli.no_prompt |= no_prompt_from_env(value);
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

async fn login(timeout: Option<String>, password_stdin: bool) -> Result<()> {
    ensure_agent_unlocked(password_stdin).await?;
    if let Some(timeout) = timeout {
        let secs = parse_duration(&timeout).map_err(Error::Config)?;
        let client = AgentClient::connect(agent::socket_path()).await?;
        match client.send(&Request::SetIdleTimeout { secs }).await? {
            Response::Ok => {}
            Response::Error { message } => return Err(Error::Config(message)),
            other => return Err(Error::Config(format!("unexpected reply: {other:?}"))),
        }
    }
    print_status_block().await
}

pub(crate) async fn ensure_agent_unlocked(password_stdin: bool) -> Result<()> {
    let client = AgentClient::connect_or_spawn().await?;
    match client.send(&Request::Status).await? {
        Response::Status(info) if info.unlocked => return Ok(()),
        Response::Status(_) => {}
        Response::Error { message } => return Err(Error::Config(message)),
        other => return Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }

    if ProjectConfig::load()?.is_none() {
        return Err(Error::Config(
            "no .aic/config.toml here — onboard a tenant in the TUI first".into(),
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
                "no .aic/keys.plain — onboard a tenant in the TUI first".into(),
            ));
        }
        auth::unlock_plain_agent().await?;
        println!("unlocked (plain mode)");
        return Ok(());
    }

    if ProjectConfig::load_keys_enc()?.is_none() {
        return Err(Error::Config(
            "no .aic/keys.enc — set up an auth factor in the TUI first".into(),
        ));
    }
    let wraps_file = WrapsFile::load()?.ok_or_else(|| {
        Error::Config("no .aic/wraps.toml — set up an auth factor in the TUI first".into())
    })?;

    let method = if password_stdin {
        if !wraps_file.has_password() {
            return Err(Error::Config("no password factor enrolled".into()));
        }
        MethodChoice::Password
    } else {
        ensure_prompt_available()?;
        pick_method(&wraps_file).await?
    };
    let dek = match method {
        MethodChoice::Password if password_stdin => unlock_with_password_stdin().await?,
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
async fn pick_method(wraps_file: &WrapsFile) -> Result<MethodChoice> {
    let has_password = wraps_file.has_password();
    let has_security_key = wraps_file.has_security_key();

    match (has_password, has_security_key) {
        (false, false) => Err(Error::Config(
            "no auth factors enrolled in wraps.toml".into(),
        )),
        (true, false) => Ok(MethodChoice::Password),
        (false, true) => Ok(MethodChoice::SecurityKey),
        (true, true) => {
            let line = timed_blocking_prompt(|| {
                eprintln!("Authentication methods:");
                eprintln!("  1) Master password");
                eprintln!("  2) Security key");
                eprint!("Choose [1]: ");
                use std::io::Write;
                std::io::stderr().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                Ok(line)
            })
            .await?;
            match line.trim() {
                "" | "1" => Ok(MethodChoice::Password),
                "2" => Ok(MethodChoice::SecurityKey),
                other => Err(Error::Config(format!("invalid choice: {other}"))),
            }
        }
    }
}

async fn unlock_with_password_prompt() -> Result<Dek> {
    let password =
        timed_blocking_prompt(|| rpassword::prompt_password("Master password: ")).await?;
    Ok(auth::unlock_password(password).await?.dek)
}

async fn unlock_with_security_key_prompt(wraps_file: &WrapsFile) -> Result<Dek> {
    let pin = timed_blocking_prompt(|| rpassword::prompt_password("Security key PIN: ")).await?;
    eprintln!("{}", crate::vault::security_key::TAP_MESSAGE);
    Ok(auth::unlock_security_key(wraps_file.clone(), pin)
        .await?
        .dek)
}

async fn unlock_with_password_stdin() -> Result<Dek> {
    let password = read_password_line(std::io::stdin().lock())?;
    Ok(auth::unlock_password(password).await?.dek)
}

fn read_password_line(mut reader: impl BufRead) -> Result<String> {
    let mut password = String::new();
    reader
        .read_line(&mut password)
        .map_err(|error| Error::Config(format!("read password from stdin: {error}")))?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.is_empty() {
        return Err(Error::Config("password from stdin cannot be empty".into()));
    }
    Ok(password)
}

fn should_prompt(no_prompt: bool, stdin_tty: bool, stderr_tty: bool, tty_openable: bool) -> bool {
    !no_prompt && stdin_tty && stderr_tty && tty_openable
}

fn prompt_available() -> bool {
    should_prompt(
        prompting_disabled(),
        std::io::stdin().is_terminal(),
        std::io::stderr().is_terminal(),
        std::fs::File::open("/dev/tty").is_ok(),
    )
}

fn ensure_prompt_available() -> Result<()> {
    if prompt_available() {
        Ok(())
    } else {
        Err(Error::AuthRequired)
    }
}

async fn timed_blocking_prompt<T, F>(prompt: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    let task = tokio::task::spawn_blocking(prompt);
    // The blocking read cannot be cancelled. On timeout the CLI returns an
    // error and exits, so leaving that worker blocked cannot stall the process.
    match tokio::time::timeout(Duration::from_secs(60), task).await {
        Ok(joined) => joined
            .map_err(|error| Error::Config(format!("prompt task: {error}")))?
            .map_err(|error| Error::Config(format!("read prompt: {error}"))),
        Err(_) => Err(Error::AuthRequired),
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
    let client = match AgentClient::connect(&sock).await {
        Ok(c) => c,
        Err(_) => {
            // Socket left behind by a crashed agent — nothing to stop.
            let _ = std::fs::remove_file(&sock);
            println!("no agent running (removed stale socket)");
            return Ok(());
        }
    };
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
    // The socket file can linger after a crash — only a successful connect
    // proves an agent is listening.
    if !sock.exists() || AgentClient::connect(&sock).await.is_err() {
        println!("agent: not running (start it with `aic session login`)");
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
            "idle:     {} remaining of {}",
            format_duration(info.idle_remaining_secs),
            format_duration(info.idle_timeout_secs)
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
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let current = config::read_current_context()?;
    match cmd {
        CtxCommand::List { json } => {
            if json {
                let rows = cfg
                    .tenants
                    .iter()
                    .map(|tenant| CtxTenantOutput {
                        current: Some(&tenant.name) == current.as_ref(),
                        name: tenant.name.clone(),
                        theme: tenant.theme.label().to_string(),
                        base_url: tenant.base_url.clone(),
                    })
                    .collect::<Vec<_>>();
                print_json(&rows)?;
            } else {
                let rows = cfg
                    .tenants
                    .iter()
                    .map(|tenant| {
                        vec![
                            if Some(&tenant.name) == current.as_ref() {
                                "*".to_string()
                            } else {
                                "".to_string()
                            },
                            tenant.name.clone(),
                            tenant.theme.label().to_string(),
                            tenant.base_url.clone(),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(&["CURRENT", "NAME", "THEME", "BASE_URL"], &rows);
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
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let tenant = resolve_tenant(tenant_arg, &cfg)?;
    let settings = config::Settings::load()?.unwrap_or_default();
    let tenant_config = cfg
        .tenants
        .iter()
        .find(|candidate| candidate.name == tenant);
    let operator = operator::resolve(&settings, tenant_config, NetworkAccess::Skip).await;

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
                // The service account is worth printing even though it's local
                // config: it's the `iss`/`sub` of the assertion that minted this
                // token, so it names which of several same-titled accounts in the
                // console this tenant actually authenticates as.
                let sa = cfg
                    .tenants
                    .iter()
                    .find(|candidate| candidate.name == tenant)
                    .and_then(|candidate| candidate.sa_id.as_deref())
                    .unwrap_or("(none — log-only tenant)");
                println!("tenant:  {tenant}");
                println!("sa:      {sa}");
                match operator.source {
                    NameSource::Placeholder => println!(
                        "operator: name not set on {} (run `aic settings set operator.name \
                         <name>`)",
                        operator.host
                    ),
                    NameSource::Settings => {
                        println!("operator: {} on {}", operator.name, operator.host)
                    }
                    NameSource::ServiceAccount => println!(
                        "operator: {} on {} (guessed from service account)",
                        operator.name, operator.host
                    ),
                }
                println!("expires: in {ttl}s (unix {expires_at})");
                println!("token:   {}", redact(&access_token));
            }
            Ok(())
        }
        Response::Locked => Err(Error::Auth("agent locked; run `aic session login`".into())),
        Response::Error { message } => Err(Error::Auth(message)),
        other => Err(Error::Config(format!("unexpected reply: {other:?}"))),
    }
}

/// Resolve the tenant for a resource command (flag → current context →
/// default), loading the project config.
pub(crate) fn tenant_for(tenant_arg: Option<String>) -> Result<String> {
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    resolve_tenant(tenant_arg, &cfg)
}

/// Resolve the tenant for a resource command and return the configured record.
pub(crate) fn tenant_config_for(tenant_arg: Option<String>) -> Result<crate::config::Tenant> {
    let cfg =
        ProjectConfig::load()?.ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
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

pub(crate) fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    println!("{}", render_table(headers, rows));
}

fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths = headers
        .iter()
        .map(|header| header.chars().count())
        .collect::<Vec<_>>();
    for row in rows {
        for (idx, width) in widths.iter_mut().enumerate() {
            let cell_width = row.get(idx).map(|cell| cell.chars().count()).unwrap_or(0);
            *width = (*width).max(cell_width);
        }
    }

    let mut out = String::new();
    out.push_str(&render_table_line(
        &headers
            .iter()
            .map(|header| header.to_string())
            .collect::<Vec<_>>(),
        &widths,
    ));
    for row in rows {
        out.push('\n');
        out.push_str(&render_table_line(row, &widths));
    }
    out
}

fn render_table_line(cells: &[String], widths: &[usize]) -> String {
    let mut out = String::new();
    for idx in 0..widths.len() {
        if idx > 0 {
            out.push_str("  ");
        }
        let cell = cells.get(idx).map(String::as_str).unwrap_or("");
        if idx + 1 == widths.len() {
            out.push_str(cell);
        } else {
            out.push_str(&format!("{cell:<width$}", width = widths[idx]));
        }
    }
    out
}

/// String field of a JSON object as a table cell: newlines collapsed to
/// spaces, missing/non-string rendered as `-`.
pub(crate) fn json_str_cell(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("-")
        .replace('\n', " ")
}

/// Bool field of a JSON object as a table cell; missing/non-bool renders `-`.
pub(crate) fn json_bool_cell(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// Collapse newlines and truncate to `max` display chars with an ellipsis.
pub(crate) fn clip(value: &str, max: usize) -> String {
    let value = value.replace('\n', " ");
    if value.chars().count() <= max {
        return value;
    }
    let mut out = value
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

#[derive(Serialize)]
struct CtxTenantOutput {
    current: bool,
    name: String,
    theme: String,
    base_url: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn columns_pad_to_the_widest_cell_and_the_last_is_not_padded() {
        let out = render_table(
            &["ID", "LOADED"],
            &[row(&["esv-a", "true"]), row(&["esv-longer", "false"])],
        );
        assert_eq!(
            out,
            "ID          LOADED\n\
             esv-a       true\n\
             esv-longer  false"
        );
    }

    #[test]
    fn short_rows_render_missing_trailing_cells_as_blank() {
        let out = render_table(&["A", "B"], &[row(&["only-a"])]);
        assert_eq!(out, "A       B\nonly-a  ");
    }

    #[test]
    fn header_only_when_there_are_no_rows() {
        assert_eq!(render_table(&["ID", "NAME"], &[]), "ID  NAME");
    }

    #[test]
    fn clip_collapses_newlines_and_truncates_with_ellipsis() {
        assert_eq!(clip("one\ntwo", 20), "one two");
        assert_eq!(clip("abcdefghij", 8), "abcde...");
        assert_eq!(clip("abcdefghij", 10), "abcdefghij");
    }

    #[test]
    fn workspace_is_a_root_command() {
        let cli = Cli::try_parse_from(["aic", "workspace", "init"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Workspace {
                command: crate::scripts::cli::WorkspaceCommand::Init { .. }
            })
        ));
    }

    #[test]
    fn script_workspace_is_no_longer_nested() {
        let result = Cli::try_parse_from(["aic", "script", "workspace", "init"]);

        assert!(result.is_err());
    }

    #[test]
    fn session_groups_agent_lifecycle_commands() {
        let cli = Cli::try_parse_from(["aic", "session", "status"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Session {
                command: SessionCommand::Status
            })
        ));
    }

    #[test]
    fn settings_command_forms_parse() {
        let list = Cli::try_parse_from(["aic", "settings", "list"]).unwrap();
        assert!(matches!(
            list.command,
            Some(Command::Settings {
                command: SettingsCommand::List
            })
        ));

        let set = Cli::try_parse_from(["aic", "settings", "set", "operator.name", "Dave"]).unwrap();
        assert!(matches!(
            set.command,
            Some(Command::Settings {
                command: SettingsCommand::Set { key, value }
            }) if key == "operator.name" && value == "Dave"
        ));
    }

    #[test]
    fn settings_set_refuses_the_vault_encryption_flag() {
        let mut settings = config::Settings::default();

        let error = apply_setting(&mut settings, "encrypt_keys", "true").unwrap_err();

        assert!(!settings.encrypt_keys);
        assert!(error.to_string().contains("TUI Auth Settings screen"));
    }

    #[test]
    fn legacy_login_alias_still_parses() {
        let cli = Cli::try_parse_from(["aic", "login"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Login {
                timeout: None,
                password_stdin: false
            })
        ));
    }

    #[test]
    fn login_timeout_parses() {
        let cli = Cli::try_parse_from(["aic", "login", "--timeout", "1h20m"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Login {
                timeout: Some(timeout),
                password_stdin: false
            }) if timeout == "1h20m"
        ));
    }

    #[test]
    fn no_prompt_parses_globally_and_survives_default_injection() {
        let cli = Cli::try_parse_from(["aic", "esv", "list", "--no-prompt"]).unwrap();
        assert!(cli.no_prompt);

        let command = inject_tenant_default(Cli::command(), Some("sandbox"));
        let matches = command
            .try_get_matches_from(["aic", "--no-prompt", "esv", "list"])
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(cli.no_prompt);
    }

    #[test]
    fn no_prompt_reads_the_environment() {
        use std::ffi::OsStr;

        let mut cli = Cli::try_parse_from(["aic", "status"]).unwrap();
        assert!(!cli.no_prompt);

        apply_no_prompt_env(&mut cli, Some(OsStr::new("1")));
        assert!(cli.no_prompt);
    }

    #[test]
    fn prompt_decision_requires_every_precondition() {
        assert!(should_prompt(false, true, true, true));
        assert!(!should_prompt(true, true, true, true));
        assert!(!should_prompt(false, false, true, true));
        assert!(!should_prompt(false, true, false, true));
        assert!(!should_prompt(false, true, true, false));
    }

    #[test]
    fn operator_action_covers_every_prompting_combination() {
        let cases = [
            (false, false, None, OperatorAction::Skip),
            (false, false, Some("guess"), OperatorAction::Skip),
            (false, true, None, OperatorAction::PromptRequired),
            (
                false,
                true,
                Some("guess"),
                OperatorAction::PromptWithDefault("guess".into()),
            ),
            (true, false, None, OperatorAction::Skip),
            (true, false, Some("guess"), OperatorAction::Skip),
            (true, true, None, OperatorAction::Skip),
            (true, true, Some("guess"), OperatorAction::Skip),
        ];

        for (name_set, can_prompt, guess, expected) in cases {
            assert_eq!(operator_action(name_set, can_prompt, guess), expected);
        }
    }

    #[test]
    fn password_stdin_line_parsing_strips_only_the_line_ending() {
        use std::io::Cursor;

        assert_eq!(
            read_password_line(Cursor::new("secret\nrest")).unwrap(),
            "secret"
        );
        assert_eq!(
            read_password_line(Cursor::new("secret\r\n")).unwrap(),
            "secret"
        );
        assert_eq!(read_password_line(Cursor::new("secret")).unwrap(), "secret");
        assert_eq!(
            read_password_line(Cursor::new("secret  \n")).unwrap(),
            "secret  "
        );
        assert!(read_password_line(Cursor::new("\n")).is_err());
        assert!(read_password_line(Cursor::new("\r\n")).is_err());
        assert!(read_password_line(Cursor::new("")).is_err());
    }

    #[test]
    fn password_stdin_parses_on_both_login_forms() {
        let session = Cli::try_parse_from(["aic", "session", "login", "--password-stdin"]).unwrap();
        assert!(matches!(
            session.command,
            Some(Command::Session {
                command: SessionCommand::Login {
                    password_stdin: true,
                    ..
                }
            })
        ));

        let alias = Cli::try_parse_from(["aic", "login", "--password-stdin"]).unwrap();
        assert!(matches!(
            alias.command,
            Some(Command::Login {
                password_stdin: true,
                ..
            })
        ));
    }

    #[test]
    fn every_command_variant_is_classified_for_tenant_auth() {
        let cases = [
            (vec!["aic", "agent"], false),
            (vec!["aic", "login"], false),
            (vec!["aic", "logout"], false),
            (vec!["aic", "stop"], false),
            (vec!["aic", "status"], false),
            (vec!["aic", "session", "status"], false),
            (vec!["aic", "ctx", "current"], false),
            (vec!["aic", "settings", "list"], false),
            (vec!["aic", "whoami"], true),
            (vec!["aic", "esv", "list"], true),
            (vec!["aic", "managed", "list"], true),
            (vec!["aic", "idm", "status"], true),
            (vec!["aic", "sync", "mappings"], true),
            (vec!["aic", "logs", "sources"], true),
            (vec!["aic", "journey", "list"], true),
            (vec!["aic", "oauth", "list"], true),
            (vec!["aic", "secretmap", "list"], true),
            (vec!["aic", "workspace", "init"], true),
            (vec!["aic", "script", "list"], true),
        ];

        for (argv, expected) in cases {
            let command = Cli::try_parse_from(argv).unwrap().command.unwrap();
            assert_eq!(command.needs_tenant_auth(), expected);
            // Keep this match exhaustive so adding a root command forces an
            // explicit test classification as well as a production one.
            match command {
                Command::Agent { .. }
                | Command::Login { .. }
                | Command::Logout
                | Command::Stop
                | Command::Status
                | Command::Session { .. }
                | Command::Ctx { .. }
                | Command::Settings { .. }
                | Command::Whoami { .. }
                | Command::Esv { .. }
                | Command::Managed { .. }
                | Command::Idm { .. }
                | Command::Sync { .. }
                | Command::Logs { .. }
                | Command::Journey { .. }
                | Command::Oauth { .. }
                | Command::Secretmap { .. }
                | Command::Workspace { .. }
                | Command::Script { .. } => {}
            }
        }
    }
}
