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
    Onboard(crate::onboard::screen::Event),
    /// Background unlock task finished. On success the payload carries the
    /// decrypted DEK + JWK map; on failure a human-readable message for the
    /// unlock screen.
    UnlockResult(std::result::Result<crate::auth::UnlockOk, String>),
    /// User-triggered security key enrolment finished. The payload is the new
    /// wrap entry ready to be appended to `wraps.toml`, or an error string.
    SecurityKeyEnrollResult(std::result::Result<crate::config::wraps::Wrap, String>),
    Esv(crate::esv::screen::Event),
    Secrets(crate::secrets::screen::Event),
    Scripts(crate::scripts::screen::Event),
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
