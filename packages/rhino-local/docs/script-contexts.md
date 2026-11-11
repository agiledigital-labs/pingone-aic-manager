# Script contexts

Status: design note, 2026-11-11. Implemented: the selector and, each on both
engines, `OAUTH2_ACCESS_TOKEN_MODIFICATION`, `OIDC_CLAIMS`,
`SOCIAL_IDP_PROFILE_TRANSFORMATION`, `CONFIG_PROVIDER_NODE`,
`OAUTH2_VALIDATE_SCOPE` and `OAUTH2_MAY_ACT`. `LIBRARY` is implemented on
next-gen only: legacy has no `require()`.

## The problem

Until now the tester ran one kind of AM script: the scripted decision node.
Everything else (an access-token modification script, OIDC claims, a social
profile transform, a config provider node, validate scope, may-act, a library
called directly) failed on its first context binding with a missing-binding
`ReferenceError`, because the only binding surface the harness installs is the
decision node's. The case API had no way to say which context a script runs in.

## The selector

A case names its context with one top-level key:

```ts
defineCase({
  name: "stamps the tenant claim",
  context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
  script,
  given: { engine: "next-gen", accessToken: { … }, identity: { … }, client: { … } },
  expect: { accessToken: { fields: { added: { tenant: "a" } } } },
});
```

- **Absent means the decision node.** `context` is optional and its default is
  `"SCRIPTED_DECISION_NODE"`, so every existing case, suite and lease runs
  exactly as before: same prelude, same preamble, same harvest, same verdict.
  Naming the default explicitly is allowed and changes nothing.
- **The value is AM's own context id** without the `_NEXT_GEN` suffix, because
  the engine is already a separate axis (`given.engine`, default `"next-gen"`).
  `OAUTH2_ACCESS_TOKEN_MODIFICATION` + `engine: "legacy"` is AM's
  `OAUTH2_ACCESS_TOKEN_MODIFICATION` (evaluatorVersion 1.0);
  - `engine: "next-gen"` is `OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN` (2.0). A
    context that exists on one engine only refuses the other at validation.
- **`given` and `expect` are validated per context.** Each context declares the
  `given` keys and `expect` keys it accepts; any other key is a validation error
  naming the context. A decision-node key such as `sharedState` on a token case
  is a typo-class mistake (the script cannot see it), so it must not be silently
  accepted.
- **`expect.outcome` belongs to the decision node.** A context that has no
  outcome refuses the key. Internally the case is judged with outcome `null`,
  which the script cannot produce because the outcome bindings are not
  installed.

### Why not a separate case type and runner

The brief's alternative was a parallel `ContextCase` with its own
`runContextCase`. That would duplicate validation of the shared fixtures (`esv`,
`http`, `managed`, `libraries`, `openidmFailures`), the judging of the shared
channels (`logs`, `http`, `openidm`) and the fault/tally plumbing. One `Case`
with a discriminating key keeps those single-sourced, and `runCase`, `judge` and
the pool stay the entry points. The cost is that `Given` and `Expect` carry
fields only some contexts accept, which validation enforces at runtime and the
per-context `*CaseInit` types enforce at compile time.

### What a context declares

A context is one entry in `src/case/context.ts`, plus one runtime overlay
file. It declares four things:

1. **Bindings.** Which globals exist, which are refused, and per engine. The
   overlay (`src/bindings/rhino/context-<slug>.cjs`) is appended to the shared
   prelude after `collection-catalog.cjs` and `context-common.cjs`, whose `__rhinoLocalCtx*` helpers (Java
   strings, sets, maps, lists, method arity, the `Object.prototype.toString`
   tag, the JSON value check and the harvest's mutation audit) every context
   builds its views from, with its own fault prefix and list kind. Fix a view
   defect there, once, not in a context overlay. The overlay deletes the decision-node bindings the context does not have (so
   a stray `nodeState.get()` is the same `ReferenceError` AIC throws), and
   installs the context's own. Shared bindings (`logger`, `httpClient`,
   `openidm`, `systemEnv`, `secrets`, `utils`, `require`) are the decision-node
   implementations, kept only where the context was measured to bind them. Every
   member the measurement did not cover is a named, tallied harness fault
   (`not-mocked/<binding>.<method>` or a context-specific id), never a guess.
2. **Typed fixtures.** The `given` keys the context adds (for the token context:
   `accessToken`, `identity`, `client`, `scopes`, `request`), on top of the
   shared environment keys it accepts.
3. **Invocation.** How AM runs the script and what it does with the result:
   top-level body (token modification, claims, config provider), a named
   function AM calls (validate scope), or an export a caller invokes (library).
   The overlay's harvest reports the context's result as a JSON effect beside
   the shared channels.
4. **Assertions.** The `expect` keys the context adds (for the token context:
   `accessToken`), judged fail-closed like the state channels: a change the case
   did not declare is a mismatch unless `allowUndeclared` loosens it.

The class shutter follows the context: next-gen contexts install their own
`allowLists` from `docs/api/bindings/<context>.json`, plus the same
`OBSERVED_BASE` boxed types the decision node gets. Legacy installs no
allow-list, as for the decision node, and denies script-visible
`java.lang.Class` (docs/api/12, 2026-11-10). An overload message names a Java
argument from Java (`Wrapper.unwrap().getClass().getName()`), so that deny is
not consulted while the message is built.

## The seven contexts

| Context (`context` + engine)                                          | Bindings beyond the shared set                                                                                                | Fixtures                                                                  | Invocation, and what AM does with it                                                                                             | Assertions                                                                               |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `OAUTH2_ACCESS_TOKEN_MODIFICATION` legacy + next-gen (**implemented**) | `accessToken`, `identity`, `scopes`, `requestProperties`, `clientProperties`; legacy `session` (null)                         | `accessToken` (starting state), `identity`, `client`, `scopes`, `request` | Top-level body; return value ignored; AM issues the mutated token                                                                | `expect.accessToken`: fields added/changed/removed, scope, expiry, and the other setters |
| `OIDC_CLAIMS` legacy + next-gen (**implemented**) | `claims`, `requestedClaims`, `claimsLocales`, `identity`, `scopes`, `requestProperties`, `clientProperties`; legacy `claimObjects`, `requestedTypedClaims`, `session` (null) | `identity`, `scopes`, `client`, `request`, `oidc` (claims seed, `claimsLocales`) | Completion value: next-gen `{values, compositeScopes}`, legacy a `UserInfoClaims`; read through the runner's completion hook | `expect.oidc`: `userinfo`, `idToken`, `compositeScopes`, `issued` |
| `SOCIAL_IDP_PROFILE_TRANSFORMATION` legacy + next-gen (**implemented**) | `rawProfile` (next-gen a Java map, legacy a JsonValue), `selectedIdp`, `nodeState`; legacy `sharedState`, `transientState`, `callbacks` | `socialIdp` (`rawProfile`, `selectedIdp`), the node state | Completion value: a plain object, or on legacy `new JsonValue(map)`; undefined, null or a string fails the journey | `expect.socialIdp`: `profile`, `accepted`; the state channels |
| `CONFIG_PROVIDER_NODE` legacy + next-gen (**implemented**) | `nodeState`; next-gen the decision node's bindings less `sharedState`, `transientState` and `outcome`; legacy only `nodeState`, `idRepository`, the request maps, `realm`, `scriptName`, `logger`, `httpClient`, `secrets`, `systemEnv` | the node state, request maps | Top-level body that assigns the `config` global (read after the script, not its completion value); AM configures the wrapped node from it, and fails the journey (HTTP 500) when it is not a map | `expect.configProvider`: `config`, `accepted`; the state channels |
| `OAUTH2_VALIDATE_SCOPE` legacy + next-gen (**implemented**) | next-gen `scopeValidatorHelper`, `requestedScopes`, `availableScopes`, `allowedScopes`, `defaultScopes` (Java lists), `identity`, `requestProperties`, `clientProperties`; legacy only `requestedScopes`, `allowedScopes`, `defaultScopes` (Java sets), `logger`, `realm`, `scriptName`, `systemEnv` | `validateScope` (requested / allowed / available / default scopes), `client`, `request` | Named function `validateAccessTokenScope` AM calls after the body; AM grants exactly the list it returns (no intersection); next-gen `throwInvalidScope` / legacy `InvalidScopeException` is the 400 denial | `expect.validateScope`: `granted`, `invalidScope`, `accepted` |
| `OAUTH2_MAY_ACT` legacy + next-gen (**implemented**)                    | next-gen `token` (five methods: `getField`, `getAct`, `getMayAct`, `setAct`, `setMayAct`), `scopes` (a Java list), `identity`, `requestProperties`, `clientProperties`; legacy `token` (a `StatelessAccessToken`; `setMayAct` takes a JsonValue), `scopes` (a Java set), `logger`, `realm`, `scriptName`, `systemEnv`, `session` (`typeof` `object`) | `client`, `scopes`, `request`, `identity`, `accessToken`                                | Top-level body; AM stamps `may_act` and `act` after the script; a thrown script is HTTP 500 and discards prior sets                                                                                         | `expect.mayAct`: `mayAct`, `act`, `scope`, `issued`, `refusal`                           |
| `LIBRARY` next-gen only (**implemented**) | library scope (`require`, `logger`, `openidm`, `httpClient`, `secrets`, `systemEnv`, `utils`, …); the caller's bindings are absent; `scriptName` and `logger.getName()` are the caller's | `library` (`export`, `args`, `callerContext`); `realm`, `scriptName`, `loggerScriptId`, `cookieName` | The case script is the library body. An empty caller `require()`s it and calls the named export as a method. A throw, including one while the body loads, is the result | `expect.returns` (`returned` / `thrown`, fail-closed), plus shared channels |

Shared bindings per context come from the binding descriptors in
`docs/api/bindings/` for presence and from live probes for behaviour; a
descriptor alone never makes a member callable.

## The access-token modification context in detail

See the README section "Access-token modification scripts" for the author's
view; the measured surface is in `docs/api/12-script-bindings-matrix.md` and the
gaps in `docs/rhino-local-gaps.md`.

## The social IdP profile transformation context in detail

See the README section "Social IdP profile transformation scripts" for the
author's view. The result is the completion value, read through the same
`completion: true` hook as OIDC claims. Unlike the token contexts, this one
binds the node state, so `sharedState` / `transientState` are accepted in
`given` and judged in `expect` by the decision node's state verdict. The
legacy context id is also used by `SocialProviderHandlerNodeV2`'s own `script`,
which binds `normalizedProfile` and returns a JsonValue. That invocation was
measured (docs/api/12) but is not emulated; a case for it would need its own
fixture (the normalised profile) and result shape.

## The config provider node context in detail

See the README section "Config provider node scripts" for the author's view.
The table used to list this context as next-gen only with an `expect.config`
channel; both engines exist (measured 2026-11-10: the node's script list offers
`CONFIG_PROVIDER_NODE` and `CONFIG_PROVIDER_NODE_NEXT_GEN` scripts, and a
journey runs either), and the channel is `expect.configProvider`, named like
the other context-only channels. AM reads the `config` global after the script,
so the harvest is appended to the source, as for access-token modification,
rather than read through the completion hook. Like the social context, it binds
the node state, so `sharedState` / `transientState` are accepted in `given` and
judged in `expect` by the decision node's state verdict. AM runs the script
again on every request that reaches the node; a case is one invocation, and the
re-run is a gap (`docs/rhino-local-gaps.md`).

## The OAuth2 validate scope context in detail

See the README section "OAuth2 validate scope scripts" for the author's view.
The table used to list this context as next-gen only, with an
`expect.scopes` / `expect.invalidScope` pair; both engines exist (measured
2026-11-10), and the channel is `expect.validateScope`, named like the other
context-only channels, holding `granted`, `invalidScope` and `accepted`. AM
calls the script's `validateAccessTokenScope()` after the body, so the harvest
is appended to the source and makes that call, rather than reading the
completion value. The fail-open row the table warned about is real, and wider
than it said: AM grants exactly the returned list, so narrowing by omission
issues a token without the dropped scope, and a script can also widen a grant
(`docs/api/99`, 2026-11-10).

## The OAuth2 may-act context in detail

See the README section "OAuth2 may act scripts" for the author's view. The table
used to list this context as next-gen only, with `token.setMayAct` as the whole
surface; both engines exist (measured 2026-11-10), and the channel is
`expect.mayAct`, holding `mayAct`, `act`, `scope`, `issued` and `refusal`. A key
present with `null` is a JSON-null claim; an omitted key is an absent claim. AM
stamps both claims after the body, so the harvest is appended to the source
rather than read through the completion hook. Legacy `setMayAct` takes a
`JsonValue`: a plain object is the HTTP 500 `docs/api/22` recorded, and a
`JsonValue` is issued (`docs/api/99`, 2026-11-10).

## The LIBRARY context in detail

See the README section "Library scripts" for the author's view. The sketch row
said the harness `require()`s the library and calls an export, with
`expect.returns` and no caller bindings. That holds. Two parts of it do not: the
runner is not a new kind of program, and the library does not have a scope of
its own apart from its caller.

The case script is stored under `given.libraries` and the runner executes an
empty script. The context's harvest, appended like the other non-completion
contexts, `require()`s that body through the existing loader and calls the
export with `Function.prototype.apply`, so `this` is the object `require()`
returned. A top-level `return` is legal because the loader wraps the body in a
function. A top-level throw is `expect.returns` with `phase: "module"`. The
decision-node loader still closes a library over the caller's scope when some
other context `require()`s one; this context deletes the caller's bindings
before the `require()`, which is what AIC does. That deletion is not applied to
the other contexts.

`scriptName` and `logger.getName()` are the caller's, including inside a nested
`require()`. `given.library.callerContext` is the context segment of that name
and selects that caller's class shutter. It defaults to
`AUTHENTICATION_TREE_DECISION_NODE`, where `new java.util.HashSet()` constructs.
The four OAuth callers refuse it, as the captures record. The refusal message
was not captured. Evaluate-scope is a caller a library can be required from, not
a case context; its allow-list is the same three entries as the other OAuth
callers. `OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER` was not probed.
`OAUTH2_SCRIPTED_JWT_ISSUER` has no configuration hook. OIDC claims, the social
IdP profile transformation and the config provider node were not probed as
library callers.

## The OIDC claims context in detail

See the README section "OIDC claims scripts" for the author's view. The result
is the script's completion value, which a harvest appended to the source cannot
read without changing what the script evaluates to. So a context entry may set
`completion: true` (`src/bindings/preamble.ts`). The runner then evaluates the
script unchanged and calls the context's harvest function with the completion
value. Every other context still gets the appended harvest.
