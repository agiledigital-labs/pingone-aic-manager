# 10 — IDM managed objects

Implemented in: `src/managed/`

## Purpose

"Managed objects" are IDM's domain entities: users, applications, roles,
assignments, etc. The schema is editable per-tenant (you can add fields, events,
scripts). Documented because journeys reference `managed/alpha_user` and
similar, and because we may need to expose schema/hook editing later.

## Authentication

Service-account bearer. Scope: `fr:idm:*`.

## Endpoints (tenant-global; **not** realm-scoped under `/realms/...`)

| Op                        | Method   | Path                                                | Notes                                                                                                                |
| ------------------------- | -------- | --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Read schema (all)         | `GET`    | `/openidm/config/managed`                           | Returns `{ _id: "managed", objects: [...] }`.                                                                        |
| Replace schema            | `PUT`    | `/openidm/config/managed`                           | Whole-document replace of `{ _id: "managed", objects: [...] }`. Mutate via read-modify-write; 200 on success.        |
| Read repo mapping         | `GET`    | `/openidm/config/repo.ds`                           | Maps managed properties to DJ attributes.                                                                            |
| Read object instance      | `GET`    | `/openidm/managed/{type}/{id}`                      | Per-record. `{type}` e.g. `alpha_user`.                                                                              |
| List records              | `GET`    | `/openidm/managed/{type}?_queryFilter=true`         | CREST.                                                                                                               |
| Create (client-set `_id`) | `PUT`    | `/openidm/managed/{type}/{id}` + `If-None-Match: *` | Atomic create-if-absent. 201 on create (returns instance `_rev`); **412 "Entry Already Exists"** if the id is taken. |
| Delete instance           | `DELETE` | `/openidm/managed/{type}/{id}`                      | 200 on delete.                                                                                                       |

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
unlike the `managed` _schema_ config, which is `_rev`-less). The 412 here is
driven by `If-None-Match: *`, independent of `_rev`.

### Three create paths — only two are "create-only" (verified 2026-06-10)

| Path                                                        | Exists already →                                            | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `PUT /managed/{t}/{id}` (no header)                         | **200, silent UPDATE**                                      | CREST maps bare PUT to create-or-**update** (upsert).                                                                                                                                                                                                                                                                                                                                                                                            |
| `PUT /managed/{t}/{id}` + `If-None-Match: *`                | 412                                                         | Create-only _iff the precondition is honored_ — see caveat.                                                                                                                                                                                                                                                                                                                                                                                      |
| `POST /managed/{t}?_action=create` (`_id` in body)          | 412 "Entry Already Exists"                                  | CREST `CreateRequest`; **no** update fallback.                                                                                                                                                                                                                                                                                                                                                                                                   |
| `openidm.create(container, id, content)` (script)           | throws `PreconditionFailedException` "Entry Already Exists" | Same `CreateRequest`. `id=null` → server-assigned UUID. **Never** updates.                                                                                                                                                                                                                                                                                                                                                                       |
| `openidm.read("managed/{t}/{id}")` (script), record missing | **returns `null`** (does NOT throw)                         | Verified live 2026-07-17 (next-gen decision + LIBRARY). Also `null` for a missing managed-object **type** (`managed/zzz_no_such_type/x`). Only a genuine read error (500/403/transport) throws. So a `try/catch` around `openidm.read` catches only real failures, not normal misses — guard the miss with `if (!rec) …` and reserve `logger.warn` for the `catch`. Probe: `scripts/rhino-script-tester/fixtures/lib-openidm-miss-probe.lib.js`. |

**Caveat — `If-None-Match: *` is NOT a safe distributed lock in production.**
Observed in a clustered prod tenant: 20 concurrent `PUT … If-None-Match: *` of
one `_id` returned **1×201, 4×200, 15×412** — the four 200s were _silent
updates_, i.e. the precondition was not enforced for those requests (LB/proxy
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
`e.message` starts
`org.forgerock.json.resource.PreconditionFailedException: Entry Already Exists: The entry 'uid=<id>,ou=alpha_lock,ou=managed,dc=openidm,dc=example,dc=com' …`,
and **`e.code` is undefined** (there is no numeric code property — match on the
class or the message, not a code). `getClass()` reflection is available in IDM
scripts (unlike AM next-gen), so the class is the most precise discriminator.

## Naming convention

Managed-object types that hold realm-owned data are realm-prefixed:
`alpha_user`, `alpha_role`, `alpha_application`, `bravo_user`, etc. The `alpha_`
/ `bravo_` prefix is the realm-scoping mechanism for that data on the IDM side
(whereas AM uses URL path segments).

Tenant-global service or configuration data should not borrow a realm prefix.
Use a descriptive non-realm prefix that identifies the owning service instead,
for example `idr_name_variants` for the tenant-wide IDR name-variant table.

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
        "order": [
          "_id",
          "name",
          "description",
          "url",
          "icon",
          "mappingNames",
          "owners",
          "roles",
          "members",
          "authoritative",
          "connectorId" /* … */
        ],
        "properties": {
          /* per-field type, constraints, viewable, searchable, etc. */
        },
        "required": [
          /* … */
        ]
      },
      "onCreate": { "type": "text/javascript", "source": "…" },
      "onUpdate": {
        "type": "text/javascript",
        "file": "scripts/managed/onUpdate-user.js"
      },
      "onDelete": {
        /* … */
      }
    }
    /* alpha_user, alpha_role, alpha_assignment, bravo_user, ... */
  ]
}
```

## Schema config writes (verified 2026-06-14)

`PUT /openidm/config/managed` is a whole-document replace. Mutations are
read-modify-write edits of `{ "_id": "managed", "objects": [...] }`; all sandbox
PUTs on throwaway `test_*` objects returned 200. The API stores `objects[]`
entries verbatim — no field injection, normalisation, or reordering was
observed.

Config read-back is effectively immediate: after PUT returned 200, a fresh GET
reflected the change on the first poll (~164 ms later). This is strong
consistency for the stored config. The `managed_hooks` sync path still polls
when needed because it waits for hook source to go live in the running IDM
runtime, which is separate from config read-back.

| Shape                   | Accepted / observed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Minimal custom object   | `{ "name": "...", "schema": { "type": "object", "title": "...", "properties": {}, "required": [], "order": [] } }`. Objects carry no `_id`/`$id`; the document's `_id: "managed"` is the only id.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Standard object marker  | Ping-shipped standard objects carry a top-level `type` key; custom objects (`mock_*`, `alpha_lock`, `test_*`, `idr_*`) have neither `type` nor `meta`. **Correction (verified 2026-07-27): only the `*_user` objects also carry `meta`** — `role`/`organization`/`assignment`/`application` have `type` but **no** `meta`. So the reliable "is Ping-shipped" discriminator is the **presence of top-level `type` alone**, NOT `type` + `meta` together. (`crate::managed::state::object_class` keyed on both markers until 2026-07-31 and so misclassified role/org/assignment/application as `Custom`, handing their shipped fields rename/retype/delete rights; it now keys on `type` alone.) |
| Scalar property         | `{ "title": "...", "description": "...", "type": "string", "searchable": true, "viewable": true, "userEditable": true }` round-trips.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Enum-constrained scalar | `{ "type": "string", "title": "...", "enum": ["new", "done"], "options": { "enum_titles": ["Brand new", "All done"] }, "searchable": …, "viewable": …, "userEditable": … }`. **`enum` is a constraint on a scalar, not a distinct property type** — the property keeps its `type` and gains a sibling `enum` array. Round-trips verbatim, including optional `options.enum_titles` display labels. Also works on `"type": "number"` (`enum: [1,2,3]`) and on an array's items (`{"type":"array","items":{"type":"string","enum":[…]}}`). **Enforced on record write**, not just UI metadata — see "Enum constraints" below. Verified 2026-07-31.                                                |
| Single relationship     | `{ "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] }`. `reversePropertyName`, `validate`, and explicit `_ref`/`_refProperties` are optional at config-write time.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Array of relationships  | `{ "type": "array", "items": { "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] } }`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Lifecycle hook          | Top-level sibling of `schema`, e.g. `"onCreate": { "type": "text/javascript", "source": "..." }`. Round-trips verbatim and is immediately discoverable/pullable via `aic script list managed` / `aic script pull managed/<object>.<hook>`.                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

### Enum constraints (verified 2026-07-31)

`enum` is a constraint on a scalar, not a fourth property type. The property
keeps `"type": "string"` (or `number`, or `array` with the constraint on
`items`) and gains a sibling `enum` array. Optional display labels go in
`options.enum_titles`, positionally matched to `enum`. All of these round-trip
through `PUT /openidm/config/managed` verbatim.

**The constraint is enforced on record write.** A value outside the set is
rejected `403 Forbidden` / `"Policy validation failed"` with a machine-readable
detail naming the property and the permitted values:

```json
{
  "code": 403,
  "message": "Policy validation failed",
  "detail": {
    "failedPolicyRequirements": [
      {
        "property": "status",
        "policyRequirements": [
          {
            "policyRequirement": "VALID_ENUM_VALUE",
            "params": { "enumValues": ["new", "done"] }
          }
        ]
      }
    ]
  }
}
```

Enforcement applies to array items and numeric enums too, not just strings.

**Narrowing an enum on a populated field is a data-affecting change.** Removing
a value that existing records still hold does _not_ rewrite or invalidate them,
and it does not break everything — but it breaks the most common update idiom:

| Operation on a record holding a now-removed value | Result                                                        |
| ------------------------------------------------- | ------------------------------------------------------------- |
| `GET` the record                                  | 200 — the stale value reads back                              |
| `PATCH` an **unrelated** field                    | 200 — policy checks only the properties being written         |
| `PATCH` the enum field to the removed value       | 403                                                           |
| **`PUT` the whole record as read back**           | **403** — the unchanged stale value is re-submitted and fails |

So read-modify-write of an untouched record starts failing, while targeted
patches keep working. Widening an enum is safe; narrowing one needs the affected
records migrated first. Anything that offers enum editing should treat removing
a value differently from adding one.

The CLI can set or clear these constraints with `aic managed field add` and
`aic managed field edit`; see [`docs/CLI.md`](../CLI.md) for its narrowing gate.
The whole table above was re-confirmed against the sandbox through those
commands on 2026-08-01, including the `403` on the whole-record `PUT`.

Two further observations from that run:

- **Whole floats are normalised to integers.** A config `PUT` sending
  `"enum": [1.0, 2.0]` reads back as `[1, 2]`, and a record holding integer `2`
  validates against it. So the float form is not an enforcement hazard — but it
  does mean a writer that emits `1.0` never sees its own bytes again, which
  would show as permanent drift to any content-snapshot comparison (§5). Emit
  integers for whole numbers.
- **Schema changes are not immediately effective for record policy.** A widening
  `PUT` to `config/managed` followed straight away by a record write was
  rejected `403` against the _old_ enum. A second attempt moments later
  succeeded. Don't write a record immediately after changing the constraint that
  governs it, and don't read a 403 there as a failed schema write — re-`GET` the
  config to see which one actually happened.

No cross-object reverse-property validation runs on config write. A PUT with
`validate: true` and a `reversePropertyName` that did not exist on the target
object returned 200 and stored the property. Treat `validate` and
`reverseRelationship` as runtime relationship-integrity flags, not schema write
gates. One-way relationships are accepted; fully bidirectional pairs also
round-trip.

There is no server-side rename or delete primitive for schema config: both are
whole-document RMW edits. `schema.order` is independent of `schema.properties`,
and `schema.required` is independent too; the API does not auto-prune either
list when a property is removed or renamed.

### Property `default` and `required` (verified 2026-08-05)

A scalar property may carry a sibling `default`; it round-trips through
`PUT /openidm/config/managed` verbatim, and **the server applies it on record
create.** It is not UI-only prefill metadata. Probe: `test_defaults` with four
booleans, one of them `default: true` so an applied default is distinguishable
from an absent property.

Verified for every scalar shape the CLI can write — create the record omitting
the property, and it reads back holding the default:

| Property                                                                  | Record holds     |
| ------------------------------------------------------------------------- | ---------------- |
| `{"type": "string", "default": "hello"}`                                  | `"hello"`        |
| `{"type": "number", "default": 7}` / `"default": 0`                       | `7` / `0`        |
| `{"type": "boolean", "default": true}` / `false`                          | `true`/`false`   |
| `{"type": "array", "items": {…}, "default": ["a","b"]}` / `"default": []` | `["a","b"]`/`[]` |

For an array the `default` sits on the **outer** property, beside `items` — not
inside `items`.

**A type-mismatched `default` bricks the object with no error anywhere.**
`{"type": "boolean", "default": "nope"}` is accepted by the config `PUT` with
**200**, and the managed object then never comes live: every
`/openidm/managed/<object>` call returns **404 indefinitely** (still 404 after
two minutes of polling). Nothing server-side reports why. The blast radius is
that one object — every other managed type keeps serving, and removing the bad
default restores it. Because there is no server-side signal,
`aic managed field add`/`edit` coerce `--default` against the declared type and
refuse a mismatch locally. A default outside an `enum` is refused for a
different reason: the default is applied before policy, so every create would
fail `VALID_ENUM_VALUE`.

`default` and `required` answer different questions and neither blocks the
other:

| Write                                                          | Result                                                                        |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| **Create** omitting a property that has a `default`            | 201, and the record comes back holding the default — `false`/`true`/`0` alike |
| **Create** omitting a `required` property that has a `default` | 201 — the default is applied _before_ policy runs, so `REQUIRED` is satisfied |
| **Create** sending an explicit `null`                          | **403 `NOT_NULL`** (+ `VALID_TYPE` when the property is also `required`)      |
| Same, with the property removed from `schema.required`         | **still 403 `NOT_NULL`** — the null guard is independent of `required`        |
| **Update** (`PUT` whole record) omitting the property          | **200, and the stored value is dropped** — `required` does not catch this     |
| `PATCH remove` the property                                    | 403 `REQUIRED`                                                                |
| `PATCH replace` it with `null`                                 | 400 — rejected as an invalid JSON patch, before policy                        |

Three consequences worth carrying:

- **Removing `required` will not make a `default` "start working"** — the
  default already applies on create, and dropping `required` only loses the
  `PATCH remove` guard. It does not make an explicit `null` acceptable.
- **`null` is never a substitute for "unset".** A caller that sends `null` for
  an untouched field gets a 403 that reads like "you must supply a value", when
  the fix is to _omit_ the key and let the default land. Strip null-valued keys
  from create bodies rather than adding `default`s to satisfy them.
- **Defaults apply on create only, and whole-record `PUT` silently drops absent
  properties.** So a read-modify-write that loses a key doesn't re-default it
  and isn't refused either — it just erases the value. Adding a `required`
  property to an object that already has records is likewise not retroactive:
  existing records stay without it, and `GET` → `PUT` of one still returns 200.

### Relationship cardinality + bidirectional writes (verified 2026-07-27, corrected 2026-08-04)

Verified live against `test_from`/`test_to` (all six forward×reverse
combinations, created from the web console) and `test_obj2` (self-referential
pair + custom `_refProperties`). The console model:

**Forward cardinality is the property `type`.**

- **has one** → the property _is_ the relationship node:
  `{ "type": "relationship", <attrs>, "resourceCollection": [...], <reverse fields>, "properties": { "_ref", "_refProperties" } }`.
- **has many** → an array wrapper whose `items` is the relationship node:
  `{ "type": "array", <attrs>, "returnByDefault": false, "items": { "type": "relationship", "resourceCollection": [...], <reverse fields>, "properties": {...} } }`.

**Attribute placement.** `viewable` / `searchable` / `userEditable` /
`returnByDefault` live on the **outer** property (the array wrapper for
has-many, the relationship node for has-one). `required` is carried in the
object's `schema.required` (relationships _can_ be required —
`test_from.firstName` aside, `test_obj2.asdf` is in `required`). `validate`
lives on the **relationship node** (top-level for has-one, inside `items` for
has-many). `title`/`description` on the outer property.

**Reverse cardinality is the type of the reverse property _on the target
object_** — there is no reverse-cardinality field on the source. The three
reverse options:

| Reverse  | Source relationship node                                  | Reverse property on target                                           |
| -------- | --------------------------------------------------------- | -------------------------------------------------------------------- |
| has none | `reverseRelationship: false`, no `reversePropertyName`    | **none created** (one-way)                                           |
| has one  | `reverseRelationship: true`, `reversePropertyName: <rev>` | a `type:"relationship"` property named `<rev>`, cross-linked back    |
| has many | `reverseRelationship: true`, `reversePropertyName: <rev>` | a `type:"array"`/`items:relationship` property named `<rev>`, linked |

A bidirectional pair cross-links: the source's `reversePropertyName` is the
reverse property's key and vice-versa, and **both** carry
`reverseRelationship: true`. Self-referential relationships (source == target,
e.g. `test_obj2.asdf` ↔ `test_obj2.obj2`) put both properties on the one
object.

**No server-side cascade.** Creating/editing a relationship with a reverse must
write the reverse property on the target object itself — so an add/edit is a
**whole-document RMW touching two objects** (or one, when self-referential),
_not_ a single-object splice. Reconciliation on edit: if the reverse cardinality
drops to none, or the target is repointed, the tool must remove/move the old
reverse property (the server won't). `config/managed` PUT accepted every combo
with 200 and stored verbatim.

**`resourceCollection[].query` is required by the console, not by the API**
(corrected 2026-08-04 — see `99-quirks-and-open-questions.md`). The console
additionally writes `id`/`notifySelf`/`label`/`notify`/`propName`; those really
are cosmetic, and the minimal shapes in the table above round-trip. `query` does
not: omit it and every console page that renders the property fails to load.
Always write

```json
"query": { "fields": [], "queryFilter": "true", "sortKeys": [] }
```

on each `resourceCollection` entry, on **both** ends of a bidirectional pair.

**`_refProperties` are per-side.** The baseline is
`"_refProperties": { "type": "object", "properties": { "_id": { "type": "string" } } }`.
Custom relationship properties (e.g. `test_obj2.asdf._refProperties.relProp`)
are added only on the side where they were defined — the reverse side kept just
`_id`. Console shape for a custom one:
`{ "label": "...", "type": "string", "required": false, "propName": "<name>", "labelText": "..." }`;
minimal `{ "type": "...", "label": "..." }` is sufficient.

**Backlog:** the console does not appear to guard cardinality _reductions_ (e.g.
has-many → has-one) against existing instance data that would violate the new
bound; detecting that pre-write (query the relationship edges first) is a
possible future safety check, not done here.

### Object-type rename orphans records (verified 2026-07-27)

Renaming a custom object type by changing its `objects[].name` (X → Y) and
`PUT`ing the whole `config/managed` doc returns 200, but **the record backend is
keyed by the object name, and records are NOT migrated**. Observed on
`test_from` (3 records), sandbox:

1. Immediately after the PUT, `GET /openidm/managed/X` still returns the records
   (runtime activation lags a few seconds behind config read-back).
2. After activation (~seconds): `GET /openidm/managed/Y` serves an **empty**
   collection (`resultCount: 0`, no 404), and `GET /openidm/managed/X` →
   **404**. The original records are **orphaned** in the old backend —
   inaccessible via the managed API under either name.
3. The orphaning is **soft, not destructive**: renaming Y → X back (restore the
   original config) makes all original records reappear (same `_id`s) once the
   runtime re-activates. Records are never deleted by a config rename; they
   simply detach from the API surface while no configured type owns their
   backend name.

`config/repo.ds` contains **no** references to custom object names (checked
2026-07-27), so a custom-object rename does not require a `repo.ds` edit — but
the new name serves an empty collection and any searchable-property indexing
would need the usual `repo.ds` work.

Practical consequence for tooling: an object rename must repoint every inbound
relationship (`resourceCollection[].path == "managed/X"` → `"managed/Y"`, across
all objects) itself — no server-side cascade — and must warn that existing
records will be orphaned (recoverable only by renaming back). `config/sync`
mappings referencing `managed/X` are likewise not rewritten server-side.

### Deleting an object type (verified 2026-07-31)

Probed on the sandbox with `test_del_probe` (2 records) plus `test_del_src`
holding a relationship pointing at it; both removed afterwards and the config
re-listed to confirm nothing was left behind.

- **Removing the object's entry from `objects[]` and PUTting the document is the
  only delete route** — there is no object-level DELETE. `PUT` → **200**.
- **Records survive, exactly as with a rename.** After ~6.5s the runtime
  deactivates the type and `GET /openidm/managed/test_del_probe` → **404**.
  Restoring the object entry brings the collection back with **both original
  `_id`s intact** (activation ~3s). Deletion of a type is therefore _soft_: the
  records detach from the API surface while no configured type owns their
  backend name, and nothing is destroyed. This is what makes an undo of a
  config-level delete a genuine recovery.
- **A dangling relationship path is accepted.** PUTting a document in which
  `test_del_src.link` still has
  `resourceCollection[].path == "managed/test_del_probe"` after that object is
  gone returns **200** — no validation, no cascade. The server will happily hold
  a broken reference.

Practical consequence for tooling: a delete must sweep inbound relationships
itself (remove the property, and prune its key from `schema.order` and
`schema.required` — neither is auto-reconciled), because nothing server-side
will refuse or clean up the dangling path. `aic`'s `D` (delete object) does this
and lists the properties it will remove before confirming.

## Hook scripts (verified 2026-06-13)

Hook keys **observed in use** on the sandbox schema: `onCreate`, `onUpdate`,
`onDelete`, `postCreate`, `postUpdate`, `postDelete`. Two storage forms coexist
on the same tenant:

- **Inline** — `{ "type": "text/javascript", "source": "…" }`. Round-trips
  through `PUT /openidm/config/managed`; this is the form tenant tooling can
  edit.
- **File-backed** —
  `{ "type": "text/javascript", "file": "roles/onDelete-roles.js" }` (stock Ping
  hooks). The config API provides no way to read or write the referenced file,
  so tooling must treat file-backed hooks as **read-only markers** — never
  convert them to inline or drop them on push.

Sync tooling should detect hooks **by value shape** (any object property with
`type` + `source`/`file`), not by a hardcoded key list — which event keys beyond
the six observed are accepted/fired remains an open question.

### Hook runtime bindings (live probe, 2026-06-13)

Probed by installing temporary `onCreate`/`onUpdate` hooks on the scratch type
`alpha_lock`, dumping bindings into the created record, then restoring the
schema byte-identical and deleting the probe records. Full sanitized results:
[`bindings/managed-hooks-idm.json`](bindings/managed-hooks-idm.json).

| Binding                                          | onCreate                                                                   | onUpdate              | Notes                            |
| ------------------------------------------------ | -------------------------------------------------------------------------- | --------------------- | -------------------------------- |
| `object`                                         | draft record, **mutable**                                                  | new state, mutable    | writes persist                   |
| `oldObject`                                      | `null`                                                                     | previous record state |                                  |
| `newObject`                                      | `=== object`                                                               | `=== object`          | alias, verified                  |
| `request`                                        | CreateRequest (`method:"create"`, `content`, `newResourceId`, …)           | `method:"update"`     |                                  |
| `context`                                        | full context chain (http headers ⚠ incl. Authorization, security, oauth2) | same                  | treat as sensitive               |
| `resourceName`                                   | `ResourcePath` (`managed/<type>/<id>`)                                     | same                  | only Java-classed binding        |
| `openidm`, `logger`, `identityServer`, `require` | present                                                                    | present               | same surface as endpoint scripts |

**Fatal gotcha:** `for (var k in this)` at hook top level throws — the
triggering request gets **HTTP 500** and the write rolls back. Any uncaught hook
exception surfaces the same way. Probe with `typeof <name>`, never by
enumerating scope.

## Examples

```bash
# Read the entire managed schema
$SCRIPTS/verify-endpoint.sh "/openidm/config/managed"

# Query users (alpha realm)
$SCRIPTS/verify-endpoint.sh "/openidm/managed/alpha_user?_queryFilter=true&_pageSize=1"
```

## Record querying + change detection (drives the `idmstore` sync feature)

Verified 2026-06-20/06-21 against sandbox
`alpha_user`/`bravo_user`/`alpha_role`.

**Records carry no timestamp.** A managed-object instance returns only `_id` and
`_rev` plus its declared properties — there is **no** `lastModified`/ `created`
field on the record. `_rev` is a per-object change counter (suffix e.g. `…-34`),
**not** a global "changed-since" cursor; do not use it to detect which records
changed across a collection.

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

**You cannot filter/sort the parent object by the related sidecar.**
Relationship traversal in the query is unsupported:
`_queryFilter=_meta/lastChanged/date gt …` returns `resultCount: 0`, and
`_sortKeys=-_meta/lastChanged` → **HTTP 500**
(`ByteString.toBase64String() … normalized is null`). So: query the _sidecar_
for changed ids, and keep a local `meta_id ↔ record _id` map (built when
records are pulled with `_fields=…,_meta/_id`). Detect creates/deletes with a
cheap `_fields=_id` id-list diff against the local store.

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

**Query-filter negation (verified 2026-07-03).** AIC accepts symbolic CREST
negation with `!`, for example `_queryFilter=!(/description eq "lkj")`. It
rejects the word form `not (/description eq "lkj")` with HTTP 400
`"A value could not be parsed as a valid query filter"`.

**Query-filter operators (verified 2026-07-03).** Managed-object queries reject
`ne`: `_queryFilter=/_id ne "asdf"` returns HTTP 400 with
`"unrecognized or unsupported filter operator 'ne'"`. They also reject `in`:
`_queryFilter=/_id in ["asdf"]` returns HTTP 400
`"A value could not be parsed as a valid query filter"`. Array values are not a
fallback form either: `_queryFilter=/_id eq ["asdf"]` returns the same parse
error. Do not offer `ne` or `in` in script-template query validation.

## Quirks

- **PUT is "replace entire schema"** — there's no partial patch. Read, modify
  the relevant `objects[]` entry, write back. Object entries store verbatim; no
  server field injection, normalisation, or reordering was observed.
- **`_rev`-less at the top level** — concurrency control is at the per-record
  level for instance data, not schema. Two concurrent schema edits will
  last-write-wins.
- **Inline vs file scripts.** Tooling should normalize to file form for storage;
  the API accepts either.
- **`repo.ds`** is the source of truth for which managed properties are indexed
  in DJ. Adding a searchable property requires updating both `managed` and
  `repo.ds`.
- **Config read-back is immediate; hook runtime activation can lag.** A fresh
  `GET /openidm/config/managed` reflected a 200'd PUT on the first poll (~164
  ms, 2026-06-14). Separately, the running hook registry can catch up a beat
  later (observed 2026-06-13: a record created right after a 200'd schema PUT
  still fired the _previous_ hook source; ~5s later the new source was live).
  Push tooling can re-read config to confirm storage, but should not
  fire-and-verify hooks immediately after a push.
- **`schema.order` / `schema.required` are manual.** The config API does not
  auto-prune them when properties are removed or renamed.
- **`required` does not protect a whole-record `PUT`.** Omitting a required
  property on update returns 200 and erases the stored value; only
  `PATCH remove` is refused. Defaults are applied on create only, so nothing
  puts it back. See "Property `default` and `required`".

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-08-05 (property `default` + `required`: `default` round-trips and
  is applied server-side on create, satisfying `REQUIRED`; explicit `null` is
  403 `NOT_NULL` with or without `required`; whole-record `PUT` omitting a
  property drops it silently; `PATCH remove` is 403, `PATCH replace null` is
  400). 2026-08-01 (enum constraints exercised end-to-end through
  `aic managed field add`/`edit`: string+titles, numeric and array-`items`
  shapes; `VALID_ENUM_VALUE` enforcement on all three; the read-modify-write
  table re-confirmed; whole floats normalised to integers; schema changes lag
  before record policy sees them). 2026-07-31 (object-type delete: records
  detach and return intact on config restore; dangling relationship path
  accepted by the config PUT). 2026-06-21 (record querying + change detection:
  no record timestamp; `_meta`→`<type>meta` sidecar with
  `lastChanged/createDate`, queryable/sortable on the sidecar but **not** via
  parent traversal — `_meta/...` filter → 0, sort → 500; `alpha_rolemeta` 404 /
  non-user objects have no sidecar; cursor paging works and is the required
  bulk-read path; `EXACT` count policy). 2026-07-03 (query-filter negation: `!`
  accepted, word `not` rejected; query-filter operators: `ne` rejected as
  unsupported, `in` and array values rejected as parse errors). 2026-06-14
  (managed-config write behaviour, custom object/property/relationship/hook
  round-trips, no reverse-property validation); 2026-06-13 (hook inventory, hook
  bindings probe, schema PUT round-trip + application lag); 2026-06-09
  (create-if-absent test); 2026-05-17 (schema read).
- Calls: `GET /openidm/config/managed` (200); `PUT /openidm/config/managed`
  (200, full-document replace; object entries stored verbatim; fresh GET
  reflected changes on first poll, ~164 ms); relationship PUT with
  `validate: true` + dangling `reversePropertyName` (200, stored);
  `PUT /openidm/managed/alpha_lock/{id}` + `If-None-Match: *` (201/412); bare
  `PUT` update (200, fires onUpdate); `DELETE` (200);
  `GET …?_queryFilter=true&_fields=_id` (200);
  `GET …?_queryFilter=!(/description eq "lkj")&_pageSize=1` (200);
  `GET …?_queryFilter=not (/description eq "lkj")&_pageSize=1` (400);
  `GET …?_queryFilter=/_id ne "asdf"&_pageSize=1` (400);
  `GET …?_queryFilter=/_id in ["asdf"]&_pageSize=1` (400);
  `GET …?_queryFilter=/_id eq ["asdf"]&_pageSize=1` (400).

## Source citations

- frodo-lib: `src/api/cloud/IdmApi.ts` (and `src/ops/IdmConfigOps.ts`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/managed.js`,
  `packages/fr-config-push/src/scripts/update-managed-objects.js`.

## Open questions

- Which hook event keys beyond the six observed in use (`onCreate`, `onUpdate`,
  `onDelete`, `postCreate`, `postUpdate`, `postDelete`) are accepted **and
  fired** by AIC (`onValidate`, `onRead`, `onRetrieve`, `onStore`, `onSync`, …).
  Partially resolved 2026-06-13: tooling sidesteps this by detecting hooks by
  value shape rather than key list; verify firing per-key before documenting any
  of the others as supported.
- Hook bindings for `onDelete`/`post*` hooks are assumed to match the verified
  `onCreate`/`onUpdate` surface; not yet probed.
