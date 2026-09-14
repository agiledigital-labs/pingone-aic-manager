# Local Rhino vs AIC — measured language conformance

Can a local Rhino be configured to reproduce AIC's observed JavaScript
behaviour, and where exactly does it diverge?

**Yes, under `VERSION_DEFAULT` (0) plus AM's decision-node scope.** Rhino
1.7.14.1 with `setLanguageVersion(0)` and a `ScriptContextScope` over JSR-223
Bindings (standard objects as the prototype) matches **27 of 27** verified AM
language rows from
[`docs/api/12-script-bindings-matrix.md`](api/12-script-bindings-matrix.md).
That is the language version you get when AM's
`org.forgerock.am.scripting.disableES6` is true, so the bytecode's `if` that
would call `setLanguageVersion(200)` is skipped. `VERSION_ES6` (200) is a worse
match (17/27): it accepts `let`, `for-of`, object shorthand, destructuring, and
the ES2015 collection globals, all of which AIC rejects.

The previous language-only slice stopped at 25/27. The two misses were both
**top-level `const` in a decision-node script**. Reproducing AM's scope object
closed both; they are no longer a fidelity boundary.

This file does **not** claim a live tenant probe. The AIC column is copied from
the matrix (runtime-verified 2026-06-03 through 2026-07-30 via
`scripts/rhino-script-tester/`). Local numbers come from
`scripts/rhino-local/run-corpus.sh` against Rhino 1.7.14.1 extracted from
`us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053`
(`@sha256:358d7e1e13b27619b742a759fd5a85d4fcc57cc75811027b9f3c0019a0bd9be3`).
That image is AM 8.1.1. AIC self-reports `9.0.0-SNAPSHOT`. Where the image
bytecode and AIC's observed JS disagree, AIC wins.

## How to run

The machine this was built on has no host JDK. Compile and run inside the AM
image (Temurin 25.0.4, the same runtime as the jars):

```bash
scripts/rhino-local/fetch-jars.sh    # no-op once .rhino-local/ is populated
scripts/rhino-local/run-corpus.sh    # language corpus via Probe
scripts/rhino-local/run-runner.sh    # long-lived JSON runner (stdin/stdout)
npm --prefix scripts/rhino-local/ts ci
npm --prefix scripts/rhino-local/ts test
npm --prefix scripts/rhino-local/ts run measure
```

`fetch-jars.sh` will not pull the image. If it is absent it exits 1 and names
it. Jars land in gitignored `.rhino-local/`. The Node client spawns
`run-runner.sh`; that script compiles the Java sources inside the image when
they are stale.

## Local Context configuration

The probe (`scripts/rhino-local/Probe.java`) reproduces AM 8.1.1's
`ObservedJavaScriptContext` constructor, measured with `javap -p -c` on
`org.forgerock.openam.scripting.timeouts.ObservedContextFactory$ObservedJavaScriptContext`:

```text
setOptimizationLevel(-1);
setInstructionObserverThreshold(1000);
if (!disableES6) setLanguageVersion(200);   # 200 == Context.VERSION_ES6
```

plus the other measured factory settings that do not depend on language version:

- `setMaximumInterpreterStackDepth(10000)` (property
  `org.forgerock.openam.scripting.maxinterpreterstackdepth`, default 10000)
- `hasFeature(cx, 21) == true`

**Feature 21 is `Context.FEATURE_ENABLE_JAVA_MAP_ACCESS`.** Confirmed with
`javap -p -constants` on `org.mozilla.javascript.Context` from
`rhino-1.7.14.1.jar`. The default `ContextFactory.hasFeature` returns **false**
for 21; AM's `ObservedContextFactory` overrides it to true. It is about Java
`Map` property access from JS, not ES6 syntax. The language corpus does not
exercise it; it is on so the local Context matches AM's factory.

Language version is a CLI argument so the same corpus can run under several. The
run that produced the tables below used `0`, `180` (`VERSION_1_8`), and `200`.
`VERSION_1_7` (170) was spot-checked: `let` already works there, so it cannot be
AIC.

### Image bytecode vs AIC observed JS

The image default is `disableES6=false`, which **would** set `VERSION_ES6`.
AIC's measured JS is `VERSION_DEFAULT`. A later harness that wants AIC behaviour
must pin language version **0**, not copy the image's default. We did not read
AIC's system properties; `disableES6=true` is the economical explanation, not a
measured tenant setting.

Small bytecode notes the task prompt did not have:

- `RhinoScriptEngineFactory` is
  `org.forgerock.openam.scripting.factories.RhinoScriptEngineFactory`.
- `setWrapFactory(new AMWrapFactory(...))` runs only when
  `EvaluatorVersion.hasFeature(FILTERED_BINDINGS)` is true, in
  `getContext(EvaluatorVersion)`, not in the `ObservedJavaScriptContext`
  constructor.
- The wrap factory and class shutter are not on the language-corpus path.

### Scope object — measured, not guessed

`RhinoScriptEngine.eval(Reader, ScriptContext)` (javap `-p -c` on
`org.forgerock.openam.scripting.factories.RhinoScriptEngine`):

1. `getScope(cx, scriptContext)` → `makeScriptable(cx, scriptContext)`
2. `Context.evaluateReader(scope, reader, filename, 1, null)`

`makeScriptable` does **not** exec against `initStandardObjects()` as a
top-level scope. It builds:

```text
ScriptContextScope scope = new ScriptContextScope(scriptContext);
ScriptableObject std = cx.initStandardObjects();   # every eval, unsealed
scope.setPrototype(std);                           # prototype, not parent
scope.put("context", scope, scriptContext);        # the JSR-223 context
return scope;                                      # parent stays null
```

`ScriptContextScope` implements `Scriptable` only — not `ScriptableObject`, not
`ConstProperties`. Property `put`/`get` go through `ScriptContext` attributes
(`ENGINE_SCOPE` = 100 if the name is new). That is why top-level `const` reads
back `undefined`: Rhino's `putConstProperty` silently drops the initializer when
the scope is not `ConstProperties`, after `defineConstProperty` has already
`put` `Undefined` via `Scriptable.put`.

A hypothesis that the standard-objects object was a **shared parent** was wrong
on both words. It is the **prototype**, and AM **does not share it** —
`initStandardObjects()` runs on every eval. Isolation of `Object.prototype`
mutations is therefore free. The long-lived runner does the same, so job N
cannot observe job N-1's globals _or_ prototype mutations. Sharing a sealed
standard-objects prototype is an optimisation this slice did not need: per-job
`initStandardObjects` is inside the ~0.5 ms job cost below.

`getScope` also installs CommonJS `require` when `LIBRARY_SCRIPT` is on and
`libraryBindings` is present. Decision-node eval does not take that path; the
runner does not model it.

AM's `ScriptContextScope` class depends on `org.forgerock.util.Reject`. The
harness replicates the bytecode (`scripts/rhino-local/ScriptContextScope.java`)
rather than loading the AM class, so forgerock-util is not on the classpath.

## Chosen configuration for a later harness

| Knob                           | Value                                        |
| ------------------------------ | -------------------------------------------- |
| Rhino jar                      | `rhino-1.7.14.1.jar` from the AM 8.1.1 image |
| Language version               | **0 (`VERSION_DEFAULT`)**                    |
| Optimization                   | `-1` (interpreted)                           |
| Instruction observer threshold | `1000`                                       |
| Max interpreter stack depth    | `10000`                                      |
| Feature 21                     | on (`FEATURE_ENABLE_JAVA_MAP_ACCESS`)        |
| Eval scope                     | `ScriptContextScope` (prototype, not parent) |
| Wrap factory / class shutter   | not installed                                |

`VERSION_DEFAULT` plus that scope matches AIC 27/27. Use both. Do not use
`VERSION_ES6` because it is the image default. Do not exec against
`initStandardObjects()` as the top-level scope: that is what made top-level
`const` initialize locally while AIC reads `undefined`.

## Match table

`MATCH` means the local outcome satisfies the machine-readable AIC expectation
in the corpus header (compiled/evaluated/exception substring and/or `__result`
value). `!=` means it does not. AIC wording is from the matrix; local wording is
what Rhino 1.7.14.1 actually printed.

`json` is the control row. It passed under every config, so an all-`undefined`
globals column is a real finding.

### Syntax

| Row                                      | AIC (matrix)                                  | DEFAULT (0) | 1.8 (180)                    | ES6 (200)             |
| ---------------------------------------- | --------------------------------------------- | ----------- | ---------------------------- | --------------------- |
| `var`                                    | works                                         | MATCH       | MATCH                        | MATCH                 |
| `let` (any scope)                        | parse: `missing ; before statement`           | MATCH       | `!=` (works)                 | `!=` (works)          |
| `const` in a function                    | works                                         | MATCH       | MATCH                        | MATCH                 |
| `const` at top level (decision-node)     | parses, value `undefined`                     | MATCH       | MATCH                        | MATCH                 |
| `const` in a top-level loop body         | parses, value `",,"`                          | MATCH       | MATCH                        | MATCH                 |
| `const` in a loop body inside a function | initializer runs once (`0,0,0`)               | MATCH       | MATCH                        | MATCH                 |
| same `const` name twice in one function  | parse error                                   | MATCH       | MATCH                        | MATCH                 |
| `const` in `for` init                    | parse error                                   | MATCH       | MATCH                        | MATCH                 |
| `const` in `for-in`                      | parse error                                   | MATCH       | MATCH                        | MATCH                 |
| `const` in `for-of`                      | parse error                                   | MATCH       | MATCH                        | MATCH                 |
| `for (var x of arr)`                     | parse: `missing ; after for-loop initializer` | MATCH       | MATCH                        | `!=` (works, sum `6`) |
| object shorthand `{a, b}`                | parse: `missing : after property id`          | MATCH       | `!=` (different parse error) | `!=` (works)          |
| object destructuring `var {x} = o`       | parse error                                   | MATCH       | `!=` (works)                 | `!=` (works)          |
| default parameters `f(a, b = 2)`         | parse error                                   | MATCH       | MATCH                        | MATCH                 |
| arrow functions                          | works                                         | MATCH       | MATCH                        | MATCH                 |
| template literals                        | works                                         | MATCH       | MATCH                        | MATCH                 |
| ES2015 Array/String/Object methods       | all work                                      | MATCH       | MATCH                        | MATCH                 |
| `String.prototype.normalize`             | works                                         | MATCH       | MATCH                        | MATCH                 |

Default parameters fail to parse even at `VERSION_ES6`
(`missing ) after formal parameters`). That row is version-independent in this
Rhino, not evidence about `disableES6`.

Object shorthand at `VERSION_1_8` still fails, but with a different message
(`SyntaxError: invalid object initializer`) than AIC recorded, so it is `!=`
even though both sides reject the source.

### ES2015 globals

| Row              | AIC (matrix, 2026-07-30)                               | DEFAULT (0) | 1.8 (180) | ES6 (200)                   |
| ---------------- | ------------------------------------------------------ | ----------- | --------- | --------------------------- |
| `JSON` (control) | `typeof` `"object"`                                    | MATCH       | MATCH     | MATCH                       |
| `Map`            | `typeof` `"undefined"`; `new Map()` → `ReferenceError` | MATCH       | MATCH     | `!=` (function, constructs) |
| `Set`            | same shape                                             | MATCH       | MATCH     | `!=`                        |
| `WeakMap`        | same shape                                             | MATCH       | MATCH     | `!=`                        |
| `WeakSet`        | same shape                                             | MATCH       | MATCH     | `!=`                        |
| `Symbol`         | `typeof` `"undefined"`                                 | MATCH       | MATCH     | `!=` (`"function"`)         |
| `Promise`        | `typeof` `"undefined"`; `new` → `ReferenceError`       | MATCH       | MATCH     | `!=`                        |
| `Proxy`          | `typeof` `"undefined"`                                 | MATCH       | MATCH     | MATCH                       |
| `Reflect`        | `typeof` `"undefined"`                                 | MATCH       | MATCH     | MATCH                       |

`Proxy` and `Reflect` stay `undefined` at `VERSION_ES6`. They are not evidence
for `disableES6` either.

### Scores

| Config      | Matches AIC |
| ----------- | ----------- |
| DEFAULT (0) | **27/27**   |
| 1.8 (180)   | 24/27       |
| ES6 (200)   | 17/27       |

## Fidelity boundary — closed

The language-only slice left two rows no language version reproduced. Both were
silent-data bugs on AIC. Reproducing `ScriptContextScope` closed them together,
on every language version tried (0, 180, 200):

### 1. `const` at top level of a decision-node script

- **AIC** (matrix, probed 2026-06-03): parses, value reads back `undefined`.
- **Local**, now: parses, value is `undefined`. MATCH.
- **Cite:** language / syntax feature matrix,
  `` `const` at top level (decision-node script) ``.

### 2. `const` in a top-level loop body

- **AIC** (matrix, probed 2026-06-03 / nested-block and while/do-while
  follow-ups 2026-07-03): parses, join of the values is `",,"` (three
  `undefined`s).
- **Local**, now: parses, value is `",,"`. MATCH.
- **Cite:** language / syntax feature matrix,
  `` `const` in a for/for-in/for-of/while/do-while loop body ``.

The in-function variant still matches (`0,0,0` locally and on AIC): function
bodies are `ScriptableObject` / `ConstProperties`, so `const` initializes.
LIBRARY top-level `const` (matrix: works) is still out of scope; a library's top
level is function-like and is not this scope object.

Lint rules that ban top-level `const` and `const` in a loop body stay. The
harness now reproduces the AIC bug instead of hiding it.

## Long-lived runner

`Probe.java` is still the one-shot corpus driver. The long-lived process is
`Runner.java`, spawned by `scripts/rhino-local/run-runner.sh` and driven from
Node by `scripts/rhino-local/ts/src/runner.ts`.

Line-delimited JSON on stdin; one JSON response per job on stdout. Runner
diagnostics go to stderr (`rhino-local-runner ready` on start). Jobs are handled
sequentially; the client correlates by `id` and does not assume order. A crashed
JVM rejects every pending eval (`RhinoRunnerExitError`) rather than hanging.

### Outcomes

`outcome` is one of:

| Value            | Meaning                                                    |
| ---------------- | ---------------------------------------------------------- |
| `ok`             | compiled and ran                                           |
| `compile_error`  | failed to compile (`EvaluatorException` / parse)           |
| `runtime_error`  | threw at runtime (`RhinoException` during `exec`)          |
| `timeout`        | instruction observer threw `Error("Interrupt.")`           |
| `protocol_error` | the job JSON was malformed (has an `id`, missing `source`) |

`error.sourceName` / `error.line` / `error.column` are Rhino's `RhinoException`
fields. Pass the author's file path as `sourceName` so a parse error names that
file and line, not `<eval>`. Optional `preamble` is eval'd first under
`preambleName` (default `<preamble>`) so loading mock source later does not
shift author line numbers.

`timeoutMs` is per-job. `0` is AM's `NO_TIMEOUT`. Omitted uses a 10s harness
default so a runaway cannot hang the process. AM's observer threshold of 1000
and the `Interrupt.` message are reproduced.

`globals` is a JSON object placed in `ENGINE_SCOPE` Bindings before eval. This
slice does not know what `nodeState` is; a later slice injects the generated
mock surface here (or as `preamble`).

### Isolation

Each job gets a fresh `SimpleScriptContext` + `ScriptContextScope` +
`initStandardObjects()` prototype. Proven in `test/runner.test.ts`: a global
(`leaked = …`, `var`, `function`) defined in job N is `typeof undefined` in job
N+1, and `Object.prototype` mutations do not leak either. Per-job isolation is
real for script-defined state.

### Measured cost (2026-09-11)

`npm --prefix scripts/rhino-local/ts run measure` — three spawn/ready cycles,
then 200 sequential `1+1` jobs on the first process. Host times include the Node
client and docker stdio; the JVM is Temurin 25.0.4 inside the AM image.

- Spawn until ready (docker + JVM), n=3: min 431 ms, median 431 ms, mean 431 ms,
  p95 432 ms, max 432 ms
- First job after ready: 61.3 ms
- Trivial job (`1+1`), n=200: min 0.27 ms, median 0.53 ms, mean 0.58 ms, p95
  0.99 ms, max 1.93 ms

This is actually fast. A JVM-per-case loop would pay ~430 ms + ~60 ms per test.
Two hundred trivial cases on one process are ~430 + 61 + 200×0.5 ≈ 0.6 s instead
of ~100 s. Fault 1 (slow) is answered by the figure, not an adjective.

## What this means for the downstream plan

Easier than the contradiction suggested:

- Rhino 1.7.14.1 at `VERSION_DEFAULT` already has arrow functions, template
  literals, and the ES2015 prototype methods, while still rejecting `let`,
  `for-of`, object shorthand, destructuring, and the collection globals. That
  mix is this Rhino's actual gating, not a second hidden switch. A local harness
  on language version 0 is the right default.
- The `json` control works. Corpus rows can fail honestly.
- The two top-level `const` rows now match AIC. A declarative given/expect case
  format does not need a special-case for them; they fail the same way on the
  tenant and locally, which is what the lint rules exist to catch.
- Bindings inject into ENGINE_SCOPE, which is where AM puts them. The mock
  surface generated into `generated/scripted-decision-mocks.cjs` can ride
  `preamble` (keeps author line numbers) or `globals` (JSON-shaped values).
- `AMWrapFactory` **loads** from `openam-scripting-8.1.1.jar` plus the Rhino jar
  (`Class.forName` succeeds). A later slice can reference the class without
  pulling the rest of AM onto the compile classpath.

Harder, or at least bounded:

- **Do not trust the image default language version.** AIC's JS is
  `VERSION_DEFAULT`; the image would enable ES6 unless
  `org.forgerock.am.scripting.disableES6` is set. Pin 0.
- `RhinoSandboxClassShutter` does **not** load standalone:
  `NoClassDefFoundError: com.google.common.cache.CacheLoader`. Constructing
  `AMWrapFactory` needs
  `org.forgerock.openam.annotations.service.SupportedElementService` from
  another jar; the shutter also wants `WildcardSet`, `SystemPropertiesManager`,
  and friends. This slice did not install either. Java `Map` property access
  works because feature 21 is on; the wrap factory's filtered bindings and the
  shutter allowlist are still unmodelled. A later slice that wants AM's real
  wrap factory or class shutter must extract more jars (and Guava) or
  approximate.
- Default parameters, `for (const … in/of …)`, `for (const i = 0; …)`, and
  duplicate `const` in one function fail at every language version including
  ES6. Those lint rules stay, even if someone later turns ES6 on.
- The runner puts AM's `context` (the JSR-223 `ScriptContext` Java object) on
  every scope, because `makeScriptable` does. No captured binding is named
  `context`; a later mock must not collide with it, or must overwrite it
  deliberately.

## AIC lane — live tenant verification (2026-09-12)

Verified against the sandbox tenant on 2026-09-12 by running
`runAicLane` from this checkout. These figures are from those runs, not from
inference or from a neighbouring doc.

The lane provisions a namespaced wrapper journey (`rl-aic-<runId>`: four
scripts, four nodes, one tree), invokes `/json/realms/root/realms/alpha/authenticate`,
records the effects, and deletes everything in a `finally`. It refuses to start
if a resource of the same name already exists.

| | Local lane | AIC lane |
| --- | --- | --- |
| `decide-from-state` | 6.4 ms | 10.9 s |

Roughly **1700x**. Both lanes are judged by the same `judge()`; that is what
makes the comparison meaningful.

### Two apparent disagreements exposed recording-model defects

The original live run observed three additional keys with stable values at the
**result node**. It did not establish that those keys existed when the subject
ran, so describing them as AM state readable by the subject overstated the
measurement. The wrapper now snapshots unified `nodeState` immediately before
and after the unmodified subject body. A key present in `before` but absent from
`given` is reported as ambient context; it is not attributed to the script.
A changed or removed ambient key remains a script mutation.

**2. Bucket membership is not observable from the AIC side.** A key written
with `nodeState.putTransient` is reported in `sharedState.final` with
`transientState.final` empty — but read that carefully, because the obvious
reading is wrong. The wrapper's result node reads `nodeState.get`, which is
**unified** (transient -> secure -> shared). So what was measured is that the
key is *reachable*; the shared attribution was supplied by
`src/aic/record.ts::classifyFinal`, not observed on the tenant. Next-gen AM
exposes no bucket-inspection API, so bucket membership is genuinely
unrecoverable from a unified read, and any lane that reports one is inventing
it.

The same caution applies to the ambient keys above: they were observed at the
**result** node, which does not establish that the **subject** could read them.
Establishing that needs a snapshot taken inside the subject node itself, before
and after the author's source.

New and changed unified values are now recorded as **unbucketed**, carrying
their possible buckets, rather than being assigned to shared state.

The verdict and comparison report distinguish all three cases:

- comparable observations that differ are behavioural disagreements;
- matching unified mutations with unknown bucket membership are observation
  gaps, and make a passing verdict non-conclusive;
- pre-subject state absent from `given` is ambient state in its own report.

The AIC recorder also marks `openidm`, `http`, and `logs` unobserved. Their
empty arrays are placeholders required by the common effect shape, not claims
that no such effects occurred. Its evidence also carries a standing `unified`
state-bucket qualification: same-value and lower-precedence writes may produce
no unified delta at all. The subject-snapshot wrapper and these revised
semantics have unit coverage but have **not** yet been verified on a live
tenant; the earlier live run exercised the result-node snapshot design.

A third defect found while reviewing this: the AIC recorder returns
`openidm: []`, `http: []` and `logs: []` unconditionally. Those are not
observations — the wrapper journey never measures them — so a case can pass its
`openidm` expectations having never looked. An empty array must mean "observed
none", never "did not observe".

### Conformance model — verified live 2026-09-12

The evidence model (`RecordingEvidence`, `Verdict.conclusive`) was verified
against the sandbox tenant after it was implemented. Three runs:

| Live case | Result |
| --- | --- |
| `decide-from-state`, correct expectations | `pass: true` |
| `write-state`, correct values | `pass: true`, `conclusive: false` |
| `write-state` with a deliberately wrong unbucketed value | `pass: false` |
| `decide-from-state` with a wrong outcome | `pass: false` |

The third row is the one that matters: bucket uncertainty does not excuse a
wrong value. `diffRecordedEffects` compares values **before** it reasons about
buckets, so an unobservable location can never silence a bad value. A mutation
test confirms it — introducing the permissive version fails two of the three
anti-silencing fixtures in `test/aic/conform.test.ts`.

`write-state` reports `conclusive: false` with a per-key reason:

```text
sharedState: "checked" added value matched, but its bucket is unobservable
             (could be sharedState, transientState)
nodeState:   the runner observes one unified view; per-bucket absence and
             hidden lower-precedence writes were not verified
openidm/http/logs: the runner cannot observe this channel
```

The ambient keys are now derived, not named: anything present in the subject's
`before` snapshot and absent from `given` is ambient. On this tenant that is
`realm` (`/alpha`), `maxAuthenticationSessionDuration` (20) and `authLevel`
(0) — the same three, now attributed rather than guessed.

### `action.goTo()` does not halt script execution — verified 2026-09-12

The subject wrapper appends a state snapshot **after** the author's source. The
probe scripts end in `action.goTo(...)`, so this only works if execution
continues past that call. It does: the snapshot ran and its payload was
readable by the result node on a live tenant.

This also establishes something the earlier result-node measurement could not.
Ambient keys are visible in the subject's `before` snapshot, so the subject
script really can read them — previously we only knew they existed by the time
the result node ran.

### The portability guard and managed-fixture provenance

The managed-fixture lifecycle below is offline-tested and was exercised live
by the maintainer on 2026-09-14, including successful create/read/delete and a
collision control that left the pre-existing record byte-identical.

An author-written `given.managed` still makes the lane refuse the case:
`given.managed is environment-dependent; AIC lane skips rather than run against
whatever the tenant holds`. A `Case` cannot distinguish that declaration from
records the harness owns, so the case alone is deliberately insufficient.

`Lease.execute()` now records its fixture ledger separately on `RunResult`, and
`chainFromRunResult()` carries that provenance into `conformChain()`. It is
accepted only when it exactly matches the first pass's `given.managed`; later
passes may differ because the subject itself can create, mutate or delete IDM
records. A manually assembled chain has no ledger provenance and stays skipped.

Before provisioning the subject, the AIC lane creates every owned fixture with
`POST /openidm/managed/{type}?_action=create`, including `_id` in the body. A
412 is a collision, never an update, and aborts before the subject. Every 201 is
followed by a GET that proves the record is readable and checks every declared
field that GET can expose. The only measured exception is
`managed/alpha_user.password`: a live create accepted it, but GET omitted the
write-only field. The harness therefore cannot prove the submitted password
landed; it still checks `_id` and every other declared field exactly. Do not add
another read-back exception without live evidence.

`managed/alpha_user` fixture ids are rejected locally unless they are
36-character UUIDs, before the create request is sent. Live measurement showed
that user `_id` is the `fr-idm-uuid` RDN; `managed/alpha_role` accepted a
readable id, so the early constraint is deliberately type-specific. Every
successfully created record is deleted in reverse order even when the seed
check or the subject fails.

Managed-fixture runs are serialized per tenant with a process-independent lock
under the OS temporary directory. This lets parallel test files wait instead of
racing on a shared `_id`; the cost is that all managed-fixture conformance runs
against one tenant execute one at a time. A collision while holding the lock is
therefore reported as pre-existing tenant state or a fixture leaked by an
earlier run.

### Real-script corpus, same day

52 probe scripts from `scripts/rhino-script-tester/` are now cases. After
`callbacks.isEmpty` landed: **14 pass end to end**, 24 run and disagree on
callback content, 4 legacy need `action.send`, 1 fails its outcome.

The 24 are the corpus working as a measuring instrument rather than failing.
`bindings-availability` dumps `typeof` for every binding; live AIC recorded
`require: "function"` where the local harness produces `"undefined"`. That is a
precisely located fidelity gap, which is what the corpus is for.
`docs/rhino-local-gaps.md` holds the ranked list.

## Step chains — both lanes (2026-09-14)

A journey that asks the user a question runs the same node twice. One `Case`
per pass; `.step({ expect, reply, check })` on the lease declares a suspended
pass, and `runAicChain(cases, source, { replies })` runs the same chain against
a tenant — one journey, provisioned once, entered once per pass.

**What carries between passes was measured, not assumed.** Shared state
survives; transient state is dropped, not promoted to secure state; and
`resumedFromSuspend` stays `false` (it belongs to `action.suspend()`). The
probe and its controls are in `docs/api/09-journeys.md` → "What survives a
callback round trip"; the consequence for the harness is
`src/harness/step.ts::carryGiven`.

That measurement is the point of the two lanes. Carrying transient state is the
plausible guess, and a harness making it would agree with itself forever: the
mock carries the value, the script reads it, both passes go green, and the same
script loses the value on a tenant. Mutating `carryGiven` to carry it fails four
tests, three of them through the real JVM.

**The AIC lane checks the carry rather than repeating it.** Each pass is handed
the `Given` the local lane computed for it, and `verifySeedsVisible` compares
that against the tenant's own `before` snapshot. Verified live 2026-09-14: a
two-pass chain agreed on both lanes with zero disagreements, and the control —
the same chain with pass 2's `Given` altered to claim transient state had
survived — failed with `AIC subject state did not contain the declared seed
"scratch"`. Without that arm the green run would have been evidence about the
harness talking to itself.

Two consequences elsewhere:

- The subject's state seed is wrapped in `if (callbacks.isEmpty())`. A node
  re-executes from the top, so an unguarded seed would overwrite whatever the
  earlier pass left with the request's original values — the chain would
  silently restart on every step and still look like it worked.
- `expect.outcome` is now `string | null`. A pass that suspends reaches no
  outcome, and the only way to say that before was to name one the script never
  produces.

`conformChain()` takes the cases, local effects, and submitted replies already
recorded by `Lease.execute()`; `chainFromRunResult()` is the bridge. The AIC
lane therefore cannot re-run the local chain or recompute its carry. Every pass
is judged by the shared `judge()`, while only the final pass is lane-diffed.

Intermediate passes are not observed on the tenant. The subject could emit its
state dump alongside its own callbacks on every suspend, which would make them
observable — but that extra `HiddenValueCallback` would be visible to a script
calling `callbacks.getHiddenValueCallbacks()`, which is a divergence between
the lanes in the one binding a step chain exists to exercise. Each intermediate
pass is consequently marked `aicObserved: false` and reports an
`ObservationGap`; the final pass is marked observed and is fully diffed.

## Unsettled

- Whether AIC actually sets `org.forgerock.am.scripting.disableES6=true`, or
  some other 9.0.0-SNAPSHOT path leaves the language version at 0. The observed
  JS is 0 either way. Reading the property needs a tenant (or an AIC image),
  which this slice did not touch.
- LIBRARY scripts (function-like top level, CommonJS `require` via
  `libraryBindings`). Measured 2026-09-12 to be the single largest fidelity gap
  in the real-script corpus: live AIC reports `typeof require === "function"`
  and the harness reports `"undefined"`, which blocks 8 of the 52 probe scripts.
  No longer out of scope.
- Whether a script that intentionally depends on wrapper ambient state should
  declare the required value in `given`, or be classified as environment
  dependent. The local lane does not seed hidden AM defaults.
- Live verification that the subject instrumentation executes after
  `action.goTo`, and that its before/after payload survives the transition to
  the result node. Unit tests cannot establish those AM runtime properties.
- `AMWrapFactory` behaviour and the class shutter allowlist. Approximating the
  shutter is still acceptable; using AM's class is not free.
- How the generated mock `.cjs` should be loaded: `preamble` vs concatenating
  into `source` vs a third field. `preamble` is there so the next slice does not
  have to pick concatenation (which would break source-line mapping).
- IDM. The matrix's IDM column is a different engine; nothing here speaks to it.
