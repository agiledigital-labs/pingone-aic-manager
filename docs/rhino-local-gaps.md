# rhino-local gaps — real scripts vs the local overlay

What happens when the local harness is pointed at the scripted-decision scripts
this repo actually has, instead of the three toy cases.

**This is a report, not an implementation.** The cases live under
`scripts/rhino-local/ts/cases/real/`. The overlay in
`src/bindings/rhino/runtime.cjs` was not touched. Measured 2026-09-12 against
the JVM runner (`VERSION_DEFAULT` + `ScriptContextScope`).

## Predicted local-vs-live disagreements (read these first)

None of these could be _measured_ this morning: every script that would exercise
them dies first on `callbacks.isEmpty` (below). They are still the highest-value
findings, because they are disagreements in code that already exists, not
missing methods. When `callbacks.isEmpty` lands, these cases will run and the
verdict engine will catch them — do not tune the expect to match the overlay.

### 1. `idRepository.getIdentity` accepts `userName` locally; AIC does not

Live (`docs/api/14-am-identity-attributes.md`, probed `identity-resolve-diag`):
in a next-gen scripted decision, `getIdentity(<fr-idm-uuid>)` returns a working
`ScriptedIdentity`; `getIdentity(<userName>)` and `getIdentity("amadmin")`
return a stub whose `amIdentity` is null and every `getAttributeValues` throws
`InternalError: … this.amIdentity is null`.

Local (`runtime.cjs`): `getIdentity` matches `_id` **or** `userName`. A case
seeded with `{ _id: "<uuid>", userName: "alice" }` will resolve `"alice"`. That
is the opposite of AIC.

Case: `identity-resolve-diag`. Expect is the live matrix (UUID works, userName
does not). The case uses reserved placeholders, not the sandbox test user.

### 2. `getAttributeValues` returns a JS array locally, not AIC's Java collection

Live (`docs/api/14`, probed `identity-getattribute-shape`, 2026-09-10): the
container has numeric `.length`, `.size()`, `.get(0)`, `[0]`, `.toArray()`,
`.contains()`; `String(v)` is `[{…}]`; `.includes()` **throws**. It is not a
`java.util.Set` and not a Java `String[]`.

Local `__rhinoLocalJavaList` is a JS array with hidden `size` / `get` /
`isEmpty` / `contains`. Consequences:

| Probe         | AIC     | Local overlay     |
| ------------- | ------- | ----------------- |
| `.length`     | number  | number (JS array) |
| `.size()`     | works   | works             |
| `.toArray()`  | works   | missing → throws  |
| `.includes()` | throws  | works (Array)     |
| `.contains()` | works   | works             |
| `String(v)`   | `[{…}]` | JS array toString |

`.includes()` working locally is the dangerous one: a script that type-checks
against `JavaArray` and is banned from `.includes()` on AIC would pass the local
harness.

Case: `identity-getattribute-shape`.

### 3. `httpClient.send` body coercion is unmeasured locally

Live (`docs/api/12`, probed `httpclient-body-coercion`): JS `1` goes out as
`1.0`; `undefined` becomes JSON `null` rather than being dropped.

Local `httpClient.send` records the JS object and returns a stub. It does not
run AM's Java serializer. The case's expect is the live coercion; a stubbed
reply cannot prove it. When the case can run, a pass against a stub is **not** a
fidelity match — treat a green `httpclient-body-coercion` as "the stub was hit",
not "the wire matches AIC".

### 4. Mock-method enumerability vs Java `for-in`

`enum-callbacks-utils` / `enum-utils-sub` enumerate `callbacksBuilder` and
`utils.*` with `for-in`. Live those are Java objects; locally they are JS
objects whose methods are enumerable. The name lists will disagree even after
`callbacks.isEmpty` lands. That is a mock-shape gap, not a missing method.

---

## Where the real scripts are

| Source                                                                                | What it is                                                                | Verdict                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/rhino-script-tester/fixtures/*.script.js`                                    | Next-gen scripted-decision probes, run live through `AIC-Rhino-Let-Probe` | **This is the in-repo corpus.** Recorded outcomes live in `docs/api/12-script-bindings-matrix.md` and `docs/api/14-am-identity-attributes.md`. `tmp/rhino-script-tester/probe-results.json` is gitignored and was not in this checkout. |
| `scripts/rhino-script-tester/fixtures-legacy/*.script.js`                             | Legacy (`evaluatorVersion` 1.0) scripted-decision probes                  | Same kind, different engine. Converted with `given.engine = "legacy"`.                                                                                                                                                                  |
| `scripts/rhino-script-tester/scripts/*.script.js`                                     | Earlier `let` / `var` probes, same journey                                | Converted.                                                                                                                                                                                                                              |
| `scripts/rhino-script-tester/fixtures/*.lib.js`                                       | LIBRARY bodies, not decision-node scripts                                 | Not converted as cases. Their _consumers_ (`lib-*-consumer.script.js`) are. The harness does not model `require` / `libraryBindings`.                                                                                                   |
| `scripts/rhino-script-tester/fixtures-oauth2/`, `fixtures-atm/`, `fixtures-exchange/` | Validate-scope, access-token-modification, may-act                        | **Wrong script kind.** Different binding JSON, different invocation. Not converted.                                                                                                                                                     |
| `src/scripts/templates/`                                                              | Workspace `.d.ts`, ESLint, TypeScript endpoint seeds                      | **Not scripts.** No scripted-decision body ships to users from here.                                                                                                                                                                    |
| `scripts/type-tests/leaves/nextgen-decision-node/accept.cjs`                          | Type-acceptance fixture                                                   | Uses `openidm` / `logger` / `requestParameters` but never sets `outcome` or calls `action.goTo`. Not a decision-node script.                                                                                                            |
| `<client-checkout>/sandbox-scripts`                                                            | The 384 `src/` / 56 `lib/` production corpus cited in the matrix          | **Not on this machine.** `.ai/local.md` is absent.                                                                                                                                                                                      |
| Sibling `pingone-aic-manager/workspace/sandbox/am/`                                   | Pulled tenant scripts                                                     | Almost empty of decision-node scripts (two LIBRARY probes and one ATM script).                                                                                                                                                          |

Pushback, as invited: the probe fixtures are the richest source of
scripted-decision bodies _with recorded live behaviour_ in this repo. They are
probes, not production journey scripts. A production corpus would be the client-a
sandbox checkout, and it is not here. Converting probes is still the right move
— they are the scripts whose live outcomes we can actually assert.

## What was converted

52 cases under `scripts/rhino-local/ts/cases/real/`:

- 45 next-gen (43 `fixtures/*.script.js` + 2 `scripts/*.script.js`)
- 7 legacy

Identity cases originally named a sandbox test user and three managed-object
UUIDs. Those are reserved placeholders (`alice` / `bob` /
`00000000-0000-0000-0000-000000000000` and `…0001` / `…0002`). The case comments
say so.

**How many run today: 0.** 43 die on one unimplemented binding method; 9 are
language parse errors the case format cannot express as a verdict.

The suite stays green by asserting the throw still happens
(`test/corpus/real.test.ts`). When the overlay implements the named method, that
assertion fails and the case has to be re-triaged — that is how the corpus
grows.

## Ranked gap list — first uncaught throw

Ranked by how many converted scripts the method blocks. The parallel lane should
work top-down from row 1.

### 1. `callbacks.isEmpty` — 43 scripts

Exact throw (JVM runner, 2026-09-12):

```text
rhino-local: case "<name>" runtime_error: Error: rhino-local: not mocked: callbacks.isEmpty arity=0 overload=[isEmpty()] (rhino-local-mocks.cjs#53)
```

Every next-gen probe reports through

```js
if (callbacks.isEmpty()) {
  callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
}
```

Legacy probes do the same, then `JavaImporter` +
`Action.send(new HiddenValueCallback(…))`.

This is also the reporting channel the AIC-lane result script uses
(`src/aic/emit-result.ts`). Implementing it unblocks the corpus _and_ the
wrapper journey's dump.

It is **not** always the first method the script _calls_. Logger, identity, and
`httpClient` probes call implemented methods inside `try` and then die on
`emit`. The uncaught throw is still `callbacks.isEmpty` because that is how they
publish. Implementing it will let those scripts finish and produce a verdict —
which is when the fidelity gaps above become measurable.

Scripts blocked (43):

`arrow-function`, `template-literal`, `const-in-function`,
`const-uniq-across-blocks`, `es2015-methods`, `es2015-globals`,
`bindings-availability`, `const-top-level`, `const-in-loop-body`,
`const-in-nested-loop-block`, `const-in-while-body`, `const-in-do-while-body`,
`const-in-loop-in-function`, `rhino-var-control`, `enum-callbacks-utils`,
`enum-utils-sub`, `logger-placeholders`, `request-multivalue`,
`request-headers-dump`, `httpclient-body-coercion`, `java-collections`,
`java-class-shutter`, `for-each-java-collection`, `string-normalize`,
`lib-array-fill-consumer`, `lib-const-consumer`, `lib-const-loop-consumer`,
`lib-es2015-globals-consumer`, `lib-java-collections-consumer`,
`lib-openidm-read-consumer`, `lib-openidm-miss-consumer`,
`identity-resolve-diag`, `identity-attr-mapping`, `identity-getattribute-shape`,
`identity-enum-attrs`, `identity-manager-swap`, `legacy-bindings`,
`legacy-es2015-globals`, `legacy-idrepository-methods`,
`legacy-nodestate-logger`, `legacy-logger-args`, `legacy-logger-levels`,
`legacy-request-multivalue`.

`callbacksBuilder.hiddenValueCallback` is already implemented. After `isEmpty`
returns `true` on a first visit, those 43 scripts can emit.

### 2. Nothing else is a first-uncaught binding throw today

No other generated mock method is the first uncaught throw of any converted
script. Parse errors (next section) are language, not bindings.

## After `callbacks.isEmpty` — what the same scripts hit next

Static, from the copied bodies. Not measured as uncaught throws, because they
never get there. Ordered by how many of the 43 an implementation would unblock
_further_.

| Next gap                                                                                                                            | Scripts                                                                                                                                   | What happens                                                                                                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `require` / `libraryBindings` (not a generated mock method; Rhino `require` is absent)                                              | 8: every `lib-*-consumer`, plus `string-normalize` (its last probe `require`s `rhino-lib-normalize-probe`)                                | Caught today (`try/require/catch/emit`). After `isEmpty`, the case will _run_ and emit `{ ok: false, error: "ReferenceError: \"require\" is not defined." }`, which fails the live expect. That is a finding, not a skip to pre-empt.                    |
| `JavaImporter` + `org.forgerock.openam.auth.node.api.Action.send` + `com.sun.identity.authentication.callbacks.HiddenValueCallback` | 7 legacy                                                                                                                                  | After `isEmpty`, legacy emit takes the Java path. The harness does not load those AM classes. Next throw will be a Java `TypeError` / missing class, not `not mocked: …`.                                                                                |
| `java.util.*` constructors and the class shutter                                                                                    | 3 next-gen: `java-collections`, `java-class-shutter`, `for-each-java-collection` (+ `lib-java-collections-consumer` once `require` works) | Language/Java-interop, not a mock method. Local Rhino will construct classes the AM shutter hides, and will fail to hide `HashMap`. Predicted fidelity gap, same family as the matrix's shutter table.                                                   |
| `Map.getClass()` / `JavaCollection.getClass()`                                                                                      | `request-multivalue`, `request-headers-dump`, `legacy-request-multivalue`, `java-class-shutter`                                           | Local request maps are JS objects. `getClass` is missing. Caught inside those fixtures' `try`, so the case can still emit — with `className: "err:…"`. Live recorded a Java class name. Fidelity gap on that field.                                      |
| `ScriptedIdentity.getAttributes` / `getAttributeNames` / `asMap`                                                                    | `identity-enum-attrs`                                                                                                                     | `typeof` then a guarded call. Local identity object does not have them. Live next-gen wrapper likely does not either (`getAttribute` is already known-absent). A throw here may _match_ AIC.                                                             |
| `ScriptedIdentity.getAttribute`                                                                                                     | `identity-getattribute-shape`                                                                                                             | Called, expected to throw on next-gen AIC (`TypeError: Cannot find function getAttribute in object …ScriptedIdentityScriptWrapper`). Local: `id.getAttribute` is undefined, so a JS `TypeError`. Same outcome, different message — a small fidelity gap. |
| `httpClient.send`                                                                                                                   | `httpclient-body-coercion`                                                                                                                | **Already implemented.** After `isEmpty` this case can complete against a stub. See predicted gap 3.                                                                                                                                                     |

`utils.base64.*`, `utils.crypto.*`, `action.suspend`,
`action.withIdentifiedUser`, `nodeState.keys`, `nodeState.remove`,
`nodeState.mergeShared`, `jwtAssertion`, `samlApplication`, `cacheManager`,
`oauthApplication`, `journey`, `policy`, `secrets` (the object, not `systemEnv`)
— **no converted script calls them.** They are not what stands between the
harness and this corpus. They may well stand between the harness and the client-a
production scripts.

`nodeState.keys` is used by the AIC-lane dump script, not by these probes. It is
the second method to implement if the wrapper journey needs to round-trip state,
but it is not a corpus blocker.

## Parse errors — not binding gaps

Nine cases never reach a binding. Live recorded HTTP 401 / no
HiddenValueCallback. The case format has no `compile_error` channel, so they are
blocked on `(parse)` with the JVM's exact message. Already covered as
language-corpus rows in `docs/rhino-local-harness.md`; kept here so the
real-script set is complete.

| Case                      | Exact throw (2026-09-12)                                                             | Live (matrix)                       |
| ------------------------- | ------------------------------------------------------------------------------------ | ----------------------------------- |
| `rhino-let-behaviour`     | `compile_error: missing ; before statement (rhino-let-behaviour#10)`                 | parse: `missing ; before statement` |
| `for-of-var`              | `compile_error: missing ; after for-loop initializer (for-of-var#34)`                | same                                |
| `object-shorthand`        | `compile_error: missing : after property id (object-shorthand#14)`                   | same                                |
| `destructuring-object`    | `compile_error: missing : after property id (destructuring-object#13)`               | parse error                         |
| `default-params`          | `compile_error: missing ) after formal parameters (default-params#11)`               | parse error                         |
| `const-in-for-init`       | `compile_error: syntax error (const-in-for-init#15)`                                 | parse error                         |
| `const-in-for-in`         | `compile_error: syntax error (const-in-for-in#14)`                                   | parse error                         |
| `const-in-for-of`         | `compile_error: syntax error (const-in-for-of#14)`                                   | parse error                         |
| `const-dup-across-blocks` | `compile_error: TypeError: redeclaration of const dup. (const-dup-across-blocks#21)` | parse error                         |

The `const-dup` wording differs (`TypeError: redeclaration…` locally vs an
unspecified parse error in the matrix). Both sides reject the source. Line
numbers are of the copied file (a two-line corpus header sits above the original
fixture).

Scripts that _parse_ and then hit the silent `const` bugs (`const-top-level`,
`const-in-loop-body`, …) are in the `isEmpty` list. Their expects are the live
values (`undefined` / `",,"` / `"0,0,0"`). Once `isEmpty` lands they become
language-fidelity cases on the bindings runner, not just the language corpus.

## What this does _not_ claim

- It does not claim the 384-script production corpus was run. That tree is not
  here.
- It does not claim OAuth2 / ATM / may-act scripts were pointed at this harness.
  They are a different binding surface.
- A skipped case is not coverage. Zero of 52 produce a verdict today.
