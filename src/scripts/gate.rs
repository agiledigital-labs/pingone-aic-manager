//! The one tenant-write path, and the token that proves a write came through
//! it.
//!
//! Neither AM's nor IDM's write endpoint parses the source it stores — a `PUT`
//! of an unbalanced-paren script returns 201 on both — so [`write_checked`] is
//! the whole safety net, and a write that goes around it reproduces exactly
//! the distant-failure problem the gate exists to prevent.
//!
//! Routing every write through one function used to be a convention, guarded
//! by a test that scanned `sync.rs` for the literal call it was trying to
//! prevent. That caught the obvious spelling and nothing else: a call split
//! across lines, `kind` bound to another name, a raw per-kind writer called
//! directly, or the same call in any other file all passed it, because
//! `Kind::write` and the five per-kind writers were `pub`.
//!
//! It is now a compiler check. [`WritePermit`] has a private field, so only
//! code *in this module* can construct one; [`Kind::write`] and every raw
//! per-kind writer require a `&WritePermit` and are `pub(super)`. A new write
//! site cannot skip the gate without either failing to compile or editing this
//! file.
//!
//! What the permit proves is **routing**, not correctness: it says the write
//! came through `write_checked`, not that the check inside it was the right
//! one. That is deliberate — routing is the part that was slipping, and the
//! decision itself is one `match` in [`decide`], directly below the only place
//! a permit is minted. The residual risk is a second minting site added inside
//! this module, which is a two-line diff in a file whose entire purpose is to
//! not have one.

use super::syntax::{Refusal, SyntaxCheck};
use super::{Kind, RemoteScript};
use crate::Result;

/// Proof that a tenant write is being made from inside [`write_checked`],
/// after the syntax gate had its say.
///
/// The `()` field is private, so this type is unconstructable outside this
/// module. Every writer takes one by reference and ignores it; its only job is
/// to be impossible to obtain anywhere else.
pub struct WritePermit(());

/// Whether a write runs the pre-flight syntax check.
///
/// A distinct type rather than a `bool` on purpose: `push` and `reconcile`
/// already carry `force` and `confirmed_prod`, and a third adjacent bool would
/// be transposable at a call site — with the failure being a silently
/// unchecked push or a spuriously skipped guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxGate {
    /// Ask the tenant to parse the source first; refuse the write if it will
    /// not. The default everywhere.
    Check,
    /// Write without asking. For when the check itself is in the way — the
    /// escape hatch exists because the write path never checked syntax before
    /// this gate did, so it must always be possible to get back to that.
    Skip,
}

/// What the gate did about one write.
#[derive(Debug)]
#[must_use]
pub(super) enum Gated {
    /// The write happened, and the source was parsed first (or the gate was
    /// off, at the caller's explicit request).
    Written,
    /// Nothing was written.
    Refused(Refusal),
}

/// Every tenant write in the sync engine goes through here.
///
/// Under [`SyntaxGate::Check`] this fails **closed**, with no exceptions: only
/// a pass writes. It used to warn and write anyway whenever it had no verdict,
/// on the reasoning that blocking on an unanswerable pre-flight was a
/// regression against having no pre-flight — which is wrong twice over.
/// `--force=syntax-check` is that regression, on request; and the shapes that
/// reached the fail-open path were mostly ones this gate could check and did
/// not (a nested endpoint `source`, a legacy array-form AM `script`), so it
/// reproduced exactly the distant-failure the feature exists to prevent, while
/// reporting a successful push.
///
/// The narrower version of the same mistake is worth naming because it looked
/// principled: letting a resource nothing *can* check write anyway, on the
/// grounds that the decision came from the resource rather than from a
/// response. It is still unparsed source, `idm::list` will happily sync an
/// endpoint declaring any engine at all, and in a batch such a write did not
/// even count as a refusal. Where the decision came from belongs in the
/// message and the remedy, not in the permission.
///
/// Presentation is the caller's: this returns the outcome and prints nothing.
/// The engine is shared with the TUI, where a stray `eprintln!` lands on the
/// alternate screen.
pub(super) async fn write_checked(
    kind: Kind,
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<Gated> {
    if let Some(refusal) = wrong_target(script) {
        return Ok(Gated::Refused(refusal));
    }
    if gate == SyntaxGate::Check
        && let Err(refusal) = decide(kind.check_syntax(tenant, realm, script).await?)
    {
        return Ok(Gated::Refused(refusal));
    }
    // The only `WritePermit` in the crate.
    kind.write(tenant, realm, script, confirmed_prod, &WritePermit(()))
        .await?;
    Ok(Gated::Written)
}

/// A generated endpoint bundle pushed under another endpoint's name. Checked
/// ahead of, and regardless of, the syntax gate: `--force=syntax-check` says
/// "I accept unparsed source", not "I accept another endpoint's source".
fn wrong_target(script: &RemoteScript) -> Option<Refusal> {
    if script.reference.kind != Kind::IdmEndpoint {
        return None;
    }
    let source = super::idm::decode_source(&script.raw_config).ok()?;
    let source = std::str::from_utf8(&source).ok()?;
    let built_for = super::idm::built_for(source)?;
    (built_for != script.reference.name).then(|| Refusal::WrongTarget {
        endpoint: script.reference.name.clone(),
        built_for: built_for.to_string(),
    })
}

/// Whether a verdict permits the write. Split out of [`write_checked`] because
/// it is the whole fail-closed rule and the only part of it a test can reach
/// without a tenant.
fn decide(verdict: SyntaxCheck) -> std::result::Result<(), Refusal> {
    match verdict {
        SyntaxCheck::Ok => Ok(()),
        SyntaxCheck::Invalid(errors) => Err(Refusal::Rejected(errors)),
        SyntaxCheck::NoVerdict(reason) => Err(Refusal::NoVerdict(reason)),
        SyntaxCheck::Unsupported(reason) => Err(Refusal::Unsupported(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::syntax::SyntaxError;

    /// The gate fails **closed**: `Ok` is the only verdict that writes. The
    /// two no-verdict arms are the discriminating cases — each in turn used to
    /// write with a warning, which is what let an IDM 503, an unparsed nested
    /// `source`, and an endpoint declaring an unknown engine through while the
    /// tool reported a successful push. Their arms differ only so the remedy
    /// can differ.
    fn endpoint(name: &str, source: &str) -> RemoteScript {
        crate::scripts::idm::new_script(name, source.as_bytes(), &Default::default()).unwrap()
    }

    fn bundle_for(stem: &str) -> String {
        // The shape `tools/build.mjs`'s `banner()` writes.
        format!(
            "// @generated by `npm run build` in workspace/<tenant>/typescript — DO NOT EDIT.\n\
             // @ts-nocheck\n//\n// Source:  typescript/src/endpoints/{stem}.ts\n\
             // Rebuild: (cd ../../typescript && npm run build)\n//\n\nvar x = 1;\n"
        )
    }

    /// The incident: one bundle copied over another's file and pushed. The
    /// matching case is the control — the same banner under its own name must
    /// pass, or the check refuses every generated endpoint.
    #[test]
    fn a_bundle_built_for_another_endpoint_is_refused() {
        assert_eq!(
            wrong_target(&endpoint("orders", &bundle_for("users"))),
            Some(Refusal::WrongTarget {
                endpoint: "orders".into(),
                built_for: "users".into(),
            })
        );
        assert_eq!(wrong_target(&endpoint("users", &bundle_for("users"))), None);
    }

    /// Only a claim can be wrong: hand-written source, and a `// Source:` line
    /// that is not under the generated header, make none.
    #[test]
    fn source_without_the_generated_banner_makes_no_claim() {
        assert_eq!(wrong_target(&endpoint("orders", "var x = 1;\n")), None);
        assert_eq!(
            wrong_target(&endpoint(
                "orders",
                "// Source:  typescript/src/endpoints/users.ts\nvar x = 1;\n"
            )),
            None
        );
        // A `// Source:` after the header block has ended is code, not banner.
        assert_eq!(
            wrong_target(&endpoint(
                "orders",
                "// @generated by `npm run build`\nvar x;\n// Source:  typescript/users.ts\n"
            )),
            None
        );
    }

    #[test]
    fn only_a_pass_may_write() {
        assert_eq!(decide(SyntaxCheck::Ok), Ok(()));
        assert_eq!(
            decide(SyntaxCheck::NoVerdict("503".into())),
            Err(Refusal::NoVerdict("503".into())),
            "a check with no verdict must not authorise a write"
        );
        assert_eq!(
            decide(SyntaxCheck::Unsupported("no engine for it".into())),
            Err(Refusal::Unsupported("no engine for it".into())),
            "a resource nothing can check must not authorise a write either"
        );
        let errors = vec![SyntaxError {
            line: Some(3),
            column: None,
            message: "boom".into(),
        }];
        assert_eq!(
            decide(SyntaxCheck::Invalid(errors.clone())),
            Err(Refusal::Rejected(errors))
        );
    }
}
