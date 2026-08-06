# User-token custom-endpoint proof of concept

This POC demonstrates the safe way to let an end user call an IDM custom
endpoint. It is intentionally **not** public: it relies on IDM's built-in
`rsFilter` to validate a bearer token issued by this tenant, then uses an IDM
authorization role to allow just this route.

The endpoint script must not decode JWTs, fetch JWKS documents, or call token
introspection. Before the endpoint executes, `rsFilter` delegates bearer-token
validation to AM and populates `context.security`. An invalid, expired, or
tenant-untrusted token never becomes an authenticated security context.

## What this proves

```text
client -> Authorization: Bearer <AM user access token>
       -> IDM rsFilter validates token + its required IDM scope
       -> subject mapping creates context.security (including IDM roles)
       -> config/access permits only api-poc-read on this endpoint
       -> user-token-poc.cjs returns the caller identity
```

The POC's only operation is `GET /openidm/endpoint/user-token-poc/whoami`.
It returns the authenticated subject, the CREST `request`, and a deliberately
sanitized projection of the context for binding discovery. It returns no
managed-object data and never returns request headers or OAuth credentials.

## Live sandbox result

Deployed and exercised on 2026-08-06. The endpoint remains installed in the
sandbox. For live testing, its exact-path access role is currently
`internal/role/openidm-authorized`, and its script allowlist contains that role
plus `internal/role/openidm-admin`. Those broad sandbox defaults should be
replaced with the dedicated `api-poc-read` role shown below before real use.

| Request | Observed outcome |
| --- | --- |
| Valid tenant service-account token | `200`; populated `context.security` |
| Missing token | `403 Access denied` from IDM authorization |
| Malformed bearer token | `401` before the endpoint script |
| Valid token, script allowlist deliberately unsatisfied | `403 Missing required API role` |
| Valid token, sanitized binding probe | `200`; scopes found at `context.oauth2.scopes` and `context.oauth2.rawInfo.scope` |

The intended allowlist was restored and the valid-token `200` path was checked
again. This proves tenant token validation and role enforcement, but not the
end-user subject-mapping path: no real end-user access token was available in
this session.

## Prerequisites

1. Create a dedicated OAuth 2.0 client for this API consumer. Use an
   authorization-code flow with PKCE for a browser/native user client.
2. Give that client only the scopes it needs. In the current standard AIC IDM
   configuration, the global `rsFilter.scopes` list requires `fr:idm:*` for
   every protected `/openidm` request. That requirement is service-wide, not
   endpoint-specific; do not change it to a POC-specific value without testing
   every IDM client and service account.
3. Create a custom IDM authorization role and assign it only to the test user.
   Record the exact role string as it appears in `context.security.authorization.roles`.
   This POC calls it `api-poc-read` below; use the exact value your tenant
   returns (often an `internal/role/...` reference).

## Deploy

First create and pull the endpoint so it is registered in the local sync
manifest:

```bash
cd workspace/sandbox
aic script create endpoint/user-token-poc
```

Replace `idm/endpoint/user-token-poc.cjs` with `user-token-poc.cjs`, then push
the source. The endpoint configuration also needs this global object:

```json
{
  "endpointConfig": {
    "allowedRoles": [
      "api-poc-read",
      "internal/role/openidm-admin"
    ]
  }
}
```

Use the AIC Custom Endpoints editor or a reviewed full-object update to add the
global object; script sync preserves the endpoint configuration and updates its
source only.

Then read `/openidm/config/access`, append this narrowly-scoped rule to its
existing `configs` array, and PUT the complete configuration back:

```json
{
  "pattern": "endpoint/user-token-poc/whoami",
  "roles": "api-poc-read",
  "methods": "read",
  "actions": "*"
}
```

The admin role in `allowedRoles` lets an administrator exercise the live POC;
it does not appear in the route-specific access rule because the tenant's
existing administrator rule already grants access. Remove it after testing if
administrators should also be rejected by the script.

Do not use `roles: "*"`: that would admit anonymous calls. Match the exact user
role value configured in the script. The router performs this authorization
before the script runs; the script repeats it as a fail-closed check.

## Exercise

Obtain an access token for the dedicated client as the test user, then call:

```bash
curl --fail-with-body \
  -H "Authorization: Bearer $USER_ACCESS_TOKEN" \
  "$TENANT_BASE_URL/openidm/endpoint/user-token-poc/whoami"
```

Expected outcomes:

| Request | Expected outcome |
| --- | --- |
| Valid token, correct role | `200` and the caller's subject |
| Valid token, no API role | `403` |
| Missing token | `403`, before the script executes |
| Expired, forged, or another-tenant token | `401`, before the script executes |

## Endpoint-local scope checks

The live probe confirms that an endpoint can apply an additional scope check.
The preferred path is `context.oauth2.scopes`. Ping's backing
`AccessTokenInfo` API defines it as a Java `Set<String>` populated after
`rsFilter` validates the token. The raw, space-delimited value also exists at
`context.oauth2.rawInfo.scope`:

```javascript
var requiredScope = "example:announcements:read";
if (
  !context.oauth2 ||
  !context.oauth2.scopes ||
  !context.oauth2.scopes.contains(requiredScope)
) {
  throw { code: 403, message: "Missing required OAuth scope" };
}
```

IDM's `rsFilter` enforces its configured required scopes when it validates the
token. In the standard configuration that list is `fr:idm:*`, so a custom
endpoint cannot replace it with `example:announcements:read` while other IDM
APIs retain their current requirement. The endpoint check is additive: the
caller needs the global `fr:idm:*` scope, the role accepted by `config/access`,
and the endpoint-specific scope checked in JavaScript. Use a dedicated OAuth
client, a dedicated IDM role, and an exact-path access rule. Do not implement
JWT verification or AM token introspection in the custom endpoint.

## Diagnostic safety

Do not return the full `context`, even temporarily. `context.http.headers`
contains the bearer token. Directly serializing `context.security` is also
unsafe: inherited `parent` objects lead back to `context.oauth2`, including
`token` and `rawInfo.sessionToken`. The POC therefore returns only an explicit
allowlist of safe identity fields, header names (not values), binding keys, and
scope descriptions. Its `request` value is the CREST request binding and does
not contain the HTTP Authorization header.

## Sources

- [Ping: Authentication through OAuth 2.0 and subject mappings](https://docs.pingidentity.com/pingoneaic/idm-auth/rsfilter-module.html)
- [Ping: Authentication and roles](https://docs.pingidentity.com/pingoneaic/idm-auth/authentication-and-roles.html)
- [Ping: Authorization and roles](https://backstage.forgerock.com/docs/idcloud/latest/idm-auth/authorization-and-roles.html)
- [Ping AM API: `AccessTokenInfo.getScopes()`](https://docs.pingidentity.com/pingam/7.4/_attachments/apidocs/org/forgerock/http/oauth2/AccessTokenInfo.html)
