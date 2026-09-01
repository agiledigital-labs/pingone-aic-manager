//! Client-side socket I/O. Auto-spawns the agent if no live one is found.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::{Error, Result};

use super::protocol::{Request, Response, WireRequest};

pub struct AgentClient {
    stream: UnixStream,
}

impl AgentClient {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let stream = UnixStream::connect(path.as_ref())
            .await
            .map_err(|e| Error::Config(format!("connect {}: {e}", path.as_ref().display())))?;
        Ok(Self { stream })
    }

    /// Connect to the project-local agent, auto-spawning a detached one if
    /// the socket is missing or stale. Returns once the agent answers `Ping`.
    pub async fn connect_or_spawn() -> Result<Self> {
        let sock = super::socket_path();

        // 1. Live agent? Connect and return.
        if sock.exists() && Self::answers_ping(&sock).await? {
            // Reconnect — answers_ping consumed the connection.
            return Self::connect(&sock).await;
        }
        if sock.exists() {
            // Stale socket — agent removes its own on clean exit, but if it
            // crashed it may linger. Remove and re-spawn.
            tracing::debug!("removing stale socket {}", sock.display());
            let _ = std::fs::remove_file(&sock);
        }

        spawn_detached_agent()?;

        // Wait for the socket to appear (agent binds early in startup).
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if sock.exists() && Self::answers_ping(&sock).await? {
                return Self::connect(&sock).await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::Config(format!(
            "agent did not start within 5s; check {}",
            super::log_path().display()
        )))
    }

    async fn answers_ping(path: &Path) -> Result<bool> {
        let Ok(client) = Self::connect(path).await else {
            return Ok(false);
        };
        match client.ping_owned().await {
            Ok(()) => Ok(true),
            Err(Error::AgentProtocolMismatch) => Err(Error::AgentProtocolMismatch),
            Err(_) => Ok(false),
        }
    }

    pub async fn send(mut self, req: &Request) -> Result<Response> {
        let mut buf = serde_json::to_vec(&WireRequest::current(req))?;
        buf.push(b'\n');
        self.stream
            .write_all(&buf)
            .await
            .map_err(map_transport_error)?;
        self.stream.flush().await.map_err(map_transport_error)?;
        // We never write again after this — close our half so the server's
        // read_line returns even if we forgot a newline somewhere.
        let _ = self.stream.shutdown().await;

        let mut reader = BufReader::new(self.stream);
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(map_transport_error)?;
        if n == 0 {
            return Err(agent_protocol_error());
        }
        decode_response(line.trim())
    }

    async fn ping_owned(self) -> Result<()> {
        match self.send(&Request::Ping).await? {
            Response::Pong { .. } => Ok(()),
            other => Err(Error::Config(format!("unexpected ping reply: {other:?}"))),
        }
    }

    /// Store or replace the opaque JSON secret for `(kind, tenant)`. `kind`
    /// names a [`crate::config::VaultArtifact`].
    pub async fn put_secret(
        self,
        kind: &str,
        tenant: &str,
        value: serde_json::Value,
    ) -> Result<()> {
        match self
            .send(&Request::PutSecret {
                kind: kind.to_string(),
                tenant: tenant.to_string(),
                value,
            })
            .await?
        {
            Response::Ok => Ok(()),
            Response::Locked => Err(agent_locked_error()),
            Response::Error { message } => Err(Error::Config(message)),
            _ => Err(Error::Config("unexpected agent reply to PutSecret".into())),
        }
    }

    /// Fetch the opaque JSON secret for `(kind, tenant)`. Returns
    /// [`Error::SecretMissing`] when nothing is stored — callers attach their
    /// own remediation text.
    pub async fn get_secret(self, kind: &str, tenant: &str) -> Result<serde_json::Value> {
        match self
            .send(&Request::GetSecret {
                kind: kind.to_string(),
                tenant: tenant.to_string(),
            })
            .await?
        {
            Response::Secret { value } => Ok(value),
            Response::SecretMissing { kind, tenant } => Err(Error::SecretMissing { kind, tenant }),
            Response::Locked => Err(agent_locked_error()),
            Response::Error { message } => Err(Error::Config(message)),
            _ => Err(Error::Config("unexpected agent reply to GetSecret".into())),
        }
    }

    /// Remove the stored secret for `(kind, tenant)`.
    pub async fn remove_secret(self, kind: &str, tenant: &str) -> Result<()> {
        match self
            .send(&Request::RemoveSecret {
                kind: kind.to_string(),
                tenant: tenant.to_string(),
            })
            .await?
        {
            Response::Ok => Ok(()),
            Response::Locked => Err(agent_locked_error()),
            Response::Error { message } => Err(Error::Config(message)),
            _ => Err(Error::Config(
                "unexpected agent reply to RemoveSecret".into(),
            )),
        }
    }
}

fn decode_response(line: &str) -> Result<Response> {
    let response: Response = serde_json::from_str(line).map_err(|_| agent_protocol_error())?;
    match response {
        Response::ProtocolMismatch { .. } => Err(agent_protocol_error()),
        Response::Error { ref message } if message.starts_with("bad request:") => {
            Err(agent_protocol_error())
        }
        response => Ok(response),
    }
}

fn map_transport_error(error: std::io::Error) -> Error {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::BrokenPipe
        | ErrorKind::ConnectionAborted
        | ErrorKind::ConnectionReset
        | ErrorKind::UnexpectedEof => agent_protocol_error(),
        _ => Error::Io(error),
    }
}

fn agent_protocol_error() -> Error {
    // This is deliberately a heuristic: a legacy daemon cannot identify
    // itself on the unversioned protocol. A broken/closed connection, its old
    // `bad request` reply, or an unknown response shape after a successful
    // connect are the best available evidence that the resident binary
    // predates the CLI.
    Error::AgentProtocolMismatch
}

fn agent_locked_error() -> Error {
    Error::Auth("agent locked; run `aic session login`".into())
}

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().map_err(|e| Error::Config(format!("current_exe: {e}")))
}

/// The agent is project-scoped — socket, pid and log all live in `.aic/` — so
/// there is nothing to run one *for* outside a project.
///
/// This used to be a `create_dir_all`, which meant any command that reached the
/// agent from an unrelated directory silently made a `.aic/` there and left a
/// detached daemon holding a socket in it. The mistake is easy to make and
/// invisible afterwards: the command still failed, with a message about the
/// missing config, so nothing pointed at the orphan it had just started.
/// Creating a project is `aic onboard`'s job, never a side effect of opening a
/// log file.
fn ensure_project_for_agent(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    Err(Error::Config(format!(
        "no project here ({} does not exist), so there is no agent to start; \
         run aic from a project directory, or pass --project <dir>",
        dir.display()
    )))
}

fn spawn_detached_agent() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = current_exe()?;
    let log_path = super::log_path();
    ensure_project_for_agent(&crate::config::ProjectConfig::dir())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log.try_clone()?;

    // No --detach flag here: the child should run the daemon loop directly.
    // We're already detaching it via stdio redirection + setsid(), so asking
    // the child to detach again would just fork bomb.
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("agent")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    // Detach: become a new session leader so a controlling-terminal HUP
    // doesn't take us down with it, and we survive the spawning CLI exiting.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                // setsid fails if we're already a session leader; not fatal.
            }
            Ok(())
        });
    }

    let child = cmd
        .spawn()
        .map_err(|e| Error::Config(format!("spawn agent: {e}")))?;
    tracing::debug!(pid = child.id(), "spawned agent");
    // Intentionally do not wait — agent runs detached. Dropping `child` does
    // not kill it; only the file handles are closed.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_is_refused_where_there_is_no_project() {
        let bare = std::env::temp_dir().join(format!("aic-noproj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&bare).unwrap();
        let err = ensure_project_for_agent(&bare.join(".aic")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no project here"), "unhelpful error: {msg}");
        assert!(
            msg.contains("--project"),
            "the error should name the way out: {msg}"
        );
        assert!(
            !bare.join(".aic").exists(),
            "checking for a project must not create one"
        );
    }

    #[test]
    fn an_agent_is_allowed_where_a_project_exists() {
        let root = std::env::temp_dir().join(format!("aic-proj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".aic")).unwrap();
        assert!(ensure_project_for_agent(&root.join(".aic")).is_ok());
    }

    #[test]
    fn structured_mismatch_renders_stop_remedy() {
        let line = serde_json::to_string(&Response::ProtocolMismatch {
            expected: super::super::PROTOCOL_VERSION,
            received: super::super::PROTOCOL_VERSION + 1,
        })
        .unwrap();

        let error = decode_response(&line).unwrap_err();

        assert!(matches!(error, Error::AgentProtocolMismatch));
        assert!(error.to_string().contains("aic session stop"));
        assert!(error.to_string().contains("re-run"));
    }

    #[test]
    fn malformed_response_renders_stop_remedy() {
        let error = decode_response("not json").unwrap_err();

        assert!(matches!(error, Error::AgentProtocolMismatch));
        assert!(error.to_string().contains("aic session stop"));
    }

    #[test]
    fn broken_pipe_renders_stop_remedy() {
        let error = map_transport_error(std::io::Error::from(std::io::ErrorKind::BrokenPipe));

        assert!(matches!(error, Error::AgentProtocolMismatch));
        assert!(error.to_string().contains("aic session stop"));
    }
}
