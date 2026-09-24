# pingone-aic-manager — guide for AI agents

> **Two layers.** This file is committed and true for everyone. Anything true
> only of _one machine_ — where a sibling checkout lives, the sandbox tenant
> hostname, local tooling paths — belongs in **`.ai/local.md`**, which is
> **gitignored**. Read `.ai/local.md` too if it is present;
> `.ai/local.md.example` is the committed template. Never move machine-specific
> paths into this file, and never commit `local.md`.

`pingone-aic-manager` is a Rust + Ratatui TUI (binary: `aic`) for managing
PingOne Advanced Identity Cloud (AIC, formerly ForgeRock Identity Cloud) tenant
configuration: ESVs, scripts, OAuth2/OIDC, SAML, with fast environment switching
and (stretch) log sync+search.

## 1. Always read `docs/api/` before calling AIC

Before generating any code that hits an AIC endpoint:

1. Open the relevant `docs/api/{file}.md`. Don't guess paths, headers, or API
   versions. The library research we did to bootstrap this project contained
   real errors (see `docs/api/99-quirks-and-open-questions.md`); the verified
   docs are the source of truth.
2. If the doc says "verified against … 2026-05-17" you can trust it. If the doc
   says "open question" or "not yet exercised", verify with the script below
   before writing code.

`docs/api/README.md` is the index.

## 2. Keep `docs/api/` current — but only after verification

When you discover a new endpoint, new field, new header requirement, or
non-obvious behaviour:

1. **Verify with a live call.** Run:

   ```bash
   scripts/verify-endpoint.sh "<path>" [--header "<...>"]
   ```

   The script takes a bearer from the running agent (`aic whoami --token`, for
   the tenant in the current context) and `curl`s the path with it. **The agent
   must be unlocked — run `aic login` first**, or the script exits 3 rather than
   waiting on a password prompt.

   If verification is impossible for any reason, say so and stop. Do **not**
   write to a `## Verified against` block from inference, from figures supplied
   in a task prompt, or from a claim in a neighbouring doc — that block is the
   audit trail, and a plausible-but-wrong entry stamped "verified" is invisible
   to every later reader.

2. **Update the relevant `docs/api/*.md`** file. Bump "Verified against" to
   today's date. Add the endpoint to the table.
3. **If observed behaviour contradicts a doc**, trust observation. Update the
   doc. Add a dated note in `docs/api/99-quirks-and-open-questions.md` so
   future-you knows which library/source was wrong.
4. **Never** transcribe a frodo-lib, fr-config-manager, or Ping docs claim
   without verifying it. They have stale claims today (Q1 — script encoding, Q2
   — ESV paths, secret stores availability).

## 3. Credentials hygiene — non-negotiable

- **Never commit** `.envrc`, `.env*`, `.token-cache`, `*.jwk`, `*.pem`,
  service-account secrets, log API key secrets, or any access token. The
  `.gitignore` covers these; never override with `git add -f` on them.
- `.envrc` holds `AGENT_PASSWORD` (the vault master password). Treat it as
  secret even though the tenant is a sandbox. The service-account JWK and the
  log API keys both live in the encrypted vault now (`.aic/keys.enc`,
  `.aic/log-keys.enc`); nothing outside `src/` should need them. The exception
  is `scripts/rhino-script-tester/`, whose shell helpers still read
  `LOG_API_KEY_ID`/`LOG_API_KEY_SECRET` (or `API_KEY_ID`/`API_KEY_SECRET`) from
  the environment.
- `.aic/` is the runtime state directory and is **gitignored in full** — tenant
  config, current context, the encrypted vault, the undo log, the agent socket.
  Never commit it, and never copy tenant identifiers out of it into a committed
  file (see `.ai/local.md` instead).
- **Tokens stay in memory**, everywhere — no disk caching. `.token-cache` is a
  leftover from when `verify-endpoint.sh` signed its own assertion; the script
  now borrows the agent's in-memory bearer and writes nothing. Delete the file
  if you still have one; it stays in `.gitignore` as a guard.
- Log API uses **separate** `x-api-key` / `x-api-secret` — these are
  console-issued and `api_key_secret` is shown only once at creation.

### Sanitising live evidence — enforced

Live response bodies, AM/IDM log excerpts, HAR files and SAML metadata are the
main way client-identifying data reaches this repo, because they are pasted in
as evidence for a `docs/api/` claim. **Sanitise at capture, not at commit.**

`scripts/check-sensitive-metadata.sh` enforces this by **shape**, so it catches
the next client's hostname rather than only the last one:

```bash
scripts/check-sensitive-metadata.sh --selftest   # prove the rules still fire
scripts/check-sensitive-metadata.sh --tracked    # every tracked file (CI runs this)
scripts/check-sensitive-metadata.sh --staged     # added lines only (pre-commit hook)
scripts/check-sensitive-metadata.sh --history    # every blob in every commit (audit)
scripts/check-sensitive-metadata.sh --redact     # filter stdin -> stdout, sanitised
scripts/check-sensitive-metadata.sh --fix        # rewrite tracked files in place
```

**Capture is sanitised by default.** `scripts/verify-endpoint.sh` pipes the
response body — and its own `GET <url>` banner — through `--redact`, so
evidence for a `docs/api/` claim arrives already redacted instead of being
caught at commit time. Redaction preserves shape, so a claim about a field's
type or structure survives it. `--raw` opts out and says so on stderr. Capture
also strips ESV `valueBase64` payloads (`REDACT_VALUES=1`), which `--fix`
deliberately does **not** do — `docs/api/03-esvs.md` has example values that
rewriting would destroy.

CI also runs **Gitleaks** over the history for the orthogonal class: PEM/JWK
material, API secrets, provider tokens. Baseline verified clean over 600
commits on 2026-09-15 (gitleaks 8.30.1, the version CI pins); the findings a
working-tree scan reports are all in `target/` and the gitignored `workspace/`,
none in tracked files.

`.gitleaks.toml` extends the stock ruleset — it never replaces it — and holds
one allowlist: an AM script id is a UUID, and `accessTokenModificationScript` /
`accessTokenMayActScript` contain "Token", which `generic-api-key` reads as a
token assignment. **Allowlist by shape, never by fingerprint.** A fingerprint
pins a commit hash, and this history was rewritten on 2026-09-15, so every
fingerprint taken before that names a commit which no longer exists. Any new
entry needs the same discriminating control the first one has: a value of the
wrong shape in the same key must still fire.

Enable the hook once per clone: `git config core.hooksPath .githooks`. CI runs
the same check over the whole tree, so `--no-verify` only moves the failure to
the pull request.

Use these reserved placeholders — the scanner accepts them and rejects
everything else of the same shape:

| Instead of                | Write                                                            |
| ------------------------- | ---------------------------------------------------------------- |
| a real AIC tenant host    | `<your-tenant>.forgeblocks.com`, `tenant.example.com`            |
| a client SP entity ID     | `https://sp-a.example.com`, `https://sp-b.example.com`           |
| a CoT / config name       | `client-a`, `client-b`                                           |
| an Azure AD tenant GUID   | `<tenant-guid>` or `00000000-0000-0000-0000-000000000000`        |
| a base64url entity id     | `<entityId64>`                                                   |

Two rules worth knowing before you argue with a finding:

- **Do not "fix" a violation by base64-encoding the value.** Rule 4 decodes
  base64url candidates that start with `aHR0c` and checks the host inside.
- **Do not redact to a truncated-looking form** such as `uat.client-a…`. A
  reviewer cannot tell that from a real hostname, which is the whole problem;
  `370c7de` did this and `c3133e8` undid it.
- **If you change the placeholder vocabulary, change it in both halves.** The
  checker holds it in `$PLACEHOLDER_TENANT` / `$PLACEHOLDER_HOST` /
  `$PLACEHOLDER_UUID`; the redactor mirrors them in awk. When they drift,
  `--fix` rewrites files the checker calls clean — it did exactly that to
  `example.forgeblocks.com` and to the all-zeros GUID before the selftest
  gained its idempotence fixtures.

Literal client names are **not** in the scanner — committing a denylist of
client names would commit the client names. Locally, point `SENSITIVE_DENYLIST`
at a file outside the repo (or the gitignored `.ai/denylist.txt`). CI requires
the protected `SENSITIVE_METADATA_DENYLIST` repository secret and fails closed
when it is absent or empty; the workflow passes its content without writing it
to the checkout. See `.ai/local.md`.

Known: `--history` reports 4 hits in blob `55b60cc` (the `370c7de` commit) for
`uat.client-a` / `uat.client-b`. Those are placeholders, not real names — the
finding is that the redaction was ambiguous, and it stands until the history is
rewritten again.

## 4. Realm path convention

All realm-scoped AM URLs use `/realms/root/realms/{realm}`:

```
/am/json/realms/root/realms/alpha/scripts?_queryFilter=true
```

Use that form everywhere — it is the project **convention**, so cache keys,
audit path matching and diffs all have one spelling. It is not a hard
requirement: `/am/json/alpha/...` and `/am/json/realms/alpha/...` also work (all
three return 200 and resolve the same realm — verified 2026-08-10). Corollary:
**never match an audit log's `http.request.path` on a realm-path prefix** —
other clients use the short form and `am-access` records the URL as sent. Match
on the resource id.

ESVs, logs, and IDM managed config have no realm in the path. Full table in
`docs/api/01-realms-and-paths.md`.

## 5. Conflict detection — content snapshots, both directions

This rule governs every resource `aic` syncs to a local file, not only scripts:
AM/IDM scripts, ESV variables, `config/access`, OAuth2 clients, journeys, and
AM policies / policy sets / resource types.

The user explicitly wants: _compare content, not `_rev`_. Rationale: revision
drift doesn't matter if the content is back to what we have locally.

Store the **last-synced remote content** (decoded bytes) locally — the
_snapshot_.

### Pushing a local change

1. `GET` the remote and decode it to bytes.
2. If `decoded(remote) == snapshot`, push (overwrite is safe — content matches
   what we forked from).
3. Otherwise, remote has drifted; surface a 3-way diff (`snapshot` ↔ `remote` ↔
   `local`) and prompt the user.
4. **Re-fetch after the write and record the snapshot from that confirmed
   read — never from the bytes you submitted.** A snapshot taken from the
   request body is a claim that the tenant accepted them verbatim, and a tenant
   that silently normalises, truncates or rejects part of a write poisons the
   snapshot: the next push then compares against something that was never on
   the tenant and concludes "no drift". `scripts/sync.rs::confirm_write` is the
   implementation; a write whose confirmation fails leaves **both** the snapshot
   and the local file untouched (`PushOutcome::NotConfirmed`).

### Pulling a remote change

Pull is protected by the same content comparison, and the snapshot is what
distinguishes safe from destructive:

- local file absent → install.
- `local == remote` → no-op.
- `local == snapshot != remote` → the local copy is unmodified since the last
  sync; update it.
- local differs from **both** → the operator has unsynced work. Refuse without
  `--force`; with it, back the local file up before overwriting.
- snapshot missing or malformed → untrustworthy, so treat as the protected
  case. Fail closed.

`src/pullguard.rs` is the single implementation of that matrix — callers supply
a normaliser and a content-equality predicate, nothing more. Do not grow a
sixth copy for a sixth resource kind.

Two rules the batch case adds, both learned by getting them wrong:

- **Recheck the local bytes immediately before each write, not only once for
  the batch.** A plan authorized against a preflight read can be minutes old by
  the time entry N is written, and an edit landing in that gap is backed up and
  then silently overwritten.
- **A batch that stops partway must report what landed.** Short-circuiting a
  loop of writes tells the operator nothing was installed while several files
  are already replaced with their snapshots advanced. `PullInstallError` in
  `scripts/sync.rs` carries the completed outcomes alongside the failure, and
  every surface renders them.

### Why content and not `_rev`

Scripts have **no `_rev`** at all (verified 2026-05-17 —
`docs/api/04-scripts.md`), so this is the only viable algorithm there. ESV
variables and `config/access` likewise have none
(`docs/api/19-config-access.md`); for `config/access` the snapshot is of the
**whole document**, because the API has no per-rule endpoint.

For resources that DO have `_rev`, still use content snapshots — for the same
revert-detection reason. Only send `If-Match: <_rev>` for API families verified
to support conditional writes. OAuth2 clients and journeys have `_rev` but were
verified 2026-06-14 to use plain `PUT` without `If-Match`; strip `_rev` from
their write bodies and ignore it in content comparisons.

## 6. Tokens

- TTL: 898 seconds. Refresh ≥60s before expiry, proactively.
- Single endpoint: `POST /am/oauth2/access_token` (root, no realm segment).
- `client_id=service-account` (fixed string).
- See `docs/api/00-auth.md` for the full JWT shape.

## 7. Development workflow

```bash
# Once per shell (direnv handles this automatically if installed):
source .envrc

# Unlock the agent — verify-endpoint.sh borrows its bearer:
aic login

# Sanity-check tenant connectivity:
scripts/verify-endpoint.sh

# Hit an endpoint to inspect a shape:
scripts/verify-endpoint.sh "/environment/variables"

# Build / run / verify:
cargo check
cargo test            # unit tests are co-located in the modules they test
cargo fmt
cargo run             # no args → TUI; subcommands → CLI (see `aic --help`)
```

`.envrc` prepends `target/debug` to `PATH`, so the `aic` you invoke in this repo
is the **debug build in this checkout**, not an installed release. `cargo check`
does not rebuild it: run `cargo build` before measuring any behaviour through
`aic`, or you measure the previous build. `.ai/local.md` records the local
detail and the wrong measurement this caused.

**The gate CI enforces** (`.github/workflows/ci.yml`) — run all five before
declaring a change green, because the DuckDB log-store lives behind an opt-in
feature and rots silently otherwise:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --all-targets --features logs-store -- -D warnings
cargo test --features logs-store
```

CI runs seven more that these do not cover: the sensitive-metadata scanner
(`--selftest`, `--tracked`, the pushed range), gitleaks over the history,
`scripts/shellcheck-all.sh`, and the two TypeScript gates —
`scripts/type-tests/run.sh` and `npm run type-check` in
`src/scripts/templates/typescript`. No cargo gate compiles a shipped `.d.ts` at
all, so the Rust tests can be green while the declarations they emit do not
type-check, and no cargo gate reads a line of shell.

`scripts/release-check.sh` runs **all twelve** and refuses to report ready if
any fails, so before a release that is the one command to run. It also fails
when `ci.yml` gains or loses a step it does not account for — the two drifted
apart before, and a release cut from a stale gate list is discovered by a red
build after the tag is public.

For TUI work, follow the visual + interaction rules in `docs/DESIGN.md`
(borderless panels, tally-style tabs, semantic colors). Don't redebate them.

For script-template work (`src/scripts/templates/`), the runtime ground truth is
`scripts/rhino-script-tester/` — paired probe fixtures run against a live
journey. New syntax/binding claims get a fixture pair before they get a lint
rule or a doc row.

For SAML work (`src/saml/`), the runtime ground truth is
`scripts/saml-harness/` — a throwaway Keycloak peer exercised in both directions
whose `verify-rotate` asserts a certificate rotation by key identity, not by
`KeyDescriptor` count. New metadata/rotation claims get a measured run there
before they get a doc row.

## 8. Things to NOT do

- **Don't run tenant-touching `aic` commands from an agent without
  `--no-prompt`.** A locked daemon must fail fast instead of waiting for a
  master password the agent cannot provide.
- **Don't add a "Secret Stores" UI tab.** AIC returns 403 on the entire
  secret-stores API. Use ESVs instead. (`docs/api/07-secret-stores.md`.)
- **Don't send `-encrypted` fields back on OAuth2 client `PUT`.** They contain
  cluster-local AES-wrapped values; round-tripping corrupts secrets. Strip any
  key ending in `-encrypted` from the body. (`docs/api/05-oauth2-oidc.md`.)
- **Don't rebuild a `config/access` rule from typed fields.** `actions` is
  **optional** — six of the sandbox's 65 rules omit the key — so a round-trip
  through a struct hands them an `actions` they never had, rewriting rules
  nobody asked you to touch. Mutate the parsed `Value` in place
  (`src/access/ops.rs`). And remember the rules are a **disjunction**: appending
  can only grant, so any narrowing has to edit or remove the rule that grants
  (`docs/api/19-config-access.md`).
- **Don't `PUT` a managed _record_ twice.** A repeat `PUT` to
  `managed/{obj}/{id}` is `400 Not Allowed on RDN` — a full replace would
  rewrite `fr-idm-uuid`, which is the entry's RDN. Probe with `GET` and `PATCH`
  instead. And a converging `PATCH` must **omit** `password`: re-sending the one
  the user already has trips the realm's password history
  (`docs/api/10-managed-objects.md`). This only bites a provisioner that has
  never been re-run against a live tenant.
- **Don't log `identity.getAttributes()` from an OAuth2 script.** In the legacy
  access-token-modification context it returns the whole agent profile of the
  calling client — `userpassword` included, in clear (verified 2026-08-27,
  `docs/api/12-script-bindings-matrix.md`). Read the one attribute you want with
  `getAttribute(name)`. No type can catch this one; the return is a map either
  way.
- **Don't select a directory with `Path::is_dir()` in a walker that then
  writes or deletes.** It follows symlinks, so a symlinked folder inside the
  workspace makes the walker operate on the far side of the link — outside the
  tree it believes it is confined to. Use `DirEntry::file_type()`, which does
  not follow. `src/scripts/workspace.rs` had this twice: the prune walk
  (`remove_file` on the target's contents) and the leaf-`tsconfig.json` refresh
  (`write` through the link). The contents loop was already correct; only the
  directory _selection_ was wrong, which is what makes it easy to miss.
- **Don't trust `creationDate` / `lastModifiedDate` types are consistent.**
  Scripts use epoch-ms ints; ESVs use ISO-8601 strings. Don't assume.
- **Don't try to create new realms.** AIC only allows `alpha` + `bravo` + root.
- **Don't poll `/environment/startup?_action=restart` aggressively** — rate
  limits are tighter than the read endpoints.
- **Don't expect `src/agent/` code changes to take effect while an agent is
  running — and treat this as an upgrade hazard, not just a testing one.**
  `aic session logout` only _locks_ the daemon; the old binary stays resident.
  Run `aic session stop`, then relaunch, before testing agent changes **and
  after upgrading `aic`**. A new CLI talking to a resident old daemon used to
  fail in whatever way that particular change happened to break — verified
  2026-08-06, when a daemon 5 days old replied to one request and closed the
  connection, giving the next request a broken pipe. The wire protocol (version
  3 as of 2026-09-15) now turns a detectable mismatch into a message naming
  `aic session stop` as the remedy; it does not provide compatibility. `stop`
  itself does **not** need the handshake to succeed — it falls back to
  SIGTERMing the pid in `.aic/agent.pid` — because the remedy for "this build
  cannot talk to that daemon" must not itself require talking to it.
- **Don't edit `src/scripts/templates/` without bumping `TEMPLATES_VERSION`** in
  `src/scripts/workspace.rs` — otherwise scaffolded workspaces never receive the
  update.
- **Don't subclass `Error` in any code that reaches an IDM script engine**,
  including the TypeScript endpoint project. `Reflect` is `undefined` there, so
  after Babel's ES5 downlevel `instanceof YourError` is **false** while
  `instanceof Error` is true, and every error silently classifies as unknown.
  Use the tagged fault objects in `typescript/framework/errors.ts`
  (`docs/typescript-endpoints.md`).
- **Don't mutate a managed schema outside `managed::ops`'s `apply_*`
  transforms.** They carry key normalisation, availability checks, `FieldCaps`
  capability gating, the relationship-rename refusal and the enum-narrowing
  gate. A new caller that builds its own JSON gets none of it, and the CLI has
  no visual confirmation step to catch the difference. If a transform can't
  express what you need, widen the transform.
- **Don't narrow a managed-field `enum` without the caller's consent flag.**
  Dropping an allowed value breaks whole-record updates for records still
  holding it — later, elsewhere, on a property that code never touched
  (`docs/api/10-managed-objects.md`). Adding a value and clearing the constraint
  are both widening and need no gate.

## 9. Project layout — routing map

Updated 2026-08-01; the feature-vertical restructure is **complete** (rationale:
`docs/orthogonality-review.md`). One directory per feature, with uniform seams:
`api` (HTTP), `state`, `ops` (background work), `screen` (key handling + nested
Mode/Event), `view` (rendering), `cli`. Pull only the rows you need into
context.

A feature that has both a tab and CLI verbs needs a seventh seam: a `spec.rs` of
plain input types with no TUI state, so `cli.rs` and the tab drive the same
transforms instead of each building its own request. `src/managed/` is the
worked example.

| To change…                                                                                                                             | Code lives in                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Read first                                                     |
| -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| ESV variables                                                                                                                          | `src/esv/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `docs/api/03-esvs.md`                                          |
| ESV secrets                                                                                                                            | `src/secrets/` (HTTP wrappers stay in `esv/api.rs` — same API family)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | `docs/api/03-esvs.md`                                          |
| ESV secret mappings (AM secret label → ESV secret)                                                                                     | `src/secretmap/` (surfaced as the ESVs tab's "ESV secret mappings" sub-view; sandbox/development only)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `docs/api/15-secret-mappings.md`                               |
| IDM sync mappings (browse `config/sync` mappings, reconcile)                                                                           | `src/mappings/` (TUI-only; the Mappings tab) — script pull/push for embedded mapping scripts is `aic script … sync/<mapping>.<slotpath>` via `src/scripts/sync_mapping.rs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `docs/api/16-sync-mappings.md`                                 |
| Journeys (auth trees: list/pull/push/delete, node-type introspection)                                                                  | `src/journey/` (CLI only — no TUI tab yet)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `docs/api/09-journeys.md`                                      |
| IDM internal roles (caller-chosen ids, role CRUD, managed-object privileges)                                                           | `src/roles/` (CLI only — no TUI tab)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `docs/api/18-internal-roles.md`                                |
| AM policies (resource types, policy sets, policies, PDP `eval`)                                                                        | `src/policy/` (CLI only — no TUI tab). `spec.rs` holds the content normalisation, the **measured** URL-wildcard matcher and `diagnose`, which reconstructs why `actions: {}` came back; `api.rs` hides the three-way create asymmetry behind `upsert_*`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `docs/api/21-am-policies.md`                                   |
| IDM authorization rules (`config/access`: list/add/edit/rm/apply)                                                                      | `src/access/` — `spec.rs` holds the input specs, structural validation, the rule/document digests that address a rule, and the **shared row projection** (`RuleSummary`/`rule_summaries`) that both `cli.rs` and the tab build their rows from; `ops.rs` the in-place `Value` transforms both call. The two surfaces deliberately do **not** share a column set — the CLI prints a multi-line block per rule, the tab a fixed-percentage table, and one shared `cells()` let the narrow surface mangle literals the wide one wanted. The tab **edits**: `ops::amend(before, amendment)` is the shared pure transform, and `cli.rs::plan()` is a thin wrapper adding the two CLI-only policy fields — do not fork it. The tab also **reorders** (`K`/`J`) and creates at a position (`O` above, `o`/`^N` below), via `Amendment::Move`/`Insert`; both are presentational, because `configs` is a disjunction and position cannot change who is authorized. A move carries no `changed` entries for that reason — it reports `Changes::moved` instead, so the narrowing warning and the "is anything different" guards do not read a reorder as a grant change. The **Edit** form carries keep/set/clear per optional field, never a value, so an untouched key stays absent; the **Create** form carries a value, with empty meaning absent. `apply`-from-file stays CLI-only | `docs/api/19-config-access.md`                                 |
| Trusted JWT Issuer setup (per-tenant signing key, issuer CRUD/show)                                                                    | `src/jwtbearer/` (CLI only — no TUI tab yet)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `docs/api/17-jwt-bearer-user-tokens.md`                        |
| Script sync (pull/push/sync/watch/diff)                                                                                                | `src/scripts/` (one module per `Kind`: `am`, `idm`, `schedule`, `managed_hooks`, `sync_mapping`). Every tenant write goes through `gate.rs`'s `write_checked`, which runs the pre-push syntax gate in `syntax.rs`. **You cannot call `Kind::write` directly — it will not compile.** `gate.rs` owns `WritePermit`, whose field is private, so only that module can mint one; `Kind::write` and all five raw per-kind writers require a `&WritePermit` and are `pub(super)`. Verified 2026-09-09 by planting each bypass the old source-scanning tripwire missed: a missing permit, a split/aliased call, a direct `am::write`, and constructing a permit outside `gate` all fail to compile (E0061/E0603). What the permit proves is **routing**, not correctness — it says a write came through `write_checked`, not that the check inside was right; the residual risk is a second minting site added inside `gate.rs` itself. The gate fails **closed** with no exceptions: only a pass writes. A check that gives no verdict writes nothing, and neither does source nothing can check (an unknown script engine) — where the gate found out shapes the message and the remedy, never the permission. `write_checked` returns its outcome and prints nothing — presentation is per-surface (`cli.rs` to stderr, the tab's inline strip). Note this covers script-**sync** writes; `managed/ops.rs`'s `apply_add_hook` authors a fixed comment-only hook body outside the engine                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `docs/api/04-scripts.md`, `11`, `12`, `13`, `16`               |
| Protected-pull decision (content snapshot rule, §5)                                                                                    | `src/pullguard.rs` — the single source of truth for `PullDecision`, the decision function and the consent predicate. Callers supply a normaliser plus a content-equality predicate so policy identity checks, journey export shaping and `_rev` stripping stay at the surface. OAuth, journey and policy CLI pulls are the current callers; do not reimplement the matrix in a fourth vertical. | `.ai/core.md` §5                                               |
| Workspace backups (the file a protected overwrite preserves)                                                                           | `src/backup.rs` — one collision-safe allocator (kind+realm+name+timestamp+uuid, `create_new`, `0600`) writing under `workspace/<tenant>/.aic-sync/backups/`. It drops a `*` / `!.gitignore` in that directory itself, because the workspace `.gitignore` that covers `.aic-sync/` is only written by `scaffold_at` and the CLI pull paths never scaffold. Every backup writer goes through it; do not hand-roll a second path                                                                                                   | `.ai/core.md` §5                                               |
| `--force` guards (which safety override a command accepts)                                                                             | `src/cli/force.rs` — `ForceFlags<const ALLOWED: u8>` is a clap type whose `PossibleValuesParser` emits only the guards in `ALLOWED`, so `--force=<undeclared>` is a parse error rather than a silent no-op. One `GUARDS` table is the source of truth for bit, name, help and possible values. Aliases (`OperationForce`, `SyntaxCheckForce`, …) name the masks; `--force` bare means `operation`                                                                                                                       | `docs/CLI.md` (Flag migration)                                 |
| TypeScript custom endpoints (build-time module sharing; bundle → `idm/endpoint/<name>.cjs`)                                            | `src/scripts/templates/typescript/` (the embedded project), `src/scripts/ts_project.rs` (build manifest read by `script watch`), `src/scripts/workspace.rs` (scaffold + `package.json` merge), `src/scripts/managed_types.rs` (module-form tenant types)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | `docs/typescript-endpoints.md`, `docs/api/11-idm-endpoints.md` |
| Script workspace templates (lint/types)                                                                                                | `src/scripts/templates/` + `TEMPLATES_VERSION` in `src/scripts/workspace.rs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `docs/api/12-script-bindings-matrix.md`                        |
| Managed-object schema: browse **and edit** (Managed tab + `aic managed` writes); hooks sync as `managed/<obj>.<hook>` via `aic script` | `src/managed/` — `spec.rs` holds the TUI-free input specs (`FieldEditSpec`, `AddFieldSpec`, `EnumChange`, …), `ops.rs` the pure `apply_*` transforms that both the tab and `cli.rs` call. Hook sync is `src/scripts/managed_hooks.rs`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `docs/api/10-managed-objects.md`                               |
| IDM managed-object record store + query                                                                                                | `src/idmstore/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | `docs/api/10-managed-objects.md`                               |
| Logs (fetch, key mgmt, local DuckDB sync/search/compact + journey rollup)                                                              | `src/logs/` (CLI only — no TUI tab yet); `event.rs` holds the one event-identity function `tail`'s in-memory dedupe and the DuckDB `ON CONFLICT (id)` insert both use, and is deliberately **not** behind `logs-store`; log-KEY STORAGE rides the same vault path as the SA JWK: `src/config/` (`log-keys.enc`/`log-keys.plain` read/write), `src/agent/` (vault secret verbs), `src/vault/` (`unlock.rs`/`auth.rs` load the decrypted map into `App` on unlock)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `docs/api/08-logs.md`, `docs/logs-store.md`                    |
| OAuth2 clients, the realm provider service, and RFC 8693 token exchange                                                                | `src/oauth/` — `spec.rs` holds the projections both the summary and the exchange view are built from. `aic oauth exchange list` is read-only reconnaissance: it names who holds the grant and who stamps `may_act`, and warns about the states that all fail as one opaque error. There is no `provider set`, and nothing performs an exchange                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `docs/api/05-oauth2-oidc.md`, `22-token-exchange.md`           |
| SAML 2.0 entity providers and circles of trust (list/show/cot/create-hosted/import/delete/metadata export + offline inspect/sanitise) | `src/saml/` (CLI only — no TUI tab). `metadata.rs` is a scanner that splices the original buffer, not a parse-and-reserialise. **What a signature covers is read, never inferred from placement**: coverage is read from the `<ds:Reference>` elements that are **direct children of the signature's own `<ds:SignedInfo>`**, and from nowhere else. A `<ds:Manifest>`'s references are application-defined, and one nested deeper inside `SignedInfo` is not schema-valid at all, so counting either manufactures coverage core validation never established and keeps a signature a rewrite has invalidated — both are fail-**open**, which is why the depth is checked rather than shrugged at. Each such `<ds:Reference URI>` resolves — `""` to the whole document, `#id` to the element declaring that XML ID, anything else to `Coverage::Unknown`, which is treated as covering everything and so removed. Only unqualified `ID` and `xml:id` count as IDs, because those are IDs by specification; honouring `Id`/`id` would be fail-**open**, since a reference that resolves is what lets a signature survive. Invalidation is a fixed point — removing a stale signature is itself a change — and a signature is never tested against its own removal, which is the enveloped-signature transform for free. A `DOCTYPE` is **refused** wherever it appears: these tools read no DTD, so forwarding one passes AM a document whose expansion they cannot describe. One `scan` serves every entry point, with two knobs on it: `Depth` (`validate_export_document` is the deliberately weak one, because a roleless entity's legal export carries no `entityID`) and `Roots` — `MetadataBundle` is the **only** entry point that admits an `EntitiesDescriptor` aggregate, because one `?_action=importEntity` call creates every entity in one and a metadata *export* is never an aggregate, so widening import must not widen the classifier. `spec.rs` holds the tenant-free half: `entity_id64` (base64url, **no padding**), `locate` (which collection holds an id), the list/summary projections and `classify_export`. `api.rs` runs two transports on purpose — the JSON collection through the daemon, and `exportmetadata.jsp` through a direct `AicClient` with **no bearer at all**, because that endpoint is unauthenticated. So `cli::needs_tenant_auth` splits three ways and its inner match must stay exhaustive: everything but `metadata` needs an unlock, `metadata export` reaches the tenant but must work locked, `inspect`/`sanitise` never leave the disk. **An export failure is HTTP 200** — `spec::classify_export` is the only thing between an `ERROR :` string and a file called `entity.xml`; never branch on the status. The two write verbs both exist to cover for AM: `spec::build_hosted_create` imposes the `entityId` and `/<realm>/<name>` `metaAlias` AM does not require (`{}` is a **201** with a UUID name; a role block without `services.metaAlias` is a 500 naming no field), and `delete` reads the CoT collection first because an entity `DELETE` silently rewrites every circle of trust that listed it — `spec::cots_naming` matches the entity-ID half **exactly**, never a prefix, and `spec::delete_ok` is the `--force` rule, split out so a test drives the real one, and the cascade is **re-read after the write** and reported from that second read (`spec::cascade_outcome`) rather than replayed from the pre-state — same rule as §5's confirm-from-the-read. The second read **always happens**, so a circle the pre-delete read never saw is still found, and **the exit code answers for the cascade, not for the delete**: the entity is gone either way, but a circle still listing it — or a re-read that failed and so cannot tell — exits non-zero, because automation reading a zero here is reading "the federation is consistent again". `spec::delete_ok` mints a `DeletePermit` that `api::delete_entity` requires, so the unforced preview cannot reach the write by construction rather than by statement order, and `ensure_prod_confirmed` runs **after** that refusal so previewing a delete never demands `--yes`. **`cot list`/`cot show` are not membership**: AM stores it twice and REST shows only the CoT document's `trustedProviders`, never the entity-side `cotlist` the runtime actually reads, so `spec::COT_MEMBERSHIP_CAVEAT` rides every CoT rendering and is pinned by a test. `import` is the verb that can break a live federation, so nothing about it is optional: `MetadataBundle` names **every** entity id exactly, `spec::collisions` preflights all of them against **both** collections and `spec::authorize_import` refuses the whole operation if any exists — there is no `--force` that deletes and re-imports, because the `cotlist` cannot be read back or restored, and no `--cot`, because AM discards it with a 200. `spec::standard_metadata` is base64url over the **untrimmed** document and is deliberately not `entity_id64`. `--dry-run` stops by holding no `spec::ImportPermit` — the field is private, `authorize_import` is the only minting site and `api::import_entity` requires one, so the preview arm cannot reach the write (same shape as `scripts::gate`'s `WritePermit`). Afterwards `spec::compare_imported` is a **set** comparison against `importedEntities`, never a count; a failure re-lists the realm (`spec::after_failure_lines`) because a failed aggregate import is not a rollback; and `spec::IMPORT_COTLIST_CAVEAT` ends every run, because `importEntity` rewrites extended metadata and no post-import verification of `cotlist` is possible. Every daemon-routed call in `api.rs` is assembled by a `SamlRequest` builder and asserted through `ApiCall::envelope`, because a test that rebuilds the expected path out of the same helpers the production function uses stays green while that function is broken. There is no CoT write verb | `docs/api/06-saml.md`                                          |
| SAML signing-certificate rotation (`rotate status/init/stage/complete`)                                                                | `src/saml/rotate/` (CLI only). **An AIC SAML signing certificate is not in the entity** — the entity holds a `secretIdIdentifier`, which is a label into AM's secret store, and on AIC that label is backed by an **ESV secret**. So a rollover is an ESV secret *version* operation spanning `saml/`, `esv/` and `secretmap/`, and there is nothing to upload and no metadata to push. `pem.rs` and `spec.rs` are tenant-free, `ops.rs` is the only module that performs I/O, `journal.rs` is the local record. **`stage` and `complete` are separate operations and that is not stylistic**: `esv secret disable` cannot disable the latest version (`400 Cannot disable latest secret version`), so completion disables the *old* one. **Treat `stage` as the signing cutover, not a preparation for one**: on the SP AuthnRequest path the newest ENABLED version was signing by the first observation (≤12 s, measured 2026-09-22; recency and export order were not separated), and IdP assertion signing is unmeasured and assumed the same — a safety assumption, never reported as observed — so the peer has to hold and trust the new certificate *before* `stage` runs — which is what `--key-file` taking the operator's own key pair is for — and the two-certificate export is the catch-up for a metadata-refreshing peer, not the pre-trust window. `complete` on its usual path is not expected to change the signer — it stops publishing the old certificate; `--retain` naming the older certificate disables the version to treat as signing, so it is *treated as* a signer cutover back (never measured on either role), and `CompletePlan::moves_signer` makes the plan and prompt say so. There is no un-stage — disabling the just-added version is the 400 above — so the way back is **emergency signer restoration**: re-add the old key pair (the old *private key* is required) as a newer version, then disable the two superseded versions by hand; `rotate` reports that three-version state as `Inconsistent` and does not finish it, like every other off-path state. **All three writing verbs confirm the same way** — `OperationForce`, `prompt_available() && confirm_destructive(..)`, then `spec::{init,stage,complete}_ok`, then the prod gate — because `init` replaces the published certificate and `stage` cuts over. Nothing here destroys: `complete` disables, which `esv secret enable` undoes. **A rollover preserves the identifier and its mapping** — repointing the entity rotates nothing and orphans the old mapping, which is the defect `fix/secretmap-orphan` just fixed. **And a rollover is refused unless the ESV secret backs exactly one (entity, role)**: AIC permits a secret label to be shared between providers, and `stage`/`complete` mutate the *secret*, so rotating one entity's key would publish a second certificate for every consumer and then retire one — while the export, phase and settlement checks cover the named entity alone. `spec::survey_consumers` reads every realm's mapping table and every entity provider in it (`SURVEYED_REALMS`: alpha, bravo; a survey missing one is not exclusive — the stub list carries no `secretIdIdentifier` and there is no query filter for one, so the per-entity read is the cost of knowing), catching both channels: a second (entity, role) naming the same identifier, and a second label mapped onto the same ESV secret. `spec::exclusive_ok` mints the `ExclusivityProof` that all three `authorize_*` **consume**, so a verb that forgot to survey cannot reach a write — the permit construction, one step earlier. The write holds a *second* proof, minted by a re-survey inside `ops`' pre-write recheck after the prompt (`spec::rollover_pre_write_ok` / `spec::init_pre_write_ok`, called from `ops::recheck` / `ops::init_recheck` as the first act of `add_version`, `disable_version` and `apply_init`; `init` has up to three writes, so its **activating** step — the last planned, `InitPlan::activating_step` — re-surveys again immediately before it is sent (`InitPlan::resurveys_before`, `spec::init_exclusive`), which narrows the race and cannot make it atomic; an `init` that stops after a step completed returns `spec::InitApplyError` naming what completed, §5's batch rule in `PullInstallError`'s shape, and carries the stopping step's own `spec::WriteStatus` — refused, accepted-but-unverified, or unknown — because a `PUT` whose read-back failed has landed and a lost response may have; after the send the default is **unknown**, and only an endpoint's `spec::MeasuredRefusal` (status *and* message, measured — `SECRET_ALREADY_EXISTS`, `CANNOT_DISABLE_LATEST`) or a pre-send gate is `Refused`, never a status class, since the client follows redirects; `init` promises the old signer only when no completed step `changes_resolution`; `WriteFailure` has no `From<Error>`, so every `?` on a writer's path says which side of the send it is on, and `stage`/`complete` render their write and post-write checks through the same `WriteFailure::message`. The loop is `spec::run_init_steps`, with the re-survey and the step writer injected as async closures so a test proves the activating write is not called after a refused re-survey. Every writer the three verbs reach requires `&ExclusivityProof` as a parameter — a guard against omitting the check, not atomicity), so a consumer added while the operator read the prompt is refused rather than cut over. The refusal **names the other consumers**, because "this label is shared" without the list is a dead end. What it still cannot see is the root realm — unmeasured for SAML, though every root realm-config family measured so far is 403 — and that gap rides every sharing report as `SHARING_CAVEAT` rather than going unsaid. There is no `--force`. Verification is over the exact `(role, use, fingerprint)` **set**, never a count: a `complete` that disabled the new version leaves exactly one certificate published and a count check calls that success (`spec::settled_on`). `CertRef::descriptor` is why the set is role-scoped — both roles of a dual-role entity publish `use="signing"` with no `KeyName`, so `--role` is required there. The entity `PUT` is a **full replace with no `If-Match`** (`{"entityId": "<same>"}` is a 200 that deletes the role block), so `ops::set_identifier` reads fresh, mutates one leaf, writes, re-reads and compares the whole document — §5 applied to a family with no conditional write. `--dry-run` stops by holding no permit: `spec::authorize_*` are the only minting sites for `InitPermit`/`StagePermit`/`CompletePermit` and every `ops` writer requires one, so a preview that fell through would not compile. **What nothing can know**: which published certificate came from which secret version — values are write-only and AM emits no `<ds:KeyName>` — so `journal.rs` records the pairing at stage time *from the tenant's own export*. **`complete` derives the version it disables from that pairing, or does not derive it** — never from version age. The oldest ENABLED version is the right answer only when the newest certificate is the one being kept, and `plan_complete` used to assume that even with a record in hand saying otherwise; a rollover staged with the wrong key pair is the case where the two disagree. So with no usable record — absent, or naming a certificate the entity no longer publishes — `--retain <sha256>` is **not** sufficient: `--disable-version <n>` is required with it, and even the case where age happens to agree is refused, because from inside `plan_complete` it is the same document as the case where it does not. `CompletePlan::chosen_by` tells the operator whether the version is the record's conclusion or their own claim, before they confirm  | `docs/api/06-saml.md`                                          |
| SAML peer for testing (throwaway Keycloak as IdP and SP; certificate rotation)                                                         | `scripts/saml-harness/harness.sh` — not Rust. `key_report` is the measuring instrument: per-role `KeyDescriptor` counts plus a certificate-fingerprint identity per signing key. `verify-rotate` is the guard a rotation claim must go through                                                                                          | `docs/saml-test-harness.md`                                    |
| Tokens / HTTP transport / daemon                                                                                                       | `src/aic/` (transport core), `src/agent/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | `docs/api/00-auth.md`, `01`, `02`; `src/agent/mod.rs` header   |
| Local credential vault / unlock                                                                                                        | `src/vault/` + `src/config/{crypto,wraps}.rs` storage                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | — (local-only, no AIC docs)                                    |
| Operator identity (name/host resolution + persistence)                                                                                 | `src/config/operator.rs` (`Settings` shape/storage in `src/config/mod.rs`; CLI prompt/settings/whoami in `src/cli/mod.rs`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `docs/CLI.md`                                                  |
| Onboarding (add tenant)                                                                                                                | `src/onboard/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `docs/api/00-auth.md`, `99-…` Q11/Q12                          |
| Offboarding (`aic ctx rm`; env-picker delete modal, not a tab)                                                                         | `src/offboard/` — `spec.rs` owns the sharing guard and `plan`; `ops.rs` probes and executes, and its only network call unpublishes this install's Trusted JWT kid. CLI and TUI must not re-decide what is safe                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | `docs/api/00-auth.md`, `docs/api/08-logs.md`                   |
| Undo log + history overlay                                                                                                             | `src/undo/` (executors live in each feature's `ops`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | —                                                              |
| App shell: event loop, mode/tab dispatch, prod guard, env picker                                                                       | `src/app/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | —                                                              |
| Shared TUI chrome: widgets, theme, header, toasts, modals, help                                                                        | `src/tui/`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `docs/DESIGN.md`                                               |
| CLI root (clap, login/logout/stop/status, ctx, settings, whoami)                                                                       | `src/cli/mod.rs` (feature subcommands live in each vertical's `cli.rs`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `docs/CLI.md`                                                  |

### Adding a new feature (e.g. OAuth2)

1. Verify + document the API first (§2); new `docs/api/` file if needed.
2. Create `src/<feature>/` with the standard seams; state hangs off `App` as one
   field; `screen.rs` owns a nested `Mode` + `Event` enum.
3. Register the feature in `src/app/`. A CLI-only feature is one arm in
   `cli::Command` and nothing else. **A tab is not one arm per global** — this
   step used to claim it was, and it is the single most misleading line in this
   file; corrected 2026-08-11 after it sent an agent looking for a six-arm
   footprint that does not exist. Adding a tab means ~20 arms across six files,
   because `keymap.rs` dispatches each list operation through its own exhaustive
   `match` on `View`. `src/mappings/` is the smallest complete example; the
   compiler finds all of these for you, so add the `View` variant first and
   follow the errors.

   Note the sixth file is outside `src/app/`: `src/tui/keybind_help.rs` matches
   exhaustively on `InputMode`, so a new `InputMode` variant needs an arm there
   too. That row was missing until 2026-08-12, when it blocked a second agent
   working from an allow-list that named only the `src/app/` files.

   | File                  | Sites                                                                                                                                                                                                               |
   | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
   | `app/mod.rs`          | `InputMode::<Feature>(Mode)`; the `View` variant; `View::all()`; `View::label()`; the `App` state field (+ init and tenant-switch reset); the `AppEvent` arm in `apply_event`; the `refresh_view` arm               |
   | `app/event.rs`        | `AppEvent::<Feature>(Event)`                                                                                                                                                                                        |
   | `app/draw.rs`         | the `InputMode` modal group; the active-view draw branch                                                                                                                                                            |
   | `app/keymap.rs`       | `dispatch`, `footer_hints`, and one arm each in `row_count`, `current_selection`, `set_selection`, `filter_active`, `clear_filter`, `primary`, `delete`, `new_item`, `search_mode`; plus `normal_binds` — see below |
   | `app/prod_confirm.rs` | only if the feature has tenant-write prod actions                                                                                                                                                                   |
   | `tui/keybind_help.rs` | the `InputMode` arm routing to the feature's `screen::help_lines` — **not** under `src/app/`                                                                                                                        |

   `normal_binds` is the one site the compiler will **not** find for you: it
   gates view-specific hints (`^R` refresh, `^N` new) behind an
   `app.active_view == View::<Feature>` bool rather than an exhaustive `match`,
   so omitting it compiles clean and silently costs the tab its keybinds.

   The **reachability table** in `keymap.rs`'s `mod tests`
   (`normal_bind_reachability_matches_each_view`) is the gate for this, added
   2026-08-13 after two user-facing paths shipped unreachable past every other
   gate. Add a row group for the new view: it asserts both an act missing where
   it belongs and an act bound where it does not, over `View::all()`, the three
   `EsvView` sub-views, and populated versus empty lists. `App::for_test` builds
   the app it needs with no disk, environment or network access. Two conventions
   it encodes, both learned the hard way: put an action about the list's
   _history_ (`^Z`, `^Y`) and creation from empty (`^N`) **outside** the `n > 0`
   guard, and keep selection-dependent actions inside it.

   Correspondingly, `<feature>/screen.rs` must publish `Mode`, `Event`,
   `apply_event`, `handle_key`, `footer_hints`, `help_lines`, `refresh`,
   `row_count`, `current_selection`, `select`, `filter_active`, `clear_filter`,
   `primary`, `delete`, `new_item`. A read-only tab still has to publish
   `delete`/`new_item`; leave them as empty no-ops, as `mappings/screen.rs`
   does.

4. Add the routing row above and a `mod.rs` header linking the API doc.
5. If you added or changed a subcommand, update `docs/CLI.md`. Feature-internal
   changes (new modal, new background op) must touch only the feature directory
   — if you find yourself editing `src/app/` for one, the design is being
   violated. Registering a _new_ tab (step 3) is the standing exception.

## 10. When unsure

- **Read `.ai/local.md` if it exists.** It records this machine's sandbox tenant
  and sibling-checkout paths. Several docs cite evidence from sibling checkouts
  — two AIC-adjacent ones this repo owns (`~/w/aic/who-changed`, and
  `~/w/headless-vi` in `docs/vici-editor-plan.md`), and two client checkouts
  that are **named only in `local.md`**: a prod log corpus and a production
  script corpus. A committed file must not spell a client checkout's path, even
  with the name replaced by a placeholder — the path shape alone says a client
  corpus exists and roughly what is in it. Cite the evidence, say `local.md`
  names where it lives, and stop there. None of these are guaranteed to exist
  on a given machine; if a doc cites one you cannot find, say so rather than
  inventing what it would have contained.
- **Default to reading `docs/api/`** rather than searching the web. The web
  sources we already mined had errors (Q1, Q2 in 99-…); our verified docs win.
- **If the docs don't cover it, verify before coding.** Use
  `scripts/verify-endpoint.sh` and update the relevant file.
- **If the user asks for a feature not yet in `docs/api/`** (e.g. themes, email
  templates, audit), do a verification pass first and add a new doc file before
  writing implementation code.
