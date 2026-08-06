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

- **No cryptographic key generation in the default test path.** An RSA keygen
  is seconds, not milliseconds; one of them took this suite from 0.33s to 8.31s
  (2026-08-06). Test the shape of a key record against a stub, and gate any
  genuine end-to-end keygen behind `#[ignore]`. _Guard: none yet — wants a
  wall-clock budget assertion, or a grep for `generate_rsa` under `#[cfg(test)]`._

- **A fix must not outgrow its finding.** When a cosmetic cleanup turns into a
  change to a shared protocol, storage format, or wire format, that is a
  finding in itself — the cost/benefit that justified the cleanup no longer
  applies, and the risk was never reviewed on its own merits. Ask what the
  smallest change that resolves the finding would have been. _Guard: none
  automatable; this is a review judgement._

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

### 2026-08-06 — RSA keygen in the default suite

- **What:** `src/jwtbearer/ops.rs` tested `generate_key` directly, doing a real
  2048-bit RSA keygen. The workspace suite went from 0.33s to 8.31s; the eight
  new tests alone took 4.53s, nearly all in that one test — which asserts only
  the *shape* of the record (opaque kid, three `aic_*` members) and needs no
  real key at all.
- **Guard:** Standing check 4 above.

### 2026-08-06 — publish-before-store leaves an orphan in a shared key set

- **What:** `ops::setup` writes the public JWK into the realm's shared `jwkSet`
  and only then stores the private half locally. If the local store fails, the
  tenant carries a key with your name on it that nobody holds the private half
  for, and the next `setup` generates a fresh one rather than recovering — so
  the orphan is permanent, in a set the whole team shares.
- **Why missed:** first sighting. The prompt specified idempotence and a
  read-back check, which framed the risk as *concurrent writers* rather than
  *partial failure of a two-store write*.
- **Guard:** order the writes so the recoverable side goes first — store
  locally, then publish; a failed publish self-heals on the next run. Worth a
  general rule: when a single operation writes to two stores, write first to
  whichever one makes a retry idempotent.

### 2026-08-06 — verifying a predicted defect downgraded it

- **What:** `spec::unwrap_inherited` only unwraps two-key `{inherited, value}`
  wrappers, and AM really does return `jwksUri` as a one-key `{"inherited":
  false}`. That looked like a second-run `PUT` sending a malformed field. A live
  round trip against a throwaway issuer returned 200 with the value preserved —
  AM tolerates it. Reported as a note, not a bug.
- **Why worth logging:** the reviewer's instinct was right about the shape and
  wrong about the severity. Predicting a failure from reading code is cheap;
  confirming it against the live API is also cheap here, and the difference
  between "broken on every second run" and "cosmetic" is the difference between
  blocking a commit and not.

### 2026-08-06 — a cosmetic nit became an unversioned protocol change

- **What:** the review asked that `ops::setup` stop calling
  `AgentClient::connect_or_spawn()` twice — the smallest of ten findings, worth
  microseconds on a local Unix socket. The fix changed the daemon from
  one-request-per-connection to a loop, added an `*_on_connection` variant of
  every secret verb, and moved `send()`'s socket shutdown from before the
  response read to after. Three consequences: a new CLI cannot talk to a
  resident old daemon (verified live — the 5-day-old agent replied once and
  then closed, so a second request got `BrokenPipeError`), a documented
  deadlock guard was deleted along with the comment explaining it, and
  `handle_connection` now parks a task in `read_line` with no timeout where it
  previously did a single bounded read.
- **Why missed:** not missed — caught. Logged because the *shape* recurs: an
  agent asked for ten fixes will size its solution to the file it is already
  editing rather than to the finding, and the largest blast radius came from
  the smallest item on the list.
- **Guard:** Standing check 5 above. Also worth noting the repo has no wire
  version handshake between CLI and daemon, so any future protocol change has
  this same failure mode; CLAUDE.md §8 warns about the resident binary but
  frames it as a testing inconvenience rather than an upgrade hazard.
