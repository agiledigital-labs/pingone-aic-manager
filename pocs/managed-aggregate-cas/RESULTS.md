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

Every call succeeds and almost every update is lost: last writer wins.
`derived` stayed consistent with what was stored, because `onUpdate` recomputes
from the stored entries — it cannot recover what the caller dropped.

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

| Operation                                     | N  | HTTP     | Result                         |
| --------------------------------------------- | -- | -------- | ------------------------------ |
| `increment /counter` by 1                     | 10 | 10 × 200 | counter 10 (exact)             |
| `increment /counter` by 1                     | 50 | 50 × 200 | counter 50 (exact); p50 1040 ms |
| `add /items/0`, `items` absent                | 10 | 10 × 403 | "Failed policy validation": the index made an object, not an array |

Array semantics, single requests:

| Operation                                    | Result                                                                 |
| -------------------------------------------- | ---------------------------------------------------------------------- |
| `add /items/-` on an absent array            | `200`, array created                                                   |
| `add /items/0` on an existing array          | `400` "Position-based operation is not allowed: /items/0"              |
| `remove /items/0`                            | `400`, same message                                                    |
| `remove /items` with `value`                 | `200`, removes **every** element equal to the value                    |
| `replace /items/0/w` (field under an index)  | `200`                                                                  |
| `replace /items/5/w` (index out of range)    | `500`, empty body                                                      |
| `increment /data/n`, `data.n` absent         | `200`, creates it as the increment                                     |
| one `PATCH` with `add /-`, `replace`, and a new nested path | `200`, all applied                                      |

So `/-` append and `increment` are the only position-free primitives. Anything
that edits an existing array element by index is position-based and is either
refused or races: index `i` read by the client may name a different element by
the time the retry runs.

## E8 — `postUpdate` throws

`throw { code: 500, message: "POC_POST_THROW" }` in `postUpdate`: the caller gets
`500` "POC_POST_THROW", and the change **is stored** (entry present, `derived`
updated). A caller that treats that 500 as "not written" and retries appends
twice. Never throw from `postUpdate` to signal failure; and a writer that may
retry must make its inbox entry idempotent (an id the fold de-duplicates on).

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
`onUpdate` (children mode) queries `<parent>/<rel>` and stores count and sum.
No notification settings (the `aic`/console defaults).

| pair      | N  | HTTP     | links | `derived.count` | `derived.sum` / want | `onUpdate` runs | requests retried |
| --------- | -- | -------- | ----- | --------------- | -------------------- | --------------- | ---------------- |
| mapped    | 5  | 5 × 200  | 5     | 0               | 0 / 15               | 15              | 4                |
| mapped    | 5  | 5 × 200  | 5     | 4               | 11 / 15              | 23              | 4                |
| mapped    | 5  | 5 × 200  | 5     | 4               | 10 / 15              | 25              | 4                |
| mapped    | 20 | 20 × 200 | 20    | 19              | 194 / 210            | 245             | 19               |
| mapped    | 20 | 20 × 200 | 20    | 19              | 207 / 210            | 248             | 19               |
| mapped    | 20 | 20 × 200 | 20    | 19              | 193 / 210            | 269             | 19               |
| generic   | 5  | 5 × 200  | 5     | 4               | 10 / 15              | 15              | 4                |
| generic   | 5  | 5 × 200  | 5     | 4               | 14 / 15              | 15              | 4                |
| generic   | 5  | 5 × 200  | 5     | 4               | 11 / 15              | 15              | 4                |
| generic   | 20 | 20 × 200 | 20    | 19              | 203 / 210            | 209             | 19               |
| generic   | 20 | 20 × 200 | 20    | 19              | 208 / 210            | 203             | 19               |
| generic   | 20 | 20 × 200 | 20    | 19              | 194 / 210            | 199             | 19               |

- Every link landed, in both modes.
- `derived` is **always** wrong: `onUpdate` runs before the request's own link
  is persisted, so the last writer's link is never counted, and which N−1 are
  counted depends on the interleaving (the sums differ run to run).
- The first mapped run counted nothing; it was the first write through a pair
  `aic` had just mapped, and `aic` warns that a fresh pair needs a few seconds
  before it is live. Not investigated further.
- The retries here come from `derived` itself: a plain field the hook changes
  on every commit, which moves `_rev`. That is not evidence that link writes
  are inside the CAS — E11 shows they are not.

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

`NOTIFY=1`: `notifySelf: true` on the parent's relationship items, `notify:
true` on the child's reverse property. `NOTIFY=2` swaps the ends, in case the
flags were read the wrong way round. The parent has `trace` and `postMode:
"sink"`, so both hooks' runs are counted. Each row is a single write, waited on
for 4–8 s.

| Write                                                       | default flags | `NOTIFY=1`              | `NOTIFY=2`              |
| ----------------------------------------------------------- | ------------- | ----------------------- | ----------------------- |
| child created with its reverse link set (child side)        | no hook       | no hook                 | no hook                 |
| child `PATCH replace` of its reverse link                   | no hook       | no hook                 | —                       |
| child `PATCH` of a plain field (`value`)                    | no hook       | no hook                 | no hook                 |
| child deleted                                               | —             | `onUpdate` + `postUpdate` | —                     |
| edge `PATCH <parent>/<rel>/<edgeId>` (`_refProperties`)     | no hook       | `onUpdate` + `postUpdate` | no hook               |
| parent `PATCH add /<rel>/-`                                 | its own run   | its own run, no extra   | its own run, no extra   |

Same in both storage modes. With `NOTIFY=1` the edge-triggered `onUpdate` saw
the edge's new state (`active` 0 after the status went to `inactive`) and the
child's current `value`, so a notification-driven run does derive correctly —
but it runs only for writes made **through the parent's relationship
sub-resource**, and for a deleted child. A link made from the child's side and
a change to a child's own fields never reached the parent's hooks under either
placement. The parent-side link add (E10) gets no second, post-persist run, so
its `derived` stays one link behind with or without the flags.

Not covered: notification of a has-many reverse, `notify` on a has-one
forward, and whatever a console-created pair writes beyond these two flags.

## E19 — the audit trail written by `onUpdate`, in memory

`audit` mode: `onUpdate` diffs `oldObject` → `object` over `data` and `items`
(leaf level, arrays by index), and appends one entry per changed leaf —
`{seq, path, from, to, runId, baseRev, ops}` — to `object.audit` before the
write. `auditSeq` on the record numbers them.

### Cost of k changes to nested data, sequential, one writer

| k  | (a) data `PATCH` + audit `PATCH` per change | (b) one `PATCH` per change, hook audits | (c) one `PATCH` with k operations, hook audits |
| -- | ------------------------------------------- | --------------------------------------- | ---------------------------------------------- |
| 1  | 2 calls, 237 ms                             | 1 call, 41 ms                           | 1 call, 41 ms                                  |
| 10 | 20 calls, 819 ms                            | 10 calls, 443 ms                        | 1 call, 40 ms                                  |
| 50 | 100 calls, 4245 ms                          | 50 calls, 2188 ms                       | 1 call, 49 ms                                  |

All three produced k audit entries; (b) and (c) had no chain breaks. (c) also
saw all k operations in `request.patchOperations`. Batching the changes is
where the win is: (c) is flat in k, about **85× faster than (a) at k = 50**.

### Correctness under contention

N writers at once, each `PATCH` carrying three operations: replace a shared
leaf (`/data/shared`), set a leaf of its own (`/data/per/w<i>`), append an item.

| N  | HTTP     | p50 / max ms | audit entries / want | run ids in trail | shared-leaf chain breaks | seq distinct, contiguous | `onUpdate` runs | max runs / request |
| -- | -------- | ------------ | -------------------- | ---------------- | ------------------------ | ------------------------ | --------------- | ------------------ |
| 10 | 10 × 200 | 307 / 421    | 30 / 30              | 10               | 0                        | yes, yes                 | 34              | 7                  |
| 10 | 10 × 200 | 375 / 550    | 30 / 30              | 10               | 0                        | yes, yes                 | 50              | 10                 |
| 10 | 10 × 200 | 474 / 660    | 30 / 30              | 10               | 0                        | yes, yes                 | 53              | 9                  |
| 50 | 50 × 200 | 4571 / 7328  | 150 / 150            | 50               | 0                        | yes, yes                 | 814             | 38                 |
| 50 | 50 × 200 | 3634 / 6412  | 150 / 150            | 50               | 0                        | yes, yes                 | 773             | 38                 |
| 50 | 50 × 200 | 3280 / 5553  | 150 / 150            | 50               | 0                        | yes, yes                 | 747             | 39                 |

- Exactly three entries per committed write, and exactly one `runId` per
  commit in the trail, despite ~15 `onUpdate` runs per commit at N = 50: a
  retried run's entries are discarded with the attempt, because they only ever
  lived in `object`.
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

| source         | event      | operation | object                           | revision order |
| -------------- | ---------- | --------- | -------------------------------- | -------------- |
| `idm-activity` | `activity` | CREATE    | `managed/alpha_poc_sink/<id>`    | n (onUpdate)   |
| `idm-activity` | `activity` | PATCH     | `managed/alpha_poc_agg/<id>`     | n+1            |
| `idm-activity` | `activity` | CREATE    | `managed/alpha_poc_sink/<id>`    | n+2 (postUpdate) |
| `idm-access`   | `access`   | PATCH     | the request path, `SUCCESSFUL`   | —              |

- **Writes are traceable by transaction id.** `idm-activity` has one event per
  write the change caused — the request's own and every `openidm.create` /
  `patch` a hook made — each with `operation`, `objectId`, the new `revision`,
  `runAs`/`userId`, `status`, and the full `before`/`after` documents
  (`changedFields` was `[]` here). The revisions' order shows which phase made
  each write.
- **Reads are not logged anywhere.** The hook's relationship query left no
  event in any source. Neither did an endpoint's `openidm.read` and
  `openidm.query`: for a custom-endpoint call doing one of each, the
  transaction held only the `idm-access` event of the outer `POST`.
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
  events in 171 s on the sandbox. A message tagged with the transaction id
  still has to be found by a time-window scan, not by `logs tx`.

Not yet measured: what `idm-activity` shows under contention — whether a CAS
attempt that lost and retried leaves any event (expected not: only the
committed write is a write), and whether retried `onUpdate` side effects each
appear (expected yes, as E6's sinks did).
