# aic-edit — Roadmap

Updated 2026-06-24. History lives in git — this file only tracks what's
done, what's in flight, and what's next. (Earlier revisions of this file
held the full Step 1–6 implementation narratives; see git history if you
need the archaeology.)

## Done

- **API research** — verified reference in `docs/api/` + the
  `scripts/verify-endpoint.sh` verify-first loop.
- **TUI foundation** — unlock/vault (Argon2id master password and/or FIDO2
  hmac-secret security key, DEK envelope in `wraps.toml`), onboarding
  (cookie / userpass / paste / sandbox import), env picker with per-env
  themes, prod-write guard, undo log + history screen.
- **Agent + CLI** — single `aic` binary; `ssh-agent`-shaped daemon owns
  JWKs, token cache, and the HTTP pool; all tenant HTTP from both surfaces
  goes through `aic::api`.
- **ESVs** — list / fuzzy search / edit / delete / apply (restart) for
  variables, full secrets lifecycle (versions, enable/disable/destroy),
  TUI tab + `aic esv` CLI.
- **Scripts** — typed local workspace (`.d.ts` + ESLint, runtime-verified
  against Rhino 1.7.14), pull/push/sync/watch/status/diff with
  content-based conflict detection, AM scripts + IDM endpoints + IDM
  schedules, TUI tab + `aic script` CLI.

- **Feature-vertical restructure** (2026-06-13) — one directory per feature
  (`esv/`, `secrets/`, `scripts/`, `onboard/`, `vault/`, `undo/`) with
  uniform api/state/ops/screen/view/cli seams, nested per-feature
  Mode/Event enums, `app/` as the only global glue and `tui/` as passive
  chrome. Rationale + phase log:
  [`docs/orthogonality-review.md`](docs/orthogonality-review.md); routing
  map: CLAUDE.md §9.
- **Managed objects** (2026-06-13) — `aic managed list/get` inspects the
  per-tenant IDM schema (`src/managed/`); event hooks sync as first-class
  workspace scripts (`Kind::IdmManagedHook`, `aic script pull
  managed/<obj>.<hook>`) with their own typed/linted template family. Push
  is a read-modify-write of the shared `managed` document with apply-lag
  confirmation. See `docs/api/10-managed-objects.md`.
- **OAuth2 clients** — `aic oauth list/pull/push/delete` + OAuth TUI view
  (`src/oauth/`); strips `-encrypted` fields and plain-PUTs (no `If-Match`).
  See `docs/api/05-oauth2-oidc.md`. (Provider service still to do.)
- **Journeys** — `aic journey list/pull/push/delete/using-script` plus node
  type introspection (`nodes`/`node-schema`/`node-template`); JSON export of
  tree + nodes (`src/journey/`). See `docs/api/09-journeys.md`.
- **Sync mappings** — browse IDM `config/sync` mappings, reconcile (recon), and
  pull/push their embedded behaviour/correlation/transform scripts; Mappings
  TUI view (`src/mappings/`) + the `config/sync` script namespace. See
  `docs/api/16-sync-mappings.md`.
- **Secret mappings** — `aic secretmap` re-points AM secret labels (purposes) at
  existing ESV secrets; surfaced as the ESVs tab's Mappings sub-view
  (`src/secretmap/`). See `docs/api/15-secret-mappings.md`.
- **IDM record store + query** (2026-06-21) — `aic idm sync/query/objects/
  tables/status` syncs managed-object *records* into a per-tenant local SQLite
  store and queries them with SQL, incl. joins into shredded nested arrays
  (`src/idmstore/`). Parallel cursor-paged sync across objects, user-object
  incremental via the `_meta` change timestamp, non-user full re-pull.
- **TUI function selector** (2026-06-24) — replaced the top tab strip with a
  `Ctrl-P` fuzzy modal view switcher (`src/app/selector.rs`), so navigation
  scales as more feature views are added.

## Next

- **Schema-driven script types** (Phases 1 + 2 done 2026-06-13; Phase 3
  remains). Make the editor know the *real* per-object field set. **Landed:**
  managed-hook `object`/`oldObject`/`newObject` are generated per-object
  interfaces, and `openidm.read`/`query`/`create`/`update`/`patch`/`delete` now
  return the typed object (`AlphaUser` etc.) for both engines — generated into
  the workspace at `aic script workspace init/update`
  (`src/scripts/managed_types.rs`). (Return narrowing is solid; content-input
  typo-catching is limited by overload resolution — see the plan.) **Phase 3
  started:** IDM→AM attribute mapping verified live
  (`docs/api/14-am-identity-attributes.md`) and scripted-decision `identity`
  now has typed attribute-name autocomplete (`AmUserAttribute`). **Remaining:**
  extend the same typing to the other identity contexts (oidc-claims-ng / SAML /
  oauth2-dcr / device-match / legacy oidc-claims). See
  `docs/schema-driven-types-plan.md`.

  **Mechanism.** Generate per-object TS interfaces (`AlphaUser`, `AlphaRole`,
  …) from the live `managed` schema — the same `GET /openidm/config/managed`
  the managed tool already fetches; each property carries a `type` and the
  `required` array. **Key design point:** managed schema is per-tenant
  editable, so these types are *generated into the tenant's workspace* at
  `aic script workspace init/update` (re-fetched each update), NOT baked into
  the `include_str!` binary templates like the static `.d.ts` files. New
  generated files live under `idm/types/managed/` + `am/types/managed/`,
  gitignored with the rest of the workspace.

  Three pieces, increasing complexity — the first two are the near-term ask:

  1. **Managed-hook object types.** Type `object`/`oldObject`/`newObject` as
     the interface for the hook's object (e.g. an `alpha_user.onCreate` file
     gets `object: AlphaUser`). Per-object hook folders already isolate this
     (`idm/managed/<object>/<hook>.cjs`), so the generated tsconfig for each
     object folder can declare the right binding type. Mind the wire envelope
     (`_id`/`_rev` plus the schema fields) and schema→TS nullability
     (`required` vs optional, `type: ["string","null"]`).
  2. **Typed `openidm.*` returns (IDM + AM).** Overload `read`/`query`/
     `create`/`update` on the resource-path literal so
     `openidm.read("managed/alpha_user/…")` returns `AlphaUser`, `query`
     returns `QueryResponse<AlphaUser>`, etc. Both engines address the same
     `managed/<object>` paths, so one generated overload set serves the AM
     `OpenIdm` interface and the IDM `openidm` binding.
  3. **AM `identity` object typing** (planned, not immediate — flagged by the
     maintainer). The scripted-decision / OIDC-claims `identity` binding
     exposes managed-user attributes under **AM-side names that differ from
     the OOTB IDM property names** (e.g. IDM `givenName`/`mail`/`telephoneNumber`
     vs the AM attribute names). Needs a **verification pass first**: probe
     how AM surfaces identity attributes (`identity.getAttribute(...)` keys)
     and build the IDM-property → AM-attribute mapping table before generating
     a typed `identity`. Until then leave `identity` as today's opaque type.

  Open questions to resolve during 1–2: how to regenerate cleanly on
  `workspace update` without clobbering user edits (types are generated, not
  edited — safe to overwrite); whether to type `patch`/`action` payloads too;
  how `relationship`/`array`-typed properties map (refs to other managed
  objects → cross-interface references).

- **Managed objects — schema property editing.** Read-only inspection (CLI
  `aic managed` + the **Managed TUI tab**, 2026-06-13) and hook sync are done.
  Next: add/edit properties (PUT replaces the whole document; no `_rev`,
  last-write-wins). Pairs well with the type generation above (same schema).
- **OAuth2 / OIDC** — client CRUD is done (see above); remaining is the
  **provider service** (`docs/api/05-oauth2-oidc.md`).
- **SAML** — hosted/remote entities + CoT (`docs/api/06-saml.md`). Not started.
- **Journeys** — CLI list/pull/push/delete/using-script + node introspection are
  done (see above); remaining is a **TUI browse tab** and a local-vs-tenant
  **diff** (`docs/api/09-journeys.md`).
- **IDM record store** — local query store + SQL is done (see above); next is a
  **TUI Query tab** (SQL editor with type-ahead + sync progress) — the
  `idmstore` `screen`/`view` are stubbed for it.

## Parked / stretch

- **Logs TUI tab** — fetch/sync/search/compact and journey rollup are
  implemented on `logs-feature` (`docs/api/08-logs.md`, `docs/logs-store.md`);
  remaining work is a TUI browse/search tab.
- **Browser-handoff onboarding** for SSO-only admins — blocked by AIC
  platform limitations; see `docs/api/99-quirks-and-open-questions.md`
  Q11/Q12 for the dead ends already explored.
