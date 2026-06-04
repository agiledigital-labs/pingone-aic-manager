# AM script contexts endpoint (binding metadata)

Verified against the sandbox tenant 2026-06-04.

The AM script editor populates its per-context IntelliSense from a **contexts**
endpoint that returns the authoritative binding surface (every binding, its
methods with named params + types + overloads, and the Java allow-list) for a
script context.

## Endpoint

```
GET /am/json/{realm}/contexts/{CONTEXT_ID}
Accept-API-Version: protocol=2.0,resource=1.0
```

- Both the short realm form (`/am/json/alpha/contexts/…`) and the canonical
  (`/am/json/realms/root/realms/alpha/contexts/…`) return `200` here — unlike
  the scripts endpoint, the short form is NOT rejected.
- **Read by id only.** The collection cannot be listed: a bare
  `GET …/contexts` → `400 "The resource collection cannot be read"`,
  `?_queryFilter=true` → `501`, and `?_action=…` → `501`. So context ids must be
  known up front (we keep the list in `src/aic/script/am.rs::base_slug`).

### Response shape

```jsonc
{
  "_id": "SCRIPTED_DECISION_NODE",
  "bindings": [
    { "name": "logger", "javaScriptType": "object",
      "javaClass": "…ScriptedLoggerWrapper",
      "elements": [ { "elementType": "method", "name": "info",
        "parameters": [ {"name":"format","javaScriptType":"string"}, … ],
        "returnType": "void" }, … ] },
    …
  ],
  "evaluatorVersions": { "JAVASCRIPT": ["2.0"] },
  "allowLists": [ "org.forgerock.json.JsonValue", … ]
}
```

`javaScriptType` is one of `string | number | boolean | object | array |
unknown | void`. `elements` are `method` or `field` (fields nest their own
`elements`, e.g. `utils.crypto.subtle`).

## Key behaviour: metadata only exists for next-gen contexts

A context exposes binding metadata **only once it supports next-generation
(`evaluatorVersion 2.0`)**. Legacy-only contexts return an empty `bindings`
array and `evaluatorVersions: {"JAVASCRIPT":["1.0"],"GROOVY":["1.0"]}`. So the
endpoint is a precise, no-probing source of types for every context **as it gets
upgraded** — but can't help legacy-only contexts yet.

## Contexts probed 2026-06-04

Next-gen (binding metadata captured under `docs/api/bindings/`):

| Context | bindings | artifact |
| --- | --- | --- |
| `SCRIPTED_DECISION_NODE` | 25 | `scripted-decision-next.json` |
| `DEVICE_MATCH_NODE` | 25 | `device-match-next.json` |
| `OIDC_CLAIMS_NEXT_GEN` | 18 | `oidc-claims-next.json` |
| `SOCIAL_PROVIDER_HANDLER_NODE` | 17 | `social-provider-handler-next.json` |
| `SAML2_NAMEID_MAPPER` | 16 | `saml2-nameid-mapper-next.json` |
| `OAUTH2_DYNAMIC_CLIENT_REGISTRATION` | 15 | `oauth2-dcr-next.json` |
| `SAML2_SP_ACCOUNT_MAPPER` | 14 | `saml2-sp-account-mapper-next.json` |
| `PINGONE_VERIFY_COMPLETION_DECISION_NODE` | 14 | `pingone-verify-completion-next.json` |
| `LIBRARY` | 11 | `library-next.json` |

Legacy-only (0 bindings; `JAVASCRIPT`+`GROOVY` `1.0`): `OIDC_CLAIMS`,
`OAUTH2_ACCESS_TOKEN_MODIFICATION`, `OAUTH2_MAY_ACT`,
`OAUTH2_SCRIPTED_JWT_ISSUER`, `OAUTH2_VALIDATE_SCOPE`, `OAUTH2_EVALUATE_SCOPE`,
`OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER`, `POLICY_CONDITION`,
`SOCIAL_IDP_PROFILE_TRANSFORMATION`, `CONFIG_PROVIDER_NODE`,
`SAML2_IDP_ADAPTER`, `SAML2_SP_ADAPTER`, `SAML2_IDP_ATTRIBUTE_MAPPER`,
`AUTHENTICATION_TREE_DECISION_NODE` (legacy scripted decision).

Bad ids (`400`): `DEVICE_PROFILE_MATCH_NODE`, `PINGONE_VERIFY_EVALUATION_NODE`.

## The next-gen common binding set

Intersecting the three contexts we type (`SCRIPTED_DECISION_NODE`, `LIBRARY`,
`OIDC_CLAIMS_NEXT_GEN`) gives the bindings every next-gen script has:

`cookieName`, `httpClient`, `jwtAssertion`, `jwtValidator`, `logger`,
`openidm`, `policy`, `realm`, `scriptName`, `secrets`, `utils`.

### Listed bindings vs runtime globals

This metadata is the **editor's binding list**, which is narrower than what's
actually reachable at runtime. Two globals are present at runtime on next-gen
(verified via `typeof` probe — see `docs/api/12`) but are **not** in any
next-gen context's binding list:

- `systemEnv` — present at runtime on both engines; kept in the shared
  `common.d.ts` so next-gen scripts that use it still type-check.
- `JavaImporter` — present at runtime on both engines, but next-gen has no
  configurable Java allow-list (it's fixed; see the `allowLists` array in each
  artifact). Typed only in the legacy overlay to steer next-gen scripts toward
  the documented bindings.

So "not in the binding metadata" ≠ "absent at runtime"; the matrix
(`docs/api/12`) records the runtime-`typeof` results, this file records the
editor's declared surface.
