# POC plan: atomic aggregates through IDM `PATCH` and `onUpdate`

**Audience:** an agent working in the `pingone-aic-manager` repo against its own
sandbox tenant. It needs no other context.

**Confidentiality:**

- Everything here uses throwaway, generic names.
- Do not add any client, product or business-domain names to the docs or probes.
- Follow `.ai/core.md` §3: sanitise evidence at capture, and run the
  sensitive-metadata scanner before committing.

## Why

A common IDM design keeps derived state on a "parent" managed object, worked out
from many "child" records. Examples are a status, a winner among competing
children, or a completion flag. Writers in several places add children at the
same time.

Two approaches are common. Both lead to hand-written race handling:

- Do the derivation in endpoint code, with read → compute → write and your own
  retries.
- Do it in hooks fired through relationship notifications. That brings
  re-entrant hook runs, stale plans and partial writes.

The hypothesis to test is that IDM's own `PATCH` already gives a
compare-and-swap with automatic retry. If so, putting the derived state on the
parent's **own plain fields** and computing it in `onUpdate` needs no locking or
retry code at all.

### Evidence so far (unverified on AIC)

These are hypotheses about `patchResource` and `update`, so treat every point
as a claim to measure.

1. A `PATCH` with **no revision**:
   - reads the record and applies the operations in memory;
   - runs `onUpdate` on the result;
   - writes to the repo **conditional on the revision it read**;
   - on `PreconditionFailedException`, re-reads and repeats everything,
     **including `onUpdate`**, with no visible retry bound.
2. A `PATCH` **with** a revision (`If-Match`) does not retry. A conflict gives
   `412`.
3. Relationship fields are stripped before the conditional write. They're
   persisted **afterwards** (`persistRelationships`), outside the swap.
   `postUpdate` runs after that, once per successful write.
4. `update` (PUT) passes the request's revision straight through. With no
   revision, it may be an unconditional last-writer-wins write.

### Existing verified docs to read first

- `docs/api/10-managed-objects.md`:
  - "Conditional link writes and `_rev`": on a generic relationship end,
    `If-Match` is ignored for link writes.
  - "Hook scripts", "Hook runtime bindings": hook bindings, and that an uncaught
    throw gives `500` and nothing is written.
  - Relationship storage (mapped vs generic).
- The `.ai/core.md` §8 note on creating types: config-only types don't work. Use
  `aic managed object create`.

## Setup (throwaway; delete it all at the end)

Create the throwaway types with `aic managed object create`, using
`--no-prompt`.

- **`alpha_poc_agg`** (the parent):
  - `name` (string).
  - `inbox` (array of objects): writers append here.
  - `entries` (array of objects): folded state.
  - `derived` (object): computed by the hook, e.g. count, a sum, the id of the
    "best" entry, and an `outcome` that becomes terminal once a threshold is
    reached.
  - `counter` (number).
  - `children`: a has-many relationship to `alpha_poc_child`, with
    `_refProperties` such as `status`.
    - Create one generic pair and, if `aic` supports it, one `repo.ds`-mapped
      pair, so both storage modes are covered.
- **`alpha_poc_child`**: `value` (number), `rank` (number), and the reverse of
  `children`.
- **`alpha_poc_sink`**: a records type for side-effect counting.

Hooks, written as TypeScript or inline JS through `aic script` and linted:

- **Parent `onUpdate`:**
  - logs a run id, e.g. `logger.info("POC onUpdate run " + <random>)`;
  - folds `inbox` into `entries`, recomputes `derived` from `entries` only, and
    clears `inbox`;
  - throws if `oldObject.derived.outcome` is terminal and the patch would change
    it.
- **Parent `postUpdate`:** creates one `alpha_poc_sink` record per run. It
  records the parent id, `_rev` and the outcome.

Load driver: a small script (Node or bash + `curl`) that:

- fires N concurrent HTTP requests with a bearer from `aic whoami --token`;
- records the status, latency and response `_rev` of each;
- uses a correlation id per request in a header, so logs can be fetched **by
  transaction id** (`aic logs tx <id>`) rather than by wide greps.

## Experiments

For each one, record:

- the N used and the exact operations;
- the result counts;
- whether derived state matches a recomputation from the final stored data;
- the number of `onUpdate` runs (from logs) vs the number of commits;
- latency percentiles.

| #   | Question                                                      | Method                                                                                               | Pass looks like                                                                                |
| --- | ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| E1  | Is a revision-less `PATCH` a CAS with auto-retry?             | N = 2, 10, 50 concurrent `PATCH add /inbox/-`                                                        | All N entries present; `derived` consistent; `onUpdate` runs ≥ N; no 412s or 5xx               |
| E2  | `PATCH` with a stale `If-Match`                               | Read `_rev`, patch twice with it                                                                     | Second call is `412`, nothing written                                                          |
| E3  | Revision-less `PUT` (`update`)                                | N concurrent full replaces built from a prior read                                                   | Expect lost updates. Document it as the anti-pattern                                           |
| E4  | Client-computed values lose data                              | N concurrent `PATCH replace /entries` (read, append locally, replace)                                | Lost entries even with the CAS: the read happened outside it                                   |
| E5  | Built-in atomic operations                                    | Concurrent `increment /counter` (if supported); `add /x/-` vs `add /x/<index>`                       | Exact final count                                                                              |
| E6  | Do retries duplicate `onUpdate` side effects?                 | `onUpdate` also creates a sink record                                                                | Sink count > commits under contention, so side effects in `onUpdate` are unsafe                |
| E7  | Does `postUpdate` run exactly once per commit?                | Count sink records from `postUpdate` vs distinct committed `_rev`s                                   | Equal                                                                                          |
| E8  | What does the caller see if `postUpdate` throws?              | Make it throw                                                                                        | Status code? Is the parent change kept (expected yes)?                                         |
| E9  | Does a hook throw act as a guard?                             | Patch a terminal parent                                                                              | `500`/`4xx`, nothing persisted, error text visible to caller?                                  |
| E10 | Are relationship writes inside the CAS?                       | N concurrent `PATCH add /children/-` while `onUpdate` derives from `children` (generic, then mapped) | Expect `derived` to be stale/inconsistent on generic. Measure mapped                           |
| E11 | Do edge `_refProperties` writes go through the parent CAS?    | Concurrent edge property patches + parent `onUpdate` that reads them                                 | Expect not. Characterise notification-driven re-entry (with and without `notifySelf`/`notify`) |
| E12 | Do reads of other records in `onUpdate` see a stale snapshot? | `onUpdate` reads child records; mutate a child concurrently                                          | Parent isn't recomputed when a child changes later                                             |
| E13 | Is a `PATCH` from inside a custom endpoint the same as REST?  | Endpoint calls `openidm.patch(path, null, ops)` concurrently                                         | Same as E1; caller identity kept                                                               |
| E14 | Is there a retry bound under heavy contention?                | N = 100–200 on one parent                                                                            | Any failures/timeouts? Latency curve; where it breaks                                          |
| E15 | What does self-patching from the parent's own hook do?        | `onUpdate`/`postUpdate` calls `openidm.patch` on its own record                                      | Recursion/deadlock/error? Expect a "don't"                                                     |
| E16 | Does `onUpdate` fire on a no-op patch?                        | Patch with an identical value                                                                        | Code says it short-circuits. Confirm                                                           |
| E17 | How do `onUpdate` and `postUpdate` see the patch?             | Log `request.patchOperations` shape and `oldObject`/`object` on retry                                | Does a retry's `oldObject` reflect the newer revision?                                         |
| E18 | How does array size affect cost?                              | Parent with 10 / 100 / 1000 entries                                                                  | Latency and size limits; pick a sensible cap                                                   |
| E19 | Can `onUpdate` write the audit trail in memory?               | See "Audit trail" below                                                                              | One audit entry per committed change, none duplicated by retries; far fewer round trips        |

### Audit trail (E19, added 2026-11-05)

The motivating project has complex objects with audit trails, and patches the
audit trail as it goes, so one logical update becomes many `PATCH` calls. If
changes are pushed through a root object, its `onUpdate` can derive the audit
entries from `oldObject` → `object` and append them to `object.audit` in memory,
in the same write. Measure:

- **Cost:** k changes to nested data on the root as (a) k data `PATCH`es plus k
  audit `PATCH`es (today's shape), (b) k `PATCH`es with the audit written by
  `onUpdate`, (c) one `PATCH` carrying k operations with the audit written by
  `onUpdate`. Round trips and wall-clock for k = 1, 10, 50.
- **Correctness under contention:** N concurrent writers change the root. The
  audit must hold exactly one entry per committed change, and form a chain:
  every entry's `from` equals the previous entry's `to` for that path. A retry
  that leaked a stale `oldObject` would break the chain or duplicate an entry.
- **Shape:** what `onUpdate` sees of nested changes, whether
  `request.patchOperations` or the `oldObject`/`object` diff is the better
  source, and the cost of a large `audit` array (ties to E18).

Run each concurrency experiment at least three times, and at least once through
a custom endpoint as well as raw REST.

## Pattern to confirm or refute

The "inbox + fold" aggregate:

1. Writers create children (if needed). They then **only append** to the parent:
   `PATCH add /inbox/-` with no `If-Match`, carrying the child id and any data
   the derivation needs. Never send client-computed derived fields.
2. Parent `onUpdate` is **pure on `object`**:
   - folds the inbox, recomputes derived fields and enforces invariants (throws
     to refuse);
   - makes no writes to other records;
   - is safe to re-run.
3. Parent `postUpdate` does cross-record side effects, and makes them
   idempotent: keyed on the new `_rev` or on a state transition (`oldObject` vs
   `object`).
4. Relationships are plain links with no lifecycle data. Anything the derivation
   needs lives in the parent's own fields.
5. Children are immutable once appended. A change is a new inbox entry.

Things to state explicitly in the result:

- what this pattern does **not** protect: relationships, edge properties, other
  records, and PUT;
- what replaces a "recalculate" admin action, e.g. a `PATCH add /inbox/-` with a
  `{recalculate: true}` marker.

## Deliverables (in `pingone-aic-manager`)

1. **`docs/api/10-managed-objects.md`**: a new section "Concurrent `PATCH` and
   hooks (verified <date>)". It holds the measured tables for E1–E18 in the
   existing style: what was measured, on which storage mode, and what is still
   unmeasured. Follow §2: only live-verified claims, dated.
2. **`docs/api/99-quirks-and-open-questions.md`**: dated notes where AIC differs
   from the hypotheses above.
3. **`docs/managed-object-aggregates.md`** (new), a best-practice guide:
   - the pattern above, with do/don't lists;
   - where each kind of logic belongs (`onUpdate` vs `postUpdate` vs endpoint);
   - the retry and side-effect rules, and how to test it offline (a test double
     that simulates the CAS retry by running `onUpdate` twice);
   - migration notes for moving state off relationship edges onto the parent;
   - a short decision table: "when the aggregate pattern is enough / when you
     still need explicit concurrency control".
4. **`pocs/managed-aggregate-cas/`**: the load driver, hook sources and a README
   for re-running the probes. No tenant identifiers.
5. If useful, a suggestion list for `aic` features, e.g. a
   `aic managed probe concurrency` verb or lint rules for hooks that write to
   other records from `onUpdate`.

## Cleanup and safety

- Use a sandbox tenant only. Never touch other types, or their hooks, or
  `config/access`, beyond what the POC needs.
- Make one backup before any config write, and confirm every write by re-reading
  it.
- At the end:
  - delete all POC records, the three types and any POC endpoint;
  - confirm the managed config matches the pre-POC backup apart from the removed
    types;
  - check no hook on any other type changed.
- Never log or store `context` (it carries the caller's bearer token).
