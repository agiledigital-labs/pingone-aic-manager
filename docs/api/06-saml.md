# 06 — SAML 2.0

## Purpose
Manage SAML 2.0 hosted (this tenant is the IdP/SP) and remote (another party is
the IdP/SP) entity providers, plus the circles of trust that bind them. Feature
3 of aic-edit ("manage OIDC and SAML config") is partly built on this API.

## Authentication
Service-account bearer. Scope: `fr:am:*`.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`).
Always send `Accept-API-Version: protocol=2.1,resource=1.0`.

### Entity providers

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/realm-config/saml2?_queryFilter=true` | Stubs only. |
| Filter by entityId | `GET` | `/am/json{realm-path}/realm-config/saml2?_queryFilter=entityId+eq+"…"` | Returns `_id` + `location`. |
| Read full | `GET` | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}` | `location` ∈ `hosted` \| `remote`. |
| Create hosted | `POST` | `/am/json{realm-path}/realm-config/saml2/hosted/?_action=create` | Body: config object. |
| Import remote | `POST` | `/am/json{realm-path}/realm-config/saml2/remote/?_action=importEntity` | Body has XML metadata. |
| Update | `PUT` | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}` | |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}` | |
| Export metadata XML | `GET` | `/am/saml2/jsp/exportmetadata.jsp?entityid={entityId}&realm=/{realm}` | Raw XML (not JSON). |

`{entityId64}` is the entity ID **base64-encoded without padding** (URL-safe).

### Circles of Trust

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/realm-config/federation/circlesoftrust?_queryFilter=true` | |
| Read | `GET` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}` | |
| Create | `POST` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/?_action=create` | |
| Update | `PUT` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}` | |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}` | |

## Object shapes

### Entity provider

```json
{
  "_id": "…base64-without-padding…",
  "_rev": "…",
  "entityId": "https://sp.example.com/saml",
  "entityLocation": "hosted",
  "serviceProvider": { /* SP config; null if pure IdP */ },
  "identityProvider": {
    "assertionProcessing": { "attributeMapper": { /* ... */ } },
    "advanced":           { "idpAdapter":      { /* ... */ } }
  },
  "attributeQueryProvider":     null,
  "xacmlPolicyEnforcementPoint": null
}
```

### Circle of Trust

```json
{
  "_id": "MyCircleOfTrust",
  "_rev": "...",
  "status": "enabled",
  "trustedProviders": [
    "https://sp.example.com/saml|saml2",
    "https://idp.example.com/saml|saml2"
  ]
}
```

`trustedProviders` entries are `<entityId>|<protocol>` strings.

## Examples

```bash
# List SAML entities (alpha currently has none in sandbox)
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/realm-config/saml2?_queryFilter=true" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"

# Export hosted metadata XML
curl "$TENANT_BASE_URL/am/saml2/jsp/exportmetadata.jsp?entityid=https://sp.example.com/saml&realm=/alpha"
```

## Quirks

- **`{entityId64}` is unpadded URL-safe base64.** Standard base64url (no `=`).
  `Buffer.from(entityId).toString('base64url')` in Node;
  `URL_SAFE_NO_PAD.encode(...)` in Rust (`base64` crate).
- **The list endpoint returns stubs**, not full configs. Always follow up with
  the `/{location}/{entityId64}` GET to get usable data.
- **`exportmetadata.jsp` is an old JSP page**, not a JSON API. Its realm param
  is `realm=/alpha`, not the long `/realms/root/realms/alpha` form. Returns
  `Content-Type: text/xml`.
- **Importing remote** via `?_action=importEntity` accepts XML embedded inside
  a JSON envelope; see frodo-lib `Saml2Api.ts` for the exact wrapper.
- **`null` vs absent.** Many sub-objects (e.g. `serviceProvider`) are present
  as `null` when not configured; don't treat `null` as missing/error.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/realm-config/saml2?_queryFilter=true` → 200 OK with
  `"result": []` (no entities provisioned in alpha; endpoint shape confirmed).
- Not yet exercised: hosted/remote GET, CoT, metadata export — sandbox lacks
  data. Update this section after first real call.

## Source citations

- frodo-lib: `src/api/Saml2Api.ts`, `src/api/CirclesOfTrustApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/saml.js`,
  `packages/fr-config-push/src/scripts/update-saml.js`.
- Ping docs: <https://apidocs.id.forgerock.io/> (SAML2 section).

## Open questions

- Full shape of `entityProvider.serviceProvider` and `identityProvider` —
  re-document once we have a real entity to inspect.
- Exact body for `?_action=importEntity` — JSON wrapper around the XML, or
  multipart? Check frodo-lib `Saml2Api.ts:importMetadata`.
