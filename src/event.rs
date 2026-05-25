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
    /// Pattern 2: the AM authentication journey returned a callback we need
    /// extra user input to satisfy (TOTP). `body` is the JSON to POST back
    /// once the user supplies the missing value.
    AuthCallbackProgress { body: serde_json::Value, prompt: String },
    /// Onboarding failed somewhere in the background task.
    OnboardError(String),
    /// Service account created. `onboard_id` matches the `pending_onboard_id`
    /// stamped on the App when the bootstrap was kicked off — handler must
    /// drop the event when the id doesn't match (user cancelled, or a stale
    /// completion arrived after a different bootstrap already started). The
    /// task carries `base_url` + `theme` so the handler doesn't have to look
    /// them up on a form that may have been cleared.
    ServiceAccountCreated {
        onboard_id: uuid::Uuid,
        tenant_name: String,
        base_url: String,
        theme: crate::config::tenant::TenantTheme,
        sa_id: String,
        jwk: serde_json::Value,
    },
    /// Background unlock task finished. On success the payload carries the
    /// decrypted DEK + JWK map; on failure a human-readable message for the
    /// unlock screen.
    UnlockResult(std::result::Result<crate::auth::UnlockOk, String>),
    /// User-triggered security key enrolment finished. The payload is the new
    /// wrap entry ready to be appended to `wraps.toml`, or an error string.
    SecurityKeyEnrollResult(std::result::Result<crate::config::wraps::Wrap, String>),
    /// Background ESV-variables list fetch finished for `tenant`. The result
    /// holds the variable objects (raw JSON) or a human-readable error.
    EsvListed {
        tenant: String,
        result: std::result::Result<Vec<serde_json::Value>, String>,
    },
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
