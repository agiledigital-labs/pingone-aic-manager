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
  restrictions) applies to **all** AM and IDM scripts.
- Legacy vs next-gen splits only the **bindings** overlay, not the syntax rules.
- Naming the type file `rhino-1.7.14.d.ts` per product is correct; there is no
  separate "next-gen engine" type file needed for syntax.

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

| Feature                                                                                                                                 | Status | Result                                                                                                                                                                                                                                                                                                                 | Lint action                                         |
| --------------------------------------------------------------------------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `var`                                                                                                                                   | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                               |
| `const` in a function body                                                                                                              | **V**  | ✅ works, correct value                                                                                                                                                                                                                                                                                                | **allow**                                           |
| same `const` name twice in one function (separate, non-nested blocks)                                                                   | **V**  | ❌ parse error (Rhino scopes `const` to the function for redeclaration)                                                                                                                                                                                                                                                | **ban (all AM) — custom `rhino/no-dup-const` rule** |
| `const` at top level (decision-node script)                                                                                             | **V**  | ⚠️ parses but value reads back `undefined` — silent data bug                                                                                                                                                                                                                                                           | **ban** (all AM)                                    |
| `const` at top level of a `LIBRARY` script                                                                                              | **V**  | ✅ works, correct value (probed 2026-07-13, `lib-const-probe.lib.js` — a library's top level is function-like scope, so the decision-node bug does not apply)                                                                                                                                                          | allow (ban kept in lint for uniformity is fine)     |
| `const` in a `for`/`for-in`/`for-of`/`while`/`do-while` loop body, including nested blocks such as `if` inside the loop                 | **V**  | ⚠️ parses but value reads back `undefined` — silent data bug (`value: ",,"` for nested-block/while/do-while probes)                                                                                                                                                                                                    | **ban**                                             |
| `const` in a loop body INSIDE a function (decision-node and `LIBRARY` contexts)                                                         | **V**  | ⚠️ parses but the initializer runs only on the FIRST iteration; later iterations silently keep the first value (`"0,0,0"` where correct is `"0,2,4"`; probed 2026-07-13, both contexts)                                                                                                                                | **ban** (scope does not rescue loop-body `const`)   |
| `const` in `for` init                                                                                                                   | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                 |
| `const` in `for-in`                                                                                                                     | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                 |
| `const` in `for-of`                                                                                                                     | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                 |
| `let` (any scope)                                                                                                                       | **V**  | ❌ parse error (`missing ; before statement`)                                                                                                                                                                                                                                                                          | ban (all AM)                                        |
| object shorthand `{a, b}`                                                                                                               | **V**  | ❌ parse error (`missing : after property id`)                                                                                                                                                                                                                                                                         | ban                                                 |
| object destructuring `var {x} = o`                                                                                                      | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | ban                                                 |
| default parameters `f(a, b = 2)`                                                                                                        | **V**  | ❌ parse error                                                                                                                                                                                                                                                                                                         | **ban (NEW — not in current config)**               |
| arrow functions `=>`                                                                                                                    | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                               |
| template literals                                                                                                                       | **V**  | ✅ works                                                                                                                                                                                                                                                                                                               | allow                                               |
| ES2015 methods: `Array` `includes`/`find`/`from`/`fill`, `String` `includes`/`startsWith`/`endsWith`/`repeat`, `Object` `assign`/`keys` | **V**  | ✅ all work. `Array.prototype.fill` + `Array.from({length}, () => …)` also verified in **`LIBRARY`** context (probed 2026-07-17, `fixtures/lib-array-fill-probe.lib.js` + `lib-array-fill-consumer.script.js`: `new Array(3).fill(false)` and `Array.from({length:3}, () => false)` both returned `false,false,false`) | allow                                               |
| `String.prototype.normalize` (`NFD`/`NFC`) + combining-mark regex strip (`/[̀-ͯ]/`)                                                       | **V**  | ✅ works — decision-node AND `LIBRARY` context (probed 2026-07-02; handles stacked marks, `Nguyễn` → `Nguyen`)                                                                                                                                                                                                         | allow                                               |

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

## AM binding matrix

`evaluatorVersion`: `2.0` = next-gen, `1.0` = legacy. "Folder slug" is the
workspace routing slug from `src/aic/script/am.rs::slug_for`.

| Binding                                  | Legacy                                                     | Next-gen                             | Status    | Shape / notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------------------- | ---------------------------------------------------------- | ------------------------------------ | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `logger`                                 | yes                                                        | yes                                  | **D/I**   | Legacy: `error/message/warning(+Enabled)`. Next-gen: slf4j-style `trace/debug/info/warn/error`. **Shapes differ** — must split legacy vs next-gen.                                                                                                                                                                                                                                                                                                                                   |
| `httpClient`                             | partial                                                    | yes                                  | **D/I**   | Next-gen: `httpClient.send(url, opts).get()` returning fetch-like `{status,ok,json(),text()}`. Legacy: Java `Request`/`Response`. Used in 25 `src/`.                                                                                                                                                                                                                                                                                                                                 |
| `openidm`                                | no                                                         | yes                                  | **D/I**   | CRUDPAQ. Next-gen scripted decision + IDM only. Used in 63 `src/`. Not a legacy decision-node binding. **LIBRARY confirmed** (2026-07-14): `openidm.read(...)` called from inside a function in a `require()`d LIBRARY script (not just the top-level decision-node script) works and returns the real record — `fixtures/lib-openidm-read-probe.lib.js` + `lib-openidm-read-consumer.script.js`, read `managed/alpha_name_variant/aaron_erin` and got the seeded row back verbatim. |
| `utils`                                  | no                                                         | yes                                  | **D/I**   | base64/UUID/random. Used in 86 `src/`. Next-gen only.                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `nodeState`                              | yes                                                        | yes                                  | **D/I**   | Next-gen returns coerced JS types; legacy returns `JsonValue` needing `.asString()`. Used in 250 `src/`.                                                                                                                                                                                                                                                                                                                                                                             |
| `sharedState`/`transientState`           | yes                                                        | **deprecated/removed**               | **D/I**   | Replaced by `nodeState.putShared/putTransient` in next-gen. Still in 34/4 `src/` (legacy).                                                                                                                                                                                                                                                                                                                                                                                           |
| `action`                                 | via `Action` class                                         | binding                              | **D/I**   | Next-gen: `action.goTo(...)` chainable `ActionWrapper`. Legacy: static `Action.goTo()` (`ActionBuilder`). Used in 106 `src/`.                                                                                                                                                                                                                                                                                                                                                        |
| `callbacks` / `callbacksBuilder`         | `callbacks`                                                | `callbacksBuilder`                   | **D/I**   | Legacy reads `callbacks`; next-gen builds via `callbacksBuilder.*`. Both names appear (~142 each).                                                                                                                                                                                                                                                                                                                                                                                   |
| `idRepository`                           | direct attr methods + `getIdentity()` → `ScriptedIdentity` | `getIdentity()` → `ScriptedIdentity` | **D/V/I** | `getIdentity()` exists on both engines. Legacy also has direct `getAttribute(user,attr)`. Used in 40 `src/`.                                                                                                                                                                                                                                                                                                                                                                         |
| `requestHeaders`/`requestParameters`     | yes                                                        | yes                                  | **D/I**   | `Map<String,String[]>`-ish `.get()`.                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `requestCookies`                         | no                                                         | yes                                  | **D**     | Next-gen only (migrate doc).                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `existingSession` / `resumedFromSuspend` | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `cacheManager`                           | no                                                         | yes                                  | **D**     | Next-gen scripted decision only.                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `oauthApplication` / `samlApplication`   | no                                                         | yes                                  | **D**     | Next-gen only, journey-associated.                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `auditEntryDetail`                       | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `realm` / `systemEnv` / `scriptName`     | yes                                                        | yes                                  | **D**     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `require()` / library scripts            | **no**                                                     | **yes**                              | **D/V/I** | Only next-gen can `require` libraries. Libraries can require other libraries (17 `lib/` files do). Used in 225 `src/`. **Verified beyond scripted decision (2026-07-29):** works end-to-end from a next-gen **`OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN`** script; `typeof require === "undefined"` in the legacy (`1.0`) token-mod script (`ReferenceError: "require" is not defined.`). See the token-mod section below.                                                          |
| `JavaImporter` / Java allowlist          | yes                                                        | **no**                               | **D/I**   | Legacy only. 26 `src/` use `JavaImporter`. Next-gen has no configurable Java access.                                                                                                                                                                                                                                                                                                                                                                                                 |

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

### AM script families (folder slugs)

| Family                                  | Slug                   | `evaluatorVersion` | Library support                               | Bindings overlay                                    |
| --------------------------------------- | ---------------------- | ------------------ | --------------------------------------------- | --------------------------------------------------- |
| Scripted decision (next-gen)            | `decision-node`        | `2.0`              | yes                                           | decision-node-base + next-gen                       |
| Scripted decision (legacy)              | `decision-node-legacy` | `1.0`              | no                                            | decision-node-base + legacy                         |
| Library                                 | `lib`                  | (next-gen)         | yes (CommonJS)                                | library                                             |
| OIDC claims                             | `oidc-claims`          | mixed              | next-gen only                                 | oidc-claims                                         |
| OAuth2 (token mod, scope, jwt, dcr, …)  | `oauth2-*`             | mixed              | next-gen only (token mod verified 2026-07-29) | `oauth2-dcr`, `oauth2-access-token-ng`; rest future |
| SAML2 (idp/sp adapter, mappers)         | `saml-*`               | mixed              | next-gen only                                 | per-context (future)                                |
| Social normalization/handler            | `social-*`             | mixed              | —                                             | per-context (future)                                |
| Config provider / device match / policy | various                | mixed              | —                                             | shared globals only (today)                         |

## IDM binding matrix

IDM scripts are tenant-global (no realm). The IDM **endpoint**
`request`/`context` shapes are runtime-verified (2026-06-04 — see
`docs/api/11`). The endpoint `request` is a discriminated union on `method`;
`context.http` carries the HTTP request.

| Binding          | Endpoint | Schedule | Status  | Notes                                                                                                                                                                                     |
| ---------------- | -------- | -------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `openidm`        | yes      | yes      | **I**   | CRUDPAQ + `update`. From `idmCommon.d.ts`.                                                                                                                                                |
| `logger`         | yes      | yes      | **I**   | slf4j-style.                                                                                                                                                                              |
| `identityServer` | yes      | yes      | **D/V** | `getProperty(name, defaultValue?, substitute?)`; a missing ESV/property returns `null` when no default is supplied, or the supplied default. Endpoint behavior verified 2026-07-22 below. |
| `request`        | yes      | no       | **V**   | Discriminated union per CREST method (read/create/update/patch/delete/action/query). `docs/api/11`.                                                                                       |
| `context`        | yes      | no       | **V**   | `context.http` = {method,path,headers,parameters}; `context.security` = {authenticationId,authorization}.                                                                                 |

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
   `{}` placeholder formatting; `httpClient.send(...).get()` response shape;
   `openidm` call return shapes. Presence is verified; exact shapes still lean
   on docs.
2. ~~`require()` of a real library from a next-gen scripted decision~~ RESOLVED
   — end-to-end library imports are verified from a next-gen scripted decision
   (2026-07-13/14 rows above) and from next-gen access-token modification
   (2026-07-29 section above). Still unprobed: `require()` from the other
   `…_NEXT_GEN` AM contexts (OIDC claims, scope validate/evaluate, JWT issuer,
   DCR, SAML mappers/adapters).
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
