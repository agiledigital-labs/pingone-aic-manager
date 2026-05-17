# 10 — IDM managed objects

## Purpose
"Managed objects" are IDM's domain entities: users, applications, roles,
assignments, etc. The schema is editable per-tenant (you can add fields,
events, scripts). Documented because journeys reference `managed/alpha_user`
and similar, and because we may need to expose schema/hook editing later.

## Authentication
Service-account bearer. Scope: `fr:idm:*`.

## Endpoints (tenant-global; **not** realm-scoped under `/realms/...`)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| Read schema (all) | `GET` | `/openidm/config/managed` | Returns `{ _id: "managed", objects: [...] }`. |
| Replace schema | `PUT` | `/openidm/config/managed` | Body must be the full `managed` config. |
| Read repo mapping | `GET` | `/openidm/config/repo.ds` | Maps managed properties to DJ attributes. |
| Read object instance | `GET` | `/openidm/managed/{type}/{id}` | Per-record. `{type}` e.g. `alpha_user`. |
| List records | `GET` | `/openidm/managed/{type}?_queryFilter=true` | CREST. |

## Naming convention

Object types are realm-prefixed: `alpha_user`, `alpha_role`, `alpha_application`,
`bravo_user`, etc. The `alpha_` / `bravo_` prefix is the realm scoping
mechanism on the IDM side (whereas AM uses URL path segments).

## Object shape (schema, abbreviated, from sandbox)

```json
{
  "_id": "managed",
  "objects": [
    {
      "name": "alpha_application",
      "iconClass": "fa fa-database",
      "schema": {
        "$schema": "http://forgerock.org/json-schema#",
        "description": "Application Object",
        "icon": "fa-folder",
        "order": ["_id","name","description","url","icon","mappingNames","owners","roles","members","authoritative","connectorId", /* … */],
        "properties": { /* per-field type, constraints, viewable, searchable, etc. */ },
        "required": [/* … */]
      },
      "onCreate": { "type": "text/javascript", "source": "…" },
      "onUpdate": { "type": "text/javascript", "file": "scripts/managed/onUpdate-user.js" },
      "onDelete": { /* … */ }
    },
    /* alpha_user, alpha_role, alpha_assignment, bravo_user, ... */
  ]
}
```

Hooks (`onCreate`, `onUpdate`, `onDelete`, `onValidate`, `onRead`, etc.) can
be either inline (`{ "type": "...", "source": "..." }`) or file-backed
(`{ "type": "...", "file": "..." }`). For on-disk storage we prefer file form
(matches fr-config-manager's layout: separate `.js` files per hook).

## Examples

```bash
# Read the entire managed schema
$SCRIPTS/verify-endpoint.sh "/openidm/config/managed"

# Query users (alpha realm)
$SCRIPTS/verify-endpoint.sh "/openidm/managed/alpha_user?_queryFilter=true&_pageSize=1"
```

## Quirks

- **PUT is "replace entire schema"** — there's no partial patch. Read,
  modify the relevant `objects[]` entry, write back.
- **`_rev`-less at the top level** — concurrency control is at the per-record
  level for instance data, not schema. Two concurrent schema edits will
  last-write-wins.
- **Inline vs file scripts.** Tooling should normalize to file form for
  storage; the API accepts either.
- **`repo.ds`** is the source of truth for which managed properties are
  indexed in DJ. Adding a searchable property requires updating both
  `managed` and `repo.ds`.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET /openidm/config/managed` (200 OK).

## Source citations

- frodo-lib: `src/api/cloud/IdmApi.ts` (and `src/ops/IdmConfigOps.ts`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/managed.js`,
  `packages/fr-config-push/src/scripts/update-managed-objects.js`.

## Open questions

- Full hook event names (`onCreate`, `onUpdate`, `onDelete`, `onValidate`,
  `onRead`, `onRetrieve`, `onStore`, `onSync`, `postCreate`, `postUpdate`, …)
  and which are tenant-editable in AIC. fr-config-manager has a list; copy
  after verifying with a `GET /openidm/config/managed` and grepping hook keys.
