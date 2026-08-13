# Done note — TypeScript custom-endpoint framework

Branch `ts-endpoints-claude`, 4 commits.

## What was built

| Where                                        | What                                                                                                                                                                                           |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/scripts/templates/typescript/`          | The embedded project: `framework/` (router, validate, errors, logging, openapi, types), `tools/` (build pipeline, generated-bundle linter), `tests/`, `src/` (demo endpoints + shared modules) |
| `src/scripts/ts_project.rs`                  | The build's ownership manifest, read by `aic script watch`                                                                                                                                     |
| `src/scripts/workspace.rs`                   | Scaffolding: MANAGED framework, seeded-once `src/`, merged `package.json`; `TEMPLATES_VERSION` 50 → 51                                                                                         |
| `src/scripts/managed_types.rs`               | Also emits `typescript/src/generated/managed.ts` (module form)                                                                                                                                 |
| `src/scripts/cli.rs`                         | `watch` creates a declared-but-untracked endpoint                                                                                                                                              |
| `src/scripts/templates/idm/eslint.config.js` | Manifest-driven exclusion of generated bundles from the hand-written rules                                                                                                                     |
| `docs/typescript-endpoints.md`               | Design doc; `docs/CLI.md`, `docs/api/11`, `docs/api/99`, `CLAUDE.md` updated                                                                                                                   |

The project lives at `workspace/<tenant>/typescript/` and emits
`workspace/<tenant>/idm/endpoint/<name>.cjs` — the existing sync path,
unchanged.

### The authoring API

```typescript
import { defineEndpoint, notFound, route, v } from "../../framework/index.ts";
import { widgetId } from "../shared/widget-key.ts"; // shared by both endpoints

export default defineEndpoint({
  name: "aicdemo-a1-claude-widgets", // must equal the file name
  headers: { "x-request-id": v.optional(v.uuid()) }, // validated on every route
  routes: [
    route({
      method: "action",
      action: "retire",
      path: "/{widgetId}",
      scopes: ["aicdemo:widgets:write"],
      params: { widgetId: widgetId() },
      body: v.object({ reason: v.string({ minLength: 1, maxLength: 200 }) }),
      handler: ({ params, body, log, correlationId }) => {
        log.info("retiring", { id: params.widgetId, cid: correlationId });
        throw notFound("no such widget"); // -> CREST { code, reason, message, detail }
      },
    }),
  ],
});
```

Nothing is annotated. `params.widgetId` is `string`, a `v.integer({max:100})`
query param is `number`, an optional enum is `"a" | "b" | undefined`, and a
`query` handler that returns anything but a `CrestQueryResult` fails `tsc`. I
checked each of those with a deliberately-wrong probe file: all four produced
the expected `TS2322`/`TS2741`.

## Decisions the brief left open

**Toolchain.** esbuild 0.28 + Babel 8 (`@babel/core`, `@babel/preset-env`,
`@babel/parser`, `@babel/traverse`), TypeScript 5.9, ESLint 9 +
typescript-eslint 8. No test-runner dependency: `node --test` runs the
TypeScript directly (node 24 strips types), which is why `tsconfig.json` sets
`erasableSyntaxOnly` and `verbatimModuleSyntax`. TypeScript is pinned `^5.9`
rather than `^7` deliberately — the workspace's existing `package.json` pins
`^5`, and the two should not disagree about the language.

**Type inference.** Validators are `Validator<T>` carrying both `parse` and a
JSON Schema fragment; `route()` is generic over the four input shapes and the
CREST method, so `InferShape<S>` maps the declaration to the handler's argument
type and `ResultFor<M>` constrains the return. `enumOf` uses a `const` type
parameter so a literal array narrows to a union. One validator family covers
body and query/path/header, with the scalars coercing from strings, because
`request.additionalParameters` is a string→string map.

**OpenAPI `?_action=`.** One operation per declared route, disambiguated by a
`:<action>` path-key suffix (`/{widgetId}:retire`, the AIP-136 RPC convention).
An action route _always_ gets the suffix even when it does not currently
collide, so adding a second action later cannot silently move the first one's
key; a `read`/`query` collision on one path moves the `query`. Every operation
carries `x-crest-method`, `x-crest-action`, `x-crest-path`,
`x-crest-synthetic-path-key`, **and** a required `_action` (or `_queryFilter`)
query parameter pinned by a single-value `enum`, so a generated client that
ignores the extensions still sends the right query string. The alternative — one
POST with a `oneOf` body and a free `_action` enum — types nothing per action
and loses the per-action scope requirement, which is the point of the exercise.

**Excluding generated files.** Three different mechanisms, each chosen so the
exclusion follows from a fact rather than a hand-kept list. `tsc`: the banner
carries `// @ts-nocheck`, which also works in the editor (a static `exclude`
glob would have to guess names, and the sync engine requires the plain
`<name>.cjs` filename). ESLint: `idm/eslint.config.js` reads
`.aic-ts-manifest.json` at config-load time, `ignores` those files in the block
that carries `prettier/prettier`, and adds a second block for exactly those
files holding only the three IDM runtime bans. Prettier: the build rewrites a
marked block at the end of `workspace/<tenant>/.prettierignore`.

**`package.json` is merged, not managed or seeded.** Overwriting drops a
dependency the user's endpoints need; seeding-once strands the two-step build on
a years-old esbuild/Babel, which is the failure `TEMPLATES_VERSION` exists to
prevent. `merge_package_json` refreshes the framework's own keys and keeps
everything else, including `name`/`version`/`description`.

**MANAGED vs USER.** Framework, tools, tsconfigs, ESLint config, README and the
framework's own tests are MANAGED. The demo endpoints, the shared modules and
their test are seeded once — including on an `update` that first introduces the
project to an older workspace, which would otherwise leave a framework with no
`src/` to build.

**Errors.** Tagged plain objects (`__aicCrestFault`), never an `Error` subclass,
and the ESLint config bans both `extends Error` and `instanceof …Error`. A
framework fault passes through; a thrown `{ code, message }` from an `openidm.*`
call keeps its status but loses its message; anything else becomes an opaque 500
with the real cause logged only.

## Divergences from the brief

- **The brief's `a1` agent id was never actually supplied** ("Substitute your
  own agent id for `a1` (given below)" — nothing follows). I used
  `aicdemo-a1-claude-widgets` / `aicdemo-a1-claude-reports`, which satisfies
  both the substitution intent and the `aicdemo-a1-*` naming rule in §5.
- **The emitted footer is `__aicMain.default();` exactly**, as the brief asked.
  To get there the default export is a callable carrying `.definition` and
  `.dispatch` rather than the endpoint object.
- **Supplied findings I re-checked**: esbuild's ES5 refusal reproduces verbatim
  (`Transforming const to the configured target environment ("es5") is not supported yet`).
  The two-step build produces output that runs correctly on IDM, and everything
  in the "these all work" row is exercised by the demo bundles. I did **not**
  independently re-measure `Reflect === undefined`, the native-`Error`
  subclassing failure, the 2 MB source ceiling, or the registration lag — I
  designed around them and waited 5–6 s after each deploy, which was always
  enough. Those figures in `docs/typescript-endpoints.md` are attributed to the
  2026-08-13 orchestrator probe, not to me.

### Two things the repo's own docs had wrong

Both found live, both now corrected in `docs/api/11-idm-endpoints.md` with a
dated note as `docs/api/99-…` Q16:

1. **`PUT /endpoint/x/{id}` without a conditional header is a CREST _create_,
   not an update.** `request.method` is `create`, `newResourceId` holds the id,
   `resourcePath` is **empty**, HTTP 201. With `If-Match: *` (or any revision)
   it is an `update` with `resourcePath` set and HTTP 200. `docs/api/11`'s table
   had claimed `update` unconditionally since 2026-06-04. Practical consequence:
   a header-less `PUT .../{id}` lands on the endpoint's **root create** handler.
   The framework surfaces `newResourceId` in `RouteInput` so a create handler
   can honour the client-supplied id.
2. **IDM HTML-escapes every string in a thrown error object** — `<`, `>`, `"`,
   `'`, `=` and backticks become entities, in `message` and in nested `detail`
   alike. `"must be <= 100"` reached the caller as `"must be &lt;&#61; 100"`. I
   rewrote the framework's messages in plain words and added the quirk to the
   doc.

## Live verification (sandbox, 2026-08-13)

Deployed `endpoint/aicdemo-a1-claude-widgets` (8 routes, 45 KB bundle) and
`endpoint/aicdemo-a1-claude-reports` (3 routes, 39 KB) with `PUT` (201), waited
~5 s, exercised **every route in the brief's table plus the failure cases**,
then deleted both (`DELETE` 200) and confirmed 404 on **both** the config URL
and the runtime URL, and that no `endpoint/aicdemo*` id remains in
`/openidm/config?_queryFilter=true`.

Everything below is an observed status code from a real call.

**widgets — success paths.** `query /` 200 (3 results, valid `IdmQueryResult`);
`?status=active` 200 (1); `?limit=1&offset=1` 200 (correct
`remainingPagedResults`); `?tags=mechanical,stock` 200 (comma list, AND
semantics); `read /w-abcd` 200; `read /w-abcd?expand=owner,history` 200 (both
expansions); `create /` **201**; `PUT /w-abcd` **201** as a create-with-id (see
above); `PUT /w-abcd` with `If-Match: *` **200** through the update route;
`PATCH /w-abcd` 200 (patch ops validated); `DELETE /w-abcd` 200; valid
`x-request-id` 200 and used as the correlation id.

**widgets — failure paths.** `limit=1000` 400 `query.limit must be at most 100`;
`limit=abc` 400 `expected an integer`; `status=bogus` 400
`must be one of: active, retired, draft`; `expand=bogus` 400 `query.expand[0]`;
`read /NOPE` 400 `path.widgetId must be a widget id of the form w-xxxx`;
`read /w-zzzz` 404; malformed create body 400 listing **three** issues at once;
unknown body field 400; empty patch-op list 400; `?_action=explode` 404 naming
`supported: ["retire"]`; `DELETE /` (root) 405 with
`allowed: ["query","create","action"]`; `GET /a/b/c/d` 404; malformed
`x-request-id` 400 at `header.x-request-id`.

**Scopes — both directions.** With the service-account token, which does not
hold `aicdemo:widgets:write`, `retire` and `bulkImport` returned 403
`{"missing":["aicdemo:widgets:write"]}`. As the positive control I rebuilt the
same endpoint requiring `fr:idm:*` — which that token **does** hold — and the
same two calls returned 200; `retire` on an already-retired widget returned 409,
`bulkImport` with duplicate names 400, with 0 items 400. So the 403 is a real
`java.util.Set.contains()` check, not a call that always fails. The original
scope was then restored, redeployed, and the 403 re-confirmed.

**reports.** `read /daily/2026-08-13` 200; `read /daily/13-08-2026` 400
`path.date`; `read /widget/w-abcd/summary` 200 (capture in the middle of three
segments); `read /widget/NOPE/summary` 400 — rejected by the **same shared
module** the widgets endpoint uses; `read /widget/w-zzzz/summary` 404;
`read /daily` (missing segment) 404; `query` with `from`/`to` 200;
`groupBy=status` 200; `from > to` 400; missing `from` 400 `is required`.

**`aic script watch`.** Deleted both endpoints from the tenant, then ran
`aic --no-prompt script watch --yes` and touched the two generated files: it
printed `+ created endpoint/aicdemo-a1-claude-reports` /
`+ created endpoint/aicdemo-a1-claude-widgets`, both appeared in
`/openidm/config`, both served traffic, and `aic script status` then reported
them **in sync**. A second run with a rebuilt bundle printed `→ pushed …` for
both and the live response changed accordingly — so the create path hands over
to the ordinary tracked push path.

**Inferred, not exercised:** the prod-tenant guard on the new create path (the
sandbox is not a prod tenant; it goes through the same `sync::create` →
`kind.write` path as a push, which is guarded). The `openidm.*` bindings — the
demo deliberately uses fixtures, per the brief.

## Gates

| Gate                                                    | Result                                                                               |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `cargo fmt --all`                                       | clean                                                                                |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, zero warnings                                                                 |
| `cargo test --workspace`                                | **608 passed**, 0 failed (+14 new: 4 `ts_project`, 7 `workspace`, 3 `managed_types`) |
| TypeScript `npm run type-check` (2 programs)            | clean                                                                                |
| TypeScript `npm run lint`                               | clean                                                                                |
| TypeScript `npm test`                                   | **106 passed**, 0 failed                                                             |
| TypeScript `npm run build`                              | both endpoints emitted                                                               |
| Workspace `idm` ESLint (with the bundles present)       | clean                                                                                |
| Workspace `tools/check-types.mjs`                       | clean                                                                                |
| Workspace `prettier --check am/** idm/**`               | clean                                                                                |

All gates were re-run against a workspace **re-scaffolded from the committed
templates** (`rm -rf workspace/sandbox && aic workspace init`), so the numbers
are for what is on the branch, not for a hand-patched tree.

## Known gaps and what I would do next

- **The demo endpoints ship as seeded user files.** They carry an agent id in
  their names and a real user would delete them. A `--no-example` flag on
  `workspace init`, or moving them to a separate `aic workspace example`
  command, would be cleaner.
- **`watch` cannot adopt an endpoint that already exists remotely but has no
  local snapshot** — `sync::create` refuses with "already exists; use
  `aic script push`". The right fix is a pull-then-push adoption, but that has
  to not clobber the generated file, so it needs its own design.
- **The TS build is not wired into `aic`.** You run `npm run watch` in one shell
  and `aic script watch` in another. A single `aic script watch --build` that
  shells out would be convenient but adds a node dependency to a Rust binary
  that currently has none.
- **OpenAPI response schemas are opt-in** (`response:` on a route) and the demo
  only sets one. Deriving them from the handler's return type would need a
  validator on the way out, which is a real design choice, not an omission I
  could just fill in.
- **No `openidm` typing beyond a hand-written minimum** in
  `framework/idm-globals.d.ts`. The generated `src/generated/managed.ts` gives
  the record types; wiring them into `openidm.read`/`query` the way
  `idm/types/common.d.ts` does for the `.cjs` workspace is the obvious next
  step.
- **Bundle size is ~40 KB for 3 routes**, most of it Babel helpers and the
  framework. Well inside the 2 MB ceiling, but `--minify` on the esbuild pass
  would cut it substantially if that ever matters; I left it off so the deployed
  source stays readable in the AIC console.
- **The `x-request-id` correlation id is validated but not propagated** to
  downstream `openidm.action` calls. A real deployment would want that.
