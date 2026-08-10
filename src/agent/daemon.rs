//! Agent daemon: listens on a Unix socket, holds decrypted JWKs in memory,
//! mints/refreshes bearer tokens on request.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;

use crate::aic::AicClient;
use crate::config::{self, ProjectConfig, VaultArtifact, crypto::Dek};
use crate::{Error, Result};

use super::protocol::{CachedTokenInfo, Request, Response, StatusInfo, WireRequest};
use super::{PROTOCOL_VERSION, log_path, pid_path, socket_path};

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 3600;

/// Credential vault state the daemon holds in memory.
///
/// The JWK map is cached beside the unlock material. Its backing file's mtime
/// gates reloads, so foreground re-onboarding invalidates clients without
/// making steady-state requests decrypt or parse the file again.
enum Vault {
    Locked,
    Encrypted {
        dek: Dek,
        cache: Option<(HashMap<String, serde_json::Value>, SystemTime)>,
    },
    Plain {
        cache: Option<(HashMap<String, serde_json::Value>, SystemTime)>,
    },
}

struct AgentState {
    project_dir: String,
    vault: Vault,
    /// AicClients are built lazily on the first `GetToken { tenant }` after
    /// unlock and cached here for their token-cache + HTTP-connection-pool
    /// benefit. Cleared on `Lock` (and whenever the vault changes).
    clients: HashMap<String, Arc<AicClient>>,
    last_request: Instant,
    idle_timeout: Duration,
}

impl AgentState {
    fn new(project_dir: String, idle_timeout: Duration) -> Self {
        Self {
            project_dir,
            vault: Vault::Locked,
            clients: HashMap::new(),
            last_request: Instant::now(),
            idle_timeout,
        }
    }

    fn touch(&mut self) {
        self.last_request = Instant::now();
    }

    fn lock(&mut self) {
        self.vault = Vault::Locked;
        self.clients.clear();
    }

    fn is_unlocked(&self) -> bool {
        !matches!(self.vault, Vault::Locked)
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
    let state = Arc::new(RwLock::new(AgentState::new(
        project_dir.clone(),
        idle_timeout,
    )));

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
            let accepted = listener.accept().await;
            match accepted {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let shutdown = shutdown.clone();
                    drop(tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, state, shutdown).await {
                            tracing::warn!(error = %e, "connection handler failed");
                        }
                    }));
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
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut s) => {
            s.recv().await;
        }
        _ => {
            futures::future::pending::<()>().await;
        }
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
    let wire: WireRequest<Request> = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            send(
                &mut write,
                &Response::Error {
                    message: format!("bad request: {e}"),
                },
            )
            .await?;
            return Ok(());
        }
    };
    if let Some(response) = protocol_mismatch(wire.protocol_version) {
        send(&mut write, &response).await?;
        return Ok(());
    }
    let req = wire.request;

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

fn protocol_mismatch(received: u32) -> Option<Response> {
    (received != PROTOCOL_VERSION).then_some(Response::ProtocolMismatch {
        expected: PROTOCOL_VERSION,
        received,
    })
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
        Request::PutDek { dek_b64 } => match do_put_dek(&dek_b64, state).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::UnlockPlain => match do_unlock_plain(state).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::GetDek => {
            let s = state.read().await;
            match &s.vault {
                Vault::Encrypted { dek, .. } => Response::Dek {
                    dek_b64: B64.encode(dek.as_bytes()),
                },
                Vault::Plain { .. } | Vault::Locked => Response::Locked,
            }
        }
        Request::Lock => {
            state.write().await.lock();
            Response::Ok
        }
        Request::Status => Response::Status(do_status(state).await),
        Request::SetIdleTimeout { secs } => {
            state.write().await.idle_timeout = Duration::from_secs(secs);
            Response::Ok
        }
        Request::GetToken { tenant } => match do_get_token(&tenant, state).await {
            Ok(Some((token, expires_at))) => Response::Token {
                access_token: token,
                expires_at,
            },
            Ok(None) => Response::Locked,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::PutSecret {
            kind,
            tenant,
            value,
        } => match do_put_secret(&kind, &tenant, value, state).await {
            Ok(true) => Response::Ok,
            Ok(false) => Response::Locked,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::GetSecret { kind, tenant } => match do_get_secret(&kind, &tenant, state).await {
            Ok(Some(value)) => Response::Secret { value },
            Ok(None) => Response::Locked,
            Err(Error::SecretMissing { kind, tenant }) => Response::SecretMissing { kind, tenant },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::RemoveSecret { kind, tenant } => {
            match do_remove_secret(&kind, &tenant, state).await {
                Ok(true) => Response::Ok,
                Ok(false) => Response::Locked,
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
        Request::ApiCall {
            tenant,
            method,
            path,
            body,
            confirmed_prod,
            content_type,
            api_version,
            if_match,
        } => {
            match do_api_call(
                &tenant,
                &method,
                &path,
                body,
                ApiCallOptions {
                    confirmed_prod,
                    content_type,
                    api_version,
                    if_match,
                },
                state,
            )
            .await
            {
                Ok(Some(value)) => Response::Json { value },
                Ok(None) => Response::Locked,
                Err(Error::ProdConfirmRequired) => Response::ProdConfirmRequired,
                Err(Error::Api { status, body }) => Response::ApiError { status, body },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
        Request::Shutdown => {
            shutdown.notify_waiters();
            Response::Ok
        }
    }
}

async fn do_put_dek(dek_b64: &str, state: Arc<RwLock<AgentState>>) -> Result<()> {
    let bytes = B64
        .decode(dek_b64)
        .map_err(|e| Error::Crypto(format!("put_dek: {e}")))?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Crypto("put_dek: DEK must be 32 bytes".into()))?;
    set_vault(
        state,
        Vault::Encrypted {
            dek: Dek::from_bytes(arr),
            cache: None,
        },
    )
    .await;
    Ok(())
}

/// Load `keys.plain` into the agent — the "no encryption" mode. Used by the
/// TUI/CLI when `settings.encrypt_keys = false`. Errors if the file is
/// missing or malformed; callers should fall through to a clear "set up
/// encryption first" message in that case.
async fn do_unlock_plain(state: Arc<RwLock<AgentState>>) -> Result<()> {
    let bytes = ProjectConfig::load_keys_plain()?
        .ok_or_else(|| Error::Config("no .aic/keys.plain on disk".into()))?;
    let map = serde_json::from_slice(&bytes)?;
    let cache = std::fs::metadata(ProjectConfig::keys_plain_path())
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(|modified| (map, modified));
    set_vault(state, Vault::Plain { cache }).await;
    Ok(())
}

/// Replace whatever the daemon was holding. Any AicClients we'd cached are
/// dropped so they get rebuilt against the new vault the next time someone
/// asks for a token.
async fn set_vault(state: Arc<RwLock<AgentState>>, vault: Vault) {
    let mut s = state.write().await;
    s.vault = vault;
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
                Some(CachedTokenInfo {
                    tenant: name.clone(),
                    expires_at: exp,
                })
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
    if !state.read().await.is_unlocked() {
        return Ok(None);
    }
    // This must precede the client fast path: a foreground re-onboarding can
    // replace the JWK while this daemon is still holding its old AicClient.
    refresh_jwk_cache(state.clone()).await?;

    // Fast path: AicClient was built on a previous request.
    {
        let s = state.read().await;
        if !s.is_unlocked() {
            return Ok(None);
        }
        let client = s.clients.get(tenant).cloned();
        drop(s);
        if let Some(client) = client {
            let token = client.bearer().await?;
            let expires_at = client.token_cache.lock().unwrap().expires_at();
            return Ok(Some((token, expires_at)));
        }
    }

    // Slow path: derive the JWK from the refreshed vault cache and retain the
    // AicClient for its token-cache + HTTP-connection-pool benefit.
    let client = build_client(tenant, state.clone()).await?;
    let token = client.bearer().await?;
    let expires_at = client.token_cache.lock().unwrap().expires_at();
    Ok(Some((token, expires_at)))
}

/// Resolve a wire `kind` to its [`VaultArtifact`], erroring on an unknown name.
fn artifact_for(kind: &str) -> Result<VaultArtifact> {
    VaultArtifact::from_kind(kind)
        .ok_or_else(|| Error::Config(format!("unknown secret kind: {kind}")))
}

async fn do_put_secret(
    kind: &str,
    tenant: &str,
    value: serde_json::Value,
    state: Arc<RwLock<AgentState>>,
) -> Result<bool> {
    let artifact = artifact_for(kind)?;
    let s = state.write().await;
    let Some(mut map) = load_secret_map(artifact, &s.vault)? else {
        return Ok(false);
    };
    map.insert(tenant.to_string(), value);
    save_secret_map(artifact, &s.vault, &map)?;
    Ok(true)
}

async fn do_get_secret(
    kind: &str,
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<Option<serde_json::Value>> {
    let artifact = artifact_for(kind)?;
    let s = state.read().await;
    let Some(map) = load_secret_map(artifact, &s.vault)? else {
        return Ok(None);
    };
    map.get(tenant)
        .cloned()
        .map(Some)
        .ok_or_else(|| Error::SecretMissing {
            kind: kind.to_string(),
            tenant: tenant.to_string(),
        })
}

async fn do_remove_secret(
    kind: &str,
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<bool> {
    let artifact = artifact_for(kind)?;
    let s = state.write().await;
    let Some(mut map) = load_secret_map(artifact, &s.vault)? else {
        return Ok(false);
    };
    map.remove(tenant);
    save_secret_map(artifact, &s.vault, &map)?;
    Ok(true)
}

/// The decrypted per-tenant secret map for `artifact`, or `None` when the
/// vault is locked. Values are opaque JSON — the daemon never inspects them.
fn load_secret_map(
    artifact: VaultArtifact,
    vault: &Vault,
) -> Result<Option<HashMap<String, serde_json::Value>>> {
    let dek = match vault {
        Vault::Locked => return Ok(None),
        Vault::Encrypted { dek, .. } => Some(dek),
        Vault::Plain { .. } => None,
    };
    let map = match config::load_artifact_bytes(artifact, dek)? {
        Some(bytes) if !bytes.is_empty() => serde_json::from_slice(&bytes)?,
        _ => HashMap::new(),
    };
    Ok(Some(map))
}

fn save_secret_map(
    artifact: VaultArtifact,
    vault: &Vault,
    map: &HashMap<String, serde_json::Value>,
) -> Result<()> {
    let dek = match vault {
        Vault::Locked => return Err(Error::Auth("agent is locked".into())),
        Vault::Encrypted { dek, .. } => Some(dek),
        Vault::Plain { .. } => None,
    };
    config::save_artifact_bytes(artifact, &serde_json::to_vec(map)?, dek)
}

/// Proxy a tenant-scoped HTTP call to AIC. Returns `Ok(None)` when the
/// agent is locked so the caller can answer with `Response::Locked`.
/// Connection pooling happens automatically via the per-tenant AicClient.
struct ApiCallOptions {
    confirmed_prod: bool,
    content_type: Option<String>,
    api_version: Option<String>,
    if_match: Option<String>,
}

async fn do_api_call(
    tenant: &str,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    options: ApiCallOptions,
    state: Arc<RwLock<AgentState>>,
) -> Result<Option<serde_json::Value>> {
    if !state.read().await.is_unlocked() {
        return Ok(None);
    }
    // Keep API calls consistent with GetToken: no cached client may outlive a
    // changed keys file.
    refresh_jwk_cache(state.clone()).await?;

    // Each `state.read()` is scoped to a let-binding so the read guard
    // drops before we try anything that might need a write lock. tokio's
    // RwLock is write-preferring, so a write blocked by our own read guard
    // would also block every later reader and wedge the daemon.
    let (unlocked, cached_client) = {
        let s = state.read().await;
        (s.is_unlocked(), s.clients.get(tenant).cloned())
    };
    if !unlocked {
        return Ok(None);
    }
    let client = match cached_client {
        Some(c) => c,
        None => build_client(tenant, state.clone()).await?,
    };
    let av = options.api_version.as_deref();
    let value = match method {
        "GET" => client.get(path, av).await?,
        "POST" if options.content_type.as_deref() == Some("application/x-www-form-urlencoded") => {
            let body = body
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| Error::Config("form request body must be a JSON string".into()))?;
            client
                .write_form(reqwest::Method::POST, path, &body, options.confirmed_prod)
                .await?
        }
        "POST" => {
            client
                .write(
                    reqwest::Method::POST,
                    path,
                    body.unwrap_or(serde_json::Value::Null),
                    options.confirmed_prod,
                    av,
                    None,
                )
                .await?
        }
        "PUT" => {
            client
                .write(
                    reqwest::Method::PUT,
                    path,
                    body.unwrap_or(serde_json::Value::Null),
                    options.confirmed_prod,
                    av,
                    options.if_match.as_deref(),
                )
                .await?
        }
        "PATCH" => {
            client
                .write(
                    reqwest::Method::PATCH,
                    path,
                    body.unwrap_or(serde_json::Value::Null),
                    options.confirmed_prod,
                    av,
                    None,
                )
                .await?
        }
        "DELETE" => {
            client
                .write(
                    reqwest::Method::DELETE,
                    path,
                    body.unwrap_or(serde_json::Value::Null),
                    options.confirmed_prod,
                    av,
                    None,
                )
                .await?
        }
        other => return Err(Error::Config(format!("unsupported method: {other}"))),
    };
    Ok(Some(value))
}

/// Refresh the JWK cache if its backing file changed since the last load.
///
/// A metadata failure keeps an existing cache usable (for transient filesystem
/// failures); without a cache we reload so the existing file-read error remains
/// the caller-visible result.
async fn refresh_jwk_cache(state: Arc<RwLock<AgentState>>) -> Result<()> {
    let (path, cached_mtime, dek) = {
        let s = state.read().await;
        match &s.vault {
            Vault::Locked => return Err(Error::Auth("agent is locked".into())),
            Vault::Encrypted { dek, cache } => (
                ProjectConfig::keys_path(),
                cache.as_ref().map(|(_, mtime)| *mtime),
                Some(dek.clone()),
            ),
            Vault::Plain { cache } => (
                ProjectConfig::keys_plain_path(),
                cache.as_ref().map(|(_, mtime)| *mtime),
                None,
            ),
        }
    };
    let modified = std::fs::metadata(path).and_then(|metadata| metadata.modified());
    let modified = match modified {
        Ok(modified) if Some(modified) == cached_mtime => return Ok(()),
        Ok(modified) => modified,
        Err(_) if cached_mtime.is_some() => return Ok(()),
        // Reload to preserve the old missing/corrupt-file error semantics.
        // A later successful stat replaces the sentinel with the real
        // timestamp.
        Err(_) => SystemTime::UNIX_EPOCH,
    };

    let map = match dek {
        Some(dek) => config::decrypt_keys_file(&dek)?,
        None => match ProjectConfig::load_keys_plain()? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => HashMap::new(),
        },
    };
    let mut s = state.write().await;
    match &mut s.vault {
        Vault::Locked => return Ok(()),
        Vault::Encrypted { cache, .. } | Vault::Plain { cache }
            if cache.as_ref().is_some_and(|(_, mtime)| *mtime == modified) =>
        {
            return Ok(());
        }
        Vault::Encrypted { cache, .. } | Vault::Plain { cache } => {
            *cache = Some((map, modified));
        }
    }
    s.clients.clear();
    Ok(())
}

/// Refresh from disk before building a client. Direct callers use this entry
/// point; request handlers that need a fast path call `refresh_jwk_cache`
/// themselves before consulting `state.clients`.
async fn build_client(tenant: &str, state: Arc<RwLock<AgentState>>) -> Result<Arc<AicClient>> {
    refresh_jwk_cache(state.clone()).await?;
    build_client_from_cache(tenant, state).await
}

async fn build_client_from_cache(
    tenant: &str,
    state: Arc<RwLock<AgentState>>,
) -> Result<Arc<AicClient>> {
    let jwk = {
        let s = state.read().await;
        match &s.vault {
            Vault::Locked => return Err(Error::Auth("agent is locked".into())),
            Vault::Encrypted { cache, .. } | Vault::Plain { cache } => cache
                .as_ref()
                .and_then(|(map, _)| map.get(tenant))
                .cloned()
                .ok_or_else(|| Error::Config(format!("no JWK on file for tenant: {tenant}")))?,
        }
    };
    let cfg = ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic/config.toml in current dir".into()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tenant::{Tenant, TenantTheme};
    static CWD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct CurrentDir {
        original: std::path::PathBuf,
    }

    impl Drop for CurrentDir {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).unwrap();
        }
    }

    fn tenant(name: &str) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: "https://example.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some("service-account".into()),
            scopes: Vec::new(),
        }
    }

    struct TestProject {
        directory: std::path::PathBuf,
    }

    fn test_project() -> (TestProject, CurrentDir) {
        let project = TestProject {
            directory: std::env::temp_dir()
                .join(format!("aic-daemon-test-{}", uuid::Uuid::new_v4())),
        };
        std::fs::create_dir(&project.directory).unwrap();
        let restore = CurrentDir {
            original: std::env::current_dir().unwrap(),
        };
        std::env::set_current_dir(&project.directory).unwrap();
        (project, restore)
    }

    fn save_config(names: &[&str]) {
        ProjectConfig {
            project: "test".into(),
            default_tenant: "sandbox".into(),
            tenants: names.iter().map(|name| tenant(name)).collect(),
        }
        .save()
        .unwrap();
    }

    fn set_mtime(path: &std::path::Path, modified: SystemTime) {
        std::fs::File::open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    fn state(directory: &std::path::Path) -> Arc<RwLock<AgentState>> {
        Arc::new(RwLock::new(AgentState::new(
            directory.display().to_string(),
            Duration::from_secs(60),
        )))
    }

    #[test]
    fn current_protocol_is_accepted_and_mismatch_is_structured() {
        let encoded = serde_json::to_string(&WireRequest::current(Request::Ping)).unwrap();
        let current: WireRequest<Request> = serde_json::from_str(&encoded).unwrap();

        assert_eq!(current.protocol_version, PROTOCOL_VERSION);
        assert!(matches!(current.request, Request::Ping));
        assert!(protocol_mismatch(current.protocol_version).is_none());
        assert!(matches!(
            protocol_mismatch(PROTOCOL_VERSION + 1),
            Some(Response::ProtocolMismatch {
                expected: PROTOCOL_VERSION,
                received,
            }) if received == PROTOCOL_VERSION + 1
        ));

        let legacy: WireRequest<Request> = serde_json::from_str(r#"{"op":"ping"}"#).unwrap();
        assert!(matches!(
            protocol_mismatch(legacy.protocol_version),
            Some(Response::ProtocolMismatch {
                expected: PROTOCOL_VERSION,
                received: 0,
            })
        ));
    }

    #[tokio::test]
    async fn encrypted_vault_refreshes_reonboarded_jwk_and_invalidates_clients() {
        // Regression: foreground re-onboarding rewrites keys.enc while the
        // daemon still has an AicClient bound to the previous JWK map.
        let _cwd = CWD_LOCK.lock().await;
        let (project, restore) = test_project();
        save_config(&["sandbox"]);
        let dek = Dek::random();
        let old_map = HashMap::from([("sandbox".into(), serde_json::json!({"old": "jwk"}))]);
        config::save_jwk_map(&old_map, &dek).unwrap();
        let first_mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        set_mtime(&ProjectConfig::keys_path(), first_mtime);

        let state = state(&project.directory);
        set_vault(
            state.clone(),
            Vault::Encrypted {
                dek: dek.clone(),
                cache: None,
            },
        )
        .await;
        build_client("sandbox", state.clone()).await.unwrap();
        assert!(state.read().await.clients.contains_key("sandbox"));

        save_config(&["sandbox", "reonboarded"]);
        let new_map = HashMap::from([
            ("sandbox".into(), serde_json::json!({"old": "jwk"})),
            ("reonboarded".into(), serde_json::json!({"new": "jwk"})),
        ]);
        config::save_jwk_map(&new_map, &dek).unwrap();
        set_mtime(
            &ProjectConfig::keys_path(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(20),
        );

        build_client("reonboarded", state.clone()).await.unwrap();
        let state_guard = state.read().await;
        assert!(!state_guard.clients.contains_key("sandbox"));
        assert!(state_guard.clients.contains_key("reonboarded"));

        drop(state_guard);
        drop(restore);
        std::fs::remove_dir_all(project.directory).unwrap();
    }

    #[tokio::test]
    async fn unchanged_mtime_reuses_cached_map_and_clients() {
        let _cwd = CWD_LOCK.lock().await;
        let (project, restore) = test_project();
        save_config(&["sandbox"]);
        ProjectConfig::save_keys_plain(br#"{"sandbox":{"cached":"jwk"}}"#).unwrap();
        set_mtime(
            &ProjectConfig::keys_plain_path(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(10),
        );

        let state = state(&project.directory);
        set_vault(state.clone(), Vault::Plain { cache: None }).await;

        let first = build_client("sandbox", state.clone()).await.unwrap();
        refresh_jwk_cache(state.clone()).await.unwrap();
        let state_guard = state.read().await;
        assert!(Arc::ptr_eq(
            &first,
            state_guard.clients.get("sandbox").unwrap()
        ));

        drop(state_guard);
        drop(restore);
        std::fs::remove_dir_all(project.directory).unwrap();
    }

    #[tokio::test]
    async fn plain_vault_refreshes_a_reonboarded_jwk_on_mtime_change() {
        let _cwd = CWD_LOCK.lock().await;
        let (project, restore) = test_project();
        save_config(&["sandbox"]);
        ProjectConfig::save_keys_plain(br#"{"sandbox":{"old":"jwk"}}"#).unwrap();
        set_mtime(
            &ProjectConfig::keys_plain_path(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(10),
        );

        let state = state(&project.directory);
        set_vault(state.clone(), Vault::Plain { cache: None }).await;
        build_client("sandbox", state.clone()).await.unwrap();

        save_config(&["sandbox", "reonboarded"]);
        ProjectConfig::save_keys_plain(br#"{"sandbox":{"old":"jwk"},"reonboarded":{"new":"jwk"}}"#)
            .unwrap();
        set_mtime(
            &ProjectConfig::keys_plain_path(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(20),
        );

        assert!(build_client("reonboarded", state).await.is_ok());

        drop(restore);
        std::fs::remove_dir_all(project.directory).unwrap();
    }
}
