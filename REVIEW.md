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
  assertion — `settings.toml`, `backups/` and `saml-rotations.json` all have one
  now._ **Seen a third time on 2026-09-18** (`.aic/saml-rotations.json`), which
  retires the "any future writer must add another assertion" instruction: an
  instruction to remember something is not a guard against forgetting it, and
  this one has now failed three times for three different authors. The check
  stays, but it is owed the structural fix in the 2026-09-18 findings entry —
  until a writer cannot exist without a gitignore line, expect a fourth.
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
- **When one collection is auto-discovered and a second, hand-maintained list
  validates it, assert the two are equal — and assert it the way the discoverer
  discovers.** A runner that globs its inputs and a Rust test that checks them
  against a literal array are two registries that drift in one direction only:
  the input nobody registered still runs, and silently skips every assertion the
  list exists to make. Seen 2026-09-15 with `scripts/type-tests/leaves/` —
  `run.sh` globbed `leaves/*/` while
  `type_test_leaf_manifests_are_subsets_of_the_real_leaf_configs` iterated a
  `[(&str, String); 5]`, so a sixth leaf would compile in CI and prove nothing.
  The equality assertion is the guard; the second half is that the two must
  agree on what an input _is_ (see the symlink divergence in the log below).
  _Guard: **applied** for the type-test leaves; ask it of any new
  discovery-plus-registry pair._

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
  (2026-08-12). _Guard: **applied** in `2032700` — `App::for_test` unblocked it,
  and `keymap.rs`'s table over `(View, KeyEvent) -> Option<Act>` now asserts
  both directions for every view. It found a third instance the same day
  (`0d75f96`). Also a `review-craft` principle, so it is prompted for in every
  repo._

- **A selection guard is the wrong home for an action about the selection's
  absence.** Managed registered `^Z`/`^Y` inside its `n > 0` branch, so deleting
  the last field left no way to undo the deletion that emptied the list. The
  guard reads as obviously right — an action needing a selected row must not be
  advertised without one — which is why undo slipped inside it on one tab and
  not the other two. When reviewing a `n > 0` block, ask of each binding whether
  it acts on the selection or on the list's history (2026-08-13).

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

- **A new permission needs a test that it is HONOURED, not only that it
  parses.** `--force=backup` shipped with six parsing tests and zero behaviour
  tests: every test call site of `install_pull_at` / `install_pull_entry` /
  `install_remote` / `PullPlan::install` passes `skip_backup = false`, so
  inverting `if !skip_backup` in all four leaves the suite green (2026-09-11). A
  parser change is reviewed as a parser; ask separately what the parsed value
  makes the program do differently, and test that. _Guard: one test per surface
  driving the permission's non-default value and asserting the changed effect._

- **Apply "could this test fail if the code were wrong?" to the batch's FEATURE
  code, not only its safety code.** Fourth sighting (2026-09-11,
  `consecutive_tail_windows_share_exactly_one_boundary`), and the first where
  the check was demonstrably applied to the guard code in the same batch and
  skipped on the feature shipped beside it. The tell is a test whose assertions
  restate the body's arithmetic while its _name_ claims an invariant about the
  outside world. _Guard: for each new test, name the production edit that turns
  it red; if that edit is "delete this line", the test is a change-detector._

- **Directory traversal that deletes must select with `entry.file_type()`, never
  `Path::is_dir()`.** `Path::is_dir()` follows symlinks, so a symlinked child
  directory hands a pruner a target outside the tree it was told to manage —
  `src/scripts/workspace.rs:810` deletes files on the far side (2026-09-11).
  Reviewing the match predicate is not enough; review how candidates are
  _selected_. _Guard: a test that a symlinked child directory is skipped, in
  every walker that removes files._

- **A batch operation must be all-or-nothing, or must report which entries
  landed.** `PullPlan::install` and policy's install loop run sequentially and
  `collect()` into a `Result`, so a failure part-way leaves earlier entries
  written and their snapshots advanced while the surface prints a failure
  (2026-09-11; raised in a review round and lost between rounds). Preflighting
  the whole batch makes the _consent_ atomic and is often mistaken for making
  the _application_ atomic. _Guard: a test that fails entry N of a batch and
  asserts either nothing was installed or the outcome names entries 1..N-1 as
  installed._

- **When a fix lands on one path, grep for its siblings before closing.** The
  check/use race and the batch-atomicity defect were fixed in `policy/cli.rs`
  and left in `scripts/sync.rs` in the same commit range (2026-09-11). The
  `codex-review` skill's highest-yield question — "where else does this shape
  appear?" — was not asked. _Guard: none automatable; ask it once per fixed
  defect class, and keep an open/closed ledger across review rounds rather than
  re-deriving it from the most recent brief._

- **The doc comment on a matcher must say what it matches ON.**
  `prune_if_generated_only` says it deletes "known `extra_files` output", which
  reads as content-derived; it is name-only for `tsconfig.json` and byte-exact
  only for `.js` wrappers. That comment misled the author of the change itself,
  who built a synthetic probe on the byte-exact reading and then spent effort
  explaining a non-result (2026-09-11). _Guard: none automatable; when a
  predicate mixes name-based and content-based arms, the comment must name which
  is which._

- **A promotion or cleanup hook must hang off every path that can reach the
  state, not only the one you were editing.** Deferring a modal out of an async
  result handler correctly put the promotion at the end of `apply_event` — and
  the operator's way out of Search is a **key**, which routes through
  `keymap::dispatch`, not `apply_event`. Once the event queue drains, the
  pending pull waits forever and the "waiting for confirmation" toast becomes a
  lie. _Guard: a test that drives the real key (`keymap::dispatch` with `Esc`)
  rather than assigning `input_mode` by hand — the hand-assigned version of
  this test passed against the broken code._

- **Before reusing a `reset_*` helper for a narrower action, read what else it
  drops.** `ScriptsState::reset_view` clears the filter _and_ cancels a pending
  pull, which is right for a tenant switch and wrong for `Esc` in Search — so
  clearing a filter silently discarded a protected-pull confirmation the
  operator had just been told about. The bug only became reachable when an
  earlier fix stopped opening the modal immediately, which is the general
  shape: **a fix that makes state live longer makes every existing consumer of
  that state a new call site.** _Guard: split the narrow action out
  (`clear_filter`) so the wide one cannot be reached by accident; one test per
  caller asserting what survives._

- **Project paths must be rooted once, not re-resolved from the process cwd per
  syscall.** A relative `ProjectConfig::dir()` made
  `create_dir_all(Self::dir())` followed by `write(Self::dir().join(..))` two
  independent resolutions. The daemon tests moved the process cwd between
  them; `create_dir_all` accepts mkdir's `EEXIST` only while `.is_dir()` still
  holds, so the swap surfaced as an `AlreadyExists` (or `NotFound`) error from a
  call that logically cannot produce one — a rare panic in an unrelated test
  under load (2026-09-11). `ProjectPaths` now owns one absolute root, production
  installs an immutable process default at bootstrap, and tests carry explicit
  instances. _Guard: `repo_hygiene::no_process_cwd_mutation_under_cfg_test`
  rejects `set_current_dir` below any `#[cfg(test)]` boundary._

- **A constraint only a third party can violate will not be found by a run that
  chooses its own inputs.** When a verification pass probes an API by hand, the
  prober picks every value, and picks them sensibly — so a rule about the
  *shape* of an input is silently never under test. `docs/api/06-saml.md` said
  `secretIdIdentifier` was "any string" and survived three verification passes
  saying so; all three chose alphanumeric identifiers (`aicrot1`, `aicrot2`,
  `probe3key`) because that is what a person types. AM in fact rejects `-` and
  `_` with a 400. It surfaced only when a CLI let someone else choose, on
  2026-09-18. So when a doc records that a field accepts a class of value, ask
  which member of that class the run actually sent — and if the answer is "the
  obvious one, every time", the claim covers one example, not the class.
  _Guard: a `## Verified against` row asserting permissiveness ("any string",
  "any length", "any encoding") must cite a deliberately awkward value, or say
  it was not tested. Nothing automated can catch this; it is a question to ask
  of the evidence table._

- **A count is not an identity.** When a check measures how many of something
  exist before and after an operation, ask which *distinct* outcomes produce the
  same count. Add/remove pairs, rotations and swaps are the usual offenders: the
  cardinality returns to where it started whether the right element or the wrong
  one was removed. Assert *which* element survived, not how many. Doubly so when
  the discriminating fact is already recorded in a doc or a done-note — prose
  that states the invariant is evidence the author knew it, and evidence the
  guard does not encode it. _Guard: none automatable; this is a review
  judgement._

## Findings log

### 2026-09-18 — the standing check that asked the author to remember

- **What:** `aic saml rotate` writes `.aic/saml-rotations.json` — tenant name,
  realms, entity ids and certificate fingerprints — and it was absent from
  `ProjectConfig::gitignore_content()`, while `journal.rs`'s own module header
  claimed it was covered. This repo's top-level `.gitignore` has `.aic/`, so the
  gap is invisible here; it opens in a scaffolded user project, which is exactly
  the case the existing standing check describes.
- **Why missed:** it was not missed for lack of a rule — the standing check has
  existed since 2026-08-11, names the hazard precisely, and closes by instructing
  the next author to add an assertion. That instruction is the defect. The guard
  (`gitignore_covers_every_artifact_stem`) iterates `VaultArtifact::ALL` and
  then hand-asserts each non-artifact path, so it is structurally incapable of
  failing for a path nobody thought to add — it tests the assertions someone
  remembered to write, not the files the program writes. Three authors have now
  walked past it. An external reviewer found this one; the repo's own review did
  not, because a reviewer checking "does the diff violate a standing check?"
  reads the diff, and the absence of a line in a distant function is not in it.
- **Guard:** make the writer and the ignore list share one source of truth, the
  way `VaultArtifact::ALL` already does for the vault. A `RuntimeFile` enum
  naming every non-artifact path written under `ProjectConfig::dir()`, with
  `journal::path()` and its siblings deriving their filename from it and both
  `gitignore_content()` and the test iterating it, makes a writer without a
  gitignore line fail to compile rather than fail to be noticed. **Not yet
  applied** — it reaches outside the SAML vertical and is queued as its own
  slice.

### 2026-09-16 — a harness that counts instead of identifying

- **What:** `scripts/saml-harness/harness.sh`'s `verify-rotate` asserted only
  that signing `KeyDescriptor`s went `1 -> 2 -> 1`. `rotate-add` inserts at the
  highest priority and `rotate-rm` deletes the lowest, so a bug that deleted the
  *new* provider instead of the *old* one yields the identical count sequence
  and passes. The harness would report a completed certificate rotation having
  performed a no-op rollback — and producing a genuinely rotated cert is the
  whole reason it exists.
- **Why missed:** the review that mattered was the author's own, and the
  discriminating fact had already been measured and written down in prose
  (`docs/saml-test-harness.md`: "After `rotate-rm` the remaining `KeyName` is
  the **new** kid. **Measured.**"). Knowing a fact and asserting it are
  different acts, and a done-note that quotes the measurement reads exactly like
  a guard that encodes it.
- **Guard:** have `cmd_rotate_add` capture the added provider's kid and have
  `cmd_verify_rotate` assert the surviving `<ds:KeyName>` equals it and differs
  from the pre-rotation kid. Not applied — reported to the author.


### 2026-09-11 — the deferred modal that never arrived

- **What:** the fix for "an async result handler must not set `input_mode`"
  stored the pull and promoted it from the end of `apply_event`. Leaving Search
  is a key, which never reaches `apply_event`; and `Esc` in Search called
  `reset_view`, which _discarded_ the pending pull outright. So the standing
  check was satisfied and the feature was broken two different ways.
- **Why missed:** the test asserted the promotion by setting
  `input_mode = Normal` in the test body and calling the promotion function
  directly — it proved the predicate, never the wiring. Same shape as the
  repo's existing "could this test fail if the code were wrong?" check, pointed
  at a **reachability** claim rather than a value.
- **Guard:** applied — both features now have a `#[tokio::test]` driving
  `keymap::dispatch` with the real `Esc`, and both go red against either
  regression (verified by mutation).

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
- **Resolved the same day** in `2032700`. `App::for_test` shares `from_parts`
  with `App::new`, so the two cannot drift into different initial states, and
  the table now covers every `View::all()` variant, all three ESV sub-views, and
  populated versus empty lists. Two things worth keeping from how it went:
  - **The test had to call the production lookup, not a copy of it.**
    `dispatch_normal` had its own inlined `find` over `normal_binds`; the slice
    repointed it at the `Bind::resolve` the test calls. A table asserting
    against a reimplemented lookup proves the reimplementation, which is the
    failure mode this whole entry is about, one level up.
  - **The guard paid for itself immediately**, finding Managed's undo bindings
    gated on a non-empty list (`0d75f96`). A guard's first run is the cheapest
    time it will ever find anything; writing it and not reading its output
    carefully wastes the only free hit.

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

### 2026-09-11 — a follow-the-logs command that drops the events it exists to show

- **What:** `logs tail` (`src/logs/cli.rs:912-921`, loop at `938`) advances
  `TailCursor::next_begin` to `end` unconditionally, before the fetch, and never
  revisits a closed interval. The log API window is `(beginTime, endTime]` and
  ingestion lags — a fact the **same file** states in `incomplete_tx_note`
  ("logs can lag tens of seconds behind the request") and that `logs sync`
  already handles by rewinding five minutes and relying on insert dedup. Any
  event whose timestamp falls in a window that was queried before the event was
  ingested is never seen again.
- **Why missed:** the review checked that consecutive windows were contiguous,
  which is exactly what the test asserts and exactly the wrong property. Nobody
  asked what the _server_ does between the two polls. The repo's own
  contradicting knowledge sat 60 lines below the defect.
- **Guard:** a test driving `TailCursor` with an injected clock and an injected
  fetch that returns an event stamped inside window N only on poll N+1,
  asserting it is still delivered. More durably: `logs tail` should reuse the
  overlap-and-dedupe windowing in `src/logs/ops.rs` rather than growing a
  second, weaker one.

### 2026-09-11 — the test named the invariant, asserted the arithmetic (4th)

- **What:** `consecutive_tail_windows_share_exactly_one_boundary`
  (`src/logs/cli.rs:1283`) asserts `next_window(t1) == (t0, t1)` then
  `next_window(t2) == (t1, t2)`. That is a restatement of the two-line body; it
  can only fail if the assignment is deleted. It was described in the session
  notes as "a windowing-correctness unit test proving no dropped/duplicated
  events across polls", and it proves neither.
- **Why missed:** the standing check exists and has three prior sightings, but
  it was applied to the _new safety_ code (protected pull, force parsing,
  backups) and not to the _new feature_ code shipped in the same batch. The
  check reads as being about guards; it is about tests.
- **Guard:** the standing check below now names feature code explicitly. The
  concrete guard for this instance is the injected-clock test above.

### 2026-09-11 — a new flag's parsing has six tests and its behaviour has none

- **What:** `--force=backup` was added across four surfaces. Its _parsing_ is
  covered six ways (`src/cli/force.rs`, plus
  `*_parses_operation_and_backup_permissions_independently` in oauth, journey
  and policy). Its _effect_ — not writing a backup — is covered nowhere: every
  test call site of `install_pull_at` / `install_pull_entry` / `install_remote`
  / `PullPlan::install` passes `skip_backup = false`. Inverting
  `if !skip_backup` in all four leaves the whole suite green.
- **Why missed:** the flag was reviewed as a _parser_ change, and the parser was
  reviewed thoroughly. The review rounds asked whether the guard could be
  requested, never whether requesting it did anything.
- **Guard:** one `install(..., true)` test per surface asserting the file is
  replaced and no backup exists. Generally: for every new permission, one test
  that the permission is _honoured_, not only that it parses.

### 2026-09-11 — the pruner follows a directory symlink out of the workspace

- **What:** `prune_child_dirs` (`src/scripts/workspace.rs:810`) selects children
  with `path.is_dir()`, which traverses symlinks. `prune_if_generated_only` then
  `read_dir`s the _target_ and `remove_file`s every entry that matches the
  generated-scaffold predicate. A symlink at `am/<realm>/foo` pointing outside
  the workspace has files deleted on the far side. `DirEntry::file_type()` is
  used for the folder's _contents_ (correctly, it does not follow), so only the
  directory itself is the hole.
- **Why missed:** first sighting for this repo. The review of the pruner
  concentrated on the match predicate — which is careful, fails closed on a
  partial listing, and is well tested for user content — and not on how
  candidates are selected.
- **Guard:** `entry.file_type()?.is_dir()` at the selection site, plus a test
  that a symlinked child directory is skipped. The general rule is in the
  standing check below.

### 2026-09-11 — raised in round 2, declared closed in round 3, still open

- **What:** the T14 stage-4 review loop ran four rounds and ended "Clean … no
  fourth review round is needed." Round 2 had reported that `PullPlan::install`
  installs sequentially with `collect()`, so a failure on entry N leaves entries
  1..N-1 installed with snapshots advanced while the surface reports the pull as
  failed; and that `install_remote` re-reads local bytes for the backup without
  comparing them to the preflight bytes. The round-3 brief listed only the three
  items from the round-2 _follow-up_, and round 3 answered against that narrowed
  set. Both defects are still in `src/scripts/sync.rs:169-180` and `1076-1102`;
  the equivalent policy defect _was_ fixed, so the batch shipped the same rule
  enforced on one path and not its sibling.
- **Why missed:** the closing question was "are these three closed?", which has
  a true answer of "yes". `codex-review`'s own skill names the question that
  would have caught it — "Is anything from earlier rounds still open that I have
  lost track of?" — and it was not asked. The sibling instance would also have
  been found by "where else does this shape appear?", the skill's highest-yield
  question.
- **Guard:** not automatable. Procedural: keep an explicit open/closed ledger
  across rounds rather than re-deriving it from the last brief, and never accept
  a "closed" verdict rendered against a narrowed brief.

### 2026-09-11 — a relative path, two syscalls, and a cwd that moved

- **What:**
  `undo::tests::forget_tenant_removes_only_the_named_tenant_on_disk_log` failed
  once in a full run and never again. `DiskLog` persist called
  `ProjectConfig::write_gitignore()`, which resolves the relative `.aic` twice —
  `create_dir_all` then `write`. `src/agent/daemon.rs`'s tests `set_current_dir`
  into an empty temp dir, and cwd is global to the whole test binary, so the
  second resolution can name a different directory than the first.
  `record().unwrap()` then panics with `Io(AlreadyExists)`. Reproduced on a
  quiet machine on the third attempt with a cwd swapper; the two syscalls alone
  fail ~17% of the time under one.
- **Why missed:** a flake that fires once looks like infrastructure, and the
  machine had just OOM-killed an agent — which supplied a comfortable
  explanation that was wrong. The test itself is scrupulous about isolation (a
  UUID-named temp log), so the shared state was invisible: it was two layers
  down, in a defensive `write_gitignore()` call that has nothing to do with what
  the test asserts.
- **Guard:** applied in `a18a31e` — `write_gitignore_to(dir)` resolves once and
  `DiskLog` passes its own log parent, so the persist path reads no cwd.
  `disk_log_writes_gitignore_beside_an_aic_parent` is mutation-verified red
  against the old global call. **Not** retired: six other relative two-step
  writes under `ProjectConfig::dir()` remain exposed the same way
  (`ProjectConfig::save`, `Settings::save`, `WrapsFile::save`,
  `save_private_file`, `write_current_context`, `access::ops::backup_document`).
  Resolved by R1 on 2026-09-11 with absolute `ProjectPaths` and explicit paths
  in the daemon tests.

### 2026-09-16 — inspect CLI test re-reads the input, not the command output

- **What:** `inspect_command_reads_the_same_file_the_library_does` called
  `run(Inspect)` then `metadata::inspect` on the *input file*. Deleting
  `print_json` (or returning `Ok(())` without inspecting) would leave it green.
  The sanitise sibling is the right shape: it asserts bytes `run` wrote.
- **Why missed:** the test name claims the CLI and the library agree, which is
  true of two independent reads of the same fixture. Standing check "could this
  fail if the code were wrong" already covers it; this is another instance.
- **Guard:** not applied for stdout (inspect prints JSON, no `--out`). The CLI
  path now goes through `inspect_file`, and the test asserts roles +
  `would_remove` on that result. A stdout-capture or `CARGO_BIN_EXE_aic`
  integration test would close the rest. `--output` was also renamed to `--out`
  to match `jwt-bearer key export` / `access get`.

### 2026-09-16 — sanitiser scanner treated tokenisation as XML

- **What:** Codex review of `saml/metadata` (`41c9aa2..9fb53e4`). The scanner
  accepted two roots, `--` in comments, and invalid UTF-8; matched
  `RoleDescriptor`/`Signature` by local name so `ext:RoleDescriptor` and
  `html:Signature` fired; default sanitise stripped a signature from a
  signed-but-clean document. The inspect CLI test still did not assert stdout.
- **Why missed:** fixture tests and prefix tests never varied namespace URIs or
  used a signed document with nothing else to strip. Standing check "could this
  fail if the code were wrong" already named the inspect test.
- **Guard:** applied — `NsReader` + UTF-8/comment/trailing-root checks;
  expanded-name matching (SAML / XMLDSig / WS-Fed); signature cuts only when
  another cut would change the bytes; `tests/saml_metadata_cli.rs` drives the
  binary. Discriminating cases: two roots, bad comment, invalid UTF-8,
  `urn:not-wsfed` RoleDescriptor kept, signed-clean keeps `ds:Signature`.

### 2026-09-15 — type-test leaves: a discovered runner, a hand-maintained validator

- **What:** `scripts/type-tests/run.sh` discovers leaf directories with a
  `leaves/*/` glob; the Rust subset test validated a hand-maintained list of
  five names. A leaf directory nobody added to that list would be compiled by
  the gate and exempted from the assertion that its `types` manifest is a real
  subset of the shipped `leaf_tsconfig` — the one thing stopping a leaf from
  testing a fiction. Separately, the `nextgen-decision-node` leaf pinned logger
  arity and the managed-record projection but never touched the decision-node
  contract it exists to cover (no `outcome`, no `action.goTo`, no `nodeState`),
  so the generation split between `decision-node-next.d.ts` and
  `decision-node-legacy.d.ts` was uncompiled in both directions.
- **Why missed:** the subset test reads as exhaustive — it iterates a list and
  checks every entry — and nothing in it is wrong. The gap is in what the list
  does not contain, which no assertion inside the loop can see. The fixture gap
  hid the same way: `run.sh` printed `ok nextgen-decision-node accept`, and
  "the leaf passes" was read as "the leaf covers its family".
- **Guard:** applied in `2421c99` — the test now asserts the registered set
  equals the set of directories on disk, and fails with the offending name.
  Mutation-verified: adding an unregistered `zz-stray-probe/` turns it red.
  Contract coverage applied in `a406d44`, with all four absence rows
  mutation-verified (moving `sharedState`, `transientState`, `JavaImporter` or
  the legacy `IdRepository.getAttribute` into the shared overlay turns the
  reject file red).
- **Residual — the two registries disagree on what a directory is.** The guard
  filters `DirEntry::file_type().is_dir()`, which is **false** for a symlink to
  a directory; bash's `leaves/*/` glob matches one (verified). So a symlinked
  leaf is still compiled by `run.sh` and still exempt from the subset check.
  Note this is the opposite of the `.ai/core.md` `is_dir()` ban, which governs
  walkers that write or delete; this walker only reads, and here following the
  link is what matches the runner. _Fix: `entry.path().is_dir()`._
