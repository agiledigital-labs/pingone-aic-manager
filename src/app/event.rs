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
    Vault(crate::vault::screen::Event),
    Onboard(crate::onboard::screen::Event),
    Esv(crate::esv::screen::Event),
    Secrets(crate::secrets::screen::Event),
    Scripts(crate::scripts::screen::Event),
    Managed(crate::managed::screen::Event),
    Mappings(crate::mappings::screen::Event),
    Access(crate::access::screen::Event),
    IdmStore(crate::idmstore::screen::Event),
    Oauth(crate::oauth::screen::Event),
    Secretmap(crate::secretmap::screen::Event),
    Offboard(crate::offboard::screen::Event),
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
