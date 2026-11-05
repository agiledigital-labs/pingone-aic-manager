# Managed-aggregate CAS POC — results

Raw measurements, sandbox tenant, 2026-11-05. Each row is one run; the driver is
`experiments.mjs` and the raw JSON lands in `.state/results/` (gitignored). The
curated version of these findings is in `docs/api/10-managed-objects.md`
("Concurrent `PATCH` and hooks") and `docs/managed-object-aggregates.md`.

Storage for the parent and sink: generic (`aic managed object create` adds a
per-type `genericMapping` with no property mappings). "trace" means `onUpdate`
also created one sink record per run, which is how runs are counted.

## E2 — stale `If-Match`

First `PATCH` with the read `_rev`: `200`. Second with the same `_rev`: `412`
"expected version … does not match the current version …". Only the first entry
is stored.

## E1 / E6 / E7 / E17 — concurrent revision-less `PATCH add /inbox/-`

Fold mode, trace on, `postUpdate` creating one sink per run.

| N   | HTTP     | p50 / p90 / max ms | entries | all present | `derived` consistent | `onUpdate` runs | `postUpdate` runs | requests retried | max runs / request | retry saw same base |
| --- | -------- | ------------------ | ------- | ----------- | -------------------- | --------------- | ----------------- | ---------------- | ------------------ | ------------------- |
| 2   | 2 × 200  | 352 / 352 / 352    | 2       | yes         | yes                  | 3               | 2                 | 1                | 2                  | 0                   |
| 2   | 2 × 200  | 238 / 238 / 238    | 2       | yes         | yes                  | 3               | 2                 | 1                | 2                  | 0                   |
| 2   | 2 × 200  | 267 / 267 / 267    | 2       | yes         | yes                  | 3               | 2                 | 1                | 2                  | 0                   |
| 10  | 10 × 200 | 628 / 959 / 959    | 10      | yes         | yes                  | 36              | 10                | 7                | 8                  | 0                   |
| 10  | 10 × 200 | 1060 / 1315 / 1315 | 10      | yes         | yes                  | 54              | 10                | 9                | 10                 | 0                   |
| 10  | 10 × 200 | 635 / 801 / 801    | 10      | yes         | yes                  | 55              | 10                | 9                | 10                 | 0                   |
| 50  | 50 × 200 | 6225 / 8754 / 8888 | 50      | yes         | yes                  | 836             | 50                | 47               | 40                 | 0                   |
| 50  | 50 × 200 | 3281 / 5090 / 5260 | 50      | yes         | yes                  | 667             | 50                | 49               | 34                 | 0                   |
| 50  | 50 × 200 | 4309 / 6137 / 6325 | 50      | yes         | yes                  | 693             | 50                | 48               | 33                 | 0                   |

- A revision-less `PATCH` is a compare-and-swap with automatic retry: no lost
  append, no 412, no 5xx, `derived` always equal to a recomputation.
- `onUpdate` re-runs on every retry. Distinct `postUpdate` `_rev`s equal the
  commits, so `postUpdate` ran once per commit.
- "retry saw same base" counts a re-run whose `oldObject._rev` equalled the
  previous run's for the same request: zero everywhere, so every retry got a
  fresh `oldObject`.
- The sink write inside `onUpdate` (E6) is duplicated by every retry: 836 sink
  records for 50 commits. Side effects in `onUpdate` are unsafe.
- Latency here includes one sink create per `onUpdate` run, which lengthens the
  window each attempt holds; E14 measures without it.

## E3 — concurrent revision-less `PUT` built from a prior read

| N   | HTTP     | p50 / max ms | entries kept | lost |
| --- | -------- | ------------ | ------------ | ---- |
| 5   | 5 × 200  | 90 / 192     | 1            | 4    |
| 5   | 5 × 200  | 58 / 64      | 1            | 4    |
| 5   | 5 × 200  | 64 / 67      | 1            | 4    |
| 20  | 20 × 200 | 135 / 246    | 2            | 18   |
| 20  | 20 × 200 | 178 / 221    | 2            | 18   |
| 20  | 20 × 200 | 179 / 206    | 1            | 19   |

Every call succeeds and almost every update is lost: last writer wins. `derived`
stayed consistent with what was stored, because `onUpdate` recomputes from the
stored entries — it cannot recover what the caller dropped.

## E4 — concurrent `PATCH replace /entries` with a client-computed array

| N   | HTTP     | p50 / max ms | entries kept | lost |
| --- | -------- | ------------ | ------------ | ---- |
| 5   | 5 × 200  | 65 / 86      | 1            | 4    |
| 5   | 5 × 200  | 93 / 127     | 1            | 4    |
| 5   | 5 × 200  | 65 / 93      | 1            | 4    |
| 20  | 20 × 200 | 432 / 730    | 1            | 19   |
| 20  | 20 × 200 | 387 / 590    | 1            | 19   |
| 20  | 20 × 200 | 336 / 501    | 1            | 19   |

The `PATCH` is still a CAS, but the value was computed from a read outside it,
so a retry re-applies the same stale array. Same loss as `PUT`.

## E5 — server-side operations

| Operation                      | N   | HTTP     | Result                                                             |
| ------------------------------ | --- | -------- | ------------------------------------------------------------------ |
| `increment /counter` by 1      | 10  | 10 × 200 | counter 10 (exact)                                                 |
| `increment /counter` by 1      | 50  | 50 × 200 | counter 50 (exact); p50 1040 ms                                    |
| `add /items/0`, `items` absent | 10  | 10 × 403 | "Failed policy validation": the index made an object, not an array |

Array semantics, single requests:

| Operation                                                   | Result                                                    |
| ----------------------------------------------------------- | --------------------------------------------------------- |
| `add /items/-` on an absent array                           | `200`, array created                                      |
| `add /items/0` on an existing array                         | `400` "Position-based operation is not allowed: /items/0" |
| `remove /items/0`                                           | `400`, same message                                       |
| `remove /items` with `value`                                | `200`, removes **every** element equal to the value       |
| `replace /items/0/w` (field under an index)                 | `200`                                                     |
| `replace /items/5/w` (index out of range)                   | `500`, empty body                                         |
| `increment /data/n`, `data.n` absent                        | `200`, creates it as the increment                        |
| one `PATCH` with `add /-`, `replace`, and a new nested path | `200`, all applied                                        |

So `/-` append and `increment` are the only position-free primitives. Anything
that edits an existing array element by index is position-based and is either
refused or races: index `i` read by the client may name a different element by
the time the retry runs.

## E8 — `postUpdate` throws

`throw { code: 500, message: "POC_POST_THROW" }` in `postUpdate`: the caller
gets `500` "POC_POST_THROW", and the change **is stored** (entry present,
`derived` updated). A caller that treats that 500 as "not written" and retries
appends twice. Never throw from `postUpdate` to signal failure; and a writer
that may retry must make its inbox entry idempotent (an id the fold
de-duplicates on).

## E9 — a throw in `onUpdate` as an invariant guard

Sequential, threshold 3: after three entries the outcome is `closed`; a fourth
append gets `409` "POC_TERMINAL: outcome is closed" (the `code` thrown is the
status returned) and nothing is stored.

Concurrent, threshold 5, N = 20:

| HTTP              | p50 / max ms | entries | outcome | `derived` consistent |
| ----------------- | ------------ | ------- | ------- | -------------------- |
| 5 × 200, 15 × 409 | 350 / 365    | 5       | closed  | yes                  |
| 5 × 200, 15 × 409 | 284 / 301    | 5       | closed  | yes                  |
| 5 × 200, 15 × 409 | 268 / 285    | 5       | closed  | yes                  |

Exactly `threshold` writers win: the guard runs inside the CAS, on the state the
write commits against, so "first five" is enforced without locks.

## E10 — concurrent link writes while `onUpdate` derives from the relationship

Each writer creates a child (`value` = 1..N) and links it with
`PATCH add /<rel>/-` on the parent, `_refProperties.status = "active"`.
`onUpdate` (children mode) queries `<parent>/<rel>` and stores count and sum. No
notification settings (the `aic`/console defaults).

| pair    | N   | HTTP     | links | `derived.count` | `derived.sum` / want | `onUpdate` runs | requests retried |
| ------- | --- | -------- | ----- | --------------- | -------------------- | --------------- | ---------------- |
| mapped  | 5   | 5 × 200  | 5     | 0               | 0 / 15               | 15              | 4                |
| mapped  | 5   | 5 × 200  | 5     | 4               | 11 / 15              | 23              | 4                |
| mapped  | 5   | 5 × 200  | 5     | 4               | 10 / 15              | 25              | 4                |
| mapped  | 20  | 20 × 200 | 20    | 19              | 194 / 210            | 245             | 19               |
| mapped  | 20  | 20 × 200 | 20    | 19              | 207 / 210            | 248             | 19               |
| mapped  | 20  | 20 × 200 | 20    | 19              | 193 / 210            | 269             | 19               |
| generic | 5   | 5 × 200  | 5     | 4               | 10 / 15              | 15              | 4                |
| generic | 5   | 5 × 200  | 5     | 4               | 14 / 15              | 15              | 4                |
| generic | 5   | 5 × 200  | 5     | 4               | 11 / 15              | 15              | 4                |
| generic | 20  | 20 × 200 | 20    | 19              | 203 / 210            | 209             | 19               |
| generic | 20  | 20 × 200 | 20    | 19              | 208 / 210            | 203             | 19               |
| generic | 20  | 20 × 200 | 20    | 19              | 194 / 210            | 199             | 19               |

- Every link landed, in both modes.
- `derived` is **always** wrong: `onUpdate` runs before the request's own link
  is persisted, so the last writer's link is never counted, and which N−1 are
  counted depends on the interleaving (the sums differ run to run).
- The first mapped run counted nothing; it was the first write through a pair
  `aic` had just mapped, and `aic` warns that a fresh pair needs a few seconds
  before it is live. Not investigated further.
- The retries here come from `derived` itself: a plain field the hook changes on
  every commit, which moves `_rev`. That is not evidence that link writes are
  inside the CAS — E11 shows they are not.

## E11 — an edge `_refProperties` write

`PATCH <parent>/<rel>/<edgeId>` replacing `/_refProperties/status`:

| pair    | HTTP | edge changed | parent `_rev` moved | parent `onUpdate` ran | `derived.active` |
| ------- | ---- | ------------ | ------------------- | --------------------- | ---------------- |
| mapped  | 200  | yes          | **yes**             | no                    | unchanged        |
| generic | 200  | yes          | no                  | no                    | unchanged        |

An edge write does not run the parent's hooks. On a mapped pair it does move the
parent's `_rev` (the link lives on the parent's entry), so a parent `PATCH` with
`If-Match` would see a conflict from it; on a generic pair nothing does.

## E12 — a child changes after the parent derived from it

`PATCH` the linked child's `value` 5 → 50: parent `onUpdate` did not run, its
`_rev` did not move and `derived.sum` stayed 5, in both modes. Anything derived
from another record is stale from the moment that record changes.

## E10–E12 again, with relationship notification flags

`NOTIFY=1`: `notifySelf: true` on the parent's relationship items,
`notify: true` on the child's reverse property. `NOTIFY=2` swaps the ends, in
case the flags were read the wrong way round. The parent has `trace` and
`postMode: "sink"`, so both hooks' runs are counted. Each row is a single write,
waited on for 4–8 s.

| Write                                                   | default flags | `NOTIFY=1`                | `NOTIFY=2`            |
| ------------------------------------------------------- | ------------- | ------------------------- | --------------------- |
| child created with its reverse link set (child side)    | no hook       | no hook                   | no hook               |
| child `PATCH replace` of its reverse link               | no hook       | no hook                   | —                     |
| child `PATCH` of a plain field (`value`)                | no hook       | no hook                   | no hook               |
| child deleted                                           | —             | `onUpdate` + `postUpdate` | —                     |
| edge `PATCH <parent>/<rel>/<edgeId>` (`_refProperties`) | no hook       | `onUpdate` + `postUpdate` | no hook               |
| parent `PATCH add /<rel>/-`                             | its own run   | its own run, no extra     | its own run, no extra |

Same in both storage modes. With `NOTIFY=1` the edge-triggered `onUpdate` saw
the edge's new state (`active` 0 after the status went to `inactive`) and the
child's current `value`, so a notification-driven run does derive correctly —
but it runs only for writes made **through the parent's relationship
sub-resource**, and for a deleted child. A link made from the child's side and a
change to a child's own fields never reached the parent's hooks under either
placement. The parent-side link add (E10) gets no second, post-persist run, so
its `derived` stays one link behind with or without the flags.

Not covered: notification of a has-many reverse, `notify` on a has-one forward,
and whatever a console-created pair writes beyond these two flags.

## E19 — the audit trail written by `onUpdate`, in memory

`audit` mode: `onUpdate` diffs `oldObject` → `object` over `data` and `items`
(leaf level, arrays by index), and appends one entry per changed leaf —
`{seq, path, from, to, runId, baseRev, ops}` — to `object.audit` before the
write. `auditSeq` on the record numbers them.

### Cost of k changes to nested data, sequential, one writer

| k   | (a) data `PATCH` + audit `PATCH` per change | (b) one `PATCH` per change, hook audits | (c) one `PATCH` with k operations, hook audits |
| --- | ------------------------------------------- | --------------------------------------- | ---------------------------------------------- |
| 1   | 2 calls, 237 ms                             | 1 call, 41 ms                           | 1 call, 41 ms                                  |
| 10  | 20 calls, 819 ms                            | 10 calls, 443 ms                        | 1 call, 40 ms                                  |
| 50  | 100 calls, 4245 ms                          | 50 calls, 2188 ms                       | 1 call, 49 ms                                  |

All three produced k audit entries; (b) and (c) had no chain breaks. (c) also
saw all k operations in `request.patchOperations`. Batching the changes is where
the win is: (c) is flat in k, about **85× faster than (a) at k = 50**.

### Correctness under contention

N writers at once, each `PATCH` carrying three operations: replace a shared leaf
(`/data/shared`), set a leaf of its own (`/data/per/w<i>`), append an item.

| N   | HTTP     | p50 / max ms | audit entries / want | run ids in trail | shared-leaf chain breaks | seq distinct, contiguous | `onUpdate` runs | max runs / request |
| --- | -------- | ------------ | -------------------- | ---------------- | ------------------------ | ------------------------ | --------------- | ------------------ |
| 10  | 10 × 200 | 307 / 421    | 30 / 30              | 10               | 0                        | yes, yes                 | 34              | 7                  |
| 10  | 10 × 200 | 375 / 550    | 30 / 30              | 10               | 0                        | yes, yes                 | 50              | 10                 |
| 10  | 10 × 200 | 474 / 660    | 30 / 30              | 10               | 0                        | yes, yes                 | 53              | 9                  |
| 50  | 50 × 200 | 4571 / 7328  | 150 / 150            | 50               | 0                        | yes, yes                 | 814             | 38                 |
| 50  | 50 × 200 | 3634 / 6412  | 150 / 150            | 50               | 0                        | yes, yes                 | 773             | 38                 |
| 50  | 50 × 200 | 3280 / 5553  | 150 / 150            | 50               | 0                        | yes, yes                 | 747             | 39                 |

- Exactly three entries per committed write, and exactly one `runId` per commit
  in the trail, despite ~15 `onUpdate` runs per commit at N = 50: a retried
  run's entries are discarded with the attempt, because they only ever lived in
  `object`.
- The shared leaf's audit is an unbroken chain (each `from` is the previous
  `to`) and its last `to` equals the stored value, so every run diffed against
  the state it actually committed over.
- The audit is only as fine as the diff: a write that sets a leaf to the value
  it already had leaves no entry (the diff sees no change). Recording intent
  rather than effect means reading `request.patchOperations` instead.

## E20 — tracing the reads and writes one change causes (transaction id)

The question: given one logical change, how many reads and writes did it cause,
and on which objects — ideally from a caller-chosen transaction id alone.

**One `PATCH`** sent with `X-ForgeRock-TransactionId: <root>` to a parent whose
`onUpdate` queries a relationship and creates a sink record, and whose
`postUpdate` creates another. `aic logs tx <root>` over `idm-access`,
`idm-activity`, `idm-core`, `idm-sync`, `idm-config`, `idm-authentication`
returned four events, all stored under `<root>/0`:

| source         | event      | operation | object                         | revision order   |
| -------------- | ---------- | --------- | ------------------------------ | ---------------- |
| `idm-activity` | `activity` | CREATE    | `managed/alpha_poc_sink/<id>`  | n (onUpdate)     |
| `idm-activity` | `activity` | PATCH     | `managed/alpha_poc_agg/<id>`   | n+1              |
| `idm-activity` | `activity` | CREATE    | `managed/alpha_poc_sink/<id>`  | n+2 (postUpdate) |
| `idm-access`   | `access`   | PATCH     | the request path, `SUCCESSFUL` | —                |

- **Writes are traceable by transaction id.** `idm-activity` has one event per
  write the change caused — the request's own and every `openidm.create` /
  `patch` a hook made — each with `operation`, `objectId`, the new `revision`,
  `runAs`/`userId`, `status`, and the full `before`/`after` documents
  (`changedFields` was `[]` here). The revisions' order shows which phase made
  each write.
- **Reads are not logged anywhere.** The hook's relationship query left no event
  in any source. Neither did an endpoint's `openidm.read` and `openidm.query`:
  for a custom-endpoint call doing one of each, the transaction held only the
  `idm-access` event of the outer `POST`.
- **Script `logger` output is not keyed by transaction id.** `logger.info` from
  the hook does reach `idm-core`, as `text/plain` (`"INFO: POC onUpdate run …"`)
  with no transaction id, so `aic logs tx` never returns it. An endpoint's
  `logger.debug/info/warn/error` with the tag in the message were not found by
  transaction id either.
- **A script can read its own transaction id**:
  `context.transactionId.transactionId.value` is `<root>/0` (read from an
  endpoint; only that field was read, never `context` itself). So the way to
  make script logging and read counts traceable is to put that value in the
  message — e.g. `logger.info("TX " + txid + " read managed/x/y")`.
- **Searching `idm-core` by time is slow**: `aic logs grep` scanned about 80,000
  events in 171 s on the sandbox. A message tagged with the transaction id still
  has to be found by a time-window scan, not by `logs tx`.

What `idm-activity` shows under contention — retried runs, and whether a lost
CAS attempt leaves a write — is measured in "E20 under contention" below.

## E16 — a no-op patch

One `PATCH replace /name` with the value the record already holds. Fold mode's
hook would have incremented `derived.runs` had it run; plain mode's hook changes
nothing either way. `trace: true`, so a run would have created a sink.

| mode  | HTTP | `_rev` moved | `onUpdate` ran |
| ----- | ---- | ------------ | -------------- |
| fold  | 200  | no           | no             |
| plain | 200  | no           | no             |

- The short-circuit is before the hook, and it is on the patch's effect, not on
  the hook's. A replace of the stored value returns 200, leaves `_rev` where it
  is, and does not run `onUpdate` even when the hook would have changed the
  record.
- Not measured: a multi-operation patch whose net effect is empty; `add` then
  `remove` of the same value; whether a `304` is ever returned (these two were
  `200`).

## E15 — a hook patches its own parent

Both hooks are guarded, so this is the bounded case. `onUpdate` in `selfpatch`
mode folds, then `openidm.patch`es `/counter` to `1000 + derived.count` when the
inbox entry carries `selfpatch: true`. `postUpdate` in `selfpatch` mode patches
`/counter` to `2000 + counter` while `counter < 2000`. One request each,
`timeoutMs` 90 s. Neither came close to it.

| caller       | HTTP | ms  | `onUpdate` runs | `derived.runs` | stored counter | stored entries |
| ------------ | ---- | --- | --------------- | -------------- | -------------- | -------------- |
| `onUpdate`   | 200  | 294 | 3               | 2              | 1001           | 1              |
| `postUpdate` | 200  | 77  | 2               | 2              | 2000           | 1              |

The caller's response already shows the counter the nested patch wrote.

- From `postUpdate` the nested patch is a second, ordinary revision-less `PATCH`
  after the outer write has committed. Its `onUpdate` saw
  `oldObject.derived.count == 1` and a newer base than the outer run. The guard
  then stops: the counter patch does not patch again. Two hook runs, one extra
  write, no loop.
- From `onUpdate` the nested patch races the attempt that called it. The first
  two runs shared a base revision. The nested run saw an empty inbox (the outer
  add had not committed) and its own fold committed `derived.count == 0` with
  the counter already at 1001. The outer attempt then retried, saw that count of
  0, re-applied the inbox add, and folded to one entry. Its second counter patch
  was a no-op (1001 → 1001), so E16's short-circuit meant no fourth run. Final
  state is right; a reader in the gap would have seen the counter move and the
  entry missing.
- Not measured: the unguarded loop (left bounded on purpose), self-patch under
  concurrent callers, and a nested patch that writes a field the outer fold does
  not recompute.

## E13 — the same appends through a custom endpoint

`POST /openidm/endpoint/pocagg?_action=append` runs
`openidm.patch(parent, null, add /inbox/-)`. The endpoint config `PUT` returned
`201`. Fold mode, trace on, `postUpdate` creating one sink per run — the same
parent setup as E1.

| N   | HTTP     | p50 / p90 / max ms | entries | all present | `derived` consistent | `onUpdate` runs | `postUpdate` runs | requests retried | max runs / request | retry saw same base |
| --- | -------- | ------------------ | ------- | ----------- | -------------------- | --------------- | ----------------- | ---------------- | ------------------ | ------------------- |
| 10  | 10 × 200 | 556 / 662 / 662    | 10      | yes         | yes                  | 46              | 10                | 8                | 9                  | 0                   |
| 10  | 10 × 200 | 331 / 418 / 418    | 10      | yes         | yes                  | 55              | 10                | 9                | 10                 | 0                   |
| 10  | 10 × 200 | 320 / 448 / 448    | 10      | yes         | yes                  | 54              | 10                | 9                | 10                 | 0                   |
| 50  | 50 × 200 | 2831 / 4124 / 4250 | 50      | yes         | yes                  | 755             | 50                | 47               | 37                 | 0                   |
| 50  | 50 × 200 | 2908 / 4345 / 4470 | 50      | yes         | yes                  | 807             | 50                | 49               | 37                 | 0                   |
| 50  | 50 × 200 | 2892 / 4261 / 4377 | 50      | yes         | yes                  | 743             | 50                | 47               | 35                 | 0                   |

- Same outcome as E1: no lost append, no 412, no 5xx, `derived` consistent,
  `postUpdate` once per commit, `onUpdate` re-run on every retry, and no retry
  saw the same base. A script `openidm.patch` with a null revision is the same
  compare-and-swap as the REST `PATCH`.
- Latency sits inside E1's spread (E1's N = 50 p50 was 3281–6225 ms, with
  667–836 `onUpdate` runs). The endpoint does not add a visible cost on top of
  the sink writes the trace makes.
- Caller identity, one uncontended pair fetched with `aic logs tx` over
  `idm-activity` and `idm-access`: the endpoint call and a direct REST `PATCH`
  recorded the same single `userId`, and the same `runAs` on both the parent
  `PATCH` and the `postUpdate` sink `CREATE`. `userId` equalled `runAs` on the
  activity events. The access event is the outer call (`POST` 200 for the
  endpoint, `PATCH` 200 for REST, 28 ms and 29 ms). The script's `openidm.patch`
  has no access event of its own.

## E18 — array size

One parent per size, created already holding that many `entries` (each entry an
id, a number, and a 40-character note). Then five sequential `add /inbox/-` and
ten concurrent ones. Fold mode, trace off. The runner stops at the first create
that is not 200 or 201; none failed.

| entries at create | entries JSON bytes | create ms | sequential p50 / p90 / max | concurrent 10 p50 / p90 / max | final entries | `derived` consistent |
| ----------------- | ------------------ | --------- | -------------------------- | ----------------------------- | ------------- | -------------------- |
| 10                | 721                | 142       | 39 / 188 / 188             | 391 / 430 / 430               | 25            | yes                  |
| 100               | 7381               | 40        | 43 / 45 / 45               | 160 / 228 / 228               | 115           | yes                  |
| 1000              | 74791              | 90        | 89 / 99 / 99               | 792 / 1131 / 1131             | 1015          | yes                  |
| 5000              | 378391             | 256       | 315 / 395 / 395            | 4017 / 5028 / 5028            | 5015          | yes                  |

Every patch was 200. Final count is the seeded array plus 15 appends, and
`derived` matches a recomputation, at every size.

- Create cost stays small through 5000 entries (~370 KB of array JSON, 256 ms).
  No size cap was hit. Appending through the fold gets expensive with the array
  the hook rewrites: a sequential append's p50 goes from about 40 ms at 100
  entries to 89 ms at 1000 and 315 ms at 5000, and ten concurrent appends at
  5000 land at p50 4017 ms, all still 200.
- The size-10 concurrent row is slower than size 100. One run per size, so that
  inversion is noise; the climb from 1000 to 5000 is not.
- A sensible cap for an inbox-fold parent is well under 1000 folded entries if
  writers are concurrent, and the audit-trail pattern (E19) has the same shape
  once `audit` itself is the large array. Not measured: the size at which create
  or patch fails, and a parent whose array is larger than 5000.

## E14 — heavy contention, and the cost of the trace sink

`E14_TRACE=0 E14_N=50,100,200` once, then `E14_N=100` with the trace sink on.
The runner stops at the first non-200. Nothing stopped it. Fold mode,
`postUpdate` still creating one sink per commit. With the trace off there is no
`onUpdate` sink, so `requests retried` and `onUpdate runs` stay 0: that counter
is the sink, and these rows do not say the retry stopped happening.

| trace | N   | HTTP      | p50 / p90 / max ms | entries | all present | `derived` consistent | `onUpdate` runs | `postUpdate` runs | requests retried | max runs / request | retry saw same base |
| ----- | --- | --------- | ------------------ | ------- | ----------- | -------------------- | --------------- | ----------------- | ---------------- | ------------------ | ------------------- |
| off   | 50  | 50 × 200  | 1392 / 2036 / 2093 | 50      | yes         | yes                  | —               | 50                | —                | —                  | —                   |
| off   | 100 | 100 × 200 | 2454 / 3691 / 3842 | 100     | yes         | yes                  | —               | 100               | —                | —                  | —                   |
| off   | 200 | 200 × 200 | 5019 / 8663 / 9348 | 200     | yes         | yes                  | —               | 200               | —                | —                  | —                   |
| on    | 100 | 100 × 200 | 5194 / 8894 / 9236 | 100     | yes         | yes                  | 1670            | 100               | 96               | 50                 | 0                   |

- No lost append, no 412, no 5xx, no timeout, through 200 concurrent writers
  with the trace off and through 100 with it on. `postUpdate` ran once per
  commit at every size (`distinctPostRevs` equals N). No retry bound showed up
  inside this range.
- The untraced N = 50 row is the latency to set next to E1. E1's traced N = 50
  p50 was 3281–6225 ms (max 5260–8888). Untraced p50 is 1392 ms (max 2093). The
  sink create inside `onUpdate` is a large part of the traced cost, and it
  widens the window each attempt holds. Traced N = 100 (p50 5194 ms, 1670 hook
  runs, 96 of 100 requests retried, one request ran the hook 50 times) is still
  all 200, and no retry saw the same base.
- Not measured: N above 200, a failure mode, and the retry counts for the
  untraced rows. `derived.runs` equalled N on those rows, which counts committed
  folds only.

## E20 under contention — one transaction id per request, N = 10

Ten concurrent revision-less `PATCH add /inbox/-` on one fold-mode parent,
`trace: true`, `postMode: "sink"`. Each request carried its own
`X-ForgeRock-TransactionId`, `<stem>-00` … `<stem>-09`, so no id is a prefix of
another. `aic logs tx <stem> --source idm-access,idm-activity --wait` returned
all ten; `aic logs tx <stem>-03` returned only that request (one stored id).
`node trace.mjs <stem>-03` prints the same counts.

All ten HTTP responses were 200 (p50 / p90 / max 479 / 555 / 555 ms). All ten
entries stored, `derived` consistent. Sink records: 55 `onUpdate` + 10
`postUpdate` = 65. Log `CREATE`s of `alpha_poc_sink`: 65.

| request | `onUpdate` sinks | activity sink `CREATE`s | activity parent `PATCH`es | access                       |
| ------- | ---------------- | ----------------------- | ------------------------- | ---------------------------- |
| 00      | 3                | 4                       | 1                         | PATCH SUCCESSFUL 200, 228 ms |
| 01      | 7                | 8                       | 1                         | PATCH SUCCESSFUL 200, 342 ms |
| 02      | 5                | 6                       | 1                         | PATCH SUCCESSFUL 200, 297 ms |
| 03      | 1                | 2                       | 1                         | PATCH SUCCESSFUL 200, 175 ms |
| 04      | 8                | 9                       | 1                         | PATCH SUCCESSFUL 200, 362 ms |
| 05      | 6                | 7                       | 1                         | PATCH SUCCESSFUL 200, 307 ms |
| 06      | 2                | 3                       | 1                         | PATCH SUCCESSFUL 200, 196 ms |
| 07      | 10               | 11                      | 1                         | PATCH SUCCESSFUL 200, 392 ms |
| 08      | 9                | 10                      | 1                         | PATCH SUCCESSFUL 200, 373 ms |
| 09      | 4                | 5                       | 1                         | PATCH SUCCESSFUL 200, 238 ms |

Every stored id was `<root>/0`. No request produced a second stored id. Every
parent `PATCH` was `SUCCESS` and named the one parent. No other write appeared.

- Each request's `idm-activity` shows **exactly one** `PATCH` of the parent,
  however many times `onUpdate` ran. A CAS attempt that lost leaves no write
  event. The access event is the outer call only; server elapsed time is the
  access event's, and it is lower than the client's round trip.
- Retried `onUpdate` side effects **do** appear: sink `CREATE`s equal the
  `onUpdate` sink records plus one `postUpdate` create, on every request,
  including the one whose hook ran ten times. The log and the sink query agree
  at 65.
- `trace.mjs` is the measuring tool: `node trace.mjs <txid-or-stem>`. It cannot
  see reads. A hook counts a read by logging
  `context.transactionId.transactionId.value` next to what it read;
  `aic logs tx` still will not return that line (`idm-core` has no transaction
  id), so the count has to be something the hook writes down, or a later
  time-window grep of `idm-core`.
- Not measured: a failed HTTP request's transaction (every call here was 200),
  and a revision-bearing `PATCH` that returns 412.

## E21 — relationship notification, both ends

Round 1 counted the parent's hooks only. This round adds `onUpdate`,
`postUpdate` and `onDelete` on the child, each writing one `alpha_poc_sink` when
the child has `trace: true`. The parent still writes its own sinks. A run is
void unless a direct `PATCH` of the parent and of the child fires both of that
record's hooks in the same configuration. Those controls fired for every flag
mode below. Hook counts taken the moment the HTTP call returned matched the
counts after the 6 s settle, on every row. Two rows were read again at 30 s (a
plain child field under default flags, and an edge patch through the parent
under `NOTIFY=1`, both on the mapped pair). Nothing further had arrived.

`NOTIFY=1` is `notifySelf: true` on the parent relationship's node and
`notify: true` on the child relationship's node, the same placement as round 1.
`NOTIFY=2` swaps the ends. Default clears both. Flags sit on the relationship
node, not on `resourceCollection`. `alpha_user` does not declare
`notifyRelationships`; that shape is a later part.

### Which hooks ran

Same in both storage modes. Counts are parent `onUpdate`/`postUpdate`, then
child `onUpdate`/`postUpdate`/`onDelete`. "Child update" is 1/1 and no parent
hook. "Parent update" is 1/1 and no child hook.

| Write                                          | default                            | `NOTIFY=1`                         | `NOTIFY=2`                         |
| ---------------------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| child created with its reverse link set        | neither                            | neither                            | neither                            |
| child `PATCH` of its reverse link              | child update; destination parent 0 | child update; destination parent 0 | child update; destination parent 0 |
| child `PATCH` of a plain field (`value`)       | child update                       | child update                       | child update                       |
| child deleted                                  | child `onDelete` only              | child `onDelete` only              | child `onDelete` only              |
| edge `PATCH` through the parent's relationship | neither                            | parent update                      | neither                            |
| edge `PATCH` through the child's relationship  | neither                            | neither                            | child update                       |
| parent `PATCH add /<rel>/-`                    | parent update, no second run       | parent update, no second run       | parent update, no second run       |
| parent `PATCH` of a plain field (`name`)       | parent update                      | parent update                      | parent update                      |
| parent deleted                                 | neither                            | neither                            | neither                            |

The child's reverse is addressable. `PATCH <child>/<rel>` replacing
`/_refProperties/status` returned 200 on the first attempt, for both the mapped
`parent` and the generic `gparent`. No second path was needed.

- A create is not an update, so a child created with its reverse link set runs
  no child hook either.
- The child's own reverse-link replace runs the child's update hooks and does
  not run either parent's. Round 1 left the `NOTIFY=2` cell of that row blank;
  it is the same as the other two flag modes.
- Round 1 left child-delete blank for default and `NOTIFY=2`, and reported
  parent `onUpdate` + `postUpdate` for `NOTIFY=1`. This run fills all three: the
  child's `onDelete` runs, and the parent's hooks do not, in either storage
  mode. That **disagrees with round 1** on `NOTIFY=1`. The disagreement survived
  a second run (`deletewait`, 112 s). `config/managed` then had
  `notifySelf: true` on the parent's relationship node and `notify: true` on the
  child's, and nothing on `resourceCollection`. The same configuration's edge
  patch through the parent did run the parent hooks, so the flag path is live.
  Waiting 30 s after the delete added no parent hook, mapped or generic.
  Stripping the child's scripts and deleting again (mapped, another 30 s) still
  ran no parent hook, and the mapped parent's `_rev` still moved. The child's
  `onDelete` is not what suppresses the notification. The delete measured here
  is `DELETE` of the child record. Ping's relationship-notification page
  describes a DELETE of the relationship URL (`…/manager`), which is not this
  row.
- `NOTIFY=2` makes a child-side edge patch run the child's own update hooks. It
  does not notify the parent. `NOTIFY=1` does not make a child-side edge patch
  run either side.
- A parent plain-field patch never runs the child's hooks. A child plain-field
  patch never runs the parent's, in any of these three configs. Deleting the
  parent runs nothing on the child. The parent has no `onDelete`, so its own
  delete runs nothing either.
- The parent-side link add is still one run, not a second post-persist run.
  Round 1's stale `derived` on that write is unchanged by these flags; this
  table did not re-read `derived`.

### Whose `_rev` moved

`null` means the record was deleted, or it did not exist before a create. A
deletion is not counted as a revision move.

| Write                                                   | mapped parent                       | generic parent          | child   |
| ------------------------------------------------------- | ----------------------------------- | ----------------------- | ------- |
| child created with its reverse link set                 | yes                                 | no                      | created |
| child `PATCH` of its reverse link                       | yes, and the destination parent too | no, destination neither | no      |
| child `PATCH` of a plain field                          | no                                  | no                      | yes     |
| child deleted                                           | yes                                 | no                      | deleted |
| edge `PATCH` through the parent, default and `NOTIFY=2` | yes                                 | no                      | no      |
| edge `PATCH` through the parent, `NOTIFY=1`             | yes                                 | **yes**                 | no      |
| edge `PATCH` through the child, all three flag modes    | yes                                 | no                      | no      |
| parent `PATCH add /<rel>/-`                             | yes                                 | yes                     | no      |
| parent `PATCH` of a plain field                         | yes                                 | yes                     | no      |
| parent deleted                                          | deleted                             | deleted                 | no      |

- A mapped membership or edge change moves the parent's `_rev` even when no
  parent hook runs. A generic one does not, except the `NOTIFY=1` edge patch
  through the parent: that notification commits a parent update, and the generic
  parent's `_rev` moves with it. Round 1's "a generic edge write does not move
  `_rev`" still holds for the unflagged write and for `NOTIFY=2`.
- Replacing the child's reverse link on the mapped pair moves `_rev` on the
  parent it leaves and the parent it joins, and runs neither parent's hooks. The
  child's own `_rev` stays put while its update hooks run.
- The same split on a child-side edge patch: mapped parent `_rev` moves, generic
  parent `_rev` does not, child's `_rev` does not, including the `NOTIFY=2` row
  whose child hooks did run. The relationship state is not the child's JSON
  revision.
- One sample of client latency for the edge patch through the parent, mapped
  then generic: default 48 ms / 44 ms, `NOTIFY=1` 94 ms / 73 ms, `NOTIFY=2` 50
  ms / 42 ms. The `NOTIFY=1` response already contained the sink, so the client
  waited for the notified run. Whether that run shares the caller's transaction
  id is a later part.

### `notify` on `resourceCollection`

Ping puts `notify` on `resourceCollection`, not on the relationship node. The
same writes, both storage modes (`placement`, 119 s). The live document matched:
`NOTIFY=1` had `notify: true` only on the child's `resourceCollection` and
`notifySelf: true` on the parent's relationship node; `NOTIFY=2` swapped those.
A direct patch still fired both ends first.

| Write                             | `NOTIFY=1` collection              | `NOTIFY=2` collection          |
| --------------------------------- | ---------------------------------- | ------------------------------ |
| child `PATCH` of its reverse link | parent update and child update     | child update; no parent        |
| child `PATCH` of a plain field    | child update; no parent            | child update; no parent        |
| child record `DELETE`             | parent update and child `onDelete` | child `onDelete`; no parent    |
| edge `PATCH` through the parent   | parent update                      | child update; no parent        |
| edge `PATCH` through the child    | parent update; no child            | child update; no parent        |
| parent `PATCH add /<rel>/-`       | parent, once; no child             | parent, once, and child update |

Same hooks in both storage modes. Where the parent was notified, the generic
parent's `_rev` moved as well, because the notification commits a parent update.
Where it was not, a generic membership or edge change still left that `_rev`
put, and a mapped one still moved it.

- This is the placement that notifies the referenced object, which is what
  Ping's page says `notify` does. It does it for a `DELETE` of the child record
  as well as for a relationship write. Round 1's parent-hook-on-delete result
  matches this placement. It does not match `notify` on the relationship node,
  which is where round 1's script wrote the flag and where the table above
  measured no parent hook.
- A plain field change still does not cross, in either placement.
- `NOTIFY=2` on `resourceCollection` notifies the child when the parent writes
  the relationship (edge patch or link add). The parent's own link add is then
  its own run plus the child's update. Node placement of the same flag did not
  run the child's hooks for that add.

### `notifyRelationships`

Object-level, which is what the brief asked to set and what the tenant does not
use. The live document stored `["parent","gparent"]` on the child object and
`["children","gchildren"]` on the parent object. With default flags, `NOTIFY=1`
on the node, and `NOTIFY=2` on the node, a child plain-field change and a child
reverse-link replace did not run the parent's hooks, and a parent plain-field
change and a parent link add did not run the child's. Both storage modes. The
key is accepted and ignored for these writes.

On the field that changes, which is where `alpha_role.temporalConstraints` and
`alpha_assignment.attributes` put it. No `notify` flag was set for the first row
of each pair, and adding the flags did not change that row.

| Changed field | names                   | Write                | other side                | other `_rev`         |
| ------------- | ----------------------- | -------------------- | ------------------------- | -------------------- |
| child `value` | `parent`, `gparent`     | child plain field    | parent update, both modes | generic parent moves |
| child `value` | `parent`, `gparent`     | reverse-link replace | no parent                 | generic parent stays |
| parent `name` | `children`, `gchildren` | parent plain field   | child update, both modes  | child stays          |
| parent `name` | `children`, `gchildren` | parent link add      | no child                  | —                    |

The reverse-link replace did notify the parent when `notify` was also on the
child's `resourceCollection`. That is the `notify` result above, not this field:
the same replace with the field set and no `notify` flag did not. A child
`value` patch took 91–114 ms against about 52 ms with no `notifyRelationships`,
and the sink was in the response. The parent script assigns `object.derived`, so
that notified run has a document change to commit. The child script does not
assign to `object`; its hooks ran and its `_rev` did not move. A child script
that mutates `object` was not the one measured.

`alpha_user` declares no `notifyRelationships`. The field placement above is the
shape on `alpha_role.temporalConstraints` and `alpha_assignment.attributes`.

The role copy puts `notify` only on the named relationship's
`resourceCollection` and `notifyRelationships` on `value`. A child plain-field
change still ran the parent's hooks, both modes (101 ms mapped, 110 ms generic),
and the generic parent's `_rev` moved. A reverse-link replace also ran both
parents' hooks, the one the child left and the one it joined, and moved both
generic parents' `_rev`. That replace did not do so without the
`resourceCollection` flag, so the extra parent runs are the `notify` result, not
the field. Under `NOTIFY=1` on `resourceCollection` alone, that replace already
ran both parents; the node-placement table's destination cell stayed at no
hooks.

### Naming a relationship from a relationship

`notifyRelationships` on the relationship property, set to its own name
(`parent` to `["parent"]`, `gparent` to `["gparent"]`), took that managed
resource offline. `GET` of `alpha_poc_child` returned 404,
`Resource 'managed/alpha_poc_child' not found`, for eight tries, about 24 s.
`config/managed` still listed the type. The parent resource stayed 200. The
mirror on the parent 404'd the parent and left the child at 200. Clearing the
property returned each type to 200, and the hook control fired again. The fanout
matrix was not driven at those configs. No "no hook" cell comes from them.

Naming the other relationship, which is what `alpha_organization.parent` does
(`children` and `members`, not itself): `parent` names `gparent` and `gparent`
names `parent`. The live document stored that. The hook control fired. Changing
the named relationship did not run the far object's hooks, and did not move the
far parent's `_rev`. A plain field change did not either.

| Config                                                                      | Write                                         | Far object     | Referenced object            |
| --------------------------------------------------------------------------- | --------------------------------------------- | -------------- | ---------------------------- |
| no flags                                                                    | replace `parent`, watch the `gparent` parent  | no hooks       | no hooks                     |
| no flags                                                                    | replace `gparent`, watch the `parent` parent  | no hooks       | no hooks                     |
| no flags                                                                    | child plain field, watch the `gparent` parent | no hooks       | —                            |
| `notifySelf` on the node, `notify` on `resourceCollection`, names the other | replace `parent`                              | no hooks       | parent update                |
| same                                                                        | replace `gparent`                             | no hooks       | parent update                |
| same                                                                        | child plain field                             | no hooks       | —                            |
| parent side, no flags: `children` names `gchildren`                         | add `children`, watch the `gchildren` child   | no child hooks | parent update, its own patch |
| same                                                                        | parent plain field                            | no child hooks | parent update                |
| same                                                                        | add `gchildren`, watch the `children` child   | no child hooks | parent update                |

The referenced-object update in the flagged rows is `notify` on
`resourceCollection` of the relationship that changed. The far object, the one
`notifyRelationships` names, was not told. On the generic direction the
referenced parent's `_rev` stayed put with no flags and moved once `notify` was
on `resourceCollection`. Ping's page also says a notification received through a
field propagates out along that field's `notifyRelationships`. That second hop
was not isolated: no config both notified an object through one relationship and
named a further relationship on the field that received it.

### Has-many reverse, and `notify` on a has-one forward

`peers` / `peerOf` is generic on both ends, both has-many, `status` declared on
the parent end only. Node placement, three flag modes, the same writes as the
first table. The hook cells match that table, including child `onDelete` only on
delete under `NOTIFY=1`. The `_rev` cells match its generic column: the parent's
`_rev` moves on its own patch, and on the `NOTIFY=1` edge patch through the
parent, and not on a child-side edge or link write. The child-side edge write
was `PATCH <child>/peerOf/<edgeId>` of `/_refProperties/status` and
returned 200. The value was not read back in this part.

`owner` (has-one on the child) / `owned` (has-many on the parent) is generic.
`status` is on `owned` only. `none` matches the node-placement table's generic
column. `notify` on the `owner` relationship node changed nothing. `notifySelf`
on `owner` made a child-side edge patch run the child's hooks and not the
parent's. `notify` on `owner`'s `resourceCollection` notified the parent on
create-with-link, reverse-link replace, child-side edge patch, and child
`DELETE`, and not on a plain field or a parent-side edge patch. The parent's
`_rev` moved on those notified rows. `notify` on `owned`'s `resourceCollection`
notified the child on a parent-side edge patch and on the parent's link add, and
the child's `_rev` stayed put. `notifySelf` on `owned` made the parent-side edge
patch run the parent's hooks and move its `_rev`.

Both ends in the tenant shape (`notifySelf` on the node, `notify` on
`resourceCollection`): a relationship write ran both objects' hooks, a plain
field did not cross, and child `DELETE` ran the parent's hooks plus the child's
`onDelete`. Create-with-link ran the parent once and the child not at all.

`value.notifyRelationships` of `["owner"]` plus `notify` on `owner`'s
`resourceCollection`: a child plain-field change ran the parent (the field, not
the flag). Create-with-link ran the parent twice. Every other create with
`notify` on that collection and no `notifyRelationships` ran the parent once.
The two runs were not separated.

### Where `status` is, and `returnByDefault`

`status` is declared on the parent end only. Both sides' relationship reads
returned `_refProperties` keys `_id`, `_rev` and `status`, mapped and generic.
`PATCH` of the child's has-one subresource (`PATCH <child>/<rel>` of
`/_refProperties/status`) returned 200 and the parent then read the new value. A
patch through the parent returned 200 and the child then read that value. One
value on the shared edge, not a copy per side. The child-side write moved the
mapped parent's `_rev`, not the generic parent's, and not the child's. The
parent-side write did the same. That repeats the `_rev` table above.

`GET` of either object omits every relationship when `returnByDefault` is false,
and includes them when it is true. `GET` with `_fields=<rel>` returns the edge
either way, including `status`. A plain-field `onUpdate` with `returnByDefault`
false saw no relationship field in `object` or `oldObject`, on the child (a
`label` change) and on the parent (a `counter` change from 0 to 7). With it
true, both sides' plain-field hooks saw the edge, and `status` was in it. The
first parent snapshot at `returnByDefault` false was the link-add, not a
plain-field run: the harness replaced `counter` 0 with 0, which does not run
`onUpdate`. The 0-to-7 patch is the one that counts. On that link-add, `object`
did contain `children`. That is the relationship being written, and it is not
the plain-field result.

### The notified run and the caller's request

Edge patch through the mapped parent, `NOTIFY=1` on the node, three samples,
then the same with the flags cleared. The hook control fired in both
configurations. The sink was in the response, and the 6 s settle added nothing.

| Flags | Client time   | Access event       | Activity under that fetch                                             |
| ----- | ------------- | ------------------ | --------------------------------------------------------------------- |
| on    | 85, 72, 77 ms | `PATCH` 200, 67 ms | two `alpha_poc_sink` creates, one `relationshipUpdated` on the parent |
| off   | 46, 48, 47 ms | `PATCH` 200, 30 ms | no activity events                                                    |

The id the hook stored was the id the client sent plus `/0`, one value, not a
second transaction. `aic logs tx` prefix-matches, so the fetch was by the id the
client sent and the stored row's id was longer than that. The summary kept the
relation and not the activity row's id, so equality with `<sent>/0` was checked
for the hook and not for the activity row. The two sink creates were in the one
transaction that fetch returned. There was no separate parent `UPDATE` event in
it. With the flags off, the same fetch returned the access event and no
activity. Ping's page says relationship auditing is independent of these flags.
This fetch did not see a `relationshipUpdated` without the flag. An audit under
an id that does not start with the one the client sent would not be in the
fetch, and was not searched for.

### Notified edge patches under contention

`NOTIFY=1` on the node, ten concurrent edge patches of ten different edges,
`children` mode, `trace` on. Setup's own link-add hook had already left
`derived` at count 9, sum 45: the hook runs before the link it is adding is
visible, which is the round-1 result. The burst then recomputed.

| Pair    | HTTP         | `onUpdate` | `postUpdate` | distinct `oldRev`                       | final edges        | final `derived`            |
| ------- | ------------ | ---------- | ------------ | --------------------------------------- | ------------------ | -------------------------- |
| mapped  | 9×200, 1×404 | 38         | 9            | 16, and a repeated consecutive `oldRev` | 9, all `inactive`  | count 9, sum 50, active 0  |
| generic | 10×200       | 38         | 10           | 10, and a repeated consecutive `oldRev` | 10, all `inactive` | count 10, sum 55, active 0 |

The counts at the moment the calls returned were the settled counts. Every run
recorded the same `request.patchOperations` text, so that field did not
distinguish the ten patches. Not every `onUpdate` committed: 38 runs against 9
and 10 `postUpdate`s, with a repeated `oldRev`, which is the revision-less
`PATCH` retry. The generic final `derived` matches the ten children (values 1
through 10, all inactive). The mapped final `derived` matches a nine-edge
relationship in count and active state, and the sum is 50 rather than 55. One
mapped edge was absent at the end. The 404 body was not kept. The wanted count
of 10 was not what the mapped pair stored.

### Not measured

`GET /openidm/repo/relationships?_queryFilter=true&_pageSize=1` returned 403,
body `Access denied`. Where a generic link is stored was not visible to this
service account. What a console-created pair writes still needs the console.
`DELETE` of the relationship URL, as opposed to `DELETE` of the child record,
was not its own row. The propagation of a notification received through one
relationship out along another was not isolated. A child script that assigns to
`object` was not the script measured, so a notified child run that would move
the child's `_rev` was not measured.
