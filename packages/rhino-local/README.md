# rhino-local TypeScript package

Two jobs, one package:

1. Generate the **scripted-decision mock binding surface** from the captured AM
   contexts metadata at `docs/api/bindings/scripted-decision-next.json`.
2. Talk to the long-lived host-JVM runner so tests can eval scripts without
   starting a JVM per case.

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

## Using the installed package

```sh
npm install --save-dev @agiledigital/pingone-aic-script-tester vitest zod
```

**Requirements**:

- Node 24.
- A Java 25 **runtime**. The runner classes are prebuilt, so no `javac` is
  needed. Use `AIC_SCRIPT_TESTER_JAVA_HOME` or `JAVA_HOME`, else `java` on `PATH`.
- `vitest` and `zod`, as peer dependencies.

The first run downloads the Rhino jar from Maven Central and checks it by
SHA-256. On a machine without access, point `AIC_SCRIPT_TESTER_RHINO_JAR` at a copy
instead. `npx aic-script-tester-fetch-jar` fills the cache ahead of time.

A suite runs on the local lane by default:

```ts
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { defineSuite, useLease } from "@agiledigital/pingone-aic-script-tester";

const suite = defineSuite({
  name: "greet",
  script: 'if (nodeState.get("userId") === "alice") { action.goTo("known"); } else { action.goTo("unknown"); }',
  outcomes: ["known", "unknown"],
  inputs: z.object({ userId: z.string() }),
});

describe("greet", () => {
  const lease = useLease(suite);
  it("knows alice", async () => {
    const run = await lease.run({ userId: "alice" }).expect({ outcome: "known" });
    expect(run.verdict.summary).toBe("");
  });
});
```

To run the same suite against a tenant, spread `aicWhenEnabled("<unique-id>")`
into the `useLease` options, then run with `AIC_SCRIPT_TESTER_AIC=1`. The tenant
comes from the first of these that is configured:

1. a `provider` passed in the options;
2. a provider registered with `setTenantProvider()`, typically from a Vitest
   `setupFiles` module;
3. the environment: `AIC_SCRIPT_TESTER_TENANT_URL` (https, no path), plus either
   `AIC_SCRIPT_TESTER_SA_ID` with `AIC_SCRIPT_TESTER_SA_JWK` or `AIC_SCRIPT_TESTER_SA_JWK_FILE`,
   plus optional `AIC_SCRIPT_TESTER_LOG_KEY_ID` and `AIC_SCRIPT_TESTER_LOG_KEY_SECRET`;
4. the `aic` CLI (`AIC_BIN`).

A partial environment configuration is an error rather than a fallthrough.

```ts
// test/tenant.setup.ts — list it in vitest.config's test.setupFiles
import { setTenantProvider, tokenCallbackProvider } from "@agiledigital/pingone-aic-script-tester/aic";

setTenantProvider(
  tokenCallbackProvider({
    baseUrl: "https://tenant.example.com",
    // Called again with reason "rejected" after a 401; don't return the same token.
    getToken: async ({ reason }) => fetchBearerSomehow(reason),
    // Optional, and needed only for aic-script-tester-show-log.
    logKeys: { id: process.env.LOG_KEY_ID!, secret: process.env.LOG_KEY_SECRET! },
  })
);
```

### Libraries used by `require()`

Put each next-gen library on the suite, keyed by the exact name passed to
`require()`. The value is the JavaScript source, not a path. Load source from a
file before defining the suite:

```ts
import { readFileSync } from "node:fs";
import { defineSuite } from "@agiledigital/pingone-aic-script-tester";

const suite = defineSuite({
  name: "library example",
  script: 'nodeState.putShared("code", require("codeLookup").code()); action.goTo("done");',
  outcomes: ["done"],
  libraries: {
    codeLookup: readFileSync(new URL("./lib/codeLookup.js", import.meta.url), "utf8"),
  },
});
```

For example, `lib/codeLookup.js` can contain `exports.code = function () {
return "ready"; };`. Another library in the same map can call
`require("codeLookup")`. The local lane loads both from the map; the AIC file
lease provisions absent entries as `LIBRARY` scripts in declaration order,
then creates the subject. AM's write path does not syntax-check scripts;
whether it validates `require()` targets at create time has not been measured.
An A → B → C chain resolves locally; a three-library chain has not yet been
measured on AM. In a diamond (A and B both require C), C runs once per local
pass; AM's evaluation count is unmeasured. In a cycle (A ↔ B), local Rhino
hands B A's partially built exports; AM's cycle behavior is unmeasured.
Declare every name in the suite map: an undeclared name may resolve to a
tenant-only library on AIC and make the lanes disagree.
At close, the lease deletes the subject's graph, replaces each owned library's
source with an empty body, confirms that update by reading it back, then
retries deletion until no further delete succeeds. The blanking step removes
references among owned libraries, including possible self-references in comments;
it can be replayed from a journal with explicit owned status after a lost
response. Unreleased, older library journals without that status are read-only
probes and require operator inspection. The
orchestrator's 2026-09-30 live run after `d7de205` confirmed that AM accepts
the empty-source PUT and the tested lease left no library residue. An owned
library still referenced by a reused external library remains journalled and
is reported as residue at close.
Names are realm-wide. A same-name library with byte-identical source can be
reused only if it has no lease ownership marker. A library owned by another
file lease is refused even when the source matches; the error names its
`aic.id` (or its hash for older markers). Give libraries distinct names or run
them through one lease with one `aic.id`. A different source or script context
found during preflight is an error before any lease write. AM has no measured
atomic create precondition: another writer could create the deterministic ID
between the preflight read and the lease's PUT. If AM returns 200 for that
PUT, the lease may already have overwritten the other library's source. It
marks the ID not owned in its journal, leaves it for operator inspection, and
will not automatically blank, delete, or recreate it on a later open. Use one
host per `aic.id`, as for the other leased resources.
Before blanking or deleting a library it created, the lease checks its current
source and ownership fields; if they changed, it leaves the library and recovery
journal in place and reports a cleanup warning. It never blanks or deletes a
reused library. Legacy scripts
cannot use libraries, and `given.engine: "legacy"` with `libraries` is rejected.

### Callback suspension

On a next-gen pass, queued callbacks make AM pause and wait for a reply. They
win even if the script also called `action.goTo()`: the pass has no outcome.
Expect `outcome: null` and the emitted callbacks, then use `.step({ reply })`
to submit a response and run the next pass. A pass with no queued callbacks
keeps its `goTo` outcome. Legacy `Action.send` behavior is separate.

### ESV declarations

Declare values read through `systemEnv.getProperty("esv.<name>")` with the
`esv` channel. It is available in `always`, per-test `.esv()`, and `beforeRun`'s
`request.esv`. The local lane seeds `given.esv`; the AIC lane reads the tenant's
real ESVs and never writes them. Declare an absent ESV as `null` to test its
one-argument `null` result or a supplied default. An undeclared local read
throws with the missing key, even if other ESVs were declared.
Use only the part after `esv.` as a channel key: for
`systemEnv.getProperty("esv.feature")`, declare `esv: { feature: "on" }`.
`esv: { "esv.feature": "on" }` is rejected instead of becoming
`esv.esv.feature`. A management API ID such as `esv-feature` is also rejected:
the ID does not reliably identify the script property name. Look at the
script's `getProperty` call and declare its suffix.

Before this change, `esv: { feature: "on" }` only wrote `esv.feature` into
shared state. Now it makes `systemEnv.getProperty("esv.feature")` return
`"on"` locally. If a script uses a config library that reads shared-state
overrides, opt in to both behaviors:

```ts
const suite = defineSuite({
  name: "feature check",
  script: 'action.goTo(systemEnv.getProperty("esv.feature") === "on" ? "on" : "off");',
  outcomes: ["on", "off"],
  always: { esv: { feature: "on" }, esvInState: true },
});
// One test can also call lease.run().esvInState() before .expect(...).
```

`esvInState` writes `esv.feature` into shared state on both lanes. With it on,
an input named `esv.feature` is rejected as a collision. A declared local
value can differ from the tenant's ESV; conformance can detect a difference
only when the script exposes it in recorded effects. Check tenant ESV values
before using the AIC lane for value-sensitive tests. Tenant ESV changes require
a restart and are outside the lease.

### Request cookies

Declare cookies with `always: { cookies: { name: "value" } }`, override one
name with `.run().cookies({ name: "other" })`, or edit `request.cookies` in
`beforeRun`. The three sources merge per cookie name. The local lane seeds
`requestCookies`; the AIC lane sends the same values on `/authenticate`.
When a test also requests `session`, the harness adds the tenant's session
cookie and refuses an author cookie with that name instead of overwriting it.

### `scriptName`

The local harness seeds `scriptName` on every run. It defaults to the suite's
`name`; set `scriptName` on `defineSuite` for a different local-only name.
With `useLease(..., { aic: ... })`, the local lane instead uses the exact
generated subject name that AIC uploads (`rl-aic-<hash>-subject`), so scripts
that depend on the binding compare the same value on both lanes. An explicit
suite `scriptName` with AIC enabled fails setup because the uploaded name
cannot be changed to a deployed script's name. A direct low-level `Case` may
still seed `given.scriptName` locally; the one-shot AIC runner rejects that
seed. The one-shot `conform(..., { aic: "tenant" })` path supplies its generated
name to the local runner for the same comparison.
`aicWhenEnabled()` retains an AIC-intent marker when the lane is off, so a
suite with explicit `scriptName` fails setup in both modes. Use an explicit
`scriptName` only for suites that are always local-only.

`cookieName` follows the same fixed-tenant rule. Local-only suites can set it
in `always`, with `.cookieName(name)`, or in `beforeRun` via
`request.cookieName`. With `aicWhenEnabled()`, any explicit author value is
refused even when the AIC lane is off. When AIC is enabled, the lease reads
`/am/json/serverinfo/*` before the local pass and seeds that tenant value;
the one-shot `conform` path discovers it before its local pass too.
For a pre-recorded one-shot chain, choose `oneShotRunId`, seed each local
case's `given.scriptName` and `given.loggerScriptId` with
`oneShotSubjectName(oneShotRunId)` and `oneShotSubjectId(oneShotRunId)`, then
pass that ID to `conformChain`; it checks both values and removes the local-only
seeds before invoking AIC.
For a chain produced by `suite.lease`, use
`suite.lease({ runner, oneShotRunId })` **before** calling `.run()`. The lease
seeds both bindings and `chainFromRunResult(result)` carries the run ID and
identity provenance to `conformChain()`. A plain lease result also carries
provenance, so its AIC runner is called; if the script reads its identity,
choose `oneShotRunId` before the local pass to make the names and IDs agree.
Raw author-seeded Cases remain ineligible for one-shot AIC execution. This
changes the migration path for lease consumers: forwarding a lease's seeded
Case alone loses provenance; forward `chainFromRunResult(result)` instead.

In next-gen decision-node scripts, local `logger.getName()` uses the measured
`scripts.AUTHENTICATION_TREE_DECISION_NODE.<script id>.(<script name>)` form.
With an AIC lease, the ID is its deterministic subject UUID. A local-only
suite uses `<unseeded-script-id>` as an obvious placeholder; a raw `Case` can
provide `given.loggerScriptId`. The logger-name form for other script contexts
has not been measured, and the legacy mock keeps its earlier bare name.

### Match a value's shape

An expected JSON value can be a `RegExp` (for a string) or a synchronous
Standard Schema, such as a zod 4 schema. Matchers work recursively in every
state channel's `added` and `changed` values, callback fields, OpenIDM bodies,
and HTTP request bodies:

```ts
import { z } from "zod";

const run = await lease.run().expect({
  outcome: "done",
  sharedState: {
    added: {
      tracking: { id: z.string().uuid(), token: /^[0-9a-f]{32}$/ },
    },
  },
});
```

Use `z.unknown()` when a key must be present but any JSON value is acceptable.
The key is still declared: missing keys, wrong state buckets, extra object
fields, and undeclared mutations still fail. Each lane validates its own value;
the lane comparison ignores value differences only at declared matcher leaves
and keeps presence, location, and every exact sibling strict. AIC can check a
matcher only when that channel is observable. For example, state on a pass
that suspends with callbacks is unverified because the result node did not run.
`allowUndeclared: { sharedState: true }` is the blunt option: it allows all
undeclared shared-state mutations, rather than checking one random key's shape.
Matchers are expectation-only; `given` seeds and HTTP stub replies remain
plain JSON.


**State.** Failure records, the log view and environment profiles can all
contain tenant data. They go to `<project>/.aic-script-tester/`, which writes its own
`*` `.gitignore`; the files are `0600`. `<project>` is the nearest ancestor with
a `.git`. `AIC_SCRIPT_TESTER_PROJECT` overrides the project and `AIC_SCRIPT_TESTER_STATE_DIR`
overrides the directory.

**Bins.** Both commands that talk to a tenant take
`--provider-module <file>`. That is the same setup module, or any module whose
default export is a `TenantProvider`. It exists because a bin runs in its own
process, where the Vitest registration does not reach.

- `aic-script-tester-pull-profile [--tenant <name>]` writes
  `.aic-script-tester/profiles/<tenant>.json`. It prints only counts.
- `aic-script-tester-show-log [--stdout]` fetches the logs of recorded failures. It
  writes them to `.aic-script-tester/failures/latest-logs.json`, and opens that in
  `$LOGS_EDITOR` or `$EDITOR` if one is set. `--stdout` prints them instead;
  don't use it in CI.

**Entry points:**

| Import | What it provides |
| --- | --- |
| `.` | the harness |
| `./case` | case definition and judging |
| `./aic` | providers and `setTenantProvider` |
| `./profile` | `pullProfile` and the profile store |
| `./diagnostics` | failure records, `runShowLog` and `loadProviderModule` |
| `./bindings` | the bindings runtime |
| `./runner` | `RhinoRunner` |

More detail lives in this repository's `docs/rhino-local-harness.md`, which is
not shipped with the package.

## JVM runner client

`src/runner.ts` launches Java 25 from `AIC_SCRIPT_TESTER_JAVA_HOME` or `JAVA_HOME`
(`shell.nix` provides `temurin-bin-25`) and speaks line-delimited JSON.
`AIC_SCRIPT_TESTER_JVM` selects `host` (default), `container`, or `both`; the latter
lanes require Docker and the AM image. The cache defaults to
`~/.cache/aic-script-tester`; `AIC_SCRIPT_TESTER_CACHE` overrides it. Correlate by job `id`
— do not assume the JVM answers in order. A crashed JVM rejects every pending
job rather than hanging. `close()` ends stdin and waits for the process;
`afterAll` should call it.

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
npm run measure   # JVM startup + per-job timings
npm run show-log  # fetch AIC logs for a failed test (needs `aic login`)
```

When a test that hit the AIC lane fails, `useLease` appends a JSONL record to
`.aic-script-tester/failures/failures.jsonl`. `npm run show-log` lists those records
newest first and fetches their logs from the tenant provider: the `aic` CLI's
`aic logs tx`, or the log API when log keys are configured. It fetches the
stem first, so one call covers a whole authenticate chain, and it never issues
a range query. It writes the logs to the 0600 view described above.

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
opt-out for a suite that must retain skip behaviour. Step and final `check()`
hooks replay against a tenant-backed `IdmHandle` at their matching response
boundaries. Suite `cleanup()` runs afterwards in a `finally`, before fixture
deletion, even if a check throws. The remote handle returns full materialized
tenant records rather than projecting them to the local mock's shape, so assert
the fields a check needs instead of whole-record equality. An AIC-only throw is
reported as a lane disagreement naming the remote hook.

Object-form queries reject values that cannot be represented faithfully as the
measured CREST `eq` filter (quotes, nulls, arrays/objects, control characters,
and non-finite numbers) before making a request. Unknown fields are not
preflighted because AIC reports them as a successful zero-row query.

Every AM write is followed by a confirming read. A process lock excludes the
same lease on one host, and an identifier-only journal makes stale owned
resources discoverable after process death. Simultaneous use of the same
`aic.id` on different hosts is unsupported because no atomic AM create
precondition has been established. Generated subject and session-minter source
can contain test seeds while a lease is open; never put credentials or real
secrets in them.

For capacity planning only, the design estimate uses `O` distinct subject
outcomes after adding `true` and `false`, and `R = 2O + 3` graph resources. For
`N` one-pass cases without managed fixtures or sessions, the base call count is
`2 + 4R + 3N`: two CLI session calls, three calls per resource at
open, one delete per resource at close, and three calls per case. At the
smallest `R = 9`, that is an estimated `38 + 3N`, or 68 calls for ten cases.
Each `IdmHandle` read, encodable query, or delete in checks/cleanup adds one REST
call. Each additional outcome adds an estimated eight file-lifetime calls.
Step chains, managed fixtures, session minting, and credential refresh add
calls too.

`runAicChain()` remains the one-shot compatibility facade for external callers.
It provisions and deletes a throwaway graph per call. `useLease()` uses the
pre-opened file runner and does not route through that facade. The one-shot
facade refuses `given.libraries` before contacting the tenant; use `useLease()`
for library-backed suites.

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
