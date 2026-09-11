# rhino-local gaps — real scripts vs the local overlay

What happens when the local harness is pointed at the scripted-decision scripts
this repo actually has, instead of the three toy cases.

Cases live under `scripts/rhino-local/ts/cases/real/`. Overlay:
`scripts/rhino-local/ts/src/bindings/rhino/runtime.cjs`. Measured 2026-09-12
against the JVM runner (`VERSION_DEFAULT` + `ScriptContextScope`) after
`callbacks.isEmpty`, next-gen `require()`, and legacy `Action.send`.

## What runs today

52 cases. 9 are language parse errors (blocked on `(parse)` with the JVM's exact
message). Of the 43 that execute:

| Count | Status                                             |
| ----- | -------------------------------------------------- |
| 23    | pass the live expect                               |
| 20    | run and fail the live expect (fidelity gaps below) |
| 0     | still blocked on an unimplemented method           |

The 20 failures are intentional. The expects encode live AIC behaviour; do not
tune them to match the overlay.

`string-normalize` was the one case whose outcome was `error` after `isEmpty`
landed. Cause: the last probe `require()`s `rhino-lib-normalize-probe`, and
`require` was undefined, so `results.every` was false and `emit` set
`outcome = "error"`. It passes now that `require` evals seeded library bodies.

## Ranked remaining gaps (measured)

Ranked by how many of the 20 a fix would turn green. Several cases fail for more
than one reason; they are listed under the dominant one.

### 1. Java class shutter / `java.util` — 4 cases

`java-collections`, `java-class-shutter`, `for-each-java-collection`,
`lib-java-collections-consumer`.

The local Rhino has no AM class shutter. It will construct classes AIC hides
(`HashMap`) and will not hide the ones AIC exposes. `require()` of the library
probe works; the payload then disagrees on which constructions succeed.

This is not a mock method. Faithful `require` does not need the shutter to
resolve a library; it needs it for the Java names the library then touches.

### 2. Identity object shape — 4 cases

Predicted this morning, now measured.

| Case                          | Live                                                                                                                           | Local                                                                                                              |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `identity-resolve-diag`       | `getIdentity(userName)` and `getIdentity("amadmin")` return a stub whose `getAttributeValues` throws `this.amIdentity is null` | `getIdentity` matches `_id` **or** `userName`, so `"alice"` resolves; `"amadmin"` throws `no given.managed record` |
| `identity-attr-mapping`       | `uid` count 1 (AM maps it)                                                                                                     | `uid` count 0 (no `uid` field on the seeded record, no mapping table)                                              |
| `identity-getattribute-shape` | Java collection: `.toArray()` works, `.includes()` throws, `String(v)` is `[{…}]`                                              | JS array with hidden `size`/`get`/`contains`: `.toArray()` throws, `.includes()` works                             |
| `identity-enum-attrs`         | `getAttributes` / `getAttributeNames` / `asMap` absent on next-gen wrapper                                                     | same absences (`typeof` is `"undefined"`); extra dump fields are not in the expect                                 |

`.includes()` working locally is the dangerous one: a script banned from
`.includes()` on AIC would pass the local harness.

### 3. Request map `getClass` + truncated expects — 3 cases

`request-multivalue`, `request-headers-dump`, `legacy-request-multivalue`.

Local request maps are JS objects. `getClass` throws
`Cannot find function getClass in object [object Object]`. Live recorded a Java
class name. The conversion also asserted only `{ok, feature}`, so extra dump
fields fail even on fields that match.

### 4. Mock enumerability vs Java `for-in` — 2 cases

`enum-callbacks-utils`, `enum-utils-sub`. Live `callbacksBuilder` / `utils.*`
are Java objects; locally they are JS objects whose methods are enumerable. The
name lists disagree. Mock-shape, not a missing method.

### 5. Legacy logger / `idRepository` members — 4 cases

`legacy-idrepository-methods`, `legacy-nodestate-logger`, `legacy-logger-args`,
`legacy-logger-levels`.

`Action.send` works. What remains is the **legacy** surface, which is not the
next-gen JSON:

- `idRepository.getAttribute` / `setAttribute` / `addAttribute` are functions on
  live legacy, `undefined` on the next-gen mock (`getIdentity` is present)
- `logger.message` / `warning` / `errorEnabled` / `messageEnabled` /
  `warningEnabled` are the classic Debug API; calling `errorEnabled()` throws
  `Cannot find function errorEnabled`

Do not add these to the next-gen mock just to turn the tests green. They are not
on `docs/api/bindings/scripted-decision-next.json`.

### 6. One-off

| Case                        | Why it fails                                                                                                                                                                       |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `logger-placeholders`       | extra `E0-shape` field (`caught=JavaException: java.lang.NumberFormatException…`); the `E0`/`E1`/… `ok` map otherwise matches                                                      |
| `httpclient-body-coercion`  | undeclared `http` effect (case has no `expect.http`) plus extra dump fields. The stub was hit; that is not a wire-fidelity match for JS `1` → `1.0`                                |
| `lib-openidm-miss-consumer` | missing **record** in a seeded collection returns `null` (AIC). Missing **type** (`managed/zzz_no_such_object_type/…`) throws `no given.managed entry` locally; AIC returns `null` |

`openidm.read` of an unseeded collection stays a missing-fixture throw on
purpose. Live does not distinguish that from a missing record.

---

## Where the real scripts are

| Source                                                                                | What it is                                                                | Verdict                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/rhino-script-tester/fixtures/*.script.js`                                    | Next-gen scripted-decision probes, run live through `AIC-Rhino-Let-Probe` | **This is the in-repo corpus.** Recorded outcomes live in `docs/api/12-script-bindings-matrix.md` and `docs/api/14-am-identity-attributes.md`. `tmp/rhino-script-tester/probe-results.json` is gitignored and was not in this checkout. |
| `scripts/rhino-script-tester/fixtures-legacy/*.script.js`                             | Legacy (`evaluatorVersion` 1.0) scripted-decision probes                  | Same kind, different engine. Converted with `given.engine = "legacy"`.                                                                                                                                                                  |
| `scripts/rhino-script-tester/scripts/*.script.js`                                     | Earlier `let` / `var` probes, same journey                                | Converted.                                                                                                                                                                                                                              |
| `scripts/rhino-script-tester/fixtures/*.lib.js`                                       | LIBRARY bodies, not decision-node scripts                                 | Not cases. Consumers `require()` them; library source is seeded next to the case (the case type has no `libraries` field).                                                                                                              |
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

A blocked case still asserts its exact throw, so implementing that method fails
the assertion and forces re-triage. Nothing in this corpus is currently blocked
on a binding method — only the nine parse errors.

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

The silent `const` bugs (`const-top-level`, `const-in-loop-body`, …) now pass as
language-fidelity cases on the bindings runner. Library top-level `const` also
passes (`lib-const-consumer`); loop-body `const` inside a library function still
yields `"0,0,0"` (`lib-const-loop-consumer`), matching live.

## What this does _not_ claim

- It does not claim the 384-script production corpus was run. That tree is not
  here.
- It does not claim OAuth2 / ATM / may-act scripts were pointed at this harness.
  They are a different binding surface.
- A skipped case is not coverage. Twenty of 52 still fail the live expect.
- `require()` is a CommonJS eval of seeded source, not AM's wrap factory. The
  library probes in this corpus do not need the shutter to load; they need it
  for the Java names they then construct (`lib-java-collections-consumer`).
