# Script Bindings & Runtime Feature Matrix

Implemented in: `src/scripts/templates/`

Feature matrix backing the script-linting/type-update work
(`script-linting-uplift-plan.md`). It records, per AM/IDM script family, which
language features and bindings are available, **and how we know** — so the
TypeScript declarations and ESLint rules can be grounded in fact rather than
copied assumptions.

> **Status legend (provenance of each claim):**
>
> - **D** — Documented by Ping (URL cited).
> - **V** — Runtime-verified in the sandbox via `scripts/rhino-script-tester/`.
> - **I** — Inferred from the existing sandbox corpus
>   (`<client-checkout>/sandbox-scripts`) — real scripts use it, so it must work, but not
>   isolated-probe confirmed.
> - **U** — Unknown / not yet verified. **Do not** encode as fact in types or
>   lint.

Compiled 2026-06-03 from the Ping docs listed below + the existing template
`.d.ts` files + a usage scan of the 384 `src/`, 56 `lib/`, 10 `oidc/` sandbox
scripts. Runtime-probe rows are filled in as probes land (see
`scripts/rhino-script-tester/`).

## Documentation sources

- Next-generation scripts —
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/next-generation-scripts.html
- Scripted Decision Node API —
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/scripting-api-node.html
- Migrate decision-node scripts to next-gen —
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/scripting-api-node-migrate.html
- Script bindings —
  https://docs.pingidentity.com/pingoneaic/latest/am-scripting/script-bindings.html
- Scripting environment —
  https://docs.pingidentity.com/pingoneaic/am-scripting/scripting-env.html
- Library scripts —
  https://docs.pingidentity.com/pingoneaic/am-scripting/library-scripts.html

## Key conclusion: engine generation is a _bindings_ axis, not a _syntax_ axis

Both legacy and next-generation AM scripts run on **Mozilla Rhino 1.7.14** with
"limited ES6 / ES2015 support" (scripting-env doc, **D**). The runtime probe
confirms next-gen still **rejects `let`** (`missing ; before statement`, **V**).
So "next-generation" does **not** mean a newer JS engine — it means a different
_binding set_ (simplified `logger`, fetch-like `httpClient`, `openidm`, `utils`,
`require()`/library support, `action`/`callbacksBuilder`/`nodeState` instead of
`Action`/`callbacks`/`sharedState`).

Practical consequence for this work:

- **One** Rhino syntax layer (`rhino-1.7.14.d.ts` + the shared ESLint syntax
  restrictions) applies to **all AM** scripts.
- Legacy vs next-gen splits only the **bindings** overlay, not the syntax rules.
- Naming the type file `rhino-1.7.14.d.ts` per product is correct; there is no
  separate "next-gen engine" type file needed for syntax.

**Qualified since this was written (2026-06-03):** the AM-vs-AM claim holds, but
this conclusion does **not** extend to IDM. Later probes found IDM running a
newer engine that accepts `let`, object shorthand and destructuring
(2026-06-04), and that has the ES2015 collection globals AM lacks entirely
(2026-07-30). AM and IDM share the _file_ name `rhino-1.7.14.d.ts`, not the same
language surface — the two products' syntax layers must be allowed to diverge.

## Language / syntax feature matrix (Rhino 1.7.14)

Applies to every script family. **All rows runtime-verified 2026-06-03** (the
duplicate-`const`-per-function row added 2026-06-06;
`String.prototype.normalize` row added 2026-07-02; nested-loop-block `const` and
while/do-while loop-body `const` probes added 2026-07-03; LIBRARY top-level
`const` and in-function loop-body `const` probes added 2026-07-13) via the
next-gen scripted decision probe (`scripts/rhino-script-tester/fixtures/`,
results in `tmp/rhino-script-tester/probe-results.json`). Probe semantics: a
fixture that PARSES + RUNS returns a `HiddenValueCallback` (`HTTP 200`); a
fixture that fails to PARSE returns no callback and the journey fails
(`HTTP 401`, confirmed via logs, e.g. object shorthand →
`org.mozilla.javascript.EvaluatorException: missing : after property id`).

| Feature                                                                                                                                 | Status | Result                                                                                                                                                                                                                                                                                                                 | Lint action                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `var`                                                                                                                                   | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                                             |
| `const` in a function body                                                                                                              | **V**  | ✅ works, correct value                                                                                                                                                                                                                                                                                                | **allow**                                                         |
| same `const` name twice in one function (separate, non-nested blocks)                                                                   | **V**  | ❌ parse error (Rhino scopes `const` to the function for redeclaration)                                                                                                                                                                                                                                                | **ban (all AM) — custom `rhino/no-dup-const` rule**               |
| `const` at top level (decision-node script)                                                                                             | **V**  | ⚠️ parses but value reads back `undefined` — silent data bug                                                                                                                                                                                                                                                           | **ban** (all AM)                                                  |
| `const` at top level of a `LIBRARY` script                                                                                              | **V**  | ✅ works, correct value (probed 2026-07-13, `lib-const-probe.lib.js` — a library's top level is function-like scope, so the decision-node bug does not apply)                                                                                                                                                          | allow (ban kept in lint for uniformity is fine)                   |
| `const` in a `for`/`for-in`/`for-of`/`while`/`do-while` loop body, including nested blocks such as `if` inside the loop                 | **V**  | ⚠️ parses but value reads back `undefined` — silent data bug (`value: ",,"` for nested-block/while/do-while probes)                                                                                                                                                                                                    | **ban**                                                           |
| `const` in a loop body INSIDE a function (decision-node and `LIBRARY` contexts)                                                         | **V**  | ⚠️ parses but the initializer runs only on the FIRST iteration; later iterations silently keep the first value (`"0,0,0"` where correct is `"0,2,4"`; probed 2026-07-13, both contexts)                                                                                                                                | **ban** (scope does not rescue loop-body `const`)                 |
| `const` in `for` init                                                                                                                   | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                               |
| `const` in `for-in`                                                                                                                     | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                               |
| `const` in `for-of`                                                                                                                     | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                               |
| `for...of` **itself**, even with `var` (`for (var x of arr)`)                                                                           | **V**  | ❌ parse error: `missing ; after for-loop initializer` (probed 2026-07-30, `fixtures/for-of-var.script.js`, over both an array and a string; the index-loop control in the same fixture runs fine). Consistent with `Symbol` being absent — there is no iteration protocol to drive it                                 | **ban (all AM)** — the whole statement, not just its `const` form |
| `let` (any scope)                                                                                                                       | **V**  | ❌ parse error (`missing ; before statement`)                                                                                                                                                                                                                                                                          | ban (all AM)                                                      |
| object shorthand `{a, b}`                                                                                                               | **V**  | ❌ parse error (`missing : after property id`)                                                                                                                                                                                                                                                                         | ban                                                               |
| object destructuring `var {x} = o`                                                                                                      | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                               |
| default parameters `f(a, b = 2)`                                                                                                        | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | **ban (NEW — not in current config)**                             |
| arrow functions `=>`                                                                                                                    | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                                             |
| template literals                                                                                                                       | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                                             |
| ES2015 methods: `Array` `includes`/`find`/`from`/`fill`, `String` `includes`/`startsWith`/`endsWith`/`repeat`, `Object` `assign`/`keys` | **V**  | ✅ all work. `Array.prototype.fill` + `Array.from({length}, () => …)` also verified in **`LIBRARY`** context (probed 2026-07-17, `fixtures/lib-array-fill-probe.lib.js` + `lib-array-fill-consumer.script.js`: `new Array(3).fill(false)` and `Array.from({length:3}, () => false)` both returned `false,false,false`) | allow                                                             |
| `String.prototype.normalize` (`NFD`/`NFC`) + combining-mark regex strip (`/[̀-ͯ]/`)                                                       | **V**  | ✅ works — decision-node AND `LIBRARY` context (probed 2026-07-02; handles stacked marks, `Nguyễn` → `Nguyen`)                                                                                                                                                                                                         | allow                                                             |
| ES2015 global objects: `Map`, `Set`, `WeakMap`, `WeakSet`, `Symbol`, `Proxy`, `Reflect`, `Promise`                                      | **V**  | ❌ **none of them exist on AM.** Every `typeof` is `"undefined"`; `new Map()` → `ReferenceError: "Map" is not defined.` Identical on legacy (1.0), next-gen (2.0), and inside `LIBRARY` scope (probed 2026-07-30). **IDM is the opposite** — see the ES2015-globals section below                                      | **ban (all AM)**                                                  |

> **Key takeaways for ESLint:** the existing AM bans on `let`, object shorthand,
> object destructuring, and `const` in loops are all now runtime-justified.
> `const` _inside functions_ must stay **allowed** (it works and is idiomatic).
> The top-level/loop-body `const` bans should apply to **all** AM scripted
> decision scripts, not just the old `src` glob, because the failure is a silent
> `undefined` (worse than a parse error). Loop-body means
> `for`/`for-in`/`for-of`/ `while`/`do-while` bodies at any block depth,
> stopping at nested function boundaries — but note 2026-07-13: an ENCLOSING
> function does NOT make loop-body `const` safe (a loop inside a function still
> only runs the initializer on the first iteration). Whether a function declared
> IN a loop body can safely use `const` per call is unprobed; the boundary stop
> in the lint rule predates that question. **Add a default-parameters ban** — it
> is a parse error and the current config misses it. Array destructuring was not
> probed separately but object destructuring fails, so treat both as banned.

## ES2015 global objects: absent on AM, mostly present on IDM (verified 2026-07-30)

This is the one axis where AM and IDM genuinely differ in the _language_, not
just the bindings — and it is a silent-failure class, so it gets its own table.
`typeof X` never throws, so presence and usability were probed separately.

| Global    | AM legacy (1.0) | AM next-gen (2.0) | AM `LIBRARY` scope | IDM endpoint |
| --------- | --------------- | ----------------- | ------------------ | ------------ |
| `Map`     | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `Set`     | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `WeakMap` | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `WeakSet` | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `Symbol`  | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `Promise` | ❌ undefined    | ❌ undefined      | ❌ undefined       | ✅ function  |
| `Proxy`   | ❌ undefined    | ❌ undefined      | ❌ undefined       | ❌ undefined |
| `Reflect` | ❌ undefined    | ❌ undefined      | ❌ undefined       | ❌ undefined |
| `JSON`    | ✅ object       | ✅ object         | ✅ object          | ✅ object    |

Every cell is a probe result — there are no inferred or not-applicable cells.

`JSON` is the control row: it proves an all-`undefined` column is a real finding
and not a broken probe.

On AM, using one is a **runtime** failure, not a parse error — the script
compiles, runs, and throws `ReferenceError: "Map" is not defined.` at the
`new Map()` line. On IDM the verified surface is real, not nominal:
`m.set/get/size`, `s.add/has/size`, manual iterator walking (`m.keys()` →
`next()`), and `Array.from(new Set([3,1,3]))` → `3,1` all work. `Set` was
already verified on the **IDM schedule** engine on 2026-07-15 (construct, query,
`add`); the endpoint probe extends that to the whole collection family.

**Substitutes that are verified to work:**

- **All AM contexts** — a plain object for string keys (`o["a"] = 1`), and the
  object-keyed dedupe loop (`seen[item] = true`) in `LIBRARY` scope.
- **`java.util` collections** — available on both engines, but with a Map-shaped
  hole on next-gen. See the next section; do not assume `java.util.X` is
  reachable just because `JavaImporter` is.

**Do not generalise these rows to other ES6 features.** AM's Rhino is not simply
"ES6 off": arrow functions, template literals, `Array.from`, `String.includes`
and `String.normalize` all work, while `let`, destructuring, object shorthand
and every global above do not. That mix does not correspond to any single Rhino
language-version setting, so the status of any other ES6 feature has to be
probed, not inferred from this table.

Fixtures: `fixtures/es2015-globals.script.js` (next-gen),
`fixtures-legacy/legacy-es2015-globals.script.js` (legacy),
`fixtures/lib-es2015-globals-probe.lib.js` +
`lib-es2015-globals-consumer.script.js` (`LIBRARY`, uploaded as
`rhino-lib-es2015-globals-probe`, id `…7407`). The IDM column came from a
throwaway `endpoint/aicedit-mapset-probe`, deleted after the probe (`GET` → 404
confirmed).

> **Enforced since `TEMPLATES_VERSION` 46.** This used to type-check clean and
> fail in the tenant, because `src/scripts/templates/am/tsconfig.json` set
> `"lib": ["ES2015", …]` and TypeScript's `ES2015` umbrella lib DECLARES every
> global in the table above. AM is now narrowed to
> `["ES5", "ES2015.Core", "ES2016.Array.Include"]` — which keeps every ES2015
> feature AM verifiably has, since the templates' own types use hand-rolled
> `JavaMap`/`JavaSet` rather than the JS collections — and ESLint adds a
> `no-restricted-globals` entry naming the plain-object substitute. IDM keeps
> the collections it really has and drops only `ES2015.Proxy`/`ES2015.Reflect`.
> `ES2015.Iterable` is deliberately NOT in the AM list: it transitively pulls in
> `es2015.symbol` and would re-declare the `Symbol` global we are removing.
> Existing workspaces pick this up via `aic workspace update`.

## Post-ES5 features on the IDM endpoint engine (verified 2026-08-19)

The TypeScript endpoint bundle is esbuild + Babel **ES5** output, so two
different questions hide behind one artifact: what the ENGINE provides, and what
Babel's downlevel needs in order to run. Both were probed from a single throwaway
`endpoint/aic-ts-runtime-probe`, built by the workspace's own pipeline, with each
case individually try/caught so one failure cannot mask the rest. **V**

| Probe                                             | Result                         | What it settles                                                                                 |
| ------------------------------------------------- | ------------------------------ | ----------------------------------------------------------------------------------------------- |
| `typeof globalThis`                               | `"object"`                     | present — the only global the endpoint `lib` admits that the table above did not cover          |
| `typeof Object.entries`                           | `"function"`                   | **present, although `typescript/tsconfig.json` rejects it** — ES2017 is not in the pinned `lib` |
| `[1, 2].includes(2)`                              | `true`                         | ES2016 method works, matching the `ES2016.Array.Include` lib entry                              |
| `for (const n of [1, 2, 3])` sum                  | `6`                            | works — but this is Babel's `for (var …)` output, NOT engine `for...of`                         |
| generator, `function* () { yield 7 }` → `.next()` | `7`                            | Babel 8 inlines the regenerator helpers (`_regeneratorDefine`) and they RUN on IDM              |
| `async` arrow: `typeof p` / `typeof p.then`       | `"object"` / `"function"`      | `_asyncToGenerator` runs; an async function returns a real thenable                             |
| same, value read synchronously after `.then(…)`   | `"not-resolved-synchronously"` | the continuation had **not** run by the time the handler returned                               |

The last row is the operative one for endpoint authors: **a handler must be
synchronous.** An `async` handler returns a pending Promise, and a scripted
endpoint's return value is serialised as-is, so the caller receives the Promise
instead of the value it will eventually hold — with no error anywhere.

`Object.entries` is the one row that reads the other way: it is present on the
engine, and Babel's `preset-env` does not polyfill it, so using it would work.
The pinned `lib` is NARROWER than the runtime here. That is the safe direction —
it cannot admit runtime-impossible code — but it does mean the `tsconfig.json`
comment claiming the lib is pinned "to what the IDM script engine actually
provides" overstates the match. Widening to ES2017+ is a judgement call, not a
correction, and would need `TEMPLATES_VERSION` bumped either way.

Method: created with `aic script create endpoint/aic-ts-runtime-probe --from …`,
invoked with one `GET /openidm/endpoint/aic-ts-runtime-probe` (HTTP 200), removed
with `aic script delete --force` (`no synced script matches` confirmed after, and
the local `.cjs` deleted from the workspace).

## `java.util` collections on AM (verified 2026-07-30)

The obvious answer to "no JS `Map`" is a Java collection, and `JavaImporter` is
present on **both** engines (`typeof JavaImporter` → `"function"`, `typeof java`
→ `"object"`, next-gen included). But next-gen enforces a Java allow-list, and
that list has a **Map-shaped hole**:

| Construction                           | AM legacy (1.0) | AM next-gen (2.0) | Next-gen `LIBRARY` |
| -------------------------------------- | --------------- | ----------------- | ------------------ |
| `new java.util.HashSet()`              | —               | ✅ `true:1`       | ✅ `true:1`        |
| `new java.util.ArrayList()`            | —               | ✅ `a:1`          | ✅ `a:1`           |
| `new java.util.LinkedHashSet()`        | —               | ✅ works          | —                  |
| `new java.util.TreeSet()`              | —               | ✅ `a:2`          | —                  |
| `new java.util.HashMap()`              | ✅ `1:1`        | ❌ **blocked**    | ❌ **blocked**     |
| `new java.util.LinkedHashMap()`        | —               | ❌ blocked        | —                  |
| `new java.util.TreeMap()`              | —               | ❌ blocked        | —                  |
| `java.util.Collections.emptyMap()`     | —               | ✅ `0`            | —                  |
| `java.util.Collections.singletonMap()` | —               | ✅ `1:1`          | ✅ `1:1`           |
| `JavaImporter(java.util)` + `HashSet`  | ✅              | ✅ works          | —                  |
| `JavaImporter(java.util)` + `HashMap`  | ✅ `1:1`        | ❌ blocked        | —                  |

A blocked class does not throw a security error — the name simply never resolves
to a constructor, so it stays a package object and you get a **`TypeError`**:

```
TypeError: [JavaPackage java.util.HashMap] is not a function, it is object.
```

Via `JavaImporter` the same block reads
`TypeError: org.mozilla.javascript.Undefined@… is not a function, it is undefined.`
Either way it is a runtime failure at the construction site, not a parse error.

So a **mutable** Java `Map` is unavailable to next-gen AM scripts by any of the
probed routes. `Collections.singletonMap`/`emptyMap` return usable (immutable)
maps, and `Set`/`List` types are fine. For accumulating key→value data in a
next-gen script, a plain JS object remains the only verified option.

**The metadata's `allowLists` array is not the enforced list.** Each artifact
under `docs/api/bindings/` carries an `allowLists` array, and it is only a
partial guide:

- `scripted-decision-next.json` lists 51 entries including `java.util.HashSet`,
  `ArrayList`, `LinkedHashSet`, `TreeSet`, `LinkedList` and `Collections` — and
  notably **omits `java.util.HashMap`** while including
  `java.util.HashMap$KeyIterator` and `java.util.AbstractMap$*`. Enforcement
  matches: the listed classes work, `HashMap` is blocked.
- `library-next.json`, `oauth2-atm-next.json` and `oidc-claims-next.json` each
  list only **three** entries (`java.lang.Object` plus the two
  `org.forgerock.util.promise` types). Enforcement does **not** match: a
  `LIBRARY` script reaches `java.util.HashSet` and `ArrayList` perfectly well.

Read the per-context list as "classes this context's metadata bothered to
declare", not as the sandbox boundary. The enforced boundary appears uniform
across next-gen contexts and to correspond to the 51-entry decision-node list.
This is the same metadata-is-not-behaviour trap already recorded for context
usability and invocation contracts in `docs/api/13-script-contexts.md`.

Fixtures: `fixtures/java-collections.script.js` (next-gen decision node),
`fixtures/lib-java-collections-probe.lib.js` +
`lib-java-collections-consumer.script.js` (`LIBRARY`, uploaded as
`rhino-lib-java-collections-probe`, id `…7408`). The legacy `HashMap` row comes
from `fixtures-legacy/legacy-es2015-globals.script.js`. Cells marked `—` were
not probed in that context.

## `httpClient.send` body serialization (verified 2026-07-30)

`httpClient` does **not** serialize a JS object body with `JSON.stringify`
semantics. Two differences change what receivers see, and both are silent.
Probed by sending one object to a public echo service and diffing the raw body
it received against `JSON.stringify` of the same object computed in the same
script (`fixtures/httpclient-body-coercion.script.js`, next-gen decision node):

| Source value                        | `JSON.stringify` | On the wire via `httpClient` |
| ----------------------------------- | ---------------- | ---------------------------- |
| `intOne: 1`                         | `1`              | **`1.0`**                    |
| `intZero: 0`                        | `0`              | **`0.0`**                    |
| `negInt: -5`                        | `-5`             | **`-5.0`**                   |
| `bigInt: 1000000`                   | `1000000`        | **`1000000.0`**              |
| `floatVal: 1.5`                     | `1.5`            | `1.5`                        |
| `undefField: undefined`             | _key omitted_    | **`null`**                   |
| `nested: { u: undefined }`          | _key omitted_    | **`{"u":null}`**             |
| `arr: [1, undefined, 3]`            | `[1,null,3]`     | `[1.0,null,3.0]`             |
| `javaInt: new java.lang.Integer(1)` | `1`              | `1`                          |
| `javaLong: new java.lang.Long(1)`   | `1`              | `1`                          |

### 1. Every JS number becomes a double

JS numbers are IEEE doubles and the serializer renders them as such, so `1` goes
out as `1.0` at any depth (top level, nested object, array element) and at any
magnitude. Receivers that validate a JSON integer type — strict OpenAPI
`type: integer`, Jackson binding to `int`, many .NET models — reject it.

**Workaround (verified): box it.** `new java.lang.Integer(1)` and
`new java.lang.Long(1)` both serialize as `1`. `java.lang.Integer.valueOf(1)`
also works and avoids the deprecated boxing constructor — prefer it. For
contrast, `new java.lang.Double(1)` renders `1.0`, confirming the culprit is the
Java numeric type the value lands in.

Note that `java.lang.Integer` is **not** in the next-gen allow-list declared in
`docs/api/bindings/scripted-decision-next.json` (which has `Byte`, `Short`,
`Long`, `Float`, `Number`, `Void` but no `Integer`) yet constructs fine — a
third data point for the allow-list caveat in `docs/api/13-script-contexts.md`.

### 2. `undefined` becomes an explicit `null`

`JSON.stringify` **drops** a key whose value is `undefined`; `httpClient` sends
it as `null`. This matters when the receiver distinguishes "absent" from "null"
— a PATCH-style API that treats `null` as "clear this field", or a validator
that rejects `null` for an optional-but-non-nullable field. Building a body with
`obj.maybe = someLookup()` that can return `undefined` therefore transmits an
intentional-looking `null`. Delete the key instead:

```js
if (typeof value === "undefined") {
  delete body.field;
}
```

Both behaviours were observed on the next-gen engine. Not probed: the `form`
(form-encoded) option, the legacy `httpClient`, and whether `openidm.*` write
payloads share the serializer — do not assume they do.

> **Surfaced since `TEMPLATES_VERSION` 46.** `HttpOptions.body` stays typed as
> plain `object` — neither trap is expressible as a type without contorting the
> signature — but it now carries a JSDoc block documenting both, so they appear
> on editor hover at the call site. That is the only place an author is looking
> when they build a request body.

## AM binding matrix

`evaluatorVersion`: `2.0` = next-gen, `1.0` = legacy. "Folder slug" is the
workspace routing slug from `src/aic/script/am.rs::slug_for`.

| Binding                                  | Legacy                                                     | Next-gen                             | Status    | Shape / notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ---------------------------------------- | ---------------------------------------------------------- | ------------------------------------ | --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `logger`                                 | yes                                                        | yes                                  | **D/I**   | Legacy: `error/message/warning(+Enabled)`. Next-gen: slf4j-style `trace/debug/info/warn/error`. **Shapes differ** — must split legacy vs next-gen.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `httpClient`                             | partial                                                    | yes                                  | **D/I**   | Next-gen: `httpClient.send(url, opts).get()` returning fetch-like `{status,ok,json(),text()}`. Legacy: Java `Request`/`Response`. Used in 25 `src/`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `openidm`                                | no                                                         | yes                                  | **D/I**   | CRUDPAQ. Next-gen scripted decision + IDM only. Used in 63 `src/`. Not a legacy decision-node binding. `read` and **`query`** both take a third `fields` argument (verified 2026-08-17 — docs/api/10; the IDM type definitions were missing it on `query`). **LIBRARY confirmed** (2026-07-14): `openidm.read(...)` called from inside a function in a `require()`d LIBRARY script (not just the top-level decision-node script) works and returns the real record — `fixtures/lib-openidm-read-probe.lib.js` + `lib-openidm-read-consumer.script.js`, read `managed/alpha_name_variant/aaron_erin` and got the seeded row back verbatim. |
| `utils`                                  | no                                                         | yes                                  | **D/I**   | base64/UUID/random. Used in 86 `src/`. Next-gen only.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `nodeState`                              | yes                                                        | yes                                  | **D/I**   | Next-gen returns coerced JS types; legacy returns `JsonValue` needing `.asString()`. Used in 250 `src/`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `sharedState`/`transientState`           | yes                                                        | **deprecated/removed**               | **D/I**   | Replaced by `nodeState.putShared/putTransient` in next-gen. Still in 34/4 `src/` (legacy).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `action`                                 | via `Action` class                                         | binding                              | **D/I**   | Next-gen: `action.goTo(...)` chainable `ActionWrapper`. Legacy: static `Action.goTo()` (`ActionBuilder`). Used in 106 `src/`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `callbacks` / `callbacksBuilder`         | `callbacks`                                                | `callbacksBuilder`                   | **D/I**   | Legacy reads `callbacks`; next-gen builds via `callbacksBuilder.*`. Both names appear (~142 each).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `idRepository`                           | direct attr methods + `getIdentity()` → `ScriptedIdentity` | `getIdentity()` → `ScriptedIdentity` | **D/V/I** | `getIdentity()` exists on both engines. Legacy also has direct `getAttribute(user,attr)`. Used in 40 `src/`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `requestHeaders`/`requestParameters`     | yes                                                        | yes                                  | **D/I**   | `Map<String,String[]>`-ish `.get()` (case-insensitive). `Origin` and `Referer` are passed through when the client sends them — used for hosted-UI vs custom-UI branching (verified 2026-08-13; see `docs/api/09-journeys.md`). `keySet()` is blocked; iterate with `for…in` or `String(map)`.                                                                                                                                                                                                                                                                                                                                             |
| `requestCookies`                         | no                                                         | yes                                  | **D**     | Next-gen only (migrate doc).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `existingSession` / `resumedFromSuspend` | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `cacheManager`                           | no                                                         | yes                                  | **D**     | Next-gen scripted decision only.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `oauthApplication` / `samlApplication`   | no                                                         | yes                                  | **D**     | Next-gen only, journey-associated.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `auditEntryDetail`                       | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `realm` / `systemEnv` / `scriptName`     | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `require()` / library scripts            | **no**                                                     | **yes**                              | **D/V/I** | Only next-gen can `require` libraries. Libraries can require other libraries (17 `lib/` files do). Used in 225 `src/`. **Verified beyond scripted decision (2026-07-29):** works end-to-end from next-gen **token mod, validate scope, evaluate scope and may-act** (the whole reachable OAuth2 family — see the two sections below); `typeof require === "undefined"` in the legacy (`1.0`) token-mod script (`ReferenceError: "require" is not defined.`).                                                                                                                                                                              |
| `JavaImporter` / Java allowlist          | yes                                                        | **no**                               | **D/I**   | Legacy only. 26 `src/` use `JavaImporter`. Next-gen has no configurable Java access.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

### Runtime-verified binding presence (next-gen scripted decision, 2026-06-03)

`typeof <binding>` from inside a next-gen (`evaluatorVersion: 2.0`) scripted
decision node. Non-destructive (no method calls, no reads).

- **Present:** `require` (function), `openidm` (object), `httpClient` (object),
  `utils` (object), `logger` (object), `idRepository` (object), `nodeState`
  (object), `action` (object), `callbacks` (object), `callbacksBuilder`
  (object), `requestHeaders` (object), `requestParameters` (object),
  `requestCookies` (object), `realm` (string), `systemEnv` (object),
  `scriptName` (string), `secrets` (object), `resumedFromSuspend` (boolean),
  `JavaImporter` (function).
- **Absent (`undefined`):** `sharedState`, `transientState`, `existingSession`,
  `console`, `process`, `Buffer`, `setTimeout`.

Consequences:

- **No Node globals exist** — strip `console`/`process`/`Buffer`/timers from the
  AM ESLint globals (verified, not assumed).
- `sharedState`/`transientState` are **gone in next-gen** → keep them only in
  the legacy decision-node overlay; the next-gen overlay must not declare them.
- `systemEnv` and `JavaImporter` are present at runtime here (typeof) but are
  NOT in the editor binding metadata for any next-gen context (see
  `docs/api/13`). They're unlisted runtime globals: `systemEnv` is kept in the
  shared `common.d.ts` (usable everywhere); `JavaImporter` is typed only in the
  legacy overlay because next-gen has a fixed Java allow-list (the `allowLists`
  array in each artifact), so it shouldn't be relied on in next-gen.
- `existingSession` was `undefined` here only because the probe has no prior
  session; treat as context-dependent, not globally absent.

### Runtime-verified binding presence (LEGACY scripted decision, 2026-06-04)

Same `typeof` probe on a legacy (`evaluatorVersion: 1.0`) scripted decision node
(`scripts/rhino-script-tester/fixtures-legacy/legacy-bindings.script.js`; emits
via the classic `JavaImporter` + `Action.send(HiddenValueCallback)` path since
legacy has no `callbacksBuilder`).

- **Present (both engines):** `nodeState`, `callbacks`, `idRepository`,
  `httpClient`, `requestHeaders`, `requestParameters`, `resumedFromSuspend`,
  `secrets`, `JavaImporter`, `logger`, `realm`, `systemEnv`, `scriptName`.
- **Legacy-only (absent in next-gen):** `sharedState`, `transientState`.
- **Next-gen-only (absent in legacy):** `action`, `callbacksBuilder`, `openidm`,
  `utils`, `requestCookies`.

Consequences (applied to the type layering):

- `action` is **next-gen-only** (legacy imports the `Action` class via
  `JavaImporter` instead) → moved from `decision-node-base` to
  `decision-node-next`.
- `openidm` + `utils` are **next-gen-only** → moved from the shared
  `common.d.ts` to a new `nextgen-common.d.ts` included only by next-gen
  decision + library leaves, so the legacy leaf no longer mistypes them as
  present.
- `secrets` + `resumedFromSuspend` are in **both** → moved into
  `decision-node-base`.
- `nodeState` is present on **both** engines (legacy returns `JsonValue`-style
  values needing `.asString()`; next-gen returns coerced JS — a shape
  difference, not a presence one).
- **2026-07-22:** `library.d.ts` redeclares `NodeState`, `RequestHeaders`, and
  `RequestParameters` as types for library `.load(...)` factory parameters;
  their scripted-decision globals remain outside library scope.
- **2026-08-26:** the same argument applies to every other binding a caller can
  hand a library — `CallbacksBuilder` was the one that surfaced it — so the
  library leaf now also includes **`library-args.d.ts`**, one type per binding
  merged from every next-gen context's metadata (`gen-binding-types.mjs
  --library-args`, regenerate command in the file's footer). The types are
  there; the `declare const`s are not, which is the distinction library scope
  actually makes. `NodeState` stays hand-written in `library.d.ts` (the metadata
  types `get` as a bare `object`), and `ExistingSession` and the
  `OAuthApplication` spelling live there too because the metadata cannot
  describe them. `am::leaf_tsconfig`'s unit tests read the binding artifacts
  directly, so a newly captured context fails until the file is regenerated.

### Method surfaces verified 2026-06-04 (legacy engine)

`typeof` of each member on a legacy scripted decision node
(`fixtures-legacy/legacy-nodestate-logger.script.js`):

- **`nodeState`** (legacy): `get`, `getObject`, `putShared`, `putTransient`,
  `mergeShared`, `mergeTransient` — plus **`isDefined` and `remove`** (function;
  _not in the docs_). There is **no** `nodeState.sharedState(key)` /
  `transientState(key)` / `secureState(key)` accessor — `get()` is the unified
  accessor (reads transient→secure→shared). Legacy shared/transient state is
  reached through the **standalone** `sharedState`/`transientState` bindings
  (e.g. `sharedState.get('k')`), which next-gen removes.
- **`logger`** (legacy): `error`, `message`, `warning`, `errorEnabled`,
  `messageEnabled`, `warningEnabled` — the classic `Debug` object.
  `trace`/`debug`/`info`/`warn` are **absent** (those are the next-gen slf4j
  shape). Now runtime-confirmed, not just doc-claimed.
- **`idRepository`** (legacy; verified 2026-07-06 with
  `fixtures-legacy/legacy-idrepository-methods.script.js`): `getIdentity`,
  `getAttribute`, `setAttribute`, and `addAttribute` are all functions.

Type model: the slf4j `logger` lives in `nextgen-common.d.ts`; the classic
`Debug` `logger` in `legacy-common.d.ts` (included by the legacy decision leaf
and the other unmigrated AM contexts). `nodeState.isDefined`/`remove` are merged
onto `NodeState` only on the legacy leaf (via `decision-node-legacy.d.ts`).

### Library `require()` from next-gen access-token modification (verified 2026-07-29)

Question: can an OAuth2 **access token modification** script use AM library
scripts? Answer: **yes, but only the next-gen context**
(`OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN`, `evaluatorVersion 2.0`). The
legacy context (`OAUTH2_ACCESS_TOKEN_MODIFICATION`, `1.0`) has no `require` at
all.

Probe (throwaway resources, all deleted afterwards): a `LIBRARY` script
exporting `stamp()` plus `typeof` readings; a next-gen token-mod script that
`require()`d it by **script name** and wrote the results with
`accessToken.setField(...)`; a throwaway confidential client
(`grantTypes: ["client_credentials"]`) with
`overrideOAuth2ClientConfig.providerOverridesEnabled = true`,
`accessTokenModificationPluginType: "SCRIPTED"`,
`accessTokenModificationScript: <probe id>`, and `statelessTokensEnabled: true`
so the claims are readable straight out of the JWT.

Resulting access-token claims:

| Claim                            | Value                       | Meaning                                                   |
| -------------------------------- | --------------------------- | --------------------------------------------------------- |
| `aicedit_typeof_require`         | `function`                  | `require` exists in the next-gen token-mod script         |
| `aicedit_probe`                  | `ok-from-library`           | library function executed, return value reached the token |
| `aicedit_lib_script_name`        | `aicedit-atm-nextgen-probe` | inside the library, `scriptName` is the **caller's** name |
| `aicedit_lib_typeof_accessToken` | `undefined`                 | the `accessToken` binding is **not** in library scope     |
| `aicedit_lib_typeof_openidm`     | `object`                    | `openidm` **is** in library scope                         |
| `aicedit_lib_typeof_httpClient`  | `object`                    | `httpClient` **is** in library scope                      |

Same client pointed at a legacy (`1.0`) token-mod script:
`aicedit_typeof_require = "undefined"` and
`require("…") → ReferenceError: "require" is not defined.` (caught, token still
issued).

Consequences:

- Context-specific bindings (`accessToken`, and by the same rule `identity`,
  `scopes`, `requestProperties`, `clientProperties`) stay **outside** library
  scope — same behaviour already recorded for the scripted-decision globals.
  Pass them in as arguments, or have the library return a value the caller
  applies with `accessToken.setField(...)`.
- The library's own next-gen common bindings (`openidm`, `httpClient`, `logger`,
  `secrets`, `utils`, …) are available, so shared code that reads managed
  objects or calls out over HTTP works unchanged in a token-mod call path.
- To use a library from an existing token-mod script you must first migrate that
  script to the `…_NEXT_GEN` context (`evaluatorVersion 2.0`) — the two contexts
  are separate ids, so this is a rewrite of the script's bindings, not a flag
  flip. The sandbox alpha realm's provider-level `Modify Access Token`
  (`9c98f803-…`) and the per-client `TestAccessTokenModification` (`f65303d2-…`)
  were both still legacy `1.0` as of this probe.

The context's binding metadata (18 bindings, `JAVASCRIPT: ["2.0"]`, full
`accessToken` method surface) is captured at
`docs/api/bindings/oauth2-atm-next.json`. Note `require` is a language-level
function, not a binding — it appears in **no** context's binding list, including
`SCRIPTED_DECISION_NODE`, so its absence there is not evidence either way.

### Library `require()` across the rest of the next-gen OAuth2 family (2026-07-29)

Follow-up to the token-mod probe above. Each script `require()`d the same
throwaway `LIBRARY` script and logged
`AICEDIT-PROBE <ctx> require=<typeof require> value=<lib.stamp(...)>`; markers
were read back from `GET /monitoring/logs?source=am-core`. Scripts were attached
**one at a time** via `overrideOAuth2ClientConfig` on a throwaway
`client_credentials` client (never realm-wide), and everything was deleted
afterwards.

| Context (`…_NEXT_GEN`)                    | `require`       | How it was observed                                                                                    |
| ----------------------------------------- | --------------- | ------------------------------------------------------------------------------------------------------ |
| `OAUTH2_VALIDATE_SCOPE`                   | ✅ `function`   | log marker at top level **and** inside the AM-invoked `validateAccessTokenScope()`; token issued `200` |
| `OAUTH2_EVALUATE_SCOPE`                   | ✅ `function`   | returned map surfaced as `aicedit_es_probe: "es-from-library"` in `/oauth2/tokeninfo`                  |
| `OAUTH2_MAY_ACT`                          | ✅ `function`   | `token.setMayAct({client_id: lib.stamp("ma")})` → `may_act.client_id: "ma-from-library"` in the JWT    |
| `OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER` | **unprobed**    | needs an authenticated resource-owner session — see below                                              |
| `OAUTH2_SCRIPTED_JWT_ISSUER`              | **unreachable** | no configuration hook exists in AIC — see below                                                        |

So `require()` is confirmed on every next-gen context that can currently be
reached: scripted decision, library-to-library, token mod, validate scope,
evaluate scope, may-act.

**Next-gen scripted decision sends callbacks by accumulation, not by an
`action.send` (2026-08-25).** There is no `send` on the `action` binding at all;
call `callbacksBuilder.<type>(…)` as many times as you like and AM sends the lot
if the script does not `goTo` an outcome. And `callbacks.getXCallbacks().get(0)`
returns the **submitted value**, not a callback object — `.getName()` /
`.getValue()` on it throw `TypeError`. Worked example and the two-post drive
loop: [09-journeys.md](09-journeys.md).

**Next-gen validate-scope is a function-entry-point script.** This is a contract
difference, not a bindings one, and it bites immediately: a top-level script
body (the token-mod/evaluate-scope style) fails the token request with
`500 {"error":"server_error","error_description":"Error while running validate scope script"}`,
and the log's root cause is
`java.lang.IllegalArgumentException: validateAccessTokenScope is not a function`
(`RhinoScriptEngine.invokeFunction` → `ScriptedScopeValidator.runScript`). The
script must **declare named functions** that AM invokes per operation:

```javascript
function validateAccessTokenScope() {
  var lib = require("my-lib"); // works here — verified
  return toArray(requestedScopes);
}
function validateRefreshTokenScope() { … }
function validateAuthorizationScope() { … }
function validateBackChannelAuthorizationScope() { … }
function validateDeviceCodeScope() { … }
```

Only `validateAccessTokenScope` was observed being called (client-credentials
grant); the other four names are defensive and unconfirmed. Bindings are still
globals — the functions take no arguments. The top-level body **does** run (its
`require` and log fired) before the function is invoked, so top-level setup is
fine; the return value just has to come from the function.

Other observations worth keeping:

- Returning an extra scope from validate-scope did **not** widen the grant: with
  `requestedScopes = [aicedit-probe]` and the script returning
  `[aicedit-probe, vsd-from-library]` (the extra one configured on the client
  and present in `availableScopes`), the issued token's `scope` claim was still
  just `["aicedit-probe"]`. AM appears to intersect with what was requested for
  this grant type.
- `availableScopes` reflects live client config — adding a scope to the client
  showed up in the next call's binding value.
- evaluate-scope's returned map lands in the **`/oauth2/tokeninfo` response as a
  top-level field**, and is **not** a claim in the client-based JWT.
- `identity` was not exercised: a `client_credentials` grant has no resource
  owner.

**`OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN` has metadata but nowhere to attach it.**
Searched every script/plugin field on the OAuth2 provider service
(`realm-config/services/oauth-oidc`), the OAuth2 client override block, and the
`TrustedJwtIssuer` agent template (9 fields: `allowedSubjects`, `jwkSet`,
`jwksUri`, `jwksCacheTimeout`, `jwkStoreCacheMissCacheTime`, `issuer`,
`consentedScopesClaim`, `resourceOwnerIdentityClaim`, `agentgroup`) — no
scripted-JWT-issuer reference anywhere. The context is typed for completeness;
it can't be exercised (or used) in AIC today.

**`OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER_NEXT_GEN` needs an end-user
session.** Confirmed not invoked by the two client-only paths: device-code
initiation (`POST /oauth2/device/code` → `200`, no marker logged) and an
unauthenticated `GET /oauth2/authorize` (`302` to `/am/UI/Login`; logs show
`SSOException: SessionID is empty` / `The request requires a redirect`). Probing
it requires authenticating as a resource owner in `alpha`, which means either
known test-user credentials or creating a throwaway managed user — the latter
touches the IDM sync queue and any `onCreate` hooks, so it wasn't done
unprompted.

### AM script families (folder slugs)

| Family                                  | Slug                   | `evaluatorVersion` | Library support                               | Bindings overlay                                                                |
| --------------------------------------- | ---------------------- | ------------------ | --------------------------------------------- | ------------------------------------------------------------------------------- |
| Scripted decision (next-gen)            | `decision-node`        | `2.0`              | yes                                           | decision-node-base + next-gen                                                   |
| Scripted decision (legacy)              | `decision-node-legacy` | `1.0`              | no                                            | decision-node-base + legacy                                                     |
| Library                                 | `lib`                  | (next-gen)         | yes (CommonJS)                                | library + library-args (caller argument types)                                  |
| OIDC claims                             | `oidc-claims`          | mixed              | next-gen only                                 | oidc-claims                                                                     |
| OAuth2 (token mod, scope, jwt, dcr, …)  | `oauth2-*`             | mixed              | next-gen only (token mod verified 2026-07-29) | all next-gen OAuth2 contexts typed (2026-07-29); legacy ids shared globals only |
| SAML2 (idp/sp adapter, mappers)         | `saml-*`               | mixed              | next-gen only                                 | per-context (future)                                                            |
| Social normalization/handler            | `social-*`             | mixed              | —                                             | per-context (future)                                                            |
| Config provider / device match / policy | various                | mixed              | —                                             | shared globals only (today)                                                     |

## IDM binding matrix

IDM scripts are tenant-global (no realm). The IDM **endpoint**
`request`/`context` shapes are runtime-verified (2026-06-04 — see
`docs/api/11`). The endpoint `request` is a discriminated union on `method`;
`context.http` carries the HTTP request.

| Binding          | Endpoint | Schedule | Status  | Notes                                                                                                                                                                                                                                                                                                                                               |
| ---------------- | -------- | -------- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `openidm`        | yes      | yes      | **I**   | CRUDPAQ + `update`. From `idmCommon.d.ts`.                                                                                                                                                                                                                                                                                                          |
| `logger`         | yes      | yes      | **I**   | slf4j-style.                                                                                                                                                                                                                                                                                                                                        |
| `identityServer` | yes      | yes      | **D/V** | `getProperty(name, defaultValue?, substitute?)`; a missing ESV/property returns `null` when no default is supplied, or the supplied default. Endpoint behavior verified 2026-07-22 below.                                                                                                                                                           |
| `request`        | yes      | no       | **V**   | Discriminated union per CREST method (read/create/update/patch/delete/action/query). `docs/api/11`.                                                                                                                                                                                                                                                 |
| `context`        | yes      | no       | **V**   | `context.http` = {method,path,headers,parameters}; `context.security` = {authenticationId,authorization}; `context.oauth2` = {scopes (a `JavaSet` — use `.contains()`), rawInfo, token, expiresAt}, present only behind `rsFilter`; `rawInfo` is AM's 15-key introspection record (`client_id`, `user_id`, `exp`, …) — full table in `docs/api/11`. |

### IDM `identityServer.getProperty` missing ESV behavior (verified 2026-07-22)

`identityServer.getProperty("esv.some.variable")` **does not throw** when the
ESV/property does not exist. It returns JavaScript `null`. Supplying the
optional second argument returns that string instead:

```javascript
var absent = identityServer.getProperty(
  "esv.aicedit.definitely.nonexistent.20260722",
);
// absent === null

var fallback = identityServer.getProperty(
  "esv.aicedit.definitely.nonexistent.20260722",
  "aicedit-fallback-value",
);
// fallback === "aicedit-fallback-value"
```

Live probe: a temporary scripted IDM endpoint invoked both calls, returning
`{type:"object", value:null}` for the first (Rhino reports `typeof null` as
`"object"`) and `{type:"string", value:"aicedit-fallback-value"}` for the
second. It was deleted after verification. Use a default for optional ESVs;
otherwise explicitly guard for `null`.

### IDM endpoint engine — syntax (verified 2026-06-04)

The IDM script engine is **newer than AM's Rhino 1.7.14** — a source that fails
to compile makes the endpoint un-routable (404 on invoke; reverting to `var`
restores 200), which is how each row below was checked:

| Feature                               | IDM endpoint     | AM Rhino 1.7.14 (contrast)   |
| ------------------------------------- | ---------------- | ---------------------------- |
| `var`                                 | ✅               | ✅                           |
| `let` (any scope)                     | ✅ works         | ❌ parse error               |
| `const` top-level / in-function       | ✅ works         | ⚠️ top-level reads undefined |
| arrow fns, template literals          | ✅               | ✅                           |
| object shorthand, destructuring       | ✅ works         | ❌ parse error               |
| default parameters                    | ❌ compile-fails | ❌ parse error               |
| `const` in `for`/`for-of` initializer | ❌ compile-fails | ❌ parse error               |

Added 2026-07-30: the IDM engine also has the **ES2015 global objects** AM lacks
entirely (`Map`, `Set`, `WeakMap`, `WeakSet`, `Symbol`, `Promise` — but not
`Proxy`/`Reflect`). Full cross-engine table and verified method surface in the
ES2015-globals section above. Unlike the syntax rows, these are runtime presence
checks, so a missing one fails at the point of use rather than making the
endpoint un-routable.

For IDM endpoints, ESLint bans **default parameters** and **`const` in a loop
initializer** — NOT general `let`/`const`/shorthand/destructuring. Schedule
scripts have the additional loop-body restriction below.

### IDM schedule engine — syntax (verified 2026-07-15)

A disabled throwaway schedule was invoked synchronously with
`POST /openidm/scheduler/job/aicedit-idm-modern-syntax-probe?_action=trigger`.
It successfully evaluated root-level `const` and `let`, constructed and queried
a `Set`, used a template literal, and wrote the interpolated result
`root:2:2:true` through `openidm.create`. The temporary managed-object record
and schedule were deleted after the probe.

**Schedule-specific loop-body exception.** A follow-up live probe on 2026-07-15
showed that `const` declared inside a schedule loop body parses but silently
terminates the loop after its first iteration (created sum 0; expected 3). That
follow-up also live-verified `Set.add()`, producing a three-element set.
`for (let ...)` without a loop-body `const` completed correctly. Schedule
linting and generated source must therefore use `let` for every binding declared
in a `for`/`while`/`do-while` body or nested repeated block. Root-level
immutable bindings remain safe as `const`.

## Open items still requiring runtime probes

Resolved 2026-06-03 (next-gen scripted decision): all syntax rows above; binding
_presence_ (typeof). Resolved 2026-06-04: legacy (`evaluatorVersion: 1.0`)
binding _presence_ (see the legacy section above; the tester now takes
`EVALUATOR_VERSION`). Still open:

1. Binding _shapes_ (not just presence): `logger` method names + slf4j-style
   `{}` placeholder formatting; `openidm` call return shapes. Presence is
   verified; exact shapes still lean on docs. **Partially resolved 2026-07-30**
   for `httpClient.send(...).get()`: `ok`, `status`, `statusText` and `text()`
   are runtime-verified (`fixtures/httpclient-body-coercion.script.js`), as is
   the request-side body serializer (see the section above). `json()` and the
   `headers` shape are still unverified.
2. ~~`require()` of a real library from a next-gen scripted decision~~ RESOLVED
   — end-to-end library imports are verified from a next-gen scripted decision
   (2026-07-13/14 rows above) and from next-gen access-token modification
   (2026-07-29 section above), plus **validate scope, evaluate scope and
   may-act** in the follow-up probe the same day. Still unprobed: OIDC claims,
   DCR, SAML mappers/adapters (all typed, all given the library alias on the
   family rule — inference, not probe);
   `OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER_NEXT_GEN` (needs a resource-owner
   session); and `OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN` (no config hook exists in
   AIC — unreachable, not merely unprobed).
3. IDM **schedule** scripts: binding shapes other than the now-verified
   `openidm.create`; modern syntax is verified above. Schedules can be probed
   synchronously with the scheduler `trigger` action documented in
   `docs/api/11-idm-endpoints.md`.
4. **Exact `callbacksBuilder` / `utils` argument sets.** RESOLVED (2026-06-04).
   The tenant owner extracted the script editor's authoritative binding metadata
   — saved at `docs/api/bindings/scripted-decision-next.json` (the
   `SCRIPTED_DECISION_NODE` next-gen binding surface: 25 bindings, every method
   with named params + types + overloads + the Java allow-list). All of it is
   now typed from that source: `callbacksBuilder` (every overload), `utils`
   (base64/base64url/crypto incl. full `subtle`, types), `action` (full
   chainable surface), `openidm` (CRUDPAQ + overloads), `nodeState` (incl.
   `isDefined`/`remove`/`keys`), `callbacks` getters, enriched `secrets`, and
   previously-missing bindings: `samlApplication`, `oauthApplication`,
   `jwtAssertion`, `jwtValidator`, `policy`, `journey`, `cacheManager`,
   `cookieName`. (Corrected guesses:
   `suspendedTextOutputCallback(messageType, message)`; `utils.crypto` has no
   `randomValues`.) The metadata only covers `evaluatorVersion 2.0`; the
   equivalent legacy dump would let us tighten the legacy leaf the same way.

   Source: the script-context endpoint `GET /am/json/{realm}/contexts/{ID}` (see
   `docs/api/13-script-contexts.md`). It exposes the same metadata for any
   context **once upgraded to next-gen** — 9 have it today (artifacts in
   `docs/api/bindings/`). Legacy-only contexts (plain OIDC claims, most OAuth2,
   policy condition, SAML adapters/attr-mapper, config provider) return 0
   bindings until upgraded.

5. **`httpClient` auth.** Verified (user, 2026-06-04): next-gen `httpClient`
   ignores a directly-set `Authorization` header — use `HttpOptions.token`, sent
   as `Authorization: Bearer <token>`. **Open: Basic auth delivery.** The Basic
   credential itself is built with `utils.base64.encode("user:pass")` (per a
   script-bindings doc example), but how it's _delivered_ in next-gen is unknown
   (the header is ignored; `token` is Bearer-only). Investigate, then type it.
