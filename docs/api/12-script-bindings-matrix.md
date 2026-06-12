# Script Bindings & Runtime Feature Matrix

Feature matrix backing the script-linting/type-update work
(`script-linting-uplift-plan.md`). It records, per AM/IDM script family, which
language features and bindings are available, **and how we know** — so the
TypeScript declarations and ESLint rules can be grounded in fact rather than
copied assumptions.

> **Status legend (provenance of each claim):**
>
> - **D** — Documented by Ping (URL cited).
> - **V** — Runtime-verified in the sandbox via `scripts/rhino-script-tester/`.
> - **I** — Inferred from the existing sandbox corpus (`<client-checkout>/sandbox-scripts`)
>   — real scripts use it, so it must work, but not isolated-probe confirmed.
> - **U** — Unknown / not yet verified. **Do not** encode as fact in types or lint.

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

## Key conclusion: engine generation is a *bindings* axis, not a *syntax* axis

Both legacy and next-generation AM scripts run on **Mozilla Rhino 1.7.14** with
"limited ES6 / ES2015 support" (scripting-env doc, **D**). The runtime probe
confirms next-gen still **rejects `let`** (`missing ; before statement`, **V**).
So "next-generation" does **not** mean a newer JS engine — it means a different
*binding set* (simplified `logger`, fetch-like `httpClient`, `openidm`, `utils`,
`require()`/library support, `action`/`callbacksBuilder`/`nodeState` instead of
`Action`/`callbacks`/`sharedState`).

Practical consequence for this work:

- **One** Rhino syntax layer (`rhino-1.7.14.d.ts` + the shared ESLint syntax
  restrictions) applies to **all** AM and IDM scripts.
- Legacy vs next-gen splits only the **bindings** overlay, not the syntax rules.
- Naming the type file `rhino-1.7.14.d.ts` per product is correct; there is no
  separate "next-gen engine" type file needed for syntax.

## Language / syntax feature matrix (Rhino 1.7.14)

Applies to every script family. **All rows runtime-verified 2026-06-03** (the
duplicate-`const`-per-function row added 2026-06-06) via the
next-gen scripted decision probe (`scripts/rhino-script-tester/fixtures/`,
results in `tmp/rhino-script-tester/probe-results.json`). Probe semantics:
a fixture that PARSES + RUNS returns a `HiddenValueCallback` (`HTTP 200`); a
fixture that fails to PARSE returns no callback and the journey fails
(`HTTP 401`, confirmed via logs, e.g. object shorthand →
`org.mozilla.javascript.EvaluatorException: missing : after property id`).

| Feature | Status | Result | Lint action |
| --- | --- | --- | --- |
| `var` | **V** | ✅ works | allow |
| `const` in a function body | **V** | ✅ works, correct value | **allow** |
| same `const` name twice in one function (separate, non-nested blocks) | **V** | ❌ parse error (Rhino scopes `const` to the function for redeclaration) | **ban (all AM) — custom `rhino/no-dup-const` rule** |
| `const` at top level | **V** | ⚠️ parses but value reads back `undefined` — silent data bug | **ban** (all AM) |
| `const` in a loop body | **V** | ⚠️ parses but value reads back `undefined` — silent data bug | **ban** |
| `const` in `for` init | **V** | ❌ parse error | ban |
| `const` in `for-in` | **V** | ❌ parse error | ban |
| `const` in `for-of` | **V** | ❌ parse error | ban |
| `let` (any scope) | **V** | ❌ parse error (`missing ; before statement`) | ban (all AM) |
| object shorthand `{a, b}` | **V** | ❌ parse error (`missing : after property id`) | ban |
| object destructuring `var {x} = o` | **V** | ❌ parse error | ban |
| default parameters `f(a, b = 2)` | **V** | ❌ parse error | **ban (NEW — not in current config)** |
| arrow functions `=>` | **V** | ✅ works | allow |
| template literals | **V** | ✅ works | allow |
| ES2015 methods: `Array` `includes`/`find`/`from`, `String` `includes`/`startsWith`/`endsWith`/`repeat`, `Object` `assign`/`keys` | **V** | ✅ all work | allow |

> **Key takeaways for ESLint:** the existing AM bans on `let`, object shorthand,
> object destructuring, and `const` in loops are all now runtime-justified.
> `const` *inside functions* must stay **allowed** (it works and is idiomatic).
> The top-level/loop-body `const` bans should apply to **all** AM scripted
> decision scripts, not just the old `src` glob, because the failure is a silent
> `undefined` (worse than a parse error). **Add a default-parameters ban** — it
> is a parse error and the current config misses it. Array destructuring was not
> probed separately but object destructuring fails, so treat both as banned.

## AM binding matrix

`evaluatorVersion`: `2.0` = next-gen, `1.0` = legacy. "Folder slug" is the
workspace routing slug from `src/aic/script/am.rs::slug_for`.

| Binding | Legacy | Next-gen | Status | Shape / notes |
| --- | --- | --- | --- | --- |
| `logger` | yes | yes | **D/I** | Legacy: `error/message/warning(+Enabled)`. Next-gen: slf4j-style `trace/debug/info/warn/error`. **Shapes differ** — must split legacy vs next-gen. |
| `httpClient` | partial | yes | **D/I** | Next-gen: `httpClient.send(url, opts).get()` returning fetch-like `{status,ok,json(),text()}`. Legacy: Java `Request`/`Response`. Used in 25 `src/`. |
| `openidm` | no | yes | **D/I** | CRUDPAQ. Next-gen scripted decision + IDM only. Used in 63 `src/`. Not a legacy decision-node binding. |
| `utils` | no | yes | **D/I** | base64/UUID/random. Used in 86 `src/`. Next-gen only. |
| `nodeState` | yes | yes | **D/I** | Next-gen returns coerced JS types; legacy returns `JsonValue` needing `.asString()`. Used in 250 `src/`. |
| `sharedState`/`transientState` | yes | **deprecated/removed** | **D/I** | Replaced by `nodeState.putShared/putTransient` in next-gen. Still in 34/4 `src/` (legacy). |
| `action` | via `Action` class | binding | **D/I** | Next-gen: `action.goTo(...)` chainable `ActionWrapper`. Legacy: static `Action.goTo()` (`ActionBuilder`). Used in 106 `src/`. |
| `callbacks` / `callbacksBuilder` | `callbacks` | `callbacksBuilder` | **D/I** | Legacy reads `callbacks`; next-gen builds via `callbacksBuilder.*`. Both names appear (~142 each). |
| `idRepository` | direct attr methods | `getIdentity()` → `ScriptedIdentity` | **D/I** | Next-gen: `getIdentity().getAttributeValues()/store()`. Legacy: `getAttribute(user,attr)`. Used in 40 `src/`. |
| `requestHeaders`/`requestParameters` | yes | yes | **D/I** | `Map<String,String[]>`-ish `.get()`. |
| `requestCookies` | no | yes | **D** | Next-gen only (migrate doc). |
| `existingSession` / `resumedFromSuspend` | yes | yes | **D** | |
| `cacheManager` | no | yes | **D** | Next-gen scripted decision only. |
| `oauthApplication` / `samlApplication` | no | yes | **D** | Next-gen only, journey-associated. |
| `auditEntryDetail` | yes | yes | **D** | |
| `realm` / `systemEnv` / `scriptName` | yes | yes | **D** | |
| `require()` / library scripts | **no** | **yes** | **D/I** | Only next-gen can `require` libraries. Libraries can require other libraries (17 `lib/` files do). Used in 225 `src/`. |
| `JavaImporter` / Java allowlist | yes | **no** | **D/I** | Legacy only. 26 `src/` use `JavaImporter`. Next-gen has no configurable Java access. |

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
- `sharedState`/`transientState` are **gone in next-gen** → keep them only in the
  legacy decision-node overlay; the next-gen overlay must not declare them.
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
- `openidm` + `utils` are **next-gen-only** → moved from the shared `common.d.ts`
  to a new `nextgen-common.d.ts` included only by next-gen decision + library
  leaves, so the legacy leaf no longer mistypes them as present.
- `secrets` + `resumedFromSuspend` are in **both** → moved into
  `decision-node-base`.
- `nodeState` is present on **both** engines (legacy returns `JsonValue`-style
  values needing `.asString()`; next-gen returns coerced JS — a shape difference,
  not a presence one).

### Method surfaces verified 2026-06-04 (legacy engine)

`typeof` of each member on a legacy scripted decision node
(`fixtures-legacy/legacy-nodestate-logger.script.js`):

- **`nodeState`** (legacy): `get`, `getObject`, `putShared`, `putTransient`,
  `mergeShared`, `mergeTransient` — plus **`isDefined` and `remove`**
  (function; *not in the docs*). There is **no** `nodeState.sharedState(key)` /
  `transientState(key)` / `secureState(key)` accessor — `get()` is the unified
  accessor (reads transient→secure→shared). Legacy shared/transient state is
  reached through the **standalone** `sharedState`/`transientState` bindings
  (e.g. `sharedState.get('k')`), which next-gen removes.
- **`logger`** (legacy): `error`, `message`, `warning`, `errorEnabled`,
  `messageEnabled`, `warningEnabled` — the classic `Debug` object.
  `trace`/`debug`/`info`/`warn` are **absent** (those are the next-gen slf4j
  shape). Now runtime-confirmed, not just doc-claimed.

Type model: the slf4j `logger` lives in `nextgen-common.d.ts`; the classic
`Debug` `logger` in `legacy-common.d.ts` (included by the legacy decision leaf
and the other unmigrated AM contexts). `nodeState.isDefined`/`remove` are merged
onto `NodeState` only on the legacy leaf (via `decision-node-legacy.d.ts`).

### AM script families (folder slugs)

| Family | Slug | `evaluatorVersion` | Library support | Bindings overlay |
| --- | --- | --- | --- | --- |
| Scripted decision (next-gen) | `decision-node` | `2.0` | yes | decision-node-base + next-gen |
| Scripted decision (legacy) | `decision-node-legacy` | `1.0` | no | decision-node-base + legacy |
| Library | `lib` | (next-gen) | yes (CommonJS) | library |
| OIDC claims | `oidc-claims` | mixed | next-gen only | oidc-claims |
| OAuth2 (token mod, scope, jwt, dcr, …) | `oauth2-*` | mixed | next-gen only | per-context (future) |
| SAML2 (idp/sp adapter, mappers) | `saml-*` | mixed | next-gen only | per-context (future) |
| Social normalization/handler | `social-*` | mixed | — | per-context (future) |
| Config provider / device match / policy | various | mixed | — | shared globals only (today) |

## IDM binding matrix

IDM scripts are tenant-global (no realm). The IDM **endpoint** `request`/`context`
shapes are runtime-verified (2026-06-04 — see `docs/api/11`). The endpoint
`request` is a discriminated union on `method`; `context.http` carries the HTTP
request.

| Binding | Endpoint | Schedule | Status | Notes |
| --- | --- | --- | --- | --- |
| `openidm` | yes | yes | **I** | CRUDPAQ + `update`. From `idmCommon.d.ts`. |
| `logger` | yes | yes | **I** | slf4j-style. |
| `request` | yes | no | **V** | Discriminated union per CREST method (read/create/update/patch/delete/action/query). `docs/api/11`. |
| `context` | yes | no | **V** | `context.http` = {method,path,headers,parameters}; `context.security` = {authenticationId,authorization}. |

### IDM endpoint engine — syntax (verified 2026-06-04)

The IDM script engine is **newer than AM's Rhino 1.7.14** — a source that fails
to compile makes the endpoint un-routable (404 on invoke; reverting to `var`
restores 200), which is how each row below was checked:

| Feature | IDM endpoint | AM Rhino 1.7.14 (contrast) |
| --- | --- | --- |
| `var` | ✅ | ✅ |
| `let` (any scope) | ✅ works | ❌ parse error |
| `const` top-level / in-function | ✅ works | ⚠️ top-level reads undefined |
| arrow fns, template literals | ✅ | ✅ |
| object shorthand, destructuring | ✅ works | ❌ parse error |
| default parameters | ❌ compile-fails | ❌ parse error |
| `const` in `for`/`for-of` initializer | ❌ compile-fails | ❌ parse error |

So IDM ESLint bans only **default parameters** and **`const` in a loop
initializer** — NOT `let`/`const`/shorthand/destructuring (those work). Schedule
scripts are not yet probed (cron-triggered; harder to invoke synchronously).

## Open items still requiring runtime probes

Resolved 2026-06-03 (next-gen scripted decision): all syntax rows above;
binding *presence* (typeof). Resolved 2026-06-04: legacy (`evaluatorVersion:
1.0`) binding *presence* (see the legacy section above; the tester now takes
`EVALUATOR_VERSION`). Still open:

1. Binding *shapes* (not just presence): `logger` method names + slf4j-style `{}`
   placeholder formatting; `httpClient.send(...).get()` response shape; `openidm`
   call return shapes. Presence is verified; exact shapes still lean on docs.
2. `require()` of a real library from a next-gen scripted decision (presence of
   the `require` function is verified; an end-to-end library import is not).
3. IDM **schedule** scripts: bindings + syntax (the endpoint side is now
   verified — see the IDM section above). Schedules are cron-triggered, so they
   need a trigger mechanism to probe synchronously.
4. **Exact `callbacksBuilder` / `utils` argument sets.** RESOLVED (2026-06-04).
   The tenant owner extracted the script editor's authoritative binding metadata
   — saved at `docs/api/bindings/scripted-decision-next.json` (the
   `SCRIPTED_DECISION_NODE` next-gen binding surface: 25 bindings, every method
   with named params + types + overloads + the Java allow-list). All of it is
   now typed from that source: `callbacksBuilder` (every overload), `utils`
   (base64/base64url/crypto incl. full `subtle`, types), `action` (full
   chainable surface), `openidm` (CRUDPAQ + overloads), `nodeState`
   (incl. `isDefined`/`remove`/`keys`), `callbacks` getters, enriched `secrets`,
   and previously-missing bindings: `samlApplication`, `oauthApplication`,
   `jwtAssertion`, `jwtValidator`, `policy`, `journey`, `cacheManager`,
   `cookieName`. (Corrected guesses: `suspendedTextOutputCallback(messageType,
   message)`; `utils.crypto` has no `randomValues`.) The metadata only covers
   `evaluatorVersion 2.0`; the equivalent legacy dump would let us tighten the
   legacy leaf the same way.

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
   script-bindings doc example), but how it's *delivered* in next-gen is unknown
   (the header is ignored; `token` is Bearer-only). Investigate, then type it.
