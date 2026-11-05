# Aggregates on a managed object

How to keep derived state and an audit trail on an IDM managed object when
several writers update it at once, without hand-written locks or retry loops.

Measured on a sandbox tenant on 2026-11-05 with throwaway types. The per-run
tables are in `pocs/managed-aggregate-cas/RESULTS.md`. The API-shaped write-up
is in `docs/api/10-managed-objects.md` ("Concurrent `PATCH` and hooks"). This
page is the pattern to copy.

## The result in one paragraph

A `PATCH` with no `If-Match` is a compare-and-swap. IDM reads the record,
applies the operations, runs `onUpdate`, and writes only if the revision it read
is still current. If another writer committed in between, it re-reads and does
the whole thing again, including `onUpdate`. Put the facts on the parent's own
fields and compute the derived fields in `onUpdate`. You do not write a retry
loop. A `PATCH` that sends `If-Match` does not retry: a stale revision is `412`
and nothing is stored.

The same call through `openidm.patch(path, null, ops)` from a custom endpoint
behaves the same way, and the activity log records the same caller.

## Inbox and fold

Writers never send a derived value. They append a fact:

```json
[
  {
    "operation": "add",
    "field": "/inbox/-",
    "value": { "id": "event-1", "value": 4 }
  }
]
```

No `If-Match`. The parent's `onUpdate` is a pure function of `object`:

1. Fold `inbox` into `entries`.
2. Recompute `derived` from `entries` only.
3. Clear `inbox`.
4. Throw to refuse a change that breaks an invariant.

Throwing from `onUpdate` aborts the write. The caller sees the `code` you threw
as the HTTP status (`409` with the thrown message, in the run that tested a
terminal outcome). Under contention the guard runs on the state that would be
committed, so "the first five win" holds with no lock: exactly five of twenty
concurrent appends were stored, and the rest got `409`.

`onUpdate` will run more than once per commit whenever writers collide. At 50
concurrent appends it ran 667–836 times for 50 commits. Anything it does besides
change `object` happens once per attempt. Create no other records there, and
call no `openidm.create`, `patch`, `update`, or `delete`.

A hook that patches its own record from `onUpdate` races the attempt it is
inside. In the run that did this, the nested patch committed a gap state (the
counter already moved, the new entry not stored yet) and the outer attempt
retried into the right final state. Do not do it.

## Audit trail in memory

If one logical edit currently takes a data `PATCH` plus an audit `PATCH`, stop
patching the trail from the client. Send the data changes. In `onUpdate`, diff
`oldObject` against `object` and append the audit entries to `object.audit`
before the write returns.

What that costs, one writer, k changes to nested fields:

| k   | data `PATCH` plus audit `PATCH` each | one `PATCH` per change, hook writes the trail | one `PATCH` carrying all k changes |
| --- | ------------------------------------ | --------------------------------------------- | ---------------------------------- |
| 1   | 2 calls, 237 ms                      | 1 call, 41 ms                                 | 1 call, 41 ms                      |
| 10  | 20 calls, 819 ms                     | 10 calls, 443 ms                              | 1 call, 40 ms                      |
| 50  | 100 calls, 4245 ms                   | 50 calls, 2188 ms                             | 1 call, 49 ms                      |

Batching is the win. Fifty changes in one `PATCH` took 49 ms; the
patch-per-change shape took 4.2 s.

Under contention the trail stays exact, because a retried attempt's entries
exist only on the `object` that is thrown away. Fifty writers at once, three
changes each, three runs: 150 audit entries every time, one run id per commit,
the shared field's `from`/`to` chain unbroken, while `onUpdate` ran 747–814
times. Diff `oldObject` and `object` for the effect. Read
`request.patchOperations` when you need the intent, including a write that sets
a leaf to the value it already had (the diff records nothing).

The array the hook rewrites is the cost. A parent that already held 5000 entries
still accepted concurrent appends, all of them `200`, with `derived` still
consistent, at p50 4017 ms for ten writers. A sequential append on that parent
was p50 315 ms, against about 40 ms at 100 entries. Keep the folded array well
under 1000 entries if writers are concurrent, and plan to archive an audit trail
that grows without bound.

## Where each kind of logic goes

| Logic                                     | Where                                     | Why                                                                                |
| ----------------------------------------- | ----------------------------------------- | ---------------------------------------------------------------------------------- |
| Derive fields, enforce invariants         | `onUpdate`, by assigning `object`         | Runs inside the swap, on every attempt. A throw rolls the attempt back.            |
| Create or update another record, call out | `postUpdate`                              | Runs after the commit, once per committed revision.                                |
| A write that is really a new fact         | the client or an endpoint, as one `PATCH` | The hook should not be the place that discovers the fact by reading other records. |

`postUpdate` is the wrong place to signal failure. Throwing
`{ code: 500, message: "…" }` returned that 500 to the caller and left the
parent change stored. A client that treats 500 as "not written" and retries
appends a second time.

Make the side effect idempotent on the new `_rev`, or on a transition you can
see by comparing `oldObject` and `object` (`outcome` moved from `open` to
`closed`). A repeated `postUpdate` for the same revision should notice it has
already run. The measured hook created one sink per commit and the distinct
revisions matched the commits, including at 200 concurrent writers.

## Make a retried write idempotent

IDM's own retry does not double-apply a committed append: the attempt that lost
is discarded, and the retry runs against the new revision. The duplicate comes
from the client. The `postUpdate` 500 above is the case that bites, and so is
any at-least-once sender.

Give every inbox entry an id the fold has seen before, and skip an id already in
`entries`. The same rule applies to audit entries if a client can submit the
same change twice.

## Operations that stay correct when the retry runs

The retry applies the same operations again, to a newer record. An operation
that names a position means something else the second time.

| Operation                                                              | What happened                                                                  |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `add /inbox/-`                                                         | Appends. Concurrent writers all landed.                                        |
| `increment /counter`                                                   | Exact. 50 concurrent increments of 1 left the counter at 50.                   |
| `remove /items` with a `value`                                         | Removes every element equal to that value.                                     |
| `add /items/0` on an absent array                                      | `403` "Failed policy validation": the index made an object, not an array.      |
| `add /items/0` or `remove /items/0` on an existing array               | `400` "Position-based operation is not allowed".                               |
| `replace /items/0/w`                                                   | `200`, and it names whatever element is at index 0 when that attempt runs.     |
| `replace /items/5/w` past the end                                      | `500`, empty body.                                                             |
| `replace /entries` with an array the client built from an earlier read | `200`, and the entries the client had not seen are gone.                       |
| `PUT` of a whole record built from an earlier read, no revision        | `200`, last writer wins. At N = 20, one or two of the twenty entries survived. |

`/-`, `increment`, and remove-by-value are the position-free primitives. Do not
send an index. Do not `PUT` the record. Do not send a derived field the client
computed.

A `replace` of a field with the value it already holds returns `200`, does not
move `_rev`, and does not run `onUpdate`. A no-op patch cannot force a
recompute. To recompute on purpose, append an inbox entry the fold recognises
and does not store, for example `{ "id": "recalculate", "recalculate": true }`.
The installed probe hook skips those entries and still recomputes. That marker
was not given its own timed run.

## What this does not protect

The swap covers the parent's own fields. It does not cover:

- **Relationships.** Concurrent `PATCH add /<rel>/-` stored every link, mapped
  and generic, and `onUpdate` derived from the relationship on that same request
  saw a stale set. The last writer's link was never counted, because the hook
  runs before the link is persisted. Which of the other links were visible
  changed from run to run. `derived` was wrong on every run.
- **Edge properties.** With no notification flags, `PATCH` of
  `<parent>/<rel>/<edgeId>` changing `_refProperties` does not run the parent's
  hooks. On a mapped pair the parent's `_rev` moves anyway, because the link
  lives on the parent's entry, so an `If-Match` on the parent collides with it.
  On a generic pair the parent's `_rev` stays put.
- **Notification flags.** Where the flag sits decides who hears the write.
  `notifySelf` on the relationship node runs the origin object's hooks for a
  write through that relationship's own URL. `notify` on the relationship node
  did not notify the other side. `notify` on `resourceCollection` did: a
  child-side link change and a `DELETE` of the child record then ran the
  parent's hooks, and a parent-side relationship write ran the child's. A plain
  field change crossed neither way. None of these adds a second, post-persist
  run to the parent's own link add. A derivation that has to notice a child's
  field edit does not get it from these flags. It does if the changed field
  names the relationship in `notifyRelationships`, which ran the other side's
  hooks with no `notify` flag set. See `docs/api/10-managed-objects.md`.
- **Other records.** `onUpdate` can read them. With no `notifyRelationships`
  on the field that changed, the parent is not recomputed when a child changes
  later. A child `value` patched from 5 to 50 left the parent's `derived.sum`
  at 5, in both storage modes.
- **The client's snapshot.** `PUT`, and `PATCH replace` of an array the client
  assembled, lose updates even though each call returns `200`. `onUpdate` can
  recompute from what was stored. It cannot restore what the client never sent.

Anything the derivation needs has to be copied onto the parent at the moment of
the write. A relationship is a plain link. A child is immutable after its fact
has been appended; a later change is a new inbox entry.

## Measuring a change

`idm-activity` records every write the request caused, under the transaction id
you send in `X-ForgeRock-TransactionId`, stored as `<root>/0`:

- the request's own `PATCH` of the parent, once, even when `onUpdate` ran many
  times (a lost attempt leaves no parent write);
- every `openidm.create` or `patch` a hook performed, including creates from
  `onUpdate` attempts that did not commit.

Ten concurrent appends, each with its own zero-padded id under one stem
(`<stem>-00` … `<stem>-09`, because matching is a prefix match and `<stem>-1`
would also return `<stem>-10`): 10 parent `PATCH` events, and 65 sink `CREATE`s,
equal to 55 `onUpdate` runs plus 10 `postUpdate` runs. The access event is the
outer call (`status`, `statusCode`, `elapsedTime`). A script's `openidm.patch`
has no access event of its own.

Reads are absent from `idm-access`, `idm-activity`, and the other IDM sources
that were queried. Script `logger` lines reach `idm-core` as plain text with no
transaction id, so `aic logs tx` does not return them. A hook that needs a read
counted reads its own id and writes the count down:

```javascript
var tx = context.transactionId.transactionId.value;
```

Log `tx` next to the path it read, or, better, persist a counter. Finding a
logger line afterwards is a time-window scan of `idm-core` (about 80,000 events
in 171 s on the sandbox that was measured).

`pocs/managed-aggregate-cas/trace.mjs` prints the write summary for an id or a
stem:

```sh
node trace.mjs "<txid-or-stem>"
```

It says at the top that reads are not in the sources it fetches.

## Testing the hook offline

The property to test is that running `onUpdate` twice is safe, and that a second
attempt sees the base it would actually commit against.

1. Start from a fixture base.
2. Apply the patch operations in memory.
3. Run `onUpdate` with that `object`, the original as `oldObject`, and the
   operations on `request`.
4. Run it again the same way from a fresh copy of the base. The result should
   match the first result. A hook that appended to a sink on the first run has
   already failed this test.
5. Then simulate a lost attempt: commit a different writer's result, apply the
   same operations to that newer base, and run `onUpdate` with the newer
   `oldObject`. The fold must keep both writers' entries, and the audit chain
   must connect.

A retry's `oldObject` in the live tenant was always a newer revision than the
previous attempt's. Zero retries in the traced runs reused a base. A test double
that feeds the same `oldObject` to the second run is testing a case the tenant
did not produce.

## Moving state off relationship edges

Today the lifecycle often sits on `_refProperties` (a status, a winner, a
completion flag) and several hooks recompute the parent when a notification
fires. That recompute sees a stale or partial set, and it does not run for the
writes that actually change the child.

Move it like this:

1. Add the parent's own fields: an inbox (or the nested data plus an audit
   array) and the derived object. Leave the relationship in place as a link with
   no lifecycle properties.
2. At the point that used to patch the edge, also append an inbox entry that
   carries every fact the derivation used to read off the child or the edge.
3. Change `onUpdate` to derive from those fields only. Delete the relationship
   query from the hook.
4. Move cross-record work to `postUpdate`, keyed so a retry of the commit does
   not repeat it.
5. Existing edges: one backfill `PATCH` per parent that appends an inbox entry
   per current child, built from a query you run once, outside the hook. After
   that, child edits go through the inbox. A child edited some other way will
   not move the parent; that is the same gap the notifications already have, and
   the backfill is the moment to find those writers.

`notify` and `notifySelf` are not a substitute for the move. With the flags set,
the parent hook still does not see a link add that has not been persisted, and
it still does not see a later change to the child's own fields.

## When this is enough

| Situation                                                                                         | Verdict                                                                                                                                            |
| ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Several writers append facts, and the derived fields are a function of the parent's own fields    | The inbox-and-fold pattern is enough. No lock, no client retry loop.                                                                               |
| One logical edit must also record an audit trail                                                  | One `PATCH` of the data. `onUpdate` diffs and appends the trail.                                                                                   |
| An illegal transition must be refused                                                             | Throw from `onUpdate`. The write does not land.                                                                                                    |
| A commit must create or update another record                                                     | `postUpdate`, idempotent, and it must not throw.                                                                                                   |
| The derived value depends on a relationship, an edge property, or another record's current fields | This pattern is the wrong place. The hook will store a stale answer. Copy the facts onto the parent, or accept a separately reconciled projection. |
| Writers `PUT` the record, or patch an array they computed from a read                             | Lost updates, each call `200`. Change the writers to position-free operations.                                                                     |
| You need the number of reads a change performed                                                   | The platform log will not tell you. The hook records them under its transaction id.                                                                |
