use std::collections::{HashMap, VecDeque};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::time::{interval, Duration};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{CallbackOutcome, UpField, UpForm};
use crate::config::crypto::{self, Dek};
use crate::config::tenant::{Tenant, TenantTheme};
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

    // Onboard state
    pub onboard_menu_idx: usize,
    pub cookie_form: Option<CookieForm>,
    pub up_form: Option<UpForm>,
    pub paste_form: Option<PasteForm>,
    /// UUID stamped on the in-flight bootstrap task. Set when the user kicks
    /// off Pattern 1/2 (cookie / userpass), cleared on Esc-cancel. When a
    /// `ServiceAccountCreated` event arrives with a non-matching id, the
    /// handler drops it instead of persisting a tenant the user no longer
    /// wants.
    pending_onboard_id: Option<uuid::Uuid>,

    // For Pattern 2: the in-flight callback JSON we POST'd that needs an extra prompt
    pending_callback_body: Option<serde_json::Value>,

    // Prod confirm: pending action after confirmation
    pending_prod_action: Option<PendingProdAction>,

    // Overwrite confirm: a pending tenant whose name collides with an existing one
    pending_overwrite: Option<(Tenant, serde_json::Value)>,

    /// ESV tab state — list cache, refresh book-keeping, search query +
    /// selection. See `crate::screens::esv` for the handlers.
    pub esv: crate::screens::esv::State,
}

enum PendingProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: serde_json::Value,
    },
}

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
            onboard_menu_idx: 0,
            cookie_form: None,
            up_form: None,
            paste_form: None,
            pending_onboard_id: None,
            pending_callback_body: None,
            pending_prod_action: None,
            pending_overwrite: None,
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

    fn save_jwk(&mut self, tenant_name: &str, jwk: serde_json::Value) -> Result<()> {
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

    /// Save a tenant outright — replacing any existing entry with the same
    /// name. Caller is responsible for confirming the overwrite before calling.
    fn persist_tenant_overwriting(&mut self, tenant: Tenant, jwk: serde_json::Value) -> Result<()> {
        self.save_jwk(&tenant.name, jwk)?;

        // Replace any existing entry with the same name, or append.
        if let Some(idx) = self.tenants.iter().position(|t| t.name == tenant.name) {
            self.tenants[idx] = tenant.clone();
            self.set_active_tenant(idx);
        } else {
            self.tenants.push(tenant.clone());
            self.set_active_tenant(self.tenants.len() - 1);
        }

        let project = self
            .config
            .as_ref()
            .map(|c| c.project.clone())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "aic-project".into())
            });

        let default_tenant = self
            .tenants
            .first()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let config = ProjectConfig {
            project,
            default_tenant,
            tenants: self.tenants.clone(),
        };
        config.save()?;
        self.config = Some(config);
        Ok(())
    }

    /// Persist a new tenant. If a tenant with the same name already exists,
    /// switch to the OverwriteConfirm modal and bail out — the caller's flow
    /// is paused until the user answers.
    fn persist_new_tenant(&mut self, tenant: Tenant, jwk: serde_json::Value) -> Result<()> {
        if self.tenants.iter().any(|t| t.name == tenant.name) {
            self.pending_overwrite = Some((tenant, jwk));
            self.input_mode = InputMode::OverwriteConfirm;
            return Ok(());
        }
        self.persist_tenant_overwriting(tenant, jwk)
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
    fn set_active_tenant(&mut self, idx: usize) {
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
                self.handle_sa_created(onboard_id, tenant_name, base_url, theme, sa_id, jwk)?;
            }
            AppEvent::Toast(kind, msg) => {
                self.push_toast(kind, msg);
            }
            AppEvent::EsvListed { tenant, result } => {
                crate::screens::esv::apply_listed(self, tenant, result);
            }
            AppEvent::AuthCallbackProgress { body, prompt } => {
                self.handle_auth_progress(body, prompt);
            }
            AppEvent::OnboardError(msg) => {
                tracing::error!(error = %msg, "onboard error");
                self.handle_onboard_error(msg);
            }
            AppEvent::UnlockResult(r) => crate::screens::unlock::handle_result(self, r).await,
            AppEvent::SecurityKeyEnrollResult(r) => crate::screens::auth_setup::handle_enroll_result(self, r),
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
            InputMode::OnboardMenu => self.handle_onboard_menu_key(key).await?,
            InputMode::OnboardCookie => self.handle_cookie_key(key).await?,
            InputMode::OnboardUserPass => self.handle_up_key(key).await?,
            InputMode::OnboardPaste => self.handle_paste_key(key).await?,
            InputMode::OverwriteConfirm => self.handle_overwrite_key(key)?,
            InputMode::EnvPicker => self.handle_env_picker_key(key),
            InputMode::ProdConfirm => self.handle_prod_confirm_key(key).await?,
            InputMode::EsvSearch => crate::screens::esv::handle_search_key(self, key),
        }
        Ok(())
    }

    fn handle_overwrite_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some((tenant, jwk)) = self.pending_overwrite.take() {
                    self.input_mode = InputMode::Normal;
                    match self.persist_tenant_overwriting(tenant, jwk) {
                        Ok(()) => self.push_toast(ToastKind::Success, "Tenant overwritten"),
                        Err(e) => {
                            self.push_toast(ToastKind::Error, format!("Save failed: {e}"));
                        }
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.pending_overwrite = None;
                self.input_mode = InputMode::Normal;
                self.push_toast(ToastKind::Info, "Overwrite cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    pub fn pending_overwrite_name(&self) -> Option<&str> {
        self.pending_overwrite
            .as_ref()
            .map(|(t, _)| t.name.as_str())
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
                self.onboard_menu_idx = 0;
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

    async fn handle_onboard_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        let max_idx = if self.has_env_creds { 3 } else { 2 };
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.onboard_menu_idx < max_idx {
                    self.onboard_menu_idx += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.onboard_menu_idx > 0 {
                    self.onboard_menu_idx -= 1;
                }
            }
            KeyCode::Enter => self.enter_onboard_choice(self.onboard_menu_idx).await?,
            KeyCode::Char('1') => self.enter_onboard_choice(0).await?,
            KeyCode::Char('2') => self.enter_onboard_choice(1).await?,
            KeyCode::Char('3') => self.enter_onboard_choice(2).await?,
            KeyCode::Char('4') if self.has_env_creds => self.enter_onboard_choice(3).await?,
            _ => {}
        }
        Ok(())
    }

    async fn enter_onboard_choice(&mut self, idx: usize) -> Result<()> {
        match idx {
            0 => {
                self.cookie_form = Some(CookieForm::default());
                self.input_mode = InputMode::OnboardCookie;
            }
            1 => {
                self.up_form = Some(UpForm::default());
                self.input_mode = InputMode::OnboardUserPass;
            }
            2 => {
                self.paste_form = Some(PasteForm::default());
                self.input_mode = InputMode::OnboardPaste;
            }
            3 if self.has_env_creds => {
                self.import_env_creds().await?;
            }
            _ => {}
        }
        Ok(())
    }

    // ---- Pattern 1 ----

    async fn handle_cookie_key(&mut self, key: KeyEvent) -> Result<()> {
        let form = match &mut self.cookie_form {
            Some(f) => f,
            None => return Ok(()),
        };
        if form.busy {
            // Allow Esc to cancel while busy
            if key.code == KeyCode::Esc {
                form.busy = false;
                self.cookie_form = None;
                // Drop the in-flight bootstrap's id so its
                // ServiceAccountCreated event (if it still arrives) is
                // recognised as stale and ignored.
                self.pending_onboard_id = None;
                self.input_mode = InputMode::OnboardMenu;
            }
            return Ok(());
        }

        // Normalise the domain field whenever focus leaves it.
        let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
            && form.focused == CookieField::Domain;
        if leaving_domain {
            let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
            form.domain.set(cleaned);
        }

        match key.code {
            KeyCode::Esc => {
                self.cookie_form = None;
                self.input_mode = InputMode::OnboardMenu;
            }
            KeyCode::Tab => form.focused = form.focused.next(),
            KeyCode::BackTab => form.focused = form.focused.prev(),
            KeyCode::Left if form.focused == CookieField::Theme => form.cycle_theme_backward(),
            KeyCode::Right if form.focused == CookieField::Theme => form.cycle_theme_forward(),
            KeyCode::Enter if form.focused == CookieField::Submit => {
                if let Err(e) = form.validate() {
                    form.error = Some(e);
                } else {
                    form.error = None;
                    self.start_cookie_bootstrap();
                }
            }
            KeyCode::Enter => form.focused = form.focused.next(),
            KeyCode::Backspace => {
                if let Some(f) = form.focused_field_mut() {
                    f.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(f) = form.focused_field_mut() {
                    f.push_char(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn start_cookie_bootstrap(&mut self) {
        let form = match &mut self.cookie_form {
            Some(f) => f,
            None => return,
        };
        form.busy = true;
        form.status = Some("Authenticating…".into());
        let name = form.name.trimmed().to_string();
        let base_url = form.normalised_base_url();
        let theme = form.theme;
        let cookie_name = form.cookie_name.trimmed().to_string();
        let cookie_value = form.cookie_value.trimmed().to_string();
        let tx = self.events.tx.clone();
        let onboard_id = uuid::Uuid::new_v4();
        self.pending_onboard_id = Some(onboard_id);

        tokio::spawn(async move {
            run_bootstrap_from_cookie(
                onboard_id, name, base_url, theme, cookie_name, cookie_value, tx,
            )
            .await;
        });
    }

    // ---- Pattern 2 ----

    async fn handle_up_key(&mut self, key: KeyEvent) -> Result<()> {
        let form = match &mut self.up_form {
            Some(f) => f,
            None => return Ok(()),
        };

        // OTP / extra prompt is in flight — only the prompt input listens.
        if form.pending_prompt.is_some() {
            match key.code {
                KeyCode::Esc => {
                    form.pending_prompt = None;
                    form.prompt_input.clear();
                    form.busy = false;
                    form.status = None;
                    self.pending_callback_body = None;
                }
                KeyCode::Enter => {
                    if !form.prompt_input.is_empty() {
                        let extra = form.prompt_input.clone();
                        form.prompt_input.clear();
                        form.pending_prompt = None;
                        form.status = Some("Continuing authentication…".into());
                        self.continue_up_with_extra(extra);
                    }
                }
                KeyCode::Backspace => {
                    form.prompt_input.pop();
                }
                KeyCode::Char(c) => {
                    form.prompt_input.push(c);
                }
                _ => {}
            }
            return Ok(());
        }

        if form.busy {
            if key.code == KeyCode::Esc {
                form.busy = false;
                form.status = None;
                self.up_form = None;
                self.pending_onboard_id = None;
                self.input_mode = InputMode::OnboardMenu;
            }
            return Ok(());
        }

        let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
            && form.focused == UpField::Domain;
        if leaving_domain {
            let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
            form.domain.set(cleaned);
        }

        match key.code {
            KeyCode::Esc => {
                self.up_form = None;
                self.input_mode = InputMode::OnboardMenu;
            }
            KeyCode::Tab => form.focused = form.focused.next(),
            KeyCode::BackTab => form.focused = form.focused.prev(),
            KeyCode::Left if form.focused == UpField::Theme => form.cycle_theme_backward(),
            KeyCode::Right if form.focused == UpField::Theme => form.cycle_theme_forward(),
            KeyCode::Enter if form.focused == UpField::Submit => {
                if let Err(e) = form.validate() {
                    form.error = Some(e);
                } else {
                    form.error = None;
                    self.start_up_bootstrap();
                }
            }
            KeyCode::Enter => form.focused = form.focused.next(),
            KeyCode::Backspace => {
                if let Some(f) = form.focused_field_mut() {
                    f.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(f) = form.focused_field_mut() {
                    f.push_char(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn start_up_bootstrap(&mut self) {
        let form = match &mut self.up_form {
            Some(f) => f,
            None => return,
        };
        form.busy = true;
        form.status = Some("Starting authentication journey…".into());
        let name = form.name.trimmed().to_string();
        let base_url = form.normalised_base_url();
        let theme = form.theme;
        let username = form.username.trimmed().to_string();
        let password = form.password.value.clone();
        let realm_path = form.realm_path();
        let tx = self.events.tx.clone();
        let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let onboard_id = uuid::Uuid::new_v4();
        self.pending_onboard_id = Some(onboard_id);

        tokio::spawn(async move {
            run_bootstrap_from_userpass(
                onboard_id, name, base_url, theme, realm_path, username, password, None, None,
                scopes, tx,
            )
            .await;
        });
    }

    fn continue_up_with_extra(&mut self, extra: String) {
        let body = match self.pending_callback_body.take() {
            Some(b) => b,
            None => return,
        };
        let form = match &mut self.up_form {
            Some(f) => f,
            None => return,
        };
        let name = form.name.trimmed().to_string();
        let base_url = form.normalised_base_url();
        let theme = form.theme;
        let username = form.username.trimmed().to_string();
        let password = form.password.value.clone();
        let realm_path = form.realm_path();
        let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let tx = self.events.tx.clone();
        // Re-use the existing onboard id — this is a continuation of the same
        // user-initiated bootstrap. If the user cancelled and the id is gone,
        // there's nothing to continue.
        let Some(onboard_id) = self.pending_onboard_id else { return };
        tokio::spawn(async move {
            run_bootstrap_from_userpass(
                onboard_id,
                name,
                base_url,
                theme,
                realm_path,
                username,
                password,
                Some(body),
                Some(extra),
                scopes,
                tx,
            )
            .await;
        });
    }

    fn handle_auth_progress(&mut self, body: serde_json::Value, prompt: String) {
        if let Some(form) = &mut self.up_form {
            form.pending_prompt = Some(prompt);
            form.status = None;
        }
        self.pending_callback_body = Some(body);
    }

    fn handle_onboard_error(&mut self, msg: String) {
        if let Some(form) = &mut self.cookie_form {
            form.busy = false;
            form.error = Some(msg.clone());
            form.status = None;
        }
        if let Some(form) = &mut self.up_form {
            form.busy = false;
            form.error = Some(msg.clone());
            form.status = None;
            form.pending_prompt = None;
        }
        self.push_toast(ToastKind::Error, msg);
    }

    // ---- Pattern 3 ----

    async fn handle_paste_key(&mut self, key: KeyEvent) -> Result<()> {
        let form = match &mut self.paste_form {
            Some(f) => f,
            None => return Ok(()),
        };

        let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
            && form.focused == PasteField::Domain;
        if leaving_domain {
            let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
            form.domain.set(cleaned);
        }

        match key.code {
            KeyCode::Esc => {
                self.paste_form = None;
                self.input_mode = InputMode::OnboardMenu;
            }
            KeyCode::Tab => form.focused = form.focused.next(),
            KeyCode::BackTab => form.focused = form.focused.prev(),
            KeyCode::Left if form.focused == PasteField::Theme => form.cycle_theme_backward(),
            KeyCode::Right if form.focused == PasteField::Theme => form.cycle_theme_forward(),
            KeyCode::Enter if form.focused == PasteField::Submit => {
                let jwk = match form.validate() {
                    Ok(v) => v,
                    Err(e) => {
                        form.error = Some(e);
                        return Ok(());
                    }
                };
                let tenant = form.into_tenant();
                let prod = tenant.theme == TenantTheme::Production;
                self.paste_form = None;
                if prod {
                    self.pending_prod_action = Some(PendingProdAction::SaveTenant { tenant, jwk });
                    self.input_mode = InputMode::ProdConfirm;
                } else {
                    match self.persist_new_tenant(tenant, jwk) {
                        Ok(()) => self.push_toast(ToastKind::Success, "Tenant added!"),
                        Err(e) => self.push_toast(ToastKind::Error, format!("Save failed: {e}")),
                    }
                    self.input_mode = InputMode::Normal;
                }
            }
            KeyCode::Enter if form.is_jwk_field() => {
                form.jwk_input.push_newline();
            }
            KeyCode::Enter => form.focused = form.focused.next(),
            KeyCode::Backspace => {
                if let Some(f) = form.focused_field_mut() {
                    f.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(f) = form.focused_field_mut() {
                    f.push_char(c);
                }
            }
            _ => {}
        }
        Ok(())
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
            _ => {}
        }
    }

    async fn handle_prod_confirm_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let action = self.pending_prod_action.take();
                self.input_mode = InputMode::Normal;
                if let Some(action) = action {
                    match action {
                        PendingProdAction::SaveTenant { tenant, jwk } => {
                            match self.persist_new_tenant(tenant, jwk) {
                                Ok(()) => self.push_toast(ToastKind::Success, "Tenant added!"),
                                Err(e) => {
                                    self.push_toast(ToastKind::Error, format!("Save failed: {e}"))
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.pending_prod_action = None;
                self.input_mode = InputMode::Normal;
                self.push_toast(ToastKind::Info, "Prod write cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_sa_created(
        &mut self,
        onboard_id: uuid::Uuid,
        tenant_name: String,
        base_url: String,
        theme: TenantTheme,
        sa_id: String,
        jwk: serde_json::Value,
    ) -> Result<()> {
        // Drop the event if it doesn't match the bootstrap we're waiting on.
        // Covers: user cancelled before the task finished (id was cleared),
        // user cancelled and started a new bootstrap (id was replaced),
        // late completion that arrived after a successful different flow.
        if self.pending_onboard_id != Some(onboard_id) {
            tracing::debug!(
                event_id = %onboard_id,
                pending = ?self.pending_onboard_id,
                "dropping stale ServiceAccountCreated"
            );
            return Ok(());
        }
        self.pending_onboard_id = None;

        let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        // tenant.sa_id is the IDM service-account UUID (used as JWT iss/sub).
        // It is distinct from the JWK's kid (used as the JWS header kid). They
        // happen to coincide for SAs created by frodo-cli but not for SAs we
        // bootstrap here.
        let tenant = Tenant {
            name: tenant_name,
            base_url,
            theme,
            sa_id,
            scopes,
        };

        // Clear in-flight forms.
        self.cookie_form = None;
        self.up_form = None;
        self.pending_callback_body = None;

        if tenant.theme == TenantTheme::Production {
            self.pending_prod_action = Some(PendingProdAction::SaveTenant { tenant, jwk });
            self.input_mode = InputMode::ProdConfirm;
            return Ok(());
        }

        match self.persist_new_tenant(tenant, jwk) {
            Ok(()) => {
                self.push_toast(ToastKind::Success, "Tenant added!");
                self.input_mode = InputMode::Normal;
            }
            Err(e) => {
                self.push_toast(ToastKind::Error, format!("Save failed: {e}"));
                self.input_mode = InputMode::Normal;
            }
        }
        Ok(())
    }

    async fn import_env_creds(&mut self) -> Result<()> {
        let base_url = std::env::var("TENANT_BASE_URL")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();
        let sa_id = std::env::var("SERVICE_ACCOUNT_ID").unwrap_or_default();
        let jwk_str = std::env::var("SERVICE_ACCOUNT_KEY").unwrap_or_default();

        if base_url.is_empty() || sa_id.is_empty() || jwk_str.is_empty() {
            self.push_toast(
                ToastKind::Error,
                "Missing env vars — need TENANT_BASE_URL, SERVICE_ACCOUNT_ID, SERVICE_ACCOUNT_KEY",
            );
            self.input_mode = InputMode::Normal;
            return Ok(());
        }

        let jwk: serde_json::Value = match serde_json::from_str(&jwk_str) {
            Ok(v) => v,
            Err(e) => {
                self.push_toast(ToastKind::Error, format!("JWK parse error: {e}"));
                self.input_mode = InputMode::Normal;
                return Ok(());
            }
        };

        let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let tenant = Tenant {
            name: "sandbox".into(),
            base_url,
            theme: TenantTheme::Sandbox,
            sa_id,
            scopes,
        };

        match self.persist_new_tenant(tenant, jwk) {
            Ok(()) => {
                self.push_toast(ToastKind::Success, "Imported sandbox tenant from environment");
                self.input_mode = InputMode::Normal;
            }
            Err(e) => {
                self.push_toast(ToastKind::Error, format!("Import failed: {e}"));
                self.input_mode = InputMode::Normal;
            }
        }
        Ok(())
    }
}

// ---- Background bootstrap tasks ----

async fn run_bootstrap_from_cookie(
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    cookie_name: String,
    session_value: String,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    use crate::aic::onboard::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("HTTP client init: {e}")));
            return;
        }
    };
    let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &session_value).await {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("authorize/token: {e}")));
            return;
        }
    };
    let kid = uuid::Uuid::new_v4().to_string();
    let priv_jwk = match generate_rsa_jwk(&kid) {
        Ok(j) => j,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("RSA keygen: {e}")));
            return;
        }
    };
    let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
    let sa_name = format!("aic-edit-{tenant_name}");
    let sa_id = match create_service_account(
        &http,
        &base_url,
        &bearer,
        &sa_name,
        &format!("Created by aic-edit for {tenant_name}"),
        &pub_jwk,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("SA create: {e}")));
            return;
        }
    };
    // NOTE: do NOT overwrite priv_jwk["kid"] with sa_id — the kid must match
    // the one we registered in the SA's JWKS or AM rejects the signature.
    let _ = tx.send(AppEvent::ServiceAccountCreated {
        onboard_id,
        tenant_name,
        base_url,
        theme,
        sa_id,
        jwk: priv_jwk,
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_bootstrap_from_userpass(
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    realm_path: String,
    username: String,
    password: String,
    resume_body: Option<serde_json::Value>,
    extra: Option<String>,
    _scopes: Vec<String>,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    use crate::aic::onboard::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("HTTP client init: {e}")));
            return;
        }
    };
    let auth_url = format!("{base_url}/am/json{realm_path}/authenticate");

    // Initial round or resumed round.
    let mut body = match resume_body {
        Some(b) => b,
        None => {
            // AIC's load balancer (ALB) rejects POSTs with no `Content-Length`
            // header → HTTP 411. `curl -X POST` adds `Content-Length: 0`
            // automatically; reqwest+hyper does not, even with `.body("")`.
            // Send `{}` instead — AM ignores body content on the first round,
            // and we get a deterministic `Content-Length: 2`.
            let resp = match http
                .post(&auth_url)
                .header("Accept-API-Version", "resource=2.0, protocol=1.0")
                .header("Content-Type", "application/json")
                .body("{}")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("authenticate: {e}")));
                    return;
                }
            };
            if !resp.status().is_success() {
                let _ = tx.send(AppEvent::OnboardError(format!(
                    "authenticate: HTTP {}",
                    resp.status()
                )));
                return;
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("authenticate body: {e}")));
                    return;
                }
            }
        }
    };

    let mut current_extra = extra;
    for _round in 0..6 {
        if let Some(token_id) = body.get("tokenId").and_then(|v| v.as_str()) {
            // We have a session — proceed to bootstrap.
            let token_id = token_id.to_string();
            let cookie_name = match discover_cookie_name(&http, &base_url).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("serverinfo: {e}")));
                    return;
                }
            };
            let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &token_id).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("authorize/token: {e}")));
                    return;
                }
            };
            let kid = uuid::Uuid::new_v4().to_string();
            let priv_jwk = match generate_rsa_jwk(&kid) {
                Ok(j) => j,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("RSA keygen: {e}")));
                    return;
                }
            };
            let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
            let sa_name = format!("aic-edit-{tenant_name}");
            let sa_id = match create_service_account(
                &http,
                &base_url,
                &bearer,
                &sa_name,
                &format!("Created by aic-edit for {tenant_name}"),
                &pub_jwk,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("SA create: {e}")));
                    return;
                }
            };
            // NOTE: do NOT overwrite priv_jwk["kid"] with sa_id — the kid must
            // match the one we registered in the SA's JWKS or AM rejects the
            // signature.
            let _ = tx.send(AppEvent::ServiceAccountCreated {
                onboard_id,
                tenant_name,
                base_url,
                theme,
                sa_id,
                jwk: priv_jwk,
            });
            return;
        }

        let outcome = crate::aic::onboard::userpass::walk_with_extra(
            &body,
            &username,
            &password,
            current_extra.as_deref(),
        );
        current_extra = None;
        match outcome {
            CallbackOutcome::Ready(filled) => {
                let resp = match http
                    .post(&auth_url)
                    .header("Accept-API-Version", "resource=2.0, protocol=1.0")
                    .header("Content-Type", "application/json")
                    .json(&filled)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(AppEvent::OnboardError(format!("authenticate POST: {e}")));
                        return;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let txt = resp.text().await.unwrap_or_default();
                    let _ = tx.send(AppEvent::OnboardError(format!(
                        "authentication failed ({status}): {txt}"
                    )));
                    return;
                }
                body = match resp.json::<serde_json::Value>().await {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(AppEvent::OnboardError(format!("authenticate body: {e}")));
                        return;
                    }
                };
            }
            CallbackOutcome::PromptRequired {
                prompt,
                body: pending,
            } => {
                let _ = tx.send(AppEvent::AuthCallbackProgress {
                    body: pending,
                    prompt,
                });
                return;
            }
            CallbackOutcome::Unsupported(msg) => {
                let _ = tx.send(AppEvent::OnboardError(msg));
                return;
            }
        }
    }

    let _ = tx.send(AppEvent::OnboardError(
        "too many authentication rounds — aborting".into(),
    ));
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

