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
///
/// Only [`Ok`](SyntaxCheck::Ok) authorises a write under the default gate. The
/// other three all refuse, including the two that carry no verdict: whether
/// the check could not answer or could never have answered, the source about
/// to be stored is unparsed either way, and `--no-syntax-check` is the one
/// sanctioned way to store unparsed source. What the two no-verdict arms are
/// *for* is diagnosis and remedy — retrying helps one and can never help the
/// other — not permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxCheck {
    /// The tenant parsed the source.
    Ok,
    /// The tenant refused it. Always at least one error.
    Invalid(Vec<SyntaxError>),
    /// This resource cannot be syntax-checked at all — established from what
    /// it stores, with no request made. Carries the reason. Retrying will
    /// never change it, so the remedy named is the explicit opt-out.
    Unsupported(String),
    /// The check was attempted and produced no verdict: an unexpected body, a
    /// status that does not decide, a slot with no source to send. Carries the
    /// reason, and may well be a tenant having a bad minute, so retrying is
    /// worth suggesting.
    NoVerdict(String),
}

/// Why the gate refused a write. Every arm means **nothing was written**, and
/// all are recoverable: the local source survives and the snapshot is
/// untouched, so the next push retries.
///
/// The arms exist to word the remedy, not to decide the outcome — the outcome
/// is the same for all three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The tenant parsed the source and rejected it. Always at least one
    /// error. Retrying the same bytes gets the same answer.
    Rejected(Vec<SyntaxError>),
    /// The check produced no verdict. Carries the reason. May be transient, so
    /// retrying is the first thing to try.
    NoVerdict(String),
    /// Nothing can check this resource, decided before the request was made.
    /// Retrying can never help; the explicit opt-out is the only way past it.
    Unsupported(String),
}

impl Refusal {
    /// Why, in one clause and without the per-error detail. Every surface
    /// leads with this, so a refusal reads the same in the CLI, in `watch` and
    /// in the tab.
    pub fn summary(&self) -> String {
        match self {
            Refusal::Rejected(_) => "the tenant refused to parse this source".into(),
            Refusal::NoVerdict(reason) => {
                format!("the syntax check gave no verdict — {reason}")
            }
            Refusal::Unsupported(reason) => {
                format!("the source cannot be syntax-checked — {reason}")
            }
        }
    }

    /// The detail under the summary, one string per line. Empty for
    /// `NoVerdict`, which has no per-error detail to give.
    pub fn detail(&self) -> Vec<String> {
        match self {
            Refusal::Rejected(errors) => {
                let mut lines: Vec<String> = errors.iter().map(SyntaxError::render).collect();
                if errors.iter().all(|e| e.line.is_none()) {
                    // Say that no coordinate was sent rather than let the
                    // operator hunt for one. Deliberately **not** attributed
                    // to IDM-and-JavaScript, which is the common cause but not
                    // derivable here: `Refusal` carries neither family nor
                    // engine, and AM synthesises a coordinate-free error of
                    // its own for a `success:false` with an empty `errors`.
                    lines.push("(the tenant reported no line number for this error)".into());
                }
                lines
            }
            Refusal::NoVerdict(_) | Refusal::Unsupported(_) => Vec::new(),
        }
    }

    /// One line, for a surface with one line to spend (a toast, a `watch`
    /// row): the summary plus the first detail, so coordinates survive the
    /// squeeze.
    pub fn headline(&self) -> String {
        match self.detail().first() {
            Some(first) => format!("{} — {first}", self.summary()),
            None => self.summary(),
        }
    }
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
        // Not a verdict, and not something the resource told us in advance:
        // AM answering without `success` means the action did not decide, so
        // the write does not proceed on the strength of it.
        None => SyntaxCheck::NoVerdict("AM validate answered without a `success` field".into()),
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
        Ok(other) => Ok(SyntaxCheck::NoVerdict(format!(
            "IDM compile answered {other} rather than `true`"
        ))),
        Err(Error::Api { status: 400, body }) => Ok(SyntaxCheck::Invalid(vec![idm_error(&body)])),
        // A 503 is what this endpoint answers for a script `type` it does not
        // recognise — verified 2026-09-09, deterministic across repeats with a
        // healthy call in between (`docs/api/11-idm-endpoints.md`). What that
        // measurement establishes is `bad type -> 503`, and reading it
        // backwards is how a genuine outage came to authorise a write: 503 is
        // also just 503. The accepted spellings are known, so an unrecognised
        // type is caught by [`resolve_slot`] before the request; anything that
        // still answers 503 has not decided, and nothing gets written on it.
        Err(Error::Api { status: 503, .. }) => Ok(SyntaxCheck::NoVerdict(
            "IDM compile answered 503 (unrecognised script `type`, or the service is unwell)"
                .into(),
        )),
        Err(e) => Err(e),
    }
}

/// The one `type` spelling to send for a stored one, or `None` when the stored
/// type is not a spelling of an engine this has been taught.
///
/// The action accepts exactly `javascript`, `text/javascript` and `groovy`;
/// `JAVASCRIPT`, `text/groovy` and `text/python` all answer 503
/// (`docs/api/11-idm-endpoints.md`, verified 2026-09-09). So **normalise**
/// rather than forward: `text/groovy` is the same Groovy engine spelled a way
/// the action rejects, and forwarding it buys a 503 and no verdict.
///
/// The alias set is **exact and reviewed**, not a substring test. A substring
/// test reads `application/x-not-javascript` as JavaScript and certifies it
/// under an engine nobody chose — and it is the compile call, so a wrong
/// engine means a wrong verdict, not just a wrong label. Anything not listed
/// here is `Unsupported`, which refuses the write and names the opt-out, so an
/// unknown spelling costs an explicit decision rather than a silent one.
///
/// `application/javascript` is the one alias here on **compatibility** rather
/// than measurement. `sync_mapping::is_inline_script` accepts any `type`
/// containing `javascript`, so a tenant that stores that spelling is already
/// syncable, and it is unambiguously the same engine — but no live call has
/// confirmed the compile action's answer for source stored that way, and this
/// tool has no production path that writes it.
fn engine_for(stored: &str) -> Option<&'static str> {
    match stored.trim().to_ascii_lowercase().as_str() {
        // Measured-accepted, plus the spellings measured to 503 that name the
        // same engine unambiguously, plus the one compatibility alias above.
        "javascript" | "text/javascript" | "application/javascript" => Some("text/javascript"),
        "groovy" | "text/groovy" | "application/groovy" => Some("groovy"),
        _ => None,
    }
}

/// Types that discriminate the **config shape** rather than the script engine,
/// so finding one says nothing about the language: an endpoint's `type` is
/// `text/javascript` "also seen as `scripted`" and may be `table`/`jdbc`
/// (`docs/api/11-idm-endpoints.md`), and a schedule's root `type` is its
/// trigger (`cron`). Look past these to the next candidate rather than reading
/// them as an unsupported engine.
fn is_container_type(stored: &str) -> bool {
    matches!(
        stored.trim().to_ascii_lowercase().as_str(),
        "scripted" | "cron" | "simple" | "table" | "jdbc"
    )
}

/// What an IDM slot resolves to for the compile action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotResolution<'a> {
    /// Send this source under this (normalised) type.
    Compile {
        source: &'a str,
        script_type: &'static str,
    },
    /// The slot names an engine the action does not compile. Recognised here,
    /// before the request, rather than inferred from the 503 it would answer.
    UnsupportedType(String),
    /// No plaintext source in the slot at all — file-backed, table/jdbc, or a
    /// shape this parser does not know.
    NoSource,
}

/// Locate the plaintext source in an IDM script slot and decide what `type` to
/// compile it as.
///
/// Two source shapes, both supported by every other seam and so both checked
/// here: a direct string, and the nested `{ "source": …, "type": … }` form a
/// scripted endpoint may store (`idm::has_inline_source`, `idm::decode_source`).
/// Missing the nested one is not a cosmetic gap — it was the shape that got
/// written unchecked while the tool reported a check.
///
/// The type is taken from the innermost level that names an engine, because
/// that is the level the source belongs to; a container type there (`scripted`)
/// defers to the outer one, and no engine information anywhere falls back to
/// `text/javascript`, which is what every IDM script kind this tool syncs
/// actually stores.
pub fn resolve_slot(slot: &Value) -> SlotResolution<'_> {
    let root_type = slot.get("type").and_then(Value::as_str);
    let (source, nested_type) = match slot.get("source") {
        Some(Value::String(s)) => (s.as_str(), None),
        Some(Value::Object(o)) => match o.get("source").and_then(Value::as_str) {
            Some(s) => (s, o.get("type").and_then(Value::as_str)),
            None => return SlotResolution::NoSource,
        },
        _ => return SlotResolution::NoSource,
    };
    for candidate in [nested_type, root_type].into_iter().flatten() {
        if is_container_type(candidate) {
            continue;
        }
        return match engine_for(candidate) {
            Some(script_type) => SlotResolution::Compile {
                source,
                script_type,
            },
            None => SlotResolution::UnsupportedType(candidate.to_string()),
        };
    }
    SlotResolution::Compile {
        source,
        script_type: "text/javascript",
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
    let Some(slot) = slot else {
        return Ok(SyntaxCheck::NoVerdict(
            "no script slot in this config to compile".into(),
        ));
    };
    let (source, script_type) = match resolve_slot(slot) {
        SlotResolution::Compile {
            source,
            script_type,
        } => (source, script_type),
        // Established from the resource, not from a status code — so the
        // message can name the type and skip the pointless retry advice. It
        // still refuses the write: unparsed source is unparsed source.
        SlotResolution::UnsupportedType(t) => {
            return Ok(SyntaxCheck::Unsupported(format!(
                "no compile engine is known for script type {t:?}"
            )));
        }
        SlotResolution::NoSource => {
            return Ok(SyntaxCheck::NoVerdict(
                "no plaintext `source` field to compile".into(),
            ));
        }
    };
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
// The bodies for the cases the sandbox actually produced on 2026-09-09 are
// pasted verbatim from those calls — the AM pass/fail pair, the IDM bare
// `true`, and the IDM JavaScript and Groovy 400s. Those are the fixtures that
// carry weight, because a hand-built body would only prove the parser agrees
// with my idea of the wire format, which is the thing under test. The
// `## Verified against` blocks in `docs/api/04-scripts.md` and
// `docs/api/11-idm-endpoints.md` record exactly those calls.
//
// The rest are **constructed**, and say so here rather than borrowing the
// captured fixtures' provenance: the malformed/missing-field bodies, the
// non-`true` 200, the 401, and every `resolve_slot` input. They pin policy for
// responses the tenant was never observed to send — which is the point, since
// policy for an answer nobody has seen is precisely where fail-open hid.
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
    fn am_missing_success_field_is_no_verdict_not_ok() {
        let body: Value = serde_json::from_str(r#"{"unexpected": 1}"#).unwrap();
        assert!(matches!(
            parse_am_validate(&body),
            SyntaxCheck::NoVerdict(_)
        ));
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

    /// A 503 is what an unrecognised `type` earns — but it is also just a
    /// 503, and the measurement only ever established `bad type -> 503`. So it
    /// must land as `NoVerdict` (nothing written), **not** as `Unsupported`
    /// (written anyway): reading the implication backwards is what let an
    /// outage authorise a write. An unrecognised type is caught by
    /// `resolve_slot` before the request instead.
    #[test]
    fn idm_503_yields_no_verdict_and_never_authorises_a_write() {
        let check = parse_idm_compile(Err(Error::Api {
            status: 503,
            body: r#"{"code":503,"reason":"Service Unavailable","message":"Service Unavailable"}"#
                .into(),
        }))
        .unwrap();
        assert!(
            matches!(check, SyntaxCheck::NoVerdict(_)),
            "503 must not be reported as a checkable-shape decision: {check:?}"
        );
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
    fn idm_non_true_body_is_no_verdict() {
        assert!(matches!(
            parse_idm_compile(Ok(json!({"unexpected": true}))).unwrap(),
            SyntaxCheck::NoVerdict(_)
        ));
    }

    // -- slot resolution --------------------------------------------------
    //
    // These drive the same `resolve_slot` `idm_check_slot` calls, so an
    // implementation that goes back to forwarding the stored `type`, or to
    // reading only a string-valued `source`, fails here.

    fn compiled(slot: Value) -> (String, &'static str) {
        match resolve_slot(&slot) {
            SlotResolution::Compile {
                source,
                script_type,
            } => (source.to_string(), script_type),
            other => panic!("expected Compile, got {other:?}"),
        }
    }

    /// The nested form is the hole that mattered: `idm::has_inline_source` and
    /// `idm::decode_source` both accept it, so a scripted endpoint stored this
    /// way was written unchecked while the tool reported a check.
    #[test]
    fn nested_endpoint_source_is_found_and_typed_from_the_inner_level() {
        assert_eq!(
            compiled(json!({
                "_id": "endpoint/nested",
                "type": "scripted",
                "source": {"type": "text/javascript", "source": "(function(){})();"}
            })),
            ("(function(){})();".to_string(), "text/javascript")
        );
    }

    /// The discriminating case for normalisation. `application/javascript` is
    /// accepted by mapping detection and written by our own Mappings tab, and
    /// it is **not** one of the three spellings the action takes — forwarding
    /// it earns a 503 and (now) a refused write, so it must be normalised.
    #[test]
    fn unmeasured_javascript_spellings_are_normalised_not_forwarded() {
        for stored in [
            "application/javascript",
            "JAVASCRIPT",
            "javascript",
            "text/javascript",
        ] {
            let (_, sent) = compiled(json!({"type": stored, "source": "var x = 1;"}));
            assert_eq!(sent, "text/javascript", "stored type {stored:?}");
        }
        // `text/groovy` is a measured 503; `groovy` is the accepted spelling.
        let (_, sent) = compiled(json!({"type": "text/groovy", "source": "def x = 1"}));
        assert_eq!(sent, "groovy");
    }

    /// A container type says nothing about the engine, so it must not be
    /// mistaken for one — and must not shadow a real type further out.
    #[test]
    fn container_types_fall_through_to_the_default() {
        assert_eq!(
            compiled(json!({"type": "scripted", "source": "var x = 1;"})),
            ("var x = 1;".to_string(), "text/javascript")
        );
        assert_eq!(
            compiled(json!({"type": "cron", "source": {"source": "var x = 1;"}})),
            ("var x = 1;".to_string(), "text/javascript")
        );
    }

    /// An engine the action does not compile is recognised *here*, with no
    /// request made — the structured, visible decision that replaces inferring
    /// it from a 503.
    #[test]
    fn an_engine_the_action_cannot_compile_is_named_before_the_request() {
        assert_eq!(
            resolve_slot(&json!({"type": "text/python", "source": "x = 1"})),
            SlotResolution::UnsupportedType("text/python".into())
        );
        // The discriminating case against a substring test, which would
        // certify this as JavaScript and compile it as an engine nobody chose
        // — and it is the compile call, so the wrong engine is a wrong
        // verdict, not a wrong label.
        assert_eq!(
            resolve_slot(&json!({"type": "application/x-not-javascript", "source": "x"})),
            SlotResolution::UnsupportedType("application/x-not-javascript".into())
        );
    }

    #[test]
    fn a_slot_with_no_plaintext_source_resolves_to_no_source() {
        assert_eq!(
            resolve_slot(&json!({"type": "text/javascript", "file": "sync/onUpdate.js"})),
            SlotResolution::NoSource
        );
        // Nested, but not a string underneath.
        assert_eq!(
            resolve_slot(&json!({"source": {"source": 7}})),
            SlotResolution::NoSource
        );
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

    /// The refusal wording is shared by the CLI, `watch` and the tab, so it is
    /// pinned once here. The discriminating part is the IDM note: it must
    /// appear only when *no* error carried a coordinate — printing it beside a
    /// line number would tell the operator to stop looking at the line number
    /// they were just given.
    #[test]
    fn refusal_wording_says_where_when_it_can_and_says_why_not_when_it_cannot() {
        let with_line = Refusal::Rejected(vec![SyntaxError {
            line: Some(3),
            column: Some(15),
            message: "missing ) in parenthetical".into(),
        }]);
        assert_eq!(
            with_line.detail(),
            ["3:15: missing ) in parenthetical".to_string()]
        );
        assert!(with_line.headline().contains("3:15"));

        let no_line = Refusal::Rejected(vec![SyntaxError {
            line: None,
            column: None,
            message: "syntax error".into(),
        }]);
        assert_eq!(no_line.detail().len(), 2);
        assert!(no_line.detail()[1].contains("no line number"));

        // A no-verdict refusal has no per-error detail, so the reason has to
        // be in the summary or it is lost entirely.
        let none = Refusal::NoVerdict("IDM compile answered 503".into());
        assert!(none.detail().is_empty());
        assert!(none.summary().contains("503"));
        assert_eq!(none.headline(), none.summary());
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
