use std::collections::{HashMap, VecDeque};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::time::{interval, Duration};

use crate::config::crypto::{self, Dek};
use crate::config::tenant::Tenant;
use crate::config::wraps::WrapsFile;
use crate::config::{self, ProjectConfig, Settings};
use crate::event::{AppEvent, EventHandler, ToastKind};
use crate::ui::toast::Toast;
use crate::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    Normal,
    /// First-run only: pick an auth method (none / master password /
    /// security key) and provide credentials for it.
    SetupAuth,
    /// Subsequent launches: enter the master password and/or tap the
    /// security key to decrypt `keys.enc`.
    Unlock,
    /// Auth settings panel — list factors, add/remove/change-password.
    AuthSettings,
    /// Last-step confirmation overlay (y/n) for destructive auth-settings
    /// actions: remove a factor, disable encryption.
    AuthSettingsConfirm,
    /// Inline editor for renaming the focused security-key wrap.
    AuthSettingsRename,
    OnboardMenu,
    OnboardCookie,
    OnboardUserPass,
    OnboardPaste,
    OverwriteConfirm,
    EnvPicker,
    ProdConfirm,
    /// `/` — fuzzy search the ESV list. Chars edit the query; Esc cancels
    /// (and clears the filter); Enter commits and returns to Normal with
    /// the filter still applied.
    EsvSearch,
}

/// Re-export so existing `crate::app::AuthMethod` / `AuthSetupField` /
/// `SetupContext` / `AuthSetupForm` paths (used in `ui/auth_setup.rs`,
/// `ui/auth_settings.rs`, and inside `App`) keep compiling against the
/// moved types.
pub use crate::screens::auth_setup::{
    AuthMethod, AuthSetupField, AuthSetupForm, SetupContext,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Realm {
    Alpha,
    Bravo,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Esvs,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[Tab::Esvs]
    }

    pub fn label(self) -> &'static str {
        match self {
            Tab::Esvs => "ESVs",
        }
    }
}

pub struct App {
    pub events: EventHandler,
    pub config: Option<ProjectConfig>,
    pub settings: Option<Settings>,
    pub input_mode: InputMode,
    pub current_tab: Tab,
    pub current_realm: Realm,
    pub tenants: Vec<Tenant>,
    pub active_tenant_idx: usize,
    pub env_picker_idx: usize,
    pub toasts: VecDeque<Toast>,
    pub should_quit: bool,
    pub has_env_creds: bool,

    /// Unlock-screen state — owned by `screens::unlock`. See that module
    /// for handlers; the field lives here because lifecycle code
    /// (`decide_initial_mode`, etc.) needs to pre-seed focus.
    pub unlock: crate::screens::unlock::State,


    /// First-run + add-factor screen state. See `screens::auth_setup`.
    pub auth_setup: crate::screens::auth_setup::State,

    /// Auth Settings screen state. See `screens::auth_settings`.
    pub auth_settings: crate::screens::auth_settings::State,

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

    /// Onboarding state — see `screens::onboard`. Owns the four form
    /// drafts, the in-flight bootstrap id, the OTP callback body, the
    /// prod-confirm pending action, and the overwrite-confirm draft.
    pub onboard: crate::screens::onboard::State,

    /// ESV tab state — list cache, refresh book-keeping, search query +
    /// selection. See `crate::screens::esv` for the handlers.
    pub esv: crate::screens::esv::State,
}

pub use crate::screens::onboard::PendingProdAction;

pub use crate::screens::auth_settings::PendingAuthAction;

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

        Ok(Self {
            events: EventHandler::new(),
            config,
            settings,
            input_mode: InputMode::Normal,
            current_tab: Tab::Esvs,
            current_realm: Realm::Alpha,
            tenants,
            active_tenant_idx,
            env_picker_idx: active_tenant_idx,
            toasts: VecDeque::new(),
            should_quit: false,
            has_env_creds,
            unlock: crate::screens::unlock::State::new(),
            auth_setup: crate::screens::auth_setup::State::new(),
            auth_settings: crate::screens::auth_settings::State::new(),
            dek: None,
            wraps,
            jwks: HashMap::new(),
            onboard: crate::screens::onboard::State::new(),
            esv: crate::screens::esv::State::new(),
        })
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
        match self.settings {
            Some(Settings { encrypt_keys: true, .. }) => {
                self.input_mode = InputMode::Unlock;
                // Default focus to whichever method is actually enrolled.
                // If both are enrolled we prefer the security key field — it's the
                // stronger factor and the user can Tab away if they want.
                self.unlock.focus = if self.wraps.has_security_key() {
                    crate::screens::unlock::Focus::SecurityKeyPin
                } else {
                    crate::screens::unlock::Focus::Password
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
                self.input_mode = InputMode::SetupAuth;
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
    }

    pub fn save_jwk(&mut self, tenant_name: &str, jwk: serde_json::Value) -> Result<()> {
        self.jwks.insert(tenant_name.to_string(), jwk);
        self.persist_keys()
    }

    /// Write the current JWK map to disk — encrypted with the in-memory DEK,
    /// or plain (mode 600) if the user opted out of encryption.
    pub fn persist_keys(&self) -> Result<()> {
        let encrypt = self.settings.map(|s| s.encrypt_keys).unwrap_or(true);
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
        if let Some(t) = self.tenants.get(idx) {
            if let Err(e) = config::write_current_context(&t.name) {
                tracing::warn!(error = %e, tenant = %t.name, "failed to persist current-context");
            }
        }
    }

    /// Convenience wrapper around `screens::esv::State::matches` that
    /// supplies the active tenant. Keeps existing callers (UI mostly)
    /// from threading `app.active_tenant()` in every call.
    pub fn esv_matches(&self) -> Vec<crate::screens::esv::Match> {
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
        matches!(self.settings, Some(Settings { encrypt_keys: false, .. }))
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        crate::screens::unlock::try_agent_unlock(self).await;
        self.decide_initial_mode();
        // Plain mode skips the unlock screen entirely; make sure the agent
        // knows what state we're in before any ESV refresh hits its socket.
        // Encrypted mode either gets the DEK from `try_agent_unlock` (idle-
        // window resume) or via `handle_unlock_result` once the user types.
        if matches!(self.settings, Some(Settings { encrypt_keys: false, .. })) {
            crate::screens::unlock::unlock_plain_agent(self).await;
        }
        crate::screens::esv::refresh(self, false);

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
            terminal.draw(|f| crate::ui::draw(f, self))?;
            if self.should_quit {
                break;
            }
            match self.events.rx.recv().await {
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
            AppEvent::ServiceAccountCreated {
                onboard_id,
                tenant_name,
                base_url,
                theme,
                sa_id,
                jwk,
            } => {
                crate::screens::onboard::handle_sa_created(self, onboard_id, tenant_name, base_url, theme, sa_id, jwk)?;
            }
            AppEvent::Toast(kind, msg) => {
                self.push_toast(kind, msg);
            }
            AppEvent::EsvListed { tenant, result } => {
                crate::screens::esv::apply_listed(self, tenant, result);
            }
            AppEvent::AuthCallbackProgress { body, prompt } => {
                crate::screens::onboard::handle_auth_progress(self, body, prompt);
            }
            AppEvent::OnboardError(msg) => {
                tracing::error!(error = %msg, "onboard error");
                crate::screens::onboard::handle_onboard_error(self, msg);
            }
            AppEvent::UnlockResult(r) => crate::screens::unlock::handle_result(self, r).await,
            AppEvent::SecurityKeyEnrollResult(r) => {
                crate::screens::auth_setup::handle_enroll_result(self, r).await
            }
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
        if self.current_tab == Tab::Esvs
            && self.esv.last_poll.elapsed() >= Duration::from_secs(30)
        {
            crate::screens::esv::refresh(self, true);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await?,
            InputMode::SetupAuth => crate::screens::auth_setup::handle_key(self, key).await?,
            InputMode::Unlock => crate::screens::unlock::handle_key(self, key),
            InputMode::AuthSettings => crate::screens::auth_settings::handle_key(self, key)?,
            InputMode::AuthSettingsConfirm => crate::screens::auth_settings::handle_confirm_key(self, key).await?,
            InputMode::AuthSettingsRename => crate::screens::auth_settings::handle_rename_key(self, key)?,
            InputMode::OnboardMenu => crate::screens::onboard::handle_menu_key(self, key).await?,
            InputMode::OnboardCookie => crate::screens::onboard::handle_cookie_key(self, key).await?,
            InputMode::OnboardUserPass => crate::screens::onboard::handle_up_key(self, key).await?,
            InputMode::OnboardPaste => crate::screens::onboard::handle_paste_key(self, key).await?,
            InputMode::OverwriteConfirm => crate::screens::onboard::handle_overwrite_key(self, key)?,
            InputMode::EnvPicker => self.handle_env_picker_key(key),
            InputMode::ProdConfirm => crate::screens::onboard::handle_prod_confirm_key(self, key).await?,
            InputMode::EsvSearch => crate::screens::esv::handle_search_key(self, key),
        }
        Ok(())
    }

    /// Backwards-compat shim for `ui::modal::draw_overwrite_confirm`.
    pub fn pending_overwrite_name(&self) -> Option<&str> {
        self.onboard.pending_overwrite_name()
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        // Tab-specific keys first — these take precedence over the global
        // shortcuts so e.g. `j` in the ESV list scrolls instead of doing
        // nothing.
        if self.current_tab == Tab::Esvs && crate::screens::esv::handle_normal_key(self, key) {
            return Ok(());
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.current_realm = match self.current_realm {
                    Realm::Alpha => Realm::Bravo,
                    Realm::Bravo => Realm::Alpha,
                };
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if !self.tenants.is_empty() {
                    self.env_picker_idx = self.active_tenant_idx;
                    self.input_mode = InputMode::EnvPicker;
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.onboard.menu_idx = 0;
                self.input_mode = InputMode::OnboardMenu;
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                crate::screens::auth_settings::open(self);
            }
            KeyCode::Char('L') => {
                crate::screens::unlock::lock_and_quit(self).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// Backwards-compat shim so existing UI callers
    /// (`ui::auth_settings::draw_confirm`) keep working without touching
    /// the rendering code.
    pub fn pending_auth_action_label(&self) -> Option<String> {
        self.auth_settings.pending_action_label(&self.wraps)
    }

    fn handle_env_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.env_picker_idx + 1 < self.tenants.len() {
                    self.env_picker_idx += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.env_picker_idx > 0 {
                    self.env_picker_idx -= 1;
                }
            }
            KeyCode::Enter => {
                self.set_active_tenant(self.env_picker_idx);
                self.input_mode = InputMode::Normal;
                crate::screens::esv::refresh(self, false);
            }
            KeyCode::Char(c @ '1'..='9') => {
                // Number-key hotkey, matching the Add Tenant menu: switch
                // to tenant N immediately without an Enter step.
                let target = c.to_digit(10).unwrap() as usize - 1;
                if target < self.tenants.len() {
                    self.env_picker_idx = target;
                    self.set_active_tenant(target);
                    self.input_mode = InputMode::Normal;
                    crate::screens::esv::refresh(self, false);
                }
            }
            _ => {}
        }
    }

}



// Re-exported from `crate::auth` so existing `app::UnlockOk` call sites
// (event.rs, etc.) keep compiling against the moved type.
pub use crate::auth::UnlockOk;

/// Enrol a security key and produce a wrap entry that the event handler can
/// append to `wraps.toml`. Blocks until the user taps the device. `hmac_salt`
/// Pick which tenant should be active on startup. Order:
///   1. `.aic-edit/current-context` (set by `aic ctx use ...` or by the env
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

