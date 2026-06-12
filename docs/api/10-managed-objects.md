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
| Create (client-set `_id`) | `PUT` | `/openidm/managed/{type}/{id}` + `If-None-Match: *` | Atomic create-if-absent. 201 on create (returns instance `_rev`); **412 "Entry Already Exists"** if the id is taken. |
| Delete instance | `DELETE` | `/openidm/managed/{type}/{id}` | 200 on delete. |

## Create-if-absent observations

`PUT /openidm/managed/{type}/{id}` with header `If-None-Match: *` creates the
record **only if it doesn't exist**. The uniqueness check is enforced by the DJ
directory backend as an atomic LDAP add (the 412 body references the DN, e.g.
`uid=...,ou=role,o=alpha,o=root`), **not** by an IDM read-then-write when the
precondition is honored.

Sandbox verification 2026-06-09: 8 rounds × 25 truly-parallel PUTs of the same
`_id` → **exactly one 201 and 24×412 every round, zero anomalies**. This makes a
"lock" managed object (e.g. `_id = "${object}-${id}"`) look viable in a
single-node sandbox. Production cluster testing later disproved the `PUT` +
`If-None-Match` form as a reliable distributed lock; see the caveat below.
Reproduce the sandbox test with `scripts/experiment-lock-uniqueness.sh`.

Note: managed-object **instances** DO carry `_rev` (unlike scripts/ESVs and
unlike the `managed` *schema* config, which is `_rev`-less). The 412 here is
driven by `If-None-Match: *`, independent of `_rev`.

### Three create paths — only two are "create-only" (verified 2026-06-10)

| Path | Exists already → | Notes |
|------|------------------|-------|
| `PUT /managed/{t}/{id}` (no header) | **200, silent UPDATE** | CREST maps bare PUT to create-or-**update** (upsert). |
| `PUT /managed/{t}/{id}` + `If-None-Match: *` | 412 | Create-only *iff the precondition is honored* — see caveat. |
| `POST /managed/{t}?_action=create` (`_id` in body) | 412 "Entry Already Exists" | CREST `CreateRequest`; **no** update fallback. |
| `openidm.create(container, id, content)` (script) | throws `PreconditionFailedException` "Entry Already Exists" | Same `CreateRequest`. `id=null` → server-assigned UUID. **Never** updates. |

**Caveat — `If-None-Match: *` is NOT a safe distributed lock in production.**
Observed in a clustered prod tenant: 20 concurrent `PUT … If-None-Match: *` of
one `_id` returned **1×201, 4×200, 15×412** — the four 200s were *silent
updates*, i.e. the precondition was not enforced for those requests (LB/proxy
dropping the conditional header, or differing CREST routing). Four extra callers
got a 2xx "success" and would each have believed they held the lock. The
single-node sandbox cannot reproduce this (always 1×201 / N−1×412).

Because `POST ?_action=create` and `openidm.create` are `CreateRequest`s with
**no upsert path**, they cannot silently 200-update — a duplicate always errors.
They remain subject, in principle, to a DS multi-master add-add replication
conflict (two replicas both accept the add before replicating), which no
single-node test reproduces. Prefer create-based acquisition over PUT-upsert,
but do not treat any managed-object create as a hard mutex in a replicated
deployment without further validation.

### `managed/alpha_lock` advisory-lock type (sandbox, 2026-06-10)

A minimal custom type `alpha_lock` was added to the sandbox `managed` schema for
the reconById-serialisation lock: fields `lockKey`/`owner`/`acquiredAt`/
`expiresAt` (epoch-ms numbers), **no hooks and no sync mapping** so creating a
lock has no side effects. `_id` is the lock key (e.g. `${mapping}-${objectId}`).
The full acquire/retry/auto-expire/owner-fenced-release/lease-renew template is
`scripts/idm-recon-lock.template.js`; every path was exercised in-engine via a
scripted-endpoint harness (acquire/release, finally-on-throw, contention 503,
stale reclaim, owner-fenced release, lease renewal + fence, 12-way parallel
serialisation). The open question (is `openidm.create` reliably exclusive in
clustered prod?) is with Ping; the retry+expiry makes the template usable either
way but is not a hard multi-master mutex.

Exact duplicate-create error, as caught in an IDM script (verified 2026-06-10):
a Rhino-wrapped Java exception — `e.name === "JavaException"`,
`e.javaException.getClass().getName() === "org.forgerock.json.resource.PreconditionFailedException"`,
`e.message` starts `org.forgerock.json.resource.PreconditionFailedException: Entry Already Exists: The entry 'uid=<id>,ou=alpha_lock,ou=managed,dc=openidm,dc=example,dc=com' …`,
and **`e.code` is undefined** (there is no numeric code property — match on the
class or the message, not a code). `getClass()` reflection is available in IDM
scripts (unlike AM next-gen), so the class is the most precise discriminator.

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
- Date: 2026-06-09 (sandbox create-if-absent test); 2026-05-17 (schema read).
- Calls: `GET /openidm/config/managed` (200 OK);
  `PUT /openidm/managed/alpha_role/{id}` + `If-None-Match: *`
  (201 create, 412 on duplicate); `DELETE …` (200).

## Source citations

- frodo-lib: `src/api/cloud/IdmApi.ts` (and `src/ops/IdmConfigOps.ts`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/managed.js`,
  `packages/fr-config-push/src/scripts/update-managed-objects.js`.

## Open questions

- Full hook event names (`onCreate`, `onUpdate`, `onDelete`, `onValidate`,
  `onRead`, `onRetrieve`, `onStore`, `onSync`, `postCreate`, `postUpdate`, …)
  and which are tenant-editable in AIC. fr-config-manager has a list; copy
  after verifying with a `GET /openidm/config/managed` and grepping hook keys.
