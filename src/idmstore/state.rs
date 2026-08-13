//! IDM record store UI state.

use std::path::PathBuf;

use crate::config::{ProjectConfig, tenant_file_name};

#[derive(Debug, Default)]
pub struct State;

impl State {
    pub fn new() -> Self {
        Self
    }

    pub fn reset_view(&mut self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Full,
    Incremental,
}

impl SyncMode {
    pub fn label(self) -> &'static str {
        match self {
            SyncMode::Full => "full",
            SyncMode::Incremental => "incremental",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectSyncResult {
    pub object: String,
    pub mode: SyncMode,
    pub incremental_supported: bool,
    pub fetched: usize,
    pub upserted: usize,
    pub deleted: usize,
    pub rows: usize,
    pub watermark: Option<String>,
    pub last_full_sync: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub tenant: String,
    pub store_path: PathBuf,
    pub objects: Vec<ObjectSyncResult>,
}

#[derive(Debug, Clone)]
pub struct ObjectStatus {
    pub object: String,
    pub rows: usize,
    pub incremental_supported: bool,
    pub watermark: Option<String>,
    pub last_full_sync: Option<String>,
}

pub fn store_dir() -> PathBuf {
    ProjectConfig::dir().join("idmstore")
}

pub fn store_path(tenant: &str) -> PathBuf {
    store_dir().join(format!("{}.sqlite", tenant_file_name(tenant)))
}
