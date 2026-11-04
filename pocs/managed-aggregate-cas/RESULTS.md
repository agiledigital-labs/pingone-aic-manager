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
