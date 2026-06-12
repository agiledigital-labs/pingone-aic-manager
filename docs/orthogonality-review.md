# Orthogonality review — 2026-06-13

Goal: an AI coding agent should be able to make a feature change by pulling
**one directory plus one docs/api file** into context. This report maps where
that breaks today, proposes a feature-vertical layout (approved direction),
and sequences the migration. No code has been changed yet.

## 1. How a feature change spreads today

The code is layered (`screens/` state+handlers, `ui/` rendering, `aic/` API
clients, `cli/` commands), so one feature is smeared across every layer plus
four global chokepoints. Editing the ESV feature today means touching or
reading **seven files**:

| File | Role for ESV | Lines |
|---|---|---|
| `src/screens/esv.rs` | state, key handlers, save/delete/restart plans, undo | 2,106 |
| `src/ui/mod.rs` | ESV rendering is **inline** here (~670 of 902 lines) — there is no `ui/esv.rs` | 902 |
| `src/aic/esv.rs` | HTTP wrappers | 448 |
| `src/cli/mod.rs` | `EsvCommand` + `SecretCommand` defs and impls, shared with every other feature | 1,842 |
| `src/app.rs` | 5 ESV `InputMode` variants, `esv` field, 5 `handle_event` arms | 668 |
| `src/event.rs` | 5 ESV `AppEvent` variants | — |
| `src/keymap.rs` | 5 dispatch arms + Normal-mode bind conditions | 619 |

Scripts and Secrets have the same shape. The four global chokepoints — files
**every** feature change must touch:

1. **`app::InputMode`** — one flat enum holding every feature's modal states
   (`EsvSearch`, `SecretAddVersion`, `OnboardPaste`, …). Adding one
   confirm-dialog to a feature edits this enum plus every exhaustive match
   over it (`keymap::dispatch`, `ui::draw`, the hint-suppression list in
   `ui/mod.rs`).
2. **`event::AppEvent`** — one flat enum of every feature's background-task
   completions, with a matching dispatch block in `app::handle_event`.
3. **`cli/mod.rs`** — all subcommand definitions *and implementations* for
   every feature in one 1,842-line file (~700 lines are `aic script` alone).
4. **`ui::draw`** — global match over `InputMode`, plus ESV dashboard
   rendering living inline in the same file.

### Layering violations and fossils

- `src/aic/onboard/{paste,userpass,cookie}.rs` import
  `crate::ui::widgets::text_field` — the API layer holds UI form state.
- `app.rs` carries "backwards-compat shim" re-exports (`AuthMethod`,
  `UnlockOk`, `PendingProdAction`, `pending_overwrite_name`, …) — residue of
  earlier partial refactors. Delete during the move, don't carry forward.

### What's already right (preserve these)

- The `screens/*` pattern — per-feature `State` struct hung off `App` as one
  field, free-function handlers — is already halfway to verticals.
- `aic/api.rs`: single HTTP path through the agent; enforced by
  `pub(crate) AicClient`. Keep exactly as is.
- `aic/script/` is explicitly engineered for extension (`Kind` dispatch,
  kind-agnostic sync engine) and is nearly a self-contained vertical already.
- `keymap`'s binds-as-data design (dispatch, footer, F1 help from one table).
- Excellent module-header docs (`agent/mod.rs`, `screens/secret.rs`,
  `aic/script/mod.rs`) — exactly the right pattern, extend it per vertical.
- Tests are co-located; they move with their files.

## 2. Target layout (feature verticals)

```
src/
├── main.rs, lib.rs, error.rs, logging.rs
├── app/        # App struct, event loop, tick, Tab routing, global dispatch,
│               # prod_confirm (shared write-guard modal)
├── tui/        # shared chrome: widgets/, theme, toast, header, modal_chrome,
│               # popup_confirm, keybind_help, keymap infra (Trigger/Bind/Act)
├── agent/      # unchanged (daemon, client, protocol)
├── aic/        # transport core only: api.rs, auth.rs (token mint), AicClient
├── config/     # unchanged (tenant, crypto, wraps, settings)
├── cli/        # clap root + agent/login/logout/stop/status/ctx/whoami;
│               # feature subcommands delegate out
├── vault/      # local credential vault: unlock, auth_setup, auth_settings
│               # screens + views, security_key.rs, auth.rs (UnlockOk)
├── onboard/    # aic/onboard/* + screens/onboard + ui/onboard + env_picker
│               # (fixes the aic→ui violation by construction)
├── esv/        # api.rs, state.rs, ops.rs, screen.rs, view.rs, cli.rs
├── secrets/    # state.rs, ops.rs, screen.rs, view.rs, cli.rs
├── scripts/    # everything under aic/script/ (incl. templates/) + screen,
│               # view, cli.rs
└── undo/       # undo.rs log + undo_history screen + view
```

Each vertical exposes the same five seams: `api` (HTTP), `state`,
`screen` (key handling), `view` (rendering), `cli`. Small verticals can fold
these into fewer files; the names just need to be predictable.

Known cross-feature seams, kept deliberate and documented in the module
headers: secrets' list arrives via the ESV poll (`esv::ops` calls
`secrets::apply_refresh`); esv/secrets record into `undo`'s trait;
everything calls `aic::api`.

### The dispatch problem — nested enums, one arm per feature

Moving files alone doesn't fix chokepoints 1–2. The fix that keeps
feature-internal changes inside the feature directory:

```rust
// app/mod.rs — global enums shrink to one variant per feature
pub enum InputMode {
    Normal,
    EnvPicker, ProdConfirm,            // app-level
    Vault(vault::Mode),                // Unlock | SetupAuth | Settings | …
    Onboard(onboard::Mode),
    Esv(esv::Mode),                    // Search | Edit | RestartConfirm | …
    Secrets(secrets::Mode),
    Scripts(scripts::Mode),
    UndoHistory,
}

pub enum AppEvent {
    Key(KeyEvent), Tick, Toast(ToastKind, String),
    Vault(vault::Event),
    Onboard(onboard::Event),
    Esv(esv::Event),
    Secrets(secrets::Event),
    Scripts(scripts::Event),
}
```

Each feature exposes `handle_key(app, key, mode)`, `draw(f, app)`, and
`apply_event(app, ev)`; the global `dispatch`/`draw`/`handle_event` matches
have exactly **one arm per feature**. Plain enums and matches — no trait
framework; the registration cost of a whole new feature is a new directory
plus ~6 one-line arms, and that recipe gets written down in CLAUDE.md.
Adding a modal *within* a feature touches only the feature's `Mode` enum and
its own handler/view — zero global edits.

Same move for the CLI: `Command::Esv(esv::cli::Cmd)` delegating to
`esv::cli::run`, so `cli/mod.rs` drops to parser glue + session commands.

### File splits inside the big verticals

- `screens/esv.rs` (2,106) → `esv/state.rs` (State, Match, EditState, plans),
  `esv/ops.rs` (refresh/save/delete/restart/undo + `apply_*` handlers),
  `esv/screen.rs` (key handlers), `esv/view.rs` (the ~670 rendering lines
  extracted from `ui/mod.rs`).
- `cli/mod.rs` (1,842) → `esv/cli.rs`, `secrets/cli.rs`, `scripts/cli.rs`
  (~700 lines), residual `cli/mod.rs` ≈ 400 lines.

## 3. Documentation findings

### Stale / wrong (an agent reading these gets a false map)

| Doc | Problem | Action |
|---|---|---|
| `CLAUDE.md` §9 | Says `src/main.rs # stub — TUI implementation comes in Step 2`; project is ~21k lines with agent + CLI + 4 screens | Rewrite with the vertical layout + routing table |
| `docs/api/README.md` index | Missing `12-script-bindings-matrix.md`, `13-script-contexts.md`, `bindings/` | Add rows |
| `docs/handoff.md` | Snapshot of an in-progress yubikey branch, long merged; references `src/yubikey.rs` (now `security_key.rs`) | Delete |
| `PLAN.md` | "Step 5 (next): ESV edit + apply" — done; "Deferred app.rs screen-split" — done | Refresh to current truth; keep as the single roadmap |
| `script-linting-uplift-plan.md` (repo root) | Completed plan; describes a "current state" that no longer exists | Delete (or `docs/archive/`) |
| `README.md` status paragraph | Hand-maintained status duplicate of PLAN.md, already drifting | Point at PLAN.md instead |
| `docs/*.html` (keybind / TUI-async / undo design, ~93 KB) | Decision records in HTML — bulky to load, easy to miss | Extract still-binding rules into markdown (`docs/design/`), archive the HTML |

### Gaps in CLAUDE.md (tripwires that live only in module docs or memory)

- **Agent restart gotcha**: code changes under `src/agent/` only take effect
  after `aic stop` + relaunch — `logout`/lock keeps the old binary resident.
  (Currently documented only in `agent/mod.rs`; an agent iterating on the
  daemon will "fix" the same bug twice.)
- **`TEMPLATES_VERSION` bump rule**: any edit under
  `src/aic/script/templates/` must bump the constant in `workspace.rs` or
  scaffolded workspaces never receive the update.
- Test/verify workflow: `cargo test`, the rhino-script-tester probe loop, and
  when to use each.
- Pointer to `docs/DESIGN.md` (TUI visual rules) so UI changes follow them.

### Keeping docs/api current (explicit goal)

The verify-then-document loop in CLAUDE.md §1–2 is sound; the index drift
shows the weak point is *discoverability*, not process. Two cheap additions:

1. **Bidirectional links**: each vertical's `mod.rs` header names its
   docs/api file(s) (several already do); each docs/api file gains an
   "Implemented in: `src/esv/`" line. Doc-first or code-first, the agent
   finds the other half in one hop.
2. **CLAUDE.md routing table** (the core of the new §9):

   | Feature | Code | API doc |
   |---|---|---|
   | ESV variables | `src/esv/` | `docs/api/03-esvs.md` |
   | ESV secrets | `src/secrets/` | `docs/api/03-esvs.md` |
   | Script sync | `src/scripts/` | `04`, `11`, `12`, `13` |
   | Auth/tokens/agent | `src/aic/`, `src/agent/` | `00`, `01`, `02` |
   | Local vault/unlock | `src/vault/` | — (local only) |
   | Onboarding | `src/onboard/` | `00-auth.md` |

## 4. Migration plan (each step lands green: `cargo check && cargo test && cargo fmt`)

**Phase 0 — docs only (no code risk, do first):**
fix `docs/api/README.md` index; rewrite CLAUDE.md §9 + add the gotchas above;
delete `docs/handoff.md` + `script-linting-uplift-plan.md`; refresh PLAN.md;
trim README status. *(~1 session)*

**Phase 1 — pilot vertical: `src/scripts/`.**
Most self-contained, biggest single payoff (frees ~700 lines out of
`cli/mod.rs`). Move `aic/script/*` + `screens/scripts.rs` + `ui/scripts.rs` +
the `ScriptCommand` block; nest `scripts::Mode` / `scripts::Event`; write the
vertical's `mod.rs` header with doc links. This validates the whole pattern —
review it before replicating. *(~1 session)*

**Phase 2 — `src/esv/` + `src/secrets/`.**
Includes the two hairiest mechanical steps: splitting `screens/esv.rs` and
extracting ESV rendering from `ui/mod.rs`. Do esv first, secrets immediately
after (they share the poll seam).

**Phase 3 — `src/onboard/` + `src/vault/`.**
Onboard fixes the `aic → ui` violation. Vault gathers unlock/auth_setup/
auth_settings/security_key. Delete the app.rs compat shims here.

**Phase 4 — residue.**
`undo/` vertical; shared chrome into `tui/`; `app.rs` shrinks to coordinator;
`cli/mod.rs` to parser + session commands. Final CLAUDE.md routing-table pass
to match reality.

**Operational notes for whoever executes:**
- Behaviour-preserving throughout; no feature work mixed in.
- Delete compat shims at the move, never re-export old paths.
- After touching `src/agent/`, test against a *restarted* agent (`aic stop`).
- Commit per phase at minimum, per vertical ideally.

## 5. Out of scope / explicitly not proposed

- No trait-object plugin framework for features — plain enums + one match arm
  per feature is cheaper to read and grep.
- No change to the agent protocol, crypto, undo semantics, or any HTTP path.
- No new dependencies.
- `prod_confirm` stays shared in `app/` (it guards every feature's writes by
  design).
