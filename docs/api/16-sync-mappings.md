# IDM sync mappings (`config/sync`) + mapping script bindings

Implemented in: `src/scripts/`

## Purpose

IDM **sync mappings** reconcile a source resource (a connector
`system/<connector>/<objectType>` or a managed object `managed/<obj>`) onto a
target managed/connector object. Each mapping can embed JavaScript in several
slots: whole-mapping **behaviour scripts** (`onCreate`, `onUpdate`, …), a
**correlation** script, **valid-source/target** filters, a recon **result**
script, and per-attribute **transform**/**condition** scripts. `pingone-aic-manager` syncs
those embedded scripts to the local workspace (one file per slot) with full
TypeScript typing of the runtime bindings.

## Authentication

Service-account bearer (the `fr:idm:*` scope reaches `/openidm/*`). Same token
the rest of the IDM features use. No log-API key needed.

## Endpoints

| Op | Method | Path | Accept-API-Version | Notes |
|----|--------|------|--------------------|-------|
| Read all mappings | GET | `/openidm/config/sync` | *(none)* | Single document: `{ _id:"sync", mappings:[…] }`. **No `_rev`.** |
| Write all mappings | PUT | `/openidm/config/sync` | *(none)* | **Whole-document replace** (RMW). Applies with lag — poll-verify after write, exactly like `/openidm/config/managed`. |
| Start reconciliation | POST | `/openidm/recon?_action=recon&mapping=<name>` | *(none)* | **Async**: returns `{ "_id": "<reconId>", "state": "ACTIVE" }` immediately (HTTP 200). Add `&waitForCompletion=true` to block until done (returns the final state) — but prefer async + poll for a responsive UI. |
| Recon one record | POST | `/openidm/recon?_action=reconById&mapping=<name>&ids=<sourceId>` | *(none)* | Reconciles a single source object. Add `&waitForCompletion=true` to get the **synchronous per-record error** in the response body — the only way to see *why* a record failed (see "Diagnosing a failed recon"). |
| Poll a run | GET | `/openidm/recon/<reconId>` | *(none)* | Status of one run (see shape below). |
| List recent runs | GET | `/openidm/recon` | *(none)* | `{ "_id": "recon", "reconciliations": [ … ] }` (recent/active runs, newest last). |
| Cancel a run | POST | `/openidm/recon/<reconId>?_action=cancel` | *(none)* | Stops an `ACTIVE` run (not yet exercised here). |
| Inspect links | GET | `/openidm/repo/link?_queryFilter=linkType+eq+"<mapping>"` | *(none)* | Source↔target link records (`firstId`→`secondId`). Deleting target rows alone leaves **stale links** → next recon mis-situates; delete links too when resetting test data. |

There is **no per-mapping endpoint** — like `managed`, the whole `sync` config is
one document. A single-mapping edit is read-modify-write of the array:
GET → mutate the one `mappings[i]` → PUT the whole doc → poll until applied.
Reuse the managed-config RMW/poll helper (see `docs/api/10-managed-objects.md`).
No `Accept-API-Version` header is required (IDM config endpoints, like
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

**Script envelope.** Every slot is `{ "type": "text/javascript", "globals": {…},
"source": "<js>" }`. The alternative `"file": "ui/foo.js"` form (a
platform-shipped file reference, e.g. `correlationQuery[].file`) is also valid —
**only sync the `source` form; pass `file`-referenced scripts through untouched**
(we don't own those files). Inline `source` round-trips verbatim through PUT —
IDM does **not** rewrite it to a `file` reference (verified 2026-06-18).

**`correlationQuery` vs `correlationScript`.** A mapping has at most one of:
`correlationQuery` (a structured/`file`-backed query builder — *not* synced) or
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
  "state": "ACTIVE",            // -> SUCCESS | FAILED | CANCELED (terminal)
  "stage": "COMPLETED_FAILED",  // ACTIVE_* while running; COMPLETED_* / COMPLETED_FAILED at end
  "stageDescription": "reconciliation failed",
  "progress": {
    "source": { "existing": { "processed": 0, "total": "0" } },
    "target": { "existing": { "processed": 0, "total": "?" },
                "created": 0, "unchanged": 0, "updated": 0, "deleted": 0, "retried": 0 },
    "links":  { "existing": { "processed": 0, "total": "?" }, "created": 0 }
  },
  "started": "2026-06-18T23:25:10.520Z",
  "ended":   "2026-06-18T23:25:10.533Z",   // absent while ACTIVE
  "duration": 13
}
```

- `progress.*.total` is a **string** (`"0"`, `"?"` while unknown) — don't assume int.
- A run can `FAILED` purely because a mapping's scripts throw (the config is
  otherwise valid); surface `stageDescription` so the user sees why.

### Recon vs. implicit (live) sync — two independent trigger paths

**A managed-object write on the *source* fires the mapping immediately**, without
any recon. Verified 2026-06-19: `POST /openidm/managed/test_from` (create) made
the corresponding `test_to` row appear before any recon ran; deleting the source
row deleted its target row and link. So `onCreate`/`onUpdate`/`onDelete`/`onLink`/
`onUnlink` fire on every source CRUD via **implicit sync**, and a *subsequent*
recon then sees those rows as `CONFIRMED`/unchanged (it won't re-fire CREATE/
DELETE for changes implicit sync already applied). Consequences:

- To observe a recon-driven CREATE/UPDATE/DELETE you must create drift that
  implicit sync did **not** already resolve (e.g. delete a *target* row, or mutate
  a value that changes the mapped result), not just edit the source.
- `onSync` is a post-sync hook and did **not** fire under either recon or implicit
  update (still open below).
- A terminal recon's `durationSummary` lists exactly which slots ran, as
  `<slot>Script` keys (`validSourceScript`, `correlationScript`,
  `propertyMappingScript`, `onCreateScript`, `onUpdateScript`, `resultScript`, …).
  A slot absent from `durationSummary` did not execute in that run (e.g.
  `correlationScript` is skipped entirely when the target is empty — see Quirks).

### Diagnosing a failed recon

The summary endpoints tell you *what* but not *why*:

- `situationSummary` — per-situation counts (`ABSENT`, `CONFIRMED`, `SOURCE_MISSING`…).
- `statusSummary` — `{ SUCCESS, FAILURE }` record counts.
- A record that errors **before situation assignment** (e.g. a throwing
  `correlationScript`) shows up as `statusSummary.FAILURE` with **no** situation
  counted — `situationSummary` totals less than the source count.

To get the actual exception:

1. **`audit/recon` / `audit/sync` are NOT queryable on this tenant** — they return
   `501 "Query not supported on stdout"` (audit is routed to stdout). Don't rely on
   them.
2. Run **`reconById` with `waitForCompletion=true`** on the offending source id:
   `POST /openidm/recon?_action=reconById&mapping=<name>&ids=<srcId>&waitForCompletion=true`.
   It returns the synchronous error, e.g.
   `{"code":409,"reason":"Conflict","message":"Unexpected Exception caught during SourceRecon:"}`.
   The message is often truncated, but the HTTP code + phase ("SourceRecon" =
   valid-source/correlation/situation; vs. target write) narrows it fast.
3. Bisect a suspect slot by temporarily overwriting its `source` with a trivial
   no-op (`true;` / `[];` / `""`), PUT, recon again. Restore afterward.

## Mapping script wire-ids (proposed local layout)

One workspace file per inline script, addressed by a wire-id:

| Slot | Wire-id | Example |
|------|---------|---------|
| behaviour | `sync/<mapping>.<event>` | `sync/managedTest_from_managedTest_to.onUpdate` |
| valid filter | `sync/<mapping>.<validSource\|validTarget>` | `…​.validSource` |
| correlation | `sync/<mapping>.correlationScript` | |
| result | `sync/<mapping>.result` | |
| attribute transform | `sync/<mapping>.transform.<targetAttr>` | `…​.transform.name` |
| attribute condition | `sync/<mapping>.condition.<targetAttr>` | `…​.condition.age` |

Mirrors `managed/<obj>.<hook>` from `managed_hooks`. A target attribute name can
contain `/` (nested); slugify for the filename, keep the JSON-pointer mapping in
the snapshot.

## Runtime binding surface (verified 2026-06-18)

Captured live via a recon probe: instrumented every slot of a `managed/test_from`
→ `managed/test_to` mapping with `typeof`/`Object.keys` capture into a throwaway
managed object, drove recon through ABSENT→CREATE, CONFIRMED→UPDATE,
SOURCE_MISSING→DELETE/unlink, and uncorrelated→correlation. Probe + `test_capture`
torn down afterward; tenant restored to baseline.

**Globals present in EVERY slot:** `logger` (`debug|error|info|trace|warn`),
`openidm` (`action|create|read|update|patch|delete|query|encrypt|decrypt|hash|
isEncrypted|isHashed|matches|parseFilter`), `identityServer`
(`getProperty|getInstallLocation|getProjectLocation|getWorkingLocation`),
`console` (`log`), `sync` (function), `context`, `linkQualifier` (string).
(`systemEnv`, `globals`, `request` are **undefined** in sync scripts — unlike AM
next-gen scripts. A mapping's configured `globals` are injected as top-level vars,
not as a `globals` object.)

**Per-slot extra bindings** (✓ = object of the named record type; see typing):

| Slot | `source` | `target` | `oldTarget` | `oldSource` | `situation` | `mappingConfig` | other | returns |
|------|----------|----------|-------------|-------------|-------------|-----------------|-------|---------|
| `validSource` | source ✓ | — | — | — | — | — | | boolean |
| `validTarget` | — | target ✓ | — | — | — | — | | boolean |
| `correlationScript` | source ✓ | — | — | — | — | — | | record array / id list (NOT a `{_queryFilter}` object — see Quirks) |
| `onCreate` | source ✓ | target ✓ | — | `null` | string | ✓ | | — |
| `onUpdate` | source ✓ | target ✓ | oldTarget ✓ | `null` | string | ✓ | | — |
| `onDelete` | `null` | target ✓ | — | oldSource ✓ | string | ✓ | | — |
| `onLink` | source ✓ | target ✓ | — | `null` | string | ✓ | `context.pendingAction` | — |
| `onUnlink` | source/`null` | target ✓ | — | source/`null` | string | ✓ | `context.pendingAction` | — |
| `result` | **recon summary** | **recon summary** | — | — | — | ✓ | | — |
| `transform` (prop) | attr value, **or whole source object when the property's `source` is `""`** | — | — | — | — | — | | mapped value |
| `condition` (prop) | — (use `object`) | target ✓ | oldTarget ✓/`null` | — | — | — | `object` = source ✓ | boolean |

Notes:
- **`result.source` / `result.target` are recon-statistics objects**, not records:
  keys are the situation names (`ABSENT|CONFIRMED|…`) plus
  `name|processed|entries|startTime|endTime|duration`. Do **not** type them as the
  source/target record.
- **`transform`**: when the property has a `source` attribute, `source` is that
  attribute's *value* (e.g. `number`); when the property's `source` is `""`,
  `source` is the *whole source object*. The current generated binding types it
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

| Category | Workspace folder | Binding file |
|----------|------------------|--------------|
| behaviour | `idm/sync/<mapping>/behaviour/` | `idm/types/sync/<mapping>.behaviour.d.ts` |
| result | `idm/sync/<mapping>/result/` | `idm/types/sync/<mapping>.result.d.ts` |
| transform | `idm/sync/<mapping>/transform/` | `idm/types/sync/<mapping>.transform.d.ts` |
| condition | `idm/sync/<mapping>/condition/` | `idm/types/sync/<mapping>.condition.d.ts` |

Each category folder gets its own leaf `tsconfig.json`, composed with Rhino,
IDM common bindings, generated managed interfaces, `idm/types/sync/_shared.d.ts`
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
- **`policies` actions must be executable for recon to run (verified 2026-06-19):**
  a mapping whose policies all use action `"ASYNC"` fails recon at setup
  (`COMPLETED_FAILED`, 0 processed) — ASYNC needs workflow-based recon that the
  AIC sandbox doesn't run. Use executable actions (`CREATE`/`UPDATE`/`DELETE`/
  `IGNORE`/`REPORT`) for a recon that actually processes records.
- **`correlationScript` return form (verified 2026-06-19):** returning the
  *query-definition object* `({_queryFilter: "name eq \"…\""})` throws during
  recon — `409 Conflict`, `"Unexpected Exception caught during SourceRecon:"` —
  even though the same filter runs fine over REST. Return the **candidate
  records** instead: `openidm.query("managed/<target>", {_queryFilter: …}).result`
  (an array), or an array of `_id` strings. This only surfaces once the target
  has rows: with an empty target IDM short-circuits to ABSENT *without running
  the correlation script at all*, so a broken `{_queryFilter}` form passes the
  first (create-everything) recon and only fails on the second. Prefer the
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
  source-delete→target-delete (the last via implicit managed-object sync, not the
  recon engine); all 14 pull to the workspace and type-check clean
  (`source: TestFrom`, `target: TestTo`).

## Source citations

- Slot list cross-checked against ForgeRock IDM "Synchronization reference" /
  `mapping` object; binding names verified by live probe (above), not transcribed.

## Open questions

- **`onSync` bindings** — not yet runtime-probed (didn't fire under recon or
  implicit update). Needs a dedicated trigger (e.g. `notifyChange`/targeted sync).
- **`reconById` / single-record sync** binding deltas vs full recon — not probed.
- **Multiple `linkQualifier`s** — only `default` exercised; per-qualifier scripts
  not probed.
