//! Pattern 2 — in-app username/password against AM's authentication journey.
//!
//! aic-edit POSTs `/am/json/{realm}/authenticate` to start the realm's default
//! journey, walks the resulting callbacks, prompts the user for any extra steps
//! (TOTP via NameCallback with prompt "Enter verification code"), and ends with
//! a `tokenId` — at which point we reuse the bootstrap helpers like Pattern 1.
//!
//! Passkey / push (PollingWaitCallback) is intentionally unsupported — those
//! flows need a real browser. Pattern 1 is the answer in that case.

use crate::config::tenant::TenantTheme;
use crate::ui::widgets::text_field::{fields, TextField};

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
    PromptRequired { prompt: String, body: serde_json::Value },
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
pub fn walk(
    resp_body: &serde_json::Value,
    username: &str,
    password: &str,
) -> CallbackOutcome {
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
    let cbs = match body
        .get_mut("callbacks")
        .and_then(|v| v.as_array_mut())
    {
        Some(c) => c,
        None => return CallbackOutcome::Unsupported("response has no callbacks".into()),
    };

    let mut name_used = false;
    for cb in cbs.iter_mut() {
        // Pre-extract all read-only fields before taking a mutable borrow of `input`.
        let ty = cb.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let outputs = cb.get("output").cloned();
        let prompt = outputs
            .as_ref()
            .and_then(|o| o.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("name").and_then(|n| n.as_str()) == Some("prompt") {
                        item.get("value").and_then(|v| v.as_str()).map(|s| s.to_string())
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
                        item.get("value")
                            .and_then(|v| v.as_array())
                            .map(|vs| {
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
                if !name_used && looks_like(&prompt, USER_HINTS) && !looks_like(&prompt, OTP_HINTS) {
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
