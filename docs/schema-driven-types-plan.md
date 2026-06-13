# Schema-driven script types — implementation plan

Status: planned 2026-06-13, not started. High-level workstream + rationale is
in `PLAN.md` ("Schema-driven script types"). This file is the detailed,
codex-ready plan for **Phase 1** (managed-hook object types), with Phase 2
(typed `openidm.*`) and Phase 3 (AM `identity`) sketched as follow-ons.

Goal: the script `.d.ts` files should type domain objects as their **real
per-object field set** instead of `Record<string, any>` / `any`. The field
set comes from the live `managed` schema (`GET /openidm/config/managed`),
which the `aic managed` tool already fetches.

---

## Phase 1 — managed-hook object types (the first codex task)

Type the `object` / `oldObject` / `newObject` bindings in a managed-hook
script as the interface for *that hook's* managed object. Example: editing
`idm/managed/alpha_user/onCreate.cjs` → `object: AlphaUser` with all 70
fields, not `Record<string, any>`.

### 1a. Schema → TypeScript mapping

Source: each `objects[i].schema.properties` (a map of property name →
`{ type, items?, ... }`) plus `objects[i].schema.required` (array of names).
Property `type` vocabulary observed live on the sandbox 2026-06-13:
`string`, `boolean`, `number`, `object`, `array`, `relationship`, `null`.

Mapping:

| schema | TypeScript |
|---|---|
| `"string"` | `string` |
| `"boolean"` | `boolean` |
| `"number"` | `number` |
| `"object"` | `Record<string, any>` |
| `"array"`, `items.type == "string"` | `string[]` |
| `"array"`, `items.type == "relationship"` | `RelationshipRef[]` |
| `"array"`, `items.type` other/absent | `any[]` |
| `"relationship"` | `RelationshipRef` |
| `type` is an array containing `"null"` (e.g. `["string","null"]`) | base type `| null` |

- **Optional vs required:** a property is `name?: T` unless it appears in the
  object's `schema.required` array (then `name: T`).
- **Relationship ref shape** (standard IDM — VERIFY against a live record
  during impl, don't trust this from memory):
  ```ts
  interface RelationshipRef {
    _ref: string;
    _refResourceCollection?: string;
    _refResourceId?: string;
    _refProperties?: { _id?: string; _rev?: string } & Record<string, any>;
  }
  ```
  Emit `RelationshipRef` once into a shared `idm/types/managed/_shared.d.ts`.
- **Wire envelope:** managed instances carry `_id`/`_rev`, but a hook's
  `object` in `onCreate` is pre-persist. Emit `_id?: string; _rev?: string`
  (both optional) on every generated interface to be safe. VERIFY whether
  onCreate `object` actually has `_id`/`_rev` during impl (we have the probe
  harness from the managed-objects work — `docs/api/10-managed-objects.md`).
- Property names can contain non-identifier chars (e.g. `frIndexedDate3` is
  fine, but be defensive) — quote keys that aren't valid TS identifiers.
- Interface name: PascalCase the object name (`alpha_user` → `AlphaUser`,
  `mock_sms` → `MockSms`).

### 1b. Generator module — `src/scripts/managed_types.rs` (NEW, pure)

A network-free, fully unit-testable module:

```rust
/// Workspace-relative path → file contents for every generated managed type.
pub fn generate(schema: &serde_json::Value) -> Result<Vec<(PathBuf, String)>>;
```

Emits:
- `idm/types/managed/_shared.d.ts` — `RelationshipRef` + a header comment
  marking the whole dir as tool-generated.
- `idm/types/managed/<object>.d.ts` per object — the `interface <Object> {…}`
  **plus** the hook bindings scoped to that object:
  ```ts
  declare let object: AlphaUser;
  declare const oldObject: AlphaUser | null;
  declare let newObject: AlphaUser;
  ```

Unit tests (no network) covering: each scalar type, nullable `["string","null"]`,
required vs optional, `string[]`, `relationship` + `relationship[]`,
PascalCase naming, non-identifier key quoting, and a golden small-object
snapshot.

### 1c. Wire it into the workspace

The generator needs the **live schema**, so it runs in the **CLI command
path**, not in the static `workspace.rs scaffold` (which is sync + embedded
templates only).

- `src/scripts/cli.rs`, `WorkspaceCommand::Init` and `Update` handlers:
  after `workspace::init/update(&t)`, do
  `let schema = crate::aic::api::get(&t, "/openidm/config/managed").await?;`
  then `managed_types::generate(&schema)` and write each file into the tree
  (respect the existing `WorkspaceReport` count output). Best-effort: if the
  agent is locked or the fetch fails, warn and continue (the static scaffold
  still succeeded) — don't abort the whole `workspace update`.
- `src/scripts/managed_hooks.rs`, `extra_files`: currently returns empty. Make
  it emit the **per-object leaf** `idm/managed/<object>/tsconfig.json` (one per
  object folder, idempotent rewrite — mirrors `am::leaf_tsconfig`), including:
  `../../types/rhino-1.7.14.d.ts`, `../../types/common.d.ts`,
  `../../types/managed-hook.d.ts`, `../../types/managed/_shared.d.ts`,
  `../../types/managed/<object>.d.ts`. (Path depth: hook file is at
  `idm/managed/<object>/<hook>.cjs`, so `../../types/...` from the folder.)
- `src/scripts/templates/idm/types/managed-hook.d.ts`: **REMOVE** the
  `object` / `oldObject` / `newObject` declarations (they now come from the
  per-object generated file — leaving them would be a duplicate-`declare`
  conflict). KEEP `request`, `resourceName`, `identityServer`, `require`.
- `src/scripts/templates/idm/managed/tsconfig.json`: this was the single
  shared managed tsconfig. With per-object leaf tsconfigs it's redundant —
  either delete it (and its manifest row in `workspace.rs`) or keep it as a
  no-`object` fallback for un-pulled folders. Decide during impl; deleting is
  cleaner.
- `src/scripts/workspace.rs`: bump `TEMPLATES_VERSION` (18→… current is 19, so
  **20**) because `managed-hook.d.ts` changed. Add `idm/types/managed/` to the
  workspace `GITIGNORE` const (generated, not user-authored, regenerated each
  update — keep it out of the user's git).

### 1d. Degradation / ordering notes (call out, don't over-engineer)

- If a hook folder's per-object type file is missing (user pulled a hook
  before running `workspace update` with the new generator, or the object was
  added after the last update), tsc reports "cannot find name 'object'" for
  that folder only. Acceptable; `aic script workspace update` regenerates.
  Print the existing `workspace_update_hint` after a managed-hook pull.
- Generated files are overwrite-safe (tool-owned, never hand-edited).

### 1e. Acceptance (Phase 1)

1. `cargo check && cargo test && cargo fmt --check` clean; new generator unit
   tests pass.
2. Live smoke (agent unlocked): `aic script workspace update` writes
   `idm/types/managed/alpha_user.d.ts` etc.; `aic script pull
   managed/alpha_user.onCreate` writes the per-object leaf tsconfig; opening
   the `.cjs`, `object.` resolves the real fields (verify by running the
   workspace's `npm run type-check` / `tsc --noEmit` if node deps are
   installed, else inspect the generated `.d.ts` + tsconfig by hand).
3. No regression in existing endpoint/schedule/AM typing.

---

## Phase 2 — typed `openidm.*` returns (IDM + AM) — follow-on

Reuse the Phase-1 generated interfaces. Overload the CRUD/query methods on the
resource-path string literal so the editor knows the return type:

- `openidm.read("managed/alpha_user/" + id)` → `AlphaUser`
- `openidm.query("managed/alpha_user", …)` → `QueryResponse<AlphaUser>`
- `create`/`update`/`patch` content + return typed similarly.

Both engines address the same `managed/<object>` paths, so generate **one**
overload set and include it into both the IDM `openidm` declaration
(`idm/types/common.d.ts`) and the AM `OpenIdm` interface
(`am/types/nextgen-common.d.ts`). Generated file e.g.
`{idm,am}/types/managed/openidm-overloads.d.ts`. TS template-literal /
overload resolution on a `string`-typed argument is limited — likely need
literal-typed overloads (`read(resourceName: "managed/alpha_user", …):
AlphaUser`) with a generic `string` fallback returning `any`. Prototype the TS
ergonomics before committing to a shape.

---

## Phase 3 — AM `identity` typing — planned, NOT now (verify first)

The scripted-decision / OIDC-claims `identity` binding exposes managed-user
attributes under **AM-side names that differ from the OOTB IDM property
names** (e.g. IDM `givenName`/`mail`/`telephoneNumber` ↔ AM attribute names).
A typed `identity` needs the IDM-property → AM-attribute **mapping table**,
which we don't have verified.

Required verification pass first (docs-first rule): probe how AM surfaces
identity attributes — `identity.getAttribute("<name>")` keys actually
accepted in a next-gen scripted decision — and record the mapping in
`docs/api/`. Only then generate a typed `identity`. Until then leave
`identity` as the current opaque `Identity`/`AMIdentity` type.

---

## Files in play (Phase 1)

| File | Change |
|---|---|
| `src/scripts/managed_types.rs` | NEW — pure schema→`.d.ts` generator + tests |
| `src/scripts/mod.rs` | `pub mod managed_types;` |
| `src/scripts/managed_hooks.rs` | `extra_files` emits per-object leaf tsconfig |
| `src/scripts/cli.rs` | `WorkspaceCommand::Init/Update` fetch schema + generate |
| `src/scripts/templates/idm/types/managed-hook.d.ts` | drop `object`/`oldObject`/`newObject` decls |
| `src/scripts/templates/idm/managed/tsconfig.json` | delete or demote to fallback |
| `src/scripts/workspace.rs` | `TEMPLATES_VERSION` → 20; gitignore `idm/types/managed/` |

Behavior-preserving for all non-managed kinds. The generator is the only new
surface; everything else is wiring.
