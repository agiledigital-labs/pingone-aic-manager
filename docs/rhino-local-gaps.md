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

Only the SET of declared type names crosses into the sandbox. Every schema
rule — properties, `required`, `enum` — is checked in the Node layer, so each
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
than invent `{}`; measured managed-collection actions retain their default.
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
| `node-state-merge-non-object`                                                     | `nodeState.mergeShared`/`mergeTransient` with a function. Every other non-object is AIC's measured catchable `InternalError` (2026-11-06): `Can't find method …NodeStateScriptWrapper.<m>(<type>).` for a string, number, boolean or `undefined`, `null` for `null`, `Cannot convert …NativeArray@<hash> to java.util.Map` for an array; state is unchanged |
| `action-suspend-additional-logic`                                                 | `action.suspend` with a non-function `additionalLogic`                                                               |
| `openidm-collection-path-as-record`                                               | a record verb (`read`, …) on a collection path                                                                        |
| `openidm-unknown-managed-type`                                                    | a write (`create`, `update`, `patch`, `delete`, `action`) to a managed type the environment profile does not declare |
| `openidm-query-filter`, `openidm-query-id`                                        | a `_queryFilter` the harness cannot decide: one the shared CREST grammar (`src/bindings/rhino/crest-filter.cjs`) cannot read, an empty or non-string one, an operator the matcher does not evaluate (`gt`/`ge`/`lt`/`le` in the script tester), or `ne` from a script (its script text is unmeasured); in either lane, a record whose answer turns on letter case, spaces, Unicode normalisation, a type mismatch (decided in the IDM emulator only where measured: the quoted integer on `alpha_user`, the 2026-11-06 integer rule on `frIndexedInteger1`, and a quoted literal against a number on a type `repo` declares generic), a decimal literal against a number outside those, `pr` on `""`/`[]`/`{}` (except an explicit `[]` on the three measured `alpha_user` properties, emulator only), a pointer through an array anywhere but a bare `eq` reached through `and` alone, or an `alpha_user`/`bravo_user` field whose DS matching rule is not caseIgnore (`telephoneNumber`, the date and integer families) beyond the literal forms and operators the 2026-11-06 probe measured on `alpha_user` — in the script tester, every such field, and before any record is read; an escaped quote anywhere but a bare `eq` reached through `and` alone; a `_queryId` other than the measured ones. Only measured or certain rejections are the catchable parse error, in both lanes |
| `openidm-fields-wildcard-with-path`, `openidm-fields-non-object-parent`, `openidm-fields-unresolved-ref` | `_fields` projections that were not measured                                                  |
| `identity-attribute-layout`, `identity-write-idm-field-name`, `identity-write-cn`, `identity-set-attribute-non-array`, `identity-date-rendering`, `identity-wrapper-after-recreate` | `idRepository.getIdentity()` reads and writes outside the measured mapping |
| `identity-bag-key-collision`, `identity-bag-replacement-overlap`, `identity-bag-non-object-access`, `identity-store-non-object-bag`, `identity-store-id-layout-change`, `identity-store-two-ds-rules` | `fr-idm-custom-attrs` bag shapes that were not measured                     |
| `json-value-constructor`                                                          | `new JsonValue(...)`                                                                                                  |
| `seed-cache-manager`, `seed-binding`                                              | seeding a binding the harness cannot seed                                                                            |
| `idm-runtime-script/<path>`, `idm-runtime-script-path`, `idm-runtime-script-provider` | IDM emulator: one of IDM's own runtime scripts (a hook's `file`, or `require(id)` as `<id>.js`) that the project's `runtimeScripts` provider did not supply — including a name that matches a provider file only case-insensitively, or names a directory; a name that is not a canonical path under the script root (absolute, `.`/`..`/empty segment, trailing slash, backslash, NUL) or reaches a symbolic link in a provider directory; a loader that throws or returns a non-string, or a listed file that cannot be read (E15; README "IDM runtime scripts") |
| `idm-script-file-and-source`                                                      | IDM emulator: a script declaring both `file` and `source`                                                            |
| `idm-implicit-sync/<mapping>`                                                     | IDM emulator: a committed managed create, update, changed patch or delete on a type that is the `source` of a mapping in the supplied `config/sync` (`ConfigInput.sync`). AIC runs the mapping synchronously (implicit sync, docs/api/16-sync-mappings.md, measured 2026-06-19); the emulator models no sync, so the write faults before the store changes. Without `sync`, every write records `assumed: no config/sync supplied …` instead |

**Argument counts, by provenance.** A call with an argument count AIC has no
overload for fails on AIC as Rhino's catchable
`InternalError: Can't find method <java class>.<method>(<argument types>).`
The harness reproduces that, with Rhino's argument-type naming, wherever the
accepted counts have a basis:

- **measured** on AIC: `existingSession.get` (`docs/api/12`),
  `utils.crypto.randomUUID` (corpus `binding-utils-interop`),
  `nodeState.mergeShared` (one argument, and the message's class), and
  `logger.debug`/`info`/`warn`/`error`, which are true varargs: any count from
  one up, each argument filling a `{}`, a number printed as Java's `Double`
  text (`1.0`, `1.0E7`), an object as `[object Object]` (live probe
  2026-11-06, `docs/api/12`). `logger.trace` was not probed and keeps the
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
  its `null`, array, number, boolean and `undefined` shapes are taken from
  `mergeShared`'s, on the same wrapper class. Only a function is still `node-state-merge-non-object`.
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
  - `telephoneNumber`: spaces and hyphens are ignored anywhere, in the stored
    value and the literal, for `eq`, `sw` and `co`. Decided only when both
    hold nothing but digits, `+`, space and `-`; parentheses, dots and
    ordering were not probed.
  - `frIndexedDate1`, `frUnindexedDate1`: `eq`, `gt` and `lt` compare
    instants across offsets, for literals and values of the form
    `YYYY-MM-DDThh:mm:ss` with `Z` or `±hh:mm`. `eq "notadate"` is a 400
    `illegal filter assertion value`. `ge`/`le`, fractional seconds,
    date-only and GeneralizedTime-form literals were not probed.
  - `frIndexedInteger1`: an integral literal of either JSON type (`7`, `"7"`,
    `7.0`, `7.00`, `"7.0"`) equals the integer, and `ge 7.0` orders by it;
    `7.5`, `"7.5"` and `"abc"` are the 400, with `eq`, `lt` and `gt` alike.
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
  The script tester's `openidm.query` was not part of the probe, so there
  every one of these rules — measured or not — is the fault, raised before
  any record is read.

  **Generic (JSON-stored) types (probe §4).** On a type DS stores as JSON in
  `ou=managed`, a quoted literal never matches a number (`eq`, `gt`, `lt`:
  no rows, no error) and numbers compare by value, `7.5` included. The
  emulator decides this only where it knows the storage: `ConfigInput.repo`
  takes the tenant's `config/repo.ds`, and `IdmConfig.storage` reads a type
  as generic when `genericMapping` names it or `managed/*` covers it and
  `explicitMapping` does not. A property its `genericMapping` entry maps to
  an attribute of its own is not JSON. With no `repo`, or a type in both
  sections, the faults stay. A quoted literal is decided `false` only in a
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

## What this does _not_ claim

- It does not claim the 384-script production corpus was run. That tree is not
  here.
- It does not claim OAuth2 / ATM / may-act scripts were pointed at this harness.
  They are a different binding surface.
- A known gap is not coverage. Eighteen of 57 differ from AIC on at least one key,
  and one of those (`identity-enum-attrs`) has no valid live measurement.
- `require()` is a CommonJS eval of seeded source, not AM's wrap factory. The
  library probes in this corpus do not need the shutter to load; they need it
  for the Java names they then construct (`lib-java-collections-consumer`).
