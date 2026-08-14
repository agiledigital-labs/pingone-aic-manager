# 20 — `config/authentication`: IDM rsFilter subject mappings

## Purpose

`config/authentication` is the tenant-global rsFilter document: how IDM
validates a bearer, which scopes it accepts, and how a token subject becomes
an IDM security context. This file records the **whole document** and the
`staticUserMapping[]` entries that tooling can manage individually. Role
_reference_ conventions (`_id` not `name`, synthetic roles, the two `roles`
shapes) live in [18-internal-roles.md](18-internal-roles.md). The policy
that consumes those roles is [19-config-access.md](19-config-access.md).

`subjectMapping` (how a user token is looked up in `managed/*_user`),
`anonymousUserMapping`, `scopes`, client credentials, `cache`,
`tokenIntrospectUrl`, and `augmentSecurityContext` are observed here so a
read-modify-write of one mapping cannot rewrite them. They are not modelled
as their own resources.

## Authentication

Service-account bearer, scope `fr:idm:*`. **Tenant-global — no realm
segment**, like `config/access` and IDM config generally.

## Endpoints

| Op      | Method | Path                              | Notes                                               |
| ------- | ------ | --------------------------------- | --------------------------------------------------- |
| Read    | `GET`  | `/openidm/config/authentication`  | 200, `{_id, rsFilter}`.                             |
| Replace | `PUT`  | `/openidm/config/authentication`  | Whole-document replace. `_id` may be omitted.       |

There is no per-mapping endpoint. Every change is a read-modify-write of the
whole document. Do **not** send `Accept-API-Version`.

## Object shape

Top-level keys on the sandbox are exactly `_id` (`"authentication"`) and
`rsFilter`. There is **no `_rev`**. `rsFilter` keys observed 2026-08-15:

| Key                      | Shape                                      | Notes                                      |
| ------------------------ | ------------------------------------------ | ------------------------------------------ |
| `anonymousUserMapping`   | object                                     | One mapping, not a list.                   |
| `augmentSecurityContext` | `{type, source}`                           | Product JS.                                |
| `cache`                  | `{maxTimeout}`                             |                                            |
| `clientId` / `clientSecret` | ESV wrappers                            | `&{rsfilter.resource.server.*}`            |
| `scopes`                 | array of strings                           | Sandbox: `["fr:idm:*"]`                    |
| `staticUserMapping`      | array of objects                           | The individual "rules".                    |
| `subjectMapping`         | array of objects                           | Realm user lookup; not individual grants.  |
| `tokenIntrospectUrl`     | string                                     | Internal AM URL.                           |

### `staticUserMapping[]`

Five live entries. Keys and frequency:

| Key                         | Present in | Notes                                              |
| --------------------------- | ---------- | -------------------------------------------------- |
| `subject`                   | 5 / 5      | Token subject. Unique on this tenant.              |
| `localUser`                 | 5 / 5      | Usually `internal/user/…`; `internal/role/c1` is legal. |
| `roles`                     | **4 / 5**  | **Array of strings.** `RCSClient` omits the key.   |
| `userRoles`                 | 1 / 5      | `authzRoles/*` on `amadmin` only.                  |
| `executeAugmentationScript` | 1 / 5      | `true` on `test_service_C1` only.                  |

`roles` here is an **array**. The same conceptual field in `config/access` is
a comma-separated string. Anything that writes both must know the
difference — [18-internal-roles.md](18-internal-roles.md) has the table.

A referenced role does not have to exist as an `internal/role` object.
`test_service_C1` → `internal/role/c1` is the live synthetic case.

## No `_rev` — content is the only precondition

Same as `config/access`. There is no conditional write. The only
precondition is a content comparison against the document as previously
read.

## A mapping append survives a read-modify-write

Verified 2026-08-15 on the 5-mapping sandbox document. `GET`, append one
`staticUserMapping` entry (`subject: Terraform_auth_probe`), `PUT` the whole
object (siblings left as the same JSON values, other `rsFilter` keys
untouched), and:

- the five original mappings return **byte-identical and in original order**
- the rest of `rsFilter` is **byte-identical**
- `PUT` without `_id` is accepted (200)
- restoring the saved original is **byte-exact** (canonical sha256
  `4fabd82ccc9aa358e4e466af81532191562807ccde0292721b84539e6630258f`)

So read-modify-write `PUT` is safe to build on, provided unmanaged mappings
are not decoded and re-encoded. Rebuilding a mapping that omitted `roles`
would hand `RCSClient` a key it never had. The same trap exists on
`config/access`: three live rules store `actions: ""`, which is not the
same as omitting `actions`.

## Individual mappings have no id

There is no `_id` on a `staticUserMapping` entry. Tooling that manages one
row at a time has to identify it by content. The canonical form used here
is the same as `config/access`: SHA-256 of
`json.dumps(obj, sort_keys=True, separators=(',',':'))` (no HTML escaping).
A create that would append a mapping whose digest is already present should
be refused — import the existing row instead of creating a duplicate grant.

`subjectMapping` is a different list (how a user token is resolved onto
`managed/{{realm}}_user`) and is **not** treated as the same kind of
individual rule.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com` (sandbox), no realm segment.
- Date: 2026-08-15
- Verified by the author of this file, from live calls made while writing it.

### Object shape — 2026-08-15

- `GET /openidm/config/authentication` → 200. Top-level keys exactly `_id`
  and `rsFilter`. No `_rev`. `rsFilter` keys as tabulated above.
  `staticUserMapping` has 5 entries; key frequencies as tabulated.
  `subjectMapping` has 2 entries (`managed/teammember` at realm `/`, and
  `managed/{{substring realm 1}}_user`).
- Canonical sha256 of the whole document:
  `4fabd82ccc9aa358e4e466af81532191562807ccde0292721b84539e6630258f`.

### Read-modify-write — 2026-08-15

- `GET` → 200, 5 mappings. Append
  `{"subject":"Terraform_auth_probe","localUser":"internal/user/anonymous","roles":["internal/role/Terraform_auth_probe"]}`
  and `PUT` → 200. Re-`GET` → 6 mappings, originals byte-identical and in
  order, other `rsFilter` keys byte-identical.
- `PUT` of that 6-mapping document with `_id` stripped → 200.
- Restore `PUT` of the saved original → 200; re-`GET` hashes to
  `4fabd82ccc9aa358e4e466af81532191562807ccde0292721b84539e6630258f`.
  `config/access` was never touched (still
  `75189406f2cad0de785a306176deb50fb57291319015946e98a2ae9e5900cf7f`).
