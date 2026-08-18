# 11 — IDM custom endpoints

Implemented in: `src/scripts/`

## Purpose

IDM custom endpoints are scripted (or table/jdbc) REST endpoints registered as
IDM config objects under `endpoint/<name>`. Their JavaScript lives in a plain
`source` string. `pingone-aic-manager`'s script-sync feature treats them as a
second "script kind" alongside AM scripts (see `04-scripts.md`), sharing the
same content-based conflict-detection core.

## Authentication

Service-account bearer. Scope: `fr:idm:*`. **No realm segment** — IDM config is
tenant-global (`/openidm/...`), unlike realm-scoped AM scripts.

### Public read endpoints

The endpoint **configuration** APIs above always require the service-account
bearer token. The endpoint's _runtime URL_ can, however, be intentionally made
available before authentication. IDM authorizes direct HTTP calls through the
tenant's `config/access` rules. Add a narrowly-scoped rule such as this to the
existing `configs` array (read the whole config, amend it, then PUT the complete
object back):

```json
{
  "pattern": "endpoint/announcement/*",
  "roles": "*",
  "methods": "read",
  "actions": "*"
}
```

This permits an unauthenticated `GET /openidm/endpoint/announcement/{audience}`
and is appropriate for deliberately public, read-only data such as a login-page
announcement. `roles: "*"` is not a wildcard for an authenticated role; it
includes anonymous callers. Do **not** grant it to a broad `endpoint/*` pattern,
or permit write/action methods, and return only data intended to be public.

The calling hosted-page JavaScript is same-origin in the usual login-page
deployment. A browser app hosted on another origin also needs an AIC CORS
configuration; the access rule alone does not grant cross-origin browser access.

This behavior is documented by Ping's IDM authorization guide and is also the
mechanism used in Christian Brindley's announcement-at-login example. It has not
yet been re-exercised anonymously against this sandbox because no unlocked local
agent was available during the 2026-08-05 documentation pass.

### Authenticated user endpoints

Do not make an endpoint public merely so that it can receive a user bearer
token. Leave it protected and let IDM's built-in `rsFilter` authenticate the
token before the endpoint script runs. The filter delegates validation to AM,
checks the scopes configured in `/openidm/config/authentication`, maps the token
subject to an IDM identity and populates `context.security`. The endpoint's
`config/access` rule then authorizes an IDM role:

```json
{
  "pattern": "endpoint/user-token-poc/whoami",
  "roles": "internal/role/openidm-authorized",
  "methods": "read",
  "actions": "*"
}
```

`config/access` authorizes by IDM role, not by OAuth scope. On the sandbox,
`rsFilter.scopes` is `["fr:idm:*"]`; that prerequisite applies across protected
`/openidm` routes and `config/access` cannot replace it for one endpoint.

The endpoint can apply an **additional**, endpoint-local scope check after
`rsFilter` has authenticated the token. Live probing found the validated scope
set at `context.oauth2.scopes`. The underlying token-info string is also at
`context.oauth2.rawInfo.scope`, space-delimited. Prefer the set:

```javascript
var requiredScope = "example:announcements:read";
if (
  !context.oauth2 ||
  !context.oauth2.scopes ||
  !context.oauth2.scopes.contains(requiredScope)
) {
  throw { code: 403, message: "Missing required OAuth scope" };
}
```

This supplements rather than bypasses the global `fr:idm:*` requirement. A
caller therefore needs the scope accepted by `rsFilter`, the role admitted by
`config/access`, and the endpoint-specific scope checked by the script. Do not
parse or verify the raw `Authorization` header inside the script: token
signature, issuer, expiry, and tenant validation remain `rsFilter`'s job.

Use a dedicated IDM authorization role for a real API. The default
`internal/role/openidm-authorized` is useful for the POC because the tenant's
subject mappings grant it to authenticated users, but it is broader than a
purpose-specific role.

## Endpoints

| Op          | Method   | Path                                | Accept-API-Version | Notes                                                                       |
| ----------- | -------- | ----------------------------------- | ------------------ | --------------------------------------------------------------------------- |
| List config | `GET`    | `/openidm/config?_queryFilter=true` | none required      | Returns **all** config objects; filter `_id` starting `endpoint/`.          |
| Read        | `GET`    | `/openidm/config/endpoint/{name}`   | none required      | `{name}` is the bare name (no `endpoint/` prefix in the call? — see below). |
| Create      | `PUT`    | `/openidm/config/endpoint/{name}`   | none required      | Returns **201** + echoes the object on create.                              |
| Update      | `PUT`    | `/openidm/config/endpoint/{name}`   | none required      | Returns **200** on replace of an existing object.                           |
| Delete      | `DELETE` | `/openidm/config/endpoint/{name}`   | none required      | Returns **200** + echoes the deleted object. Subsequent `GET` → 404.        |

The path segment after `/config/` is `endpoint/{name}` (e.g.
`/openidm/config/endpoint/test`). In the list response each object's `_id` is
the full `endpoint/{name}`; derive the bare name by stripping the `endpoint/`
prefix.

**No `Accept-API-Version` header is needed** — every call above was exercised
without one and succeeded. (Sending the AM `protocol=2.0,resource=1.0` value is
wrong here; omit it for `/openidm`.)

## Object shape (real example: `endpoint/test`)

```json
{
  "_id": "endpoint/test",
  "type": "text/javascript",
  "source": "(function () {\n  if (request.method === \"create\") { ... }\n})();",
  "description": "…",
  "globals": {}
}
```

- `source` is the JavaScript body as **plain text** (NOT base64 — contrast with
  AM scripts). For scripted endpoints `source` may also appear as a nested
  object `{ "source": "…", "type": "…" }` or be replaced by `"file": "…"`
  (file-backed); handle the string and nested-object forms, fall back to the raw
  config otherwise.
- `type` is a MIME-ish discriminator: `text/javascript` (scripted; also seen as
  `scripted`), `table`, `jdbc`.
- **No `_rev` field** on read or write — same as AM scripts, so conflict
  detection is content-based (see `04-scripts.md` "Conflict detection rule").
- **No `name` field** — the human name is the `_id` suffix.
- **`globals` is the endpoint global-bindings object.** A full-object PUT adding
  `{"globals":{"endpointConfig":{...}}}` made `endpointConfig` available to the
  script at runtime (verified 2026-08-06).

## Examples

```bash
# List endpoint config ids
curl -sS "$TENANT_BASE_URL/openidm/config?_queryFilter=true" \
  -H "Authorization: Bearer $TOKEN" -H "Accept: application/json" \
  | jq -r '.result[]._id | select(startswith("endpoint/"))'

# Read one
curl -sS "$TENANT_BASE_URL/openidm/config/endpoint/test" \
  -H "Authorization: Bearer $TOKEN"

# Create / update (illustrative — use a throwaway name)
curl -X PUT "$TENANT_BASE_URL/openidm/config/endpoint/my-endpoint" \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"type":"text/javascript","source":"(function(){ return {}; })();","description":"…"}'

# Delete
curl -X DELETE "$TENANT_BASE_URL/openidm/config/endpoint/my-endpoint" \
  -H "Authorization: Bearer $TOKEN"
```

## Scripted-endpoint runtime bindings (`request` / `context`)

Verified 2026-06-04 by creating a throwaway `endpoint/rhino-probe` that echoes
the `request` binding, then invoking it once per CREST method. The endpoint is
invoked at `/openidm/endpoint/<name>` (NOT `/config/...`), and a freshly-created
endpoint takes a few seconds to register (first calls 404 until it does).

`request.method` is the CREST verb, mapped from the HTTP call:

| HTTP call                                             | `request.method` | Method-specific fields (beyond the common set)                                                                                                         |
| ----------------------------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET /endpoint/x`                                     | `read`           | — (response carries an `_id` IDM adds; `""` at the root, the id for `/{id}`)                                                                            |
| `GET /endpoint/x?_queryFilter=…` or `?_queryId=…`     | `query`          | `queryFilter`, `queryId`, `queryExpression` (string\|null), `pageSize`, `pagedResultsOffset` (number), `pagedResultsCookie` (string\|null), `sortKeys` |
| `POST /endpoint/x` (optional `?_action=create`)       | `create`         | `newResourceId` (string\|null), `content`. HTTP 201, and the response body carries an `_id` IDM adds (probed 2026-08-18).                               |
| `POST /endpoint/x?_action=NAME`                       | `action`         | `action` (the action name), `content`                                                                                                                  |
| `PUT /endpoint/x/id` **with `If-Match`**              | `update`         | `revision` (string\|null), `content`; `resourcePath` = `id`                                                                                            |
| `PUT /endpoint/x/id` **without a conditional header** | `create`         | `newResourceId` = `id`, `content`; `resourcePath` = **`""`**. HTTP 201.                                                                                |
| `PATCH /endpoint/x/id`                                | `patch`          | `revision` (string\|null), `patchOperations`                                                                                                           |
| `DELETE /endpoint/x/id`                               | `delete`         | `revision` (string\|null)                                                                                                                              |

Common to every method: `method`, `resourcePath` (string; `""` at the endpoint
root), `additionalParameters` (a map of any non-`_` query params), `fields` (the
`_fields` list).

**IDM adds `_id` to every resource-shaped response.** `create`, `read`, `update`,
`delete` and `patch` all come back carrying it even when the handler never
returned it — a `read` of the collection root gets `_id: ""`, a `read` of `/{id}`
gets the id (probed 2026-08-18, all six methods). An `action` response is passed
through untouched, and a `query` gets the paging envelope instead. This is why
the generated OpenAPI document and the response validator legitimately differ:
the validator checks what the handler returns, the document describes what the
caller receives.

- **`patchOperations`** is a list of `{ operation, field, value }` (mirrored
  over the wire as an index-keyed object `{"0": {...}}` because it's a Java
  list). `operation` is
  `add`/`remove`/`replace`/`increment`/`move`/`copy`/`transform`, `field` is a
  JSON pointer (`/foo`). This is the strict body that has blocked mocking other
  PATCH APIs — the request must be CREST patch ops, not an arbitrary JSON body.
- **A bare `PUT /endpoint/x/{id}` is a CREST _create_, not an update** (probed
  2026-08-13; this table previously said `update` unconditionally, which was
  wrong). Without `If-Match` (or with `If-None-Match: *`) CREST routes it as
  create-with-client-supplied-id: `request.method` is `create`, `newResourceId`
  holds the id, `resourcePath` is **empty**, and the response is HTTP 201. With
  `If-Match: *` — or any revision — it is an `update` with `resourcePath` set to
  the id and HTTP 200. An endpoint that routes on `resourcePath` therefore sees
  a header-less `PUT .../{id}` arrive at its ROOT create handler. See
  `docs/api/99-quirks-and-open-questions.md` Q16.
- **`content`** is the request body for create/update/action.
- **`context`** is the CREST call chain. The originating HTTP request is at
  `context.http`: `{ method, path, headers (map), parameters (map) }`. Identity
  is at `context.security`:
  `{ authenticationId, authorization: { id, component, roles } }`. Many other
  contexts exist (`oauth2`, `transactionId`, `session`, `current`, `parent`, …)
  and vary by call.
- **Validated OAuth scopes are at `context.oauth2.scopes`.** Ping's backing
  `AccessTokenInfo` API defines this as a Java `Set<String>`, so endpoint code
  can use `context.oauth2.scopes.contains("scope-name")`. The same scopes are
  exposed as a space-delimited string at `context.oauth2.rawInfo.scope`. For the
  service-account probe these contained `fr:am:*`, `fr:idc:esv:*`, `fr:idm:*`,
  and `fr:idc:cookie-domain:*`. `context.oauth2.scope`,
  `context.oauth2.accessToken`, and `context.oauth2.accessToken.info` were not
  present. **The workspace types model this** — `IdmContext.oauth2` in
  `idm/types/common.d.ts` is optional, `scopes` is a `JavaSet`, and `token` /
  `rawInfo.sessionToken` are commented as credentials. So `tsc` rejects
  `scopes.includes(…)`, an unguarded `context.oauth2`, and the absent `.scope` /
  `.accessToken` (they fall to the index signature, which
  `noPropertyAccessFromIndexSignature` refuses on dot-access).
- **`context.oauth2.rawInfo` is AM's token-introspection record.** Full key
  inventory with types, verified 2026-08-07 against a service-account token:

  | Key                                        | Type      | Note                                                                                                                       |
  | ------------------------------------------ | --------- | -------------------------------------------------------------------------------------------------------------------------- |
  | `active`                                   | `boolean` |                                                                                                                            |
  | `auditTrackingId`                          | `string`  | Joins to the log API's tracking ids.                                                                                       |
  | `authGrantId`                              | `string`  |                                                                                                                            |
  | `client_id`                                | `string`  | `service-account` for an SA token.                                                                                         |
  | `exp`                                      | `number`  | Epoch **seconds**, not millis.                                                                                             |
  | `expires_in`                               | `number`  | Seconds remaining at introspection.                                                                                        |
  | `iss`                                      | `string`  | AM's **internal** URL (`https://am.fr-platform:443/am/oauth2`) — _not_ the tenant host. Don't compare it to your base URL. |
  | `realm`                                    | `string`  | `/` for a root-realm token.                                                                                                |
  | `scope`                                    | `string`  | Space-delimited; same values as `context.oauth2.scopes`.                                                                   |
  | `sessionToken`                             | `string`  | **CREDENTIAL.** Never return or log.                                                                                       |
  | `sub` / `subname` / `user_id` / `username` | `string`  | All four held the SA's UUID for a service-account token.                                                                   |
  | `token_type`                               | `string`  | `Bearer`.                                                                                                                  |

  A user token carries the same keys; only the identity values differ, and those
  variants are **not yet verified** — the JWKS cache left user-token minting
  unavailable during this probe. All of the above are typed in
  `idm/types/common.d.ts`.

- **Never return or log the full context.** `context.http.headers` includes the
  bearer `Authorization` header. Serializing `context.security` can also walk
  its inherited `parent` chain into `context.oauth2`, whose `token` and
  `rawInfo.sessionToken` fields are credentials. Reconstruct an explicit
  allowlist of safe diagnostic fields instead.
- **`context.http` is present only when an HTTP request sits at the ROOT of the
  chain — it is optional even inside a custom endpoint** (verified 2026-07-21).
  A direct REST call has it. An endpoint reached internally from _another_
  endpoint (`openidm.read`/`openidm.action`) whose origin was HTTP _still_ has
  it — the HTTP context is inherited down the chain and points at the
  originating caller, not the inner hop. But an endpoint reached from a
  **non-HTTP origin — a scheduled job, recon/liveSync, boot/startup, or any
  internal trigger — has `context.http === undefined`.** Live-verified with a
  disabled throwaway `schedule/aicedit-bindsched` that called
  `openidm.action("endpoint/aicedit-bindprobe", …)` from the scheduler thread:
  both the schedule script and the endpoint saw no `context.http` (result
  stashed in an `alpha_lock` record, then cleaned up). Always guard
  `context.http` before dereferencing it.

### Response shape

The script's returned value is the HTTP response body. A **`query` handler MUST
return** `{ result: [...], resultCount, pagedResultsCookie, … }` — returning a
plain object fails with
`500 "Script returned unexpected query result structure of type class java.util.HashMap"`.
Other methods return a resource object. The return value is not statically
type-checkable (a script isn't a typed function), but `idm/types/endpoint.d.ts`
exposes `IdmQueryResult` / `IdmResource` aliases to annotate with
`/** @type {…} */`.

IDM normalizes both query envelopes and thrown CREST faults. In a 2026-08-13
follow-up probe, a script-thrown `{ code, message, detail }` reached HTTP as
`{ code, reason, message, detail }`: IDM synthesized `reason` from the status;
the script did not need to supply it. The same run returned every required
query paging field, but IDM normalized `remainingPagedResults` to `-1` in the
HTTP response rather than preserving the script's computed value. Treat that
field as CREST-owned metadata, not an application counter.

## Requireable bundled libraries (`require('lib/<name>')`)

IDM scripts can `require()` a small, **fixed set of bundled CommonJS
libraries**. These are baked into the IDM scripting runtime — you **cannot**
push your own. The Ping scripting guide lists Lodash + Handlebars
([pingidm/8.1 scripting-guide preface](https://docs.pingidentity.com/pingidm/8.1/scripting-guide/preface.html)),
and we verified the full set 2026-06-22 with an `endpoint/aicedit-libprobe` that
`require()`d 110+ common library names (npm-style) in both `lib/<name>` and
bare-`<name>` forms. Only **three** resolved, and **only with the `lib/`
prefix**:

| `require(id)`               | Library      | Version    | Notes                                                                                                |
| --------------------------- | ------------ | ---------- | ---------------------------------------------------------------------------------------------------- |
| `require('lib/lodash')`     | Lodash       | **3.10.1** | The `_` function export. **v3, not v4** — e.g. `_.indexBy` (not v4's `_.keyBy`), no `_.fromPairs`.   |
| `require('lib/handlebars')` | Handlebars   | **4.7.7**  | Server-side use **requires a Synchronizer wrapper** — see below.                                     |
| `require('lib/validator')`  | validator.js | **13.7.0** | `v.isEmail(...)` etc. **Not** mentioned in the Ping docs; present-and-functional verified live only. |

**Workspace typing.** The script workspace (`src/scripts/templates/`) pins
`@types/lodash@3.10.1`, `handlebars@4.7.7` (ships its own types), and
`@types/validator@13.7.0` to these exact runtime versions, and
`idm/types/idm-libs.d.ts` maps `require('lib/<name>')` to them via typed
`require` overloads. So a v4-only lodash call (`_.keyBy`) correctly fails
type-check against the v3 surface (verified end-to-end with `tsc` 2026-06-22).
Available in every IDM script context (endpoint / schedule / managed-hook /
sync-mapping).

**Handlebars synchronization.** Per the Ping guide, calling Handlebars in a
server-side JS script must be wrapped in the Rhino `Synchronizer`:

```javascript
var Handlebars = require("lib/handlebars");
var out = new Packages.org.mozilla.javascript.Synchronizer(function () {
  return Handlebars.compile("Handlebars {{doesWhat}}")({ doesWhat: "rocks!" });
}, Handlebars)();
```

- **`lib/` prefix is mandatory.** `require('lodash')` (bare) →
  `Error: Module "lodash" not found.` `require('lib/lodash.js')` also resolves
  (suffix tolerated).
- **No other npm libs are bundled** — uuid, jwt/jose, crypto-js, moment, ajv,
  xml parsers, jsonpath, etc. all 404. Use `utils`/`openidm`/`httpClient`
  bindings or Java (`Date.now()`, `java.*` are allowed in IDM — see Quirks)
  instead.
- **You cannot push your own library.** Defining a CommonJS module in another
  endpoint's `source` and `require()`-ing it fails for every form tried:
  `require('aicedit-mylib')`, `'lib/aicedit-mylib'`, `'endpoint/aicedit-mylib'`,
  `'aicedit-mylib.js'` → `Module … not found`; `require('./aicedit-mylib')` →
  `Error: Can't resolve relative module ID "./..." when require() is used outside of a module`.
  Relative requires only resolve **inside** a bundled module (one `lib/` file
  requiring another). There is no SaaS-exposed path to add a `lib/` module in
  Identity Cloud (no filesystem access). To share code, inline it or call a
  shared endpoint over `openidm.action`/`httpClient`. For a complete AM + IDM
  design, see
  [Sharing code between AM and IDM](../sharing-code-between-am-and-idm.md). For
  the **build-time** answer — endpoints authored as TypeScript modules and
  bundled into one self-contained ES5 file per endpoint — see
  [TypeScript custom endpoints](../typescript-endpoints.md).
- **Scope:** `require`/`lib` resolution is a property of the IDM Rhino engine,
  so it is available to **every IDM script type**, not just custom endpoints —
  scripted endpoints, `invokeService:"script"` schedules, managed-object hooks
  (`onCreate`/`onUpdate`/virtual-property), sync-mapping transform/correlation
  scripts, and policy/router scripts. (`require` presence is independently
  verified for endpoints, schedules, and managed hooks — see
  `12-script-bindings-matrix.md` and the IDM `*.d.ts` templates.)
- **AM is different.** AM next-gen `require()` resolves **AM library scripts**
  (the `lib`-kind scripts you author in the realm), _not_ npm modules — there is
  no `lib/lodash` on the AM side. Don't conflate the two `require()` mechanisms.

## Quirks

- **IDM HTML-escapes every string in a thrown error object** before it reaches
  the caller (verified 2026-08-13). Throwing
  `{ code: 400, message: "must be <= 100" }` arrives as
  `"must be &lt;&#61; 100"`; `"` becomes `&#34;`, `'` becomes `&#39;`, and a
  backtick becomes `&#96;`. Nested `detail` strings are escaped too. Write error
  messages in plain words — `must be at most 100`, `Unknown action: explode` —
  or the caller reads entities.
- **`source` is plain text, not base64** (the opposite of AM scripts). Don't
  base64-encode on write.
- **No `_rev`** — content-based conflict detection only.
- **No `Accept-API-Version`** — `/openidm` config does not require (or want) the
  AM versioning header.
- **Missing ESV/property lookup is non-throwing.** In an IDM script,
  `identityServer.getProperty("esv.some.variable")` returns `null` when the
  ESV/property does not exist. Its optional second argument is the fallback:
  `identityServer.getProperty("esv.some.variable", "default")` returns
  `"default"`. Live-verified 2026-07-22 with a temporary scripted endpoint; use
  a fallback for optional ESVs or explicitly guard against `null`.
- **List is unfiltered** — `/openidm/config?_queryFilter=true` returns _every_
  config object (85 in the sandbox); filter client-side for `endpoint/` ids.
- **`PUT` is create-or-replace** — 201 on first write, 200 on replace.
- Some shipped endpoints (e.g. `oauthproxy`, `gettasksview`) are product
  defaults; treat with the same care as AM `default:true` scripts (avoid
  clobbering unless the user explicitly pulls + edits them).
- **`Date.now()` and `java.lang.Thread.sleep(ms)` both work** in IDM scripts
  (verified 2026-06-10 via a throwaway endpoint: `Date.now()` returns epoch ms;
  `Thread.sleep(250)` measured ~263ms — Java access is permitted, unlike AM
  next-gen which blocks reflection). Useful for retry/backoff loops (see the
  advisory-lock template `scripts/idm-recon-lock.template.js`).
- **Trailing comma in a function PARAMETER list compile-fails** (un-routable
  404), e.g. `function f(a, b,) {}`. Verified 2026-06-10. Trailing commas in a
  function _call_ argument list (`f(1, 2,)`) and in object/array literals are
  both fine. This is a third IDM syntax ban alongside default-params and
  `const`-in-for-initializer. Practical impact: prettier's default
  `trailingComma: "all"` will wrap long signatures and add the fatal comma — IDM
  script workspaces must use `trailingComma: "es5"` (the existing
  `templates/.prettierrc` and `workspace/*/.prettierrc` already do; a matching
  `scripts/.prettierrc` was added for the standalone template). Consider an
  eslint `comma-dangle: ["error", {"functions": "never"}]` for IDM as a guard.

## Scheduled jobs (`schedule/<name>`)

IDM scheduled jobs are config objects at `/openidm/config/schedule/<name>` —
same CRUD, no realm, no `_rev`. `pingone-aic-manager` syncs them as a second IDM
script kind (`--kind schedule`). They differ from endpoints in **where the
script lives** and **which ones have one**:

- **The script's nesting depends on `invokeService`, and there are two
  locations, not one.** Corrected 2026-08-15 — see the sweep note below; the
  earlier claim that only `script` schedules carry an inline script was wrong.

  | `invokeService`                | inline script at                   | seen in sweep |
  | ------------------------------ | ---------------------------------- | ------------- |
  | `script`                       | `invokeContext.script.source`      | 2 of 7        |
  | `org.forgerock.openidm.script` | `invokeContext.script.source`      | 1 of 7        |
  | `taskscanner`                  | `invokeContext.task.script.source` | 4 of 7        |

  In every case `…script.type` is `text/javascript` and a `globals` object may
  sit beside `source`.

- **`invokeService` has a fully-qualified alias.**
  `org.forgerock.openidm.script` behaves as `script` and nests its script the
  same way. A filter written as `invokeService == "script"` silently skips those
  schedules — match on the suffix, not equality.

- **`taskscanner` schedules do carry inline scripts.** All four in the latest
  sweep had 123–196 bytes of JavaScript at
  `invokeContext.task.script.source`. Their
  `invokeContext` also holds `numberOfThreads`, `waitForCompletion`, and a
  `scan` object with `object`, `_queryFilter`, `taskState` and (in one case)
  `recovery`. Do not filter taskscanner out when listing syncable schedules.

- **`globals` is emitted as `{}` even when empty**, in three of the six
  schedules — both under `invokeContext.script` and under
  `invokeContext.task.script`. A decoder that reads only `{source, type}` and
  re-encodes drops the key, so a whole-document write deletes it. No schedule in
  the sweep had a _non-empty_ `globals`, so whether it accepts nested or
  non-string values is **unverified** — a consumer that models it as a flat
  string map is guessing beyond the evidence.

- On write, round-trip the whole document. Merging only
  `invokeContext.script.source` and rebuilding the rest loses `globals`, the
  `scan` sub-objects, and (for taskscanner) the task script entirely.
  `schedule`, `enabled`, `persisted`, `type` and the cron definition must all
  survive.
- **`persisted` controls AIC console visibility.** Verified 2026-07-15 with
  `schedule/load-alpha-name-variant`: the config existed, was readable through
  `/openidm/config/schedule/...`, and could be triggered manually while
  `persisted:false`, but it did not appear in the AIC console. Changing only
  `persisted` to `true` made it appear; `enabled:false` was unchanged. Use
  `persisted:true, enabled:false` for a visible, manual-only schedule.
- **A schedule can be fired on demand, independent of its cron or `enabled`
  flag**, via `POST /openidm/scheduler/job/<name>?_action=trigger` (verified
  2026-07-14; header behaviour rechecked 2026-07-15 — `<name>` is the id segment
  after `schedule/`, not the full `_id`). **Omit `Accept-API-Version`**: sending
  `resource=1.0` to this scheduler action returns 501 `Not Implemented`, while
  the otherwise identical headerless call returns 200 `{"success":true}`. The
  script itself runs asynchronously in the scheduler's own job thread, decoupled
  from the HTTP response, so a long-running load script (tested: 500 sequential
  `openidm.create` calls in ~7s, extrapolates to ~11k in a few minutes) is not
  bounded by any request timeout. `enabled: false` with a cron string that will
  never fire on its own (e.g. a far-future date) is a safe way to deploy a
  script that should only ever run via an explicit manual trigger. The bare
  `_action=execute` guess 400s; the valid action names are enumerated in that
  error body: `create`, `listCurrentlyExecutingJobs`, `pauseJobs`, `resumeJobs`,
  `validateQuartzCronExpression`, `pause`, `resume`, `trigger`.
- **Modern JavaScript syntax works in schedule scripts.** Live-verified
  2026-07-15 with a disabled throwaway schedule: root-level `const` and `let`,
  `new Set()` (`add`, `size`, and `has`), and template-literal interpolation all
  executed successfully. The probe used `openidm.create` to write
  `root:2:2:true`; both its temporary record and schedule were then deleted.
- **Do not declare `const` inside a schedule loop body.** A 2026-07-15 live
  probe parsed and started but silently terminated the loop after its first
  iteration: the expected sum was 3 and the created result was 0. (`Set.add`
  still worked in that probe, increasing the set size to 3.) The same
  `for (let ...)` loop without a loop-body `const` completed correctly. Use
  `let` for every binding declared in a `for`/`while`/`do-while` body, including
  nested blocks that execute repeatedly; root-level immutable declarations may
  remain `const`.

Object shape (real example, `schedule/UpdateReviewList`):

```json
{
  "_id": "schedule/UpdateReviewList",
  "enabled": true,
  "type": "cron",
  "schedule": "0 0 2 * * ?",
  "persisted": true,
  "invokeService": "script",
  "invokeContext": {
    "type": "text/javascript",
    "script": {
      "type": "text/javascript",
      "source": "var oneDay = …",
      "globals": {}
    }
  }
}
```

> When creating a schedule for testing, set `"enabled": false` so it doesn't
> actually fire on its cron. Keep `"persisted": true` if it should remain
> visible in the AIC console.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-06-01 (CRUD); 2026-06-04 (`request`/`context` runtime shapes per
  method, via `endpoint/rhino-probe` echo — created, probed read/create/update/
  patch/delete/action/query, deleted); 2026-06-22 (bundled `require('lib/*')`
  libraries — `lib/lodash` 3.10.1, `lib/handlebars` 4.7.7, `lib/validator`
  13.7.0; 110+ other names 404; custom/own-module requires all fail; throwaway
  `endpoint/aicedit-libprobe{,2,3}`, `aicedit-mylib`, `aicedit-func` created,
  probed, and deleted); 2026-07-21 (`context.http` is inherited down internal
  endpoint→endpoint calls but ABSENT when the chain root is a scheduled job.
  Throwaway `endpoint/aicedit-bindprobe`, `endpoint/aicedit-bindcaller`,
  `schedule/aicedit-bindsched` created, probed via HTTP + internal +
  scheduler-triggered paths, and deleted); 2026-07-24 (AM library →
  `openidm.action` → IDM endpoint invocation, plus action response envelopes
  carrying object, number, string, and `null` results); 2026-08-06 (protected
  bearer-token endpoint, `config/access` role gate, endpoint `globals`, and
  OAuth2 context/scope bindings); 2026-08-07 (`context.oauth2.rawInfo` full key
  inventory + types); 2026-08-15 (schedule script nesting and `globals`, from a
  **full-realm sweep of all 6 `config/schedule/*` documents** rather than a
  fresh probe — the responses were captured by `pingoneaic-tf` on 2026-08-14 and
  are committed as fixtures in `terraform-provider-pingone-aic` under
  `internal/client/testdata/schedules/`. This corrected the "only
  `invokeService: script` carries an inline script" claim, added the
  `org.forgerock.openidm.script` alias, and recorded `globals: {}`. Nothing was
  written to the tenant.); 2026-08-18 (throwaway `endpoint/aic-action-probe`:
  bare `POST` and `POST ?_action=create` both reached the create route and
  returned **201**; `?_action=ping` reached the named action and returned 200; an
  unknown action returned 404 listing the supported actions. **Both** create
  responses carried an `_id` equal to the resource path, which the handler never
  returned and which the TypeScript framework contains no code to add — so IDM's
  create wrapper supplies it. The endpoint was deleted and its config absence
  confirmed by a 404. This run corrected the table row below, which said a create
  was `POST ?_action=create`; the 2026-06-04 `rhino-probe` run it was attributed
  to never tested a bare `POST`. A second run the same day, throwaway
  `endpoint/java-probe`, then swept **all six methods** for that `_id`: `read`
  (`_id: ""` at the collection root, `_id: "abc123"` for `GET /{id}`), `update`
  (`PUT /{id}` with `If-Match: *` → `{"_id":"upd1","updated":"upd1"}`), `delete`,
  and `patch` all carry it; an `action` response does **not**
  (`POST ?_action=ping` → `{"pong":"yes"}`); a query returns the paging envelope
  (`result`, `resultCount`, `pagedResultsCookie`, `remainingPagedResults`,
  `totalPagedResults`, `totalPagedResultsPolicy`) with no `_id` of its own. The
  same run confirmed the conditional-header rule from the tenant rather than from
  this document: `PUT /{id}` **without** `If-Match` on an endpoint declaring only
  read+action was refused as `"Method create is not supported on this path"`,
  with `detail: {"allowed":["read","action"]}` — so a bare PUT really is
  dispatched as a create. It also recorded the Rhino Java surface: `Mac.getInstance`
  / `init` / `doFinal` and `new java.lang.String(s).getBytes("UTF-8")` are live
  spellings, `doFinal` returns a `"[B"` (not a JS array) whose elements are
  **signed** (first byte of the published `HMAC-SHA256("key", "The quick brown
  fox…")` vector came back as `-9`), `java.text.Normalizer.normalize` returns a
  `java.lang.String` that is **not** `===` a JS literal of the same text but
  *does* expose JS string methods such as `.slice`, no-arg `getBytes()` works
  (platform charset), and `openidm.hash(input, null)` returns a different salted
  hash on each call. Both endpoints were deleted and their absence confirmed by a
  404.)
- Endpoints: `GET /openidm/config?_queryFilter=true` (200; 85 objects, 12 with
  `endpoint/` ids), `GET /openidm/config/endpoint/test` (200; keys
  `_id, description, source, type`, no `_rev`, plaintext `source`),
  `PUT /openidm/config/endpoint/aicedit-verify` (201 create), `PUT` again (200
  replace), `DELETE` (200), `GET` after delete (404).
- Schedules (2026-06-01; script-location interpretation superseded by the
  2026-08-15 and 2026-08-17 sweeps): `GET …?_queryFilter=true` → 4
  `schedule/` configs, 3 `taskscanner` (their nested task scripts were not
  recognised in this run) + 1 `invokeService:"script"`
  (`UpdateReviewList`, script at `invokeContext.script.source`, no `_rev`).
  Throwaway `schedule/aicedit-sched` (disabled): `PUT` 201 create, `PUT` 200
  replace, source-only push preserved `enabled`/`schedule`/`script.type`,
  `DELETE` 200. Removed after the run.
- Schedule nesting recheck (2026-08-17): 7 `schedule/` configs: 2 `script`, 1
  `org.forgerock.openidm.script`, and 4 `taskscanner`. All 7 carried inline
  source at the paths in the table above; the 4 taskscanner sources were
  123–196 bytes.
- Manual trigger (2026-07-14, name-variants-au load):
  `schedule/aicedit-trigger-probe` (`enabled:false`, far-future cron) created,
  `POST …/scheduler/job/<name>?_action=trigger` → 200 `{"success":true}`, target
  `managed/test_name_variant` record created moments later — confirms
  `enabled:false` does not block manual trigger. Scaled test: 500-row
  embedded-data load script triggered the same way, ~7s wall time to all 500
  records existing (polled via `_pageSize=0` count). Throwaway schedule,
  records, and `managed` schema entry all removed after.
- Console visibility (2026-07-15): `schedule/load-alpha-name-variant` with
  `enabled:false, persisted:false` was present and manually runnable through the
  API but absent from the AIC console. A full-object PUT changing only
  `persisted:true` made it visible while it remained disabled and manually
  triggerable.
- Schedule JavaScript syntax (2026-07-15): disabled throwaway
  `schedule/aicedit-idm-modern-syntax-probe` was manually triggered and
  successfully exercised root-level `const`/`let`, `Set`, template literals, and
  `openidm.create`. Its temporary managed-object record and schedule were
  deleted after verification. A follow-up loop probe showed that a `const`
  declared in the loop body silently stops the loop after its first iteration
  (sum 0 instead of 3), while `for (let ...)` with no body `const` iterates
  correctly.
- `identityServer.getProperty` missing ESV behavior (2026-07-22): temporary
  `endpoint/aicedit-missing-esv-probe` called
  `getProperty("esv.aicedit.definitely.nonexistent.20260722")`, which returned
  `null` without throwing; the same call with fallback
  `"aicedit-fallback-value"` returned that string. Endpoint deleted after the
  probe.
- Authenticated endpoint POC (2026-08-06): deployed `endpoint/user-token-poc`
  with exact-path read access for `internal/role/openidm-authorized`. A valid
  tenant service-account token returned 200 and a populated `context.security`
  (`component: "managed/svcacct"`); no token returned 403 from the access layer;
  a malformed bearer returned 401; and a valid token with the script's role
  allowlist set to a nonexistent role returned the script's 403. Restored the
  intended allowlist after the negative test. `/openidm/config/authentication`
  showed `rsFilter.scopes: ["fr:idm:*"]` and subject mappings with default role
  `internal/role/openidm-authorized`. A real end-user token has not yet been
  exercised.
- Authenticated endpoint scope probe (2026-08-06): the OAuth2 context exposed
  keys `class`, `name`, `rawInfo`, `token`, `scopes`, `expiresAt`, and `parent`.
  `context.oauth2.scopes` was a Java collection containing the token's four
  validated scopes; `context.oauth2.rawInfo.scope` held the same values as a
  space-delimited string. `context.oauth2.scope` and `accessToken` were absent.
  The first diagnostic serialized `context.security` directly and thereby
  followed its `parent` chain into OAuth2 credentials. The deployed diagnostic
  was immediately replaced with an explicit safe-field projection and the local
  agent was locked to clear its cached token. This confirms that neither
  `context.security` nor the complete `context` is safe to serialize.
- Bundled TypeScript endpoints (2026-08-13): throwaway
  `endpoint/aicdemo-a1-claude-widgets` (8 routes) and
  `endpoint/aicdemo-a1-claude-reports` (3 routes) — esbuild+Babel ES5 bundles of
  ~45 KB and ~39 KB — created (`PUT` 201), exercised across every CREST method
  (`query`, `read`, `create`, `update`, `patch`, `delete`, two named actions)
  plus multi-segment sub-paths (`daily/{date}`, `widget/{id}/summary`) and the
  failure cases (bad path param 400, out-of-range int 400, unknown enum 400,
  unknown body field 400, empty patch-op list 400, unknown widget 404, unmatched
  sub-path 404, unknown action 404, unsupported method 405, malformed
  `x-request-id` 400, missing scope 403), then deleted (`DELETE` 200; `GET` on
  both the config and runtime URLs → 404). Three behaviours established by that
  run: a header-less `PUT .../{id}` is a `create` (table above); thrown error
  strings are HTML-escaped (Quirks); and a follow-up fault-projection probe
  established that IDM adds `reason` to a script-thrown
  `{ code, message, detail }` rather than returning the thrown object verbatim.
  The follow-up query probe returned a complete paging envelope and showed IDM
  normalizing `remainingPagedResults` to `-1`. `context.oauth2.scopes.contains(...)` was
  checked in **both** directions — the service-account token was refused a scope
  it does not hold (403) and admitted for `fr:idm:*`, which it does (200) — so
  the 403 is a real scope check rather than a broken call.
- `rawInfo` key/type inventory (2026-08-07): throwaway
  `endpoint/aicedit-rawinfo-probe` created (`PUT` 201), called with a
  service-account bearer (200), and deleted (`DELETE` 200, `GET` after → 404).
  It returned `Object.keys(rawInfo)` plus `typeof` for each — never the object
  itself, and `sessionToken`'s type only, not its value. Fifteen keys, all as
  tabled above. The `scope` value matched `SA_SCOPES` exactly
  (`fr:am:* fr:idc:esv:* fr:idm:* fr:idc:cookie-domain:*`), and `iss` was AM's
  internal `https://am.fr-platform:443/am/oauth2` rather than the tenant host.
  **User-token variant not covered**: an attempt to mint one for the same probe
  failed because the realm's JWKS cache was still serving a stale key set from
  the revocation probing earlier that day, so the identity-bearing values
  (`user_id`, `username`, `subname`) are verified for a service account only.

## Source citations

- frodo-lib: `src/api/IdmConfigApi.ts` (`getConfigEntity` / `putConfigEntity`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/endpoints` (IDM
  endpoints).
- p1aic-script-editor: `src/resources/EndpointHandler.ts`,
  `src/schemas/endpoint.ts` (Zod schema covers scripted/table/jdbc + nested
  `source`).
- Ping AIC:
  [Authorization and roles](https://backstage.forgerock.com/docs/idcloud/latest/idm-auth/authorization-and-roles.html)
  (direct HTTP authorization, `config/access`, and `roles: "*"` rules).
- Ping AIC:
  [Authentication through OAuth 2.0 and subject mappings](https://docs.pingidentity.com/pingoneaic/idm-auth/rsfilter-module.html)
  (`rsFilter` token validation, required scopes, subject mapping and roles).
- Ping AM API:
  [AccessTokenInfo](https://docs.pingidentity.com/pingam/7.4/_attachments/apidocs/org/forgerock/http/oauth2/AccessTokenInfo.html)
  (`getScopes()` returns `Set<String>`).
- Christian Brindley:
  [Making announcements at login](https://medium.com/@christian.brindley/pingone-advanced-identity-cloud-making-announcements-at-login-how-to-848b3b948fd1)
  (public `endpoint/announcement/*` read-rule example).

## Open questions

- Does `PUT` accept the legacy-looking `globalsObject` field? Not tested. Use
  `globals`, which is live-verified and available to endpoint source at runtime.
- Table/JDBC endpoint write shapes are documented from the p1-sync Zod schema
  only — not yet exercised live (sandbox has only scripted endpoints).
