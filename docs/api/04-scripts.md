# 04 — Scripts

Implemented in: `src/scripts/`

## Purpose

Scripts are JavaScript (and rarely Groovy) snippets that run inside AM during
authentication, token issuance, OIDC claims, SAML mapping, policy decisions,
etc. Feature 2 of pingone-aic-manager ("sync scripts to a local directory +
watch + upload with content-based conflict detection") is built on this API.

## Authentication

Service-account bearer. Scope: `fr:am:*`.

## Endpoints (realm-scoped)

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`). Always
send `Accept-API-Version: protocol=2.0,resource=1.0`.

| Op     | Method   | Path                                                    | Notes                                                                                                                                                                                                                        |
| ------ | -------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| List   | `GET`    | `/am/json{realm-path}/scripts?_queryFilter=true`        | Returns **all** results when `_pageSize` is omitted. If you set `_pageSize`, page by **`_pagedResultsOffset`** + `remainingPagedResults` — the `pagedResultsCookie` comes back `null` and is unusable (verified 2026-06-01). |
| Filter | `GET`    | `/am/json{realm-path}/scripts?_queryFilter=name+eq+"…"` | CREST filter.                                                                                                                                                                                                                |
| Read   | `GET`    | `/am/json{realm-path}/scripts/{id}`                     | `id` is a UUID.                                                                                                                                                                                                              |
| Upsert | `PUT`    | `/am/json{realm-path}/scripts/{id}`                     | Full body. `script` MUST be base64. **201** when `{id}` is new, **200** on replace. See "Creating scripts" below.                                                                                                            |
| Create | `POST`   | `/am/json{realm-path}/scripts/?_action=create`          | **201**; server assigns the UUID. Same body as `PUT`, minus `_id`. Note the trailing slash before `?`.                                                                                                                       |
| Delete | `DELETE` | `/am/json{realm-path}/scripts/{id}`                     | **200** + echoes nothing useful; permanent. **404** if already gone. Default scripts **403** (see Quirks).                                                                                                                   |

## Creating scripts

Verified live 2026-07-30 (sandbox, realm `alpha`, throwaway `test_aic*` scripts,
all deleted afterwards). Two routes, both returning **201**:

- **`PUT …/scripts/{id}`** with a **client-chosen** `{id}` — this is the route
  `aic` uses, so the local workspace can know the id before the write.
- **`POST …/scripts/?_action=create`** with no `_id` — the server picks the
  UUID.

**Required fields.** Omitting any one of these is a `400` with a precise
message:

| Field      | Missing-field error                    |
| ---------- | -------------------------------------- |
| `name`     | `Script name must be specified`        |
| `context`  | `Script type must be specified`        |
| `language` | `Scripting language must be specified` |
| `script`   | `A script must be specified`           |

Everything else defaults: `description` → `null`, `default` → `false`,
`evaluatorVersion` → **`"1.0"`** (see below). An **empty** `script` (`""`) is
accepted (201) — only a _missing_ one 400s.

**Always send `evaluatorVersion` explicitly.** Omitting it creates a **legacy
(v1) engine** script — on _both_ routes (verified 2026-07-31). An earlier note
here claimed the default was `"2.0"`; that was wrong. `aic script create` always
sends the field and refuses `1.0` outright, so it never trips this.

**`default` is server-owned — a client-sent value is ignored** (verified
2026-07-31). `default: true` in the body is silently dropped to `false` on
`PUT`-create, on `POST ?_action=create`, and on a `PUT` update of an existing
non-default script. There is therefore **no way for a client to mint an
undeletable script**, and no way to promote one to a product default. Stripping
or overwriting the field on a copy (which `aic` does) is belt-and-braces, not a
correctness requirement.

**Body `_id` must match the URL id.** Sending a body whose `_id` is a different
script's id — the obvious way to copy a script — fails with
`400 "Script resource id and script JSON body id do not match"`. Either strip
`_id` from the body (the URL id is then used) or rewrite it to the new id.

**Server-owned fields on a copy are ignored, not honoured.** A verbatim fetched
body still carrying `_rev`, `createdBy`, `creationDate`, `lastModifiedBy`, and
`lastModifiedDate` creates fine: the server stamps its own values. So copying a
script is "fetch, rewrite `_id` + `name`, PUT" — no field stripping needed
beyond `_id`.

**`name` is unique per realm, enforced server-side.** A second script with a
name already in the realm →
`409 "Script with name <name> already exist in realm /alpha"`. The same name in
the _other_ realm is fine (201) — which is what makes an alpha→bravo copy a
plain create.

**`context` is normalised on write.** `SCRIPTED_DECISION_NODE` is stored and
returned as `AUTHENTICATION_TREE_DECISION_NODE`. Anything unrecognised →
`400 "Script type not recognised: <value>"`. Because the stored value can differ
from what you sent, re-read (or use the 201 echo) before deriving anything from
`context` — `aic` re-pulls after a create so the workspace path and snapshot
come from the server's canonical form.

**`_id` need not be a UUID.** `PUT …/scripts/test_aic_named_id` created a script
whose `_id` is that literal string (201). `aic` still mints UUIDs, to match what
the console and frodo produce.

## Script context enumeration

Endpoint:

```
GET /am/json/global-config/services/scripting/contexts?_queryFilter=true
Accept-API-Version: protocol=2.0,resource=1.0
```

Returns the full list of supported contexts. Verified live (40 distinct contexts
in the sandbox as of 2026-07-30):

```
AUTHENTICATION_CLIENT_SIDE                          OAUTH2_VALIDATE_SCOPE
AUTHENTICATION_SERVER_SIDE                          OAUTH2_VALIDATE_SCOPE_NEXT_GEN
AUTHENTICATION_TREE_DECISION_NODE                   OIDC_CLAIMS
CACHE_LOADER                                        OIDC_CLAIMS_NEXT_GEN
CONFIG_PROVIDER_NODE                                OIDC_NODE
CONFIG_PROVIDER_NODE_NEXT_GEN                       PINGONE_VERIFY_COMPLETION_DECISION_NODE
DEVICE_MATCH_NODE                                   POLICY_CONDITION
LIBRARY                                             POLICY_CONDITION_NEXT_GEN
NODE_DESIGNER                                       SAML2_IDP_ADAPTER
OAUTH2_ACCESS_TOKEN_MODIFICATION                    SAML2_IDP_ADAPTER_NEXTGEN
OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN           SAML2_IDP_ATTRIBUTE_MAPPER
OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER             SAML2_IDP_ATTRIBUTE_MAPPER_NEXT_GEN
OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER_NEXT_GEN    SAML2_NAMEID_MAPPER
OAUTH2_DYNAMIC_CLIENT_REGISTRATION                  SAML2_SP_ACCOUNT_MAPPER
OAUTH2_EVALUATE_SCOPE                               SAML2_SP_ADAPTER
OAUTH2_EVALUATE_SCOPE_NEXT_GEN                      SAML2_SP_ADAPTER_NEXTGEN
OAUTH2_MAY_ACT                                      SCRIPTED_DECISION_NODE
OAUTH2_MAY_ACT_NEXT_GEN                             SOCIAL_IDP_PROFILE_TRANSFORMATION
OAUTH2_SCRIPTED_JWT_ISSUER                          SOCIAL_IDP_PROFILE_TRANSFORMATION_NEXT_GEN
OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN                 SOCIAL_PROVIDER_HANDLER_NODE
```

(Note the inconsistent spelling `NEXTGEN` vs `NEXT_GEN` — see Quirks below.)

Each `result` element is an object with `_id`, `_rev`, `isHidden`, `languages`,
`defaultScript`, and `_type`; the context name is `_id`. `NODE_DESIGNER` is the
one hidden entry (`isHidden: true`). All 40 advertise `JAVASCRIPT`; 15 also
advertise `GROOVY` in `languages`.

## Object shape (real example from sandbox)

```json
{
  "_id": "ac40a394-b3cd-400f-b2aa-b6b2e4a8be8e",
  "name": "Cache Loader Script",
  "description": "Default global script for Cache Loader",
  "script": "LyoKICogQ29weXJpZ2h0...", // base64 — see Quirks
  "default": true,
  "language": "JAVASCRIPT",
  "context": "CACHE_LOADER",
  "createdBy": "id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org",
  "creationDate": 1433147666269,
  "lastModifiedBy": "id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org",
  "lastModifiedDate": 1433147666269,
  "evaluatorVersion": "2.0"
}
```

- `default: true` ⇒ ForgeRock-shipped default. **Editable** (a content PUT
  succeeds — verified 2026-06-03); cannot be _deleted_. (`aic` pushes defaults
  like any other script — no `--force` needed.) The field is **read-only to
  clients**: AM ignores whatever you send and computes it itself (verified
  2026-07-31 — see "Creating scripts").
- `evaluatorVersion`: `"1.0"` or `"2.0"`. Affects available bindings. v2 is the
  current engine, but **it is not the create default** — omit the field and you
  get v1 (verified 2026-07-31). **On read it is never absent**: all **126**
  scripts in `alpha` carried it on 2026-08-14 (**88** `"1.0"`, **38** `"2.0"`).
  A client's "assume 1.0 when the key is missing" fallback therefore never fires
  in practice — keep it as a guard, but don't design around it, and don't read
  an absent field as a signal.
- **No `_rev` field** on a `GET` or on an update `PUT` echo — so optimistic
  locking via `If-Match` is not available and **conflict detection must be
  content-based**. One exception, verified 2026-07-30: the **create** echo (201,
  either route) _does_ carry a `_rev`. It is write-only noise — a subsequent
  `GET` of the same script has no `_rev` at all — so never persist it or compare
  against it.
- **`GROOVY` scripts** (`language: "GROOVY"`) — AIC has dropped Groovy support;
  old tenants still carry many. `aic` does not sync them (filtered in the list).
- **Product-internal scripts** are named `"ForgeRock Internal: …"`. A
  `GET …/scripts/{id}` on one returns **403**
  `"This operation is not available in PingOne Advanced Identity Cloud."` —
  they're read-protected, so un-pullable. **No field in the list record marks
  them as internal** (verified 2026-06-03 — `default`,
  `createdBy`/`lastModifiedBy` (the literal string `"null"`, see "Authorship and
  change history"), `creationDate`, `context` all overlap normal scripts); the
  only reliable signal is the name prefix. `aic` hides them from the list.

## Authorship and change history

Answers "who last edited this script, and when?". Verified live 2026-08-10 on
the sandbox — realm `alpha` (121 scripts) and `bravo` (284), plus a throwaway
`test_aic_who` create/update/delete cycle. See the dated block under "Verified
against" for the call list.

### The four fields

Present on **every** AM script record, in both the collection query and a single
read, with no variation in the key set (121/121 in alpha, 284/284 in bravo):

| Field              | JSON type | Notes                                                         |
| ------------------ | --------- | ------------------------------------------------------------- |
| `createdBy`        | string    | Principal DN. **Never JSON `null`** — see the sentinel below. |
| `creationDate`     | number    | Epoch **milliseconds**. `0` means unknown.                    |
| `lastModifiedBy`   | string    | Principal DN, same shapes as `createdBy`.                     |
| `lastModifiedDate` | number    | Epoch milliseconds. `0` means unknown.                        |

**The `"null"` string sentinel — the single most important trap here.** When AM
has no authorship to report it does **not** send JSON `null` and does not omit
the key: it sends the four-character string `"null"`, with the date fields set
to `0`.

```json
{
  "_id": "ed925ac8-…",
  "name": "extractSecret",
  "description": "null",
  "createdBy": "null",
  "creationDate": 0,
  "lastModifiedBy": "null",
  "lastModifiedDate": 0
}
```

An `Option<String>` deserialises this to `Some("null")`, so any code that tests
for absence will happily print `null` as if it were a DN. Test for the literal
string. Counts across both realms (405 records): `createdBy == "null"` in
**277**, `lastModifiedBy == "null"` in **184**.

`"null"` is not a marker of product-default scripts — it spans both
(`default: true` 38, `default: false` 42 in alpha alone). It most likely marks
config that arrived by import/promotion rather than through an authenticated
write.

Sentinel and zero date travel together, with one exception worth knowing: the
two `ForgeRock Internal: …` scripts carry `createdBy: "null"` with a **real**
`creationDate` of `1433147666269` (2015-06-01, ForgeRock's own build stamp) and
`lastModifiedDate: 0`. So treat "author unknown" (`"null"`) and "date unknown"
(`0`) as independent tests, not one derived from the other.

Beware the same stringification on `description`: it is a genuine nullable (JSON
`null` in 11 alpha records) **and** carries `"null"` as a string in 19 others.
Two different "empty" representations on one field.

### `_fields` projection works

Both the collection and the single read honour `_fields`, and `_id` comes back
whether or not you ask for it:

```
GET /am/json/realms/root/realms/alpha/scripts
      ?_queryFilter=true
      &_fields=_id,name,createdBy,creationDate,lastModifiedBy,lastModifiedDate
Accept-API-Version: protocol=2.0,resource=1.0
```

This is the right call for a who-changed listing: it omits the base64 `script`
bodies, which dominate the response size.

### DN shapes that actually occur

Pooling `createdBy` and `lastModifiedBy` over all 405 scripts in alpha + bravo
gives 810 values, **15 distinct**, in four groups:

| Shape                                                | Distinct | Occurrences | What it is                           |
| ---------------------------------------------------- | -------- | ----------- | ------------------------------------ |
| `"null"`                                             | 1        | 461         | Unknown author (sentinel, see above) |
| `id=<uuid>,ou=user,ou=am-config`                     | 12       | 307         | Tenant admin **or** service account  |
| `id=amadmin,ou=user,ou=am-config`                    | 1        | 4           | AM's built-in super-admin            |
| `id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org` | 1        | 38          | AM's internal product account        |

Note what is **not** in that list: no realm-identity DN (nothing under
`o=alpha,ou=identities`), and no bare id. Every value is a `id=…,ou=user,…` DN.
The id-extraction rule is therefore just "take the `id=` RDN".

### Resolving a DN to a human name

**One endpoint does the job:**

```
GET /am/json/realms/root/users/{id}
      ?_fields=username,cn,givenName,sn,mail,universalid,objectClass
```

Sending `Accept-API-Version: protocol=2.1,resource=3.0` works; the header is not
enforced on this endpoint — the default `resource=1.0` and even a nonsense
`resource=4.0` also returned 200, so do not use version negotiation as a signal.

`universalid` in the response is exactly the DN you started from, which is what
makes this the right resolver rather than a guess.

Results for the 14 ids above:

| id          | Code    | What comes back                                                          |
| ----------- | ------- | ------------------------------------------------------------------------ |
| 10 uuid ids | **200** | 6 human admins + 4 service accounts (see discrimination below)           |
| `amadmin`   | **200** | `username=amadmin`, `cn=["amAdmin"]`                                     |
| `dsameuser` | **403** | `"Permission to perform the read operation denied"` — exists, unreadable |
| 2 uuid ids  | **404** | `"Resource cannot be found."` — deleted principal                        |

**Positive control (without which the 403/404s are uninterpretable):**
`GET /am/json/realms/root/users/amadmin` → **200**, returning
`universalid: ["id=amadmin,ou=user,ou=am-config"]` — the very DN shape under
test — while `GET /am/json/realms/root/users/zzz-not-a-real-id-zzz` → **404
"Resource cannot be found."** The endpoint is live and the id extraction is
right; the failures are properties of the principals, not of the call.

**Telling a human admin from a service account.** Both live at the same path
with the same DN shape:

|                         | Human tenant admin                                         | Service account       |
| ----------------------- | ---------------------------------------------------------- | --------------------- |
| `username`              | the admin's email                                          | **the uuid itself**   |
| `cn`                    | email, repeated twice in one string (unusable for display) | the SA's display name |
| `givenName`/`sn`/`mail` | populated                                                  | absent                |
| `dn`                    | `fr-idm-uuid=<uuid>,ou=people,o=root,ou=identities`        | absent                |
| `objectClass`           | includes `fraas-admin`, `fr-idm-managed-user-explicit`     | absent                |

So: `username == <the uuid>` ⇒ service account, display `cn`. Otherwise display
`givenName + " " + sn` and fall back to `mail`/`username` — **not** `cn`, which
for humans is the email concatenated with itself (`"dsbalmain@… dsbalmain@…"`).

**Endpoints that do _not_ resolve these DNs:**

- `GET /openidm/managed/alpha_user/{id}` — **404** for every DN observed
  (`"No Such Entry: The search base entry 'fr-idm-uuid=…,ou=user,o=alpha,o=root,ou=identities' does not exist"`).
  Correct: these are admin/config-store principals, not realm identities.
  Positive control: the same call on a genuine managed user id returns **200**
  with `userName`/`givenName`/`sn`/`mail`. Keep it only as a fallback for a DN
  shape that does not currently occur — do **not** send the AM
  `Accept-API-Version` header on it (`protocol=2.1,resource=3.0` turns the
  control's 200 into a 404).
- `GET /openidm/managed/svcacct/{id}` — resolves **only the caller's own**
  service account (200 for the id in our own bearer, **403 "Access denied"** for
  other SAs). Not a general resolver.
  `GET /openidm/managed/svcacct?_queryFilter=true` is **403** as well.
- `GET /am/json/realms/root/users?_queryFilter=true` — **403 "This operation is
  not available in PingOne Advanced Identity Cloud."** You can read a root user
  by id but you cannot enumerate them, so a resolver must be lookup-by-id with a
  cache, never a prefetch.

### What `aic`'s own writes look like

`aic script create` then `aic script push` on `alpha/test_aic_who`, read back
each time:

```
after create:  createdBy = lastModifiedBy = id=ad604d54-…,ou=user,ou=am-config
               creationDate = lastModifiedDate = 1786339741975
after push:    createdBy/creationDate unchanged
               lastModifiedDate = 1786339765012
```

That DN is the **service account** in the current context (matches
`aic whoami`'s `sa:`), and it resolves to
`cn: ["DaveBalmain-fr-config-manager"]` — an SA-shaped record with
`username == uuid` and no `givenName`/`sn`. So every write `aic` makes is
stamped with the credential, never with the operator: it is by far the most
common `lastModifiedBy` in this tenant (120 of 405 records).

**The CLI must therefore say "changed by the service account `<name>`", not
render the DN, and must not imply a person.** The audit trail cannot even
separate two concurrent `aic` processes sharing one SA — during this
verification another agent's writes to `/openidm/config/access` appeared under
the identical `userId`, indistinguishable from ours except by path.

`src/config/operator.rs` holds the operator identity locally; a "who changed
this" view can name the local operator for changes it made itself, but must not
attribute a remote change to them.

### Which script kinds have authorship at all

`aic script` spans five `Kind`s. **Only AM scripts carry authorship.**

| `Kind`           | Underlying object                     | Authorship fields               |
| ---------------- | ------------------------------------- | ------------------------------- |
| `Am`             | `/am/json{realm}/scripts/{id}`        | **Yes** — the four fields above |
| `Idm`            | `/openidm/config/endpoint/{name}`     | **No**                          |
| `Schedule`       | `/openidm/config/schedule/{name}`     | **No**                          |
| `IdmManagedHook` | `/openidm/config/managed` (whole doc) | **No**                          |
| `IdmSyncMapping` | `/openidm/config/sync` (whole doc)    | **No**                          |

**IDM config objects carry no authorship and no `_rev`.** Verified three ways:
`GET /openidm/config?_queryFilter=true` returns 68 elements whose key sets are
`_id` plus the object's own content and nothing else; individual reads of
`config/endpoint/idr` (`_id, description, globalsObject, source, type`) and
`config/schedule/test_sign_in` show no authorship key; and a recursive scan of
every scalar path in `config/managed` and `config/sync` finds no
`createdBy`/`lastModifiedBy`/`creationDate`/`_rev` anywhere (the only matches on
a case-insensitive `author` search are managed-object properties literally named
`authoritative`).

For these four kinds the honest CLI answer is **"this kind of script has no
authorship metadata"** — with the log query below offered as the only route to
who-and-when.

### Change history from the audit log

`/monitoring/logs` needs the `x-api-key`/`x-api-secret` pair, so
`verify-endpoint.sh` cannot drive it — `aic logs query` can, and it follows
`pagedResultsCookie` across pages (`src/logs/api.rs`), so it does not share the
single-page limitation of the `who-changed` prototype.

```bash
aic logs query \
  '/payload/component eq "Script"
   and /payload/eventName eq "AM-ACCESS-OUTCOME"
   and /payload/http/request/path co "<script-id>"' \
  --source am-access --begin 2026-08-10T05:28:00Z --end 2026-08-10T05:34:00Z
```

The `co` (contains) predicate on `/payload/http/request/path` **works
server-side** — no client-side filtering needed. Matching on the script id
rather than a path prefix also matters because the audit record stores the
**absolute URL exactly as the client sent it**, and different clients use
different realm path forms (`/am/json/alpha/scripts/…` and
`/am/json/realms/root/realms/alpha/scripts/…` both appear in one window).

Event shape for a script write:

```json
{
  "payload": {
    "component": "Script",
    "eventName": "AM-ACCESS-OUTCOME",
    "realm": "/alpha",
    "request": { "operation": "UPDATE", "protocol": "CREST" },
    "response": {
      "status": "SUCCESSFUL",
      "statusCode": "",
      "detail": { "revision": null },
      "elapsedTime": 43
    },
    "http": {
      "request": {
        "method": "PUT",
        "path": "https://<tenant>/am/json/realms/root/realms/alpha/scripts/38532136-…"
      }
    },
    "timestamp": "2026-08-10T05:29:25.035Z",
    "transactionId": "274c2091-…/0/1",
    "userId": "id=ad604d54-…,ou=user,ou=am-config"
  }
}
```

- **`payload.userId` is the full DN, identical to `lastModifiedBy`** — not a
  bare id. The same resolver serves both.
- The full `test_aic_who` lifecycle showed up: `CREATE`/`SUCCESSFUL`,
  `UPDATE`/`SUCCESSFUL`, `DELETE`/`SUCCESSFUL`, all with that `userId`.
- **Every event is emitted twice**: an `AM-ACCESS-ATTEMPT` with
  `response.status` absent, then an `AM-ACCESS-OUTCOME` with
  `SUCCESSFUL`/`FAILED`. Always filter `eventName eq "AM-ACCESS-OUTCOME"`.
- **A `PUT` update also logs a phantom `CREATE`/`FAILED`.** AM tries create
  first, gets `412 "Script with UUID … already exist in realm /alpha"`, then
  does the update — so one `aic script push` produces
  `CREATE`/`FAILED`/`statusCode 412` _and_ `UPDATE`/`SUCCESSFUL` in the same
  millisecond, **sharing one `transactionId`**. Filter
  `response/status eq "SUCCESSFUL"`, or a history view will report failures that
  never happened. A genuine create is `CREATE`/`SUCCESSFUL` (and carries a
  non-null `response.detail.revision`, where the update's is `null`).
- The 30-day server-side retention in `08-logs.md` bounds how far back this can
  see; the field-based `lastModifiedBy` has no such limit.

**IDM config writes are auditable too, but on a different source and shape.**
`/openidm/config/*` writes appear in **`idm-access`**, not `idm-config`
(`/payload/objectId sw "config"` on `idm-config` returned zero events over 24
h). There, `payload.eventName` is the literal `"access"`, there is **no
`component` field**, and `payload.userId` is a **bare uuid** with no DN wrapper
— plus a `payload.roles` array (`["internal/role/openidm-svcacct", …]`). Do not
apply the `id=…,ou=user` extraction to IDM events; feed the bare uuid to the
resolver directly. The same create-then-update double event occurs
(`CREATE`/`FAILED` `PUT` followed by `UPDATE`/`SUCCESSFUL` `PUT`, 11 pairs
observed).

## Conflict detection rule (for two-way sync)

Per user requirement: compare script content, **not** revision numbers. If a
local edit happens against an older "remote snapshot" but the remote content is
back to that snapshot (someone reverted), the local push should succeed.

Algorithm:

1. Cache the last-synced remote `script` content (base64) per script ID locally.
2. Before pushing a local change, `GET` the remote script and base64-decode.
3. If `remote.script_decoded == cached_last_synced_decoded`, push freely.
4. Otherwise (remote drifted), block and prompt the user to resolve: show 3-way
   diff of `cached_last_synced` vs `remote` vs `local`.
5. On every successful push, update the cached snapshot.
6. On successful pull (initial sync or refresh), update the cached snapshot.

Always compare **decoded** content. Re-encoding can produce different base64
strings (line breaks, padding) for the same bytes.

## Examples

```bash
# List first script in alpha
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/scripts?_queryFilter=true&_pageSize=1" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"

# Read a specific script
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/scripts/ac40a394-b3cd-400f-b2aa-b6b2e4a8be8e" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"

# Update (PUT — illustrative; do not run on a real script)
curl -X PUT "$TENANT_BASE_URL/am/json/realms/root/realms/alpha/scripts/$ID" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Accept-API-Version: protocol=2.0,resource=1.0" \
  -H "Content-Type: application/json" \
  -d '{
        "name":"My Script","description":"…",
        "script":"'"$(echo -n 'function foo(){return 1;}' | base64 -w0)"'",
        "language":"JAVASCRIPT","context":"SCRIPTED_DECISION_NODE",
        "default":false,"evaluatorVersion":"2.0"
      }'
```

## Quirks

- **`script` is base64-encoded on the wire** (both directions). Decode on read,
  encode on write. This contradicts the frodo-lib research summary but matches
  the Ping docs, fr-config-manager push code, and the live response shown above.
- **No `_rev`** — see "Conflict detection" above.
- **Context naming inconsistency.** Some SAML contexts use `NEXTGEN` (no
  underscore), most others use `NEXT_GEN` (with underscore). Keep an exact
  string list rather than try to derive it. The verified list is above.
- **Default scripts** (those with `default: true`) — `PUT` **succeeds** (content
  edits stick; verified 2026-06-03), but `DELETE` returns
  **`403 "Default script <name> cannot be deleted"`** and the script is still
  readable afterwards (verified 2026-07-30). It is a clean refusal, not a silent
  no-op, so `aic script delete` can rely on the server — it refuses locally
  first only to save the round trip.
- **LIBRARY context** scripts have an additional `exports` array describing
  functions they expose for other scripts to require.
- **A referenced LIBRARY script cannot be deleted** (verified 2026-07-29).
  `DELETE …/scripts/{lib-id}` while any script `require()`s it by name returns
  **`500`** with `"message": "The script <name> is used once"`. Delete the
  consumers first, then the library — the same `DELETE` then returns `200`.
  (Yes, a referential-integrity refusal reported as a 500, not a 409.)
- **`creationDate` / `lastModifiedDate`** are epoch milliseconds, not ISO 8601
  (unlike ESVs which use ISO 8601). Be careful when serializing.
- **Realm-scoped storage.** A script ID can exist in alpha but not bravo, or
  with totally different content in each. Always include realm in any local
  cache key.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/scripts?_queryFilter=true&_pageSize=1` (200 OK, base64 body
  confirmed by decoding first 30 chars to JS comment header),
  `GET …/scripts/{id}` (200 OK, no `_rev`),
  `GET /am/json/global-config/services/scripting/contexts?_queryFilter=true`
  (200 OK, full context list captured above).

### Create / copy / delete — 2026-07-30

Realm `alpha` (and one cross-realm write to `bravo`), all throwaway `test_aic*`
scripts deleted afterwards and the realms re-listed to confirm nothing was left
behind:

- `PUT …/scripts/{fresh-uuid}` → **201** + full object echo (with `_rev`);
  second `PUT` to the same id → **200** + echo (no `_rev`).
- `POST …/scripts/?_action=create` (no `_id`) → **201**, server-assigned UUID.
- Duplicate `name` in the same realm → **409**; same `name` in `bravo` →
  **201**.
- Each of `name` / `context` / `language` / `script` omitted in turn → **400**
  with the field-specific message tabulated above; `script: ""` → **201**.
- `context: "NOT_A_CONTEXT"` → **400 "Script type not recognised"**;
  `SCRIPTED_DECISION_NODE` stored as `AUTHENTICATION_TREE_DECISION_NODE`.
- Verbatim fetched body under a new URL id → **400** id-mismatch; same body with
  `_id` stripped → **201**; with `_id` rewritten to the URL id → **201**, and
  the re-read shows the server's own `createdBy`/`creationDate`, not the
  source's.
- `PUT …/scripts/test_aic_named_id` (non-UUID id) → **201**.
- `DELETE` of a `default: true` script (`SAML2 IDP Adapter Script`) → **403**,
  script still `GET`-able; `DELETE` of a nonexistent id → **404**.
- IDM side re-confirmed: `PUT /openidm/config/endpoint/{name}` 201 → 200 on
  replace → `DELETE` 200 → `GET` 404, and `DELETE` of an absent config → 404
  with `"No existing configuration found for …, can not delete"`. A
  `schedule/{name}` created with
  `enabled:false, persisted:true, type:"cron", schedule:"0 0 0 1 1 ? 2099", invokeService:"script"`
  → **201** and reads back verbatim (the shape
  `aic script create schedule/<name>` writes).

### `default` and `evaluatorVersion` on write — 2026-07-31

Realm `alpha`, three throwaway scripts, all deleted afterwards and the realm
re-listed (`test_aic*` → empty):

- `PUT …/scripts/test_aic_default_probe` with `"default": true` in the body →
  **201**, echo shows `"default": false`; a follow-up `GET` also shows `false`
  (so it is the stored value, not just the echo).
- A second `PUT` to that now-existing script, again with `"default": true` →
  **200**, still `false`. Clients cannot promote a script to a product default.
- `POST …/scripts/?_action=create` with `"default": true` → **201**, `false`.
- Both create routes with `evaluatorVersion` **omitted** → **201** with
  `"evaluatorVersion": "1.0"`, contradicting the previous "defaults to 2.0"
  claim. Recorded in `99-quirks-and-open-questions.md`.
- `DELETE` of all three → **200** each.

### Authorship, DN resolution, and audit history — 2026-08-10

Tenant `tenant.example.com` (context `sandbox`). All
calls below were made live by the verifying agent — reads via
`scripts/verify-endpoint.sh` (service-account bearer from the running agent),
log queries via `aic logs query` (stored api-key pair). One throwaway script,
`test_aic_who`, was created and deleted; both realms were re-listed afterwards
(`test_aic*` → empty) and the local workspace file removed.

Fields and shapes:

- `GET …/realms/alpha/scripts?_queryFilter=true` → **200**, 121 results; every
  record's key set is identical and includes all four authorship keys, typed
  `string`/`number`/`string`/`number`.
- `GET …/realms/bravo/scripts?_queryFilter=true&_fields=…` → **200**, 284
  results; same, 568/568 `createdBy`+`lastModifiedBy` values of type `string`
  (zero JSON `null`s in either realm).
- `GET …/realms/alpha/scripts?_queryFilter=name+eq+"extractSecret"` and
  `GET …/realms/alpha/scripts/ed925ac8-…` → **200**, both showing
  `"createdBy": "null"` **quoted** on the wire, with `creationDate: 0` and
  `"description": "null"`.
- `_fields=_id,name,createdBy,creationDate,lastModifiedBy,lastModifiedDate` on
  the collection (with `_pageSize=3`) → **200**, projection honoured, `_id`
  always present; the same on a single read → **200**.
- `GET /am/json/realms/root/scripts?_queryFilter=true` → **403 "This operation
  is not available in PingOne Advanced Identity Cloud."** (no global script
  list).

Resolution, with controls:

- Control, resolver live: `GET /am/json/realms/root/users/amadmin` → **200**,
  `universalid: ["id=amadmin,ou=user,ou=am-config"]`. Negative baseline:
  `…/users/zzz-not-a-real-id-zzz` → **404 "Resource cannot be found."**
- All 14 distinct principal ids tried against `…/realms/root/users/{id}`: **10
  uuids → 200** (6 human admins with `givenName`/`sn`/`mail`; 4 service accounts
  with `username == uuid` and `cn` = SA name), `amadmin` → **200**, `dsameuser`
  → **403 "Permission to perform the read operation denied"**, 2 uuids →
  **404**. That is 11 × 200, 1 × 403, 2 × 404.
- Same read with `Accept-API-Version` `protocol=2.1,resource=3.0`,
  `protocol=2.0,resource=1.0`, and `protocol=2.1,resource=4.0` → **200** in all
  three cases (header not enforced).
- `GET /am/json/realms/root/users?_queryFilter=true` → **403** (cannot
  enumerate).
- Control, IDM resolver live:
  `GET /openidm/managed/alpha_user?_queryFilter=true&_pageSize=2` → **200**, and
  `GET /openidm/managed/alpha_user/985bf175-…` → **200** with
  `userName`/`givenName`/`sn`/`mail`. The **same** single read with
  `Accept-API-Version: protocol=2.1,resource=3.0` → **404**, so the header must
  be omitted on IDM.
- `GET /openidm/managed/alpha_user/{id}` for 5 of the script DNs (including
  `dsameuser`) → **404 "No Such Entry: …
  'fr-idm-uuid=…,ou=user,o=alpha,o=root,ou=identities' does not exist"**.
- `GET /openidm/managed/svcacct/{own-sa-id}` → **200** (`name`
  `DaveBalmain-fr-config-manager`); the same for two other SA ids → **403
  "Access denied"**; `GET /openidm/managed/svcacct?_queryFilter=true` → **403**.

`aic`'s own write:

- `aic script create alpha/test_aic_who --context AUTHENTICATION_TREE_DECISION_NODE --language JAVASCRIPT --evaluator-version 2.0 --from … --yes`,
  then `GET …/scripts?_queryFilter=name+eq+"test_aic_who"&_fields=…` → **200**
  with `createdBy = lastModifiedBy = id=ad604d54-…,ou=user,ou=am-config` (the
  `sa:` in `aic whoami`) and `creationDate = lastModifiedDate = 1786339741975`.
- `aic script push alpha/test_aic_who --yes`, then
  `GET …/scripts/38532136-…?_fields=…` → **200**, `creationDate` unchanged,
  `lastModifiedDate = 1786339765012`.
- `aic script delete alpha/test_aic_who --yes` → refused
  (`script delete requires --force`); with `--force --yes` → deleted;
  `GET …/scripts/38532136-…` → **404 "Script with UUID … could not be found in
  realm /alpha"**; realm re-list → no `test_aic*` in alpha or bravo.

Other script kinds:

- `GET /openidm/config?_queryFilter=true` → **200**, 68 elements; the distinct
  key sets are `_id` + content only, with no `createdBy`/`lastModifiedBy`/
  `creationDate`/`_rev` in any of them.
- `GET /openidm/config/endpoint/idr` → **200**
  (`_id, description, globalsObject, source, type`);
  `GET /openidm/config/schedule/test_sign_in` → **200** (17 keys, none
  authorship); `GET /openidm/config/managed` → **200** (`_id, objects`);
  `GET /openidm/config/sync` → **200** (`_id, mappings`). A recursive
  scalar-path scan of the last two matched only properties named
  `authoritative`.

Audit history:

- `aic logs sources` → **200** (log api-key pair is present and valid on this
  tenant).
- `aic logs query '/payload/component eq "Script" and /payload/eventName eq "AM-ACCESS-OUTCOME" and /payload/request/operation eq "UPDATE"' --source am-access`
  over the write window → **1 event**, the `test_aic_who` `PUT`, with
  `payload.userId` equal to the `lastModifiedBy` DN character for character.
- Same window, `/payload/component eq "Script"` only → **58 events**; the
  `eventName × operation × status` cross-tab established the
  `AM-ACCESS-ATTEMPT`/`AM-ACCESS-OUTCOME` pairing and the phantom
  `CREATE`/`FAILED`/`412` that accompanies every `UPDATE`/`SUCCESSFUL` on the
  same `transactionId`.
- Same filter with `and /payload/http/request/path co "38532136-…"` → **14
  events**, all for that script — server-side `co` on the path confirmed. After
  the delete, the same query → the `DELETE`/`SUCCESSFUL` event.
- `aic logs query '/payload/objectId sw "config"' --source idm-config` → **0
  events** over 24 h;
  `aic logs query '/payload/http/request/path co "openidm/config"' --source idm-access`
  → **66 events**, including 11 `UPDATE`/`PUT`/`SUCCESSFUL` on
  `/openidm/config/access` with `eventName: "access"`, no `component`, and
  `userId` as a **bare uuid**. Those writes were **not** made by this
  verification — they came from a concurrent process on the same service account
  (the same window also shows 26 `GET /openidm/managed/svcacct` queries that
  this verification never issued), which is itself the evidence that the SA DN
  cannot distinguish concurrent writers.

### `evaluatorVersion` presence across the realm — 2026-08-14

Sandbox, realm `alpha`, contributed by the sibling
`terraform-provider-pingone-aic` project as part of a wider survey (36 trees,
178 tree nodes, 126 scripts):

- Every script in the realm listed and inspected: **126/126 carry
  `evaluatorVersion`** — **88** `"1.0"`, **38** `"2.0"`, none missing.
- `PUT` of a new script with `"evaluatorVersion": "2.0"` → **201** echoing
  `"2.0"`; a follow-up `GET` also returned `"2.0"`. The probe script was deleted
  afterwards and its removal confirmed, and the count above was **re-taken with
  the probe gone** — see the census caveat in `99-quirks-and-open-questions.md`
  (2026-08-14).
- Key set observed on script `GET`/`PUT` responses: `_id`, `_rev` (on the `PUT`
  echo only, consistent with the 2026-07-30 finding), `context`, `createdBy`,
  `creationDate`, `default`, `description`, `evaluatorVersion`, `language`,
  `lastModifiedBy`, `lastModifiedDate`, `name`, `script`.

## Source citations

- frodo-lib: `src/api/ScriptApi.ts`, `src/api/ScriptTypeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/scripts.js`,
  `packages/fr-config-push/src/scripts/update-scripts.js` (note: explicitly
  base64-encodes before PUT).
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-scripting/rest-api-scripts-read.html>

## Open questions

- Does the server reject non-base64 in the `script` field, or attempt to detect
  raw JS? frodo-lib seemed to assume raw, which would suggest a tolerant server.
- Are `LIBRARY` scripts' `exports` validated against the script body, or just
  declarative metadata?
