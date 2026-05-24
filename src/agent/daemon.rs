//! Agent daemon: listens on a Unix socket, holds decrypted JWKs in memory,
//! mints/refreshes bearer tokens on request.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;

use crate::aic::AicClient;
use crate::config::{self, crypto::Dek, ProjectConfig};
use crate::{Error, Result};

use super::protocol::{CachedTokenInfo, Request, Response, StatusInfo};
use super::{log_path, pid_path, socket_path};

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 3600;

struct AgentState {
    project_dir: String,
    /// `None` when locked. When unlocked, the 32-byte DEK that decrypts
    /// `keys.enc`. The TUI hands us this directly after a security-key
    /// unlock; the password unlock path derives it from Argon2 internally.
    dek: Option<Dek>,
    /// AicClients are built lazily on the first `GetToken { tenant }` after
    /// unlock and cached here for their token-cache + HTTP-connection-pool
    /// benefit. Cleared on `Lock` (and whenever the DEK changes).
    clients: HashMap<String, Arc<AicClient>>,
    last_request: Instant,
    idle_timeout: Duration,
}

impl AgentState {
    fn new(project_dir: String, idle_timeout: Duration) -> Self {
        Self {
            project_dir,
            dek: None,
            clients: HashMap::new(),
            last_request: Instant::now(),
            idle_timeout,
        }
    }

    fn touch(&mut self) {
        self.last_request = Instant::now();
    }

    fn lock(&mut self) {
        self.dek = None;
        self.clients.clear();
    }

    fn is_unlocked(&self) -> bool {
        self.dek.is_some()
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
                if s.is_unlocked() && s.last_request.elapsed() >= s.idle_timeout {
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
        Request::PutDek { dek_b64 } => match do_put_dek(&dek_b64, state).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::GetDek => {
            let s = state.read().await;
            match &s.dek {
                Some(dek) => Response::Dek {
                    dek_b64: B64.encode(dek.as_bytes()),
                },
                None => Response::Locked,
            }
        }
        Request::Lock => {
            state.write().await.lock();
            Response::Ok
        }
        Request::Status => Response::Status(do_status(state).await),
        Request::GetToken { tenant } => match do_get_token(&tenant, state).await {
            Ok(Some((token, expires_at))) => Response::Token { access_token: token, expires_at },
            Ok(None) => Response::Locked,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::ApiGet { tenant, path } => match do_api_get(&tenant, &path, state).await {
            Ok(Some(value)) => Response::Json { value },
            Ok(None) => Response::Locked,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::Shutdown => {
            shutdown.notify_waiters();
            Response::Ok
        }
    }
}

async fn do_unlock(password: &str, state: Arc<RwLock<AgentState>>) -> Result<()> {
    // Argon2 takes hundreds of ms — keep it off the async thread.
    let password = password.to_string();
    let (dek, _jwks) =
        tokio::task::spawn_blocking(move || config::unlock_with_password(&password))
            .await
            .map_err(|e| Error::Crypto(format!("unlock task panicked: {e}")))??;
    set_dek(state, dek).await;
    Ok(())
}

async fn do_put_dek(dek_b64: &str, state: Arc<RwLock<AgentState>>) -> Result<()> {
    let bytes = B64
        .decode(dek_b64)
        .map_err(|e| Error::Crypto(format!("put_dek: {e}")))?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Crypto("put_dek: DEK must be 32 bytes".into()))?;
    set_dek(state, Dek::from_bytes(arr)).await;
    Ok(())
}

/// Replace whatever the daemon was holding with a freshly-supplied DEK. Any
/// AicClients we'd cached are dropped so they get rebuilt against the new
/// key the next time someone asks for a token.
async fn set_dek(state: Arc<RwLock<AgentState>>, dek: Dek) {
    let mut s = state.write().await;
    s.dek = Some(dek);
    s.clients.clear();
    s.touch();
}

async fn do_status(state: Arc<RwLock<AgentState>>) -> StatusInfo {
    let s = state.read().await;
    let unlocked = s.is_unlocked();
    let mut tenants: Vec<String> = s.clients.keys().cloned().collect();
    tenants.sort();
    let cached_tokens: Vec<CachedTokenInfo> = s
        .clients
        .iter()
        .filter_map(|(name, c)| {
            let exp = c.token_cache.lock().unwrap().expires_at();
            if exp > 0 {
                Some(CachedTokenInfo { tenant: name.clone(), expires_at: exp })
            } else {
                None
            }
        })
        .collect();

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

/// Return `Ok(None)` when the agent is locked so the caller can answer with
/// `Response::Locked`. `Ok(Some(_))` on success, `Err` on any other failure.
async fn do_get_token(
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<Option<(String, i64)>> {
    // Fast path: AicClient was built on a previous request.
    {
        let s = state.read().await;
        if s.dek.is_none() {
            return Ok(None);
        }
        if let Some(client) = s.clients.get(tenant).cloned() {
            drop(s);
            let token = client.bearer().await?;
            let expires_at = client.token_cache.lock().unwrap().expires_at();
            return Ok(Some((token, expires_at)));
        }
    }

    // Slow path: decrypt keys.enc with the cached DEK and find the JWK for
    // the named tenant. Cache the AicClient for the next request.
    let client = build_client(tenant, state.clone()).await?;
    let token = client.bearer().await?;
    let expires_at = client.token_cache.lock().unwrap().expires_at();
    Ok(Some((token, expires_at)))
}

/// Proxy a GET against AIC. Returns `Ok(None)` when the agent is locked so
/// the caller can answer with `Response::Locked`; `Ok(Some(body))` on
/// success. No caching here — every call goes to AIC. Connection pooling
/// happens automatically via the per-tenant AicClient.
async fn do_api_get(
    tenant: &str,
    path: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<Option<serde_json::Value>> {
    // Each `state.read()` is scoped to a let-binding so the read guard
    // drops before we try anything that might need a write lock. tokio's
    // RwLock is write-preferring, so a write blocked by our own read guard
    // would also block every later reader and wedge the daemon.
    let (has_dek, cached_client) = {
        let s = state.read().await;
        (s.dek.is_some(), s.clients.get(tenant).cloned())
    };
    if !has_dek {
        return Ok(None);
    }
    let client = match cached_client {
        Some(c) => c,
        None => build_client(tenant, state.clone()).await?,
    };
    let body = client.get(path).await?;
    Ok(Some(body))
}

async fn build_client(
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<Arc<AicClient>> {
    let dek = {
        let s = state.read().await;
        s.dek
            .clone()
            .ok_or_else(|| Error::Auth("agent is locked".into()))?
    };
    let jwks = config::decrypt_keys_file(&dek)?;
    let jwk = jwks
        .get(tenant)
        .cloned()
        .ok_or_else(|| Error::Config(format!("no JWK on file for tenant: {tenant}")))?;
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic-edit/config.toml in current dir".into()))?;
    let tcfg = cfg
        .tenants
        .iter()
        .find(|t| t.name == tenant)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown tenant: {tenant}")))?;
    let client = Arc::new(AicClient::new(tcfg, jwk));
    state
        .write()
        .await
        .clients
        .insert(tenant.to_string(), client.clone());
    Ok(client)
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
