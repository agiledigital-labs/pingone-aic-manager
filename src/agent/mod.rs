//! In-process agent that holds decrypted service-account JWKs in memory and
//! hands out short-lived bearer tokens. Talked to over a Unix socket in
//! `.aic/agent.sock`.
//!
//! Why an agent: tokens have a ~898s TTL and minting them requires the
//! decrypted private JWK. Holding the JWKs in a long-lived process lets the
//! CLI authenticate once per session — same shape as `ssh-agent`.
//!
//! ## Lock vs. stop (why `logout` doesn't kill the process)
//!
//! Two distinct lifecycle operations, deliberately separate:
//!
//! - **Lock** (`Request::Lock`, CLI `aic session logout`, and the idle-timeout): swaps
//!   the vault to [`daemon`]'s `Vault::Locked` — dropping the `Dek` — and clears
//!   the cached `AicClient`s (which hold the bearer tokens). The process keeps
//!   running with the socket still bound.
//! - **Stop** (`Request::Shutdown`, CLI `aic session stop`): exits the process and
//!   removes the socket.
//!
//! Lock is the security-equivalent of stop: after it, *nothing sensitive is
//! left in memory* (no DEK, no tokens, no clients), so killing the process buys
//! no extra secret hygiene. What lock preserves is the cheap, shared
//! infrastructure — one Unix socket serving every connected frontend (multiple
//! TUIs + CLI). A locked agent keeps those connections alive: each just sees
//! `Response::Locked` until someone runs `aic session login`, which re-pushes the DEK
//! and makes everyone live again instantly. Killing would drop all connections
//! and force the next caller to spawn a fresh process and wait for the socket
//! (`connect_or_spawn` has a 5s timeout), with a respawn race if several start
//! at once. Manual lock is also just the on-demand form of the idle auto-lock,
//! so the two paths stay identical.
//!
//! Practical corollary: a code change to the agent (anything under this module,
//! e.g. `AicClient` response handling) only takes effect after a real process
//! restart — `aic session stop` then relaunch. `logout`/lock keeps the *old binary*
//! resident and will not pick up the new code.

pub mod client;
pub mod daemon;
pub mod protocol;

use std::path::PathBuf;

use crate::config::ProjectConfig;

pub use client::AgentClient;
pub use protocol::{CachedTokenInfo, Request, Response, StatusInfo};

pub fn socket_path() -> PathBuf {
    ProjectConfig::dir().join("agent.sock")
}

pub fn pid_path() -> PathBuf {
    ProjectConfig::dir().join("agent.pid")
}

pub fn log_path() -> PathBuf {
    ProjectConfig::dir().join("agent.log")
}
