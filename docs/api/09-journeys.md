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

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`). Always
send `Accept-API-Version: protocol=2.0,resource=1.0`.

### Trees

| Op     | Method   | Path                                                                                           | Notes                                                                                                                            |
| ------ | -------- | ---------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| List   | `GET`    | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees?_queryFilter=true` |                                                                                                                                  |
| Read   | `GET`    | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}`            | `name` is the tree name, not a UUID.                                                                                             |
| Upsert | `PUT`    | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}`            | Plain `PUT` works for create and update; no `If-Match` required. Create returned 201, update returned 200 (verified 2026-06-14). |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/trees/{name}`            | Returned 200; follow-up `GET` returned 404 (verified 2026-06-14).                                                                |

### Nodes (per type)

| Op        | Method | Path                                                                                                      | Notes                                                                                                                            |
| --------- | ------ | --------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| List type | `GET`  | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_queryFilter=true` |                                                                                                                                  |
| Read      | `GET`  | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}`          |                                                                                                                                  |
| Upsert    | `PUT`  | `/am/json{realm-path}/realm-config/authentication/authenticationtrees/nodes/{nodeType}/{nodeId}`          | Plain `PUT` works for create and update; no `If-Match` required. Create returned 201, update returned 200 (verified 2026-06-14). |

### Custom (designed) nodes

| Op                | Method | Path                                                 |
| ----------------- | ------ | ---------------------------------------------------- |
| List custom nodes | `GET`  | `/am/json/node-designer/node-type?_queryFilter=true` |

## Node catalog discovery (verified 2026-06-14)

Journey editing does not require a hand-built node reference. An AI coding agent
can enumerate the tenant's available node types, fetch a type's JSON schema
(property types, enums, defaults, descriptions, and display order), fetch the
starter template, and then author a valid node config from live tenant data.

Always send:

```http
Accept-API-Version: protocol=2.0,resource=1.0
```

The sandbox returned **235** built-in node types from `getAllTypes`. `tags`
group the catalog into useful buckets such as `marketplace`, `mfa`, and
`basic authn`.

| Op                                    | Method | Full path                                                                                                               | Body |
| ------------------------------------- | ------ | ----------------------------------------------------------------------------------------------------------------------- | ---- |
| List all built-in node types          | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes?_action=getAllTypes`         | `{}` |
| Fetch config schema for one type      | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_action=schema`   | `{}` |
| Fetch starter template for one type   | `POST` | `/am/json/realms/root/realms/{realm}/realm-config/authentication/authenticationtrees/nodes/{nodeType}?_action=template` | `{}` |
| List custom designer-built node types | `GET`  | `/am/json/node-designer/node-type?_queryFilter=true`                                                                    | none |

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

`_id` is the `nodeType` string used in tree node metadata and node config paths.
The CLI exposes this as:

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
5. Wire the same UUID into `tree.nodes` with `nodeType`, `connections`, `x`/`y`,
   and any other metadata the tree uses.
6. Update the relevant `connections` from existing nodes so the graph can reach
   the new node.

Write semantics verified live on 2026-06-14:

- Tree and node create/update both accept a plain `PUT`; no `If-Match` or
  `If-None-Match` header is required. Create returned 201, update returned 200.
- Tree and node `PUT` reject `_id`, `_rev`, and any non-whitelisted attribute
  with `400 "Invalid attribute specified"`. For trees, the response
  `detail.validAttributes` listed: `description`, `enabled`, `entryNodeId`,
  `identityResource`, `innerTreeOnly`, `maximumIdleTime`, `maximumSessionTime`,
  `mustRun`, `noSession`, `nodes`, `staticNodes`, `transactionalOnly`,
  `treeTimeout`, `uiConfig`. The client strips top-level `_id`/`_rev` before
  writing and leaves nested node metadata, connections, and config untouched.
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

On `aic journey push <name>`, the CLI re-pulls the remote tree and nodes, loads
the snapshot baseline, strips every `_rev` key before comparison, and pushes
only if the remote still matches the snapshot. If the remote drifted, push
aborts with a message naming whether the tree and/or node configs changed. Use
`--force` only when intentionally overwriting remote drift or creating from a
local export with no snapshot baseline.

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
        "ok": "6c0369ef-…"
      },
      "displayName": "Called As Inner Journey?",
      "nodeType": "ScriptedDecisionNode",
      "version": "1.0",
      "x": 60,
      "y": 427.75
    },
    "6c0369ef-…": {
      /* HOTP Generator … */
    }
  }
}
```

- **Has `_rev`**, but verified 2026-06-14 as content-derived. We use
  content-snapshot conflict detection and do not send `If-Match`.
- `nodes` is a map keyed by UUID. Each node's connections reference other node
  UUIDs (or built-in outcomes like `true`/`false`).
- `entryNodeId` points to the entry node UUID. Built-in sentinel
  `"e301438c-0bd0-429c-ab0c-66126501069a"` = failure node.
- `staticNodes` holds positions for built-in Success/Failure nodes. These do not
  have separately fetchable node configuration. **It is optional** — see the
  survey below.

## Realm-wide survey of tree bodies (verified 2026-08-14)

Every tree in `alpha` was fetched **individually** (`GET …/trees/{name}`, not
just the `_queryFilter=true` list form) and the key sets tallied: **36 trees,
178 tree nodes**. Counts below are over that population; they say what does
occur, not what the API forbids.

### `uiConfig` holds two keys, and `annotations` is a JSON-encoded string

| Key           | Trees    | Value                                                                     |
| ------------- | -------- | ------------------------------------------------------------------------- |
| `categories`  | 19 of 36 | value shape not characterised in this survey                              |
| `annotations` | 6 of 36  | a **string** containing JSON — `{"forNodes":{},"structural":[]}` on all 6 |

No other `uiConfig` key occurred on any of the 36 trees. `annotations` is the
journey editor's canvas layout — UI chrome, not behaviour — but note the type:
it is a JSON-encoded string, not a nested object, so a typed decoder must not
model it as one.

**This bites fail-closed consumers.** The sibling
`terraform-provider-pingone-aic` allowlisted `categories` as the only permitted
`uiConfig` key and treats an unknown key as an error by design; all 6 annotated
trees were therefore rejected outright — a sixth of the realm unreadable —
because this file had never recorded that `annotations` occurs. Anything that
validates the tree body must allow it. See `99-quirks-and-open-questions.md`
(2026-08-14).

### `staticNodes` is optional

**3 of 36 trees have no `staticNodes` key at all.** Treat it as absent, not as
an empty object. Where present, the 96 entries observed carry only `x` and `y` —
no other key appeared.

### Tree node metadata has a fixed key set

Across all 178 nodes in `tree.nodes`, the key set is exactly:

```text
connections  displayName  nodeType  version  x  y
```

All six were present on all 178 — none was ever absent — and `version` was the
string `"1.0"` on every one.

### Session-timeout fields are real, writable, and omitted when unset

`maximumIdleTime`, `maximumSessionTime` and `treeTimeout` are in the tree's
`validAttributes` list (see the write-semantics note above) but appear on
**none** of the 36 trees. They are not vestigial: a `PUT` creating a tree with
`maximumIdleTime: 7`, `maximumSessionTime: 11`, `treeTimeout: 13` returned
**201** with all three echoed in the response body, and a subsequent `GET`
returned them too. (The probe tree was deleted afterwards and its removal
confirmed by a `GET` returning 404.)

So they are genuine optional tree attributes that AM simply omits from the
response when unset. Two consequences for clients: model them as optional rather
than required, and do not infer from a response that a field you did not set is
unsupported.

## Script references

A `ScriptedDecisionNode` (or any `*ScriptedNode`) holds the script's UUID in its
config. To find which journeys reference a given script, walk every tree's
`nodes` and inspect node configs. Useful for the "won't-break-anything" check
before deleting a script.

## Examples

```bash
# List the first journey in alpha
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/realm-config/authentication/authenticationtrees/trees?_queryFilter=true&_pageSize=1" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"
```

## Origin-based redirect (hosted UI → custom UI)

Verified 2026-08-13 on the sandbox: a journey **can** detect that it was started
from the hosted login UI and bounce the browser to a custom UI.

Hosted login on this tenant is the `@forgerock/platform-login` SPA. `/`
301s to `/login/`; `/login/` and `/am/XUI/` serve the same HTML. The SPA POSTs
`/am/json/realms/root/realms/{realm}/authenticate`. That request carries the
browser `Origin` / `Referer`, which a next-gen scripted decision can read.

### What the script sees

`requestHeaders` is a case-insensitive multimap. `String(requestHeaders)` dumps
every key; `requestHeaders.get("origin")` / `.get("referer")` return a Java
list (use `.get(0)`). `keySet()` is blocked by the next-gen Java allow-list.

When the client sends the headers, both arrive verbatim:

| Client `Origin` | `requestHeaders.get("origin").get(0)` |
|---|---|
| _(omitted — curl / native SDK)_ | `null` |
| `https://tenant.example.com` | same (hosted UI) |
| `https://journeys.example.com` | same (custom UI; already in CORS) |
| `https://evil.example.com` | same (AM does **not** filter Origin) |

`Host` is always the tenant hostname and cannot distinguish the two UIs.
Hosted pages send `referrer-policy: origin`, so do not rely on the Referer
_path_ (`/login` vs `/am/XUI`).

### Emitting the bounce

`callbacksBuilder.redirectCallback(url, {}, "GET")` returns this callback on
`POST …/authenticate`:

```json
{
  "type": "RedirectCallback",
  "output": [
    { "name": "redirectUrl", "value": "https://custom.example/login" },
    { "name": "redirectMethod", "value": "GET" },
    { "name": "trackingCookie", "value": false },
    { "name": "redirectData", "value": {} }
  ]
}
```

The hosted login SPA handles that type: `handleRedirectCallback` reads
`redirectUrl` and calls `location.assign` (GET) or auto-submits a form (POST).
Verified by reading the live `/login/js/*.js` chunks.

An origin-gated first node, invoked three ways against `AIC-Rhino-Let-Probe`:

| `Origin` | Result |
|---|---|
| custom UI origin | `HiddenValueCallback` only — journey continues |
| tenant origin | `RedirectCallback` + hidden — hosted UI would navigate away |
| omitted | `HiddenValueCallback` only — do **not** bounce API/SDK callers |

Sketch:

```javascript
var values = requestHeaders.get("origin");
var origin = values ? String(values.get(0)) : "";
var hosted = origin === "https://tenant.example.com";
if (hosted) {
  callbacksBuilder.redirectCallback(
    "https://journeys.example.com/login",
    {},
    "GET"
  );
}
outcome = hosted ? "redirect" : "continue";
```

After the bounce the custom UI starts a **new** authenticate. Its `Origin` is
the custom host, so the same node must not redirect again.

### Related, not substitutes

- **`RequestHeaderNode`** (`allowedHeaders`) can copy `origin` into shared
  state if you would rather keep the test out of the first script.
- **`SetSuccessUrlNode` / `SetFailureUrlNode`** fire only at journey end, not
  on first paint.
- **Theme `journeyFooterScriptTag`** (see `#user-theme-script-container` in
  `/login/` HTML) runs only inside hosted pages, so it can redirect without
  reading Origin — but it applies to every journey that uses that theme unless
  the script itself inspects the journey name in the URL.
- **`platformSettings.loginUrl`** in `/openidm/config/ui/configuration` is
  empty on this tenant and is the end-user-app login override, not a per-journey
  bounce.
- Custom UI origins still need a CORS configuration. The sandbox already has
  `SSPWebPortal`, `Omni-Test`, and `customer-web-portal` configs covering
  several of ours.

## Quirks

- **Tree ID is the name**, not a UUID. Renaming = delete + create (no in-place
  rename).
- **Tree names may contain spaces.** Two of the 36 trees in `alpha` have a space
  in their `_id` (e.g. `OAuth2 Client Authorization Test`). Anything that builds
  a URL path or a filename from a tree name must encode or sanitise it.
- **Node positions (`x`, `y`)** are floats and carry visual state from the UI
  editor. Preserve them on round-trip to avoid graph jiggle.
- **`uiConfig`** is editor state, not behaviour. Across the 36 `alpha` trees it
  holds only `categories` and/or `annotations`, and `annotations` is a
  JSON-encoded **string** — see the survey above. An earlier note here said it
  "holds palette/zoom state when set"; no palette or zoom key was observed on
  any tree, so that claim is withdrawn.
- **`staticNodes` may be absent entirely** (3 of 36 trees), so a decoder must
  treat it as optional rather than defaulting it to an empty map.
- **`transactionalOnly: true`** journeys can't issue an SSO session — used for
  step-up MFA inside another flow.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/authenticationtrees/trees?_queryFilter=true&_pageSize=1` (200
  OK, full structure as shown).
- Date: 2026-06-13
- Calls: re-verified tree list, tree read, and node read
  (`nodes/ScriptedDecisionNode/{id}`) live. Scripted decision nodes carry a
  `script` UUID reference.
- Date: 2026-06-14
- Calls: `POST …/authenticationtrees/nodes?_action=getAllTypes` (200 OK, 235
  result entries); `POST …/nodes/ScriptedDecisionNode?_action=schema`;
  `POST …/nodes/ScriptedDecisionNode?_action=template`;
  `GET /am/json/node-designer/node-type?_queryFilter=true` (200 OK, 1 result
  entry).
- Date: 2026-06-14
- Calls: `PUT …/authenticationtrees/nodes/ScriptedDecisionNode/{uuid}` create
  (201) and update (200); `PUT …/authenticationtrees/trees/test_push_probe`
  create (201) and update (200); byte-identical re-PUT kept the same
  content-derived `_rev`; `DELETE …/trees/test_push_probe` returned 200 and
  follow-up `GET` returned 404. Throwaway `test_push_probe` tree and node were
  cleaned up.
- Date: 2026-08-13
- Calls: next-gen scripted decision on `AIC-Rhino-Let-Probe` dumped
  `requestHeaders` / `requestParameters` (Origin and Referer arrive when the
  client sends them; `Host` is always the tenant). `callbacksBuilder.redirectCallback`
  returned a `RedirectCallback` on `POST …/authenticate`. An origin-gated script
  redirected only when `Origin` was the tenant. Hosted `/login/` JS chunks
  contain `handleRedirectCallback` → `location.assign`. Probe script restored
  afterwards.
- Date: 2026-08-14 — realm `alpha`, contributed by the sibling
  `terraform-provider-pingone-aic` project.
- Calls: `GET …/authenticationtrees/trees/{name}` for **every** tree in the
  realm (**36 trees, 178 tree nodes** in total — the individual reads, not just
  the `_queryFilter=true` list), key sets tallied per tree and per node:
  `uiConfig` keys `categories` (19) and `annotations` (6) and nothing else;
  `staticNodes` absent on 3 trees and holding only `x`/`y` in its 96 entries
  where present; the six-key node metadata set present on 178/178 with
  `version == "1.0"` on 178/178; `maximumIdleTime`, `maximumSessionTime` and
  `treeTimeout` on 0/36. Plus `PUT …/trees/{probe}` carrying
  `maximumIdleTime: 7`, `maximumSessionTime: 11`, `treeTimeout: 13` → **201**
  with all three echoed, follow-up `GET` echoing the same, and a `DELETE` whose
  follow-up `GET` returned **404**. Both probe objects (this tree and the script
  probe in `04-scripts.md`) were deleted and their removal confirmed, and the
  realm was re-listed afterwards.

## Source citations

- frodo-lib: `src/api/TreeApi.ts`, `src/api/NodeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/journeys.js`,
  `packages/fr-config-push/src/scripts/update-auth-trees.js`.

## Open questions

- Resolved 2026-06-14: full inventory of built-in `nodeType` strings is
  available from `POST …/authenticationtrees/nodes?_action=getAllTypes` (235
  entries on the sandbox).
