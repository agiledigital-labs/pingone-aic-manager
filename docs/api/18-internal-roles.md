# 18 — Internal roles and IDM authorization

Implemented in: [`src/roles/`](../../src/roles/) (`aic role`).

## Purpose

IDM **internal roles** are the authorization roles that gate `/openidm` routes.
They are what `config/access` rules and `config/authentication` subject mappings
refer to. They are distinct from **managed roles** (`managed/alpha_role`), which
are business roles granted to end users and drive provisioning.

The reason this feature exists: creating a role through the admin console
generates a **random UUID** `_id`, and every reference to that role in
`config/access` and `config/authentication` must use the `_id`, not the `name`.
Referring to it by name silently fails to match. `aic role create <id>` gives
the role an `_id` you chose, so the reference is the name you already know.

## Authentication

Service-account bearer, scope `fr:idm:*`. **Tenant-global — no realm segment**
(`/openidm/internal/role`), like ESVs and IDM config, unlike AM realm-config.
Internal roles are outside realms entirely; a realm is only how a human signs
in.

## Endpoints

`Accept-API-Version: resource=1.0` is sufficient; no protocol version needed.

| Op                        | Method   | Path                                       | Notes                                                                        |
| ------------------------- | -------- | ------------------------------------------ | ---------------------------------------------------------------------------- |
| List                      | `GET`    | `/openidm/internal/role?_queryFilter=true` | Returns `privileges` despite the schema's `returnByDefault: false`.          |
| Read                      | `GET`    | `/openidm/internal/role/{id}`              | A bare read returns everything. `_fields` **projects** — see the trap below. |
| **Create with chosen id** | `PUT`    | `/openidm/internal/role/{id}`              | **201**, `_id` equals the path segment. Neither header required.             |
| Replace                   | `PUT`    | `/openidm/internal/role/{id}`              | 200. **Destructive full replace** — see below.                               |
| Create with generated id  | `POST`   | `/openidm/internal/role?_action=create`    | 201 with a **random UUID** `_id`. This is what the admin console does.       |
| Delete                    | `DELETE` | `/openidm/internal/role/{id}`              | 200; a follow-up `GET` 404s.                                                 |
| Field schema              | `GET`    | `/openidm/schema/internal/role`            | Titles and types. **One key is wrong** — see the `accessFlags` quirk.        |
| Effective privileges      | `GET`    | `/openidm/privilege/internal/role/{id}`    | What the _calling_ identity may do to that role. Not the role's own privs.   |

## Object shape

```json
{
  "_id": "service-desk",
  "name": "service-desk",
  "description": "Service desk operators",
  "privileges": [
    {
      "name": "Alpha realm - Users",
      "path": "managed/alpha_user",
      "actions": [],
      "permissions": ["VIEW", "UPDATE"],
      "accessFlags": [{ "attribute": "mail", "readOnly": false }]
    }
  ]
}
```

Role-level `name`, `description` and `privileges` are all **optional** — a bare
`{"name": "x"}` creates successfully. `temporalConstraints` and `condition`
exist but are not exercised here.

Reads carry an `_id` and a **`_rev`**. The schema marks both `privileges` and
`authzMembers` `returnByDefault: false`, but only `authzMembers` behaves that
way: `privileges` comes back on a bare single read _and_ on a bare
`_queryFilter=true` list. Do not rely on the flag either way — ask for what you
need.

**`_fields` projects, and that is a trap for read-modify-write.**
`?_fields=privileges` returns `_id`, `_rev` and `privileges` **only** — `name`
and `description` are dropped. Feed that into a `PUT` and you erase them. So an
amend-and-write cycle wants a bare `GET`.

**But the bare read is not writable as-is.** `GET` returns
`temporalConstraints`, and sending it back — even the empty array the read
produced — fails with `403 "Policy validation failed"`, naming that field as
invalid on write. It must be stripped. Isolated 2026-08-10 against a positive
control: strip all of `_id`/`_rev`/`condition`/`temporalConstraints` → 200;
`condition` alone retained → **200**; `temporalConstraints` alone retained →
**403**; `_id` and `_rev` retained in the body → **200**.

So the amend-and-write recipe is: bare `GET`, drop **`temporalConstraints`**,
`PUT` with `If-Match: <_rev>`. `condition`, `_id` and `_rev` may stay in the
body; the revision that counts is the header's.

**Privilege order is not preserved on write.** Replacing one privilege in place
returned the surrounding privileges reordered, which matches the LDAP-backed
store behind `cn=…,ou=roles,ou=internal` treating the list as an unordered
multi-valued attribute. Reads are self-consistent — repeated reads gave the same
order — so displaying what you read is fine, but nothing may depend on position,
and a test asserting privilege order would be testing the backend's whim.

Inside a privilege, **every** one of `name`, `path`, `actions`, `permissions`
and `accessFlags` is mandatory. Omitting any of them, or sending an empty
`accessFlags`, fails with `403 "Policy validation failed"` and a deeply nested
`failedPolicyRequirements` body. `actions` may be an empty array; `accessFlags`
may not. `filter` is optional — present on one live privilege
(`identity-access-manager` → `managed/alpha_organization`) as
`/name eq "Inactive Users to Review"`, and accepted on write.

A privilege whose `permissions` include anything other than `VIEW` also
requires **at least one** `accessFlags[].readOnly: false`. Verified 2026-08-15:
`permissions: ["VIEW","UPDATE"]` with every flag `readOnly: true` → **403**
`PRIVILEGE_AT_LEAST_ONE_ATTRIBUTE_READONLY_FALSE`. The same privilege with
one writable flag → 200. VIEW-only privileges may be all-readOnly (the
sandbox's `my-role` is).

AM validates the contents server-side: a non-existent `path`, an unknown
`permissions` value, and an `attribute` that is not a property of the target
object each produce the same opaque 403. `readOnly` must be a JSON boolean — the
string `"true"` is rejected.

### `PUT` is a destructive full replace

A `PUT` that omits `privileges` **silently empties them** — verified by writing
a privilege, then re-`PUT`ting the role without one and reading back
`privileges: []`. So:

- **create** must refuse an id that already exists, rather than replace it;
- **any privilege edit** must read the whole role, amend it, and write it back
  including `name` and `description`, or those are lost too.

**Internal roles are a verified conditional-write family** (cf. `CLAUDE.md` §5).
`If-Match: <_rev>` with the current revision returns 200, quoted or unquoted;
with a superseded revision it returns **412**; `If-Match: *` returns 200; and a
plain `PUT` with no header also returns 200. Because a full replace is
destructive, an amend-and-write cycle **should** send `If-Match` with the `_rev`
it read, so a concurrent edit fails loudly instead of being overwritten.

### Quirk: the published schema misspells `accessFlags`

`GET /openidm/schema/internal/role` declares the privilege key as
**`accessflags`** (lowercase `f`). The API requires **`accessFlags`**. Sending
the schema's own spelling fails with `403 "Policy validation failed"` and a
`REQUIRED` policy requirement — because the correctly-cased key is then missing.
Trust the API, not the schema. Also logged in
[99-quirks-and-open-questions.md](99-quirks-and-open-questions.md).

## How roles are referenced — and why a missing role is only a warning

Two configs refer to internal roles, **with different shapes**:

| Config                  | Location                                                           | `roles` shape                                                  |
| ----------------------- | ------------------------------------------------------------------ | -------------------------------------------------------------- |
| `config/access`         | `configs[].roles`                                                  | **comma-separated string**, e.g. `internal/role/openidm-admin` |
| `config/authentication` | `rsFilter.staticUserMapping[].roles`, `anonymousUserMapping.roles` | **array of strings**                                           |

Anything that writes both must know the difference; a single "add a role" helper
that assumes one shape produces invalid config in the other.

**A referenced role does not have to exist as an `internal/role` object.**
`rsFilter` puts the role strings from a subject mapping straight into the
security context, and `config/access` matches those strings. So a purely
synthetic role can be granted in `config/authentication` and consumed in
`config/access` without ever being created — and per operator experience this is
normal practice, not a mistake.

Consequence for tooling: a role reference that resolves to no `internal/role`
object is **not** an error. Treat a reference as known if it either exists under
`internal/role` **or** appears in a `config/authentication` mapping, and only
**warn** when it appears in neither. That still catches the real footgun —
referring to a UUID-`_id` role by its `name`, which appears in neither place.

On the sandbox, `internal/role/c1` is exactly the legitimate synthetic case: it
is granted to subject `test_service_C1` in `staticUserMapping` and consumed by
the `endpoint/mock-c1/*` access rule, while `GET /openidm/internal/role/c1`
404s. Its neighbouring `localUser: "internal/role/c1"` — where every other entry
uses `internal/user/…` — looks anomalous, but whether it matters has **not**
been established and it is deliberately not called a defect here.

## The policy that consumes these roles lives in its own file

Everything about `config/access` itself — its shape, the optional `actions` key,
the absence of `_rev`, the fact that `configs` is a **disjunction** so an
appended rule can never revoke access, and the scoped bypass that stops `aic`
confirming a rule change empirically — is in
[19-config-access.md](19-config-access.md), together with the live calls that
established it.

Those sections were moved there when `src/access/` was built, with only their
cross-references adjusted — no claim was reworded. What remains in this file is
the _reference_ conventions above — the `_id`-not-`name` rule, the two differing
`roles` shapes, synthetic roles — because they are properties of a role
reference rather than of the policy that consumes it.

## Open questions

- **Are `permissions` values enumerable from an API?** The role schema exposes
  no enum. `VIEW`, `CREATE`, `UPDATE`, `DELETE` and `ACTION` are the keys
  returned by `GET /openidm/privilege/internal/role/{id}`, but that is an
  inference from a different endpoint's response shape, not a published list.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com` (sandbox), no realm segment
- Date: 2026-08-10
- Verified by the author of this file, from live calls made while writing it —
  not transcribed from a task prompt or a neighbouring doc.
- Calls:
  - `GET /openidm/internal/role?_queryFilter=true&_fields=name,description` →
    200, 8 roles. Six built-ins have `_id == name`; two console-created ones
    (`identity-access-manager`, `my-role`) have UUID `_id`s.
  - `PUT /openidm/internal/role/test_aic_chosen` with
    `{"name":…,"description":…}` → **201**, `_id: "test_aic_chosen"`. Re-`PUT`
    with a privilege → 200. Third `PUT` without `privileges` → 200 and a
    read-back of `privileges: []`, establishing the destructive replace.
    `DELETE` → 200, follow-up `GET` → 404.
  - A privilege-field matrix, each batch run with a **positive control** that
    returned 201: omitting `actions`, privilege `name`, `permissions`, or
    `accessFlags` → 403; `accessFlags: []` → 403; `readOnly: "true"` → 403;
    `permissions: ["FLY"]` → 403; `attribute: "nope_not_real"` → 403;
    `path: "managed/nope_not_real"` → 403. Role-level `description` omitted →
    201, and no `privileges` at all → 201.
  - Conditional writes, each with the `_rev` re-read immediately beforehand (a
    first attempt was invalidated by the control `PUT` bumping the revision
    under it): `If-Match: <current>` → 200, quoted form → 200,
    `If-Match: <superseded>` → **412**, `If-Match: *` → 200, plain `PUT` → 200.
  - Field projection on one role: bare `GET` and `_fields=*` both return `_id`,
    `_rev`, `name`, `description`, `privileges`, `condition`,
    `temporalConstraints`; `_fields=privileges` returns only `_id`, `_rev`,
    `privileges`. A bare `_queryFilter=true` list row carries the same set, i.e.
    `privileges` is returned despite `returnByDefault: false`, while
    `authzMembers` is not.
  - `GET /openidm/schema/internal/role` → 200, declaring `accessflags`; a `PUT`
    using that spelling → 403 `REQUIRED`, with the role absent afterwards (404),
    confirming rejection rather than silent field loss.
  - `GET /openidm/config/access` → 200 plus an append-and-restore cycle; that
    bullet now lives with the section it supports, in
    [19-config-access.md](19-config-access.md).
  - `GET /openidm/config/authentication` → 200, `{_id, rsFilter}`;
    `staticUserMapping` role arrays inspected. `GET /openidm/internal/role/c1` →
    404 while being referenced by both configs.
  - All `test_*` probe roles deleted; a final list confirmed 8 roles and no
    `test_` remnants. `config/access` left byte-identical to its original.
- **`config/access` evaluation order** and **bypass scoping** — two further live
  runs on 2026-08-10, whose calls are recorded in full in
  [19-config-access.md](19-config-access.md) alongside the sections they
  establish. The one finding of theirs that belongs to _this_ file is that
  `PUT /openidm/internal/role/{new-id}` → 201 and
  `GET /openidm/internal/role?_queryFilter=true` → 200 for a bearer that no
  `config/access` rule grants either operation.

### Chosen-id create, filter, If-Match, write-flag policy — 2026-08-15

- `PUT /openidm/internal/role/Terraform_role_probe` with a single VIEW
  privilege, `filter`, and `Accept-API-Version: resource=1.0` → **201**,
  `_id` equals the path. Re-GET keeps `filter`. `If-Match: <current>`
  update that adds UPDATE plus one `readOnly: false` flag → **200** and a
  new `_rev`. `If-Match: <superseded>` → **412**.
- PUT of a bare GET body (including `temporalConstraints: []`) → **403**.
  PUT omitting `privileges` → 200 and `privileges: []`. DELETE → 200,
  follow-up GET → 404. No `Terraform_` leftover. List without
  `Accept-API-Version` still 200 (8 roles).
- Privilege keys observed on the two console-created roles: `name`, `path`,
  `actions`, `permissions`, `accessFlags`, and (on one) `filter`. Role-level
  keys remain `_id`, `_rev`, `name`, `description`, `privileges`,
  `condition` (null), `temporalConstraints` (`[]`).
