# Rhino Script Tester

Temporary harness for probing AM scripted decision runtime behavior in the sandbox tenant.

The goal is to keep the test loop small:

1. Set up one next-gen scripted decision node and one journey.
2. Edit a standalone JavaScript test script.
3. Upload only the script body.
4. Invoke the existing journey.
5. Fetch transaction logs only when the invoke response is not enough.

## Prerequisites

- `cargo build --locked --offline` has built `target/debug/aic`.
- The local `aic` agent is running, unlocked, and has a token for the sandbox tenant.
- `curl`, `jq`, and `base64` are available.
- For logs, `.envrc` exports either:
  - `LOG_API_KEY_ID` and `LOG_API_KEY_SECRET`, or
  - `API_KEY_ID` and `API_KEY_SECRET`.

No log API keys should be committed. `.envrc` is ignored by the repo.

## Tenant Resources

By default the harness uses these sandbox resources:

- Script: `AIC Rhino Let Probe`
- Journey: `AIC-Rhino-Let-Probe`
- Script ID: `2e87a29c-0e30-4d85-bf0e-a1c0a11e7001`
- Node ID: `2e87a29c-0e30-4d85-bf0e-a1c0a11e7002`

The script is created as a next-gen scripted decision script:

- `context: AUTHENTICATION_TREE_DECISION_NODE`
- `evaluatorVersion: 2.0`

Override `BASE`, `REALM_PATH`, `TENANT`, `SCRIPT_NAME`, `TREE_NAME`, `SCRIPT_ID`, or `NODE_ID` if needed.

## One-Time Setup

Run this only when the tenant resources are missing or the journey shape changes:

```bash
scripts/rhino-script-tester/setup.sh
```

Setup uploads the default test script, creates or updates the scripted decision node, and creates or updates the journey. It does not invoke the journey.

## Normal Test Loop

Use `test-cycle.sh` for the usual edit/upload/run cycle:

```bash
scripts/rhino-script-tester/test-cycle.sh scripts/rhino-script-tester/scripts/rhino-let-behaviour.script.js
```

That script calls:

```bash
scripts/rhino-script-tester/update-script.sh <script.js>
scripts/rhino-script-tester/run-journey.sh
```

`update-script.sh` only updates the AM script body. It does not update the tree or node.

## Current Probe Scripts

- `scripts/rhino-let-behaviour.script.js` intentionally uses `let`.
- `scripts/rhino-var-control.script.js` is a `var`-only control script.

Current observed behavior in the sandbox:

- `rhino-var-control.script.js` returns HTTP 200 and the expected hidden callback JSON.
- `rhino-let-behaviour.script.js` fails before callbacks are returned. Logs report a Rhino parse error at the first `let` declaration: `missing ; before statement`.

This gives us a working baseline for validating ESLint rules against real Rhino behavior.

## Logs

When `run-journey.sh` fails, copy the printed transaction id and fetch logs:

```bash
scripts/rhino-script-tester/get-transaction-logs.sh <transaction-id>
```

By default logs are written to:

```text
tmp/rhino-script-tester/logs.json
```

The repo-local `tmp/` directory is ignored. Logs can be large, so summarize or filter them before sharing output.

## Batch Probe Runner

`run-probes.sh` uploads each fixture in `fixtures/`, invokes the journey, and
records structured results to `tmp/rhino-script-tester/probe-results.json`:

```bash
scripts/rhino-script-tester/run-probes.sh                         # all fixtures
scripts/rhino-script-tester/run-probes.sh fixtures/arrow-function.script.js
FETCH_LOGS=1 scripts/rhino-script-tester/run-probes.sh ...        # also pull per-fixture logs
```

Each fixture probes ONE parse-sensitive feature in isolation (so a parse error
is attributable) or one grouped runtime check. Result semantics:

- `callback: parsed` + `HTTP 200` → the feature parsed and ran; `payload.value`
  is the observed result (a missing `value` key means the expression evaluated
  to `undefined` — a silent Rhino bug, e.g. top-level `const`).
- `callback: no-callback` + `HTTP 401` → the script failed to **parse** (the node
  threw before emitting a callback and the journey failed). Run with
  `FETCH_LOGS=1` to capture the `org.mozilla.javascript.EvaluatorException` text.

## Probe Findings (next-gen scripted decision, 2026-06-03)

Full matrix with provenance: `docs/api/12-script-bindings-matrix.md`. Summary:

- Works: `var`, `const` in a function, arrow functions, template literals, and
  ES2015 `Array`/`String`/`Object` methods (`includes`, `find`, `from`,
  `startsWith`, `endsWith`, `repeat`, `assign`, `keys`).
- Parse errors: `let` (any scope), object shorthand, object destructuring,
  default parameters, `const` in `for`/`for-in`/`for-of` initializers, and the
  same `const` name re-declared in one function across separate non-nested
  blocks (Rhino scopes `const` to the function for redeclaration; verified
  2026-06-06 — `fixtures/const-dup-across-blocks.script.js` 401s while
  `const-uniq-across-blocks.script.js` runs).
- Parses but silently `undefined` (worse than a parse error): `const` at top
  level and `const` declared anywhere in a loop body, including nested
  `if`/block bodies (`fixtures/const-in-nested-loop-block.script.js` returned
  `value: ",,"`, verified 2026-07-03).
- Bindings present: `require`, `openidm`, `httpClient`, `utils`, `logger`,
  `idRepository`, `nodeState`, `action`, `callbacks`, `callbacksBuilder`,
  `requestHeaders`, `requestParameters`, `requestCookies`, `realm`, `systemEnv`,
  `scriptName`, `secrets`, `resumedFromSuspend`, `JavaImporter`.
- Bindings absent (`undefined`): `sharedState`, `transientState`,
  `existingSession`, and all Node globals (`console`, `process`, `Buffer`,
  `setTimeout`).

## Legacy engine probes (evaluatorVersion 1.0)

`setup.sh` and `update-script.sh` take `EVALUATOR_VERSION` (default `2.0`). To
stand up a separate legacy probe (own script/node/tree, so the next-gen probe is
untouched):

```bash
SCRIPT_NAME="AIC Rhino Legacy Probe" TREE_NAME="AIC-Rhino-Legacy-Probe" \
  SCRIPT_ID=2e87a29c-0e30-4d85-bf0e-a1c0a11e7101 \
  NODE_ID=2e87a29c-0e30-4d85-bf0e-a1c0a11e7102 \
  EVALUATOR_VERSION=1.0 \
  scripts/rhino-script-tester/setup.sh scripts/rhino-script-tester/fixtures-legacy/legacy-bindings.script.js

SCRIPT_NAME="AIC Rhino Legacy Probe" TREE_NAME="AIC-Rhino-Legacy-Probe" \
  EVALUATOR_VERSION=1.0 FETCH_LOGS=1 \
  scripts/rhino-script-tester/run-probes.sh \
  "$PWD/scripts/rhino-script-tester/fixtures-legacy/legacy-bindings.script.js"
```

Legacy has no `callbacksBuilder`, so `fixtures-legacy/` emit results via the
classic `JavaImporter` + `Action.send(HiddenValueCallback)` path.

Findings (2026-06-04), vs next-gen:

- Legacy-only: `sharedState`, `transientState`.
- Next-gen-only (absent in legacy): `action`, `callbacksBuilder`, `openidm`,
  `utils`, `requestCookies`.
- Present in both: `nodeState`, `callbacks`, `idRepository`, `httpClient`,
  `requestHeaders`, `requestParameters`, `resumedFromSuspend`, `secrets`,
  `JavaImporter`, `logger`, `realm`, `systemEnv`, `scriptName`.

`fixtures-legacy/legacy-nodestate-logger.script.js` enumerates the legacy
`nodeState` and `logger` method surfaces:

- legacy `nodeState`: `get`, `getObject`, `putShared`, `putTransient`,
  `mergeShared`, `mergeTransient`, plus undocumented `isDefined`/`remove`. No
  `nodeState.sharedState(key)`/`transientState(key)`/`secureState(key)` —
  `get()` is the unified accessor; standalone `sharedState`/`transientState`
  bindings provide direct access.
- legacy `logger`: classic Debug — `error`/`message`/`warning` + `*Enabled`;
  `trace`/`debug`/`info`/`warn` are absent (those are next-gen slf4j).
