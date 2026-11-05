# Managed-aggregate CAS POC — done note

Work in progress. Updated as experiments land. The measured tables live in
`RESULTS.md`; this file is the run log, the surprises, and where the brief was
wrong.

## Ran

- **E16** (2026-11-05, `node experiments.mjs E16`, 2 s). A `replace` of `/name`
  with the value already stored returns 200, does not move `_rev`, and does not
  run `onUpdate`, in fold mode and in plain mode. The short-circuit is before
  the hook: fold mode would have incremented `derived.runs`.

## Surprises

- None yet beyond E16's status being 200 rather than a short-circuit status. The
  fork's "code says it short-circuits" is confirmed for the hook and the
  revision; the HTTP status is still success.

## Divergences from the brief

- None yet.

## Teardown

Not run.

## Open questions

- None yet.
