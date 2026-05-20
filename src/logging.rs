use std::path::PathBuf;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::Result;

fn log_dir() -> Result<PathBuf> {
    let dir = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("aic-edit")
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| crate::Error::Config("HOME not set".into()))?;
        PathBuf::from(home).join(".local/share/aic-edit")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Initialise file logging. Level controlled by AIC_EDIT_LOG env var (default: info).
/// Logs to ~/.local/share/aic-edit/aic-edit.<date>.log
pub fn init() -> Result<PathBuf> {
    let dir = log_dir()?;
    let level_str = std::env::var("AIC_EDIT_LOG").unwrap_or_else(|_| "info".into());

    let file_appender = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .max_log_files(3)
        .filename_prefix("aic-edit")
        .filename_suffix("log")
        .build(&dir)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // Leak the guard so the background thread stays alive for the process lifetime.
    std::mem::forget(_guard);

    let filter = EnvFilter::try_new(&level_str)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    Ok(dir)
}
