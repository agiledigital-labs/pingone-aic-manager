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

Not yet measured: what `idm-activity` shows under contention — whether a CAS
attempt that lost and retried leaves any event (expected not: only the committed
write is a write), and whether retried `onUpdate` side effects each appear
(expected yes, as E6's sinks did).

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
