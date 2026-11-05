# Managed-aggregate CAS POC — done note

Work in progress. Updated as experiments land. The measured tables live in
`RESULTS.md`; this file is the run log, the surprises, and where the brief was
wrong.

## Ran

- **E16** (2026-11-05, `node experiments.mjs E16`, 2 s). A `replace` of `/name`
  with the value already stored returns 200, does not move `_rev`, and does not
  run `onUpdate`, in fold mode and in plain mode. The short-circuit is before
  the hook: fold mode would have incremented `derived.runs`.
- **E15** (2026-11-05, `node experiments.mjs E15`, 1 s). Neither self-patch
  hung. `postUpdate`'s nested patch is a normal second `PATCH` after the commit
  and the counter guard stops the loop. `onUpdate`'s nested patch races the
  in-flight attempt, commits a gap state (`counter` 1001, `derived.count` 0),
  and the outer attempt retries into the right final state. See RESULTS.
- **E13** (2026-11-05, `node experiments.mjs E13`, 17 s, plus one identity pair
  via `aic logs tx`). Endpoint `openidm.patch` matches E1. The activity `userId`
  / `runAs` match a direct REST `PATCH`. The script patch has no access event of
  its own.
- **E18** (2026-11-05, `node experiments.mjs E18`, 11 s). Create and append
  succeeded at 10, 100, 1000 and 5000 entries. No cap was hit. Concurrent append
  latency climbs sharply by 5000 (p50 4017 ms for N = 10).
- **E14** (2026-11-05). `E14_TRACE=0 E14_N=50,100,200` (16 s) all 200, then
  traced `E14_N=100` (11 s) all 200 with 1670 `onUpdate` runs. No retry bound
  inside this range. Untraced rows cannot count retries: that counter is the
  trace sink. Untraced N = 50 p50 is 1392 ms against E1's traced 3281–6225 ms.
- **E20 under contention** (2026-11-05, `node experiments.mjs E20`, 20 s). Ten
  concurrent appends, zero-padded transaction ids under one stem. Each request's
  `idm-activity` has exactly one parent `PATCH` and one sink `CREATE` per
  `onUpdate` run plus one `postUpdate` create. Lost CAS attempts leave no write
  event. Retried side effects do. Log creates (65) match the sink query.
  `trace.mjs` prints the same summary.

## Surprises

- E16 returns 200, not a distinct short-circuit status. The hook and `_rev` are
  skipped; the HTTP status is still success.
- E15 from `onUpdate` does not deadlock at one caller. It loses the race against
  its own nested patch and retries. `derived.runs` is 2 while the trace sink
  shows 3 runs, because one attempt was discarded — the same shape as E6, plus a
  committed gap state a reader can observe.

## Divergences from the brief

- None yet. The 90 s hang did not happen; both E15 calls returned 200.

## Teardown

Not run.

## Open questions

- None yet.
