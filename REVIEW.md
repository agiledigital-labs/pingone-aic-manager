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
- **A `## Verified against` entry must record calls made in that run.** Figures
  quoted in a task prompt, copied from a neighbouring doc, or inferred from
  existing code are not verification, however true they happen to be — the block
  is the repo's audit trail, and a plausible-but-wrong claim stamped "verified"
  is invisible to every later reader. If the tooling fails, say so instead.
  _Guard: none obvious; wants `scripts/verify-endpoint.sh` to work so the honest
  path is also the easy one._

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

### 2026-08-06 — the documented verification tool is broken

- **What:** `scripts/verify-endpoint.sh` exits immediately with
  `error: SERVICE_ACCOUNT_ID is not set (check .envrc)`. `.envrc` defines
  `TENANT_BASE_URL`, `ORIGIN`, `API_KEY_ID`, `API_KEY_SECRET`, `REALMS` and
  `AGENT_PASSWORD` — no `SERVICE_ACCOUNT_ID`, no JWK. CLAUDE.md §2, §7 and §10
  all route agents to this script as *the* way to verify before documenting, so
  every agent that tries to follow the rule hits a wall and then either gives up
  or documents from inference.
- **Why missed:** first sighting. Humans reach for `aic whoami --token`; only an
  agent following the written instruction finds the breakage.
- **Guard:** fix the script to mint via the agent (`aic whoami --token`) rather
  than signing its own assertion, or update CLAUDE.md to name the working path.
  Not yet applied.

### 2026-08-06 — verified figures laundered through a prompt

- **What:** the `aic oauth create` slice added a dated "Verified against" entry
  to `docs/api/05-oauth2-oidc.md` asserting that `?_action=template` and
  `?_action=schema` "returned 200" with specific field counts. The agent had
  attempted verification, been blocked by the broken script above, and taken the
  figures from the task prompt instead. The content is correct — the calls were
  genuinely made earlier the same day, by the reviewer — but the run that wrote
  the entry established none of it.
- **Why missed:** nearly mis-reported in the other direction. A first grep for
  `curl|verify-endpoint` over the agent log matched only documentation text and
  suggested no call had been attempted at all; the actual invocation was on the
  line *after* the `exec` marker. Read the tool's own transcript format before
  concluding what it did or didn't run.
- **Guard:** Standing check 3 above.
