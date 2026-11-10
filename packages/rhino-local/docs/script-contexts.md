# Script contexts

Status: design note, 2026-11-10. Implemented: the selector,
`OAUTH2_ACCESS_TOKEN_MODIFICATION` and `OIDC_CLAIMS`, each on both engines. The
other five rows of the table below are sketches, not commitments.

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
   prelude after `context-common.cjs`, whose `__rhinoLocalCtx*` helpers (Java
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
`OBSERVED_BASE` boxed types the decision node gets. Legacy installs none, as for
the decision node.

## The seven contexts

| Context (`context` + engine)                                          | Bindings beyond the shared set                                                                                                | Fixtures                                                                  | Invocation, and what AM does with it                                                                                             | Assertions                                                                               |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `OAUTH2_ACCESS_TOKEN_MODIFICATION` legacy + next-gen (**implemented**) | `accessToken`, `identity`, `scopes`, `requestProperties`, `clientProperties`; legacy `session` (null)                         | `accessToken` (starting state), `identity`, `client`, `scopes`, `request` | Top-level body; return value ignored; AM issues the mutated token                                                                | `expect.accessToken`: fields added/changed/removed, scope, expiry, and the other setters |
| `OIDC_CLAIMS` legacy + next-gen (**implemented**) | `claims`, `requestedClaims`, `claimsLocales`, `identity`, `scopes`, `requestProperties`, `clientProperties`; legacy `claimObjects`, `requestedTypedClaims`, `session` (null) | `identity`, `scopes`, `client`, `request`, `oidc` (claims seed, `claimsLocales`) | Completion value: next-gen `{values, compositeScopes}`, legacy a `UserInfoClaims`; read through the runner's completion hook | `expect.oidc`: `userinfo`, `idToken`, `compositeScopes`, `issued` |
| `SOCIAL_IDP_PROFILE_TRANSFORMATION` legacy + next-gen                 | `rawProfile`, `normalizedProfile`(?), `selectedIdp`                                                                           | the provider's raw profile                                                | Top-level body that returns the normalised profile                                                                               | `expect.profile`                                                                         |
| `CONFIG_PROVIDER_NODE` (next-gen)                                     | `nodeState` (read), `config`                                                                                                  | node state seed                                                           | Top-level body that assigns `config`; AM configures the wrapped node from it                                                     | `expect.config`, plus the state channels it can read                                     |
| `OAUTH2_VALIDATE_SCOPE_NEXT_GEN`                                      | `scopeValidatorHelper`, `requestedScopes`, `availableScopes`, `allowedScopes`, `defaultScopes`                                | requested / available / allowed scopes                                    | Named function (`validateAccessTokenScope`, …) AM calls after the body; returns the scope list or throws via `throwInvalidScope` | `expect.scopes` or `expect.invalidScope` (the measured fail-open row matters)            |
| `OAUTH2_MAY_ACT_NEXT_GEN`                                             | `token` (with `setMayAct`)                                                                                                    | token starting state                                                      | Top-level body; AM stamps `may_act`                                                                                              | `expect.mayAct`                                                                          |
| `LIBRARY`                                                             | library scope only (`openidm`, `httpClient`, `logger`, …; no context bindings)                                                | arguments to the export                                                   | No body semantics: the case names an export and its arguments; the harness `require()`s the library and calls it                 | `expect.returns`, plus shared channels                                                   |

Shared bindings per context come from the binding descriptors in
`docs/api/bindings/` for presence and from live probes for behaviour; a
descriptor alone never makes a member callable.

## The access-token modification context in detail

See the README section "Access-token modification scripts" for the author's
view; the measured surface is in `docs/api/12-script-bindings-matrix.md` and the
gaps in `docs/rhino-local-gaps.md`.

## The OIDC claims context in detail

See the README section "OIDC claims scripts" for the author's view. The result
is the script's completion value, which a harvest appended to the source cannot
read without changing what the script evaluates to. So a context entry may set
`completion: true` (`src/bindings/preamble.ts`). The runner then evaluates the
script unchanged and calls the context's harvest function with the completion
value. Every other context still gets the appended harvest.
