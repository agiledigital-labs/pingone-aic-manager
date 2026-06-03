# Script Linting And Type Update Plan

This plan updates the script workspace typings and ESLint configuration so they
match the real AIC Rhino runtime closely enough to support the core feature:
editing scripts with accurate type-checking and lint feedback before pushing.

## Current State

- AM scripts are exported into per-realm, per-context folders:
  `am/<realm>/<script-kind>/<name>.cjs`.
- The Rust routing already records AM `context` and `evaluatorVersion`.
- The generated workspace still has top-level AM declaration files:
  `am/src.d.ts`, `am/lib.d.ts`, and `am/oidc.d.ts`.
- Current AM types mix shared Rhino/AM globals with next-gen scripted decision
  globals and `openidm`.
- Current IDM ESLint config appears stale: it references `tsEslint` without an
  import and bans `let`, despite IDM scripts reportedly allowing it.
- Current ESLint configs expose Node globals broadly, which is wrong for a
  Rhino runtime unless a specific script kind proves otherwise.
- The checked-in Rhino tester now verifies the AM scripted decision path:
  `scripts/rhino-script-tester/`.

Known runtime result:

- AM next-gen scripted decision with `let` fails to parse in Rhino 1.7.14:
  `missing ; before statement`.
- AM next-gen scripted decision with equivalent `var` control succeeds and
  returns the hidden callback JSON.

## Target Model

Treat script typing as a matrix of independent axes:

- Product area: AM vs IDM.
- Script family: scripted decision, library, OIDC claims, IDM endpoint, IDM
  schedule, etc.
- Engine generation: `evaluatorVersion` where it affects available bindings.
- Realm: not a typing axis; alpha and bravo should share the same type files.

Do not maintain two unrelated declaration sets for legacy and next-gen scripted
decision scripts. Instead:

- Define one shared scripted-decision base declaration set.
- Layer next-gen-only capabilities on top where verified, especially library
  support and any next-gen-only globals such as `openidm`.
- Keep folder routing by `evaluatorVersion` only if needed to apply those
  overlays cleanly.

## Documentation Research

Before changing the type model, do a focused pass over the current official
PingOne Advanced Identity Cloud scripting documentation and record the
differences in a checked-in feature matrix. This should happen before large
template edits because it determines which bindings belong in common types and
which belong in per-engine overlays.

Primary docs to review:

- Next-generation scripts:
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/next-generation-scripts.html
- Scripted Decision node API:
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/scripting-api-node.html
- Migrate decision node scripts to next-generation scripts:
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/scripting-api-node-migrate.html
- Script bindings:
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/script-bindings.html
- Scripting environment:
  https://docs.pingidentity.com/pingoneaic/am-scripting/scripting-env.html
- Library scripts:
  https://docs.pingidentity.com/pingoneaic/am-scripting/library-scripts.html

Initial findings from the docs:

- Next-generation bindings are available only for specific AM script types,
  including journey decision node scripts and library scripts.
- Only next-generation scripts can use library scripts. Library scripts can also
  import other library scripts.
- Next-generation scripts expose simplified common bindings such as updated
  `logger`, Fetch-like `httpClient`, `utils`, and `openidm`.
- Legacy scripts rely more heavily on Java allowlisted classes and `JavaImporter`.
- Scripted decision node bindings differ by engine version. The docs call out
  next-generation-specific changes such as `action`, `callbacksBuilder`,
  `idRepository`, and `openidm`.

The research output should be a table with these columns:

- Script kind or context.
- Legacy available?
- Next-generation available?
- `evaluatorVersion` value.
- Runtime folder slug.
- Supported globals and bindings.
- Library support.
- Logger shape.
- HTTP client shape.
- Java allowlist or `JavaImporter` requirement.
- Documentation source URL.
- Runtime probe status.

## Proposed Workspace Layout

Replace ambiguous root names like `src.d.ts` with explicit type files:

```text
am/
  types/
    rhino-1.7.14.d.ts
    common.d.ts
    decision-node-base.d.ts
    decision-node-next.d.ts
    decision-node-legacy.d.ts
    library.d.ts
    oidc-claims.d.ts
  eslint.config.js
  tsconfig.json
  alpha/
    decision-node/
      tsconfig.json
      <script>.cjs
    decision-node-legacy/
      tsconfig.json
      <script>.cjs
    lib/
      tsconfig.json
      <script>.cjs
      <script>.js
    oidc-claims/
      tsconfig.json
      <script>.cjs
  bravo/
    ...
idm/
  types/
    rhino-1.7.14.d.ts
    common.d.ts
    endpoint.d.ts
    schedule.d.ts
  eslint.config.js
  tsconfig.json
  endpoint/
    tsconfig.json
    <script>.cjs
  schedule/
    tsconfig.json
    <script>.cjs
```

Leaf `tsconfig.json` files should include only the declarations for that script
kind. For example:

- AM next-gen scripted decision:
  `../../types/rhino-1.7.14.d.ts`,
  `../../types/common.d.ts`,
  `../../types/decision-node-base.d.ts`,
  `../../types/decision-node-next.d.ts`.
- AM legacy scripted decision:
  same base files plus `decision-node-legacy.d.ts`, but no next-gen-only
  library overlay.
- AM library:
  shared AM/Rhino files plus `library.d.ts`.
- OIDC claims:
  shared AM/Rhino files plus `oidc-claims.d.ts`.
- IDM endpoint and schedule:
  IDM/Rhino common plus their own request/context declarations.

## ESLint Strategy

Build one compatibility layer for Rhino 1.7.14 and apply per-kind overlays.

General rules:

- Use `sourceType: "script"`.
- Use an `ecmaVersion` high enough for ESLint to parse syntax we need to ban
  or warn on. Parsing support is not runtime permission.
- Remove Node globals by default: `process`, `Buffer`, timers, `__dirname`,
  `__filename`, and `console` should only be exposed if runtime tests prove
  they exist for a specific script family.
- Keep `const` allowed in AM and IDM, including top level, unless a runtime
  test proves a narrower restriction is required.
- Ban `let` for AM scripted decision scripts. Preserve this as a hard error.
- Do not ban `let` in IDM until tested; if IDM really accepts it, allow it
  there.
- Keep targeted restrictions for verified Rhino bugs, especially `const` in
  `for` initializers or loop bodies if the tester confirms the weird behavior.
- Restrict `require`, `exports`, and typed library path resolution to script
  families that actually support libraries. Current working assumption:
  next-gen scripted decision scripts and AM library scripts only.
- Restrict `openidm` to the script families where it is documented and/or
  runtime-verified. Current working assumption: next-gen scripted decision
  supports it; other AM families should not see it by default.

Per-kind overlays should live in the relevant template configs:

- `src/aic/script/templates/am/eslint.config.js`
- `src/aic/script/templates/idm/eslint.config.js`

The config should use path globs that match the generated workspace:

- `am/*/decision-node/**/*.cjs`
- `am/*/decision-node-legacy/**/*.cjs`
- `am/*/lib/**/*.cjs`
- `am/*/oidc-claims/**/*.cjs`
- `idm/endpoint/**/*.cjs`
- `idm/schedule/**/*.cjs`

## Runtime Verification Plan

Use `scripts/rhino-script-tester/` for AM scripted decision probes.

Immediate probes:

- `let` in AM scripted decision: already verified as parse failure.
- `const` at top level.
- `const` inside a function.
- `const` shadowing or same-name declarations in unrelated scopes.
- `const` in `for` initializers.
- `const` inside loop bodies.
- object shorthand.
- destructuring.
- template literals.
- arrow functions.
- default parameters.
- `Array.prototype` and `String.prototype` methods used by real scripts.
- `require` from next-gen scripted decision.
- `openidm` availability in next-gen scripted decision.
- logger method names and argument formatting.
- `httpClient` request/response shape.

Extend the tester before adding many probes:

- Add a fixture directory with one probe script per feature.
- Add a runner that updates a probe, invokes the journey, and records:
  HTTP status, transaction id, hidden callback JSON, and optional log summary.
- Add an environment variable for `evaluatorVersion` so the same setup can
  create a legacy scripted decision probe when needed.
- Add a log summarizer helper that filters `tmp/rhino-script-tester/logs.json`
  by transaction id and extracts script errors without dumping full logs.

IDM probes can be added after AM scripted decision is stable:

- Start with a temporary IDM endpoint test resource, because endpoints are
  easy to invoke directly.
- Verify `let`, `const`, logger, request, context, and `openidm`.
- Add schedule-specific probes only after endpoint coverage is useful.

## Implementation Steps

1. Complete the documentation audit.

   - Create a feature matrix from the official docs listed above.
   - Compare the docs to existing sandbox scripts in
     `<client-checkout>/sandbox-scripts/{src,lib,oidc}`.
   - Mark each binding as documented, runtime-verified, inferred, or unknown.
   - Use this matrix to decide which declarations belong in common files and
     which belong in legacy or next-generation overlays.

2. Rename and reorganize declaration templates.

   - Move AM type templates into `src/aic/script/templates/am/types/`.
   - Move IDM type templates into `src/aic/script/templates/idm/types/`.
   - Replace `src.d.ts` with explicit `decision-node-*.d.ts` files.
   - Split shared Java/Rhino helper declarations from product/script globals.

3. Update workspace scaffolding.

   - Update `src/aic/script/workspace.rs` `MANAGED` entries for the new type
     paths.
   - Bump `TEMPLATES_VERSION`.
   - Add a managed-file cleanup list for old files:
     `am/src.d.ts`, `am/lib.d.ts`, `am/oidc.d.ts`, and old IDM root files if
     replaced.
   - Keep `.envrc`, logs, `.aic-sync`, and local scratch output ignored.

4. Update AM routing and leaf tsconfigs.

   - Update `src/aic/script/am.rs::leaf_tsconfig` to include the new type
     files.
   - Keep next-gen vs legacy split only as a way to apply overlays; share the
     scripted-decision base declarations.
   - Make library path aliases available only where libraries are valid.
   - Update `extra_files` tests to assert the new includes.

5. Update IDM leaf tsconfigs.

   - Update `src/aic/script/templates/idm/endpoint/tsconfig.json`.
   - Update `src/aic/script/templates/idm/schedule/tsconfig.json`.
   - Ensure endpoint and schedule get different request/context definitions if
     runtime or docs show differences.

6. Rebuild ESLint configs.

   - Fix the IDM config import bug.
   - Extract common Rhino syntax restrictions.
   - Apply AM and IDM `let` behavior separately.
   - Remove broad Node globals.
   - Add path-specific globals for scripted decision, library, OIDC, endpoint,
     and schedule scripts.

7. Update generated workspace docs.

   - Update `src/aic/script/templates/README.md` with the new layout.
   - Explain that type files are managed and refreshed by
     `aic script workspace update`.
   - Document the known AM `let` failure and the runtime-test approach.
   - Link to the feature matrix and explain which claims are documented versus
     runtime-verified.

8. Add automated coverage.

   - Rust tests for:
     - workspace managed file list,
     - cleanup of old managed files,
     - AM folder routing,
     - AM leaf tsconfig includes,
     - library wrapper generation.
   - Template smoke tests:
     - initialize/update a workspace,
     - run `npm run type-check`,
     - run `npm run lint`.
   - Runtime probe results should be documented in
     `scripts/rhino-script-tester/README.md`.

## Migration Notes

`aic script workspace update` currently overwrites managed files but does not
delete managed files that were removed from the template list. That means simply
removing `am/src.d.ts` from `MANAGED` will not make it disappear from existing
workspaces.

The migration needs an explicit cleanup/tombstone step:

```text
old managed path exists + path is known obsolete -> delete on workspace update
```

Only delete files that were always managed by this tool. Do not delete user
scripts, tests, package files, or anything outside the known obsolete list.

## Acceptance Criteria

- Fresh `aic script workspace init` no longer creates `am/src.d.ts`.
- Existing workspaces remove obsolete managed declaration files on
  `aic script workspace update`.
- AM next-gen scripted decision scripts get callbacks, action, nodeState,
  logger, and verified next-gen-only globals.
- AM legacy scripted decision scripts get the shared decision-node base, without
  next-gen-only library affordances unless tests prove otherwise.
- AM `let` is linted as an error.
- IDM `let` behavior is based on runtime verification, not copied from AM.
- `const` remains allowed generally in AM and IDM; only verified Rhino-broken
  patterns are linted.
- Libraries are type-resolvable only where the runtime supports them.
- `npm run type-check` and `npm run lint` pass in a freshly generated workspace.
- Rust tests cover the new template layout and routing.
- Runtime probe documentation records the feature matrix we have actually
  verified.
- A checked-in feature matrix cites official docs for legacy versus
  next-generation binding differences and marks unverified claims explicitly.

## Open Questions

- Does legacy scripted decision expose the same callback/action/nodeState
  objects as next-gen?
- Which AM script kinds expose `openidm`, `httpClient`, and `request`?
- Does IDM truly allow `let` in endpoints and schedules, or only in one of
  those contexts?
- Which Rhino 1.7.14 ES2015 features parse but behave incorrectly in AIC?
- Are AM library scripts allowed to `require` other libraries, or only to be
  required by next-gen scripted decision scripts?
- Should generated library wrappers stay as `.js` ES modules, or should typed
  CommonJS declarations be generated instead to better match Rhino?
