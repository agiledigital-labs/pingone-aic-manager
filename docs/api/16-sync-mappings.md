# IDM sync mappings (`config/sync`) + mapping script bindings

Implemented in: `src/scripts/`

## Purpose

IDM **sync mappings** reconcile a source resource (a connector
`system/<connector>/<objectType>` or a managed object `managed/<obj>`) onto a
target managed/connector object. Each mapping can embed JavaScript in several
slots: whole-mapping **behaviour scripts** (`onCreate`, `onUpdate`, …), a
**correlation** script, **valid-source/target** filters, a recon **result**
script, and per-attribute **transform**/**condition** scripts.
`pingone-aic-manager` syncs those embedded scripts to the local workspace (one
file per slot) with full TypeScript typing of the runtime bindings.

## Authentication

Service-account bearer (the `fr:idm:*` scope reaches `/openidm/*`). Same token
the rest of the IDM features use. No log-API key needed.

## Endpoints

| Op                   | Method | Path                                                             | Accept-API-Version | Notes                                                                                                                                                                                                             |
| -------------------- | ------ | ---------------------------------------------------------------- | ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Read all mappings    | GET    | `/openidm/config/sync`                                           | _(none)_           | Single document: `{ _id:"sync", mappings:[…] }`. **No `_rev`.**                                                                                                                                                   |
| Write all mappings   | PUT    | `/openidm/config/sync`                                           | _(none)_           | **Whole-document replace** (RMW). Applies with lag — poll-verify after write, exactly like `/openidm/config/managed`.                                                                                             |
| Start reconciliation | POST   | `/openidm/recon?_action=recon&mapping=<name>`                    | _(none)_           | **Async**: returns `{ "_id": "<reconId>", "state": "ACTIVE" }` immediately (HTTP 200). Add `&waitForCompletion=true` to block until done (returns the final state) — but prefer async + poll for a responsive UI. |
| Recon one record     | POST   | `/openidm/recon?_action=reconById&mapping=<name>&ids=<sourceId>` | _(none)_           | Reconciles a single source object. Add `&waitForCompletion=true` to get the **synchronous per-record error** in the response body — the only way to see _why_ a record failed (see "Diagnosing a failed recon").  |
| Poll a run           | GET    | `/openidm/recon/<reconId>`                                       | _(none)_           | Status of one run (see shape below).                                                                                                                                                                              |
| List recent runs     | GET    | `/openidm/recon`                                                 | _(none)_           | `{ "_id": "recon", "reconciliations": [ … ] }` (recent/active runs, newest last).                                                                                                                                 |
| Cancel a run         | POST   | `/openidm/recon/<reconId>?_action=cancel`                        | _(none)_           | Stops an `ACTIVE` run (not yet exercised here).                                                                                                                                                                   |
| Inspect links        | GET    | `/openidm/repo/link?_queryFilter=linkType+eq+"<mapping>"`        | _(none)_           | Source↔target link records (`firstId`→`secondId`). Deleting target rows alone leaves **stale links** → next recon mis-situates; delete links too when resetting test data.                                       |
| View sync queue      | GET    | `/openidm/sync/queue?_queryFilter=true`                          | _(none)_           | Pending **queued/async implicit-sync** events. Filter `mapping eq "<name>"` / `resourceId eq "<srcId>"` / `state eq "PENDING"`. See "Queued sync" below.                                                          |
| Read a queue item    | GET    | `/openidm/sync/queue/<id>`                                       | _(none)_           | Single event (shape below).                                                                                                                                                                                       |
| Delete a queue item  | DELETE | `/openidm/sync/queue/<id>`                                       | _(none)_           | **200**; removes the pending event (abandons that source→target sync). The only way to clear the queue — there is **no bulk action** (`POST …?_action=*` → **501**).                                              |

There is **no per-mapping endpoint** — like `managed`, the whole `sync` config
is one document. A single-mapping edit is read-modify-write of the array: GET →
mutate the one `mappings[i]` → PUT the whole doc → poll until applied. Reuse the
managed-config RMW/poll helper (see `docs/api/10-managed-objects.md`). No
`Accept-API-Version` header is required (IDM config endpoints, like
`config/endpoint/*` and `config/managed`).

## Object shapes

A mapping (abbreviated; verified 2026-06-18):

```jsonc
{
  "name": "managedTest_from_managedTest_to",
  "source": "managed/test_from",      // or "system/<connector>/<objectType>"
  "target": "managed/test_to",
  "displayName": "…", "icon": null, "consentRequired": false,
  "sourceQuery": { "_queryFilter": "…" }, "targetQuery": { … },
  "policies": [ { "situation": "ABSENT", "action": "CREATE" }, … ],

  // whole-mapping scripts — direct keys, each {type, globals, source|file}:
  "onCreate":   { "type": "text/javascript", "globals": {}, "source": "…" },
  "onUpdate":   …, "onDelete": …, "onLink": …, "onUnlink": …, "onSync": …,
  "validSource":…, "validTarget": …,
  "correlationScript": { "type": "text/javascript", "globals": {}, "source": "…" },
  "result":     { "type": "text/javascript", "globals": {}, "source": "…" },

  // per-attribute scripts — nested under each property:
  "properties": [
    { "target": "name", "source": "",
      "transform": { "type": "text/javascript", "globals": {}, "source": "…" } },
    { "target": "age",  "source": "age",
      "condition": { "type": "text/javascript", "globals": {}, "source": "…" },
      "transform": { … } }
  ]
}
```

**Script envelope.** Every slot is
`{ "type": "text/javascript", "globals": {…}, "source": "<js>" }`. The
alternative `"file": "ui/foo.js"` form (a platform-shipped file reference, e.g.
`correlationQuery[].file`) is also valid — **only sync the `source` form; pass
`file`-referenced scripts through untouched** (we don't own those files). Inline
`source` round-trips verbatim through PUT — IDM does **not** rewrite it to a
`file` reference (verified 2026-06-18).

**`correlationQuery` vs `correlationScript`.** A mapping has at most one of:
`correlationQuery` (a structured/`file`-backed query builder — _not_ synced) or
`correlationScript` (inline JS — synced). Treat them as mutually exclusive.

## Reconciliation (running a mapping)

`POST /openidm/recon?_action=recon&mapping=<name>` runs the mapping: it applies
the mapping's `policies` to the target, so it **creates / updates / deletes
target objects** and fires the behaviour scripts. **Treat it as a data-mutating
action** — confirm before running, and gate prod tenants behind the prod-write
confirm (or refuse). Verified live 2026-06-18.

The async form returns immediately; poll `GET /openidm/recon/<reconId>` until
`state` is terminal:

```jsonc
{
  "_id": "<reconId>",
  "mapping": "managedTest_from_managedTest_to",
  "state": "ACTIVE", // -> SUCCESS | FAILED | CANCELED (terminal)
  "stage": "COMPLETED_FAILED", // ACTIVE_* while running; COMPLETED_* / COMPLETED_FAILED at end
  "stageDescription": "reconciliation failed",
  "progress": {
    "source": { "existing": { "processed": 0, "total": "0" } },
    "target": {
      "existing": { "processed": 0, "total": "?" },
      "created": 0,
      "unchanged": 0,
      "updated": 0,
      "deleted": 0,
      "retried": 0,
    },
    "links": { "existing": { "processed": 0, "total": "?" }, "created": 0 },
  },
  "started": "2026-06-18T23:25:10.520Z",
  "ended": "2026-06-18T23:25:10.533Z", // absent while ACTIVE
  "duration": 13,
}
```

- `progress.*.total` is a **string** (`"0"`, `"?"` while unknown) — don't assume
  int.
- A run can `FAILED` purely because a mapping's scripts throw (the config is
  otherwise valid); surface `stageDescription` so the user sees why.

### Recon vs. implicit (live) sync — two independent trigger paths

**A managed-object write on the _source_ fires the mapping immediately**,
without any recon. Verified 2026-06-19: `POST /openidm/managed/test_from`
(create) made the corresponding `test_to` row appear before any recon ran;
deleting the source row deleted its target row and link. So
`onCreate`/`onUpdate`/`onDelete`/`onLink`/ `onUnlink` fire on every source CRUD
via **implicit sync**, and a _subsequent_ recon then sees those rows as
`CONFIRMED`/unchanged (it won't re-fire CREATE/ DELETE for changes implicit sync
already applied). Consequences:

- To observe a recon-driven CREATE/UPDATE/DELETE you must create drift that
  implicit sync did **not** already resolve (e.g. delete a _target_ row, or
  mutate a value that changes the mapped result), not just edit the source.
- `onSync` is a post-sync hook and did **not** fire under either recon or
  implicit update (still open below).
- A terminal recon's `durationSummary` lists exactly which slots ran, as
  `<slot>Script` keys (`validSourceScript`, `correlationScript`,
  `propertyMappingScript`, `onCreateScript`, `onUpdateScript`, `resultScript`,
  …). A slot absent from `durationSummary` did not execute in that run (e.g.
  `correlationScript` is skipped entirely when the target is empty — see
  Quirks).

### Diagnosing a failed recon

The summary endpoints tell you _what_ but not _why_:

- `situationSummary` — per-situation counts (`ABSENT`, `CONFIRMED`,
  `SOURCE_MISSING`…).
- `statusSummary` — `{ SUCCESS, FAILURE }` record counts.
- A record that errors **before situation assignment** (e.g. a throwing
  `correlationScript`) shows up as `statusSummary.FAILURE` with **no** situation
  counted — `situationSummary` totals less than the source count.

To get the actual exception:

1. **`audit/recon` / `audit/sync` are NOT queryable on this tenant** — they
   return `501 "Query not supported on stdout"` (audit is routed to stdout).
   Don't rely on them.
2. Run **`reconById` with `waitForCompletion=true`** on the offending source id:
   `POST /openidm/recon?_action=reconById&mapping=<name>&ids=<srcId>&waitForCompletion=true`.
   It returns the synchronous error, e.g.
   `{"code":409,"reason":"Conflict","message":"Unexpected Exception caught during SourceRecon:"}`.
   The message is often truncated, but the HTTP code + phase ("SourceRecon" =
   valid-source/correlation/situation; vs. target write) narrows it fast.
3. Bisect a suspect slot by temporarily overwriting its `source` with a trivial
   no-op (`true;` / `[];` / `""`), PUT, recon again. Restore afterward.

## Queued (asynchronous) implicit sync + the sync queue (verified 2026-07-29)

By default implicit sync is **synchronous** (source CRUD blocks on the target
write — see "Recon vs. implicit sync" above). Adding a per-mapping `queuedSync`
block makes implicit sync **asynchronous**: each source create/update/delete
enqueues one event into a persistent **sync queue** (`/openidm/sync/queue`), and
a background poller drains it.

```jsonc
// a mapping in config/sync, verified fields:
"queuedSync": {
  "enabled": true,
  "pageSize": 100,          // events a node claims per poll
  "pollingInterval": 1000,  // ms between polls
  "maxQueueSize": 1000,     // in-memory executor bound
  "maxRetries": 5,
  "retryDelay": 1000,       // ms between retries of a failing event
  "postRetryAction": "logged-ignore"  // after retries exhausted: log + drop the event
}
```

**Queue item shape** (verified):

```jsonc
{
  "_id": "c156ac15-…",
  "_rev": "…",
  "mapping": "managedTest_from_managedTest_to",
  "resourceId": "093e66dd-…", // source object _id
  "resourceCollection": "managed/test_from",
  "syncAction": "notifyCreate", // notifyCreate | notifyUpdate | notifyDelete
  "state": "PENDING",
  "nodeId": null, // null = unclaimed; a node id once claimed
  "createDate": "2026-07-29T03:31:36.175Z",
  "oldObject": {},
  "newObject": {},
}
```

**Processing model** (verified by watching `nodeId` during a drain): each
cluster node polls every `pollingInterval`, **claims** up to `pageSize`
_unclaimed_ (`nodeId == null`) events by stamping its `nodeId`, and processes
them through an in-memory executor bounded by `maxQueueSize` — situation calc +
target write + link write, retried up to `maxRetries` with `retryDelay`, then
`postRetryAction`.

**Throughput ceiling (sandbox, managed→managed, single tenant):** ~**55
events/sec** steady-state (1530 events drained in ~30 s at `pageSize=100`,
`pollingInterval=1000`); in-flight (`nodeId`-set) count tracked ≈ `pageSize` per
cycle. The poll ceiling `pageSize / pollingInterval` (100/s per node here) is
**not** usually the real limit.

### Querying the queue: counts, sort, projection (verified 2026-07-29)

A 7M-item queue can't be paged to answer "how deep is it?" — but it doesn't need
to be. All of the following cost ~60 ms regardless of depth:

- **Depth:** `?_queryFilter=true&_pageSize=1&_totalPagedResultsPolicy=EXACT` →
  `totalPagedResults`. **The response always reports
  `totalPagedResultsPolicy: "ESTIMATE"`** — `EXACT` is silently downgraded, so
  treat the number as a backend estimate (it was exact at 10–12 items; assume
  approximate at millions). Without the policy param, `totalPagedResults` is
  `-1`.
- **Counts are filter-aware for `eq` filters.** `mapping eq "<name>"`,
  `state eq "PENDING"`, `syncAction eq "notifyCreate"`,
  `resourceCollection eq "managed/x"` all return a count matching the filter (a
  bogus mapping name → `totalPagedResults: 0`). So a full breakdown by mapping ×
  `syncAction` × `state` is **one cheap GET per dimension value** — no paging.
- **Presence filters break the count.** `nodeId pr` and `!(nodeId pr)` return
  the correct `result` page but a **collection-wide** `totalPagedResults`
  (verified: `nodeId pr` → `resultCount: 0`, no cookie, yet
  `totalPagedResults: 12`). Never count claimed-vs-unclaimed this way; use
  `resultCount` on a bounded page, or sample.
- **`_countOnly=true` needs `Accept-API-Version: protocol=2.2`** (otherwise
  `400 countOnly is only supported with protocolVersion 2.2 or higher`), and
  even then it **still returns the full result page** — dangerous on a large
  queue. Prefer `_pageSize=1` + the policy param.
- **Sorting works and composes with the count policy:** `_sortKeys=createDate`
  (oldest first) / `_sortKeys=-createDate` (newest) → backlog age in one GET.
  `createDate` is an ISO-8601 nanosecond timestamp
  (`2026-07-29T04:26:19.258450542Z`).
- **Project fields.** `_fields=_id,createDate,mapping` is honored, and matters:
  each item embeds full `oldObject`/`newObject` payloads. Project when listing
  or sweeping.

### Why a real backlog processes far below the ceiling (diagnosing slow queues)

Field data from the affected env (7M-item backlog draining at ~1–2 events/sec)
plus a **full recon of the same mapping running at ~500 records/sec with a
~1-in-500 failure rate** narrows the cause sharply. Work through it in this
order:

1. **Is the target slow?** Compare against a recon of the same mapping. Recon
   does the same situation calc + target write + link write, so its rate is an
   upper bound on per-event work. Observed here: **~500/sec** — so the target
   write is _not_ the bottleneck, and "slow connector × low concurrency" is
   ruled out. (If recon is _also_ slow, the target/connector is the problem and
   nothing below applies.)
2. **Are retries burning workers?** A failing event occupies a worker for ≥
   `maxRetries × retryDelay` (≥5 s with `maxRetries: 5`, `retryDelay: 1000`)
   before `postRetryAction` disposes of it. Do the arithmetic before blaming it:
   at a **0.2 % failure rate**, 7M events ⇒ ~14 k failures ⇒ ≤ ~19 h of worker
   time even fully serialized — under 2 % of a 7-week drain, so **not** the
   throughput cause. It is still a **correctness** finding:
   `postRetryAction: "logged-ignore"` logs and **drops** each one, so ~14 k
   source changes never reach the target and nothing surfaces in the queue. Only
   a recon recovers them. **Caveat:** a recon failure rate does not bound the
   _queued_ failure rate — see 7.
3. **Queue depth slowing the claim query — MEASURED AND RULED OUT
   (2026-07-29).** The theory was that the per-poll "claim the next page ordered
   by `createDate`" query degrades with depth, which would be self-reinforcing
   and is the one cost recon doesn't share. **It does not:** on the affected
   env's 7M-item queue a 100-item claim-shaped query returns in **40 ms** —
   indistinguishable from the sandbox at 12 items. The queue collection is
   indexed well enough that depth is free. Do not spend time here; time the
   query once to confirm and move on.
4. **Stranded items — the head of the queue cannot be processed.** Verified:
   `queuedSync.enabled=false` **strands** pending events (they sit unchanged
   indefinitely, neither processed nor dropped). Items whose `mapping` no longer
   exists, was renamed, or has queued sync disabled are therefore permanently
   unprocessable, and a live trickle of new events flowing past them looks like
   a uniformly slow drain. **Test:** sum the per-mapping counts and compare to
   the total depth — any gap is items for mappings absent from `config/sync`;
   and check each mapping's `queuedSync.enabled`.
5. **Claimed-but-abandoned items.** A node claims by stamping `nodeId`. Nothing
   is verified to _un_-claim an item whose node has since gone away, so items
   claimed by a dead node may be invisible to every live node. **Test:** compare
   the oldest 1000 (`_sortKeys=createDate`) against the newest 1000
   (`-createDate`): head claimed + tail unclaimed is the signature. Also check
   distinct `nodeId` values — if effectively one node claims, you get one pool's
   worth of concurrency.
6. **Draining fine but refilling.** A depth-derived rate is a _net_ rate: 50/sec
   drained against 48/sec enqueued is indistinguishable from a stall. **Test:**
   sample depth _and_ the oldest/newest `createDate` twice, ~60 s apart. If the
   oldest advances quickly while depth barely moves, throughput is healthy and
   the real question is what produces the inflow (newest `createDate` ≈ now).
7. **Retries re-enqueueing instead of terminating.** A recon failure rate does
   **not** bound the queued failure rate: recon recomputes situations from
   _current_ state, while a queued event replays a stored
   `oldObject`/`newObject`, so stale-payload failures hit the queued path only.
   **Test:** repeated `resourceId` values within one page.

**Consequence:** queued sync is a latency-smoothing mechanism, not a bulk one —
even healthy it ran ~55/sec in the sandbox versus ~500/sec for recon, because
every event pays a queue-row insert/claim/delete round trip. For a
multi-million-item backlog, **clear the queue and reconcile** (below): recon
reconverges 7M records in ~4 h at 500/sec, versus weeks of queue drain, and it
also repairs the events that `logged-ignore` silently dropped.

Diagnosis checklist on the affected env, in the order that actually
discriminates (all read-only; `.ai/syncq-diag.py` automates it):

1. **Two depth samples ~60 s apart, plus oldest and newest `createDate`.** This
   is the decisive one — it separates a genuine stall from a healthy drain that
   is being refilled, which a net rate cannot distinguish (cause 6).
2. **Per-mapping counts summed against total depth**, plus each mapping's
   `queuedSync.enabled` — any gap is stranded/orphaned items (cause 4).
3. **Oldest 1000 vs newest 1000**: claimed/unclaimed split, distinct `nodeId`s,
   repeated `resourceId`s (causes 5 and 7).
4. **Recon rate for the same mapping** (cause 1) and one claim-shaped queue GET
   (cause 3 — expect it to be fast and to prove nothing).
5. **The sync logs** (`source=sync`) for the retry/failure reason.

### Clearing the sync queue

There is **no bulk/purge action** (`POST /openidm/sync/queue?_action=*` → **501
Not Implemented**). To clear it:

1. **Stop the flow first (recommended):** set the mapping's
   `queuedSync.enabled = false` and PUT `config/sync`. Verified: this stops new
   enqueues _and_ stops the poller — pending events then **sit unchanged**
   (neither processed nor dropped), so the sweep below races nothing. Disabling
   alone does **not** clear the queue.
2. **Sweep-delete:** loop
   `GET /openidm/sync/queue?_queryFilter=mapping eq "<name>"&_pageSize=100` →
   `DELETE /openidm/sync/queue/<id>` for each id, until empty (verified: DELETE
   → 200, count decrements; a 60-item and a 1530-item sweep both went to 0).
   Parallelize the deletes.
3. **Re-enable** `queuedSync` if async sync should resume.

**CAVEAT — clearing abandons those syncs.** Each deleted event is a source
change that never reached the target, so the target is left drifted. After
clearing, run a **full reconciliation** of the mapping (`_action=recon`) to
reconverge — recon recomputes situations from the _current_ source/target state
and re-applies the policies, which is the correct way to catch up after dropping
the queue.

## Mapping script wire-ids (proposed local layout)

One workspace file per inline script, addressed by a wire-id:

| Slot                | Wire-id                                     | Example                                         |
| ------------------- | ------------------------------------------- | ----------------------------------------------- |
| behaviour           | `sync/<mapping>.<event>`                    | `sync/managedTest_from_managedTest_to.onUpdate` |
| valid filter        | `sync/<mapping>.<validSource\|validTarget>` | `…​.validSource`                                |
| correlation         | `sync/<mapping>.correlationScript`          |                                                 |
| result              | `sync/<mapping>.result`                     |                                                 |
| attribute transform | `sync/<mapping>.transform.<targetAttr>`     | `…​.transform.name`                             |
| attribute condition | `sync/<mapping>.condition.<targetAttr>`     | `…​.condition.age`                              |

Mirrors `managed/<obj>.<hook>` from `managed_hooks`. A target attribute name can
contain `/` (nested); slugify for the filename, keep the JSON-pointer mapping in
the snapshot.

## Runtime binding surface (verified 2026-06-18)

Captured live via a recon probe: instrumented every slot of a
`managed/test_from` → `managed/test_to` mapping with `typeof`/`Object.keys`
capture into a throwaway managed object, drove recon through ABSENT→CREATE,
CONFIRMED→UPDATE, SOURCE_MISSING→DELETE/unlink, and uncorrelated→correlation.
Probe + `test_capture` torn down afterward; tenant restored to baseline.

**Globals present in EVERY slot:** `logger` (`debug|error|info|trace|warn`),
`openidm`
(`action|create|read|update|patch|delete|query|encrypt|decrypt|hash| isEncrypted|isHashed|matches|parseFilter`),
`identityServer`
(`getProperty|getInstallLocation|getProjectLocation|getWorkingLocation`),
`console` (`log`), `sync` (function), `context`, `linkQualifier` (string).
(`systemEnv`, `globals`, `request` are **undefined** in sync scripts — unlike AM
next-gen scripts. A mapping's configured `globals` are injected as top-level
vars, not as a `globals` object.)

**Per-slot extra bindings** (✓ = object of the named record type; see typing):

| Slot                | `source`                                                                    | `target`          | `oldTarget`        | `oldSource`   | `situation` | `mappingConfig` | other                   | returns                                                             |
| ------------------- | --------------------------------------------------------------------------- | ----------------- | ------------------ | ------------- | ----------- | --------------- | ----------------------- | ------------------------------------------------------------------- |
| `validSource`       | source ✓                                                                    | —                 | —                  | —             | —           | —               |                         | boolean                                                             |
| `validTarget`       | —                                                                           | target ✓          | —                  | —             | —           | —               |                         | boolean                                                             |
| `correlationScript` | source ✓                                                                    | —                 | —                  | —             | —           | —               |                         | record array / id list (NOT a `{_queryFilter}` object — see Quirks) |
| `onCreate`          | source ✓                                                                    | target ✓          | —                  | `null`        | string      | ✓               |                         | —                                                                   |
| `onUpdate`          | source ✓                                                                    | target ✓          | oldTarget ✓        | `null`        | string      | ✓               |                         | —                                                                   |
| `onDelete`          | `null`                                                                      | target ✓          | —                  | oldSource ✓   | string      | ✓               |                         | —                                                                   |
| `onLink`            | source ✓                                                                    | target ✓          | —                  | `null`        | string      | ✓               | `context.pendingAction` | —                                                                   |
| `onUnlink`          | source/`null`                                                               | target ✓          | —                  | source/`null` | string      | ✓               | `context.pendingAction` | —                                                                   |
| `result`            | **recon summary**                                                           | **recon summary** | —                  | —             | —           | ✓               |                         | —                                                                   |
| `transform` (prop)  | attr value, **or whole source object when the property's `source` is `""`** | —                 | —                  | —             | —           | —               |                         | mapped value                                                        |
| `condition` (prop)  | — (use `object`)                                                            | target ✓          | oldTarget ✓/`null` | —             | —           | —               | `object` = source ✓     | boolean                                                             |

Notes:

- **`result.source` / `result.target` are recon-statistics objects**, not
  records: keys are the situation names (`ABSENT|CONFIRMED|…`) plus
  `name|processed|entries|startTime|endTime|duration`. Do **not** type them as
  the source/target record.
- **`transform`**: when the property has a `source` attribute, `source` is that
  attribute's _value_ (e.g. `number`); when the property's `source` is `""`,
  `source` is the _whole source object_. The current generated binding types it
  as the source object `S` and adds a doc comment: attribute-mapped transforms
  receive the raw attribute value and should cast as needed. Per-attribute
  precision is a future refinement.
- **`condition`** exposes the source object as **`object`** (not `source`), plus
  `target`/`oldTarget`.
- `onSync` did **not** fire under recon or implicit update — it's a post-sync
  result hook. **Not yet runtime-probed** (open question below). Until verified,
  type it conservatively as the union of the behaviour-script bindings.

## Typing (`source`/`target` → managed interfaces)

Resolve the mapping's `source`/`target` strings:

- `managed/<obj>` → the generated managed interface (e.g. `managed/test_from` →
  `TestFrom` from `managed_types.rs`; `managed/alpha_user` → `AlphaUser`).
- `system/<connector>/<objectType>` → no schema available → loose
  `{ [k: string]: any }` connector-object type.

Generate per-mapping, per-category `.d.ts` files so slots with conflicting
globals never share one TypeScript project:

| Category  | Workspace folder                | Binding file                              |
| --------- | ------------------------------- | ----------------------------------------- |
| behaviour | `idm/sync/<mapping>/behaviour/` | `idm/types/sync/<mapping>.behaviour.d.ts` |
| result    | `idm/sync/<mapping>/result/`    | `idm/types/sync/<mapping>.result.d.ts`    |
| transform | `idm/sync/<mapping>/transform/` | `idm/types/sync/<mapping>.transform.d.ts` |
| condition | `idm/sync/<mapping>/condition/` | `idm/types/sync/<mapping>.condition.d.ts` |

Each category folder gets its own leaf `tsconfig.json`, composed with Rhino, IDM
common bindings, generated managed interfaces, `idm/types/sync/_shared.d.ts`
(`ReconSummary`), and exactly one mapping/category binding file.

## Examples

```bash
# List mappings
curl -s -H "Authorization: Bearer ${TOKEN}" \
  "${TENANT_BASE_URL}/openidm/config/sync" | jq -r '.mappings[].name'

# Edit one mapping's onUpdate: GET whole doc, mutate, PUT whole doc, poll.

# Run a recon synchronously and read the outcome
curl -s -X POST -H "Authorization: Bearer ${TOKEN}" \
  "${TENANT_BASE_URL}/openidm/recon?_action=recon&mapping=${MAP}&waitForCompletion=true" \
  | jq '{state, stageDescription}'

# Diagnose a per-record failure (returns the synchronous exception)
curl -s -X POST -H "Authorization: Bearer ${TOKEN}" \
  "${TENANT_BASE_URL}/openidm/recon?_action=reconById&mapping=${MAP}&ids=${SRC_ID}&waitForCompletion=true"

# CORRECT correlationScript body (return candidate records, NOT a {_queryFilter} object):
#   var n = ((source.firstName||"")+" "+(source.lastName||"")).trim().toLowerCase();
#   openidm.query("managed/test_to", {"_queryFilter": "name eq \"" + n + "\""}).result;
```

## Quirks

- Whole-document PUT, **no `_rev`** → use content-snapshot conflict detection
  (CLAUDE.md §5), not `If-Match`. Same as scripts/ESVs/managed.
- Write applies with lag; poll-verify after PUT (reuse managed `APPLY_RETRIES`).
- A no-match `correlationScript` that yields ABSENT can drive a CREATE that
  collides with an existing target → recon returns `409`; this is a data
  condition, not a config error.
- **`policies` actions must be executable for recon to run (verified
  2026-06-19):** a mapping whose policies all use action `"ASYNC"` fails recon
  at setup (`COMPLETED_FAILED`, 0 processed) — ASYNC needs workflow-based recon
  that the AIC sandbox doesn't run. Use executable actions
  (`CREATE`/`UPDATE`/`DELETE`/ `IGNORE`/`REPORT`) for a recon that actually
  processes records.
- **`correlationScript` return form (verified 2026-06-19):** returning the
  _query-definition object_ `({_queryFilter: "name eq \"…\""})` throws during
  recon — `409 Conflict`, `"Unexpected Exception caught during SourceRecon:"` —
  even though the same filter runs fine over REST. Return the **candidate
  records** instead:
  `openidm.query("managed/<target>", {_queryFilter: …}).result` (an array), or
  an array of `_id` strings. This only surfaces once the target has rows: with
  an empty target IDM short-circuits to ABSENT _without running the correlation
  script at all_, so a broken `{_queryFilter}` form passes the first
  (create-everything) recon and only fails on the second. Prefer the
  `openidm.query(...).result` form in templates/examples.
- `source`/`target` record shapes include `_id`/`_rev` once persisted.

## Verified against

- Tenant `tenant-example`, 2026-06-18; correlation return-form quirk
  added 2026-06-19.
- Exercised: GET/PUT `/openidm/config/sync` (round-trip of inline `source`);
  whole-doc RMW; recon-driven runtime binding capture for `validSource`,
  `validTarget`, `correlationScript`, `onCreate`, `onUpdate`, `onDelete`,
  `onLink`, `onUnlink`, `result`, property `transform`, property `condition`.
- 2026-06-19: `managedTest_from_managedTest_to` populated to **all 10
  whole-mapping slots + `transform`+`condition` on both properties** (14 inline
  scripts); full recon SUCCESS through ABSENT→CREATE, CONFIRMED→UPDATE, and
  source-delete→target-delete (the last via implicit managed-object sync, not
  the recon engine); all 14 pull to the workspace and type-check clean
  (`source: TestFrom`, `target: TestTo`).
- 2026-07-29 (queued sync + sync queue): enabled `queuedSync` on
  `managedTest_from_managedTest_to`; drove 120/60/30/1530 implicit-sync events
  into `/openidm/sync/queue`. Verified: queue item shape; `_queryFilter` +
  `mapping eq`/EXACT-vs-ESTIMATE counts; single `DELETE …/queue/<id>` (200) and
  a filtered bulk sweep to 0; `POST …/queue?_action=*` → **501** (no bulk
  action); `queuedSync.enabled=false` **strands** pending items (30 held
  unchanged over 12 s); `nodeId` claim/poll model; drain ceiling ~**55
  events/sec** managed→managed. Mapping restored (queuedSync removed) and all
  throwaway records deleted afterward.
- 2026-07-29 (queue query surface, for the diagnostic tooling): with 10–12 held
  events, verified `_totalPagedResultsPolicy=EXACT` returns a total but is
  always downgraded to `ESTIMATE`; counts are filter-aware for `eq`
  (`mapping`/`state`/`syncAction`/`resourceCollection`; bogus mapping → 0) but
  **collection-wide for `pr`** (`nodeId pr` → 0 results, total 12);
  `_countOnly=true` → 400 without `Accept-API-Version: protocol=2.2` and still
  returns full results with it; `_sortKeys=±createDate` works and composes with
  the count policy; `_fields` projection honored. All probes ~60 ms. Mapping
  restored and all throwaway records deleted afterward.
- 2026-07-29 (field observation, affected env — reported, not run from here): on
  a **7M-item** queue a 100-item claim-shaped query returns in **40 ms**, and a
  full recon of the same mapping runs at **~500 records/sec** with a
  **~1-in-500** failure rate. Queue depth therefore does **not** slow the claim
  query; see causes 3–7 above for what remains.

## Source citations

- Slot list cross-checked against ForgeRock IDM "Synchronization reference" /
  `mapping` object; binding names verified by live probe (above), not
  transcribed.

## Open questions

- **`onSync` bindings** — not yet runtime-probed (didn't fire under recon or
  implicit update). Needs a dedicated trigger (e.g. `notifyChange`/targeted
  sync).
- **`reconById` / single-record sync** binding deltas vs full recon — not
  probed.
- **Multiple `linkQualifier`s** — only `default` exercised; per-qualifier
  scripts not probed.
