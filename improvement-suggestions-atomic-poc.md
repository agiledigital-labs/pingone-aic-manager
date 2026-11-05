# `aic` suggestions from the managed-aggregate CAS POC

Each one was checked against the code in this checkout on 2026-11-05. The
measurements they cite are in `pocs/managed-aggregate-cas/RESULTS.md` and
`docs/api/10-managed-objects.md`.

## Re-verify earlier managed objects after a later create

**Evidence.** On 2026-11-05, during install, three sequential
`aic managed object create` calls each confirmed, and the second type was gone
after the third. That sequence was not re-run in the experiment pass. The code
explains how a confirmed create can still vanish. `create_object_request`
(`src/managed/ops.rs`) asks `replace_managed_confirmed` for one condition,
`ObjectPresent` of the new name. That function (`src/managed/api.rs`) retries
the `PUT` until those conditions hold. It does not ask whether an object an
earlier command in the same session created is still in the document. A later
create whose read is missing that object writes the gap back and returns
success. This is Q14 (`docs/api/99-quirks-and-open-questions.md`), and the
confirm-after-write added for it does not cover siblings. The POC installer
works around it by reading until all three names are present on three
consecutive reads (`pocs/managed-aggregate-cas/setup.mjs`).

**Sketch.** Remember the object names this process has created. On the next
`config/managed` write, refuse if any of them is absent from the pre-write read,
and say which name disappeared. A multi-name `aic managed object create` that
adds every name in one `PUT` would remove the window between them.

## `managed field add` cannot create an object field

**Evidence.** `ScalarFieldType` (`src/managed/spec.rs`) is `String`, `Boolean`,
`Number`, `StringArray`. `scalar_type` (`src/managed/cli.rs`) rejects anything
else with `--type must be string, boolean, number, or string[]`. The POC needed
`object` fields (`derived`, `data`) and arrays of objects (`inbox`, `entries`,
`audit`, `items`), and wrote `config/managed` directly.

**Sketch.** Accept `--type object` and `--type object[]`, emitting
`{"type":"object"}` and `{"type":"array","items":{"type":"object"}}` through the
same `apply_*` transforms. Leave nested property declarations for a later flag;
the POC only needed the open object and the array of objects.

## `relationship set` always maps the pair into `repo.ds`

**Evidence.** `RelationshipCommand::Set` (`src/managed/cli.rs`) has no
`--generic`. After the schema write, `map_created_pair` plans a `repo.ds`
addition whenever `storage::mode` is generic, and applies it. The comment on
that function says a failure before the `repo.ds` write leaves the generic
declaration for `relationship map` to finish, which is the other direction. The
POC's generic pair (`gchildren`) was declared with a `config/managed` write,
which does not touch `repo.ds`.

**Sketch.** `--generic` on `relationship set` skips `map_created_pair` and
prints that the pair stays generic. The default stays "map it", which is what
every current caller wants.

## `relationship delete --remove-mapping` refuses a pair that is an object's last mapping

**Evidence.** Teardown on 2026-11-05 ran
`aic managed relationship delete alpha_poc_agg.children --remove-mapping`. The
command wrote nothing. The error was: refusing to repair
`alpha_poc_agg.children` because `config/repo.ds` reads through it from
`alpha_poc_child.parent`, and refusing to repair that reverse end because it is
the last mapped property of `managed/alpha_poc_child`. Removing a mapping
another one reads through, and removing an object's last mapping, are both
called unmeasured. That text is `plan_sequence` in `src/managed/repair.rs` (the
dependents check, then `entries.len() == 1`), returned by `src/managed/cli.rs`
before any write. The planner sees a stale mapping because it judges the managed
document the delete would write, which has already dropped both ends; the live
objects were still present when the command returned. The following
`object delete` calls removed the three types. `ObjectCommand::Delete` only
`PUT`s `config/managed` and confirms `ObjectAbsent` for that one name. After a
second delete of `alpha_poc_sink` (the first confirm did not stick; see the done
note), `config/managed` matches the pre-POC backup and `genericMapping` still
has `managed/alpha_poc_agg` (property `children`) and `managed/alpha_poc_child`
(property `parent`).

**Sketch.** When both ends of a pair are being removed and each `genericMapping`
entry's only properties are those ends, remove both properties and drop the two
per-type entries in one `repo.ds` write. That is the case the last-mapping
refusal exists to avoid doing one side at a time. `object delete` of a type
whose `genericMapping` properties are only relationships this delete is removing
can drop that entry in the same operation and confirm the key is gone. A mapping
another live object still reads through stays refused.

## No way to set `notify` or `notifySelf`

**Evidence.** `FieldAttrs` and `RelationshipCommand::Set` expose title,
description, required, searchable, viewable, user-editable, enum and default.
They do not mention `notify` or `notifySelf`. An existing value is preserved as
an unmodelled key — `a_cardinality_flip_places_unmodelled_keys_by_level` in
`src/managed/ops.rs` keeps `notifySelf` across a cardinality change — so a save
does not wipe a flag the console set. Nothing in the CLI turns one on. The POC
set both with a direct `config/managed` read-modify-write (`setup.mjs`,
`NOTIFY=1`). What the flags actually fire is measured in
`docs/api/10-managed-objects.md` ("Relationship writes sit outside the parent
swap").

**Sketch.** `--notify` and `--notify-self` on `relationship set`, each a
tri-state: absent leaves the current value, `true` and `false` write it. Put
them on the relationship node (`notifySelf`) and on the resource collection
(`notify`), which is where the preserve test already finds them.

## `logs tx` cannot see script logs, and has no write summary

**Evidence.** `logger.info` from a managed hook reaches `idm-core` as
`text/plain` with no transaction id, so `aic logs tx` does not return it.
`context.transactionId.transactionId.value` is `<root>/0`, and reads
(`openidm.read`, `openidm.query`) produce no event in `idm-access`,
`idm-activity`, `idm-core`, `idm-sync`, `idm-config` or `idm-authentication`.
Writes do: under contention each request had exactly one parent `PATCH` in
`idm-activity` and one sink `CREATE` per `onUpdate` run plus the `postUpdate`
create, and a lost compare-and-swap attempt left no parent write. `trace.mjs` in
the POC shells out to `aic logs tx --output` and counts those events, because
the CLI prints the raw array. A `logs grep` of `idm-core` on the sandbox scanned
about 80,000 events in 171 s, which is the only way to find a logger line even
after the hook has tagged it with the transaction id.

**Sketch.** `aic logs tx <id> --summary` prints, per stored transaction id, the
write count by operation and object type, the object ids, and the access event's
status and latency, from `idm-activity` and `idm-access`. Say in the help that
reads are absent and that a hook counts them by logging
`context.transactionId.transactionId.value`. That makes `trace.mjs` a flag.

## Warn when a transaction-id query is a prefix of another returned id

**Evidence.** `docs/api/08-logs.md` ("Transaction id matching is a prefix
match"): querying `<stem>-1` also returns `<stem>-10`, and nothing in the output
says the set is wider than the id asked for. `LogsCommand::Tx`
(`src/logs/cli.rs`) fetches and prints. It notes a missing completion event. It
does not compare the returned `payload.transactionId` values to each other. The
POC had to zero-pad (`<stem>-00` … `<stem>-09`) and then check that a query for
`<stem>-03` returned only that request.

**Sketch.** After the fetch, if any stored id is a strict prefix of another
stored id, or the queried string is a strict prefix of a stored id that is not
`<query>` or `<query>/…` only, print a one-line note on stderr naming the two
ids. Do not drop events. The note is what stops a padded-wrong script trusting a
silent superset.

## Lint managed hooks for writes, throws, and index paths

**Evidence.** The managed-hook block in
`src/scripts/templates/idm/eslint.config.js` (`managed/**/*.cjs`) adds the hook
globals and no further rules. The POC measured three shapes that look like
ordinary script and lose data:

- `openidm.create` / `patch` / `update` / `delete` inside `onUpdate` runs once
  per attempt. 50 commits produced 836 sink records with the trace sink on, and
  a self-`patch` from `onUpdate` committed a gap state before the outer attempt
  retried (E6, E15).
- `throw` in `postUpdate` returns the error and leaves the parent write stored
  (E8). A client that retries on 500 appends twice.
- A position in a patch path (`/items/0`) is refused, or applies to whichever
  element sits there on the retry (E5).

**Sketch.** Three `no-restricted-syntax` rules on `managed/**/*.cjs`, switched
by filename. A hook's file is `idm/managed/<object>/<hook>.cjs` (`onUpdate.cjs`,
`postUpdate.cjs`), which is the path `managed_hooks.rs` assigns:

- In `onUpdate`, a call to `openidm.create`, `openidm.patch`, `openidm.update`
  or `openidm.delete` is an error. The message says the hook body runs again for
  every lost attempt.
- In `postUpdate`, a `ThrowStatement` is an error. The message says the write
  has already committed.
- In either, a string literal patch field matching `/\/[0-9]+($|\/)/` is a
  warning. The message points at `/-`, `increment`, and remove-by-value.

## `aic managed probe concurrency`

**Evidence.** The whole POC is a Node driver beside the tool
(`pocs/managed-aggregate-cas/experiments.mjs`) because nothing in `aic` will
fire N revision-less patches at one record and report lost updates. The result
that driver exists to produce — all N entries present, or how many a `PUT` kept
— is the check a migration onto this pattern needs.

**Sketch.**
`aic managed probe concurrency <object> <id> --n 50 --op add --field /inbox/-`,
sandbox-only, `--no-prompt` friendly. It sends N concurrent revision-less
`PATCH`es with distinct values, prints status counts, latency percentiles, and
how many of the N values the following read contains. No hook required for the
lost-update check. Optional `--trace` is the caller's problem; the command does
not install a sink.
