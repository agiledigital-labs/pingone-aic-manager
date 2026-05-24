use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::time::{interval, Duration};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{CallbackOutcome, UpField, UpForm};
use crate::agent::{AgentClient, Request as AgentRequest, Response as AgentResponse};
use crate::aic::AicClient;
use crate::config::crypto::{self, Dek};
use crate::config::tenant::{Tenant, TenantTheme};
use crate::config::wraps::{self, Wrap, WrapsFile};
use crate::config::{ProjectConfig, Settings};
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
}

/// Which input on the Unlock screen currently has focus. Only meaningful when
/// a security key wrap exists; otherwise the Master password is the only field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnlockFocus {
    SecurityKeyPin,
    Password,
}

/// Auth methods offered on the first-run picker (and on the in-app "add
/// factor" flow). "None" = `keys.plain`, no DEK; the other two share the
/// envelope-encryption scheme — random DEK wrapped per-method in
/// `wraps.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    None,
    Password,
    SecurityKey,
}

impl AuthMethod {
    pub const ORDER: [AuthMethod; 3] = [
        AuthMethod::None,
        AuthMethod::Password,
        AuthMethod::SecurityKey,
    ];

    pub fn next(self) -> AuthMethod {
        let i = Self::ORDER.iter().position(|m| *m == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> AuthMethod {
        let i = Self::ORDER.iter().position(|m| *m == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::None => "None",
            AuthMethod::Password => "Master password",
            AuthMethod::SecurityKey => "Security key",
        }
    }
}

/// Fields on the first-run / add-factor setup form. Only some of these are
/// visible per `AuthMethod`; navigation skips the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSetupField {
    Method,
    Password,
    Confirm,
    Pin,
    Label,
    Submit,
}

#[derive(Debug, Clone)]
pub struct AuthSetupForm {
    pub method: AuthMethod,
    pub password: String,
    pub confirm: String,
    pub pin: String,
    pub label: String,
    pub focused: AuthSetupField,
    pub error: Option<String>,
    /// True while a blocking enrol task is in flight. Locks out input and
    /// shows a "Tap your security key…" hint.
    pub busy: bool,
}

impl Default for AuthSetupForm {
    fn default() -> Self {
        Self {
            method: AuthMethod::Password,
            password: String::new(),
            confirm: String::new(),
            pin: String::new(),
            label: "Security key 1".into(),
            focused: AuthSetupField::Method,
            error: None,
            busy: false,
        }
    }
}

impl AuthSetupForm {
    /// Field order for the current method, used by Tab/BackTab. Always starts
    /// with `Method` so the user can cycle back to switch.
    fn order(&self) -> &'static [AuthSetupField] {
        match self.method {
            AuthMethod::None => &[AuthSetupField::Method, AuthSetupField::Submit],
            AuthMethod::Password => &[
                AuthSetupField::Method,
                AuthSetupField::Password,
                AuthSetupField::Confirm,
                AuthSetupField::Submit,
            ],
            AuthMethod::SecurityKey => &[
                AuthSetupField::Method,
                AuthSetupField::Pin,
                AuthSetupField::Label,
                AuthSetupField::Submit,
            ],
        }
    }

    pub fn next(&mut self) {
        let order = self.order();
        let i = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(i + 1) % order.len()];
    }

    pub fn prev(&mut self) {
        let order = self.order();
        let i = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(i + order.len() - 1) % order.len()];
    }

    /// After switching the radio, keep focus on Method. The user can ←/→
    /// through the options to compare them, then Tab into the body when
    /// they're ready. Jumping ahead is surprising — especially for None,
    /// where the next field is Submit and Shift-Tab back is non-obvious.
    pub fn settle_focus_after_method_change(&mut self) {
        self.focused = AuthSetupField::Method;
    }
}

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

    // Unlock screen state
    pub unlock_input: String,
    pub unlock_error: Option<String>,
    pub unlock_busy: bool,
    /// PIN typed for the security key field. Held in memory only while the unlock
    /// screen is open; consumed by the security key poll task.
    pub unlock_pin_input: String,
    /// Which input on the Unlock screen has focus. Defaults to the security key
    /// PIN field when a security key is enrolled, otherwise irrelevant.
    pub unlock_focus: UnlockFocus,


    // First-run auth-setup form (also used by the "add factor" flow from
    // Auth Settings).
    pub setup_form: AuthSetupForm,
    /// Whether the SetupAuth screen is being shown for first-run or for the
    /// "add factor" entry from Auth Settings. Decides where we return to on
    /// submit + whether the None radio option is offered.
    pub setup_context: SetupContext,

    // Auth Settings screen state
    pub auth_settings_idx: usize,
    /// Buffer for the rename popup (Auth Settings → `r`).
    pub rename_input: String,
    /// Pending destructive action awaiting y/n confirmation.
    pending_auth_action: Option<PendingAuthAction>,

    /// Data encryption key (random 32 bytes), held only while unlocked.
    /// `None` either means "not yet unlocked" or "user opted out of
    /// encryption" (see `settings.encrypt_keys`). The DEK is wrapped on disk
    /// by every enrolled unlock method (`wraps.toml`).
    dek: Option<Dek>,

    /// Loaded wrap envelope, kept in memory so the unlock screen can decide
    /// which methods to offer and the enrolment flow can append new entries.
    pub wraps: WrapsFile,

    /// Set by the background security key poll to stop itself once unlock has
    /// happened (via any method). Shared with the spawned task.
    security_key_cancel: Arc<AtomicBool>,
    /// True while a security key poll task is running — guards against spawning
    /// more than one.
    security_key_armed: bool,

    // JWK map (decrypted; keyed by tenant name)
    jwks: HashMap<String, serde_json::Value>,

    // AIC clients (keyed by tenant name)
    clients: HashMap<String, AicClient>,

    // Onboard state
    pub onboard_menu_idx: usize,
    pub cookie_form: Option<CookieForm>,
    pub up_form: Option<UpForm>,
    pub paste_form: Option<PasteForm>,

    // For Pattern 2: the in-flight callback JSON we POST'd that needs an extra prompt
    pending_callback_body: Option<serde_json::Value>,

    // Prod confirm: pending action after confirmation
    pending_prod_action: Option<PendingProdAction>,

    // Overwrite confirm: a pending tenant whose name collides with an existing one
    pending_overwrite: Option<(Tenant, serde_json::Value)>,
}

enum PendingProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: serde_json::Value,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupContext {
    /// Initial install: no settings.toml on disk. Picker is full 3-way
    /// (None / Password / security key) and submit returns to Normal.
    FirstRun,
    /// "Add factor" from Auth Settings: picker hides None (you can't downgrade
    /// to no-encryption by adding a factor), and submit returns to
    /// AuthSettings.
    AddFactor,
}

#[derive(Debug)]
enum PendingAuthAction {
    /// Remove the wrap at this index from `wraps.toml`. Last-factor cases
    /// transition to `DisableEncryption` before reaching this state, so
    /// the index is guaranteed not to be the only wrap.
    RemoveWrap(usize),
    /// Decrypt `keys.enc` → `keys.plain` and delete all wraps. Triggered
    /// either by [x] or by attempting to remove the last wrap.
    DisableEncryption,
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
            active_tenant_idx: 0,
            env_picker_idx: 0,
            toasts: VecDeque::new(),
            should_quit: false,
            has_env_creds,
            unlock_input: String::new(),
            unlock_error: None,
            unlock_busy: false,
            unlock_pin_input: String::new(),
            unlock_focus: UnlockFocus::SecurityKeyPin,
            setup_form: AuthSetupForm::default(),
            setup_context: SetupContext::FirstRun,
            auth_settings_idx: 0,
            rename_input: String::new(),
            pending_auth_action: None,
            dek: None,
            wraps,
            security_key_cancel: Arc::new(AtomicBool::new(false)),
            security_key_armed: false,
            jwks: HashMap::new(),
            clients: HashMap::new(),
            onboard_menu_idx: 0,
            cookie_form: None,
            up_form: None,
            paste_form: None,
            pending_callback_body: None,
            pending_prod_action: None,
            pending_overwrite: None,
        })
    }

    /// If the agent is already holding the DEK from a previous TUI session,
    /// fetch it and hydrate `self.dek` + `self.jwks` so `decide_initial_mode`
    /// can skip the Unlock screen. Best-effort: any failure (agent missing,
    /// stale socket, locked, decrypt-error) silently falls through to the
    /// normal unlock path.
    async fn try_agent_unlock(&mut self) {
        // Only meaningful when there's an encrypted blob to unlock.
        if !matches!(self.settings, Some(Settings { encrypt_keys: true, .. })) {
            return;
        }
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine as _;

        let client = match AgentClient::connect_or_spawn().await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("agent unavailable on startup: {e}");
                return;
            }
        };
        let dek_b64 = match client.send(&AgentRequest::GetDek).await {
            Ok(AgentResponse::Dek { dek_b64 }) => dek_b64,
            Ok(AgentResponse::Locked) => return,
            Ok(other) => {
                tracing::debug!("unexpected GetDek reply: {other:?}");
                return;
            }
            Err(e) => {
                tracing::debug!("GetDek failed: {e}");
                return;
            }
        };
        let bytes = match B64.decode(&dek_b64) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("agent returned non-base64 DEK: {e}");
                return;
            }
        };
        let arr: [u8; 32] = match bytes.as_slice().try_into() {
            Ok(a) => a,
            Err(_) => {
                tracing::warn!("agent returned DEK of wrong length");
                return;
            }
        };
        let dek = Dek::from_bytes(arr);
        let jwks = match crate::config::decrypt_keys_file(&dek) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("agent DEK failed to decrypt keys.enc: {e}");
                return;
            }
        };
        self.dek = Some(dek);
        self.jwks = jwks;
    }

    /// Fire-and-forget: cache the just-derived DEK in the agent so subsequent
    /// TUI launches (within the idle window) skip the Unlock screen.
    fn put_dek_to_agent(&self) {
        let Some(dek) = &self.dek else { return };
        let dek_bytes = *dek.as_bytes();
        tokio::spawn(async move {
            use base64::engine::general_purpose::STANDARD as B64;
            use base64::Engine as _;
            let dek_b64 = B64.encode(dek_bytes);
            match AgentClient::connect_or_spawn().await {
                Ok(c) => {
                    if let Err(e) = c.send(&AgentRequest::PutDek { dek_b64 }).await {
                        tracing::warn!("PutDek failed: {e}");
                    }
                }
                Err(e) => tracing::debug!("agent unavailable for PutDek: {e}"),
            }
        });
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
                self.unlock_focus = if self.wraps.has_security_key() {
                    UnlockFocus::SecurityKeyPin
                } else {
                    UnlockFocus::Password
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

    /// Spawn the security key background poll for the Unlock screen. `pin` is the
    /// FIDO2 PIN the user just typed; it's required for every assertion.
    fn spawn_security_key_poll(&mut self, pin: String) {
        if self.security_key_armed {
            return;
        }
        self.security_key_armed = true;
        self.security_key_cancel.store(false, Ordering::Relaxed);
        let wraps = self.wraps.clone();
        let tx = self.events.tx.clone();
        let cancel = self.security_key_cancel.clone();
        tokio::task::spawn_blocking(move || {
            let pin_opt = if pin.is_empty() { None } else { Some(pin.as_str()) };
            'outer: loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if !crate::security_key::device_present() {
                    // No security key plugged in; poll again in a moment.
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                for wrap in wraps.security_key_wraps() {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    match crate::config::unlock_with_security_key(wrap, pin_opt) {
                        Ok((dek, jwks)) => {
                            let _ = tx.send(AppEvent::UnlockResult(Ok(UnlockOk {
                                dek,
                                jwks,
                            })));
                            return;
                        }
                        Err(e) => {
                            // Surface PIN-related errors so the user knows
                            // to try again rather than waiting silently.
                            let msg = e.to_string();
                            let fatal = msg.contains("PIN_REQUIRED")
                                || msg.contains("PIN_INVALID")
                                || msg.contains("PIN_AUTH_INVALID")
                                || msg.contains("PIN_BLOCKED");
                            tracing::debug!("security key unlock attempt failed: {msg}");
                            if fatal {
                                let _ = tx.send(AppEvent::UnlockResult(Err(format!(
                                    "security key: {msg}"
                                ))));
                                return;
                            }
                        }
                    }
                    if cancel.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                }
                // None of the enrolled credentials matched this device.
                // Wait a bit before trying again — gives the user time to
                // swap security_keys or type their password.
                std::thread::sleep(Duration::from_secs(2));
            }
        });
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
    fn persist_keys(&self) -> Result<()> {
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
        self.save_jwk(&tenant.name, jwk.clone())?;

        let client = AicClient::new(tenant.clone(), jwk.clone());
        let tx = self.events.tx.clone();
        let token_cache = client.token_cache.clone();
        AicClient::spawn_mint_token(tenant.clone(), jwk, token_cache, tx);
        self.clients.insert(tenant.name.clone(), client);

        // Replace any existing entry with the same name, or append.
        if let Some(idx) = self.tenants.iter().position(|t| t.name == tenant.name) {
            self.tenants[idx] = tenant.clone();
            self.active_tenant_idx = idx;
        } else {
            self.tenants.push(tenant.clone());
            self.active_tenant_idx = self.tenants.len() - 1;
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

    fn push_toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toasts.push_front(Toast::new(kind, message.into()));
        if self.toasts.len() > 5 {
            self.toasts.pop_back();
        }
    }

    pub fn active_tenant(&self) -> Option<&Tenant> {
        self.tenants.get(self.active_tenant_idx)
    }

    /// True iff the in-memory DEK is set — meaning credentials are encrypted
    /// and the user is currently unlocked. The header uses this to decide
    /// whether to surface security key-enrol shortcuts.
    pub fn dek_is_set(&self) -> bool {
        self.dek.is_some()
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        self.try_agent_unlock().await;
        self.decide_initial_mode();
        self.init_clients();

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

    fn init_clients(&mut self) {
        for tenant in self.tenants.clone() {
            if let Some(jwk) = self.jwks.get(&tenant.name).cloned() {
                let client = AicClient::new(tenant.clone(), jwk.clone());
                let tx = self.events.tx.clone();
                let token_cache = client.token_cache.clone();
                AicClient::spawn_mint_token(tenant.clone(), jwk, token_cache, tx);
                self.clients.insert(tenant.name.clone(), client);
            }
        }
    }

    pub async fn handle_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::Key(key) => self.handle_key(key).await?,
            AppEvent::Tick => self.tick(),
            AppEvent::TokenMinted { tenant, expires_at } => {
                self.push_toast(ToastKind::Success, format!("Token ready: {tenant}"));
                tracing::info!("Token minted for {tenant}, expires {expires_at}");
            }
            AppEvent::TokenError { tenant, error } => {
                self.push_toast(ToastKind::Error, format!("Token error ({tenant}): {error}"));
            }
            AppEvent::ServiceAccountCreated {
                tenant_name,
                sa_id,
                jwk,
            } => {
                self.handle_sa_created(tenant_name, sa_id, jwk)?;
            }
            AppEvent::Toast(kind, msg) => {
                self.push_toast(kind, msg);
            }
            AppEvent::AuthCallbackProgress { body, prompt } => {
                self.handle_auth_progress(body, prompt);
            }
            AppEvent::OnboardError(msg) => {
                tracing::error!(error = %msg, "onboard error");
                self.handle_onboard_error(msg);
            }
            AppEvent::UnlockResult(r) => self.handle_unlock_result(r),
            AppEvent::SecurityKeyEnrollResult(r) => self.handle_security_key_enroll_result(r),
            AppEvent::OnboardCallback(_) | AppEvent::ApiResponse { .. } => {}
        }
        Ok(())
    }

    fn handle_security_key_enroll_result(
        &mut self,
        result: std::result::Result<Wrap, String>,
    ) {
        // We're always reached from SetupAuth (either first-run or add-factor),
        // because the obsolete Ctrl-Y modal has been removed. The two cases
        // diverge in `finalize_factor_addition` based on `setup_context`.
        let context = self.setup_context;
        let was_dek_minted_for_this_op = !matches!(
            self.settings,
            Some(Settings { encrypt_keys: true, .. })
        );

        match result {
            Ok(wrap) => {
                self.wraps.push_security_key(wrap);
                if let Err(e) = self.wraps.save() {
                    tracing::error!(error = %e, "wraps.toml save failed after enrol");
                    self.push_toast(
                        ToastKind::Error,
                        format!("wraps.toml: {e}"),
                    );
                    self.wraps.wraps.pop();
                    if was_dek_minted_for_this_op {
                        self.dek = None;
                    }
                    self.setup_form.busy = false;
                    self.setup_form.error = Some(format!("Save failed: {e}"));
                    return;
                }

                self.setup_form.busy = false;
                if let Err(e) =
                    self.finalize_factor_addition(was_dek_minted_for_this_op, context, "Security key enrolled")
                {
                    tracing::error!(error = %e, "finalize_factor_addition failed");
                    self.setup_form.error = Some(format!("Finalise: {e}"));
                }
            }
            Err(msg) => {
                tracing::error!(error = %msg, "security key enrolment failed");
                // Reset so the user can try again. Drop the half-minted DEK
                // if it never made it onto disk.
                if was_dek_minted_for_this_op {
                    self.dek = None;
                }
                self.setup_form.busy = false;
                self.setup_form.error = Some(format!("Enrol failed: {msg}"));
            }
        }
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
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await?,
            InputMode::SetupAuth => self.handle_setup_auth_key(key)?,
            InputMode::Unlock => self.handle_unlock_key(key),
            InputMode::AuthSettings => self.handle_auth_settings_key(key)?,
            InputMode::AuthSettingsConfirm => self.handle_auth_settings_confirm_key(key)?,
            InputMode::AuthSettingsRename => self.handle_auth_settings_rename_key(key)?,
            InputMode::OnboardMenu => self.handle_onboard_menu_key(key).await?,
            InputMode::OnboardCookie => self.handle_cookie_key(key).await?,
            InputMode::OnboardUserPass => self.handle_up_key(key).await?,
            InputMode::OnboardPaste => self.handle_paste_key(key).await?,
            InputMode::OverwriteConfirm => self.handle_overwrite_key(key)?,
            InputMode::EnvPicker => self.handle_env_picker_key(key),
            InputMode::ProdConfirm => self.handle_prod_confirm_key(key).await?,
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
                self.open_auth_settings();
            }
            KeyCode::Char('L') => {
                self.lock_and_quit().await;
            }
            _ => {}
        }
        Ok(())
    }

    /// Tell the agent to drop the cached DEK, then quit the TUI. The next
    /// launch goes back through the Unlock screen.
    async fn lock_and_quit(&mut self) {
        self.dek = None;
        // Best-effort. If the agent isn't running there's nothing to lock;
        // we deliberately don't spawn one just to immediately lock it.
        if let Ok(c) = AgentClient::connect(crate::agent::socket_path()).await {
            let _ = c.send(&AgentRequest::Lock).await;
        }
        self.should_quit = true;
    }

    fn open_auth_settings(&mut self) {
        // Clamp the cursor to the current factor list so we never index past
        // the end (e.g. if a previous session removed the wrap that was
        // selected).
        let n = self.wraps.wraps.len();
        if n == 0 {
            self.auth_settings_idx = 0;
        } else if self.auth_settings_idx >= n {
            self.auth_settings_idx = n - 1;
        }
        self.input_mode = InputMode::AuthSettings;
    }

    fn handle_unlock_key(&mut self, key: KeyEvent) {
        // While the unlock task is running, ignore everything except Esc-to-quit.
        if self.unlock_busy {
            if key.code == KeyCode::Esc {
                self.should_quit = true;
            }
            return;
        }

        // Which methods are actually enrolled. Tab toggles only when both
        // are present; otherwise focus is pinned to whichever exists.
        let yk = self.wraps.has_security_key();
        let pw = self.wraps.has_password();
        let both = yk && pw;
        let on_pin =
            (yk && !pw) || (both && self.unlock_focus == UnlockFocus::SecurityKeyPin);

        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab | KeyCode::BackTab if both => {
                self.unlock_focus = match self.unlock_focus {
                    UnlockFocus::SecurityKeyPin => UnlockFocus::Password,
                    UnlockFocus::Password => UnlockFocus::SecurityKeyPin,
                };
            }
            KeyCode::Enter => {
                if on_pin {
                    if self.unlock_pin_input.is_empty() {
                        self.unlock_error = Some("Security key PIN cannot be empty".into());
                        return;
                    }
                    self.unlock_error = Some(crate::ui::unlock::TAP_MESSAGE.into());
                    let pin = std::mem::take(&mut self.unlock_pin_input);
                    self.spawn_security_key_poll(pin);
                } else {
                    if self.unlock_input.is_empty() {
                        self.unlock_error = Some("Password cannot be empty".into());
                        return;
                    }
                    let password = std::mem::take(&mut self.unlock_input);
                    self.unlock_error = None;
                    self.unlock_busy = true;
                    let tx = self.events.tx.clone();
                    let wraps = self.wraps.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = try_password_unlock(&password, &wraps)
                            .map(|(dek, jwks)| UnlockOk { dek, jwks })
                            .map_err(|e| format!("{e}"));
                        let _ = tx.send(AppEvent::UnlockResult(result));
                    });
                }
            }
            KeyCode::Backspace => {
                if on_pin {
                    self.unlock_pin_input.pop();
                } else {
                    self.unlock_input.pop();
                }
            }
            KeyCode::Char(c) => {
                if on_pin {
                    self.unlock_pin_input.push(c);
                } else {
                    self.unlock_input.push(c);
                }
            }
            _ => {}
        }
    }

    fn handle_unlock_result(&mut self, result: std::result::Result<UnlockOk, String>) {
        // A late-arriving second result (e.g. security key unlock fired after we
        // already accepted the password) — drop it.
        if self.input_mode != InputMode::Unlock {
            return;
        }
        self.unlock_busy = false;
        match result {
            Ok(UnlockOk { dek, jwks }) => {
                self.dek = Some(dek);
                self.jwks = jwks;
                self.unlock_error = None;
                self.unlock_pin_input.clear();
                self.input_mode = InputMode::Normal;
                // Tell the security key poll task to stop (if it was running).
                self.security_key_cancel.store(true, Ordering::Relaxed);
                self.security_key_armed = false;
                self.init_clients();
                self.put_dek_to_agent();
            }
            Err(e) => {
                // A security key failure shouldn't take the screen down — let the
                // user retry the tap or fall back to the password field.
                self.security_key_cancel.store(true, Ordering::Relaxed);
                self.security_key_armed = false;
                self.unlock_error = Some(format!("Unlock failed: {e}"));
            }
        }
    }

    fn handle_setup_auth_key(&mut self, key: KeyEvent) -> Result<()> {
        // While a security key enrol task is running the form is read-only except
        // for Esc-to-quit (the blocking task can't be cancelled mid-touch).
        if self.setup_form.busy {
            if key.code == KeyCode::Esc {
                // Can't cancel an in-flight enrol cleanly — just refuse.
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => match self.setup_context {
                SetupContext::FirstRun => self.should_quit = true,
                SetupContext::AddFactor => {
                    self.setup_form = AuthSetupForm::default();
                    self.setup_context = SetupContext::FirstRun;
                    self.input_mode = InputMode::AuthSettings;
                }
            },
            KeyCode::Tab => self.setup_form.next(),
            KeyCode::BackTab => self.setup_form.prev(),
            KeyCode::Left if self.setup_form.focused == AuthSetupField::Method => {
                self.setup_form.method = step_method_prev(self.setup_form.method, self.setup_context);
                self.setup_form.error = None;
                self.setup_form.settle_focus_after_method_change();
            }
            KeyCode::Right if self.setup_form.focused == AuthSetupField::Method => {
                self.setup_form.method = step_method_next(self.setup_form.method, self.setup_context);
                self.setup_form.error = None;
                self.setup_form.settle_focus_after_method_change();
            }
            KeyCode::Char(' ') if self.setup_form.focused == AuthSetupField::Method => {
                self.setup_form.method = step_method_next(self.setup_form.method, self.setup_context);
                self.setup_form.error = None;
                self.setup_form.settle_focus_after_method_change();
            }
            KeyCode::Enter if self.setup_form.focused == AuthSetupField::Submit => {
                self.commit_setup_auth()?;
            }
            KeyCode::Enter => self.setup_form.next(),
            KeyCode::Backspace => match self.setup_form.focused {
                AuthSetupField::Password => {
                    self.setup_form.password.pop();
                }
                AuthSetupField::Confirm => {
                    self.setup_form.confirm.pop();
                }
                AuthSetupField::Pin => {
                    self.setup_form.pin.pop();
                }
                AuthSetupField::Label => {
                    self.setup_form.label.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) => match self.setup_form.focused {
                AuthSetupField::Password => self.setup_form.password.push(c),
                AuthSetupField::Confirm => self.setup_form.confirm.push(c),
                AuthSetupField::Pin => self.setup_form.pin.push(c),
                AuthSetupField::Label => self.setup_form.label.push(c),
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn commit_setup_auth(&mut self) -> Result<()> {
        let context = self.setup_context;
        match self.setup_form.method {
            AuthMethod::None => {
                // None is only valid from first-run. Defensive guard.
                if context != SetupContext::FirstRun {
                    self.setup_form.error = Some(
                        "Use [x] disable encryption from Auth Settings instead".into(),
                    );
                    return Ok(());
                }
                // No encryption: write an empty keys.plain at mode 600 and
                // record settings. No DEK; no wraps.toml.
                self.dek = None;
                let mut s = self.settings.unwrap_or_default();
                s.encrypt_keys = false;
                self.settings = Some(s);
                s.save()?;
                let bytes = serde_json::to_vec(&self.jwks)?;
                ProjectConfig::save_keys_plain(&bytes)?;
                self.setup_form = AuthSetupForm::default();
                self.setup_context = SetupContext::FirstRun;
                self.input_mode = InputMode::Normal;
            }
            AuthMethod::Password => {
                if self.setup_form.password.is_empty() {
                    self.setup_form.error = Some("Password cannot be empty".into());
                    self.setup_form.focused = AuthSetupField::Password;
                    return Ok(());
                }
                if self.setup_form.password != self.setup_form.confirm {
                    self.setup_form.error = Some("Passwords do not match".into());
                    self.setup_form.focused = AuthSetupField::Confirm;
                    return Ok(());
                }

                // Mint a DEK iff there isn't one yet. Re-using an existing
                // DEK is what makes "change password" work without touching
                // keys.enc.
                let freshly_minted = self.dek.is_none();
                let dek = self.dek.clone().unwrap_or_else(Dek::random);
                let (salt, nonce, ct) =
                    crypto::wrap_dek_with_password(&dek, &self.setup_form.password)?;
                self.wraps.upsert_password(Wrap::Password {
                    salt: wraps::b64_encode(&salt),
                    nonce: wraps::b64_encode(&nonce),
                    ciphertext: wraps::b64_encode(&ct),
                });
                self.wraps.save()?;
                self.dek = Some(dek);
                self.finalize_factor_addition(freshly_minted, context, "Password set")?;
            }
            AuthMethod::SecurityKey => {
                if self.setup_form.pin.is_empty() {
                    self.setup_form.error = Some("FIDO2 PIN cannot be empty".into());
                    self.setup_form.focused = AuthSetupField::Pin;
                    return Ok(());
                }
                if self.setup_form.label.trim().is_empty() {
                    self.setup_form.error = Some("Label cannot be empty".into());
                    self.setup_form.focused = AuthSetupField::Label;
                    return Ok(());
                }
                let dek = self.dek.clone().unwrap_or_else(Dek::random);
                self.dek = Some(dek.clone());
                self.setup_form.busy = true;
                self.setup_form.error = None;
                let pin = std::mem::take(&mut self.setup_form.pin);
                let label = self.setup_form.label.trim().to_string();
                let tx = self.events.tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = enroll_security_key_blocking(dek, label, Some(pin));
                    let _ = tx.send(AppEvent::SecurityKeyEnrollResult(result));
                });
            }
        }
        Ok(())
    }

    /// Shared "after a wrap was just persisted" tail. Handles the encryption
    /// transition (keys.plain → keys.enc) when this is the first factor, and
    /// routes back to whichever screen launched SetupAuth.
    fn finalize_factor_addition(
        &mut self,
        freshly_minted_dek: bool,
        context: SetupContext,
        success_toast: &str,
    ) -> Result<()> {
        let already_encrypted =
            matches!(self.settings, Some(Settings { encrypt_keys: true, .. }));

        if !already_encrypted {
            // We just added the first factor while encryption was disabled
            // (or this is first-run with no prior settings.toml). Promote
            // keys.plain → keys.enc and flip the flag.
            let dek = self
                .dek
                .as_ref()
                .ok_or_else(|| crate::Error::Crypto("DEK missing".into()))?;
            crate::config::enable_encryption(dek)?;
            let mut s = self.settings.unwrap_or_default();
            s.encrypt_keys = true;
            self.settings = Some(s);
        } else if freshly_minted_dek {
            // Defensive: should not happen — if encryption was already on,
            // we have a DEK and didn't mint a fresh one. But if the state
            // ever got out of sync, persist the new DEK so keys.enc is
            // readable.
            self.persist_keys()?;
        }
        // Else (already encrypted + DEK unchanged): wraps.toml already saved
        // by the caller, nothing else to do.

        self.setup_form = AuthSetupForm::default();
        let next_mode = match context {
            SetupContext::FirstRun => InputMode::Normal,
            SetupContext::AddFactor => InputMode::AuthSettings,
        };
        self.setup_context = SetupContext::FirstRun;
        self.input_mode = next_mode;
        self.push_toast(ToastKind::Success, success_toast);
        Ok(())
    }

    fn handle_auth_settings_key(&mut self, key: KeyEvent) -> Result<()> {
        let n = self.wraps.wraps.len();
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.auth_settings_idx > 0 {
                    self.auth_settings_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 && self.auth_settings_idx + 1 < n {
                    self.auth_settings_idx += 1;
                }
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.start_add_factor(AuthMethod::Password);
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.start_add_factor(AuthMethod::SecurityKey);
            }
            KeyCode::Char('d') | KeyCode::Char('D') if n > 0 => {
                // Last-factor guard — falls through to disable-encryption.
                if n == 1 {
                    self.pending_auth_action = Some(PendingAuthAction::DisableEncryption);
                } else {
                    self.pending_auth_action =
                        Some(PendingAuthAction::RemoveWrap(self.auth_settings_idx));
                }
                self.input_mode = InputMode::AuthSettingsConfirm;
            }
            KeyCode::Char('r') | KeyCode::Char('R') if n > 0 => {
                // Only security-key wraps carry a user-editable label; the
                // password row is always "Master password".
                if let Some(Wrap::SecurityKey { label, .. }) =
                    self.wraps.wraps.get(self.auth_settings_idx)
                {
                    self.rename_input = label.clone().unwrap_or_default();
                    self.input_mode = InputMode::AuthSettingsRename;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_auth_settings_rename_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.rename_input.clear();
                self.input_mode = InputMode::AuthSettings;
            }
            KeyCode::Enter => {
                let new_label = self.rename_input.trim().to_string();
                if new_label.is_empty() {
                    // Silently refuse — Esc cancels, Enter requires a label.
                    return Ok(());
                }
                if let Some(Wrap::SecurityKey { label, .. }) =
                    self.wraps.wraps.get_mut(self.auth_settings_idx)
                {
                    *label = Some(new_label);
                    self.wraps.save()?;
                    self.push_toast(ToastKind::Success, "Renamed");
                }
                self.rename_input.clear();
                self.input_mode = InputMode::AuthSettings;
            }
            KeyCode::Backspace => {
                self.rename_input.pop();
            }
            KeyCode::Char(c) => {
                self.rename_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the SetupAuth form pre-set for "add factor" mode.
    fn start_add_factor(&mut self, method: AuthMethod) {
        self.setup_form = AuthSetupForm::default();
        self.setup_form.method = method;
        self.setup_form.focused = match method {
            AuthMethod::Password => AuthSetupField::Password,
            AuthMethod::SecurityKey => AuthSetupField::Pin,
            AuthMethod::None => AuthSetupField::Method,
        };
        if method == AuthMethod::SecurityKey {
            self.setup_form.label = format!(
                "Security key {}",
                self.wraps.security_key_wraps().count() + 1
            );
        }
        self.setup_context = SetupContext::AddFactor;
        self.input_mode = InputMode::SetupAuth;
    }

    fn handle_auth_settings_confirm_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let action = self.pending_auth_action.take();
                match action {
                    Some(PendingAuthAction::RemoveWrap(idx)) => {
                        if idx < self.wraps.wraps.len() {
                            self.wraps.wraps.remove(idx);
                            self.wraps.save()?;
                            if self.auth_settings_idx >= self.wraps.wraps.len()
                                && !self.wraps.wraps.is_empty()
                            {
                                self.auth_settings_idx = self.wraps.wraps.len() - 1;
                            }
                            self.push_toast(ToastKind::Success, "Factor removed");
                        }
                    }
                    Some(PendingAuthAction::DisableEncryption) => {
                        if let Some(dek) = self.dek.clone() {
                            crate::config::disable_encryption(&dek)?;
                            self.dek = None;
                            self.wraps = WrapsFile::default();
                            let mut s = self.settings.unwrap_or_default();
                            s.encrypt_keys = false;
                            self.settings = Some(s);
                            self.auth_settings_idx = 0;
                            self.push_toast(
                                ToastKind::Info,
                                "Encryption disabled — credentials at keys.plain",
                            );
                        } else {
                            self.push_toast(
                                ToastKind::Error,
                                "Cannot disable: not unlocked",
                            );
                        }
                    }
                    None => {}
                }
                self.input_mode = InputMode::AuthSettings;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.pending_auth_action = None;
                self.input_mode = InputMode::AuthSettings;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn pending_auth_action_label(&self) -> Option<String> {
        match &self.pending_auth_action {
            Some(PendingAuthAction::RemoveWrap(idx)) => self
                .wraps
                .wraps
                .get(*idx)
                .map(|w| format!("Remove factor: {}?", w.label())),
            Some(PendingAuthAction::DisableEncryption) => Some(
                "Disable encryption? Credentials will be written to keys.plain.".into(),
            ),
            None => None,
        }
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

        tokio::spawn(async move {
            run_bootstrap_from_cookie(name, base_url, theme, cookie_name, cookie_value, tx).await;
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

        tokio::spawn(async move {
            run_bootstrap_from_userpass(
                name, base_url, theme, realm_path, username, password, None, None, scopes, tx,
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
        tokio::spawn(async move {
            run_bootstrap_from_userpass(
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
                self.active_tenant_idx = self.env_picker_idx;
                self.input_mode = InputMode::Normal;
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
        tenant_name: String,
        sa_id: String,
        jwk: serde_json::Value,
    ) -> Result<()> {
        // Build the Tenant from whichever form is active.
        let (base_url, theme) = if let Some(form) = &self.cookie_form {
            (form.normalised_base_url(), form.theme)
        } else if let Some(form) = &self.up_form {
            (form.normalised_base_url(), form.theme)
        } else {
            (String::new(), TenantTheme::Sandbox)
        };

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
    tenant_name: String,
    base_url: String,
    _theme: TenantTheme,
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
        tenant_name,
        sa_id,
        jwk: priv_jwk,
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_bootstrap_from_userpass(
    tenant_name: String,
    base_url: String,
    _theme: TenantTheme,
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
                tenant_name,
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

/// Step the method radio left/right, skipping `None` when we're inside the
/// "add factor" flow from Auth Settings (you can't degrade encryption by
/// adding a factor).
fn step_method_next(current: AuthMethod, context: SetupContext) -> AuthMethod {
    let next = current.next();
    if context == SetupContext::AddFactor && next == AuthMethod::None {
        next.next()
    } else {
        next
    }
}

fn step_method_prev(current: AuthMethod, context: SetupContext) -> AuthMethod {
    let prev = current.prev();
    if context == SetupContext::AddFactor && prev == AuthMethod::None {
        prev.prev()
    } else {
        prev
    }
}

/// Payload returned by a successful background unlock task.
#[derive(Debug)]
pub struct UnlockOk {
    pub dek: Dek,
    pub jwks: HashMap<String, serde_json::Value>,
}

/// Thin wrapper around `config::unlock_with_password` so the spawn_blocking
/// closure stays self-contained (no `&self.wraps` borrow across threads).
fn try_password_unlock(
    password: &str,
    _wraps: &WrapsFile,
) -> Result<(Dek, HashMap<String, serde_json::Value>)> {
    crate::config::unlock_with_password(password)
}

/// Enrol a security key and produce a wrap entry that the event handler can
/// append to `wraps.toml`. Blocks until the user taps the device.
fn enroll_security_key_blocking(
    dek: Dek,
    label: String,
    pin: Option<String>,
) -> std::result::Result<Wrap, String> {
    let enrolment = crate::security_key::enroll(pin.as_deref()).map_err(|e| e.to_string())?;
    let (nonce, ct) =
        crypto::wrap_dek_with_kek(&dek, &enrolment.hmac).map_err(|e| e.to_string())?;
    Ok(Wrap::SecurityKey {
        label: Some(label),
        credential_id: wraps::b64_encode(&enrolment.credential_id),
        rp_id: crate::security_key::RP_ID.to_string(),
        hmac_salt: wraps::b64_encode(&enrolment.hmac_salt),
        nonce: wraps::b64_encode(&nonce),
        ciphertext: wraps::b64_encode(&ct),
    })
}
