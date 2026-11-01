# IDM script runtime

## Purpose

What an IDM script observes at runtime: the Rhino engine configuration, how
`openidm.*` results look to a script, the managed-object hook order and
bindings, and what custom endpoints do with return values and throws. This is
the ground truth for the local tenant emulator in `packages/rhino-local`, which
must reproduce it. Binding _presence_ per context is in
`12-script-bindings-matrix.md`; this file is about _behaviour_.

## Authentication

Service-account bearer with `fr:idm:*`. Every row was measured with
`POST /openidm/script?_action=eval` (see `12-script-bindings-matrix.md`, "Probe
IDM with `script?_action=eval`"), a throwaway `config/endpoint/zzemu-probe`, or
`scripts/experiment-managed-lifecycle-trace.sh`.

**Never serialise `context` in a probe.** `String(context)` and
`JSON.stringify(context)` include the oauth2 context's `token` and the AM
`sessionToken` of the caller, in clear, and the capture redactor does not
recognise a JWE. Read the fields you need by name (see `10-managed-objects.md`,
"Caller identity through a custom endpoint").

## Verified against

Sandbox tenant, IDM bundles `9.0.0-20261019…`, **2026-11-01**.

## Engine configuration

Read from inside a script: IDM has no class shutter, so
`org.mozilla.javascript.Context.getCurrentContext()` is reachable.

| Property                    | IDM                                                                                                                                                                      | AM, for contrast (`packages/rhino-local/java/AmContextFactory.java`) |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| Rhino                       | `org.forgerock.openidm:rhino`, an OSGi repackaging of upstream **1.7.14.1** (`Export-Package` version `1.7.14.1`; `getImplementationVersion()` reports `Rhino Snapshot`) | 1.7.14.1                                                             |
| Language version            | 200 (`VERSION_ES6`)                                                                                                                                                      | per context                                                          |
| Optimisation level          | **9** — scripts compile to JVM bytecode                                                                                                                                  | -1 (interpreted)                                                     |
| Instruction observer        | threshold 0 — no script timeout                                                                                                                                          | every 1,000 instructions                                             |
| Context factory             | plain `ContextFactory`; `hasFeature` true for 3, 5, 6, 14, 16, 20 only; 21 (Java map access) false; 22 throws                                                            | custom; 21 true                                                      |
| Max interpreter stack depth | `Integer.MAX_VALUE`                                                                                                                                                      | 10000                                                                |
| Wrap factory                | plain; `isJavaPrimitiveWrap()` true                                                                                                                                      |                                                                      |
| JVM                         | JDK 21.0.12.1, `Etc/UTC`, default locale `en`                                                                                                                            | Temurin 25, `Etc/UTC`, `en-US`                                       |

The `(<hash>:script-reg-svc#13)` suffix on an engine error's message names the
compiled script, not a source file.

### The global scope

- Rhino's **shell `Global`**: `readFile`, `runCommand`, `spawn`, `sync`,
  `readUrl`, `defineClass`, `serialize`, `quit`, `load`, `loadClass`, `print`,
  `version`, `help`, `gc`, `seal`, `toint32` are all functions. `sleep` is
  undefined. `atob`/`btoa` work (`btoa("hi") === "aGk="`), `secureRandom` is an
  object.
- Its prototype is IDM's deferred-binding scope. **`for (p in this)` at top
  level fails the whole request with a 500 that `try`/`catch` cannot catch**
  (`UnsupportedOperationException: There is no way know all the bound ids…`).
  The same holds inside a managed hook, where it also rolls the write back
  (`10-managed-objects.md`).
- `json` and `util` are undefined; `org` is a Java package object.

## How results look to a script

`openidm.read`, `openidm.query`, `openidm.hash`, `openidm.encrypt` return a
**`ScriptableMap`**: a Java-backed object, not a `NativeObject`.

| Probe on a `ScriptableMap` `r`                           | Result                                                               |
| -------------------------------------------------------- | -------------------------------------------------------------------- |
| `Object.prototype.toString.call(r)`                      | `[object ScriptableMap]`                                             |
| `Object.getPrototypeOf(r) === Object.prototype`          | `true`; `r instanceof Object`, `r.constructor === Object` too        |
| `Object.keys(r)`, `for…in`, `"k" in r`, `hasOwnProperty` | all work                                                             |
| `Object.getOwnPropertyNames(r)`                          | **empty**                                                            |
| `Object.getOwnPropertyDescriptor(r, "_id")`              | `undefined`                                                          |
| `String(r)`                                              | pretty-printed JSON text (`{ "objects": [ …`), not `[object Object]` |
| `r.o = {b:1, a:2, c:3, zz:4, aa:5}; Object.keys(r.o)`    | `zz,aa,a,b,c` — the assigned object became a `HashMap`               |
| `r.o === r.o` after that assignment                      | `true`                                                               |
| `r.x = Infinity`                                         | reads back as `9223372036854775807` (`Long.MAX_VALUE`)               |
| `r.y = -0`, `r.w = NaN`                                  | `0`, `NaN`                                                           |
| `delete r.k`                                             | works                                                                |

Lists inside a result (`r.objects`, `q.result`) report as arrays —
`Array.isArray` true, `[object Array]`, `map`/`forEach`/`filter`/`indexOf`
present, `Object.keys` gives `"0","1",…` — but they are **not** native arrays:

| Probe                      | Result                                                                   |
| -------------------------- | ------------------------------------------------------------------------ |
| `JSON.stringify(q.result)` | **`{"0":{…},"1":{…}}`** — an object, not `[…]`                           |
| `JSON.stringify(q)`        | `"result":[…]` — an array when stringified as a member                   |
| `String(q.result)`         | pretty-printed JSON array text                                           |
| `r.t = [1, null, 3]`       | `length` 3, **`1 in r.t` false**, `map` visits all three (`v1,vnull,v3`) |

Scalars convert to JS primitives: strings `typeof "string"` (not
`java.lang.String`), numbers `number`, booleans `boolean`.

### `openidm.*` call forms

| Call                                                        | Result                                                                                                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `openidm.read(missingId)`, `openidm.read("unrouted/path")`  | `null`, no throw                                                                                                                                       |
| `openidm.read(5)`                                           | throws `JavaException: java.lang.NoSuchMethodException: Method not found read(Integer)`                                                                |
| `openidm.delete(id)` (one argument)                         | throws `NoSuchMethodException: Method not found delete(String)` — pass the revision (`null`) explicitly                                                |
| `openidm.delete(missingId, null)`                           | throws `JavaException: org.forgerock.json.resource.NotFoundException: …`                                                                               |
| `openidm.action(res, name, {}, null)`                       | throws `NoSuchMethodException: Method not found action(String,String,HashMap,null)` — omit the null instead                                            |
| `openidm.action("managed/<obj>", "unknown", {})`            | `BadRequestException: Expecting String containing one of: patch triggerSyncCheck updateLastSync`                                                       |
| `openidm.query(res, {})`                                    | `BadRequestException: Only one of [_queryId, _queryExpression, _queryFilter] is supported; multiple detected`                                          |
| `openidm.query(res, {…}, function (row) {…})`               | the function is called once per row; the result has **no** `result` or `resultCount` keys                                                              |
| `openidm.query(res, openidm.read, {…})`                     | a function argument that is not last is dropped; this behaves as `query(res, {…})`                                                                     |
| `openidm.query(res, params, ["_id"], fn)` | a fields array as its own argument is a `BadRequestException: The value '[_id]' for additional parameter '_fields' is not of expected type String`; `_fields: ["_id"]` inside `params` works |
| `_queryExpression` on a DS-backed repo                      | `NotSupportedException: Query Expressions are not supported when using DS as the repo`                                                                 |
| `openidm.parseFilter('/a eq "b" and /c pr')`                | `{"subfilters":[{"field":"/a","value":"b","operator":"eq"},{"field":"/c","operator":"pr"}],"operator":"and"}`; `!` is `{"subfilter":…,"operator":"!"}` |
| `openidm.hash("x", "SHA-256")`                              | a `ScriptableMap` with one key, `$crypto`; `isHashed` true; `matches("x", h)` true                                                                     |
| `openidm.encrypt("x", null, "idm.password.encryption")`     | `{$crypto: …}`; `isEncrypted` true; `decrypt` gives `"x"`                                                                                              |
| `String(openidm.read)`                                      | throws `InternalError: Primitive type expected (had …ScriptableFunction instead)`                                                                      |

A failed call throws a Rhino `JavaException`: `e.name` is `"JavaException"`,
`e.message` is `"<exception class>: <message>"`, `Object.keys(e)` is
`fileName,lineNumber`, and `e.javaException` is the CREST exception —
`getCode()` gives the status (e.g. 403) and `getDetail()` the detail (e.g. a
policy failure's `failedPolicyRequirements`).

### `logger`, `console`, `identityServer`

- `logger.info("v={}", {a:1})` works (slf4j placeholders).
  **`logger.info({a:1})` throws**
  `NoSuchMethodException: Method not found info(HashMap)` — a lone non-string
  argument has no overload.
- `console` has one key, `log`.
- `identityServer.getProperty(name, default)` returns the default for a missing
  property, of any type (`{a:1}` comes back as an object). A third argument must
  be a boolean: `getProperty(name, 1, "yes")` is `NoSuchMethodException`.

### `require` and the bundled libraries

`require("lib/lodash")` returns the bundled lodash. **Its exports object is
shared across scripts and requests**: a property set on it by one request was
visible to the next, until deleted.

## Managed-object hooks

Measured by `scripts/experiment-managed-lifecycle-trace.sh`. It installs a
throwaway type in which every hook records its phase and bindings, then drives
the requests below.

### Order

| Request                                                                  | Hooks, in order                                                                                                                                                              |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| create (`PUT …/id` with `If-None-Match: *`, or `?_action=create`)        | `onCreate` → property `onValidate` (each) → `onValidate` → property `onStore` (each, in schema order) → `onStore` → `postCreate` → `onSync`                                  |
| read                                                                     | `onRetrieve` → property `onRetrieve` (each) → `onRead`                                                                                                                       |
| read with `_fields=v`                                                    | `onRetrieve` → `onRead`; property `onRetrieve` of an unrequested field does not run                                                                                          |
| query                                                                    | **none**                                                                                                                                                                     |
| query with `executeOnRetrieve=true`                                      | `onRetrieve` → property `onRetrieve`, per row; **no `onRead`**                                                                                                               |
| `PUT` with `If-Match`                                                    | a full read (`onRetrieve` → property `onRetrieve` → `onRead`) → `onUpdate` → property `onValidate` → `onValidate` → property `onStore` → `onStore` → `postUpdate` → `onSync` |
| `PATCH`                                                                  | same as `PUT` with `If-Match`                                                                                                                                                |
| `PATCH` that changes nothing                                             | the read hooks only; no write, no `postUpdate`, no `onSync`                                                                                                                  |
| `PUT` to an existing id with **no** `If-Match` and no `_rev` in the body | the **whole create chain first** (`onCreate` → … → `onStore`), then the update chain (`onUpdate` → … → `onSync`). The create attempt's changes are discarded                 |
| create on an existing id with `If-None-Match: *`                         | the create chain through `onStore`, **then** `412 Entry Already Exists`                                                                                                      |
| delete                                                                   | `onDelete` → `postDelete` → `onSync`; the response is the deleted object                                                                                                     |
| custom action (`POST …/id?_action=<name>`)                               | a read (read hooks) → the action script → a second read; **the response is that second read; the script's return value is discarded**                                        |
| custom action on the collection                                          | `400 Expecting String containing one of: patch triggerSyncCheck updateLastSync`                                                                                              |

`onSync` runs after every create, update and delete, with or without a sync
mapping. A `PUT` carrying `_rev` in its body takes the update path (the read
hooks run first), like `If-Match`.

### Bindings

| Hook                             | Bindings present                                                                       | Notes                                                                                                                                         |
| -------------------------------- | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `onCreate`                       | `object`, `oldObject` (null), `newObject`, `request`, `context`, `resourceName`        | `object` is the request content; `newObject` has the same keys. Mutating `object` persists                                                    |
| `onUpdate`                       | same                                                                                   | `object`/`newObject` are the new content, `oldObject` the stored record with `_id`/`_rev`. Mutating `object` persists                         |
| `onValidate`, `onStore`          | `object`, `value`, `request`, `context`, `resourceName`                                | no `oldObject`/`newObject`. Mutating `object` in `onStore` persists                                                                           |
| `postCreate`, `postUpdate`       | `object`, `oldObject`, `newObject`, `request`, `context`, `resourceName`               | `object` is the stored record including `_id`, `_rev`                                                                                         |
| `onRetrieve`, `onRead`           | `object`, `oldObject` (null), `newObject` (null), `request`, `context`, `resourceName` | mutating `object` changes the response                                                                                                        |
| `onDelete`                       | `object`, `oldObject`, `newObject` (null), …                                           | `object` and `oldObject` are the stored record                                                                                                |
| `postDelete`                     | `object` (**null**), `oldObject`, `newObject` (null), …                                |                                                                                                                                               |
| `onSync`                         | `object` (**null**), `oldObject`, `newObject`, …                                       | `oldObject` null on create; `newObject` null on delete                                                                                        |
| property `onStore`, `onRetrieve` | `property`, `propertyName`, `object`, `request`, `context`, `resourceName`             | **no `value`**. The script's completion value **replaces the property**, always: a script whose last statement has no value sets it to `null` |
| property `onValidate`            | `property`, `request`, `context`, `resourceName`                                       | **no `object`, no `value`, no `propertyName`**                                                                                                |
| custom action                    | (see the order table)                                                                  |                                                                                                                                               |

`object` and its siblings are `[object ScriptableMap]`; `request` is a
`Scriptable<Method>Request` (`request.method` is `create`, `read`, `update`,
`patch`, `delete`); `resourceName` is a Java object (`[object JavaObject]`)
whose `String()` is `managed/<obj>/<id>`.

Property hooks run on **every** write, including writes that do not touch the
property. A property `onStore` therefore sees the _stored_ value on a `PATCH` of
another field and is applied again: `"a"` stored as `"a-stored"` became
`"a-stored-stored"` after an unrelated `PATCH`. It also runs when the property
is absent, with `property === null`: a server-id create without the field stored
`"null-stored"`.

### Errors

| Thrown from                                                     | Response                                                                                                                |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `onCreate`: `throw {code: 418, message: "tea", detail: {a: 1}}` | `418`, `{"code":418,"reason":"Resource Exception","message":"tea","detail":{"a":1}}`; nothing stored, no later hook ran |
| `onCreate`: `throw new Error("plain error")`                    | `500`, message `Internal Server Error`                                                                                  |
| property `onValidate`: `throw "bad pval"`                       | `403`, message `bad pval`                                                                                               |
| property `onValidate`: a `ReferenceError`                       | `403`, message `javax.script.ScriptException: ReferenceError: …`                                                        |

A property typed `string` whose value is `null` fails policy (`VALID_TYPE`,
`invalidType: "null"`) on the next write even with `nullable: true`; a property
`onStore` that returns nothing therefore poisons every later full-body `PUT`.

## Custom endpoints

Measured with a throwaway `config/endpoint/zzemu-probe`.

| Request                      | `request.method` | `request` keys                                    | `resourceName` / `request.resourcePath` |
| ---------------------------- | ---------------- | ------------------------------------------------- | --------------------------------------- |
| `GET endpoint/x`             | `read`           | `method,fields,resourcePath,additionalParameters` | `""`                                    |
| `GET endpoint/x/a/b`         | `read`           | same                                              | `"a/b"`                                 |
| `POST endpoint/x?_action=go` | `action`         | `+ action, content`                               | `""`                                    |
| `POST endpoint/x`            | `create`         | `+ content, newResourceId`                        | `""`                                    |
| `PUT endpoint/x/id1`         | **`create`**     | `+ content, newResourceId`                        | `""`                                    |
| `PATCH endpoint/x/id1`       | `patch`          | `+ patchOperations, revision`                     | `"id1"`                                 |
| `DELETE endpoint/x/id1`      | `delete`         | `+ revision`                                      | `"id1"`                                 |

`resourceName` is an object (`typeof`), `object` is undefined, and
`context.http.parameters` is an object.

| Script returns / throws                           | Response                                                                                                              |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| an object, for read/create/patch/delete           | that object with `_id` set to the last path segment (`""` at the root); create is `201`                               |
| an object, for an action                          | the object, no `_id`                                                                                                  |
| `null` or `undefined`, for read                   | `404 script returned null`                                                                                            |
| `null`, for an action                             | `200`, body `null`                                                                                                    |
| `null`, for a query                               | `200`, empty `result`                                                                                                 |
| an object, for a query                            | `500 Script returned unexpected query result structure of type class java.util.HashMap`                               |
| an array, for a query                             | the rows as `result`; a row with `id` and no `_id` gains `_id` (its `id` kept)                                        |
| an array, string or number, for read              | `200` with a **truncated, invalid body** (`{"_id":""`)                                                                |
| a nested value                                    | `Date` → `{}`, `NaN` → `"NaN"`, `Infinity` → `9223372036854775807`; `{b:1,a:2}` → `{"a":2,"b":1}`                     |
| `throw {code: 400}`                               | `400`, message `[object Object]`, `detail: {}`                                                                        |
| `throw {code: 409, message: "…", detail: {k: 1}}` | `409` with that message and detail                                                                                    |
| `throw {code: "404"}`                             | `500` with an **empty body**                                                                                          |
| `throw new Error("x")`                            | `500`, message `Error: x`                                                                                             |
| `throw "x"`                                       | `500`, message `x`                                                                                                    |

## Not yet measured

Read from IDM's bytecode while preparing these probes, and **not** verified live
— treat as hypotheses:

- A `PUT` whose content equals the stored record runs no hooks and does not bump
  `_rev`. (The `PATCH` row above is verified; the `PUT` probe was confounded by
  a `null` property failing policy.)
- Object-level `onStore` sees encrypted/hashed property values, and its `value`
  binding is the copy taken before `onValidate`.
- A throw from `onSync` fails the request after the write has committed.
- `policyFilter.js` runs on an external `PUT`'s raw body before any hook;
  `PATCH` runs `policy.js` per patched property even for internal callers.
- Scheduled scripts receive `object` as the schedule's `invokeContext.input`.
