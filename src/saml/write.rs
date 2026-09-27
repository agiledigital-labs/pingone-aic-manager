//! What a SAML write is allowed to claim about itself, shared by `rotate`
//! and `cert`.
//!
//! Three pieces, each a rule both verticals follow and neither should own:
//!
//! - [`Decision`]: a `--dry-run` stops by holding no permit, never by an `if`
//!   in front of the write.
//! - [`consent`]: `--force` or an answered prompt, and nothing else, lets an
//!   operation through; a headless run without `--force` is refused by the
//!   verb's own gate, which names what would change.
//! - [`WriteStatus`] / [`WriteFailure`]: a failed write says which side of the
//!   send it failed on, because "it failed" is two different claims.

use crate::{Error, Result};

/// What `authorize`-style functions return: a preview holds no permit, so a
/// preview that fell through to the write would not compile.
#[derive(Debug)]
pub enum Decision<P> {
    /// Print the plan and stop. Carries no permit.
    Preview,
    Send(P),
}

/// Whether an operation that needs consent has it: `--force`, or — only
/// where a prompt can be answered — a yes to `ask`.
///
/// `ask` is not called when forced or when there is no terminal, so a
/// headless run never blocks on a prompt and falls through to the verb's own
/// refusal (`rotate::spec::stage_ok`, `cert::spec::write_ok`, …), which names
/// what would change rather than `confirm_destructive`'s generic message.
pub fn consent(
    forced: bool,
    prompt_available: bool,
    ask: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    if forced {
        return Ok(true);
    }
    if !prompt_available {
        return Ok(false);
    }
    ask()
}

/// What is known about a tenant write that did not succeed.
///
/// Three states, because "it failed" is two different claims and an operator
/// acts on whichever one they are told. The only safe retry is from a
/// [`Refused`](WriteStatus::Refused) write; after either of the others the
/// tenant has to be read first, since re-sending an ESV version or a mapping
/// that already landed is a second write, not a retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStatus {
    /// Not applied: refused before it was sent — a recheck, the production
    /// gate, a locked agent — or answered with one of the refusals
    /// **measured** for that endpoint ([`MeasuredRefusal`]). A status class
    /// alone never earns this: a `408` does not say whether a forwarded write
    /// applied, a `409` can accompany a concurrent change, and the
    /// authenticated client follows redirects, so the status seen need not
    /// be the answer to the write that was sent.
    Refused,
    /// Sent, answered with success, and then the proof failed: the read-back
    /// or the export that should show it could not be read, or showed
    /// something else. **The write landed**; what it left is unverified.
    AcceptedUnverified,
    /// Attempted, and possibly sent, with no response this command can read
    /// as settling it — any error status that is not a measured refusal, a
    /// transport failure, a lost connection to the agent, or a success whose
    /// body could not be decoded (the agent reports that last one without its
    /// status). "Possibly": the transport mints its bearer before sending,
    /// and a minting failure is an `Error::Api` indistinguishable from the
    /// write's own, so this state cannot claim the request left.
    Unknown,
}

/// A refusal measured on a live tenant for one endpoint: the status **and**
/// the message, both required, because neither alone identifies it.
///
/// This is the only way a post-send error is classified as not applied, so
/// an entry needs the evidence a `docs/api/` row needs. The two that exist
/// are `rotate::spec`'s ESV refusals, both in `docs/api/03-esvs.md`;
/// `importEntity` has none, since every failure measured there is the same
/// opaque 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredRefusal {
    pub status: u16,
    pub message: &'static str,
}

/// A failed tenant write and what is known about it ([`WriteStatus`]).
///
/// There is deliberately no `From<Error>`: every `?` on a writer's path has
/// to say which side of the send it is on, because a conversion that
/// defaulted to one of them would label a read-back failure "not sent" the
/// first time someone added a `?` after the write.
#[derive(Debug)]
pub struct WriteFailure {
    pub status: WriteStatus,
    pub source: Error,
}

impl WriteFailure {
    /// A failure before anything was sent: a read, a recheck, a refusal.
    pub fn before_send(source: Error) -> Self {
        Self {
            status: WriteStatus::Refused,
            source,
        }
    }

    /// The write call itself failed. **Unknown by default**: only a refusal
    /// raised before the request left — the production gate, a locked agent
    /// — or one of `measured`, this endpoint's measured refusals, proves it
    /// was not applied. Everything else, whatever its status, is unknown.
    pub fn from_send(source: Error, measured: &[MeasuredRefusal]) -> Self {
        let status = match &source {
            Error::ProdConfirmRequired | Error::AuthRequired => WriteStatus::Refused,
            Error::Api { status, body }
                if measured
                    .iter()
                    .any(|refusal| refusal.status == *status && body.contains(refusal.message)) =>
            {
                WriteStatus::Refused
            }
            _ => WriteStatus::Unknown,
        };
        Self { status, source }
    }

    /// The write was accepted; proving what it left failed.
    pub fn unverified(source: Error) -> Self {
        Self {
            status: WriteStatus::AcceptedUnverified,
            source,
        }
    }

    /// The operator-facing message for the failed write `what`: the source,
    /// preceded — unless the write was refused — by the sentence that says
    /// it may have landed, and `remedy`, the read to do before retrying.
    ///
    /// Shared by `rotate init|stage|complete` and `cert add|remove|import`,
    /// so every SAML write describes the same state in the same words.
    pub fn message(&self, what: &str, remedy: &str) -> String {
        let source = self.source.to_string();
        match self.status {
            WriteStatus::Refused => source,
            WriteStatus::AcceptedUnverified | WriteStatus::Unknown => {
                format!("{} {remedy}\n{source}", self.status.sentence(what))
            }
        }
    }
}

impl WriteStatus {
    /// What this status says about `what`, for the two states that are not a
    /// refusal. A refusal's own message already says what it says.
    ///
    /// Not public: a write's message goes through [`WriteFailure::message`].
    /// `rotate`'s `InitApplyError` is the one caller that has to interleave
    /// its own clause between the sentence and the remedy.
    pub(in crate::saml) fn sentence(self, what: &str) -> String {
        match self {
            Self::Refused => format!("{what} was not applied."),
            Self::AcceptedUnverified => format!(
                "{what} was **accepted by the tenant**, and proving what it left then failed. \
                 **The write landed; this is not a no-op.**"
            ),
            Self::Unknown => format!(
                "{what} was attempted and may have been sent, and **whether it was applied is \
                 unknown**: what came back — an \
                 error status that is not a refusal measured for this endpoint, a lost \
                 connection, or a response that could not be read — does not establish \
                 either way. **Do not assume it did not land.**"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A permission needs a test that it is honoured, and one that its
    /// absence is: every combination, with whether the prompt was shown.
    #[test]
    fn consent_is_force_or_an_answered_prompt_and_never_asks_headless() {
        for (forced, terminal, answer, expected, asked) in [
            (true, true, false, true, false),
            (true, false, false, true, false),
            (false, false, true, false, false),
            (false, true, true, true, true),
            (false, true, false, false, true),
        ] {
            let shown = Cell::new(false);
            let got = consent(forced, terminal, || {
                shown.set(true);
                Ok(answer)
            })
            .unwrap();
            assert_eq!(
                (got, shown.get()),
                (expected, asked),
                "forced={forced} terminal={terminal} answer={answer}"
            );
        }
        let error = consent(false, true, || Err(Error::Config("prompt failed".into())));
        assert!(error.is_err(), "a prompt that fails is not a no");
    }
}
