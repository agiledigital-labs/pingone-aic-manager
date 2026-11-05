# Managed-aggregate CAS POC — done note

The experiments and the docs are in. Teardown restored `config/managed` and left
two `repo.ds` mappings. The measured tables live in `RESULTS.md`; this file is
the run log, the surprises, and where the brief was wrong.

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
- **Docs** (2026-11-05, commit `660ec9e`). `docs/api/10-managed-objects.md`
  gained the concurrent-`PATCH` section, the array-operations subsection, and
  the relationship-notification subsection. Its Verified-against bullet dates
  those measurements. `docs/api/08-logs.md` gained the hook-writes subsection.
  `docs/api/99-quirks-and-open-questions.md` records the four OpenIDM-fork
  claims and the lost create. `docs/managed-object-aggregates.md` is the pattern
  guide. `README.md` in this directory is how to re-run.
  `improvement-suggestions-atomic-poc.md` is the `aic` list, checked against
  `src/managed` and `src/logs` before it was written. `setup.mjs` teardown
  deletes the `pocagg` and `poctrace` endpoints before the backup compare.

## Surprises

- E16 returns 200, not a distinct short-circuit status. The hook and `_rev` are
  skipped; the HTTP status is still success.
- E15 from `onUpdate` does not deadlock at one caller. It loses the race against
  its own nested patch and retries. `derived.runs` is 2 while the trace sink
  shows 3 runs, because one attempt was discarded — the same shape as E6, plus a
  committed gap state a reader can observe.
- `relationship delete --remove-mapping` refused this pair and wrote nothing,
  schema included. The refusal is computed against the managed document the
  delete would write, in which both ends are already gone, so each mapping looks
  stale: `alpha_poc_child.parent` is that object's last mapping, and
  `alpha_poc_agg.children` is read through by it (`src/managed/repair.rs`,
  surfaced by `src/managed/cli.rs`). The live objects were still present when
  the command returned. The later object deletes removed the types and left the
  two `genericMapping` entries.
- The teardown script's compare, immediately after it had printed
  `managed object alpha_poc_sink deleted`, still listed `alpha_poc_sink`. One
  more `aic managed object delete alpha_poc_sink` removed it.
  `delete_object_request` confirms only `ObjectAbsent` for the name being
  deleted (`src/managed/ops.rs`). The child delete's document still contained
  the sink type, and a full-document `PUT` from that earlier command can land
  after the sink delete's confirm. Same family as the lost create in the
  2026-11-05 quirks note.

## Divergences from the brief

- E15's 90 s hang did not happen. Both calls returned 200. The gap state is the
  result the timeout was there to catch, and the calls finished in under a
  second.
- E13 and E18 landed in one commit (`102c091`). The brief asked for a commit
  after each experiment.
- E14's untraced rows cannot report retry counts. The counter is the trace sink.
  The traced N = 100 row is the one that counts retries.
- E14 and E18 never saw a non-200 or a failed create, so the "stop at the first
  failure" branches did not run.
- `docs/api/*.md` was spliced in place so the existing pages stayed at their
  hand-wrapped width. The new subsections are wrapped near 80 columns.
- Teardown did not print `config equals backup: true` and
  `genericMapping equals backup: true`. Stopped there. No hand-written `PUT` of
  `config/managed` or `config/repo.ds`. The one extra command was a second
  `aic managed object delete alpha_poc_sink`, the same verb the script had just
  run, after the script's own read still showed the type.

## Teardown

`node setup.mjs teardown` on 2026-11-05, exit code 1. Node's `NO_COLOR` warning
and the `target tenant: sandbox` lines are omitted. The relationship-delete
failure line below says `$AIC` in place of this machine's binary path:

```
alpha_poc_agg: deleted 100 record(s)
alpha_poc_child: deleted 218 record(s)
alpha_poc_sink: deleted 11839 record(s)
aic error: Config error: refusing relationship delete alpha_poc_agg.children --remove-mapping; nothing was written: refusing to repair alpha_poc_agg.children: config/repo.ds reads through it from alpha_poc_child.parent (also stale, but repair refuses it too: it is its object's last mapping). Removing a mapping another one reads through is unmeasured; refusing to repair alpha_poc_child.parent: it is the last mapped property of managed/alpha_poc_child, and removing an object's last mapping is unmeasured
children: delete failed: Command failed: $AIC --no-prompt managed relationship delete alpha_poc_agg.children --remove-mapping
managed object alpha_poc_agg deleted; also changed: alpha_poc_child.gparent (removed); alpha_poc_child.parent (removed)
managed object alpha_poc_child deleted
managed object alpha_poc_sink deleted
endpoint pocagg: 200
endpoint poctrace: 200
leftover POC types: alpha_poc_sink; config equals backup: false
repo.ds POC mappings left: managed/alpha_poc_agg,managed/alpha_poc_child; genericMapping equals backup: false
```

The script wrote `.state/managed-after.json` and `.state/repo.ds-after.json`
(gitignored) and stopped. Those files are that snapshot and still list
`alpha_poc_sink`. A name-only diff of the snapshot against
`.state/managed-before.json` and `.state/repo.ds-before.json` found one extra
managed object, `alpha_poc_sink`, and two extra `genericMapping` keys,
`managed/alpha_poc_agg` and `managed/alpha_poc_child`. No pre-existing object
body and no pre-existing mapping value differed.

Then:

```
$AIC --no-prompt managed object delete alpha_poc_sink
```

It printed `managed object alpha_poc_sink deleted` and exited 0.

A live read after that retry (same day, `GET /openidm/config/managed` and
`GET /openidm/config/repo.ds`, compared in process):

- `config/managed` equals `.state/managed-before.json`: the objects array, the
  order, and the whole document.
- `genericMapping` does not. Counts 28 against the backup's 26. Extra keys only
  `managed/alpha_poc_agg` (property `children`) and `managed/alpha_poc_child`
  (property `parent`). Each entry's subkeys are `dnTemplate`, `jsonAttribute`,
  `jsonQueryEqualityMatchingRule`, `objectClasses`, `properties`. No
  pre-existing mapping changed. Repo top-level keys and `resourceMapping`
  section names match the backup.
- `GET /openidm/config/endpoint/pocagg` and `.../poctrace` are both 404.

The two `true` lines the brief required were not both printed. The two
`genericMapping` entries were left in place. A second `node setup.mjs teardown`
was not run: the relationship delete would refuse again and would not clear
them.

## Open questions

Unmeasured:

- Whether revision-less `PATCH` has a retry bound above N = 200.
- `PUT` with `If-Match`. The no-revision half lost updates. The revision-bearing
  half was not run.
- A failed HTTP request's transaction, and a `412` transaction, under the E20
  trace. Every E20 call was 200.
- The unguarded self-patch loop, self-patch under concurrent callers, and a
  nested patch of a field the outer fold does not recompute (E15).
- The array size at which create or patch fails. 5000 succeeded (E18).
- A multi-operation patch whose net effect is empty, and whether a no-op is ever
  `304` (E16 returned 200).
- The recalculate inbox marker is what the installed hook does. It was not timed
  on its own.
- Whether `aic` will remove an object's last `repo.ds` mapping. The tool's
  refusal says that removal is unmeasured, which is why the two entries above
  are still on the tenant.

## Round 2

The harness is `e21.mjs`. Measured tables go in `RESULTS.md` under "E21 —
relationship notification, both ends" as each part finishes. This section is the
run log.

### What `relationship set` does with the leftover mapping

Read before install, in `src/managed/cli.rs` (`map_created_pair`) and
`src/managed/storage.rs` (`set_with`, `mode`). A new `relationship set` writes
`config/managed`, then maps the pair only when the property did not already
exist. When the property is new but `repo.ds` already has a matching mapping for
both ends, `mode` is `Mapped` and the command does not write `repo.ds`. It
checks the reference column for orphaned values and stops if it finds any. A
second or different mapping is not the path for this leftover. The install
compares `genericMapping` with `.state/run1/repo.ds-after.json` immediately
after `relationship set` and refuses to continue if they differ. It does not
write `config/repo.ds` itself, and it does not overwrite `managed-before.json`
or `repo.ds-before.json`.

The tenant does not declare `notifyRelationships` on the object.
`alpha_role.assignments` and `alpha_role.temporalConstraints` declare it on the
property that changes, naming `members`. `notify` on those types sits on
`resourceCollection`, and `notifySelf` sits on the relationship node. Round 1
put both flags on the node. E21 measures both placements. The object-level
placement the brief named is measured as well, because that is not the shape the
tenant uses.

### Install

`node setup.mjs preflight` on 2026-11-05: `managedEqualsBackup` true,
`genericMappingEqualsRun1After` true, no POC types present.
`genericMappingEqualsRun1Before` false, which is the two known orphans and
nothing else.

`node setup.mjs install` then printed (the `target tenant: sandbox` lines and
Node's `NO_COLOR` warning omitted):

```
managed object alpha_poc_agg created; undo it from the TUI history overlay
managed object alpha_poc_child created; undo it from the TUI history overlay
managed object alpha_poc_sink created; undo it from the TUI history overlay
round 1: present alpha_poc_agg,alpha_poc_child,alpha_poc_sink
alpha_poc_agg.children already has matching mappings; checking for orphaned values
mapping configuration verified; runtime storage mode is untested without links (run verify --probe later if appropriate)
relationship alpha_poc_agg.children set; undo it from the TUI history overlay
note: managed/alpha_poc_agg has no records to probe alpha_poc_agg.children through; wait a few seconds before writing through it — a write made before it is live is lost
note: managed/alpha_poc_child has no records to probe alpha_poc_child.parent through; wait a few seconds before writing through it — a write made before it is live is lost
relationship set left genericMapping equal to run1/repo.ds-after.json
schema+hooks: confirmed (attempt 1)
hooks live after 1 probe(s)
```

No `repo.ds` write. A direct parent `PATCH` and a direct child `PATCH` each ran
`onUpdate` and `postUpdate` on that record. A parent link add did the same on
the parent only.

### Table

`node e21.mjs table` (307 s) ran every round-1 write, plus a parent plain-field
`PATCH`, a parent delete, and an edge patch through the child, in both storage
modes and all three node-placement flag configs. The direct-PATCH control fired
on both ends in every config. Two rows waited 30 s and gained no further hooks.
The RESULTS section for this table is written with the rest of E21. One
disagreement with round 1: under `NOTIFY=1`, deleting the child did not run the
parent's hooks. `deletewait` repeats that delete with a 30 s wait, and once more
with the child's scripts removed. `peers` and `owned` now declare `status` on
the parent end, the same as `children`, so their edge patch is the same write.
The reverse schema does not declare it.
