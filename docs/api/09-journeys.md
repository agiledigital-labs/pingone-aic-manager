# 09 — Journeys (authentication trees)

## Purpose
Journeys ("authentication trees") are AM's flow definitions: a graph of nodes
that handle login, registration, MFA, password reset, etc. Not in the initial
feature set, but documented because they reference scripts heavily and we'll
need them for cross-feature integration (e.g. "which journeys use script X").

## Authentication
Service-account bearer. Scope: `fr:am:*`.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`).
Always send `Accept-API-Version: protocol=2.0,resource=1.0`.

### Trees

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees?_queryFilter=true` | |
| Read | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}` | `name` is the tree name, not a UUID. |
| Upsert | `PUT` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}` | |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}` | |

### Nodes (per type)

| Op | Method | Path |
|----|--------|------|
| List type | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_queryFilter=true` |
| Read | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}` |
| Upsert | `PUT` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}` |

### Custom (designed) nodes

| Op | Method | Path |
|----|--------|------|
| List custom nodes | `GET` | `/am/json/node-designer/node-type?_queryFilter=true` |

## Object shape (real example from sandbox)

```json
{
  "_id": "_VerifyEmail",
  "_rev": "1872923965",
  "identityResource": "managed/alpha_user",
  "entryNodeId": "06fa2a1c-…",
  "innerTreeOnly": false,
  "description": "Verify a user's email address",
  "noSession": false,
  "mustRun": false,
  "enabled": true,
  "transactionalOnly": false,
  "uiConfig": {},
  "nodes": {
    "06fa2a1c-…": {
      "connections": {
        "disallowed": "e301438c-…",
        "ok":         "6c0369ef-…"
      },
      "displayName": "Called As Inner Journey?",
      "nodeType":    "ScriptedDecisionNode",
      "version":     "1.0",
      "x": 60, "y": 427.75
    },
    "6c0369ef-…": { /* HOTP Generator … */ }
  }
}
```

- **Has `_rev`** — use `If-Match` for optimistic locking.
- `nodes` is a map keyed by UUID. Each node's connections reference other
  node UUIDs (or built-in outcomes like `true`/`false`).
- `entryNodeId` points to the entry node UUID. Built-in sentinel
  `"e301438c-0bd0-429c-ab0c-66126501069a"` = failure node.

## Script references

A `ScriptedDecisionNode` (or any `*ScriptedNode`) holds the script's UUID in
its config. To find which journeys reference a given script, walk every tree's
`nodes` and inspect node configs. Useful for the "won't-break-anything" check
before deleting a script.

## Examples

```bash
# List the first journey in alpha
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/realm-config/authentication/authenticationtrees/trees?_queryFilter=true&_pageSize=1" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"
```

## Quirks

- **Tree ID is the name**, not a UUID. Renaming = delete + create (no in-place
  rename).
- **Node positions (`x`, `y`)** are floats and carry visual state from the UI
  editor. Preserve them on round-trip to avoid graph jiggle.
- **`uiConfig`** is often `{}` but holds palette/zoom state when set.
- **`transactionalOnly: true`** journeys can't issue an SSO session — used for
  step-up MFA inside another flow.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/authenticationtrees/trees?_queryFilter=true&_pageSize=1`
  (200 OK, full structure as shown).

## Source citations

- frodo-lib: `src/api/TreeApi.ts`, `src/api/NodeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/journeys.js`,
  `packages/fr-config-push/src/scripts/update-auth-trees.js`.

## Open questions

- Full inventory of `nodeType` strings — only enumerable by walking real trees
  or hitting the per-type endpoint repeatedly. Likely hundreds; defer.
