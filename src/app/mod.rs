pub mod draw;
pub mod env_picker;
pub mod event;
pub mod keymap;
pub mod prod_confirm;
pub mod selector;

use std::collections::{HashMap, VecDeque};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;
use tokio::time::{Duration, interval};

use crate::app::event::{AppEvent, EventHandler, ToastKind};
use crate::config::crypto::{self, Dek};
use crate::config::tenant::Tenant;
use crate::config::wraps::WrapsFile;
use crate::config::{self, ProjectConfig, Settings, VaultArtifact};
use crate::logs::{LogKeyMap, LogKeyPair};
use crate::tui::toast::Toast;
use crate::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    Normal,
    Vault(crate::vault::screen::Mode),
    Onboard(crate::onboard::screen::Mode),
    EnvPicker,
    Offboard(crate::offboard::screen::Mode),
    Selector,
    ProdConfirm,
    UndoHistory,
    Esv(crate::esv::screen::Mode),
    Secrets(crate::secrets::screen::Mode),
    Scripts(crate::scripts::screen::Mode),
    Managed(crate::managed::screen::Mode),
    Mappings(crate::mappings::screen::Mode),
    Access(crate::access::screen::Mode),
    IdmStore(crate::idmstore::screen::Mode),
    Oauth(crate::oauth::screen::Mode),
    Secretmap(crate::secretmap::screen::Mode),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Realm {
    Alpha,
    Bravo,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Esvs,
    Scripts,
    Managed,
    Mappings,
    Access,
    IdmStore,
    Oauth,
}

impl View {
    pub fn all() -> &'static [View] {
        &[
            View::Esvs,
            View::Scripts,
            View::Managed,
            View::Mappings,
            View::Access,
            View::IdmStore,
            View::Oauth,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Esvs => "ESVs",
            View::Scripts => "Scripts",
            View::Managed => "Managed",
            View::Mappings => "Mappings",
            View::Access => "Access",
            View::IdmStore => "Query",
            View::Oauth => "OAuth",
        }
    }
}

pub struct App {
    pub events: EventHandler,
    pub config: Option<ProjectConfig>,
    pub settings: Option<Settings>,
    pub input_mode: InputMode,
    pub active_view: View,
    pub current_realm: Realm,
    pub tenants: Vec<Tenant>,
    pub active_tenant_idx: usize,
    pub env_picker_idx: usize,
    pub selector: selector::State,
    pub undo_history_idx: usize,
    pub toasts: VecDeque<Toast>,
    pub should_quit: bool,
    pub has_env_creds: bool,

    /// Unlock-screen state — owned by `vault::unlock`. See that module
    /// for handlers; the field lives here because lifecycle code
    /// (`decide_initial_mode`, etc.) needs to pre-seed focus.
    pub unlock: crate::vault::unlock::State,

    /// First-run + add-factor screen state. See `vault::setup`.
    pub auth_setup: crate::vault::setup::State,

    /// Auth Settings screen state. See `vault::settings`.
    pub auth_settings: crate::vault::settings::State,

    /// Data encryption key (random 32 bytes), held only while unlocked.
    /// `None` either means "not yet unlocked" or "user opted out of
    /// encryption" (see `settings.encrypt_keys`). The DEK is wrapped on disk
    /// by every enrolled unlock method (`wraps.toml`).
    dek: Option<Dek>,

    /// Loaded wrap envelope, kept in memory so the unlock screen can decide
    /// which methods to offer and the enrolment flow can append new entries.
    pub wraps: WrapsFile,

    // JWK map (decrypted; keyed by tenant name). Held in memory only so the
    // onboarding flow can insert a freshly-generated JWK and re-encrypt the
    // file; all AIC HTTP goes through the agent (see `aic::api`) so we never
    // need a JWK to mint a token from this process.
    jwks: HashMap<String, serde_json::Value>,

    /// Decrypted log API key pairs, keyed by tenant name. Kept alongside
    /// `jwks` so onboarding can add a freshly-created pair without replacing
    /// credentials already stored for other tenants.
    log_keys: LogKeyMap,

    /// Onboarding state — see `crate::onboard`. Owns the form
    /// drafts, the in-flight bootstrap id, the OTP callback body, and the
    /// overwrite-confirm draft.
    pub onboard: crate::onboard::screen::State,

    /// Pending production write confirmation shared by onboarding, ESV edits,
    /// and any future write-capable screens.
    pub prod_confirm: crate::app::prod_confirm::State,

    /// Best-effort undo log. Domain screens record through the trait so
    /// they don't know whether the entry was persisted or held in memory.
    pub undo: Box<dyn crate::undo::UndoLog>,

    /// Confirm-style keybind help popover. This overlays the current screen
    /// without changing the underlying input mode.
    pub keybind_help_open: bool,

    /// ESV view state — list cache, refresh book-keeping, search query +
    /// selection. See `crate::esv` for the handlers.
    pub esv: crate::esv::state::State,

    /// Secrets sub-view of ESVs (list, versions, create/add forms).
    /// Populated by the same poll as `esv`. See `crate::secrets`.
    pub secret: crate::secrets::state::State,

    /// Scripts view state — per-tenant candidate list, search + selection,
    /// in-flight pull/push tracking. See `crate::scripts::screen`.
    pub scripts: crate::scripts::screen::State,

    /// Managed view state — per-tenant schema cache, search + selection.
    /// See `crate::managed::screen`.
    pub managed: crate::managed::state::State,

    /// Mappings view state — per-tenant IDM sync mappings list.
    /// See `crate::mappings::screen`.
    pub mappings: crate::mappings::state::State,

    /// Access view state — per-tenant raw `config/access` rule document.
    /// See `crate::access::screen`.
    pub access: crate::access::state::State,

    /// Query view state — local IDM managed-object record store.
    /// See `crate::idmstore::screen`.
    pub idmstore: crate::idmstore::state::State,

    /// OAuth view state — per-tenant alpha client list + lazy detail cache.
    /// See `crate::oauth::screen`.
    pub oauth: crate::oauth::state::State,

    /// Secret-mapping sub-view state — per-tenant alpha mapping list + alias picker.
    /// See `crate::secretmap::screen`.
    pub secretmap: crate::secretmap::state::State,

    /// Env-picker delete-tenant modal. See `crate::offboard`.
    pub offboard: crate::offboard::screen::State,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = ProjectConfig::load()?;
        let settings = Settings::load()?;
        let wraps = WrapsFile::load()?.unwrap_or_default();
        let tenants = config
            .as_ref()
            .map(|c| c.tenants.clone())
            .unwrap_or_default();

        // Match whatever the CLI's `aic ctx use` last persisted, falling back
        // to the project's default_tenant. Falling back further to 0 keeps
        // first-run (where the file doesn't exist) sane.
        let active_tenant_idx = pick_initial_tenant_idx(&tenants, config.as_ref());

        // Sandbox import path is offered when the three required vars are
        // already exported in our environment (typically via direnv loading
        // the project's .envrc).
        let has_env_creds = std::env::var("TENANT_BASE_URL").is_ok()
            && std::env::var("SERVICE_ACCOUNT_ID").is_ok()
            && std::env::var("SERVICE_ACCOUNT_KEY").is_ok();

        // Undo entries persist across sessions; retire anything older than the
        // TTL up front so a stale entry can't be offered for undo. Best-effort:
        // a failed sweep just leaves the old statuses in place.
        let mut undo: Box<dyn crate::undo::UndoLog> =
            Box::new(crate::undo::DiskLog::load_default()?);
        if let Err(e) = undo.expire_stale(chrono::Utc::now()) {
            tracing::warn!("undo expire-stale sweep failed: {e}");
        }

        Ok(Self::from_parts(
            config,
            settings,
            wraps,
            tenants,
            active_tenant_idx,
            View::Esvs,
            has_env_creds,
            undo,
        ))
    }

    /// Build a fully in-memory app for unit tests.
    ///
    /// Unlike [`App::new`], this does not load configuration, inspect the
    /// process environment, initialise persistent storage, or sweep the undo
    /// log. Keep it test-only: separating runtime loading from construction is
    /// a wider design decision than the unit tests need.
    #[cfg(test)]
    pub fn for_test(tenants: Vec<Tenant>, active_view: View) -> Self {
        Self::from_parts(
            None,
            None,
            WrapsFile::default(),
            tenants,
            0,
            active_view,
            false,
            Box::new(crate::undo::MemoryLog::new()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        config: Option<ProjectConfig>,
        settings: Option<Settings>,
        wraps: WrapsFile,
        tenants: Vec<Tenant>,
        active_tenant_idx: usize,
        active_view: View,
        has_env_creds: bool,
        undo: Box<dyn crate::undo::UndoLog>,
    ) -> Self {
        Self {
            events: EventHandler::new(),
            config,
            settings,
            input_mode: InputMode::Normal,
            active_view,
            current_realm: Realm::Alpha,
            tenants,
            active_tenant_idx,
            env_picker_idx: active_tenant_idx,
            selector: selector::State::new(active_view),
            undo_history_idx: 0,
            toasts: VecDeque::new(),
            should_quit: false,
            has_env_creds,
            unlock: crate::vault::unlock::State::new(),
            auth_setup: crate::vault::setup::State::new(),
            auth_settings: crate::vault::settings::State::new(),
            dek: None,
            wraps,
            jwks: HashMap::new(),
            log_keys: HashMap::new(),
            onboard: crate::onboard::screen::State::new(),
            prod_confirm: crate::app::prod_confirm::State::new(),
            undo,
            keybind_help_open: false,
            esv: crate::esv::state::State::new(),
            secret: crate::secrets::state::State::new(),
            scripts: crate::scripts::screen::State::new(),
            managed: crate::managed::state::State::new(),
            mappings: crate::mappings::state::State::new(),
            access: crate::access::state::State::new(),
            idmstore: crate::idmstore::state::State::new(),
            oauth: crate::oauth::state::State::new(),
            secretmap: crate::secretmap::state::State::new(),
            offboard: crate::offboard::screen::State::new(),
        }
    }

    /// Pick the initial mode from on-disk state:
    ///   - DEK already hydrated (e.g. via `try_agent_unlock`) → `Normal`.
    ///   - `settings.toml` says encrypt_keys=true → `Unlock`.
    ///   - `settings.toml` says encrypt_keys=false → `Normal`; load `keys.plain`.
    ///   - No `settings.toml` → first run → `SetupAuth`.
    fn decide_initial_mode(&mut self) {
        if self.dek.is_some() {
            self.input_mode = InputMode::Normal;
            return;
        }
        match self.settings.as_ref() {
            Some(Settings {
                encrypt_keys: true, ..
            }) => {
                self.input_mode = InputMode::Vault(crate::vault::screen::Mode::Unlock);
                // Default focus to whichever method is actually enrolled.
                // If both are enrolled we prefer the security key field — it's the
                // stronger factor and the user can Tab away if they want.
                self.unlock.focus = if self.wraps.has_security_key() {
                    crate::vault::unlock::Focus::SecurityKeyPin
                } else {
                    crate::vault::unlock::Focus::Password
                };
            }
            Some(Settings {
                encrypt_keys: false,
                ..
            }) => {
                self.load_plain_keys();
                self.input_mode = InputMode::Normal;
            }
            None => {
                self.input_mode = InputMode::Vault(crate::vault::screen::Mode::Setup);
            }
        }

        // The security key poll is not auto-spawned here: hmac-secret needs a PIN,
        // so the user has to enter it on the Unlock screen first. The poll
        // is spawned from `handle_unlock_key` once that happens.
    }

    fn load_plain_keys(&mut self) {
        if let Ok(Some(bytes)) = ProjectConfig::load_keys_plain() {
            if let Ok(map) = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&bytes) {
                self.jwks = map;
            }
        }
        if let Ok(Some(bytes)) = config::load_artifact_bytes(VaultArtifact::LogKeys, None) {
            if let Ok(map) = serde_json::from_slice::<LogKeyMap>(&bytes) {
                self.log_keys = map;
            }
        }
    }

    pub fn save_jwk(&mut self, tenant_name: &str, jwk: serde_json::Value) -> Result<()> {
        self.jwks.insert(tenant_name.to_string(), jwk);
        self.persist_keys()
    }

    /// Write the current JWK map to disk — encrypted with the in-memory DEK,
    /// or plain (mode 600) if the user opted out of encryption.
    pub fn persist_keys(&self) -> Result<()> {
        let encrypt = self
            .settings
            .as_ref()
            .map(|settings| settings.encrypt_keys)
            .unwrap_or(true);
        let bytes = serde_json::to_vec(&self.jwks)?;
        if encrypt {
            let dek = self
                .dek
                .as_ref()
                .ok_or_else(|| Error::Crypto("not unlocked — no DEK in memory".into()))?;
            let enc = crypto::encrypt_data(&bytes, dek)?;
            ProjectConfig::save_keys_enc(&enc)?;
        } else {
            ProjectConfig::save_keys_plain(&bytes)?;
        }
        Ok(())
    }

    pub fn save_log_key(&mut self, tenant_name: &str, pair: LogKeyPair) -> Result<()> {
        self.log_keys.insert(tenant_name.to_string(), pair);
        self.persist_log_keys()
    }

    /// Write the current log API key map using the same vault mode and DEK
    /// source as the service-account JWK map.
    pub fn persist_log_keys(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.log_keys)?;
        let dek = if self
            .settings
            .as_ref()
            .map(|settings| settings.encrypt_keys)
            .unwrap_or(true)
        {
            Some(
                self.dek
                    .as_ref()
                    .ok_or_else(|| Error::Crypto("not unlocked — no DEK in memory".into()))?,
            )
        } else {
            None
        };
        config::save_artifact_bytes(VaultArtifact::LogKeys, &bytes, dek)
    }

    pub fn push_toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toasts.push_front(Toast::new(kind, message.into()));
        if self.toasts.len() > 5 {
            self.toasts.pop_back();
        }
    }

    pub fn active_tenant(&self) -> Option<&Tenant> {
        self.tenants.get(self.active_tenant_idx)
    }

    /// Single point of mutation for `active_tenant_idx` — also writes the
    /// per-project `current-context` file so the CLI (`aic ctx use ...`) and
    /// TUI agree on the active tenant. Best-effort: a write failure is
    /// logged but doesn't take the TUI down.
    pub fn set_active_tenant(&mut self, idx: usize) {
        self.active_tenant_idx = idx;
        // Drop ESV view state (filter + selection) when the data behind it
        // changes — anything else is just confusing.
        self.esv.reset_view();
        self.secret.reset_view();
        self.scripts.reset_view();
        self.managed.reset_view();
        self.mappings.reset_view();
        self.access.reset_view();
        self.idmstore.reset_view();
        self.oauth.reset_view();
        self.secretmap.reset_view();
        let _mappings_allowed = self
            .tenants
            .get(idx)
            .is_some_and(|tenant| tenant.allows_secret_mappings());
        self.esv.view = self.esv.view.clamp(_mappings_allowed);
        if let Some(t) = self.tenants.get(idx) {
            if let Err(e) = config::write_current_context(&t.name) {
                tracing::warn!(error = %e, tenant = %t.name, "failed to persist current-context");
            }
        }
        refresh_view(self, self.active_view, false);
    }

    /// Convenience wrapper around `esv::state::State::matches` that
    /// supplies the active tenant. Keeps existing callers (UI mostly)
    /// from threading `app.active_tenant()` in every call.
    pub fn esv_matches(&self) -> Vec<crate::esv::state::Match> {
        self.esv
            .matches(self.active_tenant().map(|t| t.name.as_str()))
    }

    /// True iff the in-memory DEK is set — meaning credentials are encrypted
    /// and the user is currently unlocked. The header uses this to decide
    /// whether to surface security key-enrol shortcuts.
    pub fn dek_is_set(&self) -> bool {
        self.dek.is_some()
    }

    /// Cheap clone of the current DEK (if any) — used by the unlock
    /// module's "PutDek to the agent" helper. The DEK lives on `App`
    /// because multiple screens read/write it.
    pub fn dek_clone(&self) -> Option<Dek> {
        self.dek.clone()
    }

    pub fn set_dek(&mut self, dek: Option<Dek>) {
        self.dek = dek;
    }

    pub fn set_jwks(&mut self, map: HashMap<String, serde_json::Value>) {
        self.jwks = map;
    }

    pub fn set_log_keys(&mut self, map: LogKeyMap) {
        self.log_keys = map;
    }

    /// Read-only access to the decrypted JWK map for callers that need to
    /// serialise it (e.g. plain-mode write of `keys.plain` from auth-setup).
    pub fn jwks(&self) -> &HashMap<String, serde_json::Value> {
        &self.jwks
    }

    /// True iff the TUI is ready to call AIC: either we hold a DEK
    /// (encrypted mode, unlocked) or `settings.encrypt_keys = false`
    /// (plain mode — there's no DEK, the JWKs live in `keys.plain`).
    /// Plain mode still requires the agent's plain-vault to be loaded, but
    /// `try_agent_unlock` + the post-setup hook handle that.
    pub fn is_unlocked(&self) -> bool {
        if self.dek.is_some() {
            return true;
        }
        matches!(
            self.settings.as_ref(),
            Some(Settings {
                encrypt_keys: false,
                ..
            })
        )
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        crate::vault::unlock::try_agent_unlock(self).await;
        self.decide_initial_mode();
        // Plain mode skips the unlock screen entirely; make sure the agent
        // knows what state we're in before any ESV refresh hits its socket.
        // Encrypted mode either gets the DEK from `try_agent_unlock` (idle-
        // window resume) or via `handle_unlock_result` once the user types.
        if matches!(
            self.settings.as_ref(),
            Some(Settings {
                encrypt_keys: false,
                ..
            })
        ) {
            crate::vault::unlock::unlock_plain_agent(self).await;
        }
        crate::esv::ops::refresh(self, false);

        let tx = self.events.tx.clone();
        tokio::spawn(async move {
            let mut stream = EventStream::new();
            while let Some(Ok(event)) = stream.next().await {
                if let Event::Key(key) = event {
                    let _ = tx.send(AppEvent::Key(key));
                }
            }
        });

        let tx_tick = self.events.tx.clone();
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_millis(500));
            loop {
                tick.tick().await;
                if tx_tick.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        loop {
            terminal.draw(|f| crate::app::draw::draw(f, self))?;
            if self.should_quit {
                break;
            }
            let event = self.events.rx.recv().await;
            match event {
                Some(ev) => self.handle_event(ev).await?,
                None => break,
            }
        }
        Ok(())
    }

    pub async fn handle_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::Key(key) => self.handle_key(key).await?,
            AppEvent::Tick => self.tick(),
            AppEvent::Vault(event) => crate::vault::screen::apply_event(self, event).await,
            AppEvent::Onboard(event) => crate::onboard::screen::apply_event(self, event)?,
            AppEvent::Toast(kind, msg) => {
                self.push_toast(kind, msg);
            }
            AppEvent::Esv(event) => crate::esv::screen::apply_event(self, event),
            AppEvent::Secrets(event) => crate::secrets::screen::apply_event(self, event),
            AppEvent::Scripts(event) => crate::scripts::screen::apply_event(self, event),
            AppEvent::Managed(event) => crate::managed::screen::apply_event(self, event),
            AppEvent::Mappings(event) => crate::mappings::screen::apply_event(self, event),
            AppEvent::Access(event) => crate::access::screen::apply_event(self, event),
            AppEvent::IdmStore(event) => crate::idmstore::screen::apply_event(self, event),
            AppEvent::Oauth(event) => crate::oauth::screen::apply_event(self, event),
            AppEvent::Secretmap(event) => crate::secretmap::screen::apply_event(self, event),
            AppEvent::Offboard(event) => crate::offboard::screen::apply_event(self, event),
        }
        Ok(())
    }

    fn tick(&mut self) {
        self.toasts.retain_mut(|t| {
            if t.ticks_remaining == 0 {
                false
            } else {
                t.ticks_remaining -= 1;
                true
            }
        });

        // Background poll for the resource the user is currently viewing.
        // 30s cadence is a tradeoff between freshness and sandbox load —
        // the sandbox API itself takes ~7s to answer a list, so polling
        // faster than that just stacks in-flight requests.
        if self.active_view == View::Esvs && self.esv.last_poll.elapsed() >= Duration::from_secs(30)
        {
            crate::esv::ops::refresh(self, true);
        }
        // Scripts list is heavier to fetch (one list call per namespace) and
        // changes far less often than ESVs, so poll it on a slower cadence.
        if self.active_view == View::Scripts
            && self.scripts.last_poll.elapsed() >= Duration::from_secs(120)
        {
            crate::scripts::screen::refresh(self, true);
        }
        crate::vault::unlock::poll_agent_status(self);
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.keybind_help_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::F(1)
            ) {
                self.keybind_help_open = false;
            }
            return Ok(());
        }

        if self.should_open_keybind_help(key) {
            self.keybind_help_open = true;
            return Ok(());
        }

        // One dispatch entry point for every mode — see `crate::app::keymap`.
        crate::app::keymap::dispatch(self, key).await
    }

    fn should_open_keybind_help(&self, key: KeyEvent) -> bool {
        if key.code == KeyCode::F(1) {
            return true;
        }

        // `?` is only intercepted in list/command modes where it cannot be
        // mistaken for text entry.
        key.code == KeyCode::Char('?')
            && matches!(
                self.input_mode,
                InputMode::Normal
                    | InputMode::Vault(crate::vault::screen::Mode::Settings)
                    | InputMode::Onboard(crate::onboard::screen::Mode::Menu)
                    | InputMode::EnvPicker
                    | InputMode::Offboard(_)
            )
    }

    pub fn handle_env_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete => {
                crate::offboard::screen::open_from_picker(self);
            }
            KeyCode::Char('j') | KeyCode::Down if self.env_picker_idx + 1 < self.tenants.len() => {
                self.env_picker_idx += 1;
            }
            KeyCode::Char('k') | KeyCode::Up if self.env_picker_idx > 0 => {
                self.env_picker_idx -= 1;
            }
            KeyCode::Enter => {
                self.set_active_tenant(self.env_picker_idx);
                self.input_mode = InputMode::Normal;
                crate::esv::ops::refresh(self, true);
            }
            KeyCode::Char(c @ '1'..='9') => {
                // Number-key hotkey, matching the Add Tenant menu: switch
                // to tenant N immediately without an Enter step.
                let target = c.to_digit(10).unwrap() as usize - 1;
                if target < self.tenants.len() {
                    self.env_picker_idx = target;
                    self.set_active_tenant(target);
                    self.input_mode = InputMode::Normal;
                    crate::esv::ops::refresh(self, true);
                }
            }
            _ => {}
        }
    }
}

pub fn refresh_view(app: &mut App, view: View, force: bool) {
    match view {
        View::Esvs => crate::esv::ops::refresh(app, force),
        View::Scripts => crate::scripts::screen::refresh(app, force),
        View::Managed => crate::managed::screen::refresh(app, force),
        View::Mappings => crate::mappings::screen::refresh(app, force),
        View::Access => crate::access::screen::refresh(app, force),
        View::IdmStore => crate::idmstore::screen::refresh(app, force),
        View::Oauth => crate::oauth::screen::refresh(app, force),
    }
}

/// Pick which tenant should be active on startup. Order:
///   1. `.aic/current-context` (set by `aic ctx use ...` or by the env
///      picker on the previous TUI session)
///   2. `config.default_tenant`
///   3. index 0 (first-run, or stale current-context pointing at a removed
///      tenant)
fn pick_initial_tenant_idx(tenants: &[Tenant], config: Option<&ProjectConfig>) -> usize {
    if tenants.is_empty() {
        return 0;
    }
    if let Ok(Some(name)) = config::read_current_context() {
        if let Some(i) = tenants.iter().position(|t| t.name == name) {
            return i;
        }
    }
    if let Some(cfg) = config {
        if !cfg.default_tenant.is_empty() {
            if let Some(i) = tenants.iter().position(|t| t.name == cfg.default_tenant) {
                return i;
            }
        }
    }
    0
}
