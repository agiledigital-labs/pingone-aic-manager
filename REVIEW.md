# REVIEW.md — pingone-aic-manager review notes

Repo-specific review guidance, accumulated by the review-craft skill. The skill
reads **Standing checks** before every review and appends to the **Findings
log** when a review uncovers a durable lesson. Keep entries terse.

## Standing checks

Mandatory extra criteria every review applies here (promoted from recurring
findings). Each should name the guard that will eventually retire it.

- **Work added to the `cli::run` pre-flight must be free when
  `prompt_available()` is false.** AI agents drive this CLI non-interactively
  and run _every_ command that way, so anything the pre-flight does only to
  serve a prompt (network reads, tenant resolution, agent round-trips) is pure
  waste repeated forever. _Guard: none yet — wants a test asserting the
  non-interactive path makes no `aic::api` call._
- **Machine-local state added to `.aic/settings.toml` must be covered by
  `ProjectConfig::write_gitignore`.** `.aic/` is ignored in _this_ repo but not
  in user projects, where `config.toml` is deliberately shareable — anything
  per-person or per-machine that lands beside it will be committed and then
  silently applied to the whole team. _Guard: extend
  `gitignore_covers_every_artifact_stem` to assert `settings.toml`._

## Findings log

### 2026-08-06 — operator identity slice

- **What:** Five orthogonality defects in one slice. The `cli::run` pre-flight
  made a tenant HTTP call before checking whether it could prompt, so every
  non-interactive command paid for a value it discarded. The resolved value was
  threaded through `run()` for a single consumer, producing an unreachable
  `None` arm with a runtime error. `prepare_operator` re-implemented `whoami`'s
  tenant resolution and special-cased one command's `--tenant`. Three separate
  implementations of "validate and persist operator.name" appeared. And
  `resolve_admin_username`, a query, acquired a persistence side effect on a
  path also reached by `aic logs key mint`.
- **Why missed:** First sighting; the slice was specified by prompt rather than
  grown from the code, and the prompt asked for a pre-flight "to prove it works
  end to end" without saying what it must cost when it does nothing.
- **Guard:** Standing check 1 above. The structural fix (pre-flight returns
  `()`, consumers resolve their own) removes three of the five at once.

### 2026-08-06 — personal identity in a shareable config file

- **What:** `.aic/settings.toml` gained an operator name/host. The
  `.aic/ .gitignore` that `aic` writes into user projects covers vault artifacts
  and `wraps.toml` but not `settings.toml`, while `config.toml` beside it is
  intentionally committable. A committed operator name makes `set_name_if_unset`
  a no-op for every teammate, so they never get prompted and their JWKS keys are
  named after whoever ran `aic` first — defeating the purpose of naming keys by
  owner.
- **Why missed:** First sighting. `encrypt_keys` had the same latent exposure
  before this change; nothing personal was stored, so it never mattered.
- **Guard:** Standing check 2 above.

### 2026-08-06 — the load-bearing requirement had no test

- **What:** "A missing operator name must never block or fail an agent" was the
  slice's one hard requirement and nothing asserted it. The decision sits inline
  in `prepare_operator` reading process-global state (`NO_PROMPT`, `isatty`), so
  it is not reachable from a unit test — unlike `should_prompt` beside it, which
  is pure and has five cases.
- **Why missed:** First sighting.
- **Guard:** Extract the decision into a pure
  `fn operator_decision(name_set: bool, prompting: bool) -> Decision` and
  table-test it, mirroring the existing `should_prompt` test. Not yet applied.
