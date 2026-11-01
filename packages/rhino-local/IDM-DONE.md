# Slice B — IDM emulator core

Built in `src/idm/`: frozen REST-shaped config (`config.ts`, `types.ts`), request
provenance (`provenance.ts`), managed repository/lifecycle (`managed.ts`), script
execution and bindings (`scripts.ts`), filters (`filter.ts`), pointer projection
and patches (`document.ts`), endpoint dispatch (`endpoints.ts`), errors
(`errors.ts`), REST/host routing and forks (`emulator.ts`). Invented integration
fixtures and tests live in `test/idm/`; every script executes on the real JVM.

## Interface and choices

- Workspace entry: `src/idm/index.ts`, exporting `IdmConfig`, `IdmEmulator` and
  types. No new npm entry point: keep the contract experimental until tenant
  conformance tests stabilize it. No test-framework concepts in the core.
- `new IdmConfig({managed, endpoints?})` clones/freezes config; endpoint keys are
  bare names. Both direct scripts and `{type:"scripted", script}` are accepted.
- `new IdmEmulator(config, runner, {policy?})`; `request({method,path,query?,headers?,body?})`
  returns `{status,body,provenance,rawBody?}`. The runner is caller-owned.
- `fork()` shares config and runner and deep-copies records. This is the seam
  for a later copy-on-write store. No disk loader or config mutation.
- `onCall` maps script host operations to the same core and CREST exceptions;
  `hostRequest` also exposes a standalone host call's provenance. Standalone
  `onCall` returns only the JSON value, as required by the runner protocol.
- Optional `rawBody` resolves a necessary gap in `{status,body}`: JSON cannot
  represent the measured malformed response. Root scalar/array reads reproduce
  status 200 and exactly `{"_id":""`; string-valued thrown codes reproduce
  status 500 and zero body bytes. `body` is null in either case.
- `_queryId` has **no named-query error row in doc 24**. Zero/multiple selectors
  and `_queryExpression` use the measured errors; a named query id fails as
  missing. No doc 24 claim was changed or found contradictory.
- Policy is not reconstructed. The evaluator seam runs after validation hooks,
  before store hooks, and accepts rejection via the runner's `HostCallError`.
- Request scopes aggregate nested evidence and keep concurrent jobs independent.
  Missing behavior cannot be hidden by a script catch. A missing nested call
  prevents the outer write from committing, but earlier nested commits remain.
- Small Java exceptions to scope: `IdmRuntime.java` records unsupported helper
  uses in an opt-in `reportMissing` response field. TypeScript cannot observe a
  helper exception that script code catches without this diagnostic. Opt-in
  `crestBindings` supplies request class tags and case-insensitive HTTP headers
  through `IdmValues.java`; ordinary JSON bindings cannot represent those types.
  Default runner responses/bindings and the engine itself remain intact.

## Provenance inventory

Labels below are the literal behavior strings, with angle-bracket placeholders
for runtime values. Measured entries cite docs 10, 11 or 24 on the result.

Assumed (one line per label):

- `context contains originating HTTP fields; authentication is not modeled`
- `invalid request shape uses local 400 status and diagnostic`
- `config snapshot reads are immediately consistent`
- `unknown managed type is a REST 404`
- `unconfigured endpoint is a REST 404`
- `nested host writes commit independently`
- `bare managed POST maps to create`
- `missing-record message omits tenant directory DN`
- `duplicate-create message omits tenant directory DN`
- `revision mismatch message and precondition timing`
- `generic scalar/array materialization follows measured user null/array shape`
- `undeclared attribute checking applies to external and internal writes`
- `caller-supplied policy evaluator`
- `policy not evaluated`
- `revisions are monotonic decimal strings starting at zero`
- `equal-content PUT still runs the write chain`
- `concurrent writes are checked again after hooks`
- `managed update and patch return the stored object with status 200`
- `post-hook mutations affect responses only; post-hook failures retain committed writes`
- `custom action receives object, request, context and resourceName`
- `CREST comparison uses case-sensitive strings and existential array matching`
- `sort nulls first and break ties by insertion order`
- `offset paging with zero page size unbounded and null result cookie`
- `nested field projection on ordinary JSON objects`
- `patch add/replace/remove use JSON pointer mutation semantics`
- `object validation/store value binding uses the current object snapshot`
- `sibling object bindings cross the runner as independent JSON snapshots`
- `executeOnRetrieve request.method is query`
- `Java long values use JavaScript number precision`
- `endpoint query defaults and sortKeys are plain JSON values`
- `endpoint array query paging metadata uses NONE and unknown totals`
- `truncated endpoint read stamps the final subpath segment`
- `<context>: numeric object throw uses CREST status` (contexts other than endpoint/onCreate)
- `property onValidate: other throws are forbidden` (other than string/ReferenceError)
- `endpoint: other script errors yield 500 with message` (other than string/Error)
- `<context>: host CREST exception preserves status and detail`
- `<context>: unmeasured throw yields generic 500`

Missing (one line per label):

- `request parameter: <name>` (unknown underscore-prefixed parameter)
- `config writes (config is read-only)`
- `config read query parameters`
- `managed subresource routing`
- `resource route: <path>`
- `fifth create host argument`
- `fifth action host argument`
- `query separate non-array fields argument`
- `script host consumption of invalid JSON endpoint output`
- `If-None-Match other than star`
- `managed relationship: <type>/<property>`
- `managed collection method <method>` (unsupported collection operation)
- `managed object action: <name>` (undeclared action)
- `named _queryId: <name>`
- `query paging cookies` (non-null; explicit null has measured 500 behavior)
- `nested wildcard field projection`
- `patch operation <operation>` (anything except add/replace/remove)
- `patching managed identity fields`
- `script file reference: <file>`
- `script type: <type>` (anything except text/javascript)
- `script without inline source`
- `runtime helper: <name>` (require, parseFilter, crypto helpers still absent from runner)
- `endpoint query completion type: <type>` (non-null scalar query completion)
- `endpoint <method> null completion` (other than measured read/action/query cases)
- `endpoint <method> non-object completion` (other than measured scalar/array reads)

## Validation log

- Original host suite: 1351 passed, 4 skipped, one sandbox failure
  (`listen EPERM` in the existing local HTTP server test). Collection matches
  the supplied 1352 passed / 4 skipped baseline; no tests removed.
- Final host `npx vitest run --maxWorkers=2`: **1463 passed, 4 skipped** across
  118 files: **111 added tests**, no count drop. Local-network permission allowed
  the original HTTP listener test to pass. Duration 49.99 seconds.
- Final Docker `AIC_SCRIPT_TESTER_JVM=both npx vitest run --maxWorkers=2`:
  **1465 passed, 2 skipped**, 118 files. Duration 68.16 seconds. Used the existing
  Slice A cache. Both new runner options and nested calls passed shadow replay.
- Final focused IDM run: **234 passed** (also selects the existing three
  `idm-*` runtime/host/replay files).
- `npm run typecheck`, `npm run lint`, `npm run lint:am`: all passed, zero
  warnings. No dependencies changed. `git diff --check` passed.
- Staged sensitive-metadata scan passed. Pre-commit also runs the scanner and
  gitleaks. No push.
