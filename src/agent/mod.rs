//! In-process agent that holds decrypted service-account JWKs in memory and
//! hands out short-lived bearer tokens. Talked to over a Unix socket in
//! `.aic-edit/agent.sock`.
//!
//! Why an agent: tokens have a ~898s TTL and minting them requires the
//! decrypted private JWK. Holding the JWKs in a long-lived process lets the
//! CLI authenticate once per session — same shape as `ssh-agent`.

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
