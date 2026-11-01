# Relationship storage and conversion

IDM relationships have two storage modes. In **generic** mode neither end
has a relationship mapping in `config/repo.ds`; the links appear to live in
IDM's generic relationships table (the location is inferred, because the
service account cannot read that table). In **mapped** mode a `reference`
stores links in a numbered `fr-idm-reference-<n>` column on the forward
record, and a `reverseReference` reads through it from the other end. A one-way
relationship has only the reference.

Saving a generic relationship in the console added mappings in the measured
case and hid its existing links. Removing those mappings made the generic
links visible again; links created while mapped became hidden instead. The
switch chooses a store; it does not move or delete links. A pair with one end
mapped and the other generic can read and write through different stores.
See the [storage contract](api/10-managed-objects.md#relationship-storage-and-repods-mappings)
and [conversion measurements](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01).

## Before you start

Use tenant names from your local `aic` configuration. The examples use `dev`,
`uat` and `prod`; substitute your names. Keep every application, job, console
user and other CLI run that changes this pair's links paused until conversion
and verification finish. There is no atomic server lock across the census,
config write and record updates. Account for hooks that themselves write links.

For a new conversion, the plan prints the number of links that will be
**invisible between the mapping switch and completion of restore**. Each missing link is re-created
with a record update: budget for `onUpdate`/`postUpdate` hooks, sync mappings
and notifications per link. Their exact invocation counts were not measured
in the API investigation; these are real writes, not a metadata-only change.
Re-running a partial restore skips links already present with the captured
values, so it does not deliberately repeat those writes.

Capture preserves each forward record ID, target reference and both ends'
custom `_refProperties`, excluding each edge's `_id` and `_rev`. If the ends
report different custom values (including a value missing on either end),
the census lists the differences and refuses before any write. Different
per-side values can be legitimate data: this is an aic support limitation,
not proof of an inconsistency. Preserve those values and arrange a supported
migration; do not equalise them just to make conversion proceed.
It scans all records on both ends with cursor paging and projected fields. A `pr` query cannot
find generic links reliably; see [relationship querying](api/10-managed-objects.md#querying-a-relationship-collection-verified-2026-10-31).
The census refuses and lists forward-only or reverse-only links. Resolve that
pre-existing inconsistency first: converting them would change what an end
reports. Duplicate forward/target edges are also refused because a retry could
not safely identify which one to re-create.

Captures are local files containing relationship data. The current format is
version 2 and requires both recorded custom-property views for a two-way link.
Files from earlier development builds refuse to load with “captured by an
earlier development build; re-capture”; there is no silent upgrade and no
inferred reverse data. Use a fresh capture before a new conversion, while the
generic links are visible. If a switch has already hidden them, preserve the
old file and inspect the environment instead of capturing the mapped store
as though it were the original generic data.

The default capture location is `.aic/relationship-captures/<uuid>.json`;
the command prints it before the switch. New files are owner-readable/writable. For a predictable recovery
command, choose an explicit path as below. Keep the file and its sidecars
available on the same machine while a run is active. The capture is never
automatically deleted or overwritten with a different capture. It is marked
complete only after the final checks. A run lock prevents two processes using
the same capture simultaneously; it does not lock other clients of the tenant.

## Convert a generic pair in dev

First pause link writers. Preview the column allocation, any per-type entry,
both mappings, link count and visibility window:

```sh
aic managed relationship map custom_widget.owner --tenant dev \
  --capture .aic/relationship-captures/dev-widget.json --dry-run
```

The preview says **planned capture; no file written** and holds no write
permit. When mappings are already installed, a resume plan names the saved
state, verifies existing links and re-creates only missing links; it does not
propose another mapping switch. Inspect any refusal before continuing. Then run the same operation with confirmation:

```sh
aic managed relationship map custom_widget.owner --tenant dev \
  --capture .aic/relationship-captures/dev-widget.json --force
```

Without `--force` the command asks at a terminal; noninteractive use requires
it. Production-themed tenants additionally require `--yes`.

The command durably saves the links before any tenant write, repeats the
census, checks the fresh declarations, backs up `repo.ds`, and writes both
mapping entries in one whole-document PUT with `waitForCompletion=true`.
Missing per-type entries go in that same PUT. It confirms the stored document
by re-read and checks the newly assigned column for exposed orphaned data.
It then restores each missing link through the forward end, preserving custom
properties. Has-many uses relationship collection create; has-one uses a
conditional JSON-patch replace. No null replacement is used.

Look for each pair's **exact captured set confirmed** message and **mapped
runtime confirmed on each end**, followed by the capture's completion message.
The final comparison includes forward record, target and both ends' custom
properties; an extra link or changed properties fail verification. Every link must resolve
from the reverse end when one exists. Runtime confirmation uses the existing
`verify` presence-query judgement on linked records, on both ends. A one-way
pair checks its forward runtime and target records. An empty pair cannot prove
runtime storage mode and is reported untested; no probe link is written.

You can inspect the pair independently before resuming writers:

```sh
aic managed relationship verify custom_widget.owner --tenant dev
```

`map` also works on a link-free generic pair using the same flow. A pair
already mapped requires its existing capture for resume, rather than a new
capture of a store whose generic links are now hidden.

## If conversion or restore stops

Keep writers paused and preserve the printed capture. Each request records
its intent before it is sent. A response lost after the write landed leaves
that intent for a later run to reconcile. The live store determines what is
missing; progress flags do not authorize duplicate writes. Response edge IDs
and read-confirmed receipts record the conversion's work. Logical link identity
is the captured record, target and custom properties; rollback reads current
edge IDs rather than assuming their counter format is an allocation rule. Do
not hand-edit the capture or run separate captures concurrently for the same
pair.

For an in-place conversion, repeat the original command with the **same**
file. It switches if the first switch did not land, or resumes restoration if
the mapping is already installed:

```sh
aic managed relationship map custom_widget.owner --tenant dev \
  --capture .aic/relationship-captures/dev-widget.json --force
```

Once the mapping is installed, the record-only continuation is:

```sh
aic managed relationship restore-links .aic/relationship-captures/dev-widget.json \
  --tenant dev --force
```

The command first re-reads `repo.ds` and refuses unless the captured mappings
are present and match. It checks the declarations again before writing records.
For pairs with captured links, it also uses set's read-only activation wait
before permitting record writes; an endpoint must be live, including after a
schema repair. Already restored links are verified and skipped. It refuses an
occupied has-one, different custom properties or an extra mapped link; it never
overwrites those to make the comparison pass.
Correct the reported problem and retry. A failed send or read-back leaves the
outcome unknown, not automatically undone. If the mapping disappeared after
restoration began, inspect the environment before remapping.

`relationship restore` without `-links` is the older definition-metadata
restore command and does not restore relationship records.

## Promotion: capture UAT's links, promote, restore UAT

Promotion carries `repo.ds` according to the operator's statement; the API
investigation measured a dry-run report that omitted its mapping change,
not an actual promotion proving propagation. **Records are never promoted.**
A promoted generic-to-mapped conversion is therefore expected to hide the
upper environment's own generic links. That consequence is inferred from the
measured storage switch. See [the promotion evidence](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01).

Use a separate capture of UAT's data. A dev capture is bound to dev and cannot
be restored into UAT. Pause UAT link writers and follow the existing
[promotion checks](CLI.md#aic-promotion--promotion-lock-state-status-reports-and-the-write-cycle) for your tenant.
The following shows the order; inspect the dry-run result before `promote`:

```sh
aic promotion lock --tenant uat --force
aic managed relationship capture-links --lower dev --upper uat \
  --output .aic/relationship-captures/uat-widget.json --dry-run
aic managed relationship capture-links --lower dev --upper uat \
  --output .aic/relationship-captures/uat-widget.json --force
aic promotion dry-run --tenant uat --force
# Inspect the promotion plan before continuing.
aic promotion promote --tenant uat --force
aic promotion unlock --tenant uat --force
```

If this run of `capture-links` confirmed **capture durable at** the exact path
below, restore that file after promotion:

```sh
aic managed relationship restore-links .aic/relationship-captures/uat-widget.json \
  --tenant uat --force
```

If it instead printed **no file written** (only empty or acknowledged new
pairs), skip restore and perform the empty/new-pair checks below. Restore only
the file this run confirmed writing. Never substitute an older file already
at the same path; it describes another capture, even if its tenant matches.

`capture-links` first compares complete storage bindings across every
`resourceMapping` section, including `explicitMapping`: the owning reference
end, section, column (`ldapAttribute`), target (`resourcePath`), cardinality
(`isMultiValued`), reverse entries and other mapping fields. Before skipping
equivalent bindings, it compares every named end's declaration footprint:
presence, relationship versus scalar, target set, cardinality and reverse key.
Different footprints refuse with both versions, even when repo.ds is identical.
Identical footprints (including identically absent platform declarations) skip.
This comparison supports stock multi-target/internal targets without passing
them through the narrower conversion validator. Pairs generic in both are
also skipped. For lower mapped / upper generic candidates, it validates the
declared pair, visits the reference end once, and saves only pairs with upper
links. Unsupported candidates refuse with their name and reason.

**Mapped in both is safe to skip only when the bindings are equivalent.** A
different column, owner, section or reverse binding refuses with both versions.
An upper mapping absent in lower also refuses: whole-document promotion is
expected to remove that binding and hide the upper's mapped links. This removal
consequence is inference from the [storage/promotion mechanism](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01), not a measured promotion.
Do not promote after either refusal. Inspect the printed bindings and bring
the lower configuration into agreement with upper, or resolve the data
migration separately before continuing. These commands convert generic data;
they do not migrate between mapped columns or reference owners.

The command prints each captured pair and count, then confirms the durable
file. It writes no tenant configuration or records. If none need capture it
explicitly says no file was written, after a fresh declaration/binding recheck.
Existing output files are preserved: choose a new path for another promotion
rather than replacing an earlier recovery source.

A missing lower custom declaration that upper still declares is suspected
Q14 loss: restore the declaration and re-read. A confirmed stale genericMapping
entry can be reviewed for cleanup with `relationship repair`. That verb cannot
remove explicitMapping entries or undeclared platform mappings; differences
there require investigation with the tenant administrator. The diagnostic
names the binding and says not to promote until resolved, rather than
suggesting an unavailable repair. None of these differences is silently skipped.
A missing upper declaration is an error rather than proof of zero links:
under Q14 it may have disappeared. A mixed rollout can also contain genuinely
new pairs that have never existed in the upper environment. After checking
that history, explicitly identify each such lower **reference** end with
repeatable `--new-pair` flags:

```sh
aic managed relationship capture-links --lower dev --upper uat \
  --output .aic/relationship-captures/uat-widget.json \
  --new-pair new_widget.owner --force
```

Every bound end and schema-named reverse end must have no upper declaration
or mapping entry; a typo, existing end, or stale mapping refuses the acknowledgment.
This check precedes the narrow conversion validator, so a genuinely new
multi-target component can be acknowledged even though aic cannot migrate its
links. It does not bypass a missing lower custom declaration. The command prints
the exclusions and records them in any capture it creates. This lets it capture
other existing linked pairs in the same rollout. Never use `--new-pair` to
bypass a possible Q14 loss: it is the operator's assertion that there have
never been upper links on these ends, not a conclusion a missing API property
can establish. The complete selection and mappings are rechecked before save.

After promotion, check acknowledged new ends and other pairs that had no
captured links for exposed orphaned column data before resuming writers. For
example, the presence query below must have an empty result for a new pair:

```sh
aic idm get 'managed/new_widget?_queryFilter=/owner%20pr' --fields _id --tenant uat
```

Any record is unexpected data: stop and inspect it. Captured-set verification
covers the captured pairs; it cannot certify an uncaptured pair's empty store.

After promotion, `restore-links` refuses if the expected mappings have not
landed. Once they match, it restores and verifies the exact captured UAT set
using the same engine as dev conversion. It writes **records only**, never
config, so it can run while upper-environment config is locked. Keep writers
paused until its completion message. If it stops, repeat `restore-links` with
that UAT file; do not capture again after the switch, when generic links are
hidden. The promotion verbs themselves are unchanged.

For prod, repeat the entire process with **UAT as lower and prod as upper**,
and capture prod's own links to a new file. For example:

```sh
aic promotion lock --tenant prod --force --yes
aic managed relationship capture-links --lower uat --upper prod \
  --output .aic/relationship-captures/prod-widget.json --force --yes
aic promotion dry-run --tenant prod --force --yes
# Inspect the promotion plan before continuing.
aic promotion promote --tenant prod --force --yes
aic promotion unlock --tenant prod --force --yes
# Run only if this capture-links run confirmed writing this exact file.
# If no file was written, do the empty/new-pair checks instead.
aic managed relationship restore-links .aic/relationship-captures/prod-widget.json \
  --tenant prod --force --yes
```

The lower environment being empty says nothing about destination links.
`--lower` and `--upper` have the same direction as `promotion verify`.

## Explicit rollback in dev

Rollback is available only with an **in-place** capture bound to the same
tenant. A promotion capture cannot authorize a config write on UAT or prod.
There is no automatic rollback after an error.

```sh
aic managed relationship rollback-map .aic/relationship-captures/dev-widget.json \
  --tenant dev --dry-run
aic managed relationship rollback-map .aic/relationship-captures/dev-widget.json \
  --tenant dev --force
```

Before removing any records it plans the complete mapping removal, refusing
external reverse dependents. Whole-batch preflight checks required fields,
link ownership, record revisions and runtime/far resolution for every actual
planned unlink before the first mutation. A switch with no mapped links needs
no record removal, so a required forward property does not block its rollback.
A refusal leaves all records intact. It then submits the **unchanged** `repo.ds`
through
the backed-up, read-confirmed writer. A config write denial stops there, with
all links intact; the capture's origin alone does not establish current write
access. This preliminary PUT removes no mappings. The operation then removes
this conversion's restored links from the mapped store, confirming each
removal and mapped runtime, and removes both captured mappings in one
backed-up, read-confirmed PUT. It keeps unrelated mappings and per-type metadata: dropping
a per-type entry could hide records created under that entry. Finally it
checks that the original captured generic set is visible again and marks the
capture rolled back. Check the restored generic links and their notes from
both ends before resuming writers.

Unknown, extra or altered mapped links stop rollback for inspection. A
required forward property cannot be cleared by the measured removal method;
when mapped links actually need removal, explicitly adjust that policy before
rollback and reinstate it afterwards. No schema change is needed when the
mapped store is empty.
If rollback stops, keep writers paused and repeat **rollback-map** with the
same file to finish; completed removals are skipped. If an external dependent
caused the refusal, resolve that dependency first. A run killed after mapping
removal checks the visible generic set on retry before marking success.

To reverse an interrupted rollback instead, while the exact captured mappings
are still installed and **no Remove intent is pending**, explicitly run:

```sh
aic managed relationship restore-links .aic/relationship-captures/dev-widget.json \
  --tenant dev --force
```

If the file has an unresolved `Remove` intent, direction change refuses before
writing or clearing that intent. A link observed present (or absent) cannot
rule out a delayed unlink. Keep writers paused and finish `rollback-map` with
the same file; do not hand-edit away the intent.

With no outstanding removal, its plan names the saved `RollingBack` state and
the change back to restoration.
It verifies links still present, restores missing captured links and marks the
capture complete after verification. This writes records only. Once the
mapping has been removed, finish `rollback-map`; `restore-links` refuses a
generic pair or a completed `RolledBack` capture.

## Q14: confirmed declarations can disappear

[Q14](api/99-quirks-and-open-questions.md#q14-configmanaged-reads-appeared-to-go-backwards-2026-08-05) describes reads going backwards
and properties vanishing after confirmed writes, without another client write.
Before switching, aic re-reads both ends' declarations; before restoring, it
reads them again. Missing ends or changed cardinality/targets refuse with a
Q14/schema diagnostic. The checks recur during a long restore and rollback.
A stale or missing declaration is never read as an empty link set.

If this happens, keep the capture and writers paused. Inspect both ends:

```sh
aic managed get custom_widget --property owner --tenant dev
aic managed get alpha_user --property widgets --tenant dev
```

Substitute the actual reverse object/key and destination tenant. Resolve the
tenant's declaration problem through its configuration workflow before retrying.
On a locked upper environment that may require a corrected promotion. These
checks narrow the risk; they cannot lock the tenant's config store or prevent
another Q14 rollback after the last read.

## New relationships, columns and orphaned values

`relationship set` creates new pairs mapped by default: confirm the managed
schema first, then add mappings. Edits preserve existing storage mode; there
is no `--generic` opt-out. A new unmapped declaration remains a consistent
generic pair if mapping fails before its PUT, unless another writer changed
it. Use `map` with a capture to finish. A failed send is reported unknown.
Matching leftover mappings on a recreated key are preserved.

The named end holds the reference. Which end the console chooses when saved
from the other side is unmeasured. Each end's `isMultiValued` follows its own
schema array shape. Same-object pairs use two distinct keys in one per-type
entry and consume one column; a key naming itself as reverse is refused.
`set` retains its activation wait and `--check`; use `--check` before a script
writes links. An object without records cannot be probed. Conversion itself
never writes a test/probe link.

Only `fr-idm-reference-1` through `-10` work, per object type. A PUT naming
`-11` succeeded in the API investigation but link writes failed with
`objectClassViolation`. aic counts all property mentions, including
`explicitMapping`, prefers max-in-use + 1, then the lowest free column when
max+1 exceeds ten. If all are occupied it refuses and names the holders.
Reverse references consume no reverse-side column.

Ping warns a reused column can expose orphaned values. Immediately after an
in-place switch and before restoration, the forward `/<key> pr` query must be
empty. Matches are reported as **ORPHANED DATA** and exit non-zero. Mapping
stays installed, evidence stays visible, and no records are cleaned; an
automatic compensating PUT could overwrite concurrent config. A retry cannot
silently bypass that failed check. Inspect the values and backup before any
recovery decision. See the [column measurements and warning](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01).

Ping recommends managing relationships only through the console or Schema
API and warns other methods may de-sync data. The measured service-account
Schema API request returns 403; the direct whole-document repo.ds PUT returns
200, reads back identically, and new links resolve from both ends with runtime
mapped confirmation. That measured route explains aic's implementation, not
vendor support. The [API evidence](api/10-managed-objects.md#converting-a-pair-between-storage-modes-measured-2026-11-01)
is the contract behind it; no new live measurements are claimed here.
