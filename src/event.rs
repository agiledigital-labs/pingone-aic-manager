use std::collections::HashMap;

use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Tick,
    TokenMinted { tenant: String, expires_at: i64 },
    TokenError { tenant: String, error: String },
    ApiResponse { request_id: u64, result: Result<serde_json::Value, String> },
    /// OAuth code received from localhost callback server (Ok = code, Err = message)
    OnboardCallback(std::result::Result<String, String>),
    /// Pattern 2: the AM authentication journey returned a callback we need
    /// extra user input to satisfy (TOTP). `body` is the JSON to POST back
    /// once the user supplies the missing value.
    AuthCallbackProgress { body: serde_json::Value, prompt: String },
    /// Onboarding failed somewhere in the background task.
    OnboardError(String),
    /// Service account created; carries (sa_uuid, private_jwk)
    ServiceAccountCreated { tenant_name: String, sa_id: String, jwk: serde_json::Value },
    /// Background unlock task finished. On success the payload is the password
    /// (kept in memory after unlock) plus the decrypted JWK map; on failure a
    /// human-readable message for the unlock screen.
    UnlockResult(std::result::Result<(String, HashMap<String, serde_json::Value>), String>),
    Toast(ToastKind, String),
}

pub struct EventHandler {
    pub tx: mpsc::UnboundedSender<AppEvent>,
    pub rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx }
    }
}

impl Default for EventHandler {
    fn default() -> Self {
        Self::new()
    }
}
