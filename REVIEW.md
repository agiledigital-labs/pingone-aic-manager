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
- **Anything new written under `ProjectConfig::dir()` must appear in
  `gitignore_content()` — not merely be preceded by a `write_gitignore()`
  call.** `.aic/` is ignored in _this_ repo but not in user projects, where
  `config.toml` is deliberately shareable — anything per-person, per-machine or
  per-tenant that lands beside it will be committed and then silently applied to
  the whole team. Seen twice: `settings.toml` (2026-08-06) and `.aic/backups/`
  (2026-08-11, where the code did call `write_gitignore()` and the content it
  writes still covered nothing). _Guard: `gitignore_covers_every_artifact_stem`
  iterates `VaultArtifact::ALL`, so each non-artifact path needs its own
  assertion — `settings.toml` and `backups/` both have one now. Any future
  writer under `ProjectConfig::dir()` must add a third._
- **A `## Verified against` entry must record calls that support its
  conclusion.** Not merely calls that were made — check that the experiment
  described could actually distinguish the outcomes it claims to distinguish. An
  agent asked "does X differ from Y?" will often vary both at once, or hold the
  wrong one fixed, and then report the answer it expected. Read the listed calls
  as an experiment design, not as an activity log. _Guard: none automatable;
  this is a review judgement._
- **A `## Verified against` entry must record calls made in that run.** Figures
  quoted in a task prompt, copied from a neighbouring doc, or inferred from
  existing code are not verification, however true they happen to be — the block
  is the repo's audit trail, and a plausible-but-wrong claim stamped "verified"
  is invisible to every later reader. If the tooling fails, say so instead.
  _Guard: none obvious; wants `scripts/verify-endpoint.sh` to work so the honest
  path is also the easy one._

- **A write guard must be able to express the dangerous verb's outcome.** When a
  whole-document write takes caller-supplied expectations, check that the set of
  expressible conditions covers _removal_ and not only presence — a
  presence-only guard is satisfied by a no-op `PUT`, so the verb that can lock
  operators out is the one it fails to protect. Check too that the "expectations
  must be non-empty" guard does not forbid a legal document state (an empty rule
  list). _Guard: one test per verb asserting the confirmation fails against a
  document where the write was silently discarded._

- **Ask whether a test could fail if the code were wrong.** Seen three times now
  (`ops::rotate_steps`' ordering test, 2026-08-07; `access::spec`'s
  `actions_absent_and_empty_are_distinct_and_preserved`, 2026-08-11;
  `access::ops`' "remove addressed duplicate" row, 2026-08-11). The first two
  name the invariant in the test name and then assert a property of the language
  or of the fixture literal. The third is subtler and worth its own clause:
  **when a fix replaces a guessing algorithm, the regression test must use an
  input on which guess and truth differ.** Removing index 5 of a duplicate pair
  and removing index 4 yield the identical document, so the alignment-based
  derivation the fix deleted returns the right answer for index 5 and the wrong
  one for index 4 — and the test picked 5. _Guard: for each new test, name the
  edit to production code that would turn it red; for a regression test, state
  the wrong answer the old code gave for that exact input._

- **No cryptographic key generation in the default test path.** An RSA keygen is
  seconds, not milliseconds; one of them took this suite from 0.33s to 8.31s
  (2026-08-06). Test the shape of a key record against a stub, and gate any
  genuine end-to-end keygen behind `#[ignore]`. _Guard: **enforced**
  (2026-08-07) by two complementary checks —
  `repo_hygiene::no_direct_key_generation_under_cfg_test` in `src/lib.rs` greps
  for keygen calls under `#[cfg(test)]`, and a 3000ms wall-clock budget in
  `scripts/release-check.sh` catches the transitive case the grep cannot see._

- **A fix must not outgrow its finding.** When a cosmetic cleanup turns into a
  change to a shared protocol, storage format, or wire format, that is a finding
  in itself — the cost/benefit that justified the cleanup no longer applies, and
  the risk was never reviewed on its own merits. Ask what the smallest change
  that resolves the finding would have been. _Guard: none automatable; this is a
  review judgement._

- **A confirmation prompt must gate on `prompt_available()`, not
  `prompting_disabled()`.** `prompting_disabled()` only reads the `--no-prompt`
  flag. inquire's `NotTTY` is not a substitute: crossterm's `tty_fd` falls back
  to opening `/dev/tty` when stdin is not a terminal, so a command whose stdin
  is a pipe but which has a controlling terminal enables raw mode and **blocks
  on a keypress** — invisibly, if stderr is redirected.
  `should_prompt(no_prompt, stdin_tty, stderr_tty, tty_openable)` in
  `src/cli/mod.rs` is the repo's correct, table-tested predicate. _Guard: one
  lifted `cli::confirm_destructive(...)` helper plus a `repo_hygiene` grep test
  (same shape as `no_direct_key_generation_under_cfg_test`) asserting no
  `Confirm::new` outside `src/cli/` and `src/tui/`. Three copies exist today —
  `scripts/cli.rs`, `roles/cli.rs`, `access/cli.rs`._

- **`--dry-run` must remove the write capability, not branch around the write.**
  An `if dry_run { return Ok(()) }` placed before the write is an ordering
  convention that any later edit can invalidate, and it also drags the
  production `--yes` gate onto a command that writes nothing — which teaches
  operators to type `--dry-run --yes` on prod, one deleted word away from an
  unprompted write. _Guard: make the permission token (`WriteOk`) an `Option`
  that is `None` in dry-run, so the write path is unreachable by construction
  rather than by statement order._

- **After a CLI slice over a shared `spec`/`ops`, audit `cli.rs`'s private
  helpers.** Each one is either presentation (stays) or a property of the
  document (belongs in `ops`/`spec`, because the tab needs it too). Seen twice:
  the core slice missing `RoleIndex`/digest-address/`--if-digest` (2026-08-11),
  then the CLI slice keeping duplicate detection and the comma-list field
  predicate in `cli.rs` while `spec::validate_document` computed duplicates
  twice more (2026-08-11). _Guard: none automatable; ask of each private fn in a
  feature's `cli.rs` whether a TUI tab would need it._

- **Send the smallest credential that works.** Before a new transport helper
  attaches the service-account bearer, ask whether the call authenticates some
  other way — OAuth2 token endpoints authenticate by client credentials in the
  body and need no bearer. Copying an existing helper inherits its auth by
  default, and that reads as consistency rather than as new exposure. Related:
  the daemon must not be asked to send tenant credentials to a host outside
  `tenant.base_url`. _Guard: none yet — wants an origin assertion in
  `AicClient::url`._

- **A shared row projection must return values, not display strings.** When one
  `cells()`-style projection feeds both an auto-width CLI table and a
  fixed-percentage TUI table, the two have opposite width models: the wide one
  wants the word `dup`, the narrow one has room for one character, and the
  shared literal loses. Keep booleans and `Option`s on the shared summary and
  let each surface choose its glyph. _Guard: assert no marker literal is
  reachable from the shared projection; per-column minimum-content assertions
  rather than exact-width arrays._

- **A wire-format pin must serialise the type that is actually written to the
  socket.** `Request` travels inside `WireRequest<T>` via `#[serde(flatten)]`,
  and flattening is where the shape can change without any field changing — an
  internally-tagged _newtype_ variant reaches the flat map through serde's
  `TaggedSerializer`, a _struct_ variant does not. A test that serialises the
  inner enum proves nothing about the composition, and the failure mode is
  `to_vec` returning `Err` for every daemon call with a fully green suite.
  _Guard: assert on `WireRequest::current(...)` with `protocol_version` in the
  expected literal, so the shape pin and the version check are one test._

- **Lifting a shared line is not the same as removing the duplication.** When a
  finding says "lift X into the shared module and use it from both", check what
  stayed behind. If each caller now holds an identical wrapper around the lifted
  primitive, the duplication moved up a level and _grew_ — the shared home got
  the easy arithmetic and each feature kept the invariant. Ask which **type**
  should own the invariant, not which function should own the expression. Seen
  2026-08-12: `list_chrome::clamp_detail_scroll` (one line, shared) against a
  24-line `Cell`-plus-three-methods block copied verbatim into `access::State`
  and `oauth::State`. Count the call sites before believing "both" — there were
  three (`secretmap`), and the third was dead. _Guard: none automatable; a grep
  for identical method bodies across `src/*/state.rs` is conceivable._

- **A glyph, flag or column legend must be reachable from the mode that renders
  it.** `tui/keybind_help.rs::lines_for` routes `InputMode::Normal` to
  `normal_lines`, which renders `normal_binds` plus one hardcoded `View::Esvs`
  block and **never** consults a feature's `help_lines`. Anything a feature puts
  in `help_lines(SomeMode)` is invisible while browsing — and `?` inside a
  search mode is typed into the query rather than opening help, so a search-mode
  legend hides behind `/` then `F1`. _Guard: a test asserting every glyph a
  view's table can render appears in `keybind_help::lines_for` for that view
  under `InputMode::Normal`._

- **For every user-facing action a change adds, name the registration site.**
  `normal_binds` gates view hints on an `app.active_view == View::X` **bool**,
  not an exhaustive match, and `dispatch_normal` resolves acts from that same
  table — so an unlisted hint is an unreachable action, and omitting it compiles
  clean. Trace keystroke → `Act` → handler and say where each hop lives; the
  same applies to a CLI flag reaching its transform. Seen twice on one slice
  (2026-08-12). _Guard: blocked — the table test this wants cannot be written
  until `App` is constructible in tests; see 2026-08-13. Now also a
  `review-craft` principle, so it is prompted for in every repo, not just here._

- **A routing table keyed on an enum variant must either be exhaustive or be
  owned by the enum.** `UndoOp` is dispatched to its executor by **five**
  independent hand-written chains — `keymap::run_normal`'s `Undo` arm,
  `undo/screen.rs`'s `if managed / else if access / else Esv`, each feature's
  `execute_undo` type check, `esv::ops::apply_undo_entry`'s reject-guards, and
  `esv::ops::request_latest_undo`'s _absent_ filter. None is exhaustive, so
  adding a variant compiles clean with four of the five updated, and the fifth
  routes the new op to the ESV executor. _Guard:
  `impl UndoOp { fn executor(&self) -> UndoExecutor }` as one exhaustive match,
  with every chain deriving from it; then the compiler finds the fifth site._

- **An async result handler must not set `input_mode`.** Every other feature
  mutates `input_mode` only in a synchronous, key-driven path and lets the
  background result speak through a toast. `access::ops` sets it from
  `apply_write_result`/`set_draft_error`, so a write that completes while the
  operator has the selector, tenant picker or undo history open silently
  replaces their modal. _Guard: assert in review that a feature's
  `apply__*result`touches no`input_mode`; a `repo_hygiene`grep
  for`input_mode =`inside a function named`apply*__result` is conceivable._

- **When a second surface is added over an existing CLI, walk the CLI's safety
  defaults, not only its verbs.** Verb parity is easy to check and easy to
  advertise; the defaults that surround the verb are not. `aic access` writes a
  0600 backup before every write and validates against a live `RoleIndex`; the
  new Access tab does neither, so the surface the README now recommends is the
  weaker one. The pre-existing check about auditing `cli.rs`'s private helpers
  finds both (`backup_document`, `resolve_roles`) — it just has to be applied to
  the _new_ surface's absence rather than the old surface's duplication. _Guard:
  none automatable; enumerate the CLI's write-path steps and tick off each one
  against the tab._

## Findings log

### 2026-08-11 — `write_gitignore()` was called; the gitignore covered nothing

- **What:** `access::cli::backup_document` writes a tenant's whole authorization
  document to `.aic/backups/access-<tenant>-<UTC>.json` at mode 0600, and does
  call `ProjectConfig::write_gitignore()` first — but `gitignore_content()`
  emits only the vault stems, `wraps.toml`, `settings.toml`, `local-config/` and
  `*.log`. No `backups/`. In a user project, where `.aic/config.toml` is
  deliberately committable, `git add .aic` commits every backup.
- **Why missed:** the spec (`.ai/access-spec.md` §5.2) said "mode 0600, after
  `ProjectConfig::write_gitignore()`", and both the implementer and the review
  checked for the call. Standing check 2 named `settings.toml` specifically, so
  it read as satisfied. `.gitignore:39` ignores `.aic/` wholesale in this repo,
  so nothing local could surface it.
- **Guard:** standing check 2 broadened above; concretely, one assertion per
  non-artifact path under `ProjectConfig::dir()` in
  `gitignore_covers_every_artifact_stem`.

### 2026-08-11 — the prod gate fired on `--dry-run`

- **What:** `access::cli::write` calls `ensure_prod_confirmed` before it
  branches on `--dry-run`, so previewing a change on a production tenant
  requires `--yes` — the flag whose whole purpose is to skip the confirmation.
  The habit that builds (`--dry-run --yes`) is one deleted word from an
  unprompted prod write.
- **Why missed:** first sighting. The spec ordered the prod gate "before any
  fetch" and the review checked the order; nobody asked whether the gate applies
  to a verb that performs no write. A dry run is not a write, but it was
  implemented as a write that returns early.
- **Guard:** standing check above — represent dry-run as the absence of a
  `WriteOk`, which makes both the gate and the write inapplicable by
  construction.

### 2026-08-11 — a guard lifted into `cli` while a sibling guard stayed triplicated

- **What:** the slice correctly lifted `WriteOk`/`ensure_prod_confirmed` from
  `managed/cli.rs` to `cli/mod.rs`, then wrote a third near-verbatim copy of the
  inquire confirm helper (`scripts/cli.rs`, `roles/cli.rs`, `access/cli.rs`) —
  and all three gate on `prompting_disabled()` rather than the repo's
  table-tested `prompt_available()`, which is the one that catches the
  stdin-piped-but-`/dev/tty`-openable hang.
- **Why missed:** the prompt named the guard to lift (`WriteOk`) and the copy
  that got made was of the code immediately adjacent to it in the most recent
  sibling feature (`roles`, shipped one commit earlier). Copying the newest
  neighbour propagates its defects at the speed the codebase grows.
- **Guard:** standing checks above; the automatable half is the `repo_hygiene`
  grep test banning `Confirm::new` outside `src/cli/` and `src/tui/`.

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
  all route agents to this script as _the_ way to verify before documenting, so
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
  line _after_ the `exec` marker. Read the tool's own transcript format before
  concluding what it did or didn't run.
- **Guard:** Standing check 3 above.

### 2026-08-06 — RSA keygen in the default suite

- **What:** `src/jwtbearer/ops.rs` tested `generate_key` directly, doing a real
  2048-bit RSA keygen. The workspace suite went from 0.33s to 8.31s; the eight
  new tests alone took 4.53s, nearly all in that one test — which asserts only
  the _shape_ of the record (opaque kid, three `aic_*` members) and needs no
  real key at all.
- **Guard:** Standing check 4 above.

### 2026-08-06 — publish-before-store leaves an orphan in a shared key set

- **What:** `ops::setup` writes the public JWK into the realm's shared `jwkSet`
  and only then stores the private half locally. If the local store fails, the
  tenant carries a key with your name on it that nobody holds the private half
  for, and the next `setup` generates a fresh one rather than recovering — so
  the orphan is permanent, in a set the whole team shares.
- **Why missed:** first sighting. The prompt specified idempotence and a
  read-back check, which framed the risk as _concurrent writers_ rather than
  _partial failure of a two-store write_.
- **Guard:** order the writes so the recoverable side goes first — store
  locally, then publish; a failed publish self-heals on the next run. Worth a
  general rule: when a single operation writes to two stores, write first to
  whichever one makes a retry idempotent.

### 2026-08-06 — verifying a predicted defect downgraded it

- **What:** `spec::unwrap_inherited` only unwraps two-key `{inherited, value}`
  wrappers, and AM really does return `jwksUri` as a one-key
  `{"inherited": false}`. That looked like a second-run `PUT` sending a
  malformed field. A live round trip against a throwaway issuer returned 200
  with the value preserved — AM tolerates it. Reported as a note, not a bug.
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
  resident old daemon (verified live — the 5-day-old agent replied once and then
  closed, so a second request got `BrokenPipeError`), a documented deadlock
  guard was deleted along with the comment explaining it, and
  `handle_connection` now parks a task in `read_line` with no timeout where it
  previously did a single bounded read.
- **Why missed:** not missed — caught. Logged because the _shape_ recurs: an
  agent asked for ten fixes will size its solution to the file it is already
  editing rather than to the finding, and the largest blast radius came from the
  smallest item on the list.
- **Guard:** Standing check 5 above. Also worth noting the repo has no wire
  version handshake between CLI and daemon, so any future protocol change has
  this same failure mode; CLAUDE.md §8 warns about the resident binary but
  frames it as a testing inconvenience rather than an upgrade hazard.

### 2026-08-06 — a privileged credential sent where it is not needed

- **What:** `aic auth` mints a user token by POSTing an assertion plus
  `client_id`/`client_secret` to the OAuth2 token endpoint — an exchange that
  needs no bearer at all (verified: the whole flow succeeds with no
  `Authorization` header). The new `AicClient::write_form` attaches the
  service-account bearer anyway, so every `aic auth` ships a full
  `fr:am:* fr:idm:*` credential to an endpoint with no business seeing it.
  Alongside it, `AicClient::url()` was changed to forward absolute URLs
  verbatim, dropping the invariant that every proxied request stays under
  `tenant.base_url` — in a daemon that holds decrypted keys, that combination
  means the SA bearer can be addressed at an arbitrary host.
- **Why missed:** first sighting. The reviewer's habit is to ask whether an
  endpoint is _authenticated correctly_, not whether it is authenticated _more
  than necessary_. The transport helper was copied from `write`, and inheriting
  the bearer looked like consistency rather than a new exposure.
- **Guard:** promoted to Standing check 6.

### 2026-08-07 — the reviewer repeated the mistake he had just logged

- **What:** one day after adding the standing check about experiment design, the
  reviewer probed whether `key remove` actually revokes by relabelling the `kid`
  on **the same RSA key material** and re-minting. It minted, which proved
  nothing: AM falls back to trying every key in the set when the `kid` matches
  none, so the same material verifies under any label. A second attempt with
  genuinely fresh material also minted — but by then a third confounder (a ~20s
  propagation delay on freshly created OAuth2 clients, which returns
  `invalid_client`) was flipping results between runs, and the question was
  abandoned as unresolved rather than answered.
- **Why worth logging:** the check said "read the listed calls as an experiment
  design". It did not say _apply that to your own probes before running them_.
  Writing the rule down is not the same as internalising it, and the cost here
  was roughly a dozen live probe rounds that settled nothing.
- **Guard:** before a behavioural probe, state the hypothesis and what result
  would falsify it. If varying the intended input also varies something else
  (key material vs key _label_), the probe is not testing what it claims. And
  when results stop being reproducible, stop and report the confounders — do not
  keep adding cases.

### 2026-08-07 — a helper that existed only to be tautologically tested

- **What:** `ops::rotate_steps` wrapped `publish(); store(); remove();` behind
  three generic parameter pairs, forcing six `.clone()`s at its one call site.
  Its test asserted the three closures ran in the order they were passed — a
  property of `await?`, not of the code. `rotate` could have passed them in any
  order and the test would still pass, so the ordering the helper existed to
  protect was exactly what went unverified. Inlined the three calls with the
  reasoning in a comment; the sequence is now legible at the call site.
- **Why missed:** not missed — the done-note advertised it as "cleanly expressed
  through stubbed publish → store → remove steps", which reads as a testability
  win. A test that names the invariant is not the same as a test that holds it.
- **Guard:** when a prompt asks for an ordering guarantee, ask what the test
  would have to intercept to catch a reordering. If the answer is "nothing that
  exists", prefer a comment over an abstraction.

### 2026-08-07 — the right answer from an experiment that could not test it

- **What:** `aic auth`'s error mapper hinted "supply --client-secret-stdin" for
  a client missing the JWT-bearer grant, because AM answers `invalid_client` and
  the `unauthorized_client` branch that named the remedy never fires. The fix
  asked whether AM's `error_description` distinguishes the two causes. It does
  not — but the probe run to establish that created a client **without** the
  grant and then compared correct-secret against wrong-secret on it. Both fail
  for the same reason, so the comparison could not have distinguished anything.
  The doc nonetheless recorded "did not distinguish the two causes" in a
  `## Verified against` block. A controlled re-run (grant present + wrong secret
  vs grant absent + correct secret, on one client, with a grant-present +
  correct-secret positive control) confirmed the conclusion.
- **Why missed:** nearly accepted on the strength of being right. The done-note
  read as a clean negative result and the conclusion matched the reviewer's own
  expectation; only reading the listed calls as an experiment design exposed
  that the discriminating case was never run.
- **Guard:** Standing check 3 above, extended. Also worth stating in a prompt
  that asks "does X differ from Y?": name the control, not just the question.

### 2026-08-06 — the absolute URL was never required

- **What:** absolute-URL support existed only because the POST target was taken
  verbatim from discovery's `token_endpoint`, which carries an explicit `:443`.
  The `:443` is load-bearing for the **`aud` claim**, not for the request URL —
  during the original verification the POST went to the port-less path and AM
  processed it normally, failing only on `aud`. Taking `issuer` from discovery
  and building the POST as a tenant-relative path removes the SSRF surface at no
  cost.
- **Why worth logging:** a quirk that is real in one place ("read it from
  discovery, it has `:443`") got generalised to a place it did not apply. When a
  doc says a value must come from a specific source, check _which_ consumer of
  that value the requirement attaches to.

### 2026-08-11 — the read-back guard protected only the safe verbs

- **What:** `access::api::put_access_confirmed` takes `expect_rules: &[Value]`
  and confirms each is present after the `PUT`. `add` and `edit` are covered;
  `rm` is not — the rules it expects to see are the ones that survive, and a
  `PUT` the tenant silently discarded still shows all of them. The same
  signature also rejects an empty expectation set, so removing the last rule (or
  applying a backup with an empty `configs`) cannot go through the guarded path
  at all. `managed::api::ConfigConfirm` already had `ObjectAbsent` and
  `DocumentEquals`; the access version kept only the presence half of the
  precedent it was pointed at.
- **Why missed:** the spec's test list (`.ai/access-spec.md` §8) enumerated
  transform cases and never asked for a case on the confirmation itself, so
  neither the prompt nor the tests raised the question of what `rm` confirms.
  The prompt described the guard purely as "confirm every rule in `expect_rules`
  is present".
- **Guard:** Standing check above; concretely, replace the rule list with an
  `AccessConfirm` enum (or a single `DocumentEquals(intended)`, which the
  verified byte-identical read-back makes sound) and add a per-verb test that
  feeds the confirmation a document representing a discarded write.

### 2026-08-11 — the core slice shipped without the seams its callers need

- **What:** `src/access/{spec,ops}` are genuinely TUI-free, but three things
  every caller needs have no home in them: building the `RoleIndex` from
  `internal/role` + `config/authentication` (tenant I/O, so it belongs in
  `api.rs` and nothing in the repo reads `config/authentication` yet), matching
  an 8-char rule digest for `show <index-or-digest>`, and the `--if-digest`
  precondition check. `cli.rs` will hand-roll all three and the later TUI tab
  will hand-roll them again — the exact duplication the `spec`+`ops` split
  exists to prevent.
- **Why missed:** first sighting. The prompt listed the functions to write, and
  the list was complete with respect to itself; nothing asked "what will the
  next caller still have to write?". A spec that names the module seams should
  also name each published operation's owner, including the ones only the CLI
  slice will call.
- **Guard:** when reviewing a "core, CLI later" slice, walk the CLI surface in
  the spec (§7 here) verb by verb and ask which module each step lands in. Any
  step with no owner is a finding at core-review time, not at CLI-review time.

### 2026-08-12 — the duplication ran the other way

- **What:** the Access tab's `state::RuleRow::new` was character-for-character
  `cli::RuleEntry::new`, and the column header array was duplicated verbatim
  between `cli::print_rule_table` and `view::draw_table`. The shared primitives
  (`spec::RuleView`, `short_digest`, `ops::duplicate_flags`) were used correctly
  by both; what got rebuilt was the layer above them.
- **Why missed:** the standing check above asks of each private fn in a
  feature's `cli.rs` whether a TUI tab would need it — and it had been applied,
  twice. This is the inverse direction: the **tab** rebuilt a helper the CLI
  already had. The commit message then claimed the projection "come[s] from the
  existing `spec`/`ops` seams, which needed no new helper", which was true of
  the primitives and false of the projection.
- **Guard:** make the check symmetric — when adding the second surface over an
  existing feature, diff the new surface's row/summary construction against the
  existing one before writing it. Also: a commit message asserting that no new
  abstraction was needed is a claim to verify, not a note to skim.

### 2026-08-12 — the fix for silent clipping pinned a mangled marker

- **What:** D1's fix made every table column a `Percentage` and added ellipsis
  truncation. At 80 columns the 5% `DUP` column then rendered the literal
  `"dup"` as `d…` and its header `DUP` as `D…` — and a new test asserted `"d…"`
  as intended behaviour, so the regression shipped pinned.
- **Why missed:** the brief specified the constraint _kind_ (all `Percentage`)
  and the truncation helper, but never asked which cell contents must survive
  the narrowest column. Exact-width assertions record what the code does; they
  cannot express what the operator must still be able to read.
- **Guard:** per-column minimum-content assertions against named constants
  (`FLAGS` fits two glyphs, `#` fits two digits for 65 rules) instead of pinning
  width arrays the operator intends to tune by eye.

### 2026-08-12 — the sentinel fix redefined the haystack as the display cells

- **What:** D7 removed an `"<absent>"` sentinel from the fuzzy-search haystack,
  then rebuilt the haystack as the shared `cells()` output — admitting the
  marker literals `"dup"` and `"yes"` as undocumented match terms, and
  perturbing scores for unrelated queries. Net: one display string removed, two
  added.
- **Why missed:** the fix was specified as "omit the field when `None`", which
  it did. Nothing said the haystack must be built from _values_, so reusing the
  projection that had just been lifted looked like concept reuse.
- **Guard:** assert that no marker literal or glyph is a match term. Search
  semantics defined by display formatting change silently when a glyph changes.

### 2026-08-12 — the wire pin tested one level below the wire

- **What:** three literal-JSON tests were added specifically to prove
  `Request::ApiCall` still serialises to today's object after becoming a newtype
  variant. All three serialised a bare `Request`; the socket carries
  `WireRequest<Request>` with `#[serde(flatten)]`. The only pre-existing
  `WireRequest` test uses `Ping`, a unit variant, which exercises none of the
  relevant serde machinery. Had the composition not worked, `to_vec` would have
  returned `Err` on every daemon call and all 528 tests would still have passed.
- **Why missed:** the brief named the risk correctly — "serde flattens a newtype
  variant" — but described the flattening done by the _enum tag_, not the
  `#[serde(flatten)]` field one level up. Implementer and brief-author were both
  looking at the right mechanism in the wrong place.
- **Guard:** the standing check above.

### 2026-08-12 — the clamp was reviewed; the keybind was never registered

- **What:** a review filed a finding against
  `access::State::clamp_detail_scroll`'s over-scroll behaviour — "five `^D`
  presses leave `detail_scroll == 50`". On the Access tab that could not happen:
  `normal_binds`' chain read `else if oauth_view && n > 0`, `access_view`
  appeared nowhere else in it, and `dispatch_normal` resolves acts from that
  same table, so `^D`/`^U` were unbound and the whole clamp was dead code. The
  real defect was a missing registration, not a wrong bound.
- **Why missed:** §9 of `CLAUDE.md` warns that `normal_binds` is the one site
  the compiler cannot check, and the reviewer read that as "check the hints are
  listed" rather than "check the action is reachable". A dead code path is
  exactly what lets you verify arithmetic in isolation and feel finished.
- **Guard:** for any new view-specific `Act`, a table test over
  `(View, KeyEvent) -> Option<Act>` asserting `dispatch_normal` resolves it.
  That catches both directions — an unbound act, and an act bound on the wrong
  view.

### 2026-08-12 — 544 tests, two reviews, and a feature nothing could reach

- **What:** the same slice shipped a `FLAGS` column whose legend lived in
  `help_lines(Mode::Search)`, unreachable from the normal mode that renders the
  glyphs. Combined with the entry above: two separate user-facing paths in one
  feature were unreachable while every gate was green.
- **Why missed:** fmt, clippy `-D warnings`, 544 tests and two human reviews all
  test the **code**; none tests the **wiring**. Reachability has no gate here.
- **Guard:** for any change adding a user-facing action or symbol, trace the
  path from keystroke or CLI argument to the new code and name the registration
  site in the review. Proposed as a `review-craft` principle, not just a repo
  check.

### 2026-08-13 — the reachability guard is blocked on `App` not being constructible

- **What:** the guard proposed the day before — a table test over
  `(View, KeyEvent) -> Option<Act>` driving `dispatch_normal` — cannot be
  written today. `normal_binds` and `dispatch_normal` both take `&App`, and
  `App::new()` loads `ProjectConfig`, `Settings`, `WrapsFile` and the undo
  `DiskLog`, then sweeps it for expiry. There is no `Default`, no test
  constructor, and `src/app/keymap.rs` has no `mod tests` at all — which is why
  the one file `CLAUDE.md` §9 says the compiler cannot check for you is also the
  one file with no tests.
- **Why missed:** the guard was proposed from the shape of the functions without
  checking that their argument could be built. A guard that cannot be
  implemented reads identically to one nobody has got round to.
- **Guard:** the structural prerequisite is a `#[cfg(test)] fn App::for_test()`
  that skips every disk load — empty config, empty settings, an in-memory
  `UndoLog` (the trait is already boxed, so this is cheap). Until that exists,
  reachability stays a review judgement, and the honest statement is that it is
  unguarded rather than pending. Slice 5b was checked by hand: every new Access
  key sits in the `access_view` branch of `normal_binds`, with `^N` outside the
  `n > 0` guard so a rule can be created into an empty list.

### 2026-08-13 — a limit that is too small looks exactly like a working scroll

- **What:** `secretmap/view.rs` rendered its detail pane with
  `Wrap { trim: false }` but passed `lines.len()` to `DetailScroll::clamp`, so
  the limit was computed against unwrapped rows and the pane stopped short of
  its own content. Nothing failed, because a too-small scroll limit is
  indistinguishable from having reached the end.
- **Why missed:** the review that moved `wrap_lines` into `list_chrome` noted
  the under-count and called fixing it optional. It was the only caller of the
  shared clamp whose height was wrong, so "optional" left the shared type with a
  precondition one of its three callers violated.
- **Guard:** applied — `list_chrome::wrapped_height` measures what the widget
  will produce, and its doc comment states which of the two shapes a caller is,
  since `lines.len()` is correct for a pane that pre-wraps and wrong for one
  that lets the widget wrap. Generally: **when a shared type takes a measurement
  it cannot verify, name the two ways of producing it.** A silent wrong answer
  needs the naming more than a loud one does.

### 2026-08-13 — `^Z` on the wrong tab burns the Access undo entry

- **What:** slice 5b added `UndoOp::AccessConfigReplace` and, with it, a third
  reject-guard in `esv::ops::apply_undo_entry` returning
  `UndoFailure::Failed("Access undo must be applied from the Access tab…")`. But
  `esv::ops::apply_undo_result` treats `Failed` as a real failure and calls
  `mark_applied(id, AppliedFailure)`. `esv::ops::request_latest_undo` uses
  `latest_pending(tenant)`, which filters on tenant/status/capability and
  **not** on op kind — so the sequence "edit an Access rule → Ctrl-P → ESVs →
  `^Z`" retires the Access entry. `access::ops::request_latest_undo` and the
  history overlay both require `Pending`, so the change becomes permanently
  un-undoable — moments after a toast that says "Press ^Z to undo." The same
  hole exists for `ManagedObjectReplace` and `SecretMappingReplace`; this slice
  widened it to the document that can lock operators out.
- **Why missed:** the guard reads as defensive, and it is — the mistake is
  downstream, in the shared handler that cannot tell "this executor does not own
  this op" from "the tenant refused the write". Reviewing the guard in the file
  it was added to never brings the retirement policy into view. The reachability
  check that was applied to slice 5b's _keys_ was not applied to its _undo
  entry_: `^Z` is registered on three views and only one of them filters.
- **Guard:** two, both wanted. Narrow: `request_latest_undo` must filter by op
  kind, as `access`/`managed`/`secretmap` already do — then the reject-guard is
  unreachable rather than load-bearing. Structural: the new standing check above
  (`UndoOp::executor()`), plus a routing rejection must not be expressible as
  `UndoFailure::Failed`. A test is cheap once either lands: record an
  `AccessConfigReplace` entry in a `MemoryLog`, run the ESV undo path, assert
  the entry is still `Pending`.

### 2026-08-13 — verb parity advertised, safety defaults dropped

- **What:** the Access tab reached verb parity with `aic access add/edit/rm` and
  the README was updated to say so — but the tab writes no backup (the CLI
  writes `.aic/backups/access-<tenant>-<UTC>.json` unless `--no-backup`) and
  validates with `known_roles: None`, discarding every warning the CLI prints:
  unknown role reference, unrecognised method, byte-identical duplicate,
  "customAuthz can only deny". A typo'd method now creates a silently dead rule
  from the surface the README recommends. `docs/CLI.md` still says "the undo log
  is TUI-only — the backup file is the entire safety net here", which was a
  complete statement when the TUI could not write and is now half of one from
  either side.
- **Why missed:** first sighting of this direction. The existing check ("audit
  `cli.rs`'s private helpers — each is presentation or a property of the
  document") finds both `backup_document` and `resolve_roles` immediately; it
  was read as a check on the _old_ surface duplicating, not on the _new_ surface
  omitting. The commit message and the doc updates both frame the slice as verb
  parity, which is the axis on which it is complete.
- **Guard:** standing check above. Note that lifting `backup_document` into
  `ops`/`api` needs no gitignore work — `backups/` is already covered since
  2026-08-11.

### 2026-08-13 — the invariant went into a free function again, one day later

- **What:** the 2026-08-12 lesson was "ask which **type** should own the
  invariant, not which function should own the expression", filed against a
  one-line shared `clamp_detail_scroll` with a 24-line wrapper copied into two
  callers. The `wrapped_height` fix the next day put the measurement in a new
  `pub fn` in `list_chrome` for exactly one caller, leaving
  `DetailScroll::clamp` still accepting an unverifiable `usize` and still
  documenting its precondition in prose. Separately, the doc comment claims the
  function returns "rows a paragraph will occupy after ratatui wraps it", but it
  measures with our `wrap_lines`, which re-indents continuations where ratatui
  does not — so the two disagree for any indented line, in the over-counting
  direction.
- **Why worth logging:** the fix is an improvement and the entry above records
  it as such. What recurred is the shape: a precondition a caller can get wrong
  was answered with a helper plus documentation rather than with an API that
  cannot be called wrongly.
  `DetailScroll::clamp_wrapping(&[Line], width, viewport)` removes the choice;
  `ratatui`'s own `Paragraph::line_count` (0.30, behind
  `unstable-rendered-line-info`) removes the approximation.
- **Guard:** **applied** in `c5d33e1` — `wrapped_height` is private and reached
  only through `DetailScroll::clamp_wrapping(&[Line], width, viewport)`, so a
  wrapping caller cannot supply the measurement or pair it with the wrong
  viewport, and the doc comment now states that the height is an estimate and
  which direction it errs. `ratatui`'s `Paragraph::line_count` (0.30, behind
  `unstable-rendered-line-info`) would remove the approximation entirely; not
  worth an unstable feature today. Generally: when a review's own fix lands in
  the same category as the finding it closes, say so in the commit.
