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

| Path                                                        | Exists already →                                            | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PUT /managed/{t}/{id}` (no header)                         | **200, silent UPDATE**                                      | CREST maps bare PUT to create-or-**update** (upsert).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `PUT /managed/{t}/{id}` + `If-None-Match: *`                | 412                                                         | Create-only _iff the precondition is honored_ — see caveat.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `POST /managed/{t}?_action=create` (`_id` in body)          | 412 "Entry Already Exists"                                  | CREST `CreateRequest`; **no** update fallback.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `openidm.create(container, id, content)` (script)           | throws `PreconditionFailedException` "Entry Already Exists" | Same `CreateRequest`. `id=null` → server-assigned UUID. **Never** updates.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `openidm.read("managed/{t}/{id}")` (script), record missing | **returns `null`** (does NOT throw)                         | Verified live 2026-07-17 (next-gen decision + LIBRARY), and again 2026-08-17 in the **IDM custom-endpoint** context (`endpoint/example-managed-users`, unused UUID: the handler's own `null` branch fired). Also `null` for a missing managed-object **type** (`managed/zzz_no_such_type/x`). Only a genuine read error (500/403/transport) throws. So a `try/catch` around `openidm.read` catches only real failures, not normal misses — guard the miss with `if (!rec) …` and reserve `logger.warn` for the `catch`. Probe: `scripts/rhino-script-tester/fixtures/lib-openidm-miss-probe.lib.js`. |

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

**Corrected 2026-08-05 — `config/managed` is NOT read-your-writes consistent,
and a 200 on the `PUT` does not mean the change is durable.** This paragraph
previously claimed strong consistency on the strength of one observation (a
fresh GET reflecting a 200'd PUT ~164 ms later). That generalised wrongly.
`scripts/experiment-managed-lost-updates.sh` reproduces two failures on demand:
a read backing the next read-modify-write returns the _pre-write_ state, so the
next write silently discards the previous one; and a property confirmed present
immediately after its write is absent from a later read with no write in
between. Every call returns 2xx, and the observing reads bypass the local agent,
so this is the tenant's config store. See Q14 in
`99-quirks-and-open-questions.md`.

Two practical rules follow. **Do not write a new object's fields until its type
has instantiated** — without that wait, the first `field add` after
`object create` is lost every time. And **a write path that must not lose
changes has to re-read and confirm its own change landed**, with a bounded
retry, rather than trusting the status code; waiting for instantiation alone is
not sufficient, since a later write in the same sequence was still lost.

`aic` does this as of 2026-08-05: every `config/managed` write goes through
`api::replace_managed_confirmed`, which states what it expects to observe
(`ConfigConfirm`), re-reads, and retries six times over ~15s before failing with
an error rather than reporting a success it did not verify. That eliminates the
deterministic post-`object create` loss. It does **not** make long write
sequences safe — see Q14 for why the residue is platform-side and what to do
about it.

**Concurrent writers of the same document lose inserts.** Verified 2026-08-15:
two parallel Terraform creates (test_from + test_to copies) each GET-appended
and PUT; the second PUT won and `Terraform_test_from` vanished even though its
own confirm had passed. Serialising GET+mutate+PUT in one process recovered both
copies. Confirm-after-write is not enough when two mutators share `objects[]`.

The `managed_hooks` sync path already polls, because it waits for hook source to
go live in the running IDM runtime — which is a separate concern from config
read-back, and remains so.

| Shape                   | Accepted / observed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Minimal custom object   | `{ "name": "...", "schema": { "type": "object", "title": "...", "properties": {}, "required": [], "order": [] } }`. Objects carry no `_id`/`$id`; the document's `_id: "managed"` is the only id.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Standard object marker  | **Two markers, governing two different things — do not collapse them.** (a) Top-level `"type": "Managed Object"` marks a Ping-shipped _object_: all ten realm objects carry it, custom objects carry no `type` at all, and no other value of `type` occurs (verified 2026-08-07). This is the "cannot rename or delete the object" test — `crate::managed::state::is_ping_shipped_object`. (b) Top-level `meta` marks the objects whose _fields_ Ping constrains: **only `alpha_user` and `bravo_user`** carry it (verified 2026-07-27, re-confirmed 2026-08-07). This is the "additions need a `custom_` prefix, shipped fields are read-only" test — `object_class`. `role`/`organization`/`assignment`/`application` have `type` but no `meta`, and **accept ordinary un-prefixed properties** (confirmed by the maintainer against `alpha_organization`, 2026-08-07 — not independently re-run here), so they get full field freedom while the object stays protected. Keying field capability on `type` (2026-07-31 to 2026-08-07) forced `custom_` names the server never required. |
| Scalar property         | `{ "title": "...", "description": "...", "type": "string", "searchable": true, "viewable": true, "userEditable": true }` round-trips.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Enum-constrained scalar | `{ "type": "string", "title": "...", "enum": ["new", "done"], "options": { "enum_titles": ["Brand new", "All done"] }, "searchable": …, "viewable": …, "userEditable": … }`. **`enum` is a constraint on a scalar, not a distinct property type** — the property keeps its `type` and gains a sibling `enum` array. Round-trips verbatim, including optional `options.enum_titles` display labels. Also works on `"type": "number"` (`enum: [1,2,3]`) and on an array's items (`{"type":"array","items":{"type":"string","enum":[…]}}`). **Enforced on record write**, not just UI metadata — see "Enum constraints" below. Verified 2026-07-31.                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Single relationship     | `{ "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] }`. `reversePropertyName`, `validate`, and explicit `_ref`/`_refProperties` are optional at config-write time.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Array of relationships  | `{ "type": "array", "items": { "type": "relationship", "resourceCollection": [{ "path": "managed/<target>" }] } }`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Lifecycle hook          | Top-level sibling of `schema`, e.g. `"onCreate": { "type": "text/javascript", "source": "..." }`. Round-trips verbatim and is immediately discoverable/pullable via `aic script list managed` / `aic script pull managed/<object>.<hook>`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

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

**A managed type answering queries does not mean its property schema is
effective.** Immediately after a new object started returning 200 on
`GET /openidm/managed/<object>?_queryFilter=true`, record creates succeeded
while applying **no defaults at all** and enforcing **no policy** — an explicit
`null` on a `required` property returned 201. Seconds later the identical calls
behaved correctly. This is the record-policy lag noted under "Enum constraints"
extended to defaults, and it makes the obvious readiness check useless: the only
trustworthy signal is a default actually landing on a throwaway record. Tooling
that writes a schema and then immediately writes records against it must poll
for that, not for the type responding. (`scripts/experiment-managed-defaults.sh`
does exactly this.)

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

**A dangling reverse stays dangling, at runtime too** (verified 2026-08-21).
No cross-object validation runs on write (see above), so a
`reversePropertyName` naming a property the target never got is accepted and
stored — and Ping ships six of them: `alpha_application.members`/`owners`/
`roles` and their `bravo_` twins name `alpha_user.applications`,
`alpha_user.ownerOfApp` and `alpha_role.applications`, none of which exist in
`alpha_user`/`alpha_role`. The missing side is **not** inferred from the
source's declaration; it is absent from the runtime as well as the schema:

| Request                                             | Result              |
| --------------------------------------------------- | ------------------- |
| `GET /openidm/managed/alpha_user/<id>/roles`        | 200 `{"result":[]}` |
| `GET /openidm/managed/alpha_user/<id>/applications` | 404 Not Found       |
| `GET /openidm/managed/alpha_user/<id>/bogusField`   | 404 Not Found       |

A declared-but-uncreated reverse is indistinguishable from a field name nobody
ever mentioned. Consequences:

- **Generated types are right to omit it.** A missing member in
  `idm/types/managed/alpha_user.d.ts` reflects the tenant, not the generator.
  `aic workspace update` names every dangling pair it finds;
  `managed::ops::dangling_reverses` is the audit behind that warning.
- **There is no traversal from the target side.** Query the source instead:
  `managed/alpha_application?_queryFilter=/members/_ref eq "managed/alpha_user/<id>"`.
- **The relationship editor treats it as its own state**, labelled
  `declared, missing on target`, rather than folding it into one of the three
  choices. Reporting it as `has one` made an otherwise-untouched save create
  the property on the target; reporting it as `has none` made the same save
  strip the source's claim. Both are decisions the operator did not make, so an
  untouched reverse is re-written verbatim and stays dangling. Cycling the field
  leaves that state and cannot return to it — authoring a _new_ dangling
  declaration is not on offer — so clearing the claim, or creating the missing
  property, is an explicit choice either way.

Caveat on the evidence: the sandbox holds no `alpha_application` records, so the
404s establish that IDM registers no route for the property, not that a
populated read would skip it.

**An edit keeps what the editor does not model.** Both surfaces rebuild the
property out of typed fields (`managed::ops::source_property`), so every key
outside that vocabulary — `policies`, the console's `id`/`notify`/`notifySelf`,
the `required`/`labelText` on a custom `_refProperties` definition, a resource
collection entry pointing outside `managed/` — is carried across from the
property the edit opened rather than reconstructed (`ops::carry_unmodelled`).
The property also goes back to the `schema.order` slot it came from instead of
to the end. Keys the form _does_ model are still rewritten from the form, so
clearing a title clears it and repointing a target drops the entry it left.

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

**A custom type can carry the same hook shapes as the Ping-shipped objects.**
Verified 2026-08-15: a throwaway custom type `Terraform_lifecycle_probe` was
inserted via the usual whole-document RMW. It accepted:

- copies of `alpha_user.onCreate` / `onUpdate` (inline `require('onCreateUser')`
  / `require('onUpdateUser')` one-liners);
- a copy of `alpha_role.postCreate` (inline
  `require('roles/postOperation-roles').manageTemporalConstraints`);
- a file-backed `onDelete` pointing at the same product path
  `roles/onDelete-roles.js` as `alpha_role.onDelete`.

The config PUT stored those siblings verbatim. `alpha_user` / `alpha_role` /
`bravo_user` hook sources were byte-identical before and after. No records of
the probe type were created, so hook **runtime** was not re-fired. Empty
`globals: {}` (present on `bravo_user` hooks) is accepted on read and can be
omitted on write.

Live inventory the same day: of 26 types, only the Ping-shipped `*_user` and
`*_role` objects carried hooks. All 14 custom types (plus the two Terraform\_
relationship copies) had none. `bravo_user` onCreate/onUpdate are the only
inline hooks with a body longer than a one-line `require` (and the only ones
with a `globals` key, empty).

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

## Field selectors from a script (verified 2026-08-17)

Probed from a custom endpoint against `managed/alpha_user`, read-only. This is
what the `fields` projection in the type definitions encodes — `Projected<T, F>`
in `idm/types/common.d.ts`, `am/types/nextgen-common.d.ts` and
`typescript/framework/idm-globals.d.ts`.

| Call                                             | Keys returned                                          |
| ------------------------------------------------ | ------------------------------------------------------ |
| `openidm.read(path)`                             | `_id`, `_rev`, every non-relationship property         |
| `openidm.read(path, null, ["userName", "mail"])` | `_id`, `_rev`, `mail`, `userName` — and nothing else   |
| `openidm.read(path, null, ["_id"])`              | `_id`, `_rev` — **both**, having asked for one         |
| `openidm.read(path, null, ["*"])`                | same as no selector                                    |
| `openidm.query(coll, params)`                    | rows of `_id`, `_rev`, every non-relationship property |
| `openidm.query(coll, params, ["userName"])`      | rows of `_id`, `_rev`, `userName`                      |
| `openidm.query(coll, {…, _fields: "userName"})`  | identical to the line above                            |

Four things fall out of that:

- **`_id` and `_rev` come back whatever you ask for.** A selector cannot drop
  them, so a field-restricted read keeps the "`_id` and `_rev` plus properties"
  guarantee. This file previously implied the opposite and the type definitions
  hedged accordingly; both are corrected.
- **`openidm.query` takes a third `fields` argument**, exactly like `read`, with
  the same effect as `_fields` in the params. The IDM type definitions were
  missing it — a hand-written sandbox endpoint had been calling the
  three-argument form for months and failing `tsc` with "Expected 2 arguments,
  but got 3".
- **Relationships are absent unless requested.** No `manager`, `roles`,
  `memberOfOrg` … on an unselected read or query. Asking for `manager` (bare) or
  `manager/userName` (a path) both add the key.
- **A `parent/child` path returns the reference envelope PLUS the requested
  members**, not the bare reference. `_meta/lastChanged` returned:

  ```json
  {
    "_meta": {
      "_id": "c44d79cf-…",
      "_rev": "af504ab6-…",
      "_ref": "managed/alpha_usermeta/c44d79cf-…",
      "_refResourceCollection": "managed/alpha_usermeta",
      "_refResourceId": "c44d79cf-…",
      "_refResourceRev": "af504ab6-…",
      "_refProperties": { "_id": "7a5654ba-…", "_rev": "7a5654ba-…" },
      "lastChanged": { "date": "2025-01-25T06:37:03.551561402Z" }
    }
  }
  ```

  Note `_refResourceRev`, which the generated `RelationshipRef` interface does
  not declare, and that `_meta` is itself a relationship — to
  `managed/<realm>_usermeta`. The bare `_meta` form returned the envelope
  without the target's `_id`/`_rev`/`_refResourceRev`.

Two more observations from the same probe:

- **`openidm.query` does not return `remainingPagedResults`.** The envelope was
  `result`, `resultCount`, `pagedResultsCookie`, `totalPagedResults`,
  `totalPagedResultsPolicy` — five keys. IDM **requires** the sixth on a query
  handler's _return_ value, so a script cannot pass a query result straight back
  out of an endpoint. The type definitions now make that a compile error.
- **A record carries properties `config/managed` does not declare.**
  `assignedDashboard`, `displayName`, `isMemberOf` and `profileImage` came back
  on every `alpha_user` read but are absent from that object's
  `schema.properties`, so the generated interfaces cannot include them.
  Platform-injected; reach them through an index, and do not treat a generated
  interface as exhaustive.

### A selected member is PRESENT and `null`, not absent (verified 2026-08-18)

Probed from a throwaway `endpoint/aic-fields-probe` (deleted after), reading
`managed/alpha_user/4542b497-…` — user `demo3`, who has no `telephoneNumber`, no
`description` and no `manager` — with
`["userName", "telephoneNumber", "manager/userName", "description"]`:

```
_id="4542b497-…"  _rev="4aa64e7e-…"  userName="demo3"
telephoneNumber=null   description=null   manager=null
```

All three unset members came back as **present keys holding `null`**. So a
selector never omits what you asked for; it hands you `null` instead. Two
consequences:

- `"telephoneNumber" in projected` is **always true** for a requested member, so
  an existence check tells you nothing — check the value.
- the projected type must be a REQUIRED key with a NULLABLE value, which is what
  `SelectedMembers<T, F>` now encodes on all three surfaces. It used to be
  `Pick<T, …>`, which kept the schema's `?` and dropped the `null` — wrong in
  both directions at once.

The same probe on a **plain** read (no selector) shows the two halves differ: an
unset scalar is present and `null` (`telephoneNumber` was `null`), while an
unset relationship is **absent** (`manager` was `undefined`). The generated
interfaces say `telephoneNumber?: string`, which is therefore still optimistic
for a plain read — see the open question at the end of this file.

### `_pagedResultsCookie: null` is a 500 (verified 2026-08-18)

Passing an explicit null cursor to a script-side query throws:

```
org.forgerock.json.JsonValueException: /_pagedResultsCookie: Expecting a value
```

**Omit the key** to start at the first page. `cursor ?? null` in a handler is a
live opaque 500, and the type definitions invited it by declaring
`_pagedResultsCookie?: string | null`; they now say `string`, and neither they
nor the `QueryParams` index signature admits `null`.

The same probe pins the rest of the script-side paging envelope, for a query
with `_pageSize: 2` and no total requested:

```
rows=2  cookie="AAAAAAAAAKw="  resultCount=2
totalPagedResults=-1  totalPagedResultsPolicy="NONE"  remainingPagedResults=undefined
```

`-1` / `"NONE"` is IDM saying "I did not count", so forwarding those values out
of a query handler is honest; synthesising `totalPagedResults: rows.length`
instead (what building the envelope by hand tends to do) tells the caller the
collection has exactly one page. Confirmed end to end through
`endpoint/example-managed-users`: page one returned `['81055514', '81060852']`
and a cookie, and that cookie returned `['99999999', 'a']`.

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

### A property added to a Ping-shipped object must be named `custom_*` (verified 2026-08-28)

`bravo_user` ships with 70 properties, and AIC refuses to let you add a
71st under an arbitrary name:

```
400 Bad Request
Request content includes unprefixed attributes for bravo_user: ["myClients"]
```

The message says "unprefixed" and means one specific prefix. **`custom_` is the
one that works** — and the realm prefix does *not*: `bravo_zzprobeClients` is
rejected with the same error as the bare name, which makes the message
actively misleading if you guess from the object's own name. `custom_*`
properties are **not indexed**, so filter on them at your peril; expanding one
by `_id` (below) is unaffected.

This matters most for a **reverse relationship**: the reverse property lives on
the *target*, so a custom object pointing at `bravo_user` can only declare
`reversePropertyName: "custom_<something>"`.

```bash
aic managed relationship set bravo_client.accountManager \
    --target bravo_user --forward one --reverse many \
    --reverse-key custom_clients
```

### Two traps around a has-many reverse (verified 2026-08-28)

Both found while proving out an in-script relationship read; both are silent.

**A relationship created before the reverse property exists is not
retro-linked.** Add the reverse first, then the records. A link written while
the source declared `reverseRelationship: false` stays invisible from the target
side even after the reverse is added — the target read simply omits it, with no
error. Consistent with "no server-side cascade" above, but the failure mode is
missing data rather than a rejected write.

**After a `config/managed` write, a relationship expansion through AM's
`openidm` script binding returns nothing for a few seconds.** Not an error —
an empty list. A token-modification script that reads a user's roles or clients
to build a claim will happily mint a token with that claim **empty**, and every
downstream authorization decision then denies for no visible reason.
Recovered on its own within ~15s. This is the read-your-writes problem in
[Q14](99-quirks-and-open-questions.md) pointed at a consumer that cannot see it:
the REST API answered correctly throughout the same window, so the usual
`GET`-until-it-appears check does not detect it.

The practical rule: **after Terraform touches `config/managed`, wait before
exercising anything whose authorization depends on a relationship-derived
claim** — or have the script treat an empty expansion as a failure rather than
as "this user has none".

### A managed **record** creates with `PUT` and then refuses one (verified 2026-08-25)

`PUT /openidm/managed/{obj}/{id}` with a caller-chosen 36-char UUID creates the
record. A **second** `PUT` to the same id — the same body, even — is:

```
400 Bad Request
Not Allowed on RDN: Entry fr-idm-uuid=<id>,ou=user,o=bravo,… cannot be modified
because the change to attribute fr-idm-uuid would have removed a value used in
the RDN
```

A full replace rewrites every attribute, `fr-idm-uuid` included, and that
attribute is the directory entry's RDN. So "create or update with one idempotent
`PUT`" does not hold for managed records, however well it works for schema, for
resource types and for AM's OAuth2 clients. Probe with a `GET` and `PATCH` the
existing record instead:

```jsonc
PATCH /openidm/managed/bravo_user/{id}
[ {"operation": "replace", "field": "/mail",  "value": "…"},
  {"operation": "replace", "field": "/roles", "value": [{"_ref": "managed/bravo_role/…"}]} ]
```

A `replace` on `/roles` sets the whole relationship set, which is what a
converging provisioner wants.

This one bites late: a script that only ever runs after a teardown looks
idempotent for months, and fails the first time it is re-run against a tenant
that still has its users.

### Re-setting the password a user already has is a 400 (verified 2026-08-25)

```
400 Constraint Violation: The provided new password was found in the password
history for the user
```

…with a `passwordQualityAdvice` block naming the failing criterion. The realm
keeps a password history, so a converging update must **omit** `password`
rather than write the value it believes is already there. Only send a password
on create, or when actually rotating it.

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
- **A script-side `fields` list accepts relationship and `_meta` paths**
  (verified 2026-08-17). The `manager/userName` and `_meta/lastChanged` syntax
  was verified over REST above; the same list passed as the third argument to
  `openidm.read` inside a custom endpoint also returned 200. So
  `ManagedField<T>` in the type definitions is right to allow both forms for
  script calls as well as REST ones.

## Verified against

- Date: 2026-08-28 — realm `bravo`, throwaway `bravo_zzprobeClient` type with a
  has-one `accountManager` to `bravo_user` and a has-many `custom_zzprobeClients`
  reverse, three records, all deleted afterwards.
- Calls: `aic managed object create` / `relationship set` / `field add`; the
  reverse-key rejection reproduced with both the bare name and the realm prefix
  before `custom_` was accepted; three records created by
  `POST ?_action=create` (201 each) and read back through the reverse in one
  `GET managed/bravo_user/{id}?_fields=custom_zzprobeClients/*` (3 of 3). The
  same read then run from inside an `OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN`
  script on a throwaway client, which returned all three into a token claim.
  `capability-tokens`' `chain.sh` was run before and after; the run immediately
  after the schema deletes returned empty role claims and the next run, ~15s
  later, was correct — which is the staleness note above.

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-08-25 (managed **records** in `bravo`, writes: a repeat `PUT` to an
  existing `managed/bravo_user/{uuid}` is `400 Not Allowed on RDN`; a `PATCH`
  of the same fields is 200; a `PATCH` that re-sends the user's current password
  is `400 Constraint Violation` from the realm's password history, and the same
  `PATCH` without `/password` succeeds. Found by re-running a provisioning
  script that had only ever been exercised against a torn-down tenant.)
- Date: 2026-08-21 (dangling reverse properties: the six Ping-shipped
  half-declared relationships on `*_application` were confirmed absent from the
  target objects' `schema.properties`, and the relationship sub-resource route
  for one 404s exactly as an invented field name does — read-only, no records or
  config altered. See "A dangling reverse stays dangling".)
- Date: 2026-08-18 (two corrections to the 2026-08-17 field-selector results,
  both from a throwaway `endpoint/aic-fields-probe`, deleted afterwards, reading
  `managed/alpha_user` read-only. **One:** a selected member the record has no
  value for comes back as a PRESENT key holding `null`, not absent — `demo3`
  returned `telephoneNumber=null`, `description=null`, `manager=null` — while on
  a plain read an unset scalar is `null` and an unset relationship is absent. So
  `Pick<T, …>` was the wrong projection on all three surfaces and is now
  `SelectedMembers<T, F>`, a required key with a nullable value. **Two:**
  `_pagedResultsCookie: null` throws
  `JsonValueException: /_pagedResultsCookie: Expecting a value` — omit the key
  instead; the type said `string | null` and invited a live 500. Same probe
  pinned the script-side paging envelope: `totalPagedResults=-1`,
  `totalPagedResultsPolicy="NONE"`, `remainingPagedResults=undefined`, real
  cookie. Cursor paging then confirmed end to end through
  `endpoint/example-managed-users`.)
- Date: 2026-08-17 (field selectors from a script — see "Field selectors from a
  script" for the full table: `_id`/`_rev` returned whatever the selector says,
  `openidm.query` accepting a third `fields` argument, relationship-path
  expansion shape, `remainingPagedResults` absent from a script-side query
  result, and four runtime properties absent from `config/managed`. Plus two
  re-confirmations in the IDM **custom-endpoint** context, which the earlier
  probes did not cover: `openidm.read` on a missing record returns `null` —
  already established 2026-07-17 for next-gen decision and LIBRARY — and a
  script-side `fields` list accepts relationship and `_meta` paths. All probed
  from a throwaway endpoint plus `endpoint/example-managed-users` against
  `managed/alpha_user`, read-only, no records altered). 2026-08-15 (custom-type
  hook copy: `Terraform_lifecycle_probe` PUT accepted inline
  `onCreate`/`onUpdate`/`postCreate` plus file-backed
  `onDelete: roles/onDelete-roles.js`; originals
  `alpha_user`/`alpha_role`/`bravo_user` hook hashes unchanged; no records
  created so runtime not re-fired; inventory still 4 shipped types with hooks,
  no custom type had any). 2026-08-07 (read-only inventory of all 24 objects in
  `GET /openidm/config/managed`: the 10 Ping-shipped realm objects — `*_user`,
  `*_role`, `*_organization`, `*_assignment`, `*_application` across both realms
  — each carried top-level `"type": "Managed Object"`, and no other value of
  `type` appeared anywhere; the 14 custom objects (`alpha_lock`, `mock_*`,
  `test_*`, `Test_Object`, `idr_*`) carried **no** top-level `type` key at all;
  only `alpha_user` and `bravo_user` carried `meta`). 2026-08-05 (property
  `default` + `required`: `default` round-trips and is applied server-side on
  create, satisfying `REQUIRED`; explicit `null` is 403 `NOT_NULL` with or
  without `required`; whole-record `PUT` omitting a property drops it silently;
  `PATCH remove` is 403, `PATCH replace null` is 400; re-confirmed end-to-end
  through `aic managed field add --default` by
  `scripts/experiment-managed-defaults.sh`, which also established that a type
  answering queries is not a signal that its defaults or policies are effective
  yet). 2026-08-01 (enum constraints exercised end-to-end through
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
  `GET /openidm/managed/alpha_user/{id}/roles?_queryFilter=true` (200, empty
  result) vs `…/applications`, `…/ownerOfApp` and an invented `…/bogusNotAField`
  (all 404, identical shape);
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
- Should the **generated interfaces** declare an optional scalar as
  `name?: T | null` rather than `name?: T`? A plain read returns `null`, not
  absent, for an unset scalar (verified 2026-08-18), so `name?: T` is optimistic
  and a handler can dereference a `null` after a perfectly good `!== undefined`
  guard. The projection is now correct (`SelectedMembers`), but the plain-read
  path is not. Deliberately not changed yet: the same interfaces type an
  onCreate hook's draft object, `managed_types.rs` writes 85 files per tenant,
  and every existing handler that reads an optional scalar would start failing
  `tsc` — real bugs, but a wide blast radius that wants its own change rather
  than a rider on a point release.
