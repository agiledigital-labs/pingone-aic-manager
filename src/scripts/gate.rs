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
    if let Some(refusal) = local_refusal(realm, script) {
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

/// The refusals decided from the source alone — before the syntax check and
/// the write, though a caller may already have read the tenant — and past
/// every `--force`: a bundle that says it belongs to another script.
fn local_refusal(realm: &str, script: &RemoteScript) -> Option<Refusal> {
    wrong_target(script).or_else(|| marked_for_other(realm, script))
}

/// A generated endpoint bundle pushed under another endpoint's name. Checked
/// ahead of, and regardless of, the syntax gate: `--force=syntax-check` says
/// "I accept unparsed source", not "I accept another endpoint's source".
fn wrong_target(script: &RemoteScript) -> Option<Refusal> {
    if script.reference.kind != Kind::IdmEndpoint {
        return None;
    }
    let source = super::idm::decode_source(&script.raw_config).ok()?;
    let source = String::from_utf8_lossy(&source);
    let built_for = super::idm::built_for(&source)?;
    (built_for != script.reference.name).then(|| Refusal::WrongTarget {
        endpoint: script.reference.name.clone(),
        built_for: built_for.to_string(),
    })
}

/// The `aic-target` marker a build outside this repo writes into a bundle's
/// leading comments (decision D-S5): `// aic-target: <full name>`, the same
/// name `aic script push` takes — `managed/alpha_user.onCreate`,
/// `endpoint/users`, `alpha/<script>`.
const TARGET_MARKER: &str = "aic-target:";

/// Every marker in the source's **leading** run of `//` lines, in order. Only
/// the leading block counts: a bundle inlines other modules, and a marker
/// inside inlined code belongs to whatever that code was built for, not to
/// this file. A marker with nothing after it is kept as `""`, so it fails the
/// comparison rather than vanishing.
///
/// Parsing errs towards *finding* a marker, because one that goes unread is
/// fail-open: a leading byte-order mark is skipped (the banner check skips it
/// too), and any spacing is allowed between `//` and `aic-target:`.
pub(super) fn target_markers(source: &str) -> Vec<String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    source
        .lines()
        .map(str::trim_start)
        .map_while(|line| line.strip_prefix("//"))
        .filter_map(|comment| comment.trim_start().strip_prefix(TARGET_MARKER))
        .map(|rest| rest.trim().to_string())
        .collect()
}

/// A source marked for a different script than the one being written.
/// Unmarked source makes no claim and passes: until the producer emits the
/// marker, refusing it would refuse every hook.
///
/// The source is read **lossily**: an AM script carries arbitrary bytes, and
/// one undecodable byte elsewhere in the file must not erase an intact ASCII
/// marker. A payload that cannot be decoded at all is refused — whether it is
/// marked cannot be known, and not knowing is not "unmarked".
fn marked_for_other(realm: &str, script: &RemoteScript) -> Option<Refusal> {
    let kind = script.reference.kind;
    let source = match kind.decode_source(&script.raw_config) {
        Ok(source) => source,
        Err(error) => {
            return Some(Refusal::Undecodable(error.to_string()));
        }
    };
    let marked = target_markers(&String::from_utf8_lossy(&source));
    let target = super::full_name(
        kind,
        (kind == Kind::Am).then_some(realm),
        &script.reference.name,
    );
    marked
        .iter()
        .any(|m| *m != target)
        .then_some(Refusal::MarkedForOther { target, marked })
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

    /// Through the real gate, with the syntax check skipped: the refusal is
    /// returned before any I/O (tenant `unused` would fail any request), so
    /// removing the `wrong_target` call from `write_checked` fails this test.
    /// A leading byte-order mark does not switch the check off.
    #[tokio::test]
    async fn the_gate_refuses_a_wrong_target_before_writing() {
        for source in [
            bundle_for("users"),
            format!("\u{feff}{}", bundle_for("users")),
        ] {
            let outcome = write_checked(
                Kind::IdmEndpoint,
                "unused",
                "alpha",
                &endpoint("orders", &source),
                false,
                SyntaxGate::Skip,
            )
            .await;
            assert!(
                matches!(outcome, Ok(Gated::Refused(Refusal::WrongTarget { .. }))),
                "{outcome:?}"
            );
        }
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

    fn hook(name: &str, source: &str) -> RemoteScript {
        RemoteScript {
            reference: super::super::RemoteRef {
                kind: Kind::IdmManagedHook,
                id: format!("managed/{name}"),
                name: name.into(),
                context: None,
                is_default: false,
                evaluator_version: None,
            },
            raw_config: serde_json::json!({"type": "text/javascript", "source": source}),
        }
    }

    fn marked(target: &str) -> String {
        format!("// built by the outside project\n// aic-target: {target}\n\nvar x = 1;\n")
    }

    /// D-S5's incident: one object's hook bundle pushed to another object's
    /// hook. The matching marker is the control.
    #[test]
    fn a_hook_marked_for_another_hook_is_refused() {
        assert_eq!(
            local_refusal(
                "",
                &hook(
                    "alpha_role.onCreate",
                    &marked("managed/alpha_user.onCreate")
                )
            ),
            Some(Refusal::MarkedForOther {
                target: "managed/alpha_role.onCreate".into(),
                marked: vec!["managed/alpha_user.onCreate".into()],
            })
        );
        assert_eq!(
            local_refusal(
                "",
                &hook(
                    "alpha_user.onCreate",
                    &marked("managed/alpha_user.onCreate")
                )
            ),
            None
        );
        // Same object, another hook: still another target.
        assert!(
            local_refusal(
                "",
                &hook(
                    "alpha_user.onUpdate",
                    &marked("managed/alpha_user.onCreate")
                )
            )
            .is_some()
        );
    }

    #[test]
    fn unmarked_source_and_markers_outside_the_leading_block_make_no_claim() {
        assert_eq!(
            local_refusal("", &hook("alpha_user.onCreate", "var x = 1;\n")),
            None
        );
        // An inlined module's marker is that module's, not this file's.
        let inlined = "// header\nvar x = 1;\n// aic-target: managed/alpha_role.onCreate\n";
        assert_eq!(
            local_refusal("", &hook("alpha_user.onCreate", inlined)),
            None
        );
    }

    #[test]
    fn conflicting_or_empty_markers_are_refused() {
        let two = "// aic-target: managed/alpha_user.onCreate\n// aic-target: managed/alpha_role.onCreate\nvar x;\n";
        let Some(Refusal::MarkedForOther { marked, .. }) =
            local_refusal("", &hook("alpha_user.onCreate", two))
        else {
            panic!("two markers that disagree must refuse");
        };
        assert_eq!(marked.len(), 2);
        assert!(
            local_refusal(
                "",
                &hook("alpha_user.onCreate", "//   aic-target:\nvar x;\n")
            )
            .is_some()
        );
        // Indented comment lines are still the leading block.
        assert!(
            local_refusal(
                "",
                &hook(
                    "alpha_user.onCreate",
                    "  // aic-target: endpoint/users\nvar x;\n"
                )
            )
            .is_some()
        );
    }

    /// The marker names a full name, so an AM script's realm is part of it:
    /// `alpha/x` is not `bravo/x`.
    #[test]
    fn an_am_marker_is_compared_with_its_realm() {
        let am = |realm: &str, marker: &str| {
            let opts = super::super::NewScriptOpts {
                context: Some("OAUTH2_ACCESS_TOKEN_MODIFICATION".into()),
                ..Default::default()
            };
            let script = Kind::Am
                .new_script(
                    "Toggle",
                    format!("// aic-target: {marker}\nvar x;\n").as_bytes(),
                    &opts,
                )
                .unwrap();
            local_refusal(realm, &script)
        };
        assert_eq!(am("alpha", "alpha/Toggle"), None);
        assert!(am("bravo", "alpha/Toggle").is_some());
    }

    /// Both reviewers' discriminating input: an intact marker followed by a
    /// Latin-1 byte, which no UTF-8 decode accepts. The marker still binds.
    #[test]
    fn an_undecodable_byte_elsewhere_does_not_erase_the_marker() {
        let opts = super::super::NewScriptOpts {
            context: Some("OAUTH2_ACCESS_TOKEN_MODIFICATION".into()),
            ..Default::default()
        };
        let source = b"// aic-target: alpha/Intended\n// caf\xe9\nvar x;\n";
        let script = Kind::Am.new_script("Other", source, &opts).unwrap();
        assert!(
            matches!(
                local_refusal("alpha", &script),
                Some(Refusal::MarkedForOther { .. })
            ),
            "{:?}",
            local_refusal("alpha", &script)
        );
        let control = Kind::Am.new_script("Intended", source, &opts).unwrap();
        assert_eq!(local_refusal("alpha", &control), None);
    }

    /// A payload that cannot be decoded is refused, not read as unmarked.
    #[test]
    fn an_undecodable_payload_is_refused() {
        let mut script = hook("alpha_user.onCreate", "var x;");
        script.raw_config = serde_json::json!({"type": "text/javascript"});
        assert!(matches!(
            local_refusal("", &script),
            Some(Refusal::Undecodable(_))
        ));
    }

    /// Through the gate itself, as the banner check is: a marked hook is
    /// refused before the syntax check or the write, with and without a
    /// byte-order mark.
    #[tokio::test]
    async fn the_gate_refuses_a_hook_marked_for_another_before_writing() {
        let source = marked("managed/alpha_user.onCreate");
        for source in [source.clone(), format!("\u{feff}{source}")] {
            let outcome = write_checked(
                Kind::IdmManagedHook,
                "unused",
                "",
                &hook("alpha_role.onCreate", &source),
                false,
                SyntaxGate::Skip,
            )
            .await;
            assert!(
                matches!(outcome, Ok(Gated::Refused(Refusal::MarkedForOther { .. }))),
                "{outcome:?}"
            );
        }
    }

    /// Endpoints get both checks; the marker is not a way past the banner.
    #[test]
    fn an_endpoint_gets_the_banner_check_and_the_marker_check() {
        assert!(local_refusal("", &endpoint("orders", &marked("endpoint/users"))).is_some());
        assert_eq!(
            local_refusal("", &endpoint("orders", &marked("endpoint/orders"))),
            None
        );
        assert!(local_refusal("", &endpoint("orders", &bundle_for("users"))).is_some());
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
