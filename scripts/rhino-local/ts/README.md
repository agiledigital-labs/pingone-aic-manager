# rhino-local TypeScript package

Two jobs, one package:

1. Generate the **scripted-decision mock binding surface** from the captured AM
   contexts metadata at `docs/api/bindings/scripted-decision-next.json`.
2. Talk to the long-lived JVM runner (`scripts/rhino-local/run-runner.sh`) so a
   test can eval a script without starting a JVM per case.

The generator emits presence, not behaviour: every method throws
`rhino-local: not mocked: <binding>.<method> arity=N overload=[…]` until the
handwritten overlay in `src/bindings/rhino/runtime.cjs` replaces it. A mock
that returned `undefined` would turn a missing feature into a passing test.

Implemented (still fail-loud for missing fixtures and for every method the
overlay does not replace): `nodeState` get/putShared/putTransient/isDefined,
legacy `sharedState`/`transientState` when `given.engine === "legacy"`,
request maps, `outcome` / `action.goTo`, `logger`, `openidm`, `httpClient`,
`callbacksBuilder` (the six authenticate-response types), `systemEnv`,
`idRepository.getIdentity`, next-gen `require()` (CommonJS eval of seeded
library bodies; a missing id throws naming `given.libraries`), legacy
`JavaImporter` + `Action.send(HiddenValueCallback)`. End-to-end cases live
in `cases/`.

## JVM runner client

`src/runner.ts` spawns `run-runner.sh` (docker + the AM image JDK) and speaks
line-delimited JSON. Correlate by job `id` — do not assume the JVM answers in
order. A crashed JVM rejects every pending job rather than hanging. `close()`
ends stdin and waits for the process; `afterAll` should call it.

Job shape (open enough to carry later mock bindings):

- `source` / `sourceName` — author's script; `sourceName` is Rhino's source
  name, so a parse error names the author's file and line, not `<eval>`
- `globals` — JSON values placed in ENGINE_SCOPE Bindings
- `preamble` — evaluated first so concatenating mocks into `source` is not
  needed (that would shift line numbers)
- `timeoutMs` — per-job; AM's `Error("Interrupt.")` past the instruction
  observer. Omit for the runner's 10s harness default; `0` is AM's no-timeout

Outcomes are machine-readable and distinct: `ok`, `compile_error`,
`runtime_error`, `timeout`.

## Artefacts

Committed under `generated/`:

- `scripted-decision-mocks.cjs` — evaluated inside AM's Rhino as part of the
  script-under-test's scope
- `scripted-decision-mocks.d.ts` — TypeScript types for case authors

They are committed on purpose. Regenerating after Ping adds a method produces a
diff of the surface, which is the point of generating rather than hand-writing.

## Re-run

From this directory (Node 24):

```bash
npm install
npm run generate
npm test
npm run typecheck
npm run lint
npm run lint:am
npm run measure   # JVM startup + per-job timings (needs docker + AM image)
npm run show-log  # fetch AIC logs for a failed test (needs `aic login`)
```

When a test that hit the AIC lane fails, `useLease` appends a JSONL record
under `failures/` (gitignored, per-checkout). `npm run show-log` lists those
newest first and fetches with `aic logs tx` — the stem first, so one call
covers a whole authenticate chain. It never issues a range query. Logs open
in `$LOGS_EDITOR` or `$EDITOR`; with neither set they print to stdout.

## Per-file AIC lease

Opt a suite into automatic local-versus-AIC checking through `useLease`:

```ts
const lease = useLease(suite, {
  aic: {
    id: "resolve-identity",
    realm: "alpha",
    unsupported: "fail",
  },
});
```

`id` is a stable, repository-chosen lease identity. The adapter opens one
deterministically named journey before the file, replaces and confirms its
subject script for each run, invokes it, and deletes the graph after the file.
`realm` defaults to `alpha` and seeds the same realm into the local lane;
`tenant` selects an AIC context, and `project` selects the CLI project and
binary location. Both are optional and otherwise use the current defaults.
The returned `RunResult.conformance` contains the per-pass AIC verdict,
disagreements, and observation gaps. A file may have only one AIC-enabled
`useLease`; split different scripts or outcome vocabularies into separate test
files.

Unsupported input fails by default. `unsupported: "skip"` is the explicit
opt-out for a suite that must retain skip behaviour. `check()` and `cleanup()`
still run against the local store only: no tenant-backed `IdmHandle` exists,
so an AIC-enabled report records an `openidm` observation gap instead of
claiming those hooks ran remotely.

Every AM write is followed by a confirming read. A process lock excludes the
same lease on one host, and an identifier-only journal makes stale owned
resources discoverable after process death. Simultaneous use of the same
`aic.id` on different hosts is unsupported because no atomic AM create
precondition has been established. Generated subject and session-minter source
can contain test seeds while a lease is open; never put credentials or real
secrets in them.

For capacity planning only, the design estimate uses `O` distinct subject
outcomes after adding `true` and `false`, and `R = 2O + 3` graph resources. For
`N` one-pass cases without managed fixtures or sessions, the estimated call
count is `2 + 4R + 3N`: two CLI session calls, three calls per resource at
open, one delete per resource at close, and three calls per case. At the
smallest `R = 9`, that is an estimated `38 + 3N`, or 68 calls for ten cases.
Each additional outcome adds an estimated eight file-lifetime calls. These are
design estimates, not tenant measurements; step chains, managed fixtures,
session minting, and credential refresh add calls.

`runAicChain()` remains the one-shot compatibility facade for external callers.
It provisions and deletes a throwaway graph per call. `useLease()` uses the
pre-opened file runner and does not route through that facade.

`npm run generate` reads the captured JSON (offline; it does not call the
tenant) and overwrites the two artefacts. Completeness tests fail if the
artefacts drift from the JSON, if a method is dropped, or if you forget to
regenerate after changing the emitter.

## The `.cjs` is AM-safe JavaScript

The generated mock is **not** Node. AM's Rhino 1.7.14 rejects or silently
miscompiles a large slice of ES2015; the rules are runtime-verified in
`docs/api/12-script-bindings-matrix.md` and encoded in
`src/scripts/templates/am/eslint.config.js`. The emitter stays inside that
subset:

- `var` at top level; **no `let`**; **no top-level `const`** (parses, then
  reads back `undefined` in a scripted-decision script)
- **no `for...of`**, object shorthand, destructuring, or default parameters
- **no `Map` / `Set` / `Symbol` / `Promise` / `WeakMap` / `WeakSet`**
- `function () { … arguments … }` for methods (no rest parameters)
- `const` _inside a function_ is allowed; this file just does not need it

`npm run lint:am` runs those Rhino rules over `generated/*.cjs`. The generator
itself is ordinary TypeScript and has no such limits.

## What the JSON does not name

Empty-`elements` bindings (`requestHeaders`, `requestParameters`,
`requestCookies`, `locales`) are emitted as `{}` — a seam a test case seeds
directly. Scalar bindings (`realm`, `scriptName`, `cookieName`) start as
`"__rhino-local-unseeded__"`; `resumedFromSuspend` starts as `false`.
