use std::collections::{HashMap, VecDeque};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::time::{Duration, interval};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{CallbackOutcome, UpField, UpForm};
use crate::aic::AicClient;
use crate::config::crypto;
use crate::config::tenant::{Tenant, TenantTheme};
use crate::config::ProjectConfig;
use crate::event::{AppEvent, EventHandler, ToastKind};
use crate::ui::toast::Toast;
use crate::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    Normal,
    Unlock,
    OnboardMenu,
    OnboardCookie,
    OnboardUserPass,
    OnboardPaste,
    OverwriteConfirm,
    EnvPicker,
    ProdConfirm,
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
    pub input_mode: InputMode,
    pub current_tab: Tab,
    pub current_realm: Realm,
    pub tenants: Vec<Tenant>,
    pub active_tenant_idx: usize,
    pub env_picker_idx: usize,
    pub toasts: VecDeque<Toast>,
    pub should_quit: bool,
    pub has_envrc: bool,

    // Unlock screen state
    pub unlock_input: String,
    pub unlock_error: Option<String>,
    pub unlock_busy: bool,
    /// True when there is no `keys.enc` yet — the unlock screen treats the
    /// next submission as "set the password" rather than "unlock".
    first_run: bool,

    // Master password (kept in memory after unlock; None means "no master password set")
    master_password: Option<String>,

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

impl App {
    pub fn new() -> Result<Self> {
        let config = ProjectConfig::load()?;
        let tenants = config
            .as_ref()
            .map(|c| c.tenants.clone())
            .unwrap_or_default();

        let has_envrc = std::path::Path::new(".envrc").exists()
            && std::fs::read_to_string(".envrc")
                .unwrap_or_default()
                .contains("SERVICE_ACCOUNT_KEY");

        Ok(Self {
            events: EventHandler::new(),
            config,
            input_mode: InputMode::Normal,
            current_tab: Tab::Esvs,
            current_realm: Realm::Alpha,
            tenants,
            active_tenant_idx: 0,
            env_picker_idx: 0,
            toasts: VecDeque::new(),
            should_quit: false,
            has_envrc,
            unlock_input: String::new(),
            unlock_error: None,
            unlock_busy: false,
            first_run: false,
            master_password: None,
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

    /// Pick the initial mode. `keys.enc` exists iff a master password has been
    /// set, so:
    ///   - exists → Unlock (with possible keychain auto-unlock).
    ///   - missing → first run; the Unlock screen treats the next submission
    ///     as "set the password" and writes a verifier `keys.enc` afterwards.
    fn decide_initial_mode(&mut self) {
        if ProjectConfig::keys_path().exists() {
            self.first_run = false;
            self.try_keychain_unlock();
        } else {
            self.first_run = true;
            self.input_mode = InputMode::Unlock;
        }
    }

    pub fn is_first_run(&self) -> bool {
        self.first_run
    }

    /// Try to unlock from OS keychain; fall back to the Unlock screen if needed.
    fn try_keychain_unlock(&mut self) {
        let project_key = ProjectConfig::project_key();
        let raw_key = match crate::keychain::load_key(&project_key) {
            Ok(Some(k)) => k,
            _ => {
                self.input_mode = InputMode::Unlock;
                return;
            }
        };
        let password = match String::from_utf8(raw_key) {
            Ok(p) => p,
            Err(_) => {
                self.input_mode = InputMode::Unlock;
                return;
            }
        };
        // Try the cached password synchronously. If it works we go straight to
        // Normal; otherwise the user types it themselves.
        match try_decrypt_keys(&password) {
            Ok(Some(map)) => {
                self.jwks = map;
                self.master_password = Some(password);
                self.input_mode = InputMode::Normal;
            }
            _ => {
                self.input_mode = InputMode::Unlock;
            }
        }
    }

    fn save_jwk(&mut self, tenant_name: &str, jwk: serde_json::Value) -> Result<()> {
        self.jwks.insert(tenant_name.to_string(), jwk);
        self.persist_keys_enc()
    }

    /// Save a tenant outright — replacing any existing entry with the same
    /// name. Caller is responsible for confirming the overwrite before calling.
    fn persist_tenant_overwriting(
        &mut self,
        tenant: Tenant,
        jwk: serde_json::Value,
    ) -> Result<()> {
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

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
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
            AppEvent::OnboardCallback(_) | AppEvent::ApiResponse { .. } => {}
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
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await?,
            InputMode::Unlock => self.handle_unlock_key(key),
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
        self.pending_overwrite.as_ref().map(|(t, _)| t.name.as_str())
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
            _ => {}
        }
        Ok(())
    }

    fn handle_unlock_key(&mut self, key: KeyEvent) {
        // While the unlock task is running, ignore everything except Esc-to-quit.
        if self.unlock_busy {
            if key.code == KeyCode::Esc {
                self.should_quit = true;
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Enter => {
                if self.unlock_input.is_empty() {
                    self.unlock_error = Some("Password cannot be empty".into());
                    return;
                }
                let password = std::mem::take(&mut self.unlock_input);
                self.unlock_error = None;
                self.unlock_busy = true;
                let tx = self.events.tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = try_decrypt_keys(&password).map(|opt| {
                        // First run: no keys.enc yet → start with empty map.
                        (password.clone(), opt.unwrap_or_default())
                    });
                    let _ = tx.send(AppEvent::UnlockResult(
                        result.map_err(|e| format!("{e}")),
                    ));
                });
            }
            KeyCode::Backspace => {
                self.unlock_input.pop();
            }
            KeyCode::Char(c) => {
                self.unlock_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_unlock_result(
        &mut self,
        result: std::result::Result<(String, HashMap<String, serde_json::Value>), String>,
    ) {
        self.unlock_busy = false;
        match result {
            Ok((password, jwks)) => {
                self.jwks = jwks;
                self.master_password = Some(password.clone());
                self.unlock_error = None;
                self.input_mode = InputMode::Normal;

                // First run: persist an encrypted (empty) map so the password
                // is validated on subsequent launches.
                if self.first_run {
                    if let Err(e) = self.persist_keys_enc() {
                        tracing::error!(error = %e, "first-run verifier persist failed");
                        self.push_toast(
                            ToastKind::Warning,
                            format!("Couldn't write verifier: {e}"),
                        );
                    }
                    self.first_run = false;
                }

                // Best-effort keychain stash so future launches auto-unlock.
                let _ = crate::keychain::store_key(
                    &ProjectConfig::project_key(),
                    password.as_bytes(),
                );
                self.init_clients();
            }
            Err(e) => {
                self.unlock_error = Some(format!("Unlock failed: {e}"));
            }
        }
    }

    /// Write the current (possibly empty) JWK map encrypted with the master
    /// password. Used on first run to create the password verifier, and via
    /// `save_jwk` for normal tenant writes.
    fn persist_keys_enc(&self) -> Result<()> {
        let password = self
            .master_password
            .as_deref()
            .ok_or_else(|| Error::Crypto("no master password set".into()))?;
        let bytes = serde_json::to_vec(&self.jwks)?;
        let enc = crypto::encrypt(&bytes, password)?;
        ProjectConfig::save_keys_enc(&enc)?;
        Ok(())
    }

    async fn handle_onboard_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        let max_idx = if self.has_envrc { 3 } else { 2 };
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
            KeyCode::Char('4') if self.has_envrc => self.enter_onboard_choice(3).await?,
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
            3 if self.has_envrc => {
                self.import_envrc().await?;
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
        let leaving_domain =
            matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
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
            run_bootstrap_from_cookie(
                name,
                base_url,
                theme,
                cookie_name,
                cookie_value,
                tx,
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
                self.input_mode = InputMode::OnboardMenu;
            }
            return Ok(());
        }

        let leaving_domain =
            matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
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
                name,
                base_url,
                theme,
                realm_path,
                username,
                password,
                None,
                None,
                scopes,
                tx,
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

        let leaving_domain =
            matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
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
                let prod = tenant.theme == TenantTheme::Prod;
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

        if tenant.theme == TenantTheme::Prod {
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

    async fn import_envrc(&mut self) -> Result<()> {
        let content = std::fs::read_to_string(".envrc")?;
        let mut map: HashMap<String, String> = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            let rest = line.strip_prefix("export ").unwrap_or(line);
            if let Some(eq) = rest.find('=') {
                let key = rest[..eq].trim().to_string();
                let val = rest[eq + 1..]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                map.insert(key, val);
            }
        }

        let base_url = map
            .get("TENANT_BASE_URL")
            .cloned()
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();
        let sa_id = map.get("SERVICE_ACCOUNT_ID").cloned().unwrap_or_default();
        let jwk_str = map.get("SERVICE_ACCOUNT_KEY").cloned().unwrap_or_default();

        if base_url.is_empty() || sa_id.is_empty() || jwk_str.is_empty() {
            self.push_toast(
                ToastKind::Error,
                "Could not parse .envrc — expected TENANT_BASE_URL, SERVICE_ACCOUNT_ID, SERVICE_ACCOUNT_KEY",
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
                self.push_toast(ToastKind::Success, "Imported sandbox tenant from .envrc");
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
            let resp = match http
                .post(&auth_url)
                .header("Accept-API-Version", "resource=2.0, protocol=1.0")
                .header("Content-Type", "application/json")
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
            CallbackOutcome::PromptRequired { prompt, body: pending } => {
                let _ = tx.send(AppEvent::AuthCallbackProgress { body: pending, prompt });
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

/// Try to decrypt the on-disk keys file with the given password. Returns
/// `Ok(None)` if no file exists yet (first run), `Ok(Some(map))` on success,
/// or `Err` on decryption / parse failure.
fn try_decrypt_keys(password: &str) -> Result<Option<HashMap<String, serde_json::Value>>> {
    let enc = match ProjectConfig::load_keys_enc()? {
        Some(d) => d,
        None => return Ok(None),
    };
    let plaintext = crypto::decrypt(&enc, password)?;
    let map: HashMap<String, serde_json::Value> = serde_json::from_slice(&plaintext)?;
    Ok(Some(map))
}
