# 09 — Journeys (authentication trees)

Implemented in: `src/journey/`

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
| Upsert | `PUT` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}` | Plain `PUT` works for create and update; no `If-Match` required. Create returned 201, update returned 200 (verified 2026-06-14). |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}` | Returned 200; follow-up `GET` returned 404 (verified 2026-06-14). |

### Nodes (per type)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List type | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_queryFilter=true` | |
| Read | `GET` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}` | |
| Upsert | `PUT` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}` | Plain `PUT` works for create and update; no `If-Match` required. Create returned 201, update returned 200 (verified 2026-06-14). |

### Custom (designed) nodes

| Op | Method | Path |
|----|--------|------|
| List custom nodes | `GET` | `/am/json/node-designer/node-type?_queryFilter=true` |

## Node catalog discovery (verified 2026-06-14)

Journey editing does not require a hand-built node reference. An AI coding
agent can enumerate the tenant's available node types, fetch a type's JSON
schema (property types, enums, defaults, descriptions, and display order), fetch
the starter template, and then author a valid node config from live tenant data.

Always send:

```http
Accept-API-Version: protocol=2.0,resource=1.0
```

The sandbox returned **235** built-in node types from `getAllTypes`. `tags`
group the catalog into useful buckets such as `marketplace`, `mfa`, and
`basic authn`.

| Op | Method | Full path | Body |
|----|--------|-----------|------|
| List all built-in node types | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes?_action=getAllTypes` | `{}` |
| Fetch config schema for one type | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_action=schema` | `{}` |
| Fetch starter template for one type | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_action=template` | `{}` |
| List custom designer-built node types | `GET` | `/am/json/node-designer/node-type?_queryFilter=true` | none |

### List all built-in node types

```json
{
  "result": [
    {
      "_id": "ScriptedDecisionNode",
      "name": "Scripted Decision",
      "tags": ["basic authn"],
      "metadata": {
        "tags": ["basic authn"]
      },
      "help": "Runs a server-side script to determine the next outcome.",
      "collection": false
    }
  ]
}
```

`_id` is the `nodeType` string used in tree node metadata and node config
paths. The CLI exposes this as:

```bash
aic journey nodes --realm alpha
aic journey nodes --tag mfa --json
```

### Fetch config schema for one type

```json
{
  "type": "object",
  "properties": {
    "script": {
      "title": "Script",
      "description": "The script to execute.",
      "type": "string",
      "propertyOrder": 100,
      "enum": ["[Empty]", "8f3b2c1d-0000-0000-0000-000000000000"],
      "default": "[Empty]"
    }
  }
}
```

For `ScriptedDecisionNode`, the `script` property's `enum` lists available
script UUIDs. The CLI wrapper prints the raw schema:

```bash
aic journey node-schema ScriptedDecisionNode --realm alpha
```

### Fetch starter template for one type

`ScriptedDecisionNode` returned:

```json
{
  "script": "[Empty]",
  "outcomes": [],
  "outputs": ["*"],
  "inputs": ["*"]
}
```

The CLI wrapper prints the raw template:

```bash
aic journey node-template ScriptedDecisionNode --realm alpha
```

### List custom designer-built node types

This endpoint is realm-less: do not add `/realms/root/realms/{realm}`. The
sandbox returned one custom node type. Its object shape is not fully
characterised yet, so client code should keep entries opaque.

```json
{
  "result": [
    {
      "_id": "custom-node-type-id",
      "name": "Custom node type"
    }
  ]
}
```

## Editing journeys — pull / edit / push workflow (verified 2026-06-14)

Audience: AI coding agents editing tenant journeys from the workspace. Use this
loop:

```bash
aic journey pull <name>
# edit workspace/<tenant>/journeys/<realm>/<name>.json
aic journey push <name>
```

The workspace file is a JSON object with `{ "tree": object, "nodes": object }`.
`tree` is the authentication tree document. `nodes` is a map keyed by node UUID,
where each value is that node's separately fetched configuration.

To author a new node:

1. Run `aic journey nodes --realm alpha` to find the `nodeType`.
2. Run `aic journey node-template <nodeType> --realm alpha` for a starter
   config.
3. Run `aic journey node-schema <nodeType> --realm alpha` to see valid fields,
   enum values, defaults, and property types.
4. Add the node config under `nodes` keyed by a fresh UUID.
5. Wire the same UUID into `tree.nodes` with `nodeType`, `connections`,
   `x`/`y`, and any other metadata the tree uses.
6. Update the relevant `connections` from existing nodes so the graph can reach
   the new node.

Write semantics verified live on 2026-06-14:

- Tree and node create/update both accept a plain `PUT`; no `If-Match` or
  `If-None-Match` header is required. Create returned 201, update returned 200.
- Tree and node `PUT` reject `_id`, `_rev`, and any non-whitelisted attribute
  with `400 "Invalid attribute specified"`. For trees, the response
  `detail.validAttributes` listed:
  `description`, `enabled`, `entryNodeId`, `identityResource`,
  `innerTreeOnly`, `maximumIdleTime`, `maximumSessionTime`, `mustRun`,
  `noSession`, `nodes`, `staticNodes`, `transactionalOnly`, `treeTimeout`,
  `uiConfig`. The client strips top-level `_id`/`_rev` before writing and
  leaves nested node metadata, connections, and config untouched.
- `DELETE .../trees/{name}` returned 200, and a follow-up `GET` returned 404.
- Trees and nodes have `_rev`, but it is content-derived: re-PUTting
  byte-identical content returned the same `_rev`. Treat `_rev` equality as
  content equality, not a monotonic revision counter.

Conflict detection is therefore content-snapshot based. `aic journey pull`
writes the export to:

```text
workspace/<tenant>/journeys/<realm>/<name>.json
```

and writes the same pulled bytes to:

```text
workspace/<tenant>/journeys/<realm>/.snapshots/<name>.json
```

On `aic journey push <name>`, the CLI re-pulls the remote tree and nodes,
loads the snapshot baseline, strips every `_rev` key before comparison, and
pushes only if the remote still matches the snapshot. If the remote drifted,
push aborts with a message naming whether the tree and/or node configs changed.
Use `--force` only when intentionally overwriting remote drift or creating from
a local export with no snapshot baseline.

`aic journey delete <name>` requires `--force`; without it the CLI prints what
would be deleted and exits without changing AIC. Before deleting an AM script,
run:

```bash
aic journey using-script <script-uuid> --realm alpha
```

It lists journeys whose scripted nodes reference the script UUID in a top-level
`script` or `scriptId` field.

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
  "staticNodes": {
    "e301438c-0bd0-429c-ab0c-66126501069a": {
      "x": 700,
      "y": 300
    }
  },
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

- **Has `_rev`**, but verified 2026-06-14 as content-derived. We use
  content-snapshot conflict detection and do not send `If-Match`.
- `nodes` is a map keyed by UUID. Each node's connections reference other
  node UUIDs (or built-in outcomes like `true`/`false`).
- `entryNodeId` points to the entry node UUID. Built-in sentinel
  `"e301438c-0bd0-429c-ab0c-66126501069a"` = failure node.
- `staticNodes` holds positions for built-in Success/Failure nodes. These do
  not have separately fetchable node configuration.

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
- Date: 2026-06-13
- Calls: re-verified tree list, tree read, and node read
  (`nodes/ScriptedDecisionNode/{id}`) live. Scripted decision nodes carry a
  `script` UUID reference.
- Date: 2026-06-14
- Calls: `POST …/authenticationtrees/nodes?_action=getAllTypes` (200 OK,
  235 result entries); `POST …/nodes/ScriptedDecisionNode?_action=schema`;
  `POST …/nodes/ScriptedDecisionNode?_action=template`; `GET
  /am/json/node-designer/node-type?_queryFilter=true` (200 OK, 1 result entry).
- Date: 2026-06-14
- Calls: `PUT …/authenticationtrees/nodes/ScriptedDecisionNode/{uuid}` create
  (201) and update (200); `PUT …/authenticationtrees/trees/test_push_probe`
  create (201) and update (200); byte-identical re-PUT kept the same
  content-derived `_rev`; `DELETE …/trees/test_push_probe` returned 200 and
  follow-up `GET` returned 404. Throwaway `test_push_probe` tree and node were
  cleaned up.

## Source citations

- frodo-lib: `src/api/TreeApi.ts`, `src/api/NodeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/journeys.js`,
  `packages/fr-config-push/src/scripts/update-auth-trees.js`.

## Open questions

- Resolved 2026-06-14: full inventory of built-in `nodeType` strings is
  available from `POST …/authenticationtrees/nodes?_action=getAllTypes`
  (235 entries on the sandbox).
