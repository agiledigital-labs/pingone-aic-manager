# Relationship storage

IDM relationships have two storage modes. In **generic** mode neither end
has a relationship entry in `config/repo.ds`. Links appear to live in IDM's
generic relationships table; that location is inferred because the service
account cannot read the table. In **mapped** mode a `reference` entry stores
links in a numbered `fr-idm-reference-<n>` column on the forward record, and a
`reverseReference` reads through it from the reverse end. A one-way
relationship needs only the reference. Both modes resolve links, provided the
pair's declarations and mappings agree. See the measured
[storage contract](api/10-managed-objects.md#relationship-storage-and-repods-mappings).

## Why saving in the console can hide links

The console's measured save added a forward reference and reverse reference.
Existing generic links then disappeared from reads through both ends. Removing
the mapping made them reappear; links written while mapped became hidden
instead. The switch chooses a store; it does not migrate or delete the links.
A pair with just one end mapped can read and write through different stores.
The [conversion measurements](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01)
explain both effects. Do not save a linked generic relationship in the console
as a repair.

## What aic creates

`aic managed relationship set` creates a mapped pair by first writing both
schema ends to `config/managed`, then adding the mapping entries in one
whole-document `config/repo.ds?waitForCompletion=true` PUT. The intermediate
state is a consistent generic pair. Editing an existing relationship preserves
its storage mode. There is no `--generic` opt-out.

The end named on the command line holds the reference. Which end the console
would choose when saving from the other side is unmeasured. The reverse's
`isMultiValued`, like the forward's, follows its own schema: an array is true.
For a same-object pair, both distinct keys are added to one per-type entry,
using one reference column; a key naming itself as its reverse is refused.
Types without a per-type mapping receive the
[measured per-type entry](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01)
in the same PUT. Existing per-type metadata is preserved.

If mapping fails before its PUT, the managed declaration remains a consistent
generic pair unless another writer changed it. Finish with `relationship map`
before writing links. A failed PUT response or failed read-back leaves storage
uncertain: run `relationship verify` first, then map only if the pair is still
generic and empty. No automatic rollback or mapping undo is attempted. The
printed `repo.ds` backup preserves the document before the mapping write;
schema undo can leave mappings behind, just as relationship delete does.

New schema ends still receive the existing activation wait. Use `set --check`
if a script will write links next: it fails unless every end was confirmed
live. No-record types cannot be probed and remain unverified. The mapping
operation itself writes no records or test links. Its read-only `verify` checks
stored configuration and samples links, but **cannot confirm runtime storage
mode without a link**. After activation, an operator can separately use
`relationship verify --probe` where appropriate; it writes and removes a test
link on existing records and refuses required ends.

## Map a link-free generic pair

Pause every writer of links on this pair for the entire operation, including
console users, jobs and applications. The service offers no atomic lock across
link reads and a config write. aic scans all records on both ends, using
`_fields=_id,<key>` and cursor paging; it counts distinct edge IDs and refuses
malformed responses. It repeats the scan before the PUT. This also detects
one-sided links, but cannot prevent a writer racing after the scan. A `pr`
query cannot serve as the generic-link census: it misses generic links. See
[querying relationships](api/10-managed-objects.md#querying-a-relationship-collection-verified-2026-10-31).

```sh
aic managed relationship map custom_widget.owner --dry-run
aic managed relationship map custom_widget.owner --force
```

The plan prints the column, both entries, and each per-type entry being added.
The command refuses mapped pairs, mixed pairs, undeclared objects, incomplete
or nonreciprocal schemas, multiple targets, linked pairs (with their count),
and exhausted columns. Fix the named problem before retrying. An already
mapped pair can be checked with `relationship verify`.

Without `--force`, a terminal confirmation is required. Noninteractive use
needs `--force`; production-themed tenants also require `--yes`. `--dry-run`
has no write permit. After consent aic rechecks both documents, refuses drift,
backs up the current `repo.ds`, sends one PUT, and confirms by re-read. It then
queries the forward type with `_queryFilter=/<key> pr` to detect exposed
orphaned values and runs the existing read-only relationship verification.
`--json` includes the plan, write confirmation and verification result.

## Ten columns per object and orphaned values

Only `fr-idm-reference-1` through `-10` work. A PUT naming `-11` was accepted,
but link writes failed with `objectClassViolation`. Columns belong to each
object type, not the tenant. aic accounts for all property mappings on that
object, including `explicitMapping`, stale entries and scalar mappings. It
prefers max-in-use + 1; above 10 it takes the lowest free column. If all ten
are occupied it refuses and names their holders. Reverse references consume
no column on the reverse object.

Ping warns that reusing a deleted relationship's column can expose orphaned
values. After installing a mapping, aic requires the forward presence query
to return no records. Any match is reported as **ORPHANED DATA**, with the
column and a record sample, and exits non-zero. A failed query also exits
non-zero. The mapping stays installed and no records are cleaned: this keeps
the evidence inspectable and avoids overwriting concurrent config with an
automatic compensating PUT. Keep writers paused, inspect the data and backup,
and arrange recovery before using the pair. The
[column measurements and Ping warning](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01)
are the evidence; reuse itself has not been measured.

## Why aic writes repo.ds directly

Ping says to manage relationships only through the console or Schema API,
and warns other methods may cause data to de-sync. The measured console
Schema API route returns 403 to the service account. The measured direct
whole-document repo.ds PUT returns 200, re-reads identically, and new links
resolve from both ends with runtime checks confirming mapped storage. This
is the reason for the direct route, not a claim of vendor support. aic uses
that measured shape with backups and read-back checks; see the
[API evidence and warning](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01).

## Converting a pair that has links — not yet supported (slice 2)

`relationship map` refuses a pair with links because the switch would hide
them. Converting it requires durable capture **before** switching, recreation
in the mapped store, and verification. This slice implements none of those
migration steps. Keep linked pairs generic until that workflow is available;
`relationship relink` repairs one-sided links in the current store and is not
a storage migration tool.

## Promotion

The operator states that promotion carries `repo.ds`; the API investigation
measured a dry-run report that did **not** name its mapping change, not an
actual promotion proving that statement. Links are records, never promoted
configuration. It follows that promoting a generic-to-mapped conversion is
expected to hide pre-existing generic links in each upper environment, even
if the pair was empty in the source. That consequence is inferred from the
measured storage switch, not observed through a real promotion. See
[Promotion in the conversion evidence](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01).

Before promoting mappings, assess the pair independently in every destination.
A linked destination needs capture before promotion and restore afterwards;
that split workflow belongs to slice 2 and is not yet supported. Do not use a
source environment's empty-pair check as permission to convert a linked upper
environment.
