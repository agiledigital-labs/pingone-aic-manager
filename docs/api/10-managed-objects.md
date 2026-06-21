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
| Replace schema | `PUT` | `/openidm/config/managed` | Whole-document replace of `{ _id: "managed", objects: [...] }`. Mutate via read-modify-write; 200 on success. |
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

## Schema config writes (verified 2026-06-14)

`PUT /openidm/config/managed` is a whole-document replace. Mutations are
read-modify-write edits of `{ "_id": "managed", "objects": [...] }`; all
sandbox PUTs on throwaway `test_*` objects returned 200. The API stores
`objects[]` entries verbatim — no field injection, normalisation, or
reordering was observed.

Config read-back is effectively immediate: after PUT returned 200, a fresh
GET reflected the change on the first poll (~164 ms later). This is strong
consistency for the stored config. The `managed_hooks` sync path still polls
when needed because it waits for hook source to go live in the running IDM
runtime, which is separate from config read-back.

| Shape | Accepted / observed |
|---|---|
| Minimal custom object | `{ "name": "...", "schema": { "type": "object", "title": "...", "properties": {}, "required": [], "order": [] } }`. Objects carry no `_id`/`$id`; the document's `_id: "managed"` is the only id. |
| Standard object marker | Ping-shipped standard objects (`alpha_`/`bravo_` × `user`, `role`, `organization`, `assignment`, `application`) have both top-level `type` and `meta` keys. Custom objects (`mock_*`, `alpha_lock`, `test_*`) have neither. |
| Scalar property | `{ "title": "...", "description": "...", "type": "string", "searchable": true, "viewable": true, "userEditable": true }` round-trips. |
| Single relationship | `{ "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] }`. `reversePropertyName`, `validate`, and explicit `_ref`/`_refProperties` are optional at config-write time. |
| Array of relationships | `{ "type": "array", "items": { "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] } }`. |
| Lifecycle hook | Top-level sibling of `schema`, e.g. `"onCreate": { "type": "text/javascript", "source": "..." }`. Round-trips verbatim and is immediately discoverable/pullable via `aic script list managed` / `aic script pull managed/<object>.<hook>`. |

No cross-object reverse-property validation runs on config write. A PUT with
`validate: true` and a `reversePropertyName` that did not exist on the target
object returned 200 and stored the property. Treat `validate` and
`reverseRelationship` as runtime relationship-integrity flags, not schema
write gates. One-way relationships are accepted; fully bidirectional pairs
also round-trip.

There is no server-side rename or delete primitive for schema config: both are
whole-document RMW edits. `schema.order` is independent of
`schema.properties`, and `schema.required` is independent too; the API does
not auto-prune either list when a property is removed or renamed.

## Hook scripts (verified 2026-06-13)

Hook keys **observed in use** on the sandbox schema: `onCreate`, `onUpdate`,
`onDelete`, `postCreate`, `postUpdate`, `postDelete`. Two storage forms
coexist on the same tenant:

- **Inline** — `{ "type": "text/javascript", "source": "…" }`. Round-trips
  through `PUT /openidm/config/managed`; this is the form tenant tooling can
  edit.
- **File-backed** — `{ "type": "text/javascript", "file": "roles/onDelete-roles.js" }`
  (stock Ping hooks). The config API provides no way to read or write the
  referenced file, so tooling must treat file-backed hooks as **read-only
  markers** — never convert them to inline or drop them on push.

Sync tooling should detect hooks **by value shape** (any object property with
`type` + `source`/`file`), not by a hardcoded key list — which event keys
beyond the six observed are accepted/fired remains an open question.

### Hook runtime bindings (live probe, 2026-06-13)

Probed by installing temporary `onCreate`/`onUpdate` hooks on the scratch
type `alpha_lock`, dumping bindings into the created record, then restoring
the schema byte-identical and deleting the probe records. Full sanitized
results: [`bindings/managed-hooks-idm.json`](bindings/managed-hooks-idm.json).

| Binding | onCreate | onUpdate | Notes |
|---|---|---|---|
| `object` | draft record, **mutable** | new state, mutable | writes persist |
| `oldObject` | `null` | previous record state | |
| `newObject` | `=== object` | `=== object` | alias, verified |
| `request` | CreateRequest (`method:"create"`, `content`, `newResourceId`, …) | `method:"update"` | |
| `context` | full context chain (http headers ⚠ incl. Authorization, security, oauth2) | same | treat as sensitive |
| `resourceName` | `ResourcePath` (`managed/<type>/<id>`) | same | only Java-classed binding |
| `openidm`, `logger`, `identityServer`, `require` | present | present | same surface as endpoint scripts |

**Fatal gotcha:** `for (var k in this)` at hook top level throws — the
triggering request gets **HTTP 500** and the write rolls back. Any uncaught
hook exception surfaces the same way. Probe with `typeof <name>`, never by
enumerating scope.

## Examples

```bash
# Read the entire managed schema
$SCRIPTS/verify-endpoint.sh "/openidm/config/managed"

# Query users (alpha realm)
$SCRIPTS/verify-endpoint.sh "/openidm/managed/alpha_user?_queryFilter=true&_pageSize=1"
```

## Record querying + change detection (drives the `idmstore` sync feature)

Verified 2026-06-20/06-21 against sandbox `alpha_user`/`bravo_user`/`alpha_role`.

**Records carry no timestamp.** A managed-object instance returns only `_id`
and `_rev` plus its declared properties — there is **no** `lastModified`/
`created` field on the record. `_rev` is a per-object change counter (suffix
e.g. `…-34`), **not** a global "changed-since" cursor; do not use it to detect
which records changed across a collection.

**The change signal lives in a `_meta` relationship — user objects only.**
`alpha_user`/`bravo_user` each have a `_meta` relationship to a sidecar managed
object `managed/<type>meta` (e.g. `alpha_usermeta`) carrying `createDate` and
`lastChanged: { date }` (ISO-8601). Fetch it inline with
`_fields=*,_meta/_id,_meta/lastChanged`. The sidecar collection is **directly
queryable and sortable** by that timestamp:

```
GET /openidm/managed/alpha_usermeta
  ?_queryFilter=lastChanged/date ge "2026-06-01T00:00:00Z"
  &_sortKeys=-lastChanged/date&_fields=_id,lastChanged          → 200, ordered
```

This is the incremental-sync watermark source. **`alpha_rolemeta` → 404**:
roles, organizations, assignments, applications have **no** `*meta` sidecar and
no per-record timestamp, so they have no incremental signal — re-pull them in
full (or use the `idm-activity` audit log, which needs a Log API key, see
`08-logs.md`).

**You cannot filter/sort the parent object by the related sidecar.** Relationship
traversal in the query is unsupported: `_queryFilter=_meta/lastChanged/date gt …`
returns `resultCount: 0`, and `_sortKeys=-_meta/lastChanged` → **HTTP 500**
(`ByteString.toBase64String() … normalized is null`). So: query the *sidecar*
for changed ids, and keep a local `meta_id ↔ record _id` map (built when records
are pulled with `_fields=…,_meta/_id`). Detect creates/deletes with a cheap
`_fields=_id` id-list diff against the local store.

**Paging (verified 2026-06-21).** Unlike AM scripts, managed lists return a
**usable `pagedResultsCookie`** — pass it back as `_pagedResultsCookie` to walk
pages sequentially to completion. Bulk record reads must use this cursor, not
`_pagedResultsOffset`: offset paging re-runs the query for each page, can skip
or duplicate records under concurrent backend changes, and deep offsets are
costly on DJ. Do **not** use `totalPagedResults` as a completeness bound: empty
objects may return `totalPagedResults: -1` with policy `NONE` even when
`_totalPagedResultsPolicy=EXACT` is requested, and populated objects may return
policy `ESTIMATE`. Treat counts as optional progress hints only. Cursor walks
end on an absent/empty cookie, including an empty first page.

## Quirks

- **PUT is "replace entire schema"** — there's no partial patch. Read,
  modify the relevant `objects[]` entry, write back. Object entries store
  verbatim; no server field injection, normalisation, or reordering was
  observed.
- **`_rev`-less at the top level** — concurrency control is at the per-record
  level for instance data, not schema. Two concurrent schema edits will
  last-write-wins.
- **Inline vs file scripts.** Tooling should normalize to file form for
  storage; the API accepts either.
- **`repo.ds`** is the source of truth for which managed properties are
  indexed in DJ. Adding a searchable property requires updating both
  `managed` and `repo.ds`.
- **Config read-back is immediate; hook runtime activation can lag.** A fresh
  `GET /openidm/config/managed` reflected a 200'd PUT on the first poll
  (~164 ms, 2026-06-14). Separately, the running hook registry can catch up a
  beat later (observed 2026-06-13: a record created right after a 200'd
  schema PUT still fired the *previous* hook source; ~5s later the new source
  was live). Push tooling can re-read config to confirm storage, but should
  not fire-and-verify hooks immediately after a push.
- **`schema.order` / `schema.required` are manual.** The config API does not
  auto-prune them when properties are removed or renamed.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-06-21 (record querying + change detection: no record timestamp;
  `_meta`→`<type>meta` sidecar with `lastChanged/createDate`, queryable/sortable
  on the sidecar but **not** via parent traversal — `_meta/...` filter → 0, sort
  → 500; `alpha_rolemeta` 404 / non-user objects have no sidecar; cursor paging
  works and is the required bulk-read path; `EXACT` count policy). 2026-06-14
  (managed-config write behaviour, custom object/property/relationship/hook
  round-trips, no reverse-property validation); 2026-06-13
  (hook inventory, hook bindings probe, schema PUT round-trip + application
  lag); 2026-06-09 (create-if-absent test); 2026-05-17 (schema read).
- Calls: `GET /openidm/config/managed` (200); `PUT /openidm/config/managed`
  (200, full-document replace; object entries stored verbatim; fresh GET
  reflected changes on first poll, ~164 ms); relationship PUT with
  `validate: true` + dangling `reversePropertyName` (200, stored);
  `PUT /openidm/managed/alpha_lock/{id}` + `If-None-Match: *` (201/412);
  bare `PUT` update (200, fires onUpdate); `DELETE` (200);
  `GET …?_queryFilter=true&_fields=_id` (200).

## Source citations

- frodo-lib: `src/api/cloud/IdmApi.ts` (and `src/ops/IdmConfigOps.ts`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/managed.js`,
  `packages/fr-config-push/src/scripts/update-managed-objects.js`.

## Open questions

- Which hook event keys beyond the six observed in use (`onCreate`,
  `onUpdate`, `onDelete`, `postCreate`, `postUpdate`, `postDelete`) are
  accepted **and fired** by AIC (`onValidate`, `onRead`, `onRetrieve`,
  `onStore`, `onSync`, …). Partially resolved 2026-06-13: tooling
  sidesteps this by detecting hooks by value shape rather than key list;
  verify firing per-key before documenting any of the others as supported.
- Hook bindings for `onDelete`/`post*` hooks are assumed to match the
  verified `onCreate`/`onUpdate` surface; not yet probed.
