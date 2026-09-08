//! Pre-push syntax checking for script-like resources.
//!
//! Neither script write path parses its source: `PUT …/scripts/{id}` and
//! `PUT /openidm/config/endpoint/{name}` both store an unparseable script with
//! a 201. Each family instead exposes an opt-in action that the write path
//! never calls, and the console does not gate saving on it either — so a typo
//! is stored and surfaces far from the edit. An AM script fails whenever the
//! journey or token flow referencing it next evaluates; a broken IDM endpoint
//! is worse, because its runtime URL answers **404** while its config object
//! reads back 200, presenting as an endpoint that was never created.
//!
//! Ground truth: `docs/api/04-scripts.md` and `docs/api/11-idm-endpoints.md`
//! ("Syntax validation" in each), verified 2026-09-09.
//!
//! The two actions disagree in every way that matters, which is why the
//! verdict-parsing lives here as pure functions over captured wire bodies
//! rather than inline at the call sites:
//!
//! | | AM `?_action=validate` | IDM `script?_action=compile` |
//! |-|-|-|
//! | pass | `200 {"success":true}` | `200 true` (a bare JSON bool) |
//! | fail | **`200`** `{"success":false,"errors":[…]}` | `400 {"message":"…"}` |
//! | line/col | ✅ both languages | ❌ JavaScript, ✅ Groovy (inside the message) |
//!
//! Two traps follow. AM answers **200 on failure**, so a caller reading the
//! status code sees every script as valid. And the engines are not the same ES
//! level — AM rejects `let` and destructuring that IDM compiles — so routing
//! IDM source through AM to borrow its line numbers reports failures IDM
//! accepts. Each family uses its own action; there is no shared endpoint.

use crate::{Error, Result};
use serde_json::{Value, json};

/// The verdict of a pre-push syntax check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxCheck {
    /// The tenant parsed the source.
    Ok,
    /// The tenant refused it. Always at least one error.
    Invalid(Vec<SyntaxError>),
    /// The check could not produce a verdict. Carries the reason for the
    /// caller to surface. **Not** a failure: the push proceeds, because the
    /// write path never checked syntax to begin with and blocking on an
    /// unanswerable pre-flight would be a regression against no pre-flight.
    Skipped(String),
}

/// One parse error. `line`/`column` are 1-based and index the *decoded*
/// source, so they land on the workspace file directly — when present at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl SyntaxError {
    /// `line:col: message`, degrading to just the message when the tenant did
    /// not say where. IDM never says where for JavaScript.
    pub fn render(&self) -> String {
        match (self.line, self.column) {
            (Some(l), Some(c)) => format!("{l}:{c}: {}", self.message),
            (Some(l), None) => format!("{l}: {}", self.message),
            _ => self.message.clone(),
        }
    }
}

impl SyntaxCheck {
    pub fn is_invalid(&self) -> bool {
        matches!(self, SyntaxCheck::Invalid(_))
    }
}

// ---------------------------------------------------------------------------
// AM — POST /am/json{realm}/scripts/?_action=validate
// ---------------------------------------------------------------------------

/// Parse an AM `?_action=validate` body.
///
/// The status code is **not** the verdict — AM returns 200 for a script that
/// does not parse — so this reads `success` and nothing else.
pub fn parse_am_validate(body: &Value) -> SyntaxCheck {
    match body.get("success").and_then(Value::as_bool) {
        Some(true) => SyntaxCheck::Ok,
        Some(false) => {
            let errors: Vec<SyntaxError> = body
                .get("errors")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(am_error).collect())
                .unwrap_or_default();
            if errors.is_empty() {
                // `success: false` with nothing to show. Still a refusal, so
                // do not let it read as Ok.
                SyntaxCheck::Invalid(vec![SyntaxError {
                    line: None,
                    column: None,
                    message: "script rejected, no detail given".into(),
                }])
            } else {
                SyntaxCheck::Invalid(errors)
            }
        }
        None => SyntaxCheck::Skipped(
            "AM validate returned no `success` field — treating as unchecked".into(),
        ),
    }
}

fn am_error(v: &Value) -> SyntaxError {
    SyntaxError {
        line: v.get("line").and_then(Value::as_u64).map(|n| n as u32),
        column: v.get("column").and_then(Value::as_u64).map(|n| n as u32),
        message: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("syntax error")
            .to_string(),
    }
}

/// Request body for the AM action. `script` is base64, as everywhere in the
/// AM script API; `name`, `context` and `_id` are all unnecessary.
pub fn am_validate_body(script_b64: &str, language: &str) -> Value {
    json!({ "script": script_b64, "language": language })
}

// ---------------------------------------------------------------------------
// IDM — POST /openidm/script?_action=compile
// ---------------------------------------------------------------------------

/// Request body for the IDM action.
pub fn idm_compile_body(source: &str, script_type: &str) -> Value {
    json!({ "type": script_type, "source": source })
}

/// Interpret the outcome of an IDM `script?_action=compile` call.
///
/// Takes the whole `Result` because for IDM the verdict is carried by the
/// status: success is `200 true`, a parse failure is a `400` and therefore
/// arrives as `Err(Error::Api { .. })`, not as a body to inspect.
pub fn parse_idm_compile(outcome: Result<Value>) -> Result<SyntaxCheck> {
    match outcome {
        Ok(Value::Bool(true)) => Ok(SyntaxCheck::Ok),
        Ok(other) => Ok(SyntaxCheck::Skipped(format!(
            "IDM compile returned {other} rather than `true` — treating as unchecked"
        ))),
        Err(Error::Api { status: 400, body }) => Ok(SyntaxCheck::Invalid(vec![idm_error(&body)])),
        // A 503 from this endpoint means the script `type` is not one the
        // engine recognises (`javascript`, `text/javascript`, `groovy` are the
        // accepted spellings) — verified 2026-09-09, deterministic across
        // repeats with a healthy call in between. It is not a transient, and
        // it is not a syntax verdict either: the resource already carries that
        // type, so warn and let the push through.
        Err(Error::Api { status: 503, .. }) => Ok(SyntaxCheck::Skipped(
            "IDM compile rejected the script `type` (503) — source left unchecked".into(),
        )),
        Err(e) => Err(e),
    }
}

/// Syntax-check an IDM script slot through `script?_action=compile`.
///
/// `slot` is the object carrying `source` and `type` — the whole config object
/// for an endpoint, a hook or a mapping script, and `invokeContext.script` for
/// a schedule. Locating it is the caller's (per-kind) job; everything from
/// there is identical across the IDM families, which all store plaintext
/// JavaScript in the same two keys.
pub async fn idm_check_slot(tenant: &str, slot: Option<&Value>) -> Result<SyntaxCheck> {
    let Some(source) = slot.and_then(|v| v.get("source")).and_then(Value::as_str) else {
        return Ok(SyntaxCheck::Skipped(
            "no plaintext `source` field to compile".into(),
        ));
    };
    // Forward the stored `type` rather than assuming: an unrecognised spelling
    // comes back as a 503 that `parse_idm_compile` turns into a warning, which
    // is more honest than silently checking it as something it is not.
    let script_type = slot
        .and_then(|v| v.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("text/javascript");
    let outcome = crate::aic::api::post(
        tenant,
        "/openidm/script?_action=compile",
        idm_compile_body(source, script_type),
        // A compile stores nothing — not a tenant write, so no production
        // confirmation is consumed. (`eval` would run the script; we never
        // call it.)
        true,
    )
    .await;
    parse_idm_compile(outcome)
}

/// Pull a `SyntaxError` out of an IDM 400 body.
///
/// For JavaScript the message is the bare Rhino string with no coordinates
/// anywhere — `"syntax error"` even when the fault is on line 40. Groovy is
/// the exception and formats line/column into the message text, so lift them
/// when they are there and leave them absent otherwise rather than inventing
/// a line 1 that would point at the wrong place.
fn idm_error(body: &str) -> SyntaxError {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.trim().to_string());
    let message = unescape_idm(&message);
    let (line, column) = groovy_position(&message);
    SyntaxError {
        line,
        column,
        message,
    }
}

/// `… @ line 2, column 8.` — the Groovy compiler's position suffix.
fn groovy_position(message: &str) -> (Option<u32>, Option<u32>) {
    let Some(at) = message.find("@ line ") else {
        return (None, None);
    };
    let rest = &message[at + "@ line ".len()..];
    let line = rest
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|d| d.parse::<u32>().ok());
    let column = rest.find("column ").and_then(|c| {
        rest[c + "column ".len()..]
            .split(|ch: char| !ch.is_ascii_digit())
            .next()
            .and_then(|d| d.parse::<u32>().ok())
    });
    (line, column)
}

/// IDM HTML-escapes every string in an error before it reaches the caller
/// (`docs/api/11-idm-endpoints.md`, Quirks), so a Groovy message arrives with
/// `&#39;x&#39;` where it meant `'x'`. Undo the entities that quirk names, or
/// the operator reads entities instead of their code.
fn unescape_idm(s: &str) -> String {
    s.replace("&#39;", "'")
        .replace("&#34;", "\"")
        .replace("&#96;", "`")
        .replace("&#61;", "=")
        // `@` is escaped too, and `groovy_position` looks for a literal
        // `@ line N` — so omitting this entity does not merely leave an
        // entity on screen, it loses the only coordinates IDM ever reports.
        .replace("&#64;", "@")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        // `&amp;` last: doing it first would re-expand the entities above.
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Every fixture below is a body captured live from the sandbox on 2026-09-09
// (see the `## Verified against` blocks in `docs/api/04-scripts.md` and
// `docs/api/11-idm-endpoints.md`), pasted verbatim rather than constructed —
// a hand-built body would only prove the parser agrees with my idea of the
// wire format, which is the thing under test.
#[cfg(test)]
mod tests {
    use super::*;

    // -- AM ---------------------------------------------------------------

    #[test]
    fn am_success_is_ok() {
        let body: Value = serde_json::from_str(r#"{"success": true}"#).unwrap();
        assert_eq!(parse_am_validate(&body), SyntaxCheck::Ok);
    }

    /// The load-bearing case. AM answers **HTTP 200** for a script that does
    /// not parse, so an implementation that read the status code would call
    /// this valid. Nothing but `success` distinguishes it.
    #[test]
    fn am_failure_arrives_under_a_200_and_is_invalid() {
        let body: Value = serde_json::from_str(
            r#"{"success":false,"errors":[{"line":3,"column":15,"message":"missing ) in parenthetical"}]}"#,
        )
        .unwrap();
        assert_eq!(
            parse_am_validate(&body),
            SyntaxCheck::Invalid(vec![SyntaxError {
                line: Some(3),
                column: Some(15),
                message: "missing ) in parenthetical".into(),
            }])
        );
    }

    #[test]
    fn am_reports_coordinates_for_groovy_too() {
        let body: Value = serde_json::from_str(
            r#"{"success":false,"errors":[{"line":2,"column":8,"message":"Unexpected input: 'x'"}]}"#,
        )
        .unwrap();
        let SyntaxCheck::Invalid(errs) = parse_am_validate(&body) else {
            panic!("expected Invalid");
        };
        assert_eq!(errs[0].render(), "2:8: Unexpected input: 'x'");
    }

    /// A refusal with an empty `errors` must not degrade to `Ok` — that would
    /// turn "rejected, no detail" into a green push.
    #[test]
    fn am_failure_without_errors_is_still_invalid() {
        let body: Value = serde_json::from_str(r#"{"success":false}"#).unwrap();
        assert!(parse_am_validate(&body).is_invalid());
    }

    #[test]
    fn am_missing_success_field_is_skipped_not_ok() {
        let body: Value = serde_json::from_str(r#"{"unexpected": 1}"#).unwrap();
        assert!(matches!(parse_am_validate(&body), SyntaxCheck::Skipped(_)));
    }

    // -- IDM --------------------------------------------------------------

    #[test]
    fn idm_bare_true_is_ok() {
        assert_eq!(
            parse_idm_compile(Ok(Value::Bool(true))).unwrap(),
            SyntaxCheck::Ok
        );
    }

    /// IDM's JavaScript failure carries **no coordinates at all** — this body
    /// came from a 40-line source whose only fault was on line 40. `line` must
    /// stay `None`; defaulting it to 1 would point the operator at the wrong
    /// line with full confidence.
    #[test]
    fn idm_javascript_failure_has_no_line_number() {
        let body = r#"{"code":400,"reason":"Bad Request","message":"syntax error"}"#;
        let check = parse_idm_compile(Err(Error::Api {
            status: 400,
            body: body.into(),
        }))
        .unwrap();
        assert_eq!(
            check,
            SyntaxCheck::Invalid(vec![SyntaxError {
                line: None,
                column: None,
                message: "syntax error".into(),
            }])
        );
        let SyntaxCheck::Invalid(errs) = check else {
            unreachable!()
        };
        assert_eq!(errs[0].render(), "syntax error");
    }

    /// Groovy is the exception: IDM formats the position into the message, and
    /// HTML-escapes the quotes around the offending token on the way out.
    #[test]
    fn idm_groovy_failure_yields_position_and_unescaped_text() {
        let body = r#"{"code":400,"reason":"Bad Request","message":"startup failed:\nadfb888d8dcaaecc0b9cd271d9eb450f3e38dc2d:script-reg-svc: 2: Unexpected input: &#39;x&#39; &#64; line 2, column 8.\n   return x\n          ^\n\n1 error\n"}"#;
        let SyntaxCheck::Invalid(errs) = parse_idm_compile(Err(Error::Api {
            status: 400,
            body: body.into(),
        }))
        .unwrap() else {
            panic!("expected Invalid");
        };
        assert!(
            errs[0].message.contains("Unexpected input: 'x'"),
            "entities should be decoded, got: {}",
            errs[0].message
        );
        // `&#64;` is `@`, which this tenant escaped too — so the position
        // suffix is only findable after unescaping. If `unescape_idm` stops
        // covering it, these coordinates silently go missing.
        assert_eq!((errs[0].line, errs[0].column), (Some(2), Some(8)));
    }

    /// 503 means "unrecognised `type`", verified deterministic. It is neither
    /// a syntax verdict nor a transient, so it must not block the push and
    /// must not read as a pass.
    #[test]
    fn idm_503_is_skipped_not_invalid_and_not_ok() {
        let check = parse_idm_compile(Err(Error::Api {
            status: 503,
            body: r#"{"code":503,"reason":"Service Unavailable","message":"Service Unavailable"}"#
                .into(),
        }))
        .unwrap();
        assert!(matches!(check, SyntaxCheck::Skipped(_)));
    }

    /// Anything else is a real transport failure and belongs to the caller —
    /// swallowing a 401 as "unchecked" would hide a locked agent.
    #[test]
    fn idm_other_api_errors_propagate() {
        let err = parse_idm_compile(Err(Error::Api {
            status: 401,
            body: "unauthorized".into(),
        }));
        assert!(err.is_err());
    }

    #[test]
    fn idm_non_true_body_is_skipped() {
        assert!(matches!(
            parse_idm_compile(Ok(json!({"unexpected": true}))).unwrap(),
            SyntaxCheck::Skipped(_)
        ));
    }

    // -- helpers ----------------------------------------------------------

    #[test]
    fn groovy_position_absent_when_message_has_none() {
        assert_eq!(groovy_position("syntax error"), (None, None));
    }

    /// `&amp;` must be expanded last. Expanding it first would turn a literal
    /// `&amp;#39;` into `'`, inventing a quote the script never had.
    #[test]
    fn unescape_expands_ampersand_last() {
        assert_eq!(unescape_idm("&amp;#39;"), "&#39;");
        assert_eq!(unescape_idm("a &lt;&#61; b"), "a <= b");
    }

    #[test]
    fn render_degrades_with_partial_coordinates() {
        let only_line = SyntaxError {
            line: Some(7),
            column: None,
            message: "boom".into(),
        };
        assert_eq!(only_line.render(), "7: boom");
    }
}
