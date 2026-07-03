//! Pattern 2 — in-app username/password against AM's authentication journey.
//!
//! aic-edit POSTs `/am/json/{realm}/authenticate` to start the realm's default
//! journey, walks the resulting callbacks, prompts the user for any extra steps
//! (TOTP via NameCallback with prompt "Enter verification code"), and ends with
//! a `tokenId` — at which point we reuse the bootstrap helpers like Pattern 1.
//!
//! Passkey / push (PollingWaitCallback) is intentionally unsupported — those
//! flows need a real browser. Pattern 1 is the answer in that case.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::AppEvent;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::tui::widgets::text_field::{TextField, fields};

use super::common::{queue_overwrite_confirm, send_onboard_error, tenant_name_exists};
use super::screen::{Event, Mode, PendingConfirm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpField {
    Name,
    Domain,
    Theme,
    Username,
    Password,
    Submit,
}

impl UpField {
    pub const ORDER: [UpField; 6] = [
        UpField::Name,
        UpField::Domain,
        UpField::Theme,
        UpField::Username,
        UpField::Password,
        UpField::Submit,
    ];

    pub fn next(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug, Clone)]
pub struct UpForm {
    pub name: TextField,
    pub domain: TextField,
    pub theme: TenantTheme,
    pub theme_idx: usize,
    pub username: TextField,
    pub password: TextField,
    pub focused: UpField,
    pub error: Option<String>,
    pub busy: bool,
    pub status: Option<String>,

    /// If the journey requested an extra string from the user (typically a TOTP
    /// code), the prompt text is stashed here and the UI surfaces an inline input.
    pub pending_prompt: Option<String>,
    pub prompt_input: String,
}

impl Default for UpForm {
    fn default() -> Self {
        Self {
            name: fields::tenant_name(),
            domain: fields::hostname(),
            theme: TenantTheme::Sandbox,
            theme_idx: 0,
            username: fields::username(),
            password: fields::password(),
            focused: UpField::Name,
            error: None,
            busy: false,
            status: None,
            pending_prompt: None,
            prompt_input: String::new(),
        }
    }
}

impl UpForm {
    pub fn focused_field_mut(&mut self) -> Option<&mut TextField> {
        match self.focused {
            UpField::Name => Some(&mut self.name),
            UpField::Domain => Some(&mut self.domain),
            UpField::Username => Some(&mut self.username),
            UpField::Password => Some(&mut self.password),
            UpField::Theme | UpField::Submit => None,
        }
    }

    pub fn cycle_theme_forward(&mut self) {
        let themes = TenantTheme::all();
        self.theme_idx = (self.theme_idx + 1) % themes.len();
        self.theme = themes[self.theme_idx];
    }

    pub fn cycle_theme_backward(&mut self) {
        let themes = TenantTheme::all();
        self.theme_idx = (self.theme_idx + themes.len() - 1) % themes.len();
        self.theme = themes[self.theme_idx];
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.name.is_empty() {
            return Err("Tenant name is required".into());
        }
        super::validate_domain(&self.domain.value)?;
        if self.username.is_empty() {
            return Err("Username is required".into());
        }
        if self.password.value.is_empty() {
            return Err("Password is required".into());
        }
        Ok(())
    }

    pub fn normalised_base_url(&self) -> String {
        super::domain_to_base_url(&self.domain.value)
    }

    /// Platform admins live in (and only in) the root realm — AIC blocks
    /// admin sign-in via alpha/bravo. Hard-coded.
    pub fn realm_path(&self) -> String {
        "/realms/root".to_string()
    }
}

/// Walk one callback round. Either fills callbacks in place from the form's
/// known fields, or returns a `PromptRequired` describing a question we need to
/// surface to the user before we can continue.
pub enum CallbackOutcome {
    /// Callbacks filled in; the JSON value is ready to POST back.
    Ready(serde_json::Value),
    /// The journey is asking for an additional input (e.g. TOTP). The UI should
    /// display `prompt`, collect a string, and call `walk_with_extra` to retry.
    PromptRequired {
        prompt: String,
        body: serde_json::Value,
    },
    /// Cannot continue from here (e.g. PollingWaitCallback for passkey/push).
    Unsupported(String),
}

const OTP_HINTS: &[&str] = &["otp", "code", "token", "verification", "verify"];
const USER_HINTS: &[&str] = &["user", "name", "email", "login"];
/// Choice options we never want to auto-select. After a wrong TOTP, AIC's
/// default Login journey offers a ChoiceCallback like
/// `["Try Again", "Use Recovery Code"]` — we always pick the retry side.
const RECOVERY_HINTS: &[&str] = &["recovery", "backup", "emergency", "alternate"];

fn looks_like(prompt: &str, hints: &[&str]) -> bool {
    let p = prompt.to_lowercase();
    hints.iter().any(|h| p.contains(h))
}

/// Fill the callbacks in `resp_body` using known credentials. If we hit a
/// callback that needs user input (TOTP), return `PromptRequired` so the
/// caller can collect the value and call `walk_with_extra`.
pub fn walk(resp_body: &serde_json::Value, username: &str, password: &str) -> CallbackOutcome {
    walk_with_extra(resp_body, username, password, None)
}

/// Same as `walk`, but uses `extra` for any input the form has already
/// collected from the user (a TOTP code). One round = one extra at most.
pub fn walk_with_extra(
    resp_body: &serde_json::Value,
    username: &str,
    password: &str,
    extra: Option<&str>,
) -> CallbackOutcome {
    let mut body = resp_body.clone();
    let cbs = match body.get_mut("callbacks").and_then(|v| v.as_array_mut()) {
        Some(c) => c,
        None => return CallbackOutcome::Unsupported("response has no callbacks".into()),
    };

    let mut name_used = false;
    for cb in cbs.iter_mut() {
        // Pre-extract all read-only fields before taking a mutable borrow of `input`.
        let ty = cb
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let outputs = cb.get("output").cloned();
        let prompt = outputs
            .as_ref()
            .and_then(|o| o.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("name").and_then(|n| n.as_str()) == Some("prompt") {
                        item.get("value")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();
        let default_idx = outputs
            .as_ref()
            .and_then(|o| o.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("name").and_then(|n| n.as_str()) == Some("defaultOption") {
                        item.get("value").and_then(|v| v.as_i64())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0);
        let choices: Vec<String> = outputs
            .as_ref()
            .and_then(|o| o.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("name").and_then(|n| n.as_str()) == Some("choices") {
                        item.get("value").and_then(|v| v.as_array()).map(|vs| {
                            vs.iter()
                                .map(|v| v.as_str().unwrap_or("").to_string())
                                .collect()
                        })
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();

        let inp = match cb.get_mut("input").and_then(|v| v.as_array_mut()) {
            Some(i) if !i.is_empty() => i,
            _ => continue,
        };

        match ty.as_str() {
            "NameCallback" => {
                if !name_used && looks_like(&prompt, USER_HINTS) && !looks_like(&prompt, OTP_HINTS)
                {
                    inp[0]["value"] = serde_json::Value::String(username.to_string());
                    name_used = true;
                } else if looks_like(&prompt, OTP_HINTS) {
                    match extra {
                        Some(v) => {
                            inp[0]["value"] = serde_json::Value::String(v.to_string());
                        }
                        None => {
                            return CallbackOutcome::PromptRequired {
                                prompt: if prompt.is_empty() {
                                    "Verification code".into()
                                } else {
                                    prompt
                                },
                                body: resp_body.clone(),
                            };
                        }
                    }
                } else {
                    // No hint either way — best effort: treat as username if we haven't used it.
                    if !name_used {
                        inp[0]["value"] = serde_json::Value::String(username.to_string());
                        name_used = true;
                    } else {
                        return CallbackOutcome::PromptRequired {
                            prompt: if prompt.is_empty() {
                                ty.clone()
                            } else {
                                prompt
                            },
                            body: resp_body.clone(),
                        };
                    }
                }
            }
            "PasswordCallback" => {
                if looks_like(&prompt, OTP_HINTS) {
                    match extra {
                        Some(v) => {
                            inp[0]["value"] = serde_json::Value::String(v.to_string());
                        }
                        None => {
                            return CallbackOutcome::PromptRequired {
                                prompt: if prompt.is_empty() {
                                    "Verification code".into()
                                } else {
                                    prompt
                                },
                                body: resp_body.clone(),
                            };
                        }
                    }
                } else {
                    inp[0]["value"] = serde_json::Value::String(password.to_string());
                }
            }
            "ConfirmationCallback" => {
                inp[0]["value"] = serde_json::Value::from(default_idx);
            }
            "ChoiceCallback" => {
                // Skip any "Use recovery code" / "backup code" option so the
                // journey loops back to the TOTP prompt for another try.
                // Falls back to defaultOption if nothing matches.
                let pick = choices
                    .iter()
                    .position(|c| !looks_like(c, RECOVERY_HINTS))
                    .map(|i| i as i64)
                    .unwrap_or(default_idx);
                inp[0]["value"] = serde_json::Value::from(pick);
            }
            "BooleanAttributeInputCallback" => {
                inp[0]["value"] = serde_json::Value::Bool(false);
            }
            "TextOutputCallback" | "HiddenValueCallback" => {
                // No input expected.
            }
            "PollingWaitCallback" => {
                return CallbackOutcome::Unsupported(format!(
                    "polling required ({prompt}) — use Pattern 1 (paste session cookie) for passkey/push flows"
                ));
            }
            other => {
                return CallbackOutcome::Unsupported(format!(
                    "unsupported callback type {other} (prompt={prompt})"
                ));
            }
        }
    }

    CallbackOutcome::Ready(body)
}

// ---- Key handling ----

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.up_form {
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
                app.onboard.pending_callback_body = None;
            }
            KeyCode::Enter if !form.prompt_input.is_empty() => {
                let extra = form.prompt_input.clone();
                form.prompt_input.clear();
                form.pending_prompt = None;
                form.status = Some("Continuing authentication…".into());
                continue_with_extra(app, extra);
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
            app.onboard.up_form = None;
            app.onboard.pending_id = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        return Ok(());
    }

    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == UpField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.up_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == UpField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == UpField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == UpField::Submit => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                let name = form.name.trimmed().to_string();
                form.error = None;
                if tenant_name_exists(&app.tenants, &name) {
                    queue_overwrite_confirm(app, PendingConfirm::UserPass);
                } else {
                    start_bootstrap(app);
                }
            }
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        _ => {
            if let Some(f) = form.focused_field_mut() {
                f.handle_key(&key);
            }
        }
    }
    Ok(())
}

/// Kick off the username/password bootstrap. Public so the overwrite-confirm
/// handler can resume it after the user confirms replacing a tenant.
pub(crate) fn start_bootstrap(app: &mut App) {
    let form = match &mut app.onboard.up_form {
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
    let tx = app.events.tx.clone();
    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let onboard_id = uuid::Uuid::new_v4();
    app.onboard.pending_id = Some(onboard_id);

    tokio::spawn(async move {
        run_bootstrap(
            onboard_id, name, base_url, theme, realm_path, username, password, None, None, scopes,
            tx,
        )
        .await;
    });
}

fn continue_with_extra(app: &mut App, extra: String) {
    let body = match app.onboard.pending_callback_body.take() {
        Some(b) => b,
        None => return,
    };
    let form = match &mut app.onboard.up_form {
        Some(f) => f,
        None => return,
    };
    let name = form.name.trimmed().to_string();
    let base_url = form.normalised_base_url();
    let theme = form.theme;
    let username = form.username.trimmed().to_string();
    let password = form.password.value.clone();
    let realm_path = form.realm_path();
    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tx = app.events.tx.clone();
    // Re-use the existing onboard id — this is a continuation of the same
    // user-initiated bootstrap. If the user cancelled and the id is gone,
    // there's nothing to continue.
    let Some(onboard_id) = app.onboard.pending_id else {
        return;
    };
    tokio::spawn(async move {
        run_bootstrap(
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

// ---- Background bootstrap ----

#[allow(clippy::too_many_arguments)]
async fn run_bootstrap(
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
    use super::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("HTTP client init: {e}"));
            return;
        }
    };
    let auth_url = format!("{base_url}/am/json{realm_path}/authenticate");

    let mut body = match resume_body {
        Some(b) => b,
        None => {
            // AIC's load balancer (ALB) rejects POSTs with no
            // `Content-Length` header → HTTP 411. `curl -X POST` adds
            // `Content-Length: 0` automatically; reqwest+hyper does not,
            // even with `.body("")`. Send `{}` instead — AM ignores body
            // content on the first round, and we get a deterministic
            // `Content-Length: 2`.
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
                    send_onboard_error(&tx, onboard_id, format!("authenticate: {e}"));
                    return;
                }
            };
            if !resp.status().is_success() {
                send_onboard_error(
                    &tx,
                    onboard_id,
                    format!("authenticate: HTTP {}", resp.status()),
                );
                return;
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("authenticate body: {e}"));
                    return;
                }
            }
        }
    };

    let mut current_extra = extra;
    for _round in 0..6 {
        if let Some(token_id) = body.get("tokenId").and_then(|v| v.as_str()) {
            let token_id = token_id.to_string();
            let cookie_name = match discover_cookie_name(&http, &base_url).await {
                Ok(n) => n,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("serverinfo: {e}"));
                    return;
                }
            };
            let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &token_id).await {
                Ok(bearer) => bearer,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("authorize/token: {e}"));
                    return;
                }
            };
            let minted = match mint_log_key_from_bearer(
                &http,
                &base_url,
                &tenant_name,
                &bearer,
                Some(username.as_str()),
            )
            .await
            {
                Ok(minted) => minted,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("log API key create: {e}"));
                    return;
                }
            };
            let kid = uuid::Uuid::new_v4().to_string();
            let priv_jwk = match generate_rsa_jwk(&kid) {
                Ok(j) => j,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("RSA keygen: {e}"));
                    return;
                }
            };
            let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
            let sa_id = match create_service_account(
                &http,
                &base_url,
                &bearer,
                &minted.credential_name,
                &format!("Created by aic-edit for {tenant_name}"),
                &pub_jwk,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("SA create: {e}"));
                    return;
                }
            };
            let log_key = Some(minted.key);
            let _ = tx.send(AppEvent::Onboard(Event::ServiceAccountReady {
                onboard_id,
                tenant_name,
                base_url,
                theme,
                sa_id,
                jwk: priv_jwk,
                log_key,
            }));
            return;
        }

        let outcome = walk_with_extra(&body, &username, &password, current_extra.as_deref());
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
                        send_onboard_error(&tx, onboard_id, format!("authenticate POST: {e}"));
                        return;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let txt = resp.text().await.unwrap_or_default();
                    send_onboard_error(
                        &tx,
                        onboard_id,
                        format!("authentication failed ({status}): {txt}"),
                    );
                    return;
                }
                body = match resp.json::<serde_json::Value>().await {
                    Ok(v) => v,
                    Err(e) => {
                        send_onboard_error(&tx, onboard_id, format!("authenticate body: {e}"));
                        return;
                    }
                };
            }
            CallbackOutcome::PromptRequired {
                prompt,
                body: pending,
            } => {
                let _ = tx.send(AppEvent::Onboard(Event::AuthProgress {
                    onboard_id,
                    body: pending,
                    prompt,
                }));
                return;
            }
            CallbackOutcome::Unsupported(msg) => {
                send_onboard_error(&tx, onboard_id, msg);
                return;
            }
        }
    }

    send_onboard_error(&tx, onboard_id, "too many authentication rounds — aborting");
}
