//! Agent daemon: listens on a Unix socket, holds decrypted JWKs in memory,
//! mints/refreshes bearer tokens on request.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;

use crate::aic::AicClient;
use crate::config::{self, ProjectConfig};
use crate::{Error, Result};

use super::protocol::{CachedTokenInfo, Request, Response, StatusInfo};
use super::{log_path, pid_path, socket_path};

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 3600;

struct AgentState {
    project_dir: String,
    /// `None` when locked. `Some(map)` when unlocked: tenant_name → AicClient.
    /// Each AicClient holds its own JWK and a token cache.
    clients: Option<HashMap<String, Arc<AicClient>>>,
    last_request: Instant,
    idle_timeout: Duration,
}

impl AgentState {
    fn new(project_dir: String, idle_timeout: Duration) -> Self {
        Self {
            project_dir,
            clients: None,
            last_request: Instant::now(),
            idle_timeout,
        }
    }

    fn touch(&mut self) {
        self.last_request = Instant::now();
    }

    fn lock(&mut self) {
        self.clients = None;
    }
}

pub struct DaemonOptions {
    pub idle_timeout_secs: u64,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            idle_timeout_secs: DEFAULT_IDLE_TIMEOUT_SECS,
        }
    }
}

/// Run the agent until SIGTERM/SIGINT or a `Shutdown` request.
pub async fn run(opts: DaemonOptions) -> Result<()> {
    let project_dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".into());

    let sock = socket_path();
    std::fs::create_dir_all(ProjectConfig::dir())?;

    // Refuse to start if another live agent owns the socket.
    if sock.exists() {
        match try_ping_existing().await {
            Ok(true) => {
                return Err(Error::Config(format!(
                    "another agent is already running (socket {} is responsive)",
                    sock.display()
                )));
            }
            _ => {
                tracing::info!(socket = %sock.display(), "removing stale socket");
                let _ = std::fs::remove_file(&sock);
            }
        }
    }

    let listener = UnixListener::bind(&sock)
        .map_err(|e| Error::Config(format!("bind {}: {e}", sock.display())))?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;

    // Best-effort PID file for observability.
    let _ = std::fs::write(pid_path(), std::process::id().to_string());

    let idle_timeout = Duration::from_secs(opts.idle_timeout_secs);
    let state = Arc::new(RwLock::new(AgentState::new(project_dir.clone(), idle_timeout)));

    tracing::info!(
        socket = %sock.display(),
        project = %project_dir,
        idle_timeout_secs = opts.idle_timeout_secs,
        "agent started"
    );

    // Idle-lock background task: every 30s, if last request older than timeout
    // and we're unlocked, drop the JWKs and tokens.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let mut s = state.write().await;
                if s.clients.is_some() && s.last_request.elapsed() >= s.idle_timeout {
                    tracing::info!("idle timeout reached, locking");
                    s.lock();
                }
            }
        });
    }

    // Shutdown signal: SIGTERM or SIGINT, or a Shutdown request via the socket.
    let shutdown = Arc::new(tokio::sync::Notify::new());
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("received SIGINT, shutting down");
                }
                _ = wait_sigterm() => {
                    tracing::info!("received SIGTERM, shutting down");
                }
            }
            shutdown.notify_waiters();
        });
    }

    let accept_loop = async {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let shutdown = shutdown.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, state, shutdown).await {
                            tracing::warn!(error = %e, "connection handler failed");
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "accept failed");
                    break;
                }
            }
        }
    };

    tokio::select! {
        _ = accept_loop => {}
        _ = shutdown.notified() => {}
    }

    let _ = std::fs::remove_file(&sock);
    let _ = std::fs::remove_file(pid_path());
    tracing::info!("agent exited");
    Ok(())
}

async fn wait_sigterm() {
    use tokio::signal::unix::{signal, SignalKind};
    if let Ok(mut s) = signal(SignalKind::terminate()) {
        s.recv().await;
    } else {
        futures::future::pending::<()>().await;
    }
}

async fn try_ping_existing() -> Result<bool> {
    let client = super::AgentClient::connect(socket_path()).await?;
    match client.send(&Request::Ping).await? {
        Response::Pong { .. } => Ok(true),
        _ => Ok(false),
    }
}

async fn handle_connection(
    stream: UnixStream,
    state: Arc<RwLock<AgentState>>,
    shutdown: Arc<tokio::sync::Notify>,
) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(());
    }
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            send(&mut write, &Response::Error {
                message: format!("bad request: {e}"),
            }).await?;
            return Ok(());
        }
    };

    // Don't bump idle timer on Ping/Status — Status is used to monitor TTL
    // and we don't want it to keep the session alive forever.
    let bump = !matches!(req, Request::Ping | Request::Status);

    let resp = handle(req, state.clone(), shutdown).await;

    if bump {
        state.write().await.touch();
    }

    send(&mut write, &resp).await?;
    Ok(())
}

async fn send<W: AsyncWriteExt + Unpin>(w: &mut W, resp: &Response) -> Result<()> {
    let mut buf = serde_json::to_vec(resp)?;
    buf.push(b'\n');
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

async fn handle(
    req: Request,
    state: Arc<RwLock<AgentState>>,
    shutdown: Arc<tokio::sync::Notify>,
) -> Response {
    match req {
        Request::Ping => Response::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
        },
        Request::Unlock { password } => match do_unlock(&password, state).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::Lock => {
            state.write().await.lock();
            Response::Ok
        }
        Request::Status => Response::Status(do_status(state).await),
        Request::GetToken { tenant } => match do_get_token(&tenant, state).await {
            Ok((token, expires_at)) => Response::Token { access_token: token, expires_at },
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::Shutdown => {
            shutdown.notify_waiters();
            Response::Ok
        }
    }
}

async fn do_unlock(password: &str, state: Arc<RwLock<AgentState>>) -> Result<()> {
    // Load + decrypt off the async thread — argon2 takes hundreds of ms.
    let password = password.to_string();
    let jwks = tokio::task::spawn_blocking(move || config::load_jwk_map(&password))
        .await
        .map_err(|e| Error::Crypto(format!("unlock task panicked: {e}")))??;

    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml in current dir".into()))?;

    let mut clients = HashMap::new();
    if let Some(jwk_map) = jwks {
        for tenant in &cfg.tenants {
            if let Some(jwk) = jwk_map.get(&tenant.name) {
                clients.insert(
                    tenant.name.clone(),
                    Arc::new(AicClient::new(tenant.clone(), jwk.clone())),
                );
            }
        }
    }

    let mut s = state.write().await;
    s.clients = Some(clients);
    s.touch();
    Ok(())
}

async fn do_status(state: Arc<RwLock<AgentState>>) -> StatusInfo {
    let s = state.read().await;
    let unlocked = s.clients.is_some();
    let tenants: Vec<String> = s
        .clients
        .as_ref()
        .map(|m| {
            let mut names: Vec<_> = m.keys().cloned().collect();
            names.sort();
            names
        })
        .unwrap_or_default();
    let cached_tokens: Vec<CachedTokenInfo> = s
        .clients
        .as_ref()
        .map(|m| {
            m.iter()
                .filter_map(|(name, c)| {
                    let exp = c.token_cache.lock().unwrap().expires_at();
                    if exp > 0 {
                        Some(CachedTokenInfo { tenant: name.clone(), expires_at: exp })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let idle_remaining_secs = if unlocked {
        s.idle_timeout
            .saturating_sub(s.last_request.elapsed())
            .as_secs()
    } else {
        0
    };

    StatusInfo {
        unlocked,
        project_dir: s.project_dir.clone(),
        tenants,
        cached_tokens,
        idle_remaining_secs,
        idle_timeout_secs: s.idle_timeout.as_secs(),
    }
}

async fn do_get_token(
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<(String, i64)> {
    let client = {
        let s = state.read().await;
        let clients = s.clients.as_ref().ok_or_else(|| {
            Error::Auth("agent is locked; run `aic-edit login` first".into())
        })?;
        clients
            .get(tenant)
            .cloned()
            .ok_or_else(|| Error::Config(format!("unknown tenant: {tenant}")))?
    };
    let token = client.bearer().await?;
    let expires_at = client.token_cache.lock().unwrap().expires_at();
    Ok((token, expires_at))
}

/// Used by the foreground subcommand to print where the agent is running.
pub fn describe_paths() -> String {
    format!(
        "socket: {}\npid:    {}\nlog:    {}",
        socket_path().display(),
        pid_path().display(),
        log_path().display(),
    )
}
