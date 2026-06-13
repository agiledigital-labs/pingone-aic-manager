# Schema-driven script types — implementation plan

Status: **Phase 1 implemented 2026-06-13.** Phase 2 (typed `openidm.*`) and
Phase 3 (AM `identity`) remain follow-ons. High-level workstream + rationale is
in `PLAN.md` ("Schema-driven script types"). This file is the detailed plan for
**Phase 1** (managed-hook object types).

Live verification done during impl (the two flagged checks):
- **RelationshipRef shape** — confirmed against a live `alpha_user` record
  (`authzRoles[]`) 2026-06-13: `_ref`, `_refResourceCollection`,
  `_refResourceId`, `_refProperties{_id,_rev,…}`. Matches the emitted interface.
- **onCreate `object` `_id`/`_rev`** — emitted **optional** by design, which is
  correct for both phases: onUpdate `object` is the stored record (has both),
  onCreate is the pre-persist draft (`_rev` assigned at persist; `_id` is the
  client-supplied `request.newResourceId`). No risky scratch-hook probe needed.
- **Vocabulary coverage** — the live schema (17 generated objects) uses only
  `string`/`boolean`/`number`/`object`/`array`/`relationship` + `["string","null"]`,
  all covered; array `items.type` of `object`/`array` intentionally map to
  `any[]`.

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

**Scoping caveat (`openidm` is next-gen-only in AM).** The `openidm` binding is
**not** present in *legacy* AM scripts — only next-gen contexts get it. The AM
overloads must therefore land **only** in `am/types/nextgen-common.d.ts` (which
the `am::leaf_tsconfig` map already includes solely for next-gen slugs —
`decision-node`, `*-ng`, `device-match`, `social-handler`, the SAML/OAuth2-DCR
next-gen overlays, `pingone-verify`). Do **not** add them to
`legacy-common.d.ts` or the legacy/`oidc-claims` leaves, or the editor would
advertise a binding the runtime doesn't have. In IDM, `openidm` is always
present (endpoint/schedule/managed-hook), so the IDM side is unconditional.
`legacy-common.d.ts` already has no `openidm` (verified 2026-06-13).

### Verified TS ergonomics (prototyped with tsc 2026-06-13)

Overloads keyed on the resource-path argument **do** narrow the return type —
with one limitation and one ordering rule, both proven against tsc:

- **Narrows:** bare collection literal (`openidm.query("managed/alpha_user", …)`
  → `QueryResult<AlphaUser>`; `create("managed/alpha_user", …)` → `AlphaUser`),
  a full string literal, and a **template literal**
  (`` openidm.read(`managed/alpha_user/${id}`) `` → `AlphaUser | null`).
- **Does NOT narrow:** string **concatenation**
  (`openidm.read("managed/alpha_user/" + id)` widens to `string` → hits the
  `string` fallback → `any`). So the win on `read`/`update`/`patch`/`delete`
  (id-bearing paths) requires users to write template literals, not `"…" + id`.
  Document this as the recommended style; query/create/action (bare collection
  path) narrow regardless.
- **Ordering rule (critical):** the generated overlay must be included **after**
  the base `common.d.ts`/`nextgen-common.d.ts` in every leaf tsconfig. In
  declaration merging the later interface's overloads are tried first; TS hoists
  *string-literal* specialized overloads to the top regardless, but **template-
  literal** overloads obey merge order — so base-before-generated is required or
  `read` silently stops narrowing. (Verified both orderings with tsc.)
- Non-managed paths (`config/…`, `internal/role/…`, `system/…`) keep the
  `string` → `any` fallback (verified — no false narrowing).

### Interface / hook-binding split (a forced Phase-1 refactor)

The overloads file needs **all** managed interfaces in scope, so every
openidm-using leaf includes the whole `types/managed/*.d.ts` interface set. But
Phase 1 currently bundles `declare let object: <Object>` into each
`<object>.d.ts` — including them all would declare `object` N times (conflict).
So Phase 2 splits the generator output:
- `types/managed/<object>.d.ts` → **pure** `interface <Object> { … }` only.
- `types/managed/hooks/<object>.d.ts` → the `declare let object`/`oldObject`/
  `newObject` bindings; included **only** by that object's managed-hook leaf
  (a subdir so `types/managed/*.d.ts` globs skip it).
- `types/managed/_shared.d.ts` → `RelationshipRef` **+ new `QueryResult<T>`**.
- `types/managed/openidm-overloads.d.ts` → the merged `interface OpenIdm`
  overloads (references the interfaces by global name; no imports).

### Per-engine differences (the generated overloads file is engine-specific)

The base method signatures differ, so generate an IDM variant and an AM variant:
- **IDM base** (`idm/types/common.d.ts`) today is a `declare const openidm:
  {…}` with arrow-property types — **convert to `interface OpenIdm { … }` +
  `declare const openidm: OpenIdm`** (arrow props can't be overloaded). Methods:
  read/query/create/update/patch (no delete/action in the base).
- **AM base** (`am/types/nextgen-common.d.ts`) is already `interface OpenIdm`
  with read/create/update/patch/delete/query/action; AM `update(id, rev,
  value, …)` vs IDM `update(path, rev, content, …)`. Match each engine's arity.

### Staging

- **Phase 2a (DONE 2026-06-13):** typed `read` + `query` + `create` for both
  engines, the generator split, the IDM interface conversion, `QueryResult<T>`,
  AM-side managed-interface generation, and the leaf include-order wiring. Live
  tsc-verified: template-literal/bare-collection paths narrow; concat and
  non-managed paths stay `any`; managed-hook `object` still types after the
  split.
- **Phase 2b (DONE 2026-06-13):** typed **return** for
  `update`/`patch`/`delete` both engines (update content `Partial<Object>`);
  added the missing IDM base `delete`/`action` fallbacks. `action` return is not
  typed (varies by actionName). Live tsc-verified: returns narrow on
  template-literal managed paths; non-managed paths stay `any`.

**Known limitation (overload resolution, verified with tsc).** Typed *return*
narrowing is reliable, but typed *content input* does NOT hard-error on a typo:
`openidm.update(`managed/alpha_user/${id}`, rev, { bogusField: 1 })` does not
flag `bogusField` — when the literal doesn't fit `Partial<Object>`, overload
resolution silently falls through to the `string` fallback (`any`) instead of
erroring. So `create`/`update` content typing gives autocomplete on a
well-formed literal but not typo-rejection. This is inherent to mixing a typed
overload with a permissive `string` fallback (the fallback is required for
non-managed paths). Don't try to "fix" it by dropping the fallback — that breaks
`config/`, `internal/`, `system/` calls. Same family as the `"path/" + id`
concat limitation. The headline win is return-type narrowing.

Confirm the AM `openidm` presence/shape with a `scripts/rhino-script-tester/`
fixture before shipping (CLAUDE.md §7).

---

## Phase 3 — AM `identity` typing — planned, NOT now (verify first)

The scripted-decision / OIDC-claims `identity` binding exposes managed-user
attributes under **AM-side names that differ from the OOTB IDM property
names**. `identity.getAttribute("<name>")` is keyed by the **AM attribute
name**, not the IDM property name. A typed `identity` needs the
IDM-property → AM-attribute mapping table.

**Mapping source (Ping docs, fetched 2026-06-13 — NOT yet live-verified).**
From the [user identity properties & attributes reference][idmap]. Per
CLAUDE.md §2/§4 this is a Ping docs claim and MUST be verified live (a
`scripts/rhino-script-tester/` fixture calling `identity.getAttribute(...)` in
a next-gen scripted decision) before it drives generated types — Ping docs have
had errors before (Q1/Q2). The page says plainly: *"If you write scripts for AM
that access user profiles, then use AM attribute names."*

[idmap]: https://docs.pingidentity.com/pingoneaic/identities/user-identity-properties-attributes-reference.html

| IDM property | AM attribute |
|---|---|
| `userName` | `uid` |
| `cn` | `cn` |
| `displayName` | `displayName` |
| `password` | `userPassword` |
| `accountStatus` | `inetUserStatus` |
| `givenName` | `givenName` |
| `sn` | `sn` |
| `mail` | `mail` |
| `description` | `description` |
| `telephoneNumber` | `telephoneNumber` |
| `postalAddress` | `street` |
| `city` | `l` |
| `stateProvince` | `st` |
| `postalCode` | `postalCode` |
| `country` | `co` |
| `aliasList` | `iplanet-am-user-alias-list` |
| `applications` | `fr-idm-managed-application-member` |
| `ownerOfApp` | `fr-idm-managed-application-owner` |
| `assignedDashboard` | `assignedDashboard` |
| `assignments` | `fr-idm-managed-assignment-member` |
| `consentedMappings` | `fr-idm-consentedMapping` |
| `custom_<property>` | `fr-idm-custom-attrs` |
| `reports` | `manager` |
| `manager` | `fr-idm-managed-user-manager` |
| `passwordLastChangedTime` | `pwdChangedTime` |
| `passwordExpirationTime` | `pwdExpirationTime` |
| `groups` | `fr-idm-managed-user-groups` |
| `roles` | `fr-idm-managed-user-roles` |
| `kbaInfo` | `fr-idm-kbaInfo` |
| `preferences` | `fr-idm-preferences` |
| `profileImage` | `labeledURI` |
| `_id` | `fr-idm-uuid` |
| `_rev` | `etag` |
| `_meta` | `fr-idm-managed-user-meta` |

Caveats to verify: the `reports` ↔ `manager` vs `manager` ↔
`fr-idm-managed-user-manager` swap is surprising — confirm direction live.

Scope (clarified by maintainer 2026-06-13):
- **`fr-idm-custom-attrs` is still per-field typeable.** It's the single AM
  attribute that holds *all* custom managed-user properties, but it's an object
  — type it as an interface whose fields are the tenant's custom managed-user
  properties (from the Phase-1 schema, the non-OOTB props), so
  `identity.getAttribute("fr-idm-custom-attrs")` returns a typed object rather
  than `any`. Not a black box.
- **This mapping applies only to the core objects, not custom objects.** The
  IDM→AM attribute renaming is for the OOTB managed objects (`alpha_user` etc.).
  Custom managed objects don't get the AM-name treatment, so a typed `identity`
  is generated only for the core objects; custom objects stay opaque.

When Phase 3 starts: move this table into a new `docs/api/` file (e.g.
`docs/api/14-am-identity-attributes.md`) with a dated "Verified against …"
after the fixture run, then generate a typed `identity` (likely a
`getAttribute()` overload set keyed by the AM names, derived by joining this
mapping with the Phase-1 managed-user schema). Until verified, leave `identity`
as the current opaque `Identity`/`AMIdentity` type.

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
