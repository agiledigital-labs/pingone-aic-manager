//! Client-side socket I/O. Auto-spawns the agent if no live one is found.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::{Error, Result};

use super::protocol::{Request, Response};

pub struct AgentClient {
    stream: UnixStream,
}

impl AgentClient {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let stream = UnixStream::connect(path.as_ref()).await.map_err(|e| {
            Error::Config(format!(
                "connect {}: {e}",
                path.as_ref().display()
            ))
        })?;
        Ok(Self { stream })
    }

    /// Connect to the project-local agent, auto-spawning a detached one if
    /// the socket is missing or stale. Returns once the agent answers `Ping`.
    pub async fn connect_or_spawn() -> Result<Self> {
        let sock = super::socket_path();

        // 1. Live agent? Connect and return.
        if sock.exists() {
            if let Ok(c) = Self::connect(&sock).await {
                // Best-effort ping to confirm it answers, not a stale FD.
                if matches!(c.ping_owned().await, Ok(())) {
                    // Reconnect — ping_owned consumed the connection.
                    return Self::connect(&sock).await;
                }
            }
            // Stale socket — agent removes its own on clean exit, but if it
            // crashed it may linger. Remove and re-spawn.
            tracing::debug!("removing stale socket {}", sock.display());
            let _ = std::fs::remove_file(&sock);
        }

        spawn_detached_agent()?;

        // Wait for the socket to appear (agent binds early in startup).
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if sock.exists() {
                if let Ok(c) = Self::connect(&sock).await {
                    if matches!(c.ping_owned().await, Ok(())) {
                        return Self::connect(&sock).await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::Config(format!(
            "agent did not start within 5s; check {}",
            super::log_path().display()
        )))
    }

    pub async fn send(mut self, req: &Request) -> Result<Response> {
        let mut buf = serde_json::to_vec(req)?;
        buf.push(b'\n');
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;
        // We never write again after this — close our half so the server's
        // read_line returns even if we forgot a newline somewhere.
        let _ = self.stream.shutdown().await;

        let mut reader = BufReader::new(self.stream);
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(Error::Config("agent closed connection without reply".into()));
        }
        let resp: Response = serde_json::from_str(line.trim())?;
        Ok(resp)
    }

    async fn ping_owned(self) -> Result<()> {
        match self.send(&Request::Ping).await? {
            Response::Pong { .. } => Ok(()),
            other => Err(Error::Config(format!("unexpected ping reply: {other:?}"))),
        }
    }
}

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .map_err(|e| Error::Config(format!("current_exe: {e}")))
}

fn spawn_detached_agent() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = current_exe()?;
    let log_path = super::log_path();
    std::fs::create_dir_all(crate::config::ProjectConfig::dir())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log.try_clone()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("agent")
        .arg("--foreground")
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

    let child = cmd.spawn().map_err(|e| {
        Error::Config(format!("spawn agent: {e}"))
    })?;
    tracing::debug!(pid = child.id(), "spawned agent");
    // Intentionally do not wait — agent runs detached. Dropping `child` does
    // not kill it; only the file handles are closed.
    Ok(())
}
