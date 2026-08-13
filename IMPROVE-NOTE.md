# TypeScript endpoint framework improvement note

## Defects fixed

1. **Private fault data no longer crosses the IDM boundary.** Internal faults
   still carry `__aicCrestFault` and `reason` for duck-typed discrimination and
   logging, but the router now projects them through `toCrestResponse` before
   throwing. The projected object has exactly `code`, `message`, and `detail`.
   The router test asserts the exact key set, the absence of every `__`-prefixed
   key, and the absence of `reason`. Live verification returned no private key.
   IDM itself added the standard `reason` field to the HTTP representation; the
   script did not supply it. The previously documented claim that IDM returned
   a four-field thrown object verbatim was corrected in the verified API notes
   and quirks log.

2. **Bundle and manifest publication is atomic.** `tools/build.mjs` now writes a
   uniquely named temporary file in the destination directory and publishes it
   with `renameSync`. Cleanup runs in `finally`. Both generated `.cjs` files and
   `.aic-ts-manifest.json` use this path, so a watcher can see the complete old
   file or complete new file, never a partial write. The full build completed
   for both endpoints and left no temporary files.

3. **Scoped OpenAPI operations use a valid OAuth2 scheme.** The generator now
   declares an `oauth2` client-credentials flow at `/am/oauth2/access_token`,
   builds its `scopes` map from the union of all route scopes, and retains each
   route's `x-required-scopes`. Tests enforce that every non-empty security
   scope array refers only to an `oauth2` or `openIdConnect` scheme. Both demo
   documents also pass the offline OpenAPI 3.1 meta-schema from
   `@hyperjump/json-schema`.

4. **Query results are complete by construction.** `CrestQueryResult` and the
   ambient `IdmQueryResult` now require all paging fields. The public
   `queryResult(rows, options)` helper fills them with integer/default values,
   and every demo query route uses it. `queryResultSchema` describes the same
   complete envelope. Unit tests cover defaults and explicit paging; the live
   widget query returned HTTP 200 with `resultCount`, cookie, total, remaining,
   and policy fields. IDM normalized `remainingPagedResults` to `-1`, which is
   now documented as CREST-owned response metadata.

5. **Demo success schemas are substantive.** All eleven routes now declare a
   response schema. Shared widget, query-envelope, import, daily-report,
   summary, and report-query schemas keep the two endpoints consistent. Tests
   assert that every demo route has a non-empty schema, and both complete
   generated documents pass the OpenAPI 3.1 meta-schema.

The authorization-before-validation order was deliberately retained. A comment
at the call site records that validation detail would disclose the request
schema to an unauthorized caller, and a table case pins an invalid body plus a
missing scope to 403.

The generated-bundle linter now rejects unbound global references to both
`Reflect` and `Proxy`. Its test rejects `Reflect.get` and `new Proxy` while
allowing a qualified property such as `helpers.Reflect`. The current generated
bundles do not contain Babel's guarded `Reflect.construct` helper, so no special
exception was needed and the rule remains narrow.

## Reduction

Counts use the checked-in template at the starting commit versus this result;
generated bundles and `node_modules` are excluded.

| Area | Before | After | Change |
| --- | ---: | ---: | ---: |
| TypeScript test source | 1,585 lines | 1,450 lines | -135 (-8.5%) |
| Node-discovered test cases | 106 | 147 | +41 |
| Validation tests | 226 lines | 195 lines | -31 |
| Router tests | 403 lines | 292 lines | -111 |
| OpenAPI shape tests | 216 lines | 182 lines | -34 |
| Demo endpoints | 368 lines | 343 lines | -25 |
| Entire TypeScript template | 4,850 lines | 4,822 lines | -28 net, including all new fixes and tests |

Validation cases now share `accepted`/`rejected` fixtures and one runner.
Router successes, failures, and invalid declarations use data tables. OpenAPI
shape assertions use a tuple table. Every old semantic case remains represented;
splitting formerly grouped assertions into named rows plus the new meta-schema,
response-schema, wire-projection, and runtime-ban checks explains the higher
discovered-case count.

The demos were shortened by sharing response schemas, using `queryResult`,
collapsing filter logic, and removing route prose and audit repetition that did
not demonstrate additional framework behavior. All eight widget routes and all
three report routes remain, as do the comments demonstrating inferred handler
types and non-obvious IDM behavior.

Two areas were deliberately not reduced:

- `validate.ts` already made each validator own its JSON Schema and OpenAPI
  already consumed `validator.schema`; the suspected pair of duplicate schema
  walkers did not exist.
- Runtime-constraint comments, CREST mapping rationale, safe logging behavior,
  and native-`Error`/Rhino warnings remain load-bearing. Further compression
  there would obscure security or tenant-runtime constraints.

The task's opening estimate of 212 tests did not match the test runner. The
starting tree discovered 106 tests, agreeing with the later statement in the
task; 106 is therefore the before count used above. The cited defects themselves
were present at the approximate locations given.

## Gates

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed with zero
  warnings.
- `cargo test --workspace`: 608 passed, 0 failed; one doc test remained ignored.
- `npm run check`: strict project and test type-checks passed, ESLint passed,
  147/147 tests passed, and both ES5 bundles built successfully.
- `git diff --check`: passed.

Every cargo invocation used the worktree-local `.cargo-target` directory.
`TEMPLATES_VERSION` was bumped from 51 to 52 so managed framework changes reach
existing workspaces.

## Live verification — 2026-08-13

Only the existing `aicdemo-a1-claude-widgets` and
`aicdemo-a1-claude-reports` endpoint names were used. Credentials remained in
process memory and are not recorded here.

- Deployed widget and report configs: HTTP 201 for each; both runtime endpoints
  registered successfully.
- Fault projection: widget query with `limit=1000` returned HTTP 400. The body
  keys were `code`, `detail`, `message`, and IDM's synthesized `reason`; there
  was no `__aicCrestFault`, no other `__` key, and no unknown internal field.
- Query envelope: widget query with `limit=1&offset=0` returned HTTP 200 with
  one row, `resultCount=1`, null cookie, `totalPagedResults=3`, integer
  `remainingPagedResults=-1`, and `totalPagedResultsPolicy=EXACT`.
- Authorization order: calling the `retire` action without
  `aicdemo:widgets:write` returned HTTP 403 with the missing scope identified.
- Cleanup: both endpoint configs returned HTTP 200 on delete. Subsequent reads
  of both config URLs and both runtime URLs returned HTTP 404.

No sync-mapping types or script-watch build integration were added, and no
`Verified against` entry was written from inference.
