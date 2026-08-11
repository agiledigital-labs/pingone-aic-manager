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
may not.

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

## `config/access` survives a read-modify-write

Relevant because it is the object most likely to lock an operator out. Verified
2026-08-10 on a 65-rule tenant: `GET`, append one rule, `PUT` the whole object
back, and all 65 pre-existing rules return **byte-identical and in original
order**, with no top-level keys lost. Restoring the saved original is likewise
byte-exact. The `_id` may be left in the `PUT` body. `config/access` carries no
`_rev` at all — unlike internal roles above, so there is no conditional write to
use here and a backup is the only safety net.

So read-modify-write `PUT` is safe to build on. **Back the object up first
anyway** — the failure mode is losing authorization for everyone.

### The service-account bearer is not fully governed by `config/access`

Verified 2026-08-10, and it constrains how authz tooling can be built.

`GET /openidm/info/login` for our service-account bearer reports roles
`internal/role/openidm-svcacct` + `internal/role/openidm-authorized`, component
`managed/svcacct`. Two things follow from the live config:

- **`openidm-svcacct` does not exist** as an object
  (`GET /openidm/internal/role/openidm-svcacct` → 404) and the string appears in
  **no rule** in `config/access`. It is a synthetic role, like `c1` above.
- **Nothing in `config/access` grants these roles `query` or `create` on
  `internal/role`.** No rule with either method has a pattern covering it; rule
  25 grants `read,query` on `internal/role/*` to `platform-provisioning` only,
  and the two `pattern: "*"` rules for `openidm-authorized` (37, 38) list
  `read,action,delete` and `update,patch,action` — neither includes `query` or
  `create`.

Yet `GET /openidm/internal/role?_queryFilter=true` returns **200** and
`PUT /openidm/internal/role/{new-id}` returns **201**. Reads of `config/access`
and `config/authentication` likewise return 200. So on those paths the bearer is
not evaluated against `config/access` at all.

It is not a blanket bypass: `GET /openidm/managed/svcacct?_queryFilter=true`
returns **403**, and adding a rule for that path flips it to 200 (see the case
table below). So the filter is live for this identity on some paths and absent
on others. **The mechanism has not been established** — scope-based
authorization for service accounts is a plausible explanation but is not
verified here, and nothing should be built on it.

Two consequences for a guarded-write feature over these configs:

- **`aic` cannot confirm a rule change by trying the operation itself.** Its own
  access may not be governed by the rule it just wrote, so an empirical "did it
  work?" check would silently prove nothing. Validation has to be structural —
  check the shape, resolve every role reference, show the operator a diff —
  rather than "write it and probe".
- **A bad write cannot lock the tool out of repairing it**, because its access
  to `config/access` does not come from `config/access`. That is a real safety
  property for the feature, but it is an observation about this tenant today,
  not a guarantee — keep taking a backup first.

### `configs` is a disjunction, not first-match-wins (resolved 2026-08-10)

**A request is permitted if _any_ rule grants it.** A rule that matches the
pattern but does not grant — wrong `roles`, or a `customAuthz` that returns
false — does **not** terminate evaluation. So a rule appended to the end of
`configs` can never be shadowed by an earlier, broader rule, and **tooling may
append**.

Measured against a probe that the tenant's 65 existing rules deny:
`GET /openidm/managed/svcacct?_queryFilter=true&_fields=_id` → **403**. The
grant under test was
`{"pattern":"managed/svcacct","roles":"internal/role/openidm-authorized","methods":"query","actions":"*"}`.
Every injected rule used `pattern: "managed/svcacct"` exactly, and every `PUT`
body was rebuilt from the untouched 65-rule original, so no case could
accumulate on another.

| `configs` sent                                                     | Probe   |
| ------------------------------------------------------------------ | ------- |
| original (65) — baseline                                           | 403     |
| original + grant (66)                                              | **200** |
| non-matching-role rule first, original, grant last (67)            | 200     |
| grant first, original, non-matching-role rule last (67)            | 200     |
| original + `customAuthz: "false"` rule + grant (67)                | 200     |
| original + `customAuthz: "false"` rule only (66)                   | **403** |
| original + `customAuthz: "(function(){return false})()"` only (66) | **403** |

The last two rows are what make the fifth interpretable. A `customAuthz` of
`false` really does deny on its own, so row five is evaluation **continuing
past** a rule that matched pattern, roles and methods and then refused — not the
refusal being ignored. The baseline agrees independently: rules 37 and 38 are
`pattern: "*"` for `internal/role/openidm-authorized` with `customAuthz`
`ownDataOnly()`, which match this identity and fail for this query, and an
appended grant still took effect past them.

Consequences for tooling:

- **Append; no insertion-position logic is needed.** The feared silent no-op —
  an appended grant shadowed by an earlier broader pattern — does not occur.
- **A new rule cannot revoke anything.** There are no deny rules, only grants
  that may decline. Narrowing existing access means **editing or removing the
  rule that grants it**; appending a "restriction" is the real silent no-op
  here, the inverse of the failure that was feared.
- **Changes take effect immediately.** Both 403→200 transitions were visible on
  the first probe after the `PUT`; no retry was ever needed, and no propagation
  delay was observed.

The answer is only as general as the identity it was measured with: a
service-account bearer whose `GET /openidm/info/login` roles are
`internal/role/openidm-svcacct` and `internal/role/openidm-authorized`
(component `managed/svcacct`). Behaviour for an `openidm-admin` caller or an
anonymous one was not measured.

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
  - `GET /openidm/config/access` → 200, `{_id, configs[65]}`; append-one-rule
    `PUT` → 200; read-back byte-identical for all 65 originals; restore `PUT` →
    200 with a byte-identical final read.
  - `GET /openidm/config/authentication` → 200, `{_id, rsFilter}`;
    `staticUserMapping` role arrays inspected. `GET /openidm/internal/role/c1` →
    404 while being referenced by both configs.
  - All `test_*` probe roles deleted; a final list confirmed 8 roles and no
    `test_` remnants. `config/access` left byte-identical to its original.
- **`config/access` evaluation order** — a separate live run, also 2026-08-10,
  made by the agent that resolved that open question from its own calls. None of
  the statuses below were transcribed from a task prompt or a neighbouring doc.
  - Identity, which is the limit of the result's generality:
    `GET /openidm/info/login` → 200 with roles
    `["internal/role/openidm-svcacct","internal/role/openidm-authorized"]` and
    component `managed/svcacct`. `internal/role/openidm-svcacct` does not exist
    as an object (404) and is named in no `config/access` rule.
  - Probe `GET /openidm/managed/svcacct?_queryFilter=true&_fields=_id` → **403**
    against the pristine 65-rule object. Nonexistent types
    (`managed/nope_not_real`, `file/iwa/x`) 404 from routing before authz and so
    cannot serve as probes.
  - Seven `PUT /openidm/config/access` → 200. Every body was rebuilt from the
    pristine 65-rule object, injecting only rules whose `pattern` was exactly
    `managed/svcacct`; each `PUT` was followed by a re-`GET` asserting the rule
    count and that the 65 originals were intact and in order, then by the probe.
    Grant appended → **200**; non-matching-role rule first with the grant last →
    200; grant first with the non-matching-role rule last → 200;
    `customAuthz: "false"` rule then the grant → 200; `customAuthz: "false"`
    rule alone → **403**; `customAuthz: "(function(){return false})()"` alone →
    **403**; grant alone → **200**.
  - Restore `PUT` → 200, and the re-`GET`'s canonical form
    (`json.dumps(obj, sort_keys=True, separators=(',',':'))`) hashed to sha256
    `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f`, equal to
    the pre-experiment value; the probe returned to **403**.
    `config/authentication` was never touched.

- Bypass scoping (the "not fully governed by `config/access`" section above),
  2026-08-10, from calls made directly by the author of that section:
  - `GET /openidm/internal/role?_queryFilter=true` → **200** and `GET
    /openidm/config/access`, `GET /openidm/config/authentication` → **200**,
    none of which any rule matching `openidm-svcacct` /
    `openidm-authorized` grants. The `create` counterpart (`PUT
    /openidm/internal/role/{new-id}` → 201) is recorded in the group above.
  - `GET /openidm/managed/svcacct?_queryFilter=true` → **403** on the same
    bearer in the same minute, which is what makes it a scoped bypass rather
    than a blanket one. The mechanism was not determined.
  - Rule enumeration supporting the claim was done over the fetched 65-rule
    object, not from memory of the console: no rule pairs `openidm-svcacct` or
    `openidm-authorized` with methods `query` or `create` under a pattern
    covering `internal/role`.
  - Independent post-experiment check from a separate process: `config/access`
    → 200 with 65 rules, canonical sha256
    `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f`, the
    `configs` array equal element-for-element and in order to the
    pre-experiment capture, no `managed/svcacct` rule remaining, and the probe
    back to **403**.
