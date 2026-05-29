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
    AuthCallbackProgress {
        onboard_id: uuid::Uuid,
        body: serde_json::Value,
        prompt: String,
    },
    /// Onboarding failed somewhere in the background task.
    OnboardError {
        onboard_id: uuid::Uuid,
        message: String,
    },
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
    /// Background tenant-restart trigger finished. Toasts on error.
    EsvRestartResult {
        tenant: String,
        result: std::result::Result<serde_json::Value, String>,
    },
    /// Background ESV-edit save finished. `tenant` + `id` identify the
    /// variable so a stale result for a previously-edited entry can be
    /// dropped.
    EsvSaveResult {
        tenant: String,
        id: String,
        result: std::result::Result<crate::screens::esv::SaveOutcome, String>,
    },
    /// Background ESV delete finished.
    EsvDeleteResult {
        tenant: String,
        id: String,
        result: std::result::Result<crate::screens::esv::DeleteOutcome, String>,
    },
    /// Background undo attempt finished.
    EsvUndoResult {
        undo_id: crate::undo::UndoId,
        tenant: String,
        result: std::result::Result<
            crate::screens::esv::UndoOutcome,
            crate::screens::esv::UndoFailure,
        >,
    },
    /// Background ESV refresh finished for `tenant`. The payload keeps the
    /// variable list and startup status separate so a partial failure doesn't
    /// force us to throw away useful cached state.
    EsvListed {
        tenant: String,
        outcome: crate::screens::esv::RefreshOutcome,
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
