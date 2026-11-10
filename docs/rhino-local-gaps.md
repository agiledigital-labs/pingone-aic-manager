# rhino-local gaps — real scripts vs the local overlay

What happens when the local harness is pointed at the scripted-decision scripts
this repo actually has, instead of the three toy cases.

Cases live under `packages/rhino-local/cases/real/`. Overlay:
`packages/rhino-local/src/bindings/rhino/runtime.cjs`. Measured 2026-09-12
against the JVM runner (`VERSION_DEFAULT` + `ScriptContextScope`) after
`callbacks.isEmpty`, next-gen `require()`, and legacy `Action.send`.

## What runs today

Re-measured 2026-09-28 against the sandbox (`run-probes.sh`) and the host-JVM
runner, and on 2026-09-29 for the ten `binding-*` cases. 57 cases, and every
one is green — by asserting what is true, not by
matching the overlay:

| Count | Status                                                                                             |
| ----- | -------------------------------------------------------------------------------------------------- |
| 30    | pass the live payload exactly                                                                      |
| 15    | **known gap** — live values committed, the differing keys pinned                                   |
| 2     | **known gap**, request cases — live request data not committed (it names the tenant); local pinned |
| 1     | **unmeasured** — `identity-enum-attrs`: every live count was `err`; local pinned                   |
| 9     | parse errors, blocked on `(parse)` with the JVM's exact message                                    |

A case runs its **origin fixture**, the file the tenant runs, not a copy. The
copies had drifted, and a two-line header shifted every error line off AIC's.
Runtime errors use AIC's source name (`AIC Rhino Let Probe`), so `(name#line)`
suffixes compare equal. Identity fixtures name tenant users; a case points them
at placeholder records with a `Rewrite` matched on the fixture's structure
(`var PROBE_USER = "…"`), never on the value it replaces, so no tenant
identifier is repeated in `cases/real/index.ts`.

A `gap` entry (`cases/real/load.ts`, `KnownGap`) names the payload keys that
differ. The local payload's keys must be exactly the live ones plus those, and
every other key must equal the committed live payload. Outside the payload the
case is judged like any other — outcome, state, every callback, openidm, http
and logs — so a gap entry excuses one set of values, not the run. Each differing
key's local value is snapshotted (`test/corpus/__snapshots__/`), and where the
live value is committed too, local must still differ from it — so closing a gap
fails the test and asks for the entry to be dropped. Do not tune a live payload
to the overlay; update the snapshot only after reading why it moved.

## Environment profiles (added 2026-09-12)

`npm run pull-profile` pulls `GET /openidm/config/managed` into a normalised
snapshot at `.aic-script-tester/profiles/<tenant>.json` (before 2026-09-28,
`workspace/<tenant>/harness-profile.json`). The state directory ignores itself,
which is the guard: managed object and property names are
client business vocabulary, and `check-sensitive-metadata.sh` deliberately
holds no client-name denylist.

Verified against the sandbox 2026-09-12: 28 objects, 326 properties, all
recovered — 0 properties normalised to `any`, and object/property/enum/
relationship/`required` counts recomputed independently off the wire all
match. `type: ["string","null"]` collapses to `type: "string"` +
`nullable: true`. Array-of-relationship element schemas survive (42 of them).
Lifecycle hook **source bodies are dropped**: `text/javascript` occurs 0 times
in the profile, which is 20.5 KB against the raw document's 212 KB.

What a profile buys, and it is one thing: **type existence**. AIC returns
`null` both for a real type whose record is absent and for a type that does
not exist (`docs/api/10`), so a fixture typo is invisible on the tenant. With
a profile the harness separates them — a declared-but-unseeded type reads as
an empty collection (`null`, matching AIC), and an undeclared one is answered
as AIC answers it: `openidm.read` returns `null` and `openidm.query` throws AIC's
catchable `Resource 'managed/<type>' not found` (`docs/api/12`). A write
(`create`, `update`, `patch`, `delete`, and `action` on the collection or a
record) to an undeclared type is the harness fault
`openidm-unknown-managed-type`, naming the environment and the pull date,
because what AIC does there is unmeasured. Without a profile the pre-existing
behaviour is unchanged: any unseeded collection is the fault `fixture/managed`.

A profile lists managed **types**, so it speaks only for a bare
`managed/<type>` collection and its records. Every other unseeded resource —
`internal/role`, `config/…`, `system/…`, and a relationship collection such as
`managed/alpha_user/<id>/roles` even when its type is declared — stays the
fault `fixture/managed` with or without a profile, `create` included. Before
E19 fix round 3 a profile answered all of them as an absent managed type
(`null`, or a catchable not-found), which on AIC is false for every one that
exists.

One asymmetry is deliberate and pre-existing: **without** a profile,
`openidm.create` into an unseeded collection makes the collection and succeeds,
while `read` of the same collection is `fixture/managed`. Faulting it would
break every test that creates into a real but unseeded type; seed `[]` for the
collection, or run with a profile, and the create is checked.

**Implicit sync is not considered in this lane.** A write here on a type that
is a `config/sync` mapping's source passes without running, faulting or
recording anything about the mapping; on AIC that write runs the mapping
(`docs/api/16-sync-mappings.md`). The IDM emulator faults the same write as
`idm-implicit-sync/<mapping>`. This lane has no `config/sync` input, so it
cannot tell which writes those are (review e15-02 P3-D; the open E15 round-1
Q2 proposes routing these writes through an `IdmEmulator`).

**Hooks and policies are not run in this lane (E15b).** A write here that
reaches the lane's store, on a type whose profile declares script hooks or
policies, is a harness fault: `openidm-managed-hook/<type>.<hook>` or
`openidm-managed-policy/<type>` (table below). The profile records hook
names only, never bodies. A write a test stubs with `given.openidmFailures` is
the test's own claim and does not fault; a type with neither keeps writing.
A hook, `enum`, `policies`, `conditionalPolicies`, `fallbackPolicies` or
non-empty `required` declared **below** a top-level property, under
`properties` or `items` at any depth, counts too (astra-r1 P2-1): what AIC
does with one was never measured (only nested type mismatches were: stored
unchecked), so a nested hook faults every write of the type as
`openidm-managed-unmeasured-hook/<type>.<path>.<hook>` (`box.p.onValidate`,
`box[].onValidate`), and a nested policy faults a create, update or patch as
`openidm-managed-policy/<type>`. The IDM emulator refuses the same
declarations (`nested property behavior <hook>`, `schema policy <key>`), so
both lanes fault. A schema property is judged whatever its name: `__proto__`,
`constructor` and `toString` survive normalisation and every policy table as
own keys (astra-r1 P2-2).
Unstubbed managed-collection actions (`patch`, `triggerSyncCheck`,
`updateLastSync`) used to return an invented `{}` and now fault too.

The hook a fault names is the first the write runs in the measured order
(docs/api/24, "Managed-object hooks / Order"): an update or patch runs the read
hooks first (`onRetrieve`, property `onRetrieve`, `onRead`), then `onUpdate`,
every property's `onValidate`, `onValidate`, every property's `onStore`,
`onStore`, `postUpdate`, `onSync`.

A profile must key each type by its own `name`. One whose map key and
`object.name` disagree is refused when it is read and again when a case is
given it, because the key is what says the type exists and every write,
default and policy table is keyed by it (review sol-r1 P1-2).

The corpus's `liveHooks` acknowledgement (corpus-only, on `RealEntry`) is
checked where the corpus runner consumes it, `profileForRun`, as well as where
`realCase` builds it. Known limit: the checks prove the case carries a live
payload, not that the live run wrote to the types whose hooks it names, so an
unrelated read-only probe's payload would satisfy them. That link rests on
review.

**An invalid `default` anywhere faults every managed call (sol-r1c).** AIC
stores a `config/managed` holding an invalid default (`PUT` 200) and keeps
serving its **last good compile** for every type: the bad type and every type
added after it are 404, a removed type keeps serving, until the bad default is
gone (measured 2026-11-08, zzemu_def probe, docs/api/10). A profile records the
stored document, not that earlier compile, so nothing the lane could answer for
any managed type is trustworthy. Both lanes judge every `default` with one
function (`src/declared-default.ts` `judgeDefault`). Measured: the top-level
`type` is checked, an array default item by item against `items.type`, and an
object default only for being an object (its `properties` are not checked);
`default: null` is valid on every type (string, plain or nullable, boolean,
number, integer, object, array; zzemu_nd); and **every numeric default on
`integer` is invalid**, `3` included, because IDM's `VALID_TYPE` calls every
JSON number `"number"` (zzemu_nd). Judged by the same rule without a probe of
their own: other scalar mismatches on `string` and `number`. Unmeasured: a
`null` item, arrays of arrays, a `relationship` or untyped default, any `type`
list other than the exact `["<type>", "null"]` pair (two concrete types, a
singleton `["string"]`, a repeated `"null"`; at the top level or under `items`),
and a `default` nested inside `items` or `properties` — the profile keeps both
since version 3 (sol-r2 P2-1, sol-r3 P2-1) so they fault rather than vanish.

- **Script tester:** any `openidm` call on a `managed/…` resource — any type,
  declared or not — faults with one id naming the first offending property,
  `openidm-managed-invalid-default/<type>.<property>`, or
  `openidm-managed-unmeasured-default/<type>.<property>` for the unmeasured
  cases. The profile is never repaired.
- **IDM emulator:** refuses the whole config before every request, as it does
  for any declaration it cannot serve, with the same id
  (`openidm-managed-invalid-default/<type>.<property>: invalid default:
  <type>/<property>`). A refusal at load is what the emulator already does for a
  config it cannot serve, and the tenant's own state is "this config is not the
  one running", which no per-request answer would model better.

Neither lane emulates the measured 404 surface of an unavailable type (REST
`Resource 'managed/<t>' not found`, script `NotFoundException`, `openidm.read`
`null`): answering 404 for the bad type would imply its siblings are live as
written, which they are not.

**Defaults, nulls and types in this lane.** The profile records each
property's `default` and top-level `type` (version 3), and the lane answers
IDM's measured implicit policies (`write-guard.ts` `managedPolicy`; docs/api/10,
including the 2026-11-08 probe):

- A create that omits a property with a `default` stores the default.
  `default: null` stores `null`, and `[]` on an array, where a `null` sent to
  that array also stores `[]` (measured with a `required` array, zzemu_nd).
  Without a `required` array the string's applied `null` is stored and its sent
  `null` refused (measured, N6); on any other type the create faults,
  unmeasured.
- A create of a new record is refused with AIC's catchable `403 Policy
  validation failed` when it sends `null` (or `undefined`, which
  `openidm.create` sends as `null`) for a property with a non-null `default`
  (`NOT_NULL` keys on the non-null default, not the type: a `default: null`
  property accepts `null`, measured),
  `null` for any typed property when the schema has no `required` array, or a
  top-level value whose JSON type is not the declared `string`, `boolean`,
  `number`, `integer`, `object` or `array` — any number sent to an `integer`
  included (zzemu_nd). Any other `null` on an undefaulted property is stored as
  `null`.
  Nothing nested under `items` or `properties` is checked (measured: nested
  schemas are not enforced). The refusal's `getDetail()` faults
  (`openidm-policy-detail`), so several failing properties need no measured
  order here.
- A PATCH `remove` of a property with no `default` writes and drops the key
  (measured 200 for a non-required property; a required-named one is faulted
  with the type's declared policies).

Faulted as `openidm-managed-policy/<type>`, because unmeasured: any of those
refusals outside a new record's create (an update, a create over an existing
id, a value a PATCH writes at a top-level path); a `null` sent to a
null-defaulted array outside a new record's create; an item of a top-level array that is not the
declared `items.type` (only nested `items` were measured); a `null` on a
nullable property not declaring `default: null` (with a `required` array), or,
with no `required` array, on a `default: null` property or a property of a type
the rule does not know (`relationship`, `any`); a non-null value of the wrong
JSON type on a nullable property whose nullable spelling was not probed
(`["boolean", "null"]`, `["integer", "null"]`, `["array", "null"]`, a nullable
string without a measured null default): the plain type's `403 VALID_TYPE` was
measured, the nullable spelling's never was (sol-r3 P2-3). A value of the
concrete type writes. A `null` an update sends for an
undefaulted property that a create would accept writes as on create (assumed;
Q18). A PATCH `replace` with `null` is AIC's invalid-patch refusal, and an `add`
with `null` faults. An operation whose `value` is `undefined`, or which has no
`value` key, is the same `null` (AIC sends a script's `undefined` as `null`,
docs/api/12; the absent key is inference). Both refusals, like an unknown
operation, are judged before the record lookup and the revision check (assumed
for the script path). A PATCH `remove` of a property that declares a `default`,
`null` included, faults `openidm-managed-policy/<type>`: stock `policy.js`
predicts `403 REQUIRED`, but AIC was measured only with the property also
required (Q18). Still **not checked** here, and passing silently:

- a mismatch on a `relationship` or untyped (`any`) property — which includes a
  property whose `type` lists two concrete types (profile version 3 records it
  as `any`) — and a PATCH
  writing below a top-level property (`/box/tags`), which is nested and so
  unchecked on AIC too, except that a top-level array's items are unmeasured;
- an `undefined` in an update body, which is cloned rather than read as `null`;
- writes to a seeded sub-resource (`managed/<type>/<id>/<rel>`), which look up
  that path, not `managed/<type>`, and so bypass the hook, policy and default
  tables (the invalid-default fault still applies: it matches any
  `managed/…` resource).

`conditionalPolicies` and `fallbackPolicies` are recorded by name and count as
declared policies here. The IDM emulator does not run them either: its
capability gate faults any schema that declares them, as an unknown property
behaviour, with or without a `policy` evaluator.

**Unset properties on a custom type read back as `null`/`[]` locally, and are
absent on AIC (open, 2026-11-08).** The IDM emulator materializes every
declared property a record lacks — `null` for a scalar, `[]` for an array —
on the assumption that a generic type follows `alpha_user`'s measured shape
(`ManagedStore.materialize`). The 2026-11-08 probe (docs/api/10) contradicts it
for custom types: their create responses and `_fields=*` reads carried only the
properties written or defaulted, and a `PATCH remove` left the key absent, not
`null`. Not changed in sol-r1b: it moves every generic-type read and query, and
whether the split is by DS storage (explicit `repo.ds` mapping versus the
generic JSON store) is itself unmeasured. The policy outcomes above do not
depend on it.

Only the SET of declared type names, the per-type write-fault and policy
tables, and the one invalid-default fault that `src/profile/write-guard.ts`
decides, cross
into the sandbox. Every schema
rule — properties, `required`, `enum` — is decided in the Node layer, so each
rule has one implementation rather than an AM-safe second copy that could
drift from it.

### Typos without a profile

A read of an undeclared type returning `null` is what AIC does, and it is also
how a fixture typo hides. The profile is what tells a typo from a real absent
type, so without one the harness still refuses the read as `fixture/managed`
(a false fail, not a false pass). `lib-openidm-miss-consumer` runs under a
profile that declares its two real types, and its nonexistent-type read
returns the live `null`.

### What strict checking found on its first run

Six of the eight fixtures carrying `given.managed` were rejected, and the
rejections were correct:

- Five identity fixtures seed **AM-side attribute names into an IDM managed
  record** — `inetUserStatus`, `fr-idm-uuid`, `fr-idm-custom-attrs`. The
  tenant's `alpha_user` defines `accountStatus` and `_id`; the other two are
  AM projections with no IDM property of that name. This is exactly the
  namespace confusion `docs/api/14`'s mapping table exists to resolve, and it
  was invisible until a schema was available to check against.
- One fixture names a managed type this environment does not define at all.

Correcting them is not mechanical: the expects encode live AM-side behaviour,
so a fixture moved into the IDM namespace needs the identity binding to apply
the mapping on read. Sizing that against the live schema — `alpha_user` has 70
properties here; `docs/api/14` maps 26 of them, 8 confirmed live. The
remainder (`frIndexed*`, `effectiveRoles`, `authzRoles`, …) have no recorded
AM name, so a script reading one through `identity` cannot be tested locally
until that row is measured.

The downstream 0.2.0 gaps D9/D11 are now explicit input choices. Action replies
use `given.openidmActions` like local HTTP stubs and have the same AIC
ineligibility/observation gap. Unstubbed non-collection actions throw rather
than invent `{}`; so, since E15b, do unstubbed managed-collection actions, whose
names are measured but whose results are not (`openidm-managed-collection-action/<action>`).
Undeclared ESVs still throw by default; `esvUndeclared: "absent"` opts into AM's
null/default path and remains AIC-eligible. Comma-joined read/query field elements
and params `_fields` share projection without trimming (2026-10-02 measurement).

## Harness faults

Decision E19 (2026-11-05): every **true refusal** in the script tester fails
the run, even when the script catches it. A true refusal is the harness
declining behaviour it does not emulate. An error that **AIC itself raises**,
and that the harness emulates, stays an ordinary catchable script error. Each
true refusal carries a stable id: a kebab-case slug, or a family prefix plus the
binding method. Ids are never derived from line numbers or message text. A
failure reads `rhino-local harness fault <id>: <message> (not emulated: this
fails the run even if the script caught it)`. The run-end tally counts cases
per id (see the package README, "Harness faults").

The JVM keeps a per-job recorder (`java/HarnessFaults.java`). It reports the
fault on the job's response whatever the outcome, so a script cannot swallow
it. Under Node's `vm` (no JVM), `runtime.cjs` falls back to its own fault list
and the harvest check.

**Emulated AIC errors (catchable, not faults).** Each cites a measurement or an
assumption in `docs/api/`:

- `keySet` blocked by AM's class shutter (two sites).
- a `consentMappingCallback` config with no `fields` list.
- `No Such Entry`.
- the two query-parameter errors (`You must use exactly one of …` and
  `Query Expressions are not supported …`).
- a malformed `_queryFilter`: `The value '<filter>' for parameter
  '_queryFilter' could not be parsed as a valid query filter` (script text
  measured for `name eq`; the REST lane measures the same rejection for the word
  form `not`, `eq null`, `in` and array values). The filter is parsed before any
  record is read, so an empty collection does not hide it.
- `Resource 'managed/<type>' not found` from `openidm.query` on a type the
  profile does not declare.
- a wrong argument count where AIC's accepted counts have a basis (below).
- `Entry Already Exists` on a colliding `create`.
- invalid JSON patch.
- `Assertion Failed`.
- `Expecting String containing one of …` for an unknown managed-collection
  action.
- `Secret not accessible`.

**Ids**, by family:

| Id                                                                                | Refusal                                                                                                               |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `not-mocked/<binding>.<method>`                                                   | a generated binding method the overlay does not implement                                                            |
| `arity-unknown/<binding>.<method>`                                                | an argument count outside the harness's own guessed range, or one only a varargs reading would accept (see below) |
| `fixture/<kind>`                                                                  | an unseeded fixture: `managed`, `http`, `esv`, `secrets`, `library`, `callbacks`, `callback-value`, `openidm-action`, `cache-manager`, `journey`, `match-pattern-type` |
| `script-defect/send-body-js-date`, `script-defect/send-body-regexp`, `script-defect/send-body-function` | an `httpClient.send` body AIC **measurably** sends as a Rhino object dump. Not a harness gap: the `script-defect/` family is a refusal to pass a script that would send the dump, and the tally lists it apart from the harness's work |
| `send-body-error`, `send-body-array-cycle`                                        | an `httpClient.send` body whose AIC behaviour is unmeasured                                                           |
| `state-put-undefined`                                                             | `nodeState.put*` / `sharedState.put` / `transientState.put` with `undefined`                                         |
| `request-map-foreach-argument`                                                    | `requestParameters`/`requestHeaders`/`requestCookies`.`forEach` with anything but one function, no argument, or one number. Those three are measured (2026-11-08): the callback gets `(key, value)` and the other two are Rhino's catchable `Can't find method java.util.Map.forEach(<type>).`; key order is seeding order, where AIC's is hash order |
| `request-map-foreach-legacy`                                                      | `forEach` on a request map under the legacy engine, which was not probed |
| `request-list-unmeasured`                                                        | A next-gen request value list called any way but `size()`, `get(i)` with an integer index in range, `String(v)` (measured 2026-11-08), `[].concat(v)` with the list anywhere among the arguments or as the receiver, or `JSON.stringify` (measured 2026-11-10): another argument shape, or another `List` member (every public `java.util.List`, `Collection`, `Iterable` and `Object` method name: `set`, `remove`, `isEmpty`, `contains`, `toArray`, `forEach`, `listIterator`, …). A JavaScript object would answer with ignore-the-extra-argument rules that nothing says a Java list follows. Not covered: an out-of-range bracket read (`v[5]`) returns `undefined` unfaulted, since an index read cannot be intercepted; AIC's answer is unmeasured Nor, for either list kind, a list an author's own `toJSON` returns under an array replacer: it is written as an object of the listed keys, unfaulted (native asks `toJSON` once per value), and while such a call runs a getter that reads `list.toJSON` sees a function, where AIC's Java list has no such member |
| `openidm-list-unmeasured`                                                        | An `openidm.query` `result` or a projected has-many array (next-gen) called any way but `size()`, `get(i)` with an integer index (out of range is the measured `InternalError`), `[].concat(v)`, `JSON.stringify` or `String(v)` (measured 2026-11-08): another argument shape, another `List` member (`forEach` included), the list as `concat`'s receiver, or `String(v)` reaching a number, whose Java rendering (`Integer` or `Double`) was not measured |
| `openidm-failure-detail`                                                          | `e.javaException.getDetail()` on an `openidmFailures` failure whose stub gave no `reply.detail`; calling, or rendering as a string, a `JsonValue` member the 2026-11-08 and 2026-11-10 probes did not exercise (anything but `get`, `isNull`, `getObject`, `toString`, `asString`; `typeof` and `in` answer as measured and never fault); `get(string)` on a string or boolean value, `get(number)` on a boolean, a non-integral index; `asString` on a number or with an argument other than one number; an error pointer whose key needs a JSON Pointer escape; the string form of an empty object or array, a non-integral number or a string needing an escape |
| `node-state-merge-non-object`                                                     | `nodeState.mergeShared`/`mergeTransient` with a function, and `mergeTransient(null)`. Every other non-object is AIC's measured catchable `InternalError` (2026-11-06): `Can't find method …NodeStateScriptWrapper.<m>(<type>).` for a string, number, boolean or `undefined`, `null` for `mergeShared(null)`, `Cannot convert …NativeArray@<hash> to java.util.Map` for an array; state is unchanged |
| `action-suspend-additional-logic`                                                 | `action.suspend` with a non-function `additionalLogic`                                                               |
| `openidm-collection-path-as-record`                                               | a record verb (`read`, …) on a collection path                                                                        |
| `openidm-unknown-managed-type`                                                    | a write (`create`, `update`, `patch`, `delete`, `action`) to a managed type the environment profile does not declare |
| `openidm-query-filter`, `openidm-query-id`                                        | a `_queryFilter` the harness cannot decide: one the shared CREST grammar (`src/bindings/rhino/crest-filter.cjs`) cannot read, an empty or non-string one, an operator the matcher does not evaluate (`gt`/`ge`/`lt`/`le` in the script tester), or `ne` from a script (its script text is unmeasured); in either lane, a record whose answer turns on letter case, spaces, Unicode normalisation, a type mismatch (decided in the IDM emulator only where measured: the quoted integer on `alpha_user`, the 2026-11-06 integer rule on `frIndexedInteger1`, and a quoted literal against a number on a type `repo` declares generic), a decimal literal against a number outside those, `pr` on `""`/`[]`/`{}` (except an explicit `[]` on the three measured `alpha_user` properties, emulator only), a pointer through an array anywhere but a bare `eq` reached through `and` alone, or an `alpha_user`/`bravo_user` field whose DS matching rule is not caseIgnore (`telephoneNumber`, the date and integer families) beyond the literal forms and operators the 2026-11-06 probe measured on `alpha_user` — in the script tester, every such field, and before any record is read; an escaped quote anywhere but a bare `eq` reached through `and` alone; a `_queryId` other than the measured ones. Only measured or certain rejections are the catchable parse error, in both lanes |
| `openidm-failure-reason`                                                          | `e.javaException.getReason()` on an injected `openidmFailures` code outside CREST's reason table (`src/idm/errors.ts`); `getCode()` still answers the stub's code |
| `openidm-fields-wildcard-with-path`, `openidm-fields-non-object-parent`, `openidm-fields-unresolved-ref`, `openidm-fields-relationship-deep-path` | `_fields` projections that were not measured: `*` with a child of anything but a relationship array, a child of a scalar, `null` or an array of non-references, expanding a bare reference, and a path below an object field or envelope key of a relationship element, or more than one segment past a non-`*` name. Each names the operation, resource, selector, parent kind and the script's call site. Measured has-many projection: `docs/api/12` |
| `identity-attribute-layout`, `identity-write-idm-field-name`, `identity-write-cn`, `identity-set-attribute-non-array`, `identity-date-rendering`, `identity-wrapper-after-recreate` | `idRepository.getIdentity()` reads and writes outside the measured mapping |
| `identity-bag-key-collision`, `identity-bag-replacement-overlap`, `identity-bag-non-object-access`, `identity-store-non-object-bag`, `identity-store-id-layout-change`, `identity-store-two-ds-rules` | `fr-idm-custom-attrs` bag shapes that were not measured                     |
| `json-value-constructor`                                                          | `new JsonValue(...)`                                                                                                  |
| `seed-cache-manager`, `seed-binding`                                              | seeding a binding the harness cannot seed                                                                            |
| `idm-runtime-script/<path>`, `idm-runtime-script-path`, `idm-runtime-script-provider` | IDM emulator: one of IDM's own runtime scripts (a hook's `file`, or `require(id)` as `<id>.js`) that the project's `runtimeScripts` provider did not supply — including a name that matches a provider file only case-insensitively, or names a directory; a name that is not a canonical path under the script root (absolute, `.`/`..`/empty segment, trailing slash, backslash, NUL) or reaches a symbolic link in a provider directory, including a file or directory replaced by one after construction (checked at read); a loader that throws or returns a non-string, or a listed file that cannot be read (E15; README "IDM runtime scripts") |
| `idm-script-file-and-source`                                                      | IDM emulator: a script declaring both `file` and `source`                                                            |
| `idm-policy/not-null`                                                             | IDM emulator, no `policy` evaluator (E15a): an explicit `null` on a property with a non-null `default` outside the measured case (a create of a new record), which is emulated as AIC's `403`: `NOT_NULL`, plus `VALID_TYPE` when the schema has no `required` key (2026-11-08). Covers an update, a PUT or create over an existing record, and a refusal naming several properties, whose order is unmeasured. A PATCH `replace` with `null` never gets here: it is the measured `400` "not a valid JSON patch" (below) |
| `idm-patch/add-null`                                                              | IDM emulator: a PATCH `add` whose `value` is `null` (a script's `undefined` arrives as `null`), or that has no `value` key at all, which both lanes treat as `null` (inferred, not measured; astra-r1 P2-3). AIC refuses `replace` with `null` as `400` "not a valid JSON patch", before policy, on a defaulted boolean and on an undefaulted has-one relationship (docs/api/10), and that is emulated for every property, checked over the whole patch before the record lookup, the revision check, read hooks and the capability gate (CREST parses the body before routing; assumed for a script's call). So it also answers on a relationship type, which the gate faults for every other write. What AIC does with `add` is unmeasured. A `replace` with no `value` key gets the same 400 at the same point. Only the status and the phrase are measured; the rest of the 400's message is local |
| `idm-policy/required`                                                             | IDM emulator, no `policy` evaluator: a PATCH `remove` of a property that declares a `default`, `null` included. Stock `policy.js` adds an implicit `required` for any default; AIC's `403 REQUIRED` was measured only with the property also in `schema.required` (which the capability gate faults), so this case is unmeasured (docs/api/99 Q18). Removing an undefaulted optional property is the measured 200 (2026-11-08) |
| `idm-policy/valid-type`                                                           | IDM emulator, no `policy` evaluator (E15a): any mismatch outside a create of a new record; any value or `null` for a property with no `type`; an undefaulted `null` outside a create when the schema has no `required` array; a non-string's `default: null` when the schema has no `required` array; a `null` sent to a null-defaulted array outside a create; and a refusal naming several properties. Emulated, not faulted (measured 2026-11-08, docs/api/10): a create's top-level mismatch against `string`/`boolean`/`number`/`integer`/`object`/`array` (any number on `integer`, zzemu_nd) and its `null` with no `required` key are `403 VALID_TYPE`; a `default: null` passes a sent `null` with no `NOT_NULL`, and on an array stores `[]`; an undefaulted `null` with a `required` array not naming it is stored; nested `properties`/`items` are not checked |
| `idm-policy/valid-array-items`                                                    | IDM emulator, no `policy` evaluator (E15a): an item of a **top-level** array whose JSON type is not the declared `items.type`. Unmeasured: the 2026-11-08 probe measured only nested `items` (stored unenforced), never a top-level array's own items |
| `idm-exception/unmeasured`                                                        | IDM emulator (sol-r2 P2-3): a script reading a member of a caught tenant exception (`e.javaException`) that no probe read — anything but `getCode()`, `getReason()`, `getDetail()` and `toString()`, **including reading `getClass`** (sol-e15ab r6) — or `in` with any name but those and `getClass`, or enumerating its members (`Object.keys`, `for…in`, `JSON.stringify`; sol-r3 P2-2), or `getReason()` on an error that carries no reason phrase, or the string form of a `getDetail()` holding a spelling no capture showed (an empty object or array, a string needing an escape, a non-integral number). On AIC `e.javaException` is a CREST `ResourceException` subclass; the emulator's is not, and reproducing the class would mean shipping the vendor class, and a hand-built stand-in for `getClass()` and its `Class` stayed distinguishable from Rhino's native method and class through three review rounds (function names and source, arity and receiver checks, identity, the Java string `getName()` returns, engine diagnostics that read the implementation class), so `getClass` faults on read even though `getClass().getName()` is measured equal to the class the message names (403 and 404, probe-pol and probe-def, 2026-11-08). Match on `e.message`. **Known limit, not gated (sol-e15ab r7):** the gate is on member lookup, so a native Java method borrowed from another object — `new java.lang.Object().getClass.call(e.javaException)`, `Function.prototype.call`, `Method.invoke` — has Rhino unwrap the receiver, and passing it as a Java argument (`Class.isInstance(e.javaException)`) unwraps it too; native code then answers from the emulator's own exception (`HostCalls$HostException`) — its class, membership, or an unadmitted method such as a borrowed `Throwable.getMessage` — with no fault. No observed script does this; closing it would mean gating Rhino's receiver unwrapping or withholding `javaException`. `String(getDetail())` is the captured one-line form, and a 404's detail is a Java null (measured for an unavailable type; assumed for a missing record) |
| `openidm-managed-hook/<type>.<hook>`                                              | script tester (E15b): an unstubbed `openidm.create`/`update`/`patch`/`delete` on a type whose environment profile declares a hook that write runs on AIC. `<hook>` is the first in the measured order (docs/api/24: an update or patch runs its read hooks before `onUpdate`; property `onValidate`/`onStore` run before the object's), or `<property>.<hook>` for a property hook. Read hooks (`onRead`, `onRetrieve`, object or property) count only for an update or patch, which read the record first; a create runs none and a delete runs `onDelete → postDelete → onSync` (docs/api/24). Unknown hook names count for every write |
| `openidm-managed-unmeasured-hook/<type>.<path>.<hook>`                            | script tester (astra-r1 P2-1): any unstubbed `openidm.create`/`update`/`patch`/`delete` on a type whose profile declares a property hook **below** a top-level property, under `properties` (`box.p.onValidate`) or `items` (`box[].onValidate`), at any depth; `<path>` names the first. Whether AIC runs a nested property hook, on which writes and in what order, was never measured, so unlike a top-level property hook it faults a delete too. A measured hook the write runs (`openidm-managed-hook/…`) is reported first. The IDM emulator refuses the same config as `nested property behavior <hook>: <type>/<path>` |
| `openidm-managed-policy/<type>`                                                   | script tester (E15b): the same, on a type that declares no such hook but declares policies (`required`, an `enum`, property `policies`, `conditionalPolicies` or `fallbackPolicies`, at the top level or nested under `properties` or `items`, where a property's own non-empty `required` counts too). A delete is not judged on policy. Also, on a hookless, policy-free type, an implicit-policy outcome the lane does not emulate: a `null` or top-level type refusal outside a create of a new record, a `null` sent to a null-defaulted array outside a create, a top-level array item of the wrong `items.type`, a `null` on a nullable property (or, with no `required` array, on a `relationship`/untyped one), and a PATCH `remove` of a property with any `default` (see `idm-policy/required`). The measured refusals on a create of a new record (defaulted `null`, `null` with no `required` array, top-level mismatch against `string`/`boolean`/`number`/`integer`/`object`/`array`) are emulated as AIC's catchable `403 Policy validation failed` |
| `openidm-managed-profile-stale`                                                   | script tester (E15b): any write under a profile whose `profileVersion` is not an integer at or above the package's (3): absent, older, or a string such as `"3"`. Such a profile cannot say a type declares no hooks, defaults or conditional policies, and before version 3 it erased a `type` list naming two concrete types and defaults nested under `properties`. Re-pull it; for a hand-written profile, add `hooks` per type (and `noRequiredArray: true` where the schema has no `required` array) and `default`/`conditionalPolicies`/`fallbackPolicies` per property where the tenant has them, record a two-concrete-type list as `"type": "any"` with `types`, copy nested `properties`, then set `"profileVersion": 3` |
| `openidm-managed-invalid-default/<type>.<property>`                               | both lanes (sol-r1 P1-1, widened sol-r1c): the profile or config declares a `default` that `src/declared-default.ts` judges invalid (a top-level type mismatch, any numeric default on `integer`, or an array item not of `items.type`; an object default's `properties` and a `null` on any type are not mismatches). AIC stores such a config and keeps serving its last good compile for **every** managed type (measured 2026-11-08, docs/api/10), so the script tester faults every `openidm` call on any `managed/…` resource (`read`, `query`, `create`, `update`, `patch`, `delete`, `action`), and the IDM emulator refuses the config before every request. `openidm-managed-unmeasured-default/<type>.<property>` is the same for a default no probe covered (a `type` list naming two concrete types, a `relationship` or untyped default, arrays of arrays, a `default` nested inside `items` or `properties` in the script tester; the IDM emulator reports a nested default as `nested property behavior default`). Fix the default on the tenant and re-pull |
| `openidm-patch-add-null`                                                          | script tester: `openidm.patch` with an `add` whose `value` is `null`, `undefined` or absent (see `idm-patch/add-null`). A `replace` with any of those is AIC's invalid-patch refusal, emulated as a catchable error before the record lookup, the revision check and any hook or policy fault |
| `openidm-policy-detail`                                                           | script tester: `e.javaException.getDetail()` on an emulated policy `403` (`NOT_NULL`, or `VALID_TYPE` for a top-level mismatch or a `null` with no `required` key). AIC's detail is measured over REST and from an IDM script (docs/api/10) and from a next-gen script for `NOT_NULL` (docs/api/12); its Java type and string form on this path are not |
| `openidm-managed-collection-action/<action>`                                      | script tester (E15b): `patch`, `triggerSyncCheck` or `updateLastSync` on `managed/<type>` with no `given.openidmActions` reply. The names are measured; the result, and the write chain or sync it runs on AIC, are not |
| `idm-implicit-sync/<mapping>`                                                     | IDM emulator: a committed managed create, update, changed patch or delete on a type that is the `source` of a mapping in the supplied `config/sync` (`ConfigInput.sync`). On AIC that write runs the mapping (implicit sync, docs/api/16-sync-mappings.md): synchronously by default (measured 2026-06-19), asynchronously from the sync queue when the mapping enables `queuedSync` (measured 2026-07-29); what `enableSync: false` does is unmeasured. The emulator models none of these, so the write faults before the store changes, and the message names which case each mapping is. `sync` is a required input (E15c); `{ mappings: [] }` is the caller's explicit none. **The script tester's AM lane is silent here**: `runtime.cjs`'s `openidm.create`/`update`/`patch`/`delete` write `given.managed` with no implicit-sync check and record nothing, so a write there on a mapping's source passes; routing those writes through an `IdmEmulator` (E15 round-1 Q2) would inherit this fault |

**Argument counts, by provenance.** A call with an argument count AIC has no
overload for fails on AIC as Rhino's catchable
`InternalError: Can't find method <java class>.<method>(<argument types>).`
The harness reproduces that, with Rhino's argument-type naming, wherever the
accepted counts have a basis:

- **measured** on AIC: `existingSession.get` (`docs/api/12`),
  `utils.crypto.randomUUID` (corpus `binding-utils-interop`),
  `nodeState.mergeShared` (one argument, and the message's class), and
  `logger.debug`/`info`/`warn`/`error`, which are true varargs, each
  argument filling a `{}`, and an object prints as `[object Object]` (live
  probe 2026-11-06, `docs/api/12`). Measured: 4–6 arguments and a number's
  `1.0` form. The rest follows structurally — any other count from SLF4J's
  varargs signature, other numbers (`1.0E7`, `1.0E-4`) from Java's
  `Double.toString`, which the formatter matches on 24 values. `logger.trace` was not probed and keeps the
  overload range. The tenant's other
  measured `Can't find method` refusals (`getMemberships()`, `setAct(object)`,
  `getRandomValues(object)`) are type refusals, not argument counts.
- **assumed** from AIC's binding introspection
  (`docs/api/bindings/scripted-decision-next.json`): the overload set of each
  `nodeState`, `action`, `callbacksBuilder`, `callbacks`, `secrets`, `journey`,
  `openidm`, `cacheManager`, `logger`, `httpClient.send`,
  `systemEnv.getProperty`, `idRepository.getIdentity` and
  `utils.crypto.getRandomValues` method the overlay checks. That Rhino refuses an
  unlisted count there is assumed from the measured cases, not measured per
  method, and so is the class the refusal names: the introspection's declaring
  class, where Rhino might name an implementation subclass.
- **not decided by the overload set**: where an overload's last parameter is
  listed as `array` (`logger.*`'s `(format, arguments)`, every `openidm`
  method's `fields`, `getRandomValues`, the attribute-input and validated
  callbacks' `failedPolicies`), introspection does not say whether it is a Java
  array or varargs, and Rhino calls varargs with any count from one fewer
  upwards. A count at or above that (`arrayTail` in the basis table) that no
  listed overload takes is the fault `arity-unknown/<path>`, not a refusal and
  not an acceptance — `logger.info` with four arguments was accepted before
  E19 fix round 2, `openidm.read` with four refused catchably.

Everywhere else the accepted range is the harness's own guess — the request
maps' `get`/`containsKey`, `existingSession.put`/`containsKey`, the identity
wrapper's `setAttribute`/`addAttribute`/`store`, `secret.getAsUtf8`, `require`,
the legacy `sharedState`/`transientState`, and every check on the legacy
engine, which has no introspection — and a count outside it is the harness
fault `arity-unknown/<path>`. The basis table (`__rhinoLocalArityBasis` in
`runtime.cjs`) is pinned against the introspection file by
`test/bindings/arity-provenance.test.ts`.

**Hypotheses from on-prem jars, each settled by one live probe.** The E19
round-3 review read the AM 8.1.1 binding jars with `javap` and ran the CREST
27.1.1 `QueryFilters.parse` from a ForgeOps AM image over 112 filters. On-prem
signatures are a hypothesis in this project, not an AIC measurement (IDM's
bundles import CREST `[28.0,29)`, a neighbouring version), so each of these
stays a fault until a probe on AIC decides it:

- **Settled 2026-11-06: `logger.*` is true varargs** on `debug`, `info`,
  `warn` and `error` (4, 5 and 6 arguments accepted at all four; the
  `(format, [array])` form binds the array's elements). The
  `arity-unknown/logger.*` faults are gone for those four; `logger.trace`
  was not probed. `logger.warning` is AIC's catchable `TypeError: Cannot find
  function warning`, which the overlay already gave.
- **The other `arrayTail` parameters are probably not varargs.** The jar types
  `openidm`'s trailing parameter as `java.util.List`, the validated and
  attribute-input callbacks' as `List<String>`, and `getRandomValues`' as a
  `NativeArray`, so a count past the overloads is likely Rhino's catchable
  refusal. The fault there is noise in the safe direction.
- **Settled 2026-11-06: `mergeShared` takes a `Map`.** A non-object is a
  catchable `InternalError` in three measured shapes (see the fault table),
  and state is unchanged. `mergeTransient` was probed with a string only;
  its array, number, boolean and `undefined` shapes are Rhino's own overload
  and conversion refusals on the same `(Map)` signature, so they are taken
  from `mergeShared`'s. `null` is not: it passes resolution and fails inside
  AM's method body, never observed for `mergeTransient`, so
  `mergeTransient(null)` stays `node-state-merge-non-object`, as does a
  function.
- **The filter grammar faults on inputs CREST 27.1.1 accepts**: keyword case
  (`AND`, `Or`, `TRUE`, `PR`, `EQ`), single quotes, `+5`, `05`, `1.`, `.5`,
  the pointers `/a~2`, `text~`, `/` and `!text`, `"a"and`, `\x` escapes, a tab
  inside a pointer, and integers between 2^53 and `Long.MAX_VALUE` (the last
  on purpose: a JS number cannot hold the long CREST compares). Conversely the
  grammar now refuses, as the fault, all eleven inputs the jar rejects that it
  used to accept (exponent with no `.`, integers past `long`, tab or newline
  separators, `!` glued to a keyword). One probe of uppercase `AND` and of
  `'x'` on AIC would show whether AIC's CREST matches 27.1.1; if it does,
  transcribing its `QueryFilterParser` would retire every grammar fault.

**Journey lane.** In a journey, AM's host bridge replaces the fault channel
with its uncatchable `unsupported` refusal. The emulated AIC errors above stay
catchable there too. The ids still reach the tally: a
generated stub reports `not-mocked/<behaviour>`, and a script-tester refusal
reports its own id. The IDM emulator's `_queryFilter` refusals, the
record-level ones included (since E19 round 5), report
`openidm-query-filter`, and an IDM runtime script the provider did not supply
reports `idm-runtime-script/<path>` (E15), and a write that would run a sync
mapping reports `idm-implicit-sync/<mapping>`; its other `missing` sites, and the
journey emulator's, have no ids yet. The tally lists them under `unidentified`, keyed by cause text, and
giving them ids is a per-site change in `AmHostBridge.java` and
`src/journey/engine.ts`.

## Known gaps (measured 2026-09-28)

Each is a `gap` entry with its reason; this is the index.

| Case                          | AIC                                                                                          | Local                                                              |
| ----------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `enum-callbacks-utils`        | `callbacksBuilder`/`utils` are Java objects; for-in lists `class`, `getClass`, `wait`, …     | plain JS mocks enumerate only their methods                        |
| `enum-utils-sub`              | same for `utils.base64`/`crypto`/`types`; varargs callbacks have arity 0                     | JS methods, declared arity 2                                       |
| `request-multivalue`          | Java maps: `getClass()` → `Access to Java class "java.lang.Class" is prohibited`             | JS object: `Cannot find function getClass`                         |
| `request-headers-dump`        | same, plus Java map formatting from `String()`                                               | JS object                                                          |
| `httpclient-body-coercion`    | `java.lang.Integer`/`Long` body fields are sent                                              | the mock drops them                                                |
| `java-collections`            | `new (JavaImporter(java.util)).HashSet()` works                                              | throws                                                             |
| `identity-resolve-diag`       | `getIdentity` resolves a UUID only; payload committed abbreviated                            | same resolution; emits the full `attr-error: <message>` text       |
| `identity-getattribute-shape` | Java collection: no `includes` member (`Cannot find function`), prints `[{}]`                | `toArray`/`contains`/`[{}]` match; shadowed `includes` (`Cannot call property`) |
| `identity-enum-attrs`         | **not measured** — every count came back `err`; the probe's IDM setup users are gone         | —                                                                  |

`.includes()` on `getAttributeValues` and `getClass()` on a request map were
once dangerous — they worked locally and failed live. Both now throw on both
lanes and differ only in the message. `identity-attr-mapping` left this table
when the local identity binding gained AM's `uid` mapping; it now matches.
The custom bag no longer relies on an AM-named property seeded in IDM: the
2026-10-02 populated-bag measurement is implemented as a projection of exactly
`custom_*` properties, including boolean JSON types and whole-bag store behavior.
Equivalent explicit bag seeds remain accepted for 0.2.0 compatibility; conflicting
ones fail. The `identity-getattribute-shape` row now derives its empty `{}`.

Two request cases are also **not comparable** beyond their `getClass` row: the
live run was a curl with its own headers and parameters, not the case's
`given`, and its payload carries the tenant host and client-certificate
headers, so it is not committed.

`identity.store()` follows the layouts measured 2026-10-01
(`live-identity-store-families`, `docs/api/14` "What `store()` writes"): every
`fr-attr-*` family by one representative, plus `givenName`, `sn`, `mail` and
`telephoneNumber`, including AM's own `IdentityUpdateException` for the
refusals DS makes. The 2026-10-02 `identity-custom-attrs.script.js` measurement
adds `fr-idm-custom-attrs`: whole-bag replacement, deletion of omitted custom keys,
unknown-key persistence, no coercion, atomic errorcode 65/21 failures, and the
`[]` versus `["{}"]` read distinction. The `identity-custom-attrs-edges.script.js`
fixture at `8f4ff66` extends this to every JSON shape, unprefixed object keys,
malformed-text precedence, full IDM read failures for string/number/array bags,
and ordinary patch/update restoring
an absent bag. `identity-custom-attrs-nonobject.script.js` at `f7d684b` adds
boolean full-read failure and string delete failure (user retained; REST 500),
repaired by an AM `["{}"]` write. Persisted AM values and object-key ownership
travel across passes in validated full-resource metadata. Historical keys in
`identityCustomAttrsOwnedKeys` survive alias and whole-bag clears independently
of current AM values, so declared re-adds and later whole-bag removals retain the
same storage location. Local create/delete discards prior resource history.
Given/effect validation rejects reserved/underscore and ordinary/OOTB current or
historical bag keys using the same canonical policy asset/inventory/checker as
Rhino. Raw-text loading isolates Rhino from host coverage transformations. IDM
patch/update also check synchronization before any record or metadata mutation.
Profile checks exempt currently projected valid bag properties while checking
ordinary properties and enum values strictly. No layout declaration is needed.
What remains:

- **Non-object bag IDM access** is category-specific. Full reads of strings,
  numbers, arrays and booleans throw the measured adapter error; null permits
  a full read. String delete throws without mutation. Number/array/boolean/null
  deletes, boolean queries, projected reads, other query shapes and other IDM
  mutations are unmeasured and refused. `_id`-only filtered queries remain
  supported for string/number/array/null bags.
- **Identity-state collisions** are unmeasured and refused before persistence:
  metadata keys and known ordinary/OOTB identity targets cannot be bag keys;
  a whole-bag replacement and another AM attribute write cannot target the same
  property in one store. Declared layouts retain precedence over inferred and
  persisted bag contents: custom-key writes update the object bag and record
  together, including after an absent `[]` clear. Unused declarations permit
  bag clearing. Key replacement within a non-object bag and changing the
  identity resource through an `_id` layout remain unmeasured and refused.
- **Public check-handle materialization of non-object bags** is unmeasured.
  Both handles receive provenance and refuse affected reads/queries (including
  null), while string deletes report REST 500 and other non-object deletes
  refuse. Their object-bag success path remains supported.
- **Reference projection beyond pre-expanded object parents** is unmeasured.
  Scalar/null/array traversals, unresolved bare references and expansion refuse;
  only the seven documented envelope fields are implicitly included, rather
  than every underscore-prefixed sibling. Child selectors are checked before
  wildcard returns. Combining `*` with a child selector is unmeasured and refused
  even for pre-expanded parents, including comma-joined fields and params `_fields`.
- **Bag lifecycle on create/delete-recreate** is unmeasured. Locally old metadata
  is cleared after successful operations and fresh wrappers derive the recreated
  seed's bag; retained wrappers refuse. Update refreshes retained wrappers from the current record.
- **`cn`** is refused locally. AM stores it outside the IDM managed record,
  and the local lane holds only that record, so a `cn` written in one pass
  could not be read back in the next. A local `cn` *read* returns the seeded
  record's `cn` (normally none), where AIC derives `"<givenName> <sn>"`.
- **Family members other than the representative** (`fr-attr-istr2`..`20`,
  …) are assumed to match it, not measured.
- **Any other attribute** throws until the suite declares its layout in
  `identityAttributes`. The declaration is trusted locally and checked only by
  conformance, through whatever the script reads back.
- **An unset IDM property** reads `null` on AIC (the property is in the
  schema) and is missing locally (`undefined`), including after `store()`
  removes it. The families probe compares the two as "absent".

With a session requested, AM lists the session cookie in `requestCookies` as
a string (measured 2026-10-01, `live-session-cookie`). The local lane adds it
under `cookieName` with a fixed placeholder value, so presence, count and type
match; the value is per-run on the tenant and never matches. Both lanes refuse
an author cookie with the session cookie's name.

`JsonValue` prints the measured forms on both lanes
(`test/harness/live-json-value.e2e.test.ts`): `object()` after `put("a", "b")`
prints `{ "a": "b" }`, and `json(object())` prints `{  }`. Locally `json(x)`
returns `x`, so those two match because `object()` already prints as a map.
`json()` of anything else — a JavaScript object literal, an array, a scalar —
is **unmeasured** on AIC, and locally prints whatever `x` prints.
`JsonValue` is reachable through `JavaImporter(org.forgerock.json.JsonValue)`
or `JavaImporter(org.forgerock.json)`, as a class (`typeof` is `"function"`),
and is `undefined` on `JavaImporter()` or `JavaImporter(java.util)` (measured
2026-10-01, `live-java-importer-scope`); the local lane matches all four.
`object()` is the same local Java-map model as a registered `objectAttributes`.
Both follow `live-java-map-enumeration` (2026-10-01): an entry named like a
method shadows it, `for…in` and `Object.keys` list every entry, `keySet()` is
refused, and `entrySet()`/`values()` are snapshots whose entries and iterator
respectively are refused (`docs/api/12-script-bindings-matrix.md`). An
earlier local model kept method-named entries apart from the methods; that was
never measured, and AIC does the opposite. Not modelled: anything on an
`entrySet()` view beyond `size`, `iterator().hasNext/next` and `toArray`;
`for…in` over a view (AIC enumerates the Java object's members); a key named
`toString` or `toJSON` (the local map uses those to print and harvest).
The legacy result classes `Action` and `HiddenValueCallback` are `undefined`
on next-gen from every importer — `JavaImporter()`, `JavaImporter(java.util)`,
and even one naming `org.forgerock.openam.auth.node.api.Action` or
`com.sun.identity.authentication.callbacks.HiddenValueCallback` (measured
2026-10-01, `live-java-importer-scope`); the local lane matches all six.
Locally the legacy engine exposes each one only to an importer that names its
class or its package. That legacy scope is **unmeasured**: the AIC wrapper
emit is next-gen only, so no legacy script reaches the tenant lane.

What the 2026-09-12 ranking called the shutter gap is closed:
`java-class-shutter`, `for-each-java-collection` and
`lib-java-collections-consumer` now equal the live payload exactly, as does
`logger-placeholders` (its `E0-shape` row was the only miss, and it was the
expectation that was incomplete). The five legacy cases were dropped on
2026-09-12 (`7bb0195`); legacy scripts are not supported.

### Unlabelled query matching and send bodies (listed 2026-11-04)

Two script-tester behaviours disagree with, or go beyond, what was measured,
and neither carries a label:

- **Query filters: case, spaces, Unicode, types, presence and quotes.**
  `docs/api/10-managed-objects.md` measured `eq` on `alpha_user` ignoring
  letter case and leading/trailing spaces, a number field matching a quoted
  integer (`frIndexedInteger1 eq "7"`), and an escaped quote never matching
  in a bare `eq`. Since E19 fix round 4 both lanes run one evaluator
  (`src/bindings/rhino/crest-filter.cjs`, which also holds the grammar):
  `openidm.query` over `given.managed` in `runtime.cjs`, and the IDM emulator
  through `src/idm/filter.ts`, which adds only its per-field string policy. A
  leaf is undecided when its answer changes under any case, space or Unicode
  reading; when it compares different JSON types; when it reads an object or
  a `null` array element; when it asks `pr` of `""`, `[]` or `{}`; when a
  decimal literal (`7.0`, `6.5`, `1.5e3`; the parsed node keeps its spelling)
  meets a number on a field with no measured rule; when its pointer steps through an array, except a bare `eq`
  reached through `and` alone, the measured shape; or when the field is one of
  the user attributes below. The decided type mismatches are a number field
  against the canonical spelling of a safe integer, only in the IDM emulator
  on `alpha_user` (not `custom_*`), and the measured rules below; the script
  tester knows no types and leaves them undecided. `and`/`or`/`!`
  follow Kleene logic, and a record left undecided is the
  `openidm-query-filter` fault in both lanes (the emulator's refusal carries
  the id since round 5). An escaped quote is decided `false` only in an `eq`
  reached from the root through `and` alone, where "the leaf is false" and "the
  whole query matches nothing" agree; anywhere else the filter is the fault.
  The script tester has no per-field policy, so a case-different value that AIC
  is measured to match is a fault there too, in the safe direction. The
  emulator materialises an absent array property as `[]` and remembers which
  ones it injected; a query reads those as absent, as DS reads an unset
  multi-valued attribute, so only an explicitly written `[]` is undecided under
  `pr` — except on the three `alpha_user` properties measured below. Same-type number and boolean decisions carry an `assumed` label, except
  `eq` on `alpha_user` `frIndexedInteger1`, which is measured. The harness
  `check idm.query` (`src/harness/idm.ts`, exact `JSON.stringify` equality)
  still compares exactly.

  The Unicode readings follow RFC 4518, which DS's `caseIgnoreMatch` cites:
  NFKC, and the characters it maps to nothing (soft hyphen, zero-width space,
  joiners, variation selectors, other controls and format characters).
  **Omitted:** the astral-plane characters in that map-to-nothing list (for
  example U+E0001 and the U+1D173 musical controls), because the reading
  works on UTF-16 code units, and RFC 4518's prohibited-character and
  bidirectional checks. A value holding one of those is compared as it is, so
  a filter whose answer would turn on one is decided rather than refused.
  None of these readings is measured on AIC; each is a reason to refuse, not a
  rule that is applied. Since round 5 the case readings also include Java's
  per-character lower-casing (U+0130 to `i`) and a full fold (U+1E9E to `ss`).

  **User attributes: measured rules (live probe 2026-11-06).** DS's core
  schema (opendj-core 8.1.1, RFC 4519) gives `telephoneNumber` `EQUALITY
  telephoneNumberMatch`; `frIndexedDate1`–`5` / `frUnindexedDate1`–`5` are
  GeneralizedTime and `frIndexedInteger1`–`5` / `frUnindexedInteger1`–`5`
  integers (`docs/api/14`). The probe (`docs/api/10`, REST and IDM's
  `openidm.query`, which agreed on every case) measured these on
  `alpha_user`, and the IDM emulator applies exactly them, labelled
  `measured`:
  - `telephoneNumber`: spaces, hyphens and `+` are ignored anywhere, in the
    stored value and the literal, for `eq`, `sw` and `co` (`+` settled by the
    second probe: `eq "61499999999"`, `eq "6+1499999999"` and `co "+9999"`
    match `+614-9999-9999`, `eq "61499999998"` does not). Decided only when
    both hold nothing but digits, `+`, space and `-`, and the literal holds a
    digit; parentheses, dots and ordering were not probed.
  - `frIndexedDate1`, `frUnindexedDate1`: `eq`, `gt` and `lt` compare
    instants across offsets, for literals and values of the form
    `YYYY-MM-DDThh:mm:ss` with `Z` or `±hh:mm`. `eq "notadate"` is a 400
    `illegal filter assertion value`. `ge`/`le`, fractional seconds,
    date-only and GeneralizedTime-form literals were not probed.
  - `frIndexedInteger1`: an integral literal of either JSON type (`7`, `"7"`,
    `7.0`, `7.00`, `"7.0"`) equals the integer, and `ge 7.0` orders by it;
    `eq 7.5`, `lt 7.5`, `gt 6.5`, `eq "7.5"` and `eq "abc"` are the 400.
    Other operators with a fraction, exponents, booleans and `co`/`sw` stay
    undecided.
  - `frIndexedMultivalued1`, `frUnindexedMultivalued1`, `roles`: an
    explicitly written `[]` is not present under `pr`.

  **The 400 and the refusal are query-level.** AIC converts each assertion
  to its attribute's syntax before reading a record, so the emulator raises
  the 400 (HTML-escaped over REST, a catchable `BadRequestException` in a
  script) on an empty collection too, and refuses any literal a ruled field
  cannot be judged on before reading a record. A per-record refusal would
  let a fixture with no matching rows pass where AIC throws. When a filter
  holds both, the refusal wins: which leaf AIC reports first is unmeasured.

  **Still refused, by name:** `bravo_user` (none of it was probed), and on
  `alpha_user` `frIndexedDate2`–`5`, `frUnindexedDate2`–`5` (every
  comparison) and `frIndexedInteger2`–`5`, `frUnindexedInteger1`–`5` (any
  literal but a plain or canonical quoted integer). The probe note suggests
  the families share the measured rule; the emulator does not assume it.

  **The script tester (AM's `openidm.query`).** The second probe ran the
  rules through AM's next-gen binding and got exactly the REST rows, so the
  script tester decides the same `alpha_user` rules. Its grammar evaluates
  `eq`, `co` and `sw` everywhere and ordering only on the two instant fields
  (`gt`/`lt` measured there); integer ordering stays the fault, as it was
  not measured through this binding. A literal AIC cannot convert throws
  AM's measured surface: a catchable `JavaException` whose message is
  `org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: `
  plus REST's message, HTML escaping included (`&#39;`), with no
  `(script#line)` suffix, and `e.javaException.getCode()` 400 /
  `getReason()` `Bad Request`. It has no `repo`, so it has no JSON-storage
  rule and no quoted-integer reading on other fields. Everything unmeasured
  is still the fault, raised before any record is read.

  **Emulated `InternalError`s carry no `(script#line)` suffix; AIC's do.**
  The second probe's control (`openidm.read` with four arguments) put
  `(<script name>#<line>)` inside AIC's `e.message`, not only in
  `String(e)`. Every `InternalError` the harness throws (arity and merge
  refusals among them) lacks it, so a test asserting an exact `e.message`
  passes locally and fails on AIC; match a prefix instead. A
  `JavaException` (the 400 above, the `ResourceExceptionScriptAdapter`
  family) has no suffix on AIC either. `mergeShared(null)`'s `null`
  carries none on AIC (`docs/api/12`).

  **Emulated `JavaException`s enumerate like AIC's, with other values.**
  `for (p in e)` gives `fileName,lineNumber` on the JVM, as on AIC, and
  nothing on node:vm; `name` is not enumerable. The values are not the
  script's (the JVM lane's are `""` and `0`), and `e.rhinoException`, an
  object on AIC, is absent in both lanes. `e.javaException` (`getCode()`,
  `getReason()`) is on the illegal-assertion 400 and on injected
  `openidmFailures`, whose code is the stub's; a revision mismatch and
  `Response is not application/json` have none, because their status
  through this binding was never captured.

  **Generic (JSON-stored) types (probe §4).** On a type DS stores as JSON in
  `ou=managed`, a quoted literal never matches a number (`eq`, `gt`, `lt`:
  no rows, no error) and numbers compare by value, `7.5` included. The
  emulator decides this only where it knows the storage: `ConfigInput.repo`
  takes the tenant's `config/repo.ds`, and `IdmConfig.storage` reads a type
  as generic when `genericMapping` names it or `managed/*` covers it and
  `explicitMapping` does not. A property its `genericMapping` entry maps to
  an attribute of its own is not JSON. With no `repo`, or a type in both
  sections, the faults stay. The probe's type had no entry of its own (only
  `managed/*` covered it), so a type with its own `genericMapping` entry is
  decided the same way but labelled `assumed`: its JSON keys and matching
  rule were not measured. **A partial hand-written `repo` is unsound**: one
  holding only `managed/*` makes every non-user type generic, including types
  the tenant maps under `explicitMapping` (doc 10 names `alpha_group` and
  `bravo_group`). Pass the tenant's whole `config/repo.ds`, as read. A quoted literal is decided `false` only in a
  leaf reached through `and` alone (the probe ran no negation); `ge`/`le`
  are `assumed` like `eq`/`gt`. `custom_*` on `alpha_user` (a JSON
  attribute inside a DS-mapped type) is still undecided: the tenant had none
  to probe.

  **Still open:**
  - `!(/<rel>/_refResourceId eq "<id>")`: whether the negated pointer through
    an array returns every record or none.
  - `givenName eq "istanbul"` against `\u0130stanbul` (written by code point):
    which lower-casing DS applies.
  - `custom_n eq "7"` on `alpha_user`, and every rule above on `bravo_user`
    and on the unprobed family members.
  - The same filters through AM's `openidm` binding, whose 400 text is
    unmeasured.
- **`httpClient.send` converts an object body as AIC measured, and fails on
  what it cannot reproduce.** `docs/api/12` §5 (2026-11-04) measured AIC's
  serializer. The recorded body is a snapshot taken at send time, and it
  follows that measurement:
  - `NaN`, `1/0` and `-1/0` are recorded as the strings `"NaN"`,
    `"Infinity"` and `"-Infinity"`.
  - A `java.util.Date` is recorded as its epoch milliseconds. Only `Date(0)`
    → `0` was measured, and every unit gives `0` there, so milliseconds is
    inferred for other instants.
  - A cycle of plain objects throws AIC's catchable
    `InternalError: Cannot produce JSON from [object Object]` and records
    nothing. Only a root self-cycle (`b.self = b`) was measured; other shapes
    are inferred to do the same.
  - `toJSON` is never called. AIC did not apply `Date.prototype.toJSON`; other
    `toJSON` methods are inferred the same.

  A JS `Date`, a `RegExp` or a function goes out from AIC as a 1–1.7 KB dump of
  the Rhino object, which the harness cannot reproduce. For a function, that
  dump includes the script's name, the function's encoded source, and the name
  of every binding and variable in the script. Such a send is a **harness fault**: it throws a `rhino-local:`
  message saying what AIC sends, and it fails the run even if the script
  catches the throw, because AIC would not have thrown. A cycle that passes
  through an array is treated the same way, since its message was not
  measured. So is an `Error` (`{error: e}`): what AIC sends for one is
  unmeasured, but it is a built-in like `Date` and `RegExp`, so AIC most likely
  dumps it too, and recording any guess could pass a body assertion AIC would
  fail. Still unlabelled:
  - `-0` is sent as `-0.0` but recorded as `0`, because recorded effects are
    JSON. This is the same lost precision as `1.0` recorded as `1`.
  - These conversions were measured under an object key only. Array elements
    and a top-level body reuse the same rule.
  - Other Java objects and other built-ins pass through JSON as before,
    unmeasured. A constructor instance is recorded as its own fields, where
    the journey emulator reports a non-plain prototype as missing.

  The journey emulator applies the same rules in Java. It labels them
  measured, labels the array case, a non-zero `java.util.Date` and a cycle
  other than a root self-cycle assumed, and reports the dump cases and an
  `Error` as missing.

## Binding probes (measured 2026-09-29)

Ten `binding-*` fixtures exercise the scripted-decision binding surface
member by member. Each case's expectation is the live payload, committed
verbatim, and the local payload is compared to it key by key. Three things are
not raw live values. `binding-openidm-writes` masks `_rev` and the generated id
in the fixture itself, on both sides. Java identity hashes (`@1a2b3c`) are
masked when a gap's local value is compared. `binding-callbacks-builder` pins
AIC's callbacks parsed from the `/authenticate` body, because that fixture emits
no payload. Every binding member is run
by at least one corpus case's origin fixture, or is excluded with a reason in
`test/corpus/member-coverage.test.ts`. The four exclusions are
`emailService.send` (it sends mail), `action.suspend` (it ends the probe), and
`callbacksBuilder.httpCallback`/`x509CertificateCallback` (AM cannot render
them as REST JSON).

| Case                        | Keys matched | What still differs                                                                                                                  |
| --------------------------- | ------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `binding-nodestate`         | 32/32        | — (final state not captured live, so state channels are not judged)                                                                 |
| `binding-createuser`        | 4/4          | —                                                                                                                                   |
| `binding-callbacks-builder` | callbacks    | — (AIC returned the raw callbacks; the case pins them parsed)                                                                       |
| `binding-utils-subtle`      | 19/20        | RSA encrypt given the whole key-pair map: both fail, with different Java messages                                                   |
| `binding-callbacks-getters` | 19/21        | `getChoiceCallbacks` is `int[]` on AIC (unconstructible through the shutter); `getConsentMappingCallbacks` has no seedable body     |
| `binding-utils`             | 24/29        | error suffixes name the mock, not the script; `checkBcrypt` unmocked (no bcrypt in the JDK); PBKDF2 JS-array salt message          |
| `binding-action`            | 13/16        | a mock-thrown `InternalError` carries no `(name#line)`                                                                              |
| `binding-utils-interop`     | 17/20        | AIC wraps as `JavaException ScriptCryptoException`; the arity refusal's text matches but lacks AIC's `(name#line)` suffix          |
| `binding-services`          | 57/80        | tenant-backed JWT and policy results; absent-user `getIdentity` wrapper's Java members; secret `JavaException`s; tenant script id in `getName`     |
| `binding-openidm-writes`    | 18/36        | AIC throws `JavaException` wrappers carrying the LDAP DN; `validateObject` answers from tenant policy; query errors as `Error`, not `JavaException` |

Two conventions keep these honest. An AIC `InternalError:` is reproduced with
`throw new InternalError(msg)`. An AIC `JavaException: …Adapter: <msg>` becomes
`Error("rhino-local: <method>: <AIC phrase>")`. The mocks never forge a Java
exception, so those keys stay gaps rather than being tuned to match. The full
reasons are the `gap` entries in `cases/real/index.ts`, and the measured
behaviour is in `docs/api/12-script-bindings-matrix.md`.

`given.bindings` accepts only `journey` (whose getters throw when unseeded) and
`cacheManager`. Any other name throws, so a case cannot quietly seed a binding
the harness does not model. `samlApplication` and `oauthApplication` are always
`null`, as measured on a journey not started by a SAML or OAuth2 flow. A journey
those flows start is not modelled.

## Where the real scripts are

| Source                                                                                | What it is                                                                | Verdict                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/rhino-script-tester/fixtures/*.script.js`                                    | Next-gen scripted-decision probes, run live through `AIC-Rhino-Let-Probe` | **This is the in-repo corpus.** Recorded outcomes live in `docs/api/12-script-bindings-matrix.md` and `docs/api/14-am-identity-attributes.md`. `tmp/rhino-script-tester/probe-results.json` is gitignored and was not in this checkout. |
| `scripts/rhino-script-tester/fixtures-legacy/*.script.js`                             | Legacy (`evaluatorVersion` 1.0) scripted-decision probes                  | Same kind, different engine. Converted with `given.engine = "legacy"`.                                                                                                                                                                  |
| `scripts/rhino-script-tester/scripts/*.script.js`                                     | Earlier `let` / `var` probes, same journey                                | Converted.                                                                                                                                                                                                                              |
| `scripts/rhino-script-tester/fixtures/*.lib.js`                                       | LIBRARY bodies, not decision-node scripts                                 | Not cases. Consumers `require()` them; suites carry library source by exact require name in `libraries`, which becomes `given.libraries` on each case.                                                                                     |
| `scripts/rhino-script-tester/fixtures-oauth2/`, `fixtures-atm/`, `fixtures-exchange/` | Validate-scope, access-token-modification, may-act                        | **Wrong script kind.** Different binding JSON, different invocation. Not converted.                                                                                                                                                     |
| `src/scripts/templates/`                                                              | Workspace `.d.ts`, ESLint, TypeScript endpoint seeds                      | **Not scripts.** No scripted-decision body ships to users from here.                                                                                                                                                                    |
| `scripts/type-tests/leaves/nextgen-decision-node/accept.cjs`                          | Type-acceptance fixture                                                   | Pins the next-gen decision-node contract (`action.goTo`, `outcome`, `nodeState`, `callbacks`, `idRepository`) at the type level, plus `openidm` / `logger` / `requestParameters`. Still not a runnable scripted-decision body — `tsc` only.                                                                                                            |
| The production script corpus (`.ai/local.md` names the checkout)                          | The 384 `src/` / 56 `lib/` production corpus cited in the matrix          | **Not on this machine.** `.ai/local.md` is absent.                                                                                                                                                                                      |
| Sibling `pingone-aic-manager/workspace/sandbox/am/`                                   | Pulled tenant scripts                                                     | Almost empty of decision-node scripts (two LIBRARY probes and one ATM script).                                                                                                                                                          |

Pushback, as invited: the probe fixtures are the richest source of
scripted-decision bodies _with recorded live behaviour_ in this repo. They are
probes, not production journey scripts. A production corpus would be the client
sandbox checkout named in `.ai/local.md`, and it is not here. Converting probes
is still the right move — they are the scripts whose live outcomes we can
actually assert.

## What was converted

57 cases in `packages/rhino-local/cases/real/index.ts`, each running its
origin fixture under `scripts/rhino-script-tester/`:

- 55 next-gen (53 `fixtures/*.script.js` + 2 `scripts/*.script.js`)
- 2 legacy

Identity cases originally named a sandbox test user and three managed-object
UUIDs. Cases rewrite those to reserved placeholders (`alice` / `bob` /
`00000000-0000-0000-0000-000000000000` and `…0001` / `…0002`) at load time, via
`Rewrite`s in `index.ts`.

A blocked case still asserts its exact throw, so implementing that method fails
the assertion and forces re-triage. Nothing in this corpus is currently blocked
on a binding method — only the nine parse errors.

## Parse errors — not binding gaps

Nine cases never reach a binding. Live recorded HTTP 401 / no
HiddenValueCallback. The case format has no `compile_error` channel, so they are
blocked on `(parse)` with the JVM's exact message. Already covered as
language-corpus rows in `docs/rhino-local-harness.md`; kept here so the
real-script set is complete.

| Case                      | Exact throw (2026-09-28)                                                             | Live (matrix)                       |
| ------------------------- | ------------------------------------------------------------------------------------ | ----------------------------------- |
| `rhino-let-behaviour`     | `compile_error: missing ; before statement (rhino-let-behaviour#8)`                 | parse: `missing ; before statement` |
| `for-of-var`              | `compile_error: missing ; after for-loop initializer (for-of-var#32)`                | same                                |
| `object-shorthand`        | `compile_error: missing : after property id (object-shorthand#12)`                   | same                                |
| `destructuring-object`    | `compile_error: missing : after property id (destructuring-object#11)`               | parse error                         |
| `default-params`          | `compile_error: missing ) after formal parameters (default-params#9)`               | parse error                         |
| `const-in-for-init`       | `compile_error: syntax error (const-in-for-init#13)`                                 | parse error                         |
| `const-in-for-in`         | `compile_error: syntax error (const-in-for-in#12)`                                   | parse error                         |
| `const-in-for-of`         | `compile_error: syntax error (const-in-for-of#12)`                                   | parse error                         |
| `const-dup-across-blocks` | `compile_error: TypeError: redeclaration of const dup. (const-dup-across-blocks#19)` | parse error                         |

The `const-dup` wording differs (`TypeError: redeclaration…` locally vs an
unspecified parse error in the matrix). Both sides reject the source. Line
numbers are the fixture's own. AIC does not report them (a failed parse is a
bare 401), so they are the local runner's, re-measured 2026-09-28.

The silent `const` bugs (`const-top-level`, `const-in-loop-body`, …) now pass as
language-fidelity cases on the bindings runner. Library top-level `const` also
passes (`lib-const-consumer`); loop-body `const` inside a library function still
yields `"0,0,0"` (`lib-const-loop-consumer`), matching live.

## Access-token modification context (listed 2026-11-10)

`context: "OAUTH2_ACCESS_TOKEN_MODIFICATION"` runs on both engines from the
overlay `packages/rhino-local/src/bindings/rhino/context-oauth2-access-token.cjs`.
Its conformance test replays six live probes
(`test/bindings/oauth2-access-token.e2e.test.ts`). What it approximates or
leaves out:

- **Java values are script objects.** A legacy `JavaString` is a null-prototype
  object with `toString`, `valueOf` and `length()`; it compares `==` but not
  `===` with a literal, as on AIC. `Object.prototype.toString` is emulated only
  where a capture measured the tag: a legacy `JsonValue` (`getField`, `getAct`)
  reports `[object JavaObject]`. On every other view the call is the named fault
  `oauth2-token-unmeasured/Object.prototype.toString.call(<binding>)` (Sol ctx
  r1 P2-5); the conformance test compares the captured tags unchanged.
- **Mutating a binding is unmeasured, and faults.** Assigning a map entry or a
  `HashSet` index faults at once. Adding, deleting or redefining a member of a
  map, set, `accessToken` or `identity` cannot be intercepted in this Rhino (no
  `Proxy`), so the harvest audits each view against its build-time snapshot and
  records the fault then: the script may read its own write back before the run
  fails. A seeded map key that shares a name with a `java.util.Map` method
  (`get`, `size`, …) is refused at seed.
- **Lists admit only their measured routes.** `JSON.stringify`, `[].concat(v)`
  and `Array.prototype.<method>.call(v, …)` on a token-context list fault; the
  decision node's request and openidm lists keep their measured behaviour.
- **Unmeasured call counts fault.** Only `setField`/`getField` at a wrong count
  are Rhino's ordinary overload error (measured); any other method at an
  unmeasured count is a named fault rather than a catchable error.
- **Setter values are JSON or a fault.** `setField`, `setFields`, `setAct`,
  `setMayAct` and `setPermissions` accept strings, finite numbers, booleans,
  dense arrays and objects whose prototype is `Object.prototype` (or `null`)
  with only enumerable data properties, recursively. A `Date`, `RegExp`,
  getter, list view or Java object is unmeasured and faults.
- **Property reads cannot be intercepted.** A Java bean's missing member reads
  as `undefined` on both sides, but a member that exists on the script object
  only to raise a fault is visible to `typeof` and `in`.
- **Hash order is not reproduced.** Sets and maps keep the fixture's order.
  Live legacy `allowedScopes` came back in an order a `HashSet` chose; the
  fixture has to list it that way to render the same `String()`.
- **Header aliases are lower and Title case only.** AIC's header map is
  case-insensitive; `requestHeaders["CONTENT-TYPE"]` is not emulated.
- **JSON numbers.** Legacy `getField` renders numbers as Java doubles (`42.0`)
  and is emulated; the JWT itself carries `42`, and the recorded effect holds
  the JS value the script set.
- **`expires_in`.** The default custom field and the response body say `3599`
  and the JWT claim says `3600`, measured. The effect records the custom field.
- **Shared bindings are the decision node's.** `logger`, `httpClient`,
  `systemEnv`, and on next-gen `openidm`, `secrets`, `utils` and `require`, were
  measured as present (`typeof`) in this context, not member by member. Their
  behaviour is the decision node's implementation.
- **The next-gen identity converts through `valueOf`.** `String(identity)` is
  the wrapper's class name plus `@rhino-local` where AIC prints a hash code;
  `identity.toString()` is a `Cannot find function` as on AIC, and so
  `identity.valueOf()` answers where AIC is unmeasured.
- **Assigning into a map.** `requestProperties.requestParams.x = …` and the
  like are unmeasured and are not refused.
- **`requestUri`'s type** on next-gen is inferred from its rendering (a path
  after the host), not from a method call.
- **Faults** (`oauth2-token-unmeasured/<binding>.<member>`): every
  `accessToken` method not listed as measured in
  `docs/api/12-script-bindings-matrix.md` (`toMap`, `getTokenInfo`, `setScope`
  with anything but the measured shapes, the other `remove*`/`set*`,
  next-gen `getResourceOwner`), Object methods on the views, `JSON.stringify` of
  any view, empty-container `JsonValue` reads, legacy `scopes.add()` (callable
  on AIC; its effect on the token is unmeasured), set iterators and streams,
  next-gen `clientProperties.redirectUris`, overwriting a reserved claim, and
  the `identity` writers. `identity.getAttributes()` is a fault on purpose: it
  returns the client's password (`.ai/core.md` §8).
- **The AIC lane** runs only decision-node scripts, so a token-context case is
  reported unsupported there and runs locally only.

## OIDC claims context (listed 2026-11-10)

`context: "OIDC_CLAIMS"` runs on both engines from the overlay
`packages/rhino-local/src/bindings/rhino/context-oidc-claims.cjs`, on the same
`context-common.cjs` views as the token context, so the view-level gaps listed
above apply here too, with the prefix `oidc-claims-unmeasured/`. The
conformance test (`test/bindings/oidc-claims.e2e.test.ts`) replays five live
probes at both the token and the userinfo endpoint, plus twelve one-line
result scripts (eighteen engine runs). What it approximates or leaves out:

- **One invocation per run.** AM runs the script once for the id token and
  again for each userinfo call. A case models one run. The userinfo run is
  modelled by its request (the userinfo URI, no params and no
  `claimsLocales`), not by a second pass.
- **The id token is derived, not issued.** The effect's `idToken` holds only
  the script's claims, under the measured subject rule, with nulls dropped. AM's
  own claims (`aud`, `azp`, `realm`, `tokenName`, `tokenType`, `iss`, `exp`, …)
  are not emulated. A returned value that names one of them faults, because
  which side wins was not observed.
- **`requestedClaims` and `requestedTypedClaims` are always empty.** The
  sandbox realm has `claimsParameterSupported: false`, so a populated request
  could not be measured without a realm-wide change. That change was declined.
- **Composite scopes filter nothing.** As measured: a claim mapped to a scope
  the token was not granted is still emitted. The recorded `compositeScopes`
  is what the script returned.
- **Legacy uses real Java strings.** The legacy `claims` map, the identity's
  name and the other legacy getters return `java.lang.String`. The
  access-token context's legacy strings are real `java.lang.String`s too
  (since 2026-11-10: `getName() == getClientId()` of the same text was
  measured `true`, which two script objects cannot reproduce). The identity's
  `getName()`/`getUniversalId()` return the same object on every call, as
  measured; other legacy getters make a new one per call, which is unmeasured.
- **Known fidelity limits of the JS view representation** (Sol ctx r2 P2-2,
  P2-4; not fixed by decision, 2026-11-10: the harness runs trusted scripts,
  so it targets fidelity, not defence against reflective JS).
  - An author's `toJSON` plus a property-list `JSON.stringify` replacer
    serialises a list view as an object instead of refusing it.
  - The mutation audit compares values only: a changed property descriptor
    (`enumerable`, ...) or a replaced `__proto__` on a view is not detected.
  - A write to a view that the script undoes before it ends (write, read,
    delete) is not detected, because the audit compares final state.
- **Legacy Java objects admit only measured members** (Sol ctx r2 P2-6). A
  legacy string answers `length()`, `toString()`, `==`, String conversion and
  the `[object JavaObject]` tag. Any other `java.lang.String` or
  `String.prototype` member (`substring`, `slice`, ...), an index, an
  assignment, `in` or enumeration is a `<context>-unmeasured/java.lang.String.<member>`
  fault. The legacy `claims` map answers entry reads, `for...in`, String
  conversion and `get`/`containsKey`/`remove` (1 argument), `put` (2) and
  `size` (0). Any other Java member, another argument count or a member
  assignment faults. Known limit, as for `idm-exception/unmeasured`: a native
  method borrowed from another object, or the object passed as a Java argument,
  reaches the unguarded Java object. Where AM's scope stores a map binding bare
  and wraps it afresh on every read, the emulator stores the guard itself
  (`ScriptContextScope`); the script reads the same map either way.
- **Next-gen `claims` is a script object.** Its seeded `toString`/`toJSON`
  render the measured `{sub, subname}` text. After the script changes the map,
  either one faults. `put` over an existing key faults as unmeasured. `delete
  claims.<key>` answers false on AIC and keeps the key; this Rhino cannot do
  that while `remove()` works, so the delete is a fault, found by `remove()` or
  by the harvest audit. A deleted key the script itself added is not detected.
- **Numbers.** AM renders a returned `42` as `42.0` in both the userinfo body
  and the id token. JSON cannot tell those apart, and the effect holds `42`.
- **Results AM was not measured with fault.** On next-gen that is anything but
  `undefined`, `{values}`, `{values, compositeScopes}` or an object without
  `values`. On legacy it is anything but `undefined`, a `UserInfoClaims`, a
  `LinkedHashMap` or a `{values, compositeScopes}` object. Inside the values, a nested null, a
  non-finite number, a Java object other than a string, number, boolean, list
  or map, and a map with a non-string key all fault.
- **Shared bindings are the decision node's.** `logger` (next-gen),
  `httpClient`, `systemEnv`, and on next-gen `openidm`, `secrets`, `utils`,
  `require`, `jwtAssertion`, `jwtValidator`, `policy` and `emailService` were
  measured as present only (`typeof`). The legacy Debug `logger`'s
  `messageEnabled()`, `errorEnabled()` and `getName()`, and next-gen
  `logger.getName()`, are faults.
- **The AIC lane** runs only decision-node scripts, so an OIDC case runs
  locally only.

## Social IdP profile transformation context (listed 2026-11-10)

`context: "SOCIAL_IDP_PROFILE_TRANSFORMATION"` runs on both engines from the
overlay `packages/rhino-local/src/bindings/rhino/context-social-idp.cjs`, on
the same `context-common.cjs` views, so the view-level gaps above apply here
too, with the prefix `social-idp-unmeasured/`. The conformance test
(`test/bindings/social-idp.e2e.test.ts`) replays the surface probe captured
during a headless social login, plus eight one-line result scripts per engine
(seventeen engine runs). What it approximates or leaves out:

- **The transform only.** The legacy context id is shared with
  `SocialProviderHandlerNodeV2`'s own `script`, which binds
  `normalizedProfile` (a JsonValue) and returns a JsonValue. That invocation
  was measured but is not emulated.
- **Legacy `rawProfile` is a view of a JsonValue.** It answers `get`,
  `isDefined`, `keys`, `size`, `getObject()` (whose map only prints),
  `get(k).getObject()` of a string entry, String conversion and
  `JSON.stringify`. On AIC `for...in` lists about seventy Java member names
  (`asString`, `expect`, `isNull`, …); the view lists none. A Java-binding
  candidate.
- **Next-gen `JSON.stringify(rawProfile)` key order.** AIC lists the keys in
  the Java map's hash order, while String conversion and `keys()` keep the
  provider's order. The view keeps the provider's order everywhere. A
  Java-binding candidate.
- **Only string profile values were measured.** Reading any other value faults
  as `social-idp-unmeasured/rawProfile.<key>`. So does a seeded key named like
  a Java map method.
- **Legacy node state.** `nodeState.get` answers a JsonValue (measured for a
  string). A key the state does not hold faults, and of the JsonValue only
  String conversion and `getObject()` of a string answer. `nodeState.keys()`
  answers the keys held when the script started. Measured: a key the script
  had just put was not listed. On next-gen `keys()` is AIC's class-shutter
  error. The decision node's legacy `nodeState.get` returns plain values; it
  was not compared in this slice.
- **Seeded node state.** `selectedIdp` is seeded into the shared state, as the
  SelectIdPNode leaves it. AM also holds `expectProfileInformation` there.
  Its value was not read, so it is not seeded.
- **Results AM was not measured with fault.** Measured: `undefined`, `null`
  and a string (each a 401), a plain object, and on legacy
  `new JsonValue(LinkedHashMap)`. A top-level `undefined` or `null` property is
  kept as `null`. A nested null or undefined, a non-finite number, a function
  or Java object in a plain result, a top-level number, boolean or array, and
  a JsonValue over anything but a Java map all fault. Numbers reach the node as
  doubles, which JSON cannot tell apart.
- **Shared bindings are the decision node's.** `requestHeaders` and
  `requestParameters` were measured by key count only. The next-gen
  `httpClient`, `openidm`, `secrets`, `utils`, `require`, `jwtAssertion`,
  `jwtValidator`, `policy`, `emailService` and `systemEnv` were measured as
  present only (`typeof`), and so were legacy `sharedState`,
  `transientState` and `systemEnv`. Legacy `callbacks` is bound, and every
  member faults. `logger.getName()` was measured on both engines.
- **The AIC lane** runs only decision-node scripts, so a social IdP case runs
  locally only.

## Config provider node context (listed 2026-11-10)

`context: "CONFIG_PROVIDER_NODE"` runs on both engines from the overlay
`packages/rhino-local/src/bindings/rhino/context-config-provider.cjs`, on the
same `context-common.cjs` views, so the view-level gaps above apply here too,
with the prefix `config-provider-unmeasured/`. The conformance test
(`test/bindings/config-provider.e2e.test.ts`) replays the surface probe
captured from a journey (a decision node seeding the node state, a
ConfigProviderNode wrapping a MessageNode, a decision node reading back), plus
thirty-one one-line `config` scripts per engine (sixty-two engine runs), each
compared with the message and buttons the MessageNode showed or the HTTP 500.
What it approximates or leaves out:

- **One invocation.** AM runs the script again on every request that reaches
  the node, including the one that submits the wrapped node's callbacks. In
  that re-run (measured) transient values from the earlier request are gone,
  next-gen `callbacks.isEmpty()` is `false`, and a next-gen node-state object
  comes back as a Java map (`[object JavaMap]`, numbers as doubles). A case is
  the first invocation only.
- **The wrapped node is not emulated.** The case asserts the `config` map;
  what the wrapped node does with it (here, a MessageNode rendering numbers as
  `42.0` and objects as `[object Object]`) is that node's business.
- **Values AM was not measured with fault.** Measured inside a plain-object
  `config`: strings, integral and fractional numbers, booleans, `null` and
  `undefined` (kept as `null`) at depths one and two, an array of a number and
  a string, and nested objects. A `null` or `undefined` inside an array, a
  non-finite number and a non-plain object (a `Date`, a view) fault as
  `config-provider-unmeasured/config.<path>`. On legacy a `java.util.HashMap`
  is accepted (measured with a string and a nested HashMap of a string); any
  other Java value inside it faults. On next-gen `new java.util.HashMap()` is
  AIC's `TypeError`; the emulator's class shutter refuses it with a different
  message, but the run fails either way.
- **Refusals.** Measured on both engines: `config` never assigned,
  `undefined`, `null`, a string, a number, a boolean, an array, a function and
  a script that only evaluates to a map, each an HTTP 500 with AM logging
  "Script must set 'config' to a Map.". A `var config;` that is never assigned
  is recorded as `undefined`.
- **Next-gen bindings whose effect belongs to the wrapped node.**
  `callbacks.isEmpty()` (`true`), `callbacksBuilder.textOutputCallback(type,
  message)` (accepted, adds nothing to what the wrapped node shows) and
  `action.goTo(name)` (accepted; whether it can change routing is unmeasured)
  answer. Every other member of the three faults. The rest of the next-gen
  surface is the decision node's, measured as present (`typeof`) only, apart
  from `nodeState` (`get` of a string, number, nested object, boolean,
  transient value and absent key; `isDefined`; `putShared`/`putTransient`,
  read back by the next node) and `nodeState.keys()`, which is AIC's
  class-shutter error.
- **Legacy node state.** `nodeState.get` answers a JsonValue (measured for a
  string, a number, a nested object, a boolean and a transient value) of
  which only String conversion and `JSON.stringify` answer; an absent key is
  `null`. A key holding `null` faults. `nodeState.keys()` is runtime.cjs's
  live list, measured: a key the script had just put is listed, and so are
  transient keys. This is the opposite of the social context, whose
  `keys()` is a start-of-script snapshot. AM's own keys (`authLevel`,
  `maxAuthenticationSessionDuration`, `realm`, `username`) appear only if the
  case seeds them.
- **`idRepository` and `journey` under `for...in`.** AIC lists their Java
  members, `java.lang.Object`'s included (legacy `idRepository` also lists
  `getAttribute`, `setAttribute`, `addAttribute`); the emulator lists only its
  own JS members. No member of `idRepository` was called in this slice. A
  Java-binding candidate.
- **Request maps.** `requestHeaders.get(name)` and
  `requestParameters.get(name)` are tagged Java lists here (measured
  `[object JavaList]`, `[v1]`, on both engines). runtime.cjs's decision-node
  maps answer a JS array on legacy and an untagged view on next-gen; that was
  not compared for the decision node in this slice.
- **`existingSession`** was measured only on a request without a session
  cookie, where it is a `ReferenceError` on both engines (the next-gen
  descriptor lists it). With a session it may be bound, as on the decision
  node; `given.existingSession` is refused until that is measured.
- **The AIC lane** runs only decision-node scripts, so a config provider case
  runs locally only.

## OAuth2 validate scope context (listed 2026-11-10)

`context: "OAUTH2_VALIDATE_SCOPE"` runs on both engines from the overlay
`packages/rhino-local/src/bindings/rhino/context-oauth2-validate-scope.cjs`,
on the same `context-common.cjs` views, so the view-level gaps above apply
here too, with the prefix `oauth2-validate-scope-unmeasured/`. The
conformance test (`test/bindings/oauth2-validate-scope.e2e.test.ts`) replays
the surface probe captured from a `client_credentials` token request, with and
without a `scope` parameter, plus twenty-nine one-line result scripts compared
with the token's `scope` or AM's error. What it approximates or leaves out:

- **One entry point.** Only `validateAccessTokenScope` on a
  `client_credentials` grant was measured. AM's refresh, authorization-code,
  back-channel and device-code entry points are never called, and
  `inheritAccessTokenScopesOnRefresh()` returns `undefined` without changing
  anything a case can see.
- **`identity` with a resource owner.** On `client_credentials` the next-gen
  `identity` has a null `AMIdentity`: `exists()` is `false`, `getName()` is
  AIC's `InternalError`, and every other member faults. A grant with a user
  (`password`, token exchange) was not measured here.
- **What a script that catches a denial reads.** `throwInvalidScope(msg)` and
  legacy `new InvalidScopeException(msg)` raise a Java exception whose message
  says it was not measured; AM's own exception object was not inspected. A
  script that catches it and returns a list gets that list (measured on
  next-gen). `throwInvalidScope()` with no argument, an object or a boolean
  faults: AIC answers the request with a 500, but whether the call itself
  throws something the script could catch was not observed.
- **`for...in` over the Java bindings.** AIC lists the Java members of
  `scopeValidatorHelper` and of the legacy sets, `java.lang.Object`'s
  included; the emulator lists only its own JS members.
- **Legacy set order.** `requestedScopes` is a `java.util.HashSet`, whose
  iteration order is the JVM's hash order; the emulator keeps the seed's
  order. `allowedScopes` and `defaultScopes` are `TreeSet`s, sorted on AIC and
  in the seed order here. A case that depends on the order should sort.
- **Legacy set iterators.** `requestedScopes.iterator()` answers a Java
  iterator on AIC whose members were not read; here every member of it
  faults. A `TreeSet`'s is the class shutter's refusal on both.
- **`String(s.length)` of a legacy Java string** (`logger.getName()`) prints a
  JS function's source here, not Rhino's rendering of the Java method.
- **`clientProperties` sets.** Reading `allowedGrantTypes`, `allowedScopes`
  or `allowedResponseTypes` is AIC's class-shutter error, so a case cannot see
  them either way.
- **Why widening differs from the 2026-07-29 note.** The emulator grants
  whatever the function returns, as measured. An older
  `docs/api/12` note on a different client saw no widening; the cause is not
  identified (`docs/api/99`, 2026-11-10).
- **The AIC lane** runs only decision-node scripts, so a validate scope case
  runs locally only.

## What this does _not_ claim

- It does not claim the 384-script production corpus was run. That tree is not
  here.
- It does not claim OAuth2 scripts in general run here. Access-token
  modification, OIDC claims and the social IdP profile transformation do,
  as do the config provider node and validate scope (sections above);
  may-act and the other OAuth2 contexts are
  a different binding surface and are not emulated.
- A known gap is not coverage. Eighteen of 57 differ from AIC on at least one key,
  and one of those (`identity-enum-attrs`) has no valid live measurement.
- `require()` is a CommonJS eval of seeded source, not AM's wrap factory. The
  library probes in this corpus do not need the shutter to load; they need it
  for the Java names they then construct (`lib-java-collections-consumer`).
- Faults are not proof against a script that replaces a standard builtin
  (`String`, `JSON`, `Array.prototype.*`, `Function.prototype.call`) while it
  calls a binding. The emulator's own code uses the standard library to decide
  and to describe a refusal, so a replaced builtin can throw an ordinary,
  catchable error before the fault is recorded. The request-list, JsonValue and
  request-map `forEach` refusal messages are built by primitive concatenation,
  because those paths were the ones reported; the rest are not, and closing
  them all is not possible from script (any helper, `call` included, is
  replaceable). Real scripts do not do this; a test that does is outside what a
  fault guarantees.
