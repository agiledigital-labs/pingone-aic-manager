# Local Rhino vs AIC — measured language conformance

Can a local Rhino be configured to reproduce AIC's observed JavaScript
behaviour, and where exactly does it diverge?

**Yes, closely, under `VERSION_DEFAULT` (0).** Rhino 1.7.14.1 with
`setLanguageVersion(0)` matches 25 of 27 verified AM language rows from
[`docs/api/12-script-bindings-matrix.md`](api/12-script-bindings-matrix.md).
That is the language version you get when AM's
`org.forgerock.am.scripting.disableES6` is true, so the bytecode's `if` that
would call `setLanguageVersion(200)` is skipped. `VERSION_ES6` (200) is a worse
match (15/27): it accepts `let`, `for-of`, object shorthand, destructuring, and
the ES2015 collection globals, all of which AIC rejects.

The two rows no local language version reproduces are both about **top-level
`const` in a decision-node script**. That is the harness's fidelity boundary.

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
scripts/rhino-local/run-corpus.sh
```

`fetch-jars.sh` will not pull the image. If it is absent it exits 1 and names
it. Jars land in gitignored `.rhino-local/`.

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

## Chosen configuration for a later harness

| Knob                           | Value                                        |
| ------------------------------ | -------------------------------------------- |
| Rhino jar                      | `rhino-1.7.14.1.jar` from the AM 8.1.1 image |
| Language version               | **0 (`VERSION_DEFAULT`)**                    |
| Optimization                   | `-1` (interpreted)                           |
| Instruction observer threshold | `1000`                                       |
| Max interpreter stack depth    | `10000`                                      |
| Feature 21                     | on (`FEATURE_ENABLE_JAVA_MAP_ACCESS`)        |

`VERSION_DEFAULT` is the closest match (25/27). Use it. Do not use `VERSION_ES6`
because it is the image default.

## Match table

`MATCH` means the local outcome satisfies the machine-readable AIC expectation
in the corpus header (compiled/evaluated/exception substring and/or `__result`
value). `!=` means it does not. AIC wording is from the matrix; local wording is
what Rhino 1.7.14.1 actually printed.

`json` is the control row. It passed under every config, so an all-`undefined`
globals column is a real finding.

### Syntax

| Row                                      | AIC (matrix)                                  | DEFAULT (0)        | 1.8 (180)                    | ES6 (200)             |
| ---------------------------------------- | --------------------------------------------- | ------------------ | ---------------------------- | --------------------- |
| `var`                                    | works                                         | MATCH              | MATCH                        | MATCH                 |
| `let` (any scope)                        | parse: `missing ; before statement`           | MATCH              | `!=` (works)                 | `!=` (works)          |
| `const` in a function                    | works                                         | MATCH              | MATCH                        | MATCH                 |
| `const` at top level (decision-node)     | parses, value `undefined`                     | `!=` (value works) | `!=`                         | `!=`                  |
| `const` in a top-level loop body         | parses, value `",,"`                          | `!=` (`0,0,0`)     | `!=`                         | `!=`                  |
| `const` in a loop body inside a function | initializer runs once (`0,0,0`)               | MATCH              | MATCH                        | MATCH                 |
| same `const` name twice in one function  | parse error                                   | MATCH              | MATCH                        | MATCH                 |
| `const` in `for` init                    | parse error                                   | MATCH              | MATCH                        | MATCH                 |
| `const` in `for-in`                      | parse error                                   | MATCH              | MATCH                        | MATCH                 |
| `const` in `for-of`                      | parse error                                   | MATCH              | MATCH                        | MATCH                 |
| `for (var x of arr)`                     | parse: `missing ; after for-loop initializer` | MATCH              | MATCH                        | `!=` (works, sum `6`) |
| object shorthand `{a, b}`                | parse: `missing : after property id`          | MATCH              | `!=` (different parse error) | `!=` (works)          |
| object destructuring `var {x} = o`       | parse error                                   | MATCH              | `!=` (works)                 | `!=` (works)          |
| default parameters `f(a, b = 2)`         | parse error                                   | MATCH              | MATCH                        | MATCH                 |
| arrow functions                          | works                                         | MATCH              | MATCH                        | MATCH                 |
| template literals                        | works                                         | MATCH              | MATCH                        | MATCH                 |
| ES2015 Array/String/Object methods       | all work                                      | MATCH              | MATCH                        | MATCH                 |
| `String.prototype.normalize`             | works                                         | MATCH              | MATCH                        | MATCH                 |

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
| DEFAULT (0) | **25/27**   |
| 1.8 (180)   | 22/27       |
| ES6 (200)   | 15/27       |

## Fidelity boundary — rows no local config reproduces

These two, and only these two, of the 27 checked rows. Both are silent-data bugs
on AIC, so a local harness that "parses and runs" them will hide the bug the
lint rules exist to catch.

### 1. `const` at top level of a decision-node script

- **AIC** (matrix, probed 2026-06-03): parses, value reads back `undefined`.
- **Local**, every language version tried (0, 170, 180, 200): parses, value is
  the initializer (`const-top-level-ok`).
- **Cite:** language / syntax feature matrix,
  `` `const` at top level (decision-node script) ``.

This harness evals a source file with `compileString` + `exec` on
`initStandardObjects()`. That is the decision-node analogue, not LIBRARY (the
matrix says LIBRARY top-level `const` works, because a library's top level is
function-like). Wrapping the script as a function body would make this row match
`const`-in-function (works), which is still not AIC's `undefined`. Language
version is not the lever.

### 2. `const` in a top-level loop body

- **AIC** (matrix, probed 2026-06-03 / nested-block and while/do-while
  follow-ups 2026-07-03): parses, join of the values is `",,"` (three
  `undefined`s).
- **Local**, every language version tried: parses, value is `0,0,0` (the
  initializer runs on the first iteration only — the same bug AIC shows **inside
  a function**).
- **Cite:** language / syntax feature matrix,
  `` `const` in a for/for-in/for-of/while/do-while loop body ``.

The in-function variant **does** match (`0,0,0` locally and on AIC). The gap is
specific to script top level.

Those two rows move together: on AIC, top-level `const` does not initialize, so
a top-level loop body reading that `const` sees `undefined` every iteration.
Locally, top-level `const` **does** initialize, so the same loop shows Rhino's
first-iteration-only bug instead. A later slice that finds AM's actual eval
wrapper (or scope object) for decision-node scripts may close both at once. This
slice did not guess at that wrapper.

## What this means for the downstream plan

Easier than the contradiction suggested:

- Rhino 1.7.14.1 at `VERSION_DEFAULT` already has arrow functions, template
  literals, and the ES2015 prototype methods, while still rejecting `let`,
  `for-of`, object shorthand, destructuring, and the collection globals. That
  mix is this Rhino's actual gating, not a second hidden switch. A local harness
  on language version 0 is the right default.
- The `json` control works. Corpus rows can fail honestly.
- `AMWrapFactory` **loads** from `openam-scripting-8.1.1.jar` plus the Rhino jar
  (`Class.forName` succeeds). A later slice can reference the class without
  pulling the rest of AM onto the compile classpath.

Harder, or at least bounded:

- **Do not trust the image default language version.** AIC's JS is
  `VERSION_DEFAULT`; the image would enable ES6 unless
  `org.forgerock.am.scripting.disableES6` is set. Pin 0.
- Top-level `const` (decision-node) is not a language-version problem. A
  declarative case format that only varies language version will keep reporting
  those two rows as mismatches. Mocked bindings will not fix them either.
  Closing them needs an eval-wrapper / scope model.
- `RhinoSandboxClassShutter` does **not** load standalone:
  `NoClassDefFoundError: com.google.common.cache.CacheLoader`. Constructing
  `AMWrapFactory` needs
  `org.forgerock.openam.annotations.service.SupportedElementService` from
  another jar; the shutter also wants `WildcardSet`, `SystemPropertiesManager`,
  and friends. A later slice that wants AM's real wrap factory or class shutter
  must extract more jars (and Guava) or approximate. For language-syntax work,
  neither is required.
- Default parameters, `for (const … in/of …)`, `for (const i = 0; …)`, and
  duplicate `const` in one function fail at every language version including
  ES6. Those lint rules stay, even if someone later turns ES6 on.

## Unsettled

- Whether AIC actually sets `org.forgerock.am.scripting.disableES6=true`, or
  some other 9.0.0-SNAPSHOT path leaves the language version at 0. The observed
  JS is 0 either way. Reading the property needs a tenant (or an AIC image),
  which this slice did not touch.
- What AM wraps a decision-node script in before `exec`. That is the leading
  candidate for the two `const` mismatches; it was not measured.
- LIBRARY scripts (function-like top level). Out of scope; this harness evals a
  file as a script.
- Bindings, `AMWrapFactory` behaviour, and the class shutter allowlist. Language
  only.
- IDM. The matrix's IDM column is a different engine; nothing here speaks to it.
