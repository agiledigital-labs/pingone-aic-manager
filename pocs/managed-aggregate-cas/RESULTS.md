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
