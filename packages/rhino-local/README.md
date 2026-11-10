# rhino-local TypeScript package

Two jobs, one package:

1. Generate the **scripted-decision mock binding surface** from the captured AM
   contexts metadata at `docs/api/bindings/scripted-decision-next.json`.
2. Talk to the long-lived JVM runner so tests can eval AM or IDM scripts without
   starting a JVM per case.

The generator emits presence, not behaviour: every method throws
`rhino-local: not mocked: <binding>.<method> arity=N overload=[…]` until the
handwritten overlay in `src/bindings/rhino/runtime.cjs` replaces it. A mock
that returned `undefined` would turn a missing feature into a passing test. That
throw is a [harness fault](#harness-faults-refusals-a-script-cannot-catch)
(`not-mocked/<binding>.<method>`), so catching it does not rescue the case.

Implemented (still fail-loud for missing fixtures and for every method the
overlay does not replace): `nodeState` get/putShared/putTransient/isDefined,
legacy `sharedState`/`transientState` when `given.engine === "legacy"`,
request maps, `outcome` / `action.goTo`, `logger`, `openidm`, `httpClient`,
`callbacksBuilder` (the six authenticate-response types), `systemEnv`,
`idRepository.getIdentity` (reads, and `store()` as `identityWrites`), next-gen `require()` (CommonJS eval of seeded
library bodies; a missing id throws naming `given.libraries`), legacy
`JavaImporter` + `Action.send(HiddenValueCallback)`. End-to-end cases live
in `cases/`.

## Breaking change in 0.4.0-emulator.1: request values are Java lists

On the next-gen engine, a `requestParameters` or `requestHeaders` value is now
the Java list AIC hands over (measured 2026-11-08), not a JavaScript array.
Earlier builds returned an array, so a script calling an array method passed
here and failed on the tenant.

| Works, as on AIC                                  | Fails, as on AIC                                     |
| ------------------------------------------------- | ---------------------------------------------------- |
| `v.length`, `v[0]`, `v.size()`, `v.get(0)`        | `v.map(…)`, `v.join(…)`, any `Array.prototype` method |
| `String(v)` gives Java's `"[a, b]"`               | `Array.isArray(v)` is `false`                        |
| `requestParameters.forEach(function (k, v) {…})`  | `v.getClass()` (the class shutter refuses it)        |
| `[].concat(v)` copies it into a JS array          |                                                      |
| `JSON.stringify(v)` writes a JSON array           |                                                      |

`[].concat(v)` (alone, among several lists or among plain values),
`Array.prototype.concat.call(v, …)` and `JSON.stringify` by every route were
measured on 2026-11-10. An empty header or parameter is a list of one empty
string, and a key not sent is `null`, so `[].concat(missing)` is `[null]`.
A size-zero list was therefore never seen on AIC: a `[]` seed is accepted and
answers `size()` (0), but its `length`, `String(v)`, `concat`, `JSON.stringify`
and every array method borrowed onto it fault `request-list-unmeasured`. Beyond those, only `size()` and `get(i)` with an index in range were measured.
Any other call shape (`get()`, `get("0")`, an out-of-range index, `size(1)`)
and every other `List` member (`isEmpty`, `contains`, `toArray`, `indexOf`,
`iterator`, …) faults `request-list-unmeasured`. One gap: an out-of-range
bracket read (`v[5]`) returns `undefined` without a fault, because an index
read cannot be intercepted here; AIC's answer is unmeasured, so use `get(i)`
with an index below `size()`. The simplest copy is `[].concat(v)`; the loop
below also works:

```js
var values = requestParameters.get("q");
var copy = [];
for (var i = 0; i < values.size(); i++) copy.push(String(values.get(i)));
```

`requestCookies` values are still plain strings (measured). The legacy engine
was not measured and keeps the older array shape.

### `openidm` query results and relationship arrays are Java lists too

`openidm.query(…).result`, and a has-many relationship array on an
`openidm.read`, are Java lists on AIC (measured 2026-11-08), and now here.
`length`, `[i]`, `size()` and `get(i)` work; `get(i)` out of range either way
throws AIC's `InternalError: Index 2 out of bounds for length 2`; `.map`,
`.some`, `.filter`, `.join` and every other array method are absent, so calling
one is the same `TypeError: Cannot find function some in object [].` the tenant
throws. `String(v)` has Java's shape, `[{_id=u1, _rev=1}]`, and there is no
`toJSON` member, but `JSON.stringify` still writes a JSON array, however it
reaches the list (nested, through a `toJSON`, a getter or a replacer). Copy into
a JS array with `[].concat(v)`, which is the measured way:

```js
var ids = [].concat(openidm.query("managed/alpha_user", params, ["_id"]).result)
  .map(function (r) { return r._id; });
```

Any other `List` member or call shape (`forEach`, `isEmpty`, `get(0.5)`, an
index beyond a Java `int`, …) faults `openidm-list-unmeasured`, and so does `String(v)` of a list holding a
number, whose Java rendering was not measured. A has-many array read with `*`
alone, or with no field list, is still a JS array.

## Using the installed package

```sh
npm install --save-dev @agiledigital/pingone-aic-script-tester vitest zod
```

**Requirements**:

- Node 24.
- A Java 25 **runtime**. The runner classes are prebuilt, so no `javac` is
  needed. Use `AIC_SCRIPT_TESTER_JAVA_HOME` or `JAVA_HOME`, else `java` on `PATH`.
- `vitest` and `zod`, as peer dependencies.

The first run downloads the Rhino jar from Maven Central and checks it by
SHA-256. On a machine without access, point `AIC_SCRIPT_TESTER_RHINO_JAR` at a copy
instead. `npx aic-script-tester-fetch-jar` fills the cache ahead of time.

A suite runs on the local lane by default:

```ts
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { defineSuite, useLease } from "@agiledigital/pingone-aic-script-tester";

const suite = defineSuite({
  name: "greet",
  script: 'if (nodeState.get("userId") === "alice") { action.goTo("known"); } else { action.goTo("unknown"); }',
  outcomes: ["known", "unknown"],
  inputs: z.object({ userId: z.string() }),
});

describe("greet", () => {
  const lease = useLease(suite);
  it("knows alice", async () => {
    const run = await lease.run({ userId: "alice" }).expect({ outcome: "known" });
    expect(run.verdict.summary).toBe("");
  });
});
```

To run the same suite against a tenant, spread `aicWhenEnabled("<unique-id>")`
into the `useLease` options, then run with `AIC_SCRIPT_TESTER_AIC=1` and
Vitest's `--no-file-parallelism`: with test files in parallel, AM answers
journey writes and node deletes with 500s. The tenant comes from the first of these that is configured:

1. a `provider` passed in the options;
2. a provider registered with `setTenantProvider()`, typically from a Vitest
   `setupFiles` module;
3. the environment: `AIC_SCRIPT_TESTER_TENANT_URL` (https, no path), plus either
   `AIC_SCRIPT_TESTER_SA_ID` with `AIC_SCRIPT_TESTER_SA_JWK` or `AIC_SCRIPT_TESTER_SA_JWK_FILE`,
   plus optional `AIC_SCRIPT_TESTER_LOG_KEY_ID` and `AIC_SCRIPT_TESTER_LOG_KEY_SECRET`;
4. the `aic` CLI (`AIC_BIN`).

A partial environment configuration is an error rather than a fallthrough.

```ts
// test/tenant.setup.ts — list it in vitest.config's test.setupFiles
import { setTenantProvider, tokenCallbackProvider } from "@agiledigital/pingone-aic-script-tester/aic";

setTenantProvider(
  tokenCallbackProvider({
    baseUrl: "https://tenant.example.com",
    // Called again with reason "rejected" after a 401; don't return the same token.
    getToken: async ({ reason }) => fetchBearerSomehow(reason),
    // Optional, and needed only for aic-script-tester-show-log.
    logKeys: { id: process.env.LOG_KEY_ID!, secret: process.env.LOG_KEY_SECRET! },
  })
);
```

### Harness faults: refusals a script cannot catch

When a script does something the harness does not emulate, the harness refuses
it with a **harness fault** rather than guessing. Examples are an unmocked
binding method, an unseeded fixture, an argument count whose accepted range is
the harness's guess, or a JS `Date` in an `httpClient.send` body. A harness fault **fails the case even if the script
catches it and then succeeds**, because the harness cannot say what AIC does
there: it may throw something the harness has not measured (a write to a type
the tenant lacks), or nobody knows (a guessed argument count). One family is
different in kind. Under `script-defect/` AIC's behaviour is measured — it sends
a `Date`, `RegExp` or function body as a dump of the Rhino object and carries on
— and the fault is a refusal to pass a script that would send that dump, not a
gap in the harness. The tally lists that family apart, so it does not read as
harness work. In each case a `try`/`catch` around the call must not turn a
gap in the harness into a passing test.

```text
rhino-local harness fault script-defect/send-body-js-date: httpClient.send body.d is a JS Date: … (script defect: AIC's behaviour here is measured and is not what the script means; this fails the run even if the script caught it)
```

Every fault has a stable id: a kebab-case slug, or a family prefix such as
`arity-unknown/<binding.method>`, `not-mocked/<binding.method>`,
`fixture/<kind>` or `script-defect/<kind>`.
Ids are never derived from line numbers or message text, so they survive a
release. [`docs/rhino-local-gaps.md`](../../docs/rhino-local-gaps.md#harness-faults)
lists them.

Errors that AIC itself raises and the harness emulates stay ordinary,
catchable script errors. One example is `Entry Already Exists` from a
colliding `openidm.create`, `Resource 'managed/<type>' not found` from
`openidm.query` on a `managed/<type>` the environment profile (from
`aic-script-tester-pull-profile`) does not declare, or the `_queryFilter` parse error for a filter AIC is measured (or
certain) to reject, such as `name eq` or the word form `not (…)` (raised before any record is read, so an empty
collection does not hide it). A filter the harness's CREST grammar cannot read, an operator it does not evaluate
(`gt`/`ge`/`lt`/`le`, except `gt`/`lt` on `alpha_user`'s measured date fields), `ne` (refused by AIC, in words not measured from a script), an empty or `null` filter, an escaped quote anywhere
but a bare `eq` joined by `and` alone, and a record whose match turns on letter case, spaces or Unicode normalisation
(AIC folds them for some fields, unmeasured for the rest), on a type mismatch (a number field against a quoted
integer such as `"7"` is decided on `alpha_user` `frIndexedInteger1` only, and this lane knows no other types), on a decimal literal against a
number, on `pr` of `""`, `[]` or `{}`, on a pointer through an array (except a bare `eq` joined by `and` alone), or on
an `alpha_user`/`bravo_user` field DS does not compare by caseIgnoreMatch (`telephoneNumber`, the date and integer
families) beyond what the 2026-11-06 probes measured (refused before any record is read) are the harness fault
`openidm-query-filter` instead: AIC may well parse a filter the harness cannot. Where the probes measured AM's
`openidm.query` on `alpha_user` — `telephoneNumber` with spaces, hyphens and `+` ignored, `frIndexedDate1` and
`frUnindexedDate1` as instants under `eq`/`gt`/`lt`, an integral literal of either JSON type under `eq` on
`frIndexedInteger1`, `pr` of an explicitly written `[]` — the script tester answers as AIC does, and a literal AIC
cannot convert (`frIndexedInteger1 eq 7.5`, `frIndexedDate1 eq "notadate"`) throws AIC's catchable `JavaException`:
`org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: The request cannot be processed because it
contained an illegal filter assertion value &#39;7.5&#39; for field &#39;/frIndexedInteger1&#39;`, with
`e.javaException.getCode()` 400 and `getReason()` `Bad Request` (`docs/api/12`). A script
that catches one of those passes, just as it would on the tenant. `openidm.read`
of an undeclared type returns `null`, as AIC's does. A profile speaks only for `managed/<type>`: any other unseeded
resource (`internal/role`, `config/…`, a relationship collection) is the fault `fixture/managed` with or without one.

**Argument counts.** A call with an argument count AIC has no overload for is
refused the way Rhino refuses it on AIC — a catchable
`InternalError: Can't find method <java class>.<method>(<argument types>).` —
wherever the accepted counts have a basis: measured on AIC
(`existingSession.get`, `utils.crypto.randomUUID`, `nodeState.mergeShared`) or read from AIC's binding
introspection (`docs/api/bindings/scripted-decision-next.json`). Where the
harness's range is its own guess, the call is the harness fault
`arity-unknown/<binding.method>` instead, and so is every argument-count check
on the legacy engine, which has no introspection. The same fault covers a count
that only a reading of the introspection decides: where an overload's last
parameter is listed as an `array`, it may be Java varargs, which Rhino calls
with any count from one fewer upwards. So `logger.trace` with four or more
arguments and `openidm.read(r, null, ["_id"], "x")` fault rather than pass or
throw. `logger.debug`/`info`/`warn`/`error` were measured true varargs on
2026-11-06: any count is accepted, each argument fills a `{}`, and a number
prints as Java's `Double` text (`1.0`). `nodeState.mergeShared`/`mergeTransient`
with a non-object throws AIC's measured catchable `InternalError`, leaving state
unchanged; only a function argument is still the fault
`node-state-merge-non-object`. The class named in an introspection-based refusal is the introspection's
declaring class; that Rhino names that class, and not an implementation
subclass, is measured for `existingSession.get` and `randomUUID` only.

**The fault tally.** At the end of a run the package's Vitest reporter prints a
per-id summary to stderr. It shows how many cases hit each fault and in how
many files, and counts a case once per id however often it hit it. Each run
also writes the tally as JSON, even when it is empty. The default path is
`<state dir>/harness-faults/tally.json`, where the state dir is
`AIC_SCRIPT_TESTER_STATE_DIR` or else `<project>/.aic-script-tester`. Set
`AIC_SCRIPT_TESTER_FAULT_TALLY` to write it somewhere else. The tally covers
every file, worker and pooled JVM in the run, and each fault is attributed to
the case whose job raised it. Missing journey and IDM behaviour that has no id
yet is listed separately, under its cause text. In watch mode with
`isolate: false` the audit history spans reruns, so a rerun's tally can count
cases from earlier runs; read the tally from a plain `vitest run`.

```json
{ "version": 1, "harness": "0.4.0-emulator.1",
  "faults": [{ "id": "script-defect/send-body-js-date", "cases": 2, "failed": 2,
               "files": ["test/a.test.ts", "test/b.test.ts"], "message": "…" }],
  "unidentified": [] }
```

`harness` is the version of this package that ran the suite; the summary's
heading names it too, so a pasted tally says which build produced it.
`failed` is the number of cases the fault failed. The rest declared it on
purpose with `expectHarnessFault(id)`, which is how a test that exercises the
refusal itself asserts it:

```ts
import { expectHarnessFault } from "@agiledigital/pingone-aic-script-tester";
expectHarnessFault("script-defect/send-body-js-date");
await expect(lease.run().expect({ outcome: "done" })).rejects.toThrow(/harness fault script-defect\/send-body-js-date/);
```

### Libraries used by `require()`

Put each next-gen library on the suite, keyed by the exact name passed to
`require()`. The value is the JavaScript source, not a path. Load source from a
file before defining the suite:

```ts
import { readFileSync } from "node:fs";
import { defineSuite } from "@agiledigital/pingone-aic-script-tester";

const suite = defineSuite({
  name: "library example",
  script: 'nodeState.putShared("code", require("codeLookup").code()); action.goTo("done");',
  outcomes: ["done"],
  libraries: {
    codeLookup: readFileSync(new URL("./lib/codeLookup.js", import.meta.url), "utf8"),
  },
});
```

For example, `lib/codeLookup.js` can contain `exports.code = function () {
return "ready"; };`. Another library in the same map can call
`require("codeLookup")`. The local lane loads both from the map; the AIC file
lease provisions absent entries as `LIBRARY` scripts in declaration order,
then creates the subject. AM's write path does not syntax-check scripts;
whether it validates `require()` targets at create time has not been measured.
An A → B → C chain resolves locally; a three-library chain has not yet been
measured on AM. In a diamond (A and B both require C), C runs once per local
pass; AM's evaluation count is unmeasured. In a cycle (A ↔ B), local Rhino
hands B A's partially built exports; AM's cycle behavior is unmeasured.
Declare every name in the suite map: an undeclared name may resolve to a
tenant-only library on AIC and make the lanes disagree.
At close, the lease deletes the subject's graph, replaces each owned library's
source with an empty body, confirms that update by reading it back, then
retries deletion until no further delete succeeds. The blanking step removes
references among owned libraries, including possible self-references in comments;
it can be replayed from a journal with explicit owned status after a lost
response. Unreleased, older library journals without that status are read-only
probes and require operator inspection. The
orchestrator's 2026-09-30 live run after `d7de205` confirmed that AM accepts
the empty-source PUT and the tested lease left no library residue. An owned
library still referenced by a reused external library remains journalled and
is reported as residue at close.
Names are realm-wide. A same-name library with byte-identical source can be
reused only if it has no lease ownership marker. A library owned by another
file lease is refused even when the source matches; the error names its
`aic.id` (or its hash for older markers). Give libraries distinct names or run
them through one lease with one `aic.id`. A different source or script context
found during preflight is an error before any lease write. AM has no measured
atomic create precondition: another writer could create the deterministic ID
between the preflight read and the lease's PUT. If AM returns 200 for that
PUT, the lease may already have overwritten the other library's source. It
marks the ID not owned in its journal, leaves it for operator inspection, and
will not automatically blank, delete, or recreate it on a later open. Use one
host per `aic.id`, as for the other leased resources.
Before blanking or deleting a library it created, the lease checks its current
source and ownership fields; if they changed, it leaves the library and recovery
journal in place and reports a cleanup warning. It never blanks or deletes a
reused library. Legacy scripts
cannot use libraries, and `given.engine: "legacy"` with `libraries` is rejected.

### Callback suspension

On a next-gen pass, queued callbacks make AM pause and wait for a reply. They
win even if the script also called `action.goTo()`: the pass has no outcome.
Expect `outcome: null` and the emitted callbacks, then use `.step({ reply })`
to submit a response and run the next pass. A pass with no queued callbacks
keeps its `goTo` outcome. Legacy `Action.send` behavior is separate.

`action.putSessionProperty` and `removeSessionProperty` produce a judged
`sessionProperties` state diff. Declare `added`, `changed`, and `removed` keys
in the pass's expectation; undeclared changes fail locally. The local step
runner keeps the journey's starting `existingSession` on the next callback
pass; action writes are not visible through that binding within the journey
(measured). When AM applies them to the session — at journey completion or
otherwise — is unmeasured. AIC cannot currently read the completed subject
session, so this effect channel reports an observation gap there.

`identity.store()` on an `idRepository.getIdentity(id)` handle produces judged
`identityWrites` effects, one per attribute stored:
`{ identity: id, attribute: <AM attribute name>, values: [...] }`. Declare them
in `expect.identityWrites` (`identity` and `attribute` take a string or RegExp,
`values` any expected value, `times` a count). The channel is fail-closed like
`openidm` writes: an undeclared write fails unless
`allowUndeclared: { identityWrites: true }`. `store()` is not an `openidm` call —
AM persists through its identity repository — so it never appears in
`expect.openidm` and `openidmFailures` stubs do not count or fail it. Passing an
IDM field name (`userName`, `frUnindexedString1`, …) to `setAttribute` or
`addAttribute` throws naming the AM attribute (`uid`, `fr-attr-str1`), because
what AM stores for an unknown name is unmeasured. After `store()` the same
wrapper and a fresh `getIdentity` see the new values at once; without
`store()` nothing persists, matching AIC.

`store()` lays each attribute out in IDM as measured on AIC
(`docs/api/14-am-identity-attributes.md`, "What `store()` writes"):

| AM attribute | IDM property | 1 value | 2+ values | `[]` |
| --- | --- | --- | --- | --- |
| `fr-attr-str1`..`5`, `fr-attr-istr1`..`20` | `frUnindexedStringN`, `frIndexedStringN` | string | throws | removed |
| `fr-attr-multi1`..`5`, `fr-attr-imulti1`..`5` | `frUnindexedMultivaluedN`, `frIndexedMultivaluedN` | array | array | `[]` |
| `fr-attr-int1`..`5`, `fr-attr-iint1`..`5` | `frUnindexedIntegerN`, `frIndexedIntegerN` | number | throws | removed |
| `fr-attr-date1`..`5`, `fr-attr-idate1`..`5` | `frUnindexedDateN`, `frIndexedDateN` | ISO string | throws | removed |
| `givenName`, `telephoneNumber`, `mail` | same name | string | array | removed |
| `sn` | `sn` | string | array | throws |
| `fr-idm-custom-attrs` | persisted bag keys (initial seed: `custom_*`) | whole-bag replace from JSON | throws 65 for valid JSON, otherwise 21 | removed; bag size 0 |

One member of each family was measured; the rest are assumed to match it.
Pass integers as decimal strings (`"42"`) and dates as GeneralizedTime
(`"20261001120000Z"`); `getAttributeValues` returns them in that form. Where
AIC refuses a store — several values on a single-valued attribute, `[]` on
`sn`, a non-integer, an ISO date — the script gets AM's own `JavaException`
(`…IdentityUpdateException: … ldap errorcode=65` or `21`) and nothing in that
`store()` is applied, so a script's error handling can be tested. Inputs AIC
was not measured with (a signed integer, a GeneralizedTime with an offset)
throw a `rhino-local:` refusal instead. `cn` is refused: AM keeps it outside
the IDM managed record, which the local lane does not model.

`fr-idm-custom-attrs` needs no declared layout. An initial seed derives one
compact JSON object from exactly the record's `custom_*` properties, preserving
JSON types; with none, the default is one `{}`. Ordinary seeded properties are
not inferred to be bag keys. Direct IDM `custom_x` attribute reads return empty.
Check the bag's size before parsing its element; JSON key ordering is not a
contract. Reads show persisted values until `store()`.

A bag write preserves its exact JSON text. Object bags expose every key as an
IDM property, including unknown `custom_*` keys, nested values and unprefixed
keys such as `plain`, with no type coercion. Collisions are unmeasured and
refused before persistence: `_`-prefixed record metadata and known ordinary/OOTB
identity fields cannot be bag keys. A whole-bag replacement and another AM
attribute write cannot target the same property in one `store()`, in either
pending order; use separate stores for those writes. A replacement touches the
whole `custom_*` set, including keys it removes. Distinct targets can share a store.

Declared `identityAttributes` layouts retain precedence: an alias mapped to a
`custom_*` property can write it whether seeded or bag-written. Each accepted
key write updates the persisted object bag and record together, preserves other
keys, and restores an absent `[]` bag to one object element. Single-valued clears
remove that key; multi-valued clears leave its value as `[]`. An unused
declaration permits clearing the whole bag. A declaration of
`fr-idm-custom-attrs` itself still overrides the default and its seed contract.
Key writes into non-object bags (including JSON null) refuse as unmeasured;
restore an object bag in a separate store first. An `_id` declaration cannot
change the identity resource. Every transition is preflighted before applying
or recording any pending write. Whole-bag replacement deletes omitted bag-owned keys. `["{}"]` leaves one empty-object element; `[]` leaves
size 0. An ordinary IDM patch or update restores a cleared bag to `["{}"]`.
Multiple valid JSON elements throw errorcode 65; malformed text throws 21 in
any position, even with multiple elements. These failures are atomic.

A single JSON string, number, array, boolean or `null` also persists and reads
back through AM. **Tenant hazard:** string, number, array and boolean bags make
a full IDM read throw “Response is not application/json”; JSON `null` permits
that read. A string bag also makes `openidm.delete` fail without deleting the
record (REST DELETE returns 500); restoring `["{}"]` through AM permits deletion.
Other non-object deletes are unmeasured and refused locally. `_id`-only filtered
queries work for string/number/array/null bags; boolean queries are unmeasured.
Other projected reads, query shapes and IDM mutations refuse locally before
changing the record. IDM field projection also refuses `*` combined with a
`parent/child` selector as unmeasured, even for pre-expanded parents. Child
selectors are checked before wildcard return; standalone `*` remains supported.
This applies to read/query fields and params `_fields`, including comma-joined
forms. See the maintainer's 2026-10-02
[edge measurements](../../docs/api/14-am-identity-attributes.md#fr-idm-custom-attrs),
including `identity-custom-attrs-nonobject.script.js` at `f7d684b`.

Both public `check(idm)` handles receive bag metadata. Non-object materialized
read/query behavior through the external handle is unmeasured, so checks refuse
it (including null); string deletes report the measured REST failure and other
non-object deletes refuse as unmeasured. Final checks clone metadata alongside
the store, keeping housekeeping out of recorded effects.

Declare one effect for the bag using the original string array, not one per IDM key:

```ts
identityWrites: [{
  identity: "example-id",
  attribute: "fr-idm-custom-attrs",
  values: ['{"custom_example":"written"}'],
}]
```

A 0.2.0 explicit `"fr-idm-custom-attrs"` record seed is accepted only when its
parsed JSON agrees with the derived bag, ignoring key order; it is then
canonicalised away. Conflicting seeds now fail (**breaking for 0.2.0**): seed
the IDM `custom_*` properties once. An explicit `identityAttributes` layout
still wins and exempts that attribute's seeds from this normalization,
consistently with other measured overrides, and stays AIC-eligible.

Identity wrappers resolve the current persisted record after IDM update. Create
and delete/recreate bag lifecycle is unmeasured: successful local operations
clear old bag metadata, and fresh wrappers derive the new seed's bag. A retained
wrapper after delete/recreate refuses and requires a fresh wrapper.

Any other attribute — a tenant's own, say — throws until you declare how the
tenant stores it, keyed by the AM name:

```ts
const suite = defineSuite({
  // ...
  always: {
    identityAttributes: {
      "custom-consent-date": { field: "custConsentDate", cardinality: "single" },
      "custom-history": { field: "custHistory", cardinality: "multi" },
    },
  },
});
```

`"single"` stores one value as a string and removes the property on `[]`;
several values are refused, since what DS does then depends on its schema.
`"multi"` always stores an array. Values are stored as the strings given — a
declared attribute gets no integer or date conversion. A declaration also
overrides a measured default. Add or override entries per test with
`.run().identityAttributes({...})`, or edit `request.identityAttributes` in
`beforeRun`; entries merge by attribute name. A raw `Case` takes the same map
as `given.identityAttributes`.

The declaration is **local only** and, unlike `http` and `bindingOverrides`,
it does **not** make a case AIC-ineligible. Those inputs change what the
script observes in a way the tenant cannot reproduce, so a tenant run without
them is a different case. A declaration changes nothing the script can do; it
is a claim about the tenant's mapping, and the AIC lane simply writes through
the real one. Keeping the case eligible is what checks the claim: a wrong
`field` or `cardinality` makes the lanes disagree on whatever the script reads
back (`live-identity-declared.e2e.test.ts` pins both outcomes). The AIC lane cannot observe
the write itself (only what the script reads back), so this channel reports an
observation gap there.

### Declared call and log channels

An omitted `openidm` expectation permits undeclared reads and queries, while
still rejecting undeclared writes. An omitted `logs` expectation permits log
lines. Declaring either channel makes its list exhaustive, including an empty
list:

```ts
// Before 0.1.x change: a query could run and this expectation still passed.
// Now: the query fails the verdict as an undeclared OpenIDM read.
.expect({ outcome: "done", openidm: [] })

// Now: any emitted log line fails as undeclared.
.expect({ outcome: "done", logs: [] })
```

`allowUndeclared: { logs: true }` permits additional log lines while every
entry in `expect.logs` remains required, unordered, with `times` honoured:

```ts
.expect({
  outcome: "done",
  logs: [{ level: "info", message: "completed", times: 2 }],
  allowUndeclared: { logs: true },
}) // requires exactly two matching lines; other lines are allowed
```

Likewise, `allowUndeclared: { openidmReads: true }` permits extra reads and
queries while each entry in `expect.openidm` remains required with its declared
`times`. Undeclared writes still fail unless `openidmWrites: true` is explicit.
These waivers do not ignore the declared lists. This changes verdicts for 0.1.x
tests that declared either channel while relying on its former fail-open default.

Two more channels are new since 0.1.2 and fail closed on undeclared effects:

- `sessionProperties`. 0.1.2 accepted `action.putSessionProperty` and
  `removeSessionProperty` and recorded nothing. Now every put or removal must
  be declared in `expect.sessionProperties`, or the test opts out with
  `allowUndeclared: { sessionProperties: true }` in its expectation.
- `identityWrites`. 0.1.2 recorded `identity.store()` as an `openidm` patch with
  an invented body. Move such declarations from `expect.openidm` to
  `expect.identityWrites`, or opt out with
  `allowUndeclared: { identityWrites: true }`.

Every undeclared-effect failure names its remedy: the channel to declare it in
and the `allowUndeclared` flag that waives it.

`judge()` still accepts effects recorded by 0.1.2, which carry neither channel.
It reports the two as unobserved rather than empty — a 0.1.2 run may have made
writes it never recorded — so the verdict passes but is not `conclusive` unless
the case opts out of both channels.

The same holds at compile time. In the public `RecordedEffects` type both
channels are optional, so a 0.1.2 producer still type-checks;
`diffRecordedEffects()` reports an omitted one as an observation gap, and
`normaliseEffects()` fills it in and marks it unobserved. Effects the harness
hands back (`RunResult.effects`) are `CompleteRecordedEffects`, with both
present. Likewise the channels `RequestDraft` gained since 0.1.2 (`cookies`,
`http`, `openidmActions`, `openidmFailures`, `bindingOverrides`, `esvInState`,
`esvUndeclared`, `identityAttributes`) are optional, and
`toGiven()` reads an absent one as empty (`resolveDraft()` fills them).
`beforeRun` is handed a `ResolvedRequestDraft`, with all of them present.
`esv` values widened from `string` to `string | null` (a `null` declares an
absent ESV), so code that reads a draft's `esv` value as a `string` needs a
null check.

Optional `identityCustomAttrs` and `identityCustomAttrsOwnedKeys` runner
bookkeeping accompanies `managedStore` across callback passes. Both maps use
full managed resource paths. The first stores persisted AM string arrays,
preserving exact JSON text and the `[]`/`["{}"]` distinction. The second stores
unique historical bag-owned keys independently of current values: clearing an
alias, an object bag or the whole bag does not forget ownership. Re-adding a
proven unprefixed key such as `plain` through its declared alias restores it in
the bag and record; a later whole-bag clear removes it. Successful local
create/delete discards the prior resource's ownership history.

Entries must identify existing records and use dense arrays; every currently
present owned property must project from the current bag. Both current object
bag keys and historical keys reject `_`-prefixed metadata and known ordinary/OOTB
AM fields at given/effect validation. Validation and Rhino use one shared layout
inventory and checker from the packaged `identity-policy.cjs` asset. Node requires
it; Rhino reads canonical file text, so host coverage instrumentation cannot
introduce missing helpers into the sandbox. IDM patch/update also preflight bag
synchronization before changing the record or metadata, so supplied history
cannot promote `givenName`
or `_rev` into the bag. Only properties actually present in a valid proven bag
bypass profile checking; ordinary properties and their enum values remain strict.
Older metadata without an ownership map infers ownership from current bag values,
but cannot reconstruct cleared historical keys. Both fields
are optional for 0.1.2 producers and local carry metadata, with no draft/builder
methods or judged channels. Bag writes are judged only through `identityWrites`.
Full resource paths prevent collisions across collections.

Every function that takes effects also takes the 0.1.2 shape and normalises
it: `judgeBoth()`, `chainFromRunResult()` (whose parameter is
`RunResultInput`, so a 0.1.2 `RunResult` is accepted), and the `LeaseLaneCheck`
hooks a `LeaseLane` calls, which hand your `check()` the complete form with
the missing channels marked unobserved. What the harness hands *to* your code
(`RunResult`, `StepResult`, `CheckContext`, `StepContext`, `BeforeRunContext`)
stays complete, so a test that builds one of those by hand needs the new
fields. `test/compat/consumer-0.1.2.ts` holds this line: it compiles every
0.1.2 export's shape against the current one, and `npm run typecheck` fails on
any new break.

### Project-wide suite defaults

Channels and libraries that most suites in a project share, such as the ESVs a
common library reads on most paths, can be declared once:

```ts
// test/suite.ts
import { readFileSync } from "node:fs";
import { withSuiteDefaults } from "@agiledigital/pingone-aic-script-tester";

export const defineSuite = withSuiteDefaults({
  always: { esv: { "common.url": "https://api.example.com" } },
  libraries: { common: readFileSync("src/library/common.js", "utf8") },
});
```

Each suite built with that `defineSuite` starts from the defaults. Precedence,
lowest first: the defaults, the suite's `always`, then a case's `.run()`
overrides. Maps merge per key, stub lists put the upper layer's entries first,
and a suite's library replaces a default library of the same name. Set
`defaults: false` on a suite to opt it out. Defaults only add declarations, so
an ESV nobody declared is still refused unless a layer sets `esvUndeclared`.
`layerChannels(base, top)` is the merge on its own, for composing defaults.

### ESV declarations

Declare values read through `systemEnv.getProperty("esv.<name>")` with the
`esv` channel. It is available in `always`, per-test `.esv()`, and `beforeRun`'s
`request.esv`. The local lane seeds `given.esv`; the AIC lane reads the tenant's
real ESVs and never writes them. Declare an absent ESV as `null` to test its
one-argument `null` result or a supplied default. An undeclared local read
throws with the missing key, even if other ESVs were declared.
Use only the part after `esv.` as a channel key: for
`systemEnv.getProperty("esv.feature")`, declare `esv: { feature: "on" }`.
`esv: { "esv.feature": "on" }` is rejected instead of becoming
`esv.esv.feature`. A management API ID such as `esv-feature` is also rejected:
the ID does not reliably identify the script property name. Look at the
script's `getProperty` call and declare its suffix.

Before this change, `esv: { feature: "on" }` only wrote `esv.feature` into
shared state. Now it makes `systemEnv.getProperty("esv.feature")` return
`"on"` locally. If a script uses a config library that reads shared-state
overrides, opt in to both behaviors:

```ts
const suite = defineSuite({
  name: "feature check",
  script: 'action.goTo(systemEnv.getProperty("esv.feature") === "on" ? "on" : "off");',
  outcomes: ["on", "off"],
  always: { esv: { feature: "on" }, esvInState: true },
});
// One test can also call lease.run().esvInState() before .expect(...).
```

`esvInState` writes `esv.feature` into shared state on both lanes. With it on,
an input named `esv.feature` is rejected as a collision. A declared local
value can differ from the tenant's ESV; conformance can detect a difference
only when the script exposes it in recorded effects. Check tenant ESV values
before using the AIC lane for value-sensitive tests. Tenant ESV changes require
a restart and are outside the lease.

An explicit `null` reproduces AM's absent-ESV behavior for one property:
`.esv({ feature: null })`, or raw `given.esv: { "esv.feature": null }`, returns
`null` without a default and the supplied default otherwise. Harness `.esv()`
keys omit `esv.`; raw `given.esv` keys include it.

Undeclared ESV reads throw by default. Use `always: { esvUndeclared: "absent" }`,
`.run().esvUndeclared("absent")`, or `request.esvUndeclared = "absent"` in
`beforeRun` to make every undeclared property behave like a declared `null`.
Raw cases use `given.esvUndeclared`. A third `returnType` argument converts a
non-null default just as for a declared `null`; null bypasses conversion.
`"error"` restores strict reads. This given-side policy changes script input;
it does not waive verdict checks. Cases remain AIC-eligible: AIC resolves real
ESVs, so a tenant property that exists can still produce a disagreement.

### Session and its principal

`always: { session: { tier: "gold" } }` or `.run().session({ tier: "gold" })`
asks for a logged-in session; the script sees it as `existingSession`, a flat
string map. Custom properties only: `UserId`, `AuthLevel` and the other keys AM
sets itself are refused, because the AIC lane cannot override them.

The five keys AM derives from the principal (`UserId`, `Principals`,
`UserToken`, `Principal`, `sun.am.UniversalIdentifier`) come from
`sessionPrincipal`: `always: { sessionPrincipal: "alice" }` or
`.run().sessionPrincipal("alice")`. Declaring it also asks for a session. It is
independent of shared state, so a test can put one user in `existingSession`
and another in `nodeState`. Without it, the principal is shared-state
`username`, then `rl-session`. The AIC lane logs in as the principal through a
mini journey (the principal need not exist), and the local lane mints the same
five keys (`live-session-principal`). A principal that would need escaping in a
DN (`,+"\<>;=`, or edge spaces) is refused, because AM's handling of it is
unmeasured.

### Request cookies

Declare cookies with `always: { cookies: { name: "value" } }`, override one
name with `.run().cookies({ name: "other" })`, or edit `request.cookies` in
`beforeRun`. The three sources merge per cookie name. The local lane seeds
`requestCookies`; the AIC lane sends the same values on `/authenticate`.
When a test also requests `session`, the harness adds the tenant's session
cookie and refuses an author cookie with that name instead of overwriting it.
AM lists that session cookie in `requestCookies` as a string, beside the
author's cookies (measured, `live-session-cookie`). The local lane does the
same under its `cookieName`, with the fixed value
`"rhino-local-session-token"`, and refuses the same collision. Presence,
`size()` and type agree across lanes; the value cannot, because the tenant's
token changes every run, so do not copy it into an effect you compare.

`state.shared.objectAttributes` seeds a plain object, just as `putShared`
does on AIC. To seed the registered Java-map container, use
`always: { registeredObjectAttributes: { key: "value" } }`, override keys with
`.run().registeredObjectAttributes({ key: "other" })`, or set
`request.registeredObjectAttributes` in `beforeRun`. This channel uses
`mergeShared` on AIC and is eligible for conformance. The two seed paths
cannot be declared together for the same run.

HTTP replies can be declared in `always.http`, supplied with `.run().http()`,
or edited in `beforeRun` via `request.http`. The first matching stub wins;
per-test stubs precede suite defaults. Each call gets a fresh copy of its reply,
including body and headers, so response mutations cannot change later replies.
They feed local `httpClient.send()` and the request is judged through `expect.http`. AIC cannot inject an HTTP reply,
so such cases are AIC-ineligible and report an observation gap. Set the file
lease's `aic.unsupported` to `"skip"` to keep the local verdict while making
that gap explicit.

Declare local action replies in `always.openidmActions`, `.run().openidmActions()`
or `request.openidmActions` in `beforeRun`:

```ts
openidmActions: [{
  match: { resource: "endpoint/example", action: "evaluate" },
  reply: { body: { accepted: true } },
}]
```

Like HTTP stubs, the first matching reply wins, per-test replies precede suite
defaults, and resource accepts a string or regex. Match uses resource and action
name; HTTP reply matching also does not inspect request content. The unchanged
call effect is judged by `expect.openidm` (`actionName`, `body`). Failure stubs
win over reply stubs. Unmatched actions now throw naming `given.openidmActions`
(**breaking for 0.2.0**), instead of inventing `{}`. The managed-collection
actions `patch`, `triggerSyncCheck` and `updateLastSync` are the names AIC's
refusal lists (measured); what they return was not measured, and on AIC they
run the records' write chain or the type's sync mappings. Unstubbed, each is
the harness fault `openidm-managed-collection-action/<action>` (E15b; it used
to return an invented `{}`). An explicit reply answers it. Other
managed-collection actions still throw the measured refusal, even with a reply.
Action replies are local only: like HTTP replies, they make cases AIC-ineligible
and report the same observation gap (`aic.unsupported: "skip"` opts into skipping).

Use `openidmFailures` to make one local IDM call fail: each stub has
`match: { method, resource, ordinal }` and `reply: { code, detail?, message? }`. `ordinal` is
one-based among calls with the same method and resource, so a second patch
can fail after the first succeeds. On a `.step()` chain the count runs across
the whole journey, not per pass: "patch #1" fails once, so a retry on the
next pass reaches #2 and succeeds. (HTTP stubs keep no count; a stub answers
every matching request on every pass.) Declare stubs in `always`, add them with
`.run().openidmFailures()`, or edit `request.openidmFailures` in `beforeRun`.
The failed call is still recorded in `expect.openidm`. The script sees a
`JavaException` whose text reads
`…ResourceExceptionScriptAdapter: injected ResourceException code <code>`, and
no `code` property — AIC's adapter has none (`typeof error.code` measured
`"undefined"`), so a script cannot route on one there either. It routes on
`e.javaException.getCode()`, which answers the stub's code, as AIC's adapter
answers its status; `getReason()` answers CREST's reason phrase for it (a code
outside CREST's table faults, `openidm-failure-reason`). The message text
is the harness's own unless the stub gives `message`, which replaces the text
after `ResourceExceptionScriptAdapter: `; AIC's carries the reason phrase or
IDM's own words, not the number. `detail` is what `getDetail()` returns, as
AIC's CREST `JsonValue` (measured 2026-11-08 and 2026-11-10, docs/api/12):
`get(key)`, `get(index)`, `isNull()`, `getObject()`, `asString()` and its
one-line string form. `asString()` is the string, or `null` for a null value
(a null detail, a missing key, an index past the end), and on a boolean,
object or array throws AIC's catchable `<pointer>: Expecting a
java.lang.String but got <class>`. A policy failure's detail is its
`{ result: false, failedPolicyRequirements: [...] }` object; a 404 or 400 from
`openidm` carries `null`, so `getDetail().get("x").asString()` is `null`
there. `typeof detail.<member>` is `"function"` and `"<member>" in detail` is
`true` for every `JsonValue` method, as measured, so feature detection works;
calling a member the probes did not exercise (or binding it, rendering it as a
string, or serialising it with `JSON.stringify` by any route but one: an
author's `toJSON` returning it under a property list is not caught) is the
fault `openidm-failure-detail`, as
is a `get` index that is not an integer within a Java `int` (on any value,
a null one included), as is calling `getDetail()` on a stub
without `detail`. These
stubs are AIC-ineligible. An AIC-enabled file lease refuses such a case unless
its `aic.unsupported` is `"skip"`, which keeps the local verdict and reports
the observation gap.

For a local method that the mock does not implement, use `bindingOverrides`.
Each value is a JavaScript expression evaluated after the normal seed and
assigned to a known binding, for example
`always: { bindingOverrides: { logger: '({getName: function(){return "probe";}})' } }`.
Use `.run().bindingOverrides({ logger: expression })` for one test, or edit
`request.bindingOverrides` in `beforeRun`. Entries merge by binding name;
per-test values take precedence over suite defaults. These replacements are
local only. An AIC-enabled file lease refuses a case with one unless its
`aic.unsupported` is `"skip"`; with `"skip"` it keeps the local verdict,
reports the observation gap and does not run the tenant lane. This is separate from `given.bindings`, which supplies JSON
seed data to supported mocks.

An override expression can also wrap the original binding and call through to
it. This is useful for a local-only call spy, but the expression is arbitrary
JavaScript: the harness cannot prove that it preserves arguments, return
values, or errors. It therefore remains AIC-ineligible. For the reported
observation-only preambles, use the judged `sessionProperties` effect and the
`esv`/`esvInState` channels instead. A generic AIC-eligible spy would produce
call evidence only on the local lane, with no tenant call trace to compare, so
there is no separate spy channel. Replacements for still-unimplemented
binding behavior remain supported through `bindingOverrides`.

### `scriptName`

The local harness seeds `scriptName` on every run. It defaults to the suite's
`name`; set `scriptName` on `defineSuite` for a different local-only name.
With `useLease(..., { aic: ... })`, the local lane instead uses the exact
generated subject name that AIC uploads (`rl-aic-<hash>-subject`), so scripts
that depend on the binding compare the same value on both lanes. An explicit
suite `scriptName` with AIC enabled fails setup because the uploaded name
cannot be changed to a deployed script's name. A direct low-level `Case` may
still seed `given.scriptName` locally; the one-shot AIC runner rejects that
seed. The one-shot `conform(..., { aic: "tenant" })` path supplies its generated
name to the local runner for the same comparison.
`aicWhenEnabled()` retains an AIC-intent marker when the lane is off, so a
suite with explicit `scriptName` fails setup in both modes. Use an explicit
`scriptName` only for suites that are always local-only.

`cookieName` follows the same fixed-tenant rule. Local-only suites can set it
in `always`, with `.cookieName(name)`, or in `beforeRun` via
`request.cookieName`. With `aicWhenEnabled()`, any explicit author value is
refused even when the AIC lane is off. When AIC is enabled, the lease reads
`/am/json/serverinfo/*` before the local pass and seeds that tenant value;
the one-shot `conform` path discovers it before its local pass too. A failed
discovery is handled like any other tenant failure on that path: the lease
fails the file's setup, as a failed lease `open()` does, while `conform`
reports it as `report.aic.error`, skips the AIC lane and still runs the local
lane, without a cookie name rather than with an invented one.
For a pre-recorded one-shot chain, choose `oneShotRunId`, seed each local
case's `given.scriptName` and `given.loggerScriptId` with
`oneShotSubjectName(oneShotRunId)` and `oneShotSubjectId(oneShotRunId)`, then
pass that ID to `conformChain`; it checks both values and removes the local-only
seeds before invoking AIC.
For a chain produced by `suite.lease`, use
`suite.lease({ runner, oneShotRunId })` **before** calling `.run()`. The lease
seeds both bindings and `chainFromRunResult(result)` carries the run ID and
identity provenance to `conformChain()`. A plain lease result also carries
provenance, so its AIC runner is called; if the script reads its identity,
choose `oneShotRunId` before the local pass to make the names and IDs agree.
Raw author-seeded Cases remain ineligible for one-shot AIC execution. This
changes the migration path for lease consumers: forwarding a lease's seeded
Case alone loses provenance; forward `chainFromRunResult(result)` instead.

In next-gen decision-node scripts, local `logger.getName()` uses the measured
`scripts.AUTHENTICATION_TREE_DECISION_NODE.<script id>.(<script name>)` form.
With an AIC lease, the ID is its deterministic subject UUID. A local-only
suite uses `<unseeded-script-id>` as an obvious placeholder; a raw `Case` can
provide `given.loggerScriptId`. The logger-name form for other script contexts
has not been measured, and the legacy mock keeps its earlier bare name.

### Match a value's shape

An expected JSON value can be a `RegExp` (for a string) or a synchronous
Standard Schema, such as a zod 4 schema. Matchers work recursively in every
state channel's `added` and `changed` values, callback fields, OpenIDM bodies,
and HTTP request bodies:

```ts
import { z } from "zod";

const run = await lease.run().expect({
  outcome: "done",
  sharedState: {
    added: {
      tracking: { id: z.string().uuid(), token: /^[0-9a-f]{32}$/ },
    },
  },
});
```

Use `z.unknown()` when a key must be present but any JSON value is acceptable.
The key is still declared: missing keys, wrong state buckets, extra object
fields, and undeclared mutations still fail. Each lane validates its own value;
the lane comparison ignores value differences only at declared matcher leaves
and keeps presence, location, and every exact sibling strict. AIC can check a
matcher only when that channel is observable. For example, state on a pass
that suspends with callbacks is unverified because the result node did not run.
`allowUndeclared: { sharedState: true }` is the blunt option: it allows all
undeclared shared-state mutations, rather than checking one random key's shape.
Matchers are expectation-only; `given` seeds and HTTP stub replies remain
plain JSON.


**State.** Failure records, the log view and environment profiles can all
contain tenant data. They go to `<project>/.aic-script-tester/`, which writes its own
`*` `.gitignore`; the files are `0600`. `<project>` is the nearest ancestor with
a `.git`. `AIC_SCRIPT_TESTER_PROJECT` overrides the project and `AIC_SCRIPT_TESTER_STATE_DIR`
overrides the directory.

**Bins.** Both commands that talk to a tenant take
`--provider-module <file>`. That is the same setup module, or any module whose
default export is a `TenantProvider`. It exists because a bin runs in its own
process, where the Vitest registration does not reach.

- `aic-script-tester-pull-profile [--tenant <name>]` writes
  `.aic-script-tester/profiles/<tenant>.json`. It prints only counts. The
  profile records each type's properties, `required` (and whether the schema
  has the array at all), `enum`s, `default`s, the `policyId`s of declared
  `policies`, `conditionalPolicies` and `fallbackPolicies`, and the **names**
  of its script hooks (object-level `onCreate`, `postUpdate`, …, and
  property-level `onStore`, …), never their source or conditions. It carries
  `profileVersion: 3`: version 3 keeps any `type` list other than the exact
  `["<type>", "null"]` pair (as `type: "any"` plus `types`) and the `properties` nested under a property,
  so a default the earlier versions erased is still judged.

**Writes on types with hooks or policies (E15b).** The script tester's
`openidm.create`/`update`/`patch`/`delete` write `given.managed` directly: they
run no hook and evaluate no policy. On AIC the same write runs both, so for a
type whose profile declares either, a local pass says nothing (an `alpha_user`
create without `mail`/`givenName`/`sn` is a measured 403 there). Such a write
is a harness fault: `openidm-managed-hook/<type>.<hook>`, naming the first hook
the write would run, in the measured order (a create runs `onCreate`, property
`onValidate`, `onValidate`, property `onStore`, `onStore`, `postCreate`,
`onSync`; an update or patch first reads the record, running `onRetrieve`,
property `onRetrieve` and `onRead`, then the `onUpdate`/`postUpdate`
equivalents of the create chain; a delete `onDelete`, `postDelete`, `onSync`;
hook names the harness does not know count for every write), or
`openidm-managed-policy/<type>` when the type declares only policies
(`required`, an `enum`, property `policies`, `conditionalPolicies` or
`fallbackPolicies`; a delete is not judged on policy). Declarations nested
under a property's `properties` or `items` count as well, since what AIC does
with them was never measured: a nested hook faults every write as
`openidm-managed-unmeasured-hook/<type>.<path>.<hook>`, and a nested policy or
non-empty `required` as `openidm-managed-policy/<type>`. A write a test stubs with `given.openidmFailures`
never reaches the store and does not fault. A type with neither keeps writing
silently, and without a profile nothing changes. A profile whose
`profileVersion` is absent, below 3 or not an integer (`"3"` is a string)
cannot say a type declares nothing, nor describe every default, so every write under it is
`openidm-managed-profile-stale` until it is pulled again. A hand-written
profile cannot be re-pulled: add `hooks` to each type (`[]` for none) and
`noRequiredArray: true` to a type whose schema has no `required` array, add
`default`, `conditionalPolicies` and `fallbackPolicies` to each property the
tenant gives them, record any `type` list other than the exact `["<type>", "null"]` pair as
`"type": "any"` with the list in `types`, and copy any `properties` nested under
a property, then set `"profileVersion": 3`. A type's
defaults are applied on create (`default: null` stores `null`, and `[]` on an
array, as does a `null` sent to that array), and a `null`
sent for a property with a non-null default is AIC's measured `403 Policy
validation failed` on a create of a new record and a harness fault otherwise
(`docs/rhino-local-gaps.md`). If any type's profile records an invalid
`default` (a type mismatch such as `{"type": "boolean", "default": "nope"}`,
any numeric default on `integer` — AIC calls every JSON number `"number"`, so
`3` fails too — or an array item not of `items.type`), every `openidm` call on any `managed/…`
resource faults, reads included, as
`openidm-managed-invalid-default/<type>.<property>`: AIC stores such a config
but keeps serving an earlier one for every type, which the profile does not
describe. A default no probe covered — on any `type` list other than the exact
`["<type>", "null"]` pair, or nested under `items` or `properties` — faults the same way as
`openidm-managed-unmeasured-default/<type>.<property>`. A `null` default is
valid on every type. The IDM emulator refuses the same declarations with the same rule. A profile must key each type by its
own `name`: one whose `objects` key and `name` disagree is refused when read
and when a case is given it. Expect this to fail
client tests that write without stubs; the fault tally counts them, by type
and hook (E19).
- `aic-script-tester-show-log [--stdout]` fetches the logs of recorded failures. It
  writes them to `.aic-script-tester/failures/latest-logs.json`, and opens that in
  `$LOGS_EDITOR` or `$EDITOR` if one is set. `--stdout` prints them instead;
  don't use it in CI.

**Entry points:**

| Import | What it provides |
| --- | --- |
| `.` | the harness |
| `./case` | case definition and judging |
| `./aic` | providers and `setTenantProvider` |
| `./profile` | `pullProfile` and the profile store |
| `./diagnostics` | failure records, `runShowLog` and `loadProviderModule` |
| `./bindings` | the bindings runtime |
| `./runner` | `RhinoRunner` |

More detail lives in this repository's `docs/rhino-local-harness.md`, which is
not shipped with the package.

### Phase timing

To see where a suite's time goes, set `AIC_SCRIPT_TESTER_TIMING` to a file and
summarise it afterwards:

```sh
AIC_SCRIPT_TESTER_TIMING=timing.jsonl npx vitest run
npx aic-script-tester-timing timing.jsonl        # --json for machine output
npx aic-script-tester-timing --share timing.jsonl  # names replaced by hashes
```

Every JVM spawn, pool lease and eval job appends one JSON line. A job row
carries the bytes written (`lineBytes`), the source text shipped with it
(`sourceBytes`, `sourcesShipped`), `stringifyMs`, `roundTripMs` and
`transportMs`, and the JVM's own phases under `jvm`: reader decode, setup,
prelude, preamble (where the seed is parsed) and subject compile and exec,
library compile and load, harvest, host-call wait, and compile-cache hits and
misses. A phase that threw or timed out still reports its time.

Subject execution breaks down further. `moduleBodyMs` is the time library
module bodies ran (a body runs when a script first requires it, inside subject
execution), and `moduleBodies` gives each library its own share, less the
libraries it required in turn. `harvestMs` is collecting the case's result.
`subjectOwnMs` is what is left: the script's own work and the bindings it
called. `libraryLoaderExecMs` is only creating each module's loader function,
not running its body.

`outcome` is the script's own outcome, so a script that caught a harness
refusal reads `"ok"`; `emulationFault` says whether the run failed the fault
audit, and `faultIds` names the faults.

The summary adds a table of module-body time per library and of JVM time per
test source. `--share` replaces every library id and source name with a salted
hash, so the summary can be sent to someone outside the project; the salt is
random unless you pass `--salt=<text>`, which makes two summaries comparable.
Timing is off unless the variable is set, and it never changes a response or a
lane comparison. `src/timing.ts` documents every field.

### Vitest settings for a fast suite

Vitest's default isolation re-evaluates every module in every test file. One
client suite of 97 files evaluated 117 modules 666 times under it, and took
58 s; with `isolate: false` it took 26 s (client measurement, Node 24 and
Java 25 on 4 cores). Both settings are supported: the readiness suite runs the
`forks` and `threads` pools with isolation on and off.

```ts
// vitest.config.ts
export default defineConfig({
  test: {
    isolate: false,
    maxWorkers: 3, // see below
    setupFiles: ["@agiledigital/pingone-aic-script-tester/vitest-setup"],
    runner: "@agiledigital/pingone-aic-script-tester/vitest-runner",
    reporters: ["default", "@agiledigital/pingone-aic-script-tester/vitest-reporter"],
  },
});
```

With `isolate: false`, module-level state in a test file or a setup module
carries into the next file on the same worker, so keep fixtures inside
`defineSuite` and `useLease`. In watch mode the fault tally also spans reruns
(above).

Each Vitest worker borrows one warm JVM from the pool, and in that client's
suite the Node workers were idle 81–83% of the time, waiting on Java. A worker
beyond the core count therefore adds a JVM competing for the same CPUs; they
ran 3 workers on 4 cores. The best number for another machine is not measured
here: try one or two values with phase timing on.

A suite on the local lane does not load the tenant lane's modules; they load
in `beforeAll` only for a suite given `aicWhenEnabled(...)` options with
`AIC_SCRIPT_TESTER_AIC=1` set. That halves the package entry's import time
(63 ms to 33 ms here), which counts once per worker with `isolate: false` and
once per file without it.

## JVM runner client

`src/runner.ts` launches Java 25 from `AIC_SCRIPT_TESTER_JAVA_HOME` or `JAVA_HOME`
(`shell.nix` provides `temurin-bin-25`) and speaks line-delimited JSON.
`AIC_SCRIPT_TESTER_JVM` selects `host` (default), `container`, or `both`; the latter
lanes require Docker and the AM image. The cache defaults to
`~/.cache/aic-script-tester`; `AIC_SCRIPT_TESTER_CACHE` overrides it. Correlate by job `id`
— do not assume the JVM answers in order. A crashed JVM rejects every pending
job rather than hanging. `close()` ends stdin and waits for the process;
`afterAll` should call it.

**JVM reuse.** Under the package's Vitest reporter, `useLease` borrows its JVM
from a pool the reporter keeps in the main Vitest process, so a worker runs
every file it is given on one JVM rather than starting one per file. The pool
keeps one JVM per worker, per lane (two with `AIC_SCRIPT_TESTER_JVM=both`) and
per lease a worker holds at once. A borrowed JVM is returned at `afterAll` and
lent again only if it is idle: one that crashed, retired itself after a spawned
thread survived cleanup, or still has a job running (a lease closed while a
case was still executing) is discarded, and the next file gets a fresh JVM.

What reuse guarantees:

- **JavaScript state never crosses a case.** Every case runs in a fresh
  `initStandardObjects()` scope, and only compiled code is shared.
- **Java-level state crosses a file only on a JVM whose every job ran behind
  the class shutter, which hides the classes that could change it.** A script that can reach `java.lang.System`, `java.util.TimeZone` or
  `java.lang.Thread` can change the JVM itself (a default timezone, a system
  property, a thread it started), and a normal finish does not undo that. The
  next-gen decision-node shutter, which is on by default, hides all of those
  classes. Any job without it (a `legacy`-engine case, `classShutter: false`,
  an IDM-engine script, or a raw `runner.eval` with no `classAllowList`) marks
  its JVM as not lendable, so the file that ran it gets a JVM to itself and the
  next file starts a new one, as it would with no pool.
- **Within one file, cases share a JVM, as they always have.** An unshuttered
  case can still leave Java-level state for a later case in the same file.

`test/harness/isolation.e2e.test.ts` and `pool-files.e2e.test.ts` pin these.

The pool runs in the main Vitest process, so it and the JVMs it launches read
**that** process's environment: set `AIC_SCRIPT_TESTER_JVM_POOL`,
`JAVA_TOOL_OPTIONS` and the like in the shell that runs Vitest, not in
`test.env`, which only reaches workers. Set `AIC_SCRIPT_TESTER_JVM_POOL=0` to
start a JVM per lease as before.

Within a JVM, the case prelude (generated mocks, identity policy, runtime) and
each library body are compiled once and cached by content hash, and each
distinct text crosses the pipe once per JVM, whichever lease, worker or file
sends it: the pool hands each lease the hashes earlier leases shipped. A case therefore costs a few
milliseconds, plus executing the libraries it `require`s, however large the
library map. `node bench/run.ts [--workers=N]` and `node bench/micro.ts`
measure this on invented fixtures.

Job shape (open enough to carry later mock bindings):

- `source` / `sourceName` — author's script; `sourceName` is Rhino's source
  name, so a parse error names the author's file and line, not `<eval>`
- `engine` — `"am"` (default) or `"idm"`
- `globals` — AM JSON values placed in ENGINE_SCOPE Bindings
- `preamble` — evaluated first so concatenating mocks into `source` is not
  needed (that would shift line numbers)
- `prelude` — optional; evaluated before `preamble` and compiled once per JVM
  per distinct text. `preamble` continues its line numbers
- `preambleData` — optional string, visible to the preamble only as
  `__rhinoLocalPreambleData`; per-case data travels here rather than as a
  literal that would be compiled every case
- `libraries` — optional `require()` id → body, compiled once per JVM per
  distinct body. The runtime's `require` reads them; an undeclared id is
  still refused
- `timeoutMs` — per-job; AM's `Error("Interrupt.")` past the instruction
  observer. Omit for the runner's 10s harness default; `0` disables timeouts

Outcomes are machine-readable and distinct: `ok`, `compile_error`,
`runtime_error`, `timeout`.

### IDM scripts and re-entrant host calls

The IDM lane uses ES6 (200), compiled optimization level 9, the plain Rhino
feature set, and a Context locale of `en`. AM retains its interpreter and
`en-US` Context locale. `timeoutMs: 0` uses an actual plain `ContextFactory`
and observer threshold 0, matching the measured IDM configuration. Positive
timeouts enable instruction checks in compiled bytecode, including tight loops;
they also bound waits for host replies. Java code called by a script is outside
the instruction observer. Both engines run on the runner's Java 25 runtime;
the measured tenant IDM runtime uses Java 21.

Each IDM job has a fresh shell `Global` above a deferred binding scope. Named
bindings resolve; enumerating the global with `for (p in this)` fails the job
with an uncatchable Java `UnsupportedOperationException`. Fresh `Global`
initialization measured about 0.063 ms after warmup, so globals and standard
prototypes remain isolated per job.

```ts
import { HostCallError, RhinoRunner } from "@agiledigital/pingone-aic-script-tester/runner";

const runner = await RhinoRunner.spawn();
try {
  const response = await runner.eval({
    engine: "idm",
    source: "object.label = openidm.read('managed/example/1').label; object",
    bindings: { object: {}, resourceName: "managed/example/1" },
    resourceNameBindings: ["resourceName"],
  }, {
    onCall: async (call) => {
      // This may await runner.eval(...) to run a hook that itself calls out.
      if (call.method === "read") return { label: "example" };
      throw new HostCallError({
        class: "org.forgerock.json.resource.NotSupportedException",
        message: "example handler only supports read",
        code: 405,
        detail: {},
      });
    },
  });
  // response.value is JSON; response.bindings includes mutations to seeded maps/lists.
} finally {
  await runner.close();
}
```

`bindings` supplies whichever top-level names the caller needs. Maps and lists
are Java-backed replicas: map own-property names/descriptors are empty even
though enumeration works, assigned objects become `HashMap`s, and lists pass
`Array.isArray` while direct `JSON.stringify(list)` produces indexed objects.
Map JSON serialization turns nested lists into arrays. `resourceNameBindings`
marks string-valued bindings to wrap as Java resource-name objects.

`openidm.read/create/update/patch/delete/query/action` emit host calls with
converted JSON arguments. Numeric-path metadata outside the user value preserves
NaN, infinities and negative zero in internal protocol frames, including nested
endpoint arguments/results; literal data fields cannot collide with the metadata.
This internal choice is labeled assumed because tenant special-number transfer
has not been measured. Endpoint HTTP output retains its measured serializer.
The emulator opts into `internalValues` for raw completions/bindings while the
ordinary runner's `value` remains endpoint wire output. Function arguments are dropped; a final query callback
runs on the JVM once per returned row, with `result` and `resultCount` removed
from the query completion. Resource routing, missing-resource behavior, query
validation, and storage policy belong to `onCall`; the experimental IDM core
below provides that handler. A job without `onCall` fails with a protocol diagnostic. A handler must return a JSON value (use `null` for no result).
Throw `HostCallError` to preserve a CREST class, message, numeric code, reason and detail;
the script receives a Rhino `JavaException` whose `message` names the CREST
class you named, with `javaException.getCode()`, `getReason()` and
`getDetail()`, including in joined shell `spawn` threads. Reading
`javaException.getClass` faults, although AIC's `getClass().getName()` is
measured: the emulator cannot reproduce the Java method and class faithfully,
so match on `e.message` instead. `String(getDetail())` is
CREST's one-line JsonValue form, and a `null` detail is a Java null; any other
member of `javaException` (read, tested with `in`, or enumerated; `in` is `true`
for the members above), and a detail spelling no probe captured (an empty
object or array, a string needing an escape), faults
`idm-exception/unmeasured`, because the exception is not the CREST class it names. Ordinary handler errors
are internal emulation failures; use an explicit CREST HostCallError for a
catchable tenant failure.
Emulation failures (missing behavior, admission/protocol failures and harness
timeouts) carry structured `emulation` diagnostics and always fail the outer
job/request. On the pinned Rhino, feature 13 is off and the Java `Error`
used for these failures escapes script catches. Tenant CREST exceptions remain
catchable. Nested runner failures survive adapters that ignore their response;
policy and hook mappings preserve the same failure class. The asynchronous fatal
scope spans runner instances along a logical call chain. A job's sticky timeout
state includes host waits and instruction observers on joined shell threads;
`finally` returns and child-thread completion cannot erase a recorded failure.

IDM responses add `bindings`, ordered `logs` (`surface`, `level`, `message`,
`args`), and, on failure, `thrown`. `thrown.kind` distinguishes `object`, `error`,
`string`, `primitive`, `javaException` and raw `java` failures; `value`, `class`,
`code`, `message` and `detail` retain the information the emulator needs to apply
its hook/endpoint HTTP policy. A string-valued `code` stays a string. AM's
response shape is unchanged. IDM completion objects are returned as JSON;
undefined completions have `valueKind: "undefined"` and `value: null`.

`logger` and `console.log` append to `logs` without host calls. Shell `print`
writes to stderr to protect the protocol. `identityServer.getProperty` reads the
job's optional `properties` map, returning the supplied default for missing names
and recording `assumed: property <name> absent` in the response. `require`, `parseFilter`, `hash`,
`encrypt`, `isHashed`, `matches`, `decrypt`, and `isEncrypted` fail explicitly
with `rhino-local: <name> is not emulated yet`.

In the `both` lane, the primary runs first and invokes `onCall` once. The shadow
replays snapshots of the primary replies, checking method, arguments and sequence
as well as the final response. Extra, missing or different calls raise
`LaneDivergenceError`, even when the script catches the failure and returns the
same final value. Nested evaluations use the same policy. The primary transcript
is frozen when its response arrives. JVM responses name emitted host sequences
whose replies the worker never consumed; replay withholds those replies even
when a synchronous Node handler returned before Node read the timeout response.

The stdin reader dispatches jobs to worker threads and routes replies to waiting
calls; stdout writes are serialized. Jobs may complete in any order. IDs must
be unique among active jobs; sequence numbers start at 1 for each job.
Admission uses at most 128 job workers by default; excess jobs receive `protocol_error` immediately, including nested
evaluations. There is no queue that can deadlock behind blocked parent jobs.
`RhinoRunner.spawn({maxConcurrentJobs})` sets the limit. Capacity refusal on
either lane is reported as an emulation failure, not a script divergence.
A primary refusal is returned without admitting the job on the shadow.
Shutdown stops admission, keeps routing replies, and waits up to 5 seconds for
accepted jobs; remaining jobs receive `protocol_error` before the shutdown
acknowledgement and process exit. `shutdownWaitMs` sets that bound; client
public `close()` first drains accepted evaluations, including shadow replay,
while permitting their nested callbacks and replies. It then requests protocol
shutdown instead of closing stdin. Each phase is bounded by `shutdownWaitMs`;
process termination has one extra second of grace and a two-second SIGTERM bound.

Existing job/response records remain untagged. The new records are:

```jsonl
{"id":"outer","engine":"idm","source":"openidm.read('managed/example/1').label"}
{"type":"call","id":"outer","seq":1,"method":"read","args":["managed/example/1"]}
{"type":"reply","id":"outer","seq":1,"result":{"label":"example"}}
{"id":"outer","outcome":"ok","valueKind":"string","value":"example","error":null,"logs":[],"bindings":{}}
```

A failed call uses an error reply instead of `result`:

```jsonl
{"type":"reply","id":"outer","seq":1,"error":{"class":"org.forgerock.json.resource.ForbiddenException","message":"denied","code":403,"detail":{"policy":"example"}}}
```

## Experimental IDM emulator core

`src/idm/index.ts` exposes `IdmConfig`, `IdmEmulator` and their REST/config types
inside the workspace. There is deliberately no new npm export yet: cross-tenant
conformance should settle this contract before it becomes a package entry point.
The core has no dependency on a test framework.

```ts
import { IdmConfig, IdmEmulator } from "./src/idm/index.ts";

const config = new IdmConfig({
  sync: { mappings: [] }, // required: the tenant's config/sync, or [] for none
  managed: { _id: "managed", objects: [{
    name: "widget",
    schema: { properties: { label: { type: "string" } } },
    onCreate: { type: "text/javascript", source: "object.label += '-created';" },
  }] },
  endpoints: { example: { type: "text/javascript", source: "({ok:true});" } },
});
const idm = new IdmEmulator(config, runner);
const result = await idm.request({
  method: "POST", path: "managed/widget", query: { _action: "create" },
  body: { label: "sample" },
});
// result: {status:201, body:{_id, _rev, label:"sample-created"}, provenance:[...]}
const isolated = idm.fork(); // shares frozen config and runner; deep-copies state
```

Paths can have a leading `/openidm/` or use resource names directly. Query values
are already decoded. Inline scripts use the tenant's `{type, source, globals?}`
shape; endpoint configs can instead wrap one in `{type:"scripted", script}`.
Managed schemas retain object hooks, property hooks and custom `actions`.
`config/managed` and `config/endpoint/<name>` are readable snapshots; config writes
fail as missing behavior. A script `file` reference and a `require(id)` name one
of IDM's own runtime scripts, which come from the `runtimeScripts` provider
(below). The caller owns the runner and closes it after every emulator/fork
using it has finished.

Namespace roots (`managed`, `endpoint`, `config`, and `config/endpoint`) are
unmeasured and fail as `missing`, regardless of completeness declarations.
Unimplemented routes fail as `missing`. Supplying managed types or endpoint
implementations covers only the listed names; unlisted names remain unknown,
including with an empty map. To model genuinely absent names, explicitly declare
namespace completeness:

```ts
new IdmConfig({
  sync: { mappings: [] },
  managed: { objects: [widgetType] },
  endpoints: {},
  complete: { managed: true, endpoints: true },
});
```

Both flags default to false. Managed config is required, but even a full
`managed.json` pull needs `complete.managed: true`: the emulator cannot distinguish
that document from a partial fixture by its shape. A caller loading a complete
pull can make that declaration explicitly. Endpoint completeness requires an
explicit endpoint map, including `{}` for an empty namespace. Omission or
undefined means no endpoint snapshot was supplied. Namespace containers must be
plain objects (ordinary or null prototype); null, arrays, primitives and class
instances are configuration errors at construction, before normalization.

`repo` takes the tenant's `config/repo.ds` document, optionally. It is how the
emulator knows a type's storage kind (`IdmConfig.storage`): a type that
`genericMapping` names, or that `managed/*` covers without an
`explicitMapping` entry, is stored as JSON. There a quoted literal never matches
a number and numbers compare by value, `7.5` included, as measured on
2026-11-06. A property that the type's `genericMapping` entry maps to an
attribute of its own is excluded. Without `repo`, or for a type in both
sections, those queries stay the `openidm-query-filter` fault. The measured
shape is a type covered by `managed/*` alone; a type with its own
`genericMapping` entry is decided the same way, labelled `assumed`. Pass the
whole document: a partial `repo` holding only `managed/*` makes every non-user
type generic, including types the tenant maps under `explicitMapping`, and the
emulator cannot tell.

`sync` takes the tenant's `config/sync` document (`{mappings: [...]}`), and
it is **required**: omitting it is a type error and a `TypeError` at
construction. Pass `{ mappings: [] }` to state that the tenant has no
mappings; like `complete.managed`, that is your claim and the emulator records
it rather than checking it. There is no loader that builds an `IdmConfig` from
a pull yet (the profile pull reads only `config/managed`, for types), so take
`sync` from the same `config/sync` read as the rest of the tenant's config.

On AIC a committed managed write on a mapping's `source` runs that mapping
(implicit sync, `docs/api/16-sync-mappings.md`): synchronously by default,
measured 2026-06-19, and asynchronously from the sync queue when the mapping
enables `queuedSync`, measured 2026-07-29. What `enableSync: false` does to
implicit sync is unmeasured. The emulator models none of these, so a create,
update, changed patch or delete on a type some mapping names as its source is
the harness fault `idm-implicit-sync/<mapping>`, raised before the store
changes, and its message says for each mapping which of the three it is. A
patch that changes nothing writes nothing and does not fault. A write on a
type no mapping names as its source records that the supplied `config/sync`
has no mapping for it.

With explicit completeness, an unlisted name reads as null internally and queries
throw NotFoundException. Each such absence carries `assumed: caller declared
managed namespace complete` or `assumed: caller declared endpoints namespace
complete`: the emulator cannot verify the caller's claim. The null read and
managed-query diagnostic are measured by orchestrator p24 (2026-11-02); endpoint
query diagnostic reuse is assumed. Endpoint completeness also covers absent
`config/endpoint/<name>` snapshots (assumed). REST absence uses 404 (assumed).
Declare endpoints complete only with the tenant's whole `config/endpoint`
pull: AIC also ships endpoints of its own (`oauthproxy`, `gettasksview`,
docs/api/11-idm-endpoints.md), and a map built from your custom endpoints
alone, declared complete, makes those read as absent locally where AIC answers
them.
Forks preserve the declarations. Unknown general `config/*`, `system/*` and other
namespaces remain missing, even though p24 observed null for genuinely absent
tenant resources there: the emulator was not given those namespaces.
An absent id in an implemented collection continues to read as null. The built-in collection actions `patch`, `triggerSyncCheck` and
`updateLastSync` are explicit missing boundaries. Readable ids remain supported
except on measured `alpha_user` and `bravo_user`, which require UUID syntax.

Declaration shape/location validation runs first across supplied configuration,
before routing. Tuple/array-form `items`, malformed schemas/scripts and nested
property hooks/file references/defaults are missing; only immediate property
hooks/defaults are implemented. Type-mismatched defaults, including recursively typed object/array members, are
missing until schema activation/unavailability is modeled; nested activation is
unmeasured, and the scalar 404 measurement is not generalized. Routing, known absence, and implemented
rejections (including unknown collection actions and invalid query selectors)
then run before storage-capability checks. Schema `id` remains unmeasured and
refused when storage is reached; it cannot block a known-absent record read or
the implemented unknown-action rejection.

Updates retain pre-read/retrieval hooks before revision validation, including
stale revisions. The stale-case ordering is explicitly assumed until measured;
a stale revision cannot hide an unsupported retrieval hook.

Storage capability checks retain the conservative whole-schema refusal rule. Writes with
`encryption` or `secureHash`, and reads/writes with `isVirtual`,
`returnByDefault`, `viewable`, `userEditable`, `scope`, unknown types or declarations
are missing until implemented and measured. Checks include nested properties and
array items; they conservatively refuse the declared capability even if that
request supplies no value. Relationships remain missing. `searchable` is the
measured UI/schema setting, not an index declaration. Display metadata and supported immediate defaults/hooks remain implemented. Declared `enum`, `required` and `policies` require the
explicit policy evaluator on writes; without one they are missing rather than
silently accepted. Schemas without such declarations get the implicit checks
described under "Policy" below (E15a). HTTP reads/queries omit `alpha_user.password`;
internal calls expose a 90-character `{PBKDF2-HMAC-SHA256}`-prefixed string
(orchestrator p22/p23, 2026-11-02). Its opaque body is assumed: 70 hex characters
from a randomly keyed HMAC-SHA512, stored at commit. Orchestrator p25 (2026-11-02)
measured verbatim hash retention through full-object update, password PATCH and
copying the hash into a second user's create. The emulator preserves its issued
prefixed tokens in all three forms, independently of the destination's old value.
Token recognition is shared by forks. Since the tenant body grammar is unmeasured,
other prefixed bodies (even plausible 70-character bodies) fail as missing; no
format validator is guessed. Foreign `{SCHEME}` prefixes are also missing.
This does not model IDM's salt/iteration encoding or password verification.
Commit timing and explicit HTTP password projection omission remain assumptions.
Unmeasured non-string password values or retrieval-hook changes to its
representation are missing. `openidm.matches` remains missing: p25's observed
false results do not establish verifier semantics.
`bravo_user.password` remains missing; no cross-type password rule is inferred.

Results add `provenance: {kind, behavior, source?}[]`, deduplicated per request.
`measured` entries cite the verified API docs; `assumed` entries identify the
choices the measurements leave open. A reached `missing` entry fails the request
with 501 and names the behavior. Nested calls share provenance, including a
missing call caught by script code; concurrent requests use separate scopes.
Both `IdmEmulator` and `RhinoRunner` expose detached `emulationFailures` snapshots
with a sequence, context, owning test and failure cause. Entries go to one
process-wide registry and cannot be deleted. Forks share a family journal for
inspection; verdicts belong to the test that started each request or evaluation,
including when callbacks finish during a later test. Replacing or clearing a
fixture variable does not remove its evidence.

The supported Vitest integration uses all three package entries (Vitest 5):

```ts
// vitest.config.ts
export default defineConfig({
  test: {
    setupFiles: ["@agiledigital/pingone-aic-script-tester/vitest-setup"],
    runner: "@agiledigital/pingone-aic-script-tester/vitest-runner",
    reporters: ["default", "@agiledigital/pingone-aic-script-tester/vitest-reporter"],
  },
});
```

Vitest has one `test.runner` slot. To retain a client's runner behavior, adapt
that behavior into a subclass of the package runner and select that subclass:

```ts
// test/client-runner.ts
import EmulationRunner from "@agiledigital/pingone-aic-script-tester/vitest-runner";
import type { RunnerTask } from "vitest";

export default class ClientRunner extends EmulationRunner {
  override onAfterRunTask(task: RunnerTask): void {
    // Perform the client's task bookkeeping here.
    super.onAfterRunTask(task); // Preserve the final emulation verdict.
  }
}
// vitest.config.ts: test.runner = "./test/client-runner.ts"
```

When overriding them, preserve calls to `super.onBeforeCollect`,
`super.importFile`, `super.onCollectStart`, `super.onAfterRunTask`,
`super.onAfterRunSuite` and `super.onAfterRunFiles`. Await superclass imports and
async hooks; retain final verdict hooks after any client result changes.
Leave the `onTaskUpdate` transport hook to Vitest. An unrelated runner cannot
also occupy that single slot; port its hooks into this subclass.

Other setup files and reporters coexist in their normal arrays. Append client
setup files to `setupFiles` and client reporters to `reporters`, retaining the
three package entries above. Asynchronous reporter hooks are supported:
imports made before collection starts bypass readiness, while imports for each
new collection file await its readiness, with a timeout
bounded by `hookTimeout` (10 seconds when disabled). A client overriding a
collection hook without its superclass call receives a readiness error.

The setup wraps each test and its hooks in an async ownership context, then checks
the global registry after cleanup. It covers every runner and emulator without
naming sources. Vitest 5's `aroundEach` preserves separate contexts for concurrent
tests, including sibling forks; a clean sibling does not inherit another test's
failures. `useLease` also installs ownership guarding automatically. Legacy
`useEmulationGuard()` calls remain supported; their source arguments are ignored.
The custom runner publishes audit deltas in task metadata through Vitest's
`onTaskUpdate` hook: each boundary sends only the entries recorded or changed
since the last acknowledged publish, plus any earlier ones not yet
acknowledged, and the reporter merges them into one ledger per worker, newest
revision of each entry winning. The hook resolves only after the main process
has handled the update, so an entry is dropped from the worker's resend set
only once the reporter holds it; a worker that crashes or times out loses
nothing it had already published. `AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS=1` sends
the whole audit at every boundary instead, as earlier releases did; it is a
diagnostic fallback and gives the same verdict. Its `onAfterRunFiles` check
runs outside test control flow; the reporter checks the ledger in the main
process at `onTestRunEnd` and
sets a non-zero exit status, naming every unexpected cause and its original
owner. It also handles updates received after that final check. This backstop
works in the `forks` and `threads` pools with either isolation setting. Import-time
records and outstanding work fail their file when attributable; unowned evidence
always fails the run. Late callbacks retain the file and owner where their work
started, even if those checks have already finished.

The runner marks an owning test failed after Vitest's expected-failure inversion
and retry handling, and always marks its file failed when it contains unexpected
emulation failures. Dynamic skips can remain displayed as skipped; their file
still fails. These are persistent task results, so
`dangerouslyIgnoreUnhandledErrors` cannot suppress them. Named expectations
continue to allow intentional failures.

The setup requires both the custom runner and the main-process reporter. The
reporter announces its presence with Vitest's public `provide`/`inject` context;
setup refuses a missing reporter, including when CLI `--reporter` selection
replaces the configured list. Keep the package reporter alongside other
reporters, including on the CLI. The final worker throw is supplemental; the
failed task states and independent main-process verdict enforce the result.
Runners and reporters are trusted configuration; spoofing the presence handshake
or forcing `process.exitCode = 0` from a reporter is out of scope.

Direct use without this configuration remains possible. `useLease` and legacy
helpers install the per-test guard only; they cannot install a main-process
reporter from inside a worker. Core users in another framework must enforce
their own run verdict over the registry.

An unfinished request, evaluation or accepted host callback fails immediately as `outstanding work at
test end`. The guard does not wait for a gate that only later hooks might release.
Late completion keeps the original owner. A shell `spawn` child still alive when
its job finishes reports `missing: spawned thread outlived its job` before the
result is published; detached-thread tenant semantics are unmeasured. Joined or
already completed children remain supported. Detached cleanup has a 100 ms total
bound and interrupts script observers / Java waits; daemon children cannot hold
JVM shutdown open. `spawn` with `timeoutMs: 0` is refused as uncatchable
`missing: spawn with disabled deadline`: bytecode compiled without observer
checks cannot gain them in a child. A deadline-disabled parent keeps its measured
plain context when it does not spawn. With a deadline, joined children retain
measured language 200, optimisation 9, features 13/21 off, closure visibility,
`openidm` and throwing-child join behavior (orchestrator p20, 2026-11-02).
The harness observer threshold 1000 and custom deadline factory are assumed
execution controls, differing from the measured tenant child threshold 0/plain
factory. If any child survives the bounded cleanup (for example an
uninterruptible Java wait), that JVM is retired: subsequent evaluations receive
an explicit protocol refusal. Close that runner and create another; shutdown
remains available. It never accepts a new job in that contaminated JVM.

Log appends and response snapshots are synchronized. Shared Java-backed binding
maps/lists synchronize each operation and copy each container under its own
lock for serialization. Wrapper identities remain cached per graph. Compound
script operations such as concurrent read-modify-write remain subject to normal
interleaving; graph-wide atomicity and thread log order are not inferred.
Log argument snapshots at emission carry an explicit assumption label.

Tests deliberately exercising an unsupported feature declare its named cause:

```ts
import { expectEmulationFailure } from "@agiledigital/pingone-aic-script-tester";
expectEmulationFailure("named _queryId: intentionally-unsupported");
await idm.request({ method: "GET", path: "managed/widget",
  query: { _queryId: "intentionally-unsupported" } });
```

Each declaration covers exactly one cause: an exact missing behavior name, or
an exact failure message for other failure kinds. A regular expression must
match the entire cause name and binds to one cause, even if it matches several.
Declare every cause in an aggregated result separately. Repeated observations
of the same cause through nested mappings need only one declaration; the
`runtime helper: ` provenance prefix is normalized to that underlying cause.
Declarations apply only to the current test, retain the entries with an
`expected` annotation once all their causes are covered, and fail the test if
the declared cause never occurs. It cannot waive outstanding work. There is no reset
API. Ordinary tenant failures, including tenant 501, are not recorded. The
emulator's diagnostic 501 response alone is insufficient to validate a negative
test; the setup enforces the independent verdict.

Some hooks can commit nested writes before a later failure. A post-hook failure
after repository commit reports `assumed: post-hook failure after commit keeps
the write`; successful post hooks do not add that failure label. This is not a transactional clone of
DS. See [IDM-DONE.md](IDM-DONE.md) for the complete assumption/boundary inventory.

The optional `rawBody` is authoritative when present and `body` is then null.
This preserves both the measured invalid JSON bytes (`{"_id":""`) for scalar/
array endpoint reads and the empty-body 500 for a string-valued thrown `code`.
A transport must send those bytes verbatim, including the missing closing brace.

Attach `idm.onCall` to `runner.eval(..., {onCall})` for standalone script callers.
It maps all seven host operations into the same core, returns null for missing
reads, and throws `HostCallError` with CREST class/code/detail otherwise.
`idm.hostRequest(call)` exposes a standalone call's body/status/provenance when
the host needs to retain its evidence; `onCall` returns only the script value.
Evaluation results now retain host-side evidence too: `provenance` preserves
kind, behavior and source, while `assumed` and `measured` include host behavior
summaries alongside JVM runtime evidence. This covers successful and caught-error
calls, `hostRequest`/REST adapters, bound hook callbacks and nested evaluations,
even if an adapter ignores a nested result. Metadata is carried on host replies
and recorded by the JVM only when consumed. Shadow replay uses the identical
reply metadata; late unanswered replies cannot change the completed report.
Missing metadata remains fatal even if an adapter returns an error body as data.

REST-driven scripts automatically aggregate all their nested evidence. Missing
host behavior also travels in error metadata to the outer JVM job: its response
contains `missing` and a non-`ok` outcome without requiring `reportMissing`.
Script catches cannot handle that failure as a tenant error. Independent jobs retain separate diagnostics.

Managed create applies declared property defaults to omitted keys before policy;
explicit null is retained and updates never apply defaults. Placement before
create hooks and generic JSON default handling are labeled assumptions.
`onCreate` and `onUpdate` preserve the measured `object === newObject` alias.
JSON pointers traverse own properties only; literal `__proto__`, `constructor`
and `prototype` fields are ordinary data (assumed). Paging returns authenticated
opaque continuation cookies and the measured count/policy envelope. Cookies
re-evaluate offset pages; stability under concurrent data changes is assumed.
Cookie parsing rejects trailing data. Forks copy the signing key: a parent cookie
works on an unchanged fork, and after divergence it remains valid but re-evaluates
the fork's own rows with the encoded offset. Updates and deletes guard the captured
repository entry identity as well as revision, so deleting/recreating the same ID
cannot satisfy a stale guard. Visible recreated revisions restart at `0`, assumed.
Config snapshots have null prototypes; routes, actions, hook slots and document
property lookups require own fields. Property-hook writeback defines own data
properties, preserving literal prototype-named keys.

Constructor options can supply `{properties: {"esv.feature": "on"}}`. Each
emulator copies this map; forks inherit the snapshot. Property lookups missing
from that map carry the absence assumption on the request's provenance.

Policy: `policy.js` is **not run**. Without an evaluator, a validated
create/update/changed patch is checked against IDM's implicit schema policies
where they were measured, and faults where they were not (E15a,
`src/idm/policy.ts`):

- Measured on a create of a new record (`docs/api/10-managed-objects.md`, the
  2026-11-08 probe over REST and from an IDM script), and emulated as
  `403 Policy validation failed` in doc 10's envelope, one
  `failedPolicyRequirements` entry per failing requirement, e.g.
  `{"code":403,"reason":"Forbidden","message":"Policy validation failed","detail":{"result":false,"failedPolicyRequirements":[{"policyRequirements":[{"params":{"invalidType":"null","validTypes":["string"]},"policyRequirement":"VALID_TYPE"}],"property":"<name>"}]}}`.
  An IDM script's `openidm.create` catches it as CREST's
  `ForbiddenException` with `getCode()` 403 and the same `getDetail()`:
  - **only the top-level `type` is checked**: a value whose JSON type is not
    the declared `string`, `boolean`, `number`, `integer`, `object` or `array`
    is `VALID_TYPE` (`invalidType`, `validTypes`). Every JSON number is
    `"number"`, so **any number sent to an `integer` is refused**, `3` as much
    as `3.5`. Schemas under `properties` and `items` are **not** enforced, and
    nested values are stored as sent;
  - a `null` on a property with a non-null `default` is `NOT_NULL`;
  - a `null` passes the type check only when the schema's `required` key
    exists and does not name the property (without an evaluator a `required`
    array can only be empty). With no `required` key it is `VALID_TYPE`,
    `invalidType "null"`, after any `NOT_NULL` for the same property;
    otherwise an undefaulted property's `null` is stored as `null`;
  - a `default: null` is applied on create, and a `null` sent to such a
    property passes with no `NOT_NULL`; on an array both store `[]`. With no
    `required` key a string's sent `null` is still `VALID_TYPE`, while its
    applied default is stored.
- A PATCH `remove` of an undefaulted, non-required property commits (measured
  200).
- Faults (`idm-policy/*`), because unmeasured: a mismatch on any write but a
  new record's create, and any value for a property with no `type`
  (`idm-policy/valid-type`); a defaulted `null` outside a new record's create,
  or an undefaulted one when the schema has no `required` array
  (`idm-policy/not-null`, `idm-policy/valid-type`); a non-string's
  `default: null` when the schema has no `required` array, and a `null` sent
  to a null-defaulted array outside a create (`idm-policy/valid-type`); a
  property whose `type` lists two concrete types is not checked at all; a
  refusal naming more than
  one property, whose order is unmeasured; a PATCH `remove` of a defaulted
  property (`idm-policy/required`); and an item of a top-level array that is
  not the declared `items.type` (`idm-policy/valid-array-items`: only nested
  `items` were measured unenforced).
- A `null` on an undefaulted property outside a create, where a create would
  accept it, is accepted with that assumption (the probe measured create only).
- A PATCH is checked only on the properties its operations name or whose value
  changed, as AIC's enum check is measured to do.

Every pass records the measured outcomes it relied on, plus
`assumed: policy.js not run: no config/policy resource policies apply to this
write`. The optional constructor
`{policy: evaluator}` seam replaces all of this, and runs between validation hooks and store hooks. It can
throw `HostCallError` to reject with a CREST status and detail; its provenance is
`assumed: caller-supplied policy evaluator`. It is intended for the real
`policy.js`, not a TypeScript reconstruction of it.

Runner jobs can opt into `reportMissing: true`, which adds a `missing: string[]`
diagnostic even for caught uses of unsupported runtime helpers. The emulator
uses this to enforce its fidelity boundary. Reached missing behavior is always
reported; the option also requests an empty diagnostic list on complete jobs. `crestBindings: true` supplies the measured request class tags and
case-insensitive HTTP header maps. These optional wrappers and missing-use
diagnostics are the slice's Java changes; the IDM engine itself is unchanged.

### IDM runtime scripts

IDM runs scripts of its own: about 60 files under the tenant's
`/opt/openidm/bin/defaults/script` (`policy.js`, `roles/effectiveRoles.js`,
`onCreateUser.js`, `router-authz.js`, …) plus the engine's bundled `lib/lodash`,
`lib/handlebars` and `lib/validator`. A tenant reaches them from configuration,
either as a hook's `{type: "text/javascript", file: "roles/onDelete-roles.js"}`
or from an inline one-liner such as `alpha_user.onCreate`'s
`require('onCreateUser')`. This package does not ship them. The project
supplies them, through a provider:

```ts
const idm = new IdmEmulator(config, runner, {
  // A directory named by IDM's relative paths: <dir>/roles/effectiveRoles.js.
  // Absolute, or a file: URL, so its base never depends on the process cwd.
  runtimeScripts: new URL("./idm-runtime-scripts/", import.meta.url),
  // …or a loader: (scriptPath) => string | undefined (it may return a promise)
  // runtimeScripts: (path) => myScripts[path],
});
```

The intended directory is a fetch from the tenant, run when you choose and
never at test time: `aic idm runtime-scripts pull` (planned; until it ships,
copy the files by hand in the same layout). Commit the output beside the rest
of the configuration. Test runs then stay offline, and a later re-fetch shows
ForgeRock's changes to these scripts as a diff. The directory holds each
script at its IDM path, byte for byte, plus a `manifest.json` recording the
tenant, the fetch time, the IDM version and each file's SHA-256. The emulator
does not read the manifest. `test/fixtures/idm-runtime-scripts/` is an
invented example of the layout.

A directory provider is checked and indexed once, when the emulator is
constructed: a relative path, one that is not a directory, or a tree with a
subdirectory it cannot read, is a `TypeError` naming it, and a file added
afterwards is not seen (its fault says the file is missing from the index made
at construction). Names are looked up exactly
as AIC's case-sensitive loader does, so a name that matches a file only in
another letter case is unsupplied (the message names the near match), as is a
directory's name. Symbolic links are never followed: a name that is a link, or
passes through a linked directory, is `idm-runtime-script-path`, because a link
can stand in for something that is not the tenant's bytes. That holds at read
time too: a file or directory replaced by a link after construction is
`idm-runtime-script-path`, because the read opens the file without following a
final link and then checks that the open file is the one at the canonical path
under the root. Forks share the index.

A `file` reference runs the supplied source with the bindings an inline script
in the same place gets. `require(id)` loads `<id>.js` (or `id` if it already
ends in `.js`) as a CommonJS module, following IDM 9.0.0's engine as read from
its bytecode rather than measured on AIC. The module sees the caller's bindings
(`openidm`, `object`, `request`, …) and keeps its own top-level `var`s. The
module cache is keyed by the id as written, as IDM's is: within one script
invocation `require('x')` runs `x.js` once, but `require('x')` and
`require('x.js')` are two instances and each runs the body. Everything runs
again in the next invocation. A require cycle sees the partial exports: the
original `exports` object, not a replacement already assigned to
`module.exports`; once the module returns, `require` answers
`module.exports` (a primitive comes back as its object wrapper). A module that
throws is not cached. These rules are recorded as `assumed` provenance. Two
differences from IDM are deliberate and also recorded: IDM preloads
`lib/lodash`, `lib/handlebars` and `lib/validator` once per engine, shares
their exports and runs them without the caller's bindings, where here they run
per invocation and see the caller's bindings; and a relative id (`./x`) is
refused, because IDM's resolution of it is unmeasured.

A script the provider does not supply is the harness fault
`idm-runtime-script/<path>`, for example `idm-runtime-script/onCreateUser.js`.
This includes the case where no provider is configured at all. The message
names the script, how it was reached (`onCreate file reference`,
`require("onCreateUser")`) and how to fetch it. As with every harness fault, a
script that catches the failure still fails the request with 501, and the run's
fault tally counts it; a fault raised inside a nested write keeps its id on the
outer result's `emulation.faults`. A name that is not a canonical path under
the script root (absolute, a `.`, `..` or empty segment, a trailing slash, a
backslash or a NUL byte) is the fault `idm-runtime-script-path`. A loader that
throws or returns something other than a string or `undefined`, or a listed
file that cannot be read, is `idm-runtime-script-provider`. A script declaring
both `file` and `source` is `idm-script-file-and-source`. Supplying a
script does not make the behaviour around it emulated: `policy.js` in
particular is not wired. Writes go through the `policy` evaluator seam or the
implicit checks under "Policy" above.

`runner.eval({ engine: "idm" })` called directly, outside the emulator, keeps
the old behaviour: `require` is the `missing` behaviour `require`. The emulator
opts its own jobs in with `idmRequire: true`, which turns `require` into a
`require` host call that it answers from the provider.

## Artefacts

Committed under `generated/`:

- `scripted-decision-mocks.cjs` — evaluated inside AM's Rhino as part of the
  script-under-test's scope
- `scripted-decision-mocks.d.ts` — TypeScript types for case authors

They are committed on purpose. Regenerating after Ping adds a method produces a
diff of the surface, which is the point of generating rather than hand-writing.

## Re-run

From this directory (Node 24):

```bash
npm install
npm run generate
npm test
npm run typecheck
npm run lint
npm run lint:am
npm run measure   # JVM startup + per-job timings
npm run show-log  # fetch AIC logs for a failed test (needs `aic login`)
```

When a test that hit the AIC lane fails, `useLease` appends a JSONL record to
`.aic-script-tester/failures/failures.jsonl`. `npm run show-log` lists those records
newest first and fetches their logs from the tenant provider: the `aic` CLI's
`aic logs tx`, or the log API when log keys are configured. It fetches the
stem first, so one call covers a whole authenticate chain, and it never issues
a range query. It writes the logs to the 0600 view described above.

## Per-file AIC lease

Opt a suite into automatic local-versus-AIC checking through `useLease`:

```ts
const lease = useLease(suite, {
  aic: {
    id: "resolve-identity",
    realm: "alpha",
    unsupported: "fail",
  },
});
```

`id` is a stable, repository-chosen lease identity. The adapter opens one
deterministically named journey before the file, replaces and confirms its
subject script for each run, invokes it, and deletes the graph after the file.
`realm` defaults to `alpha` and seeds the same realm into the local lane;
`tenant` selects an AIC context, and `project` selects the CLI project and
binary location. Both are optional and otherwise use the current defaults.
The returned `RunResult.conformance` contains the per-pass AIC verdict,
disagreements, and observation gaps. A file may have only one AIC-enabled
`useLease`; split different scripts or outcome vocabularies into separate test
files.

Unsupported input fails by default. `unsupported: "skip"` is the explicit
opt-out for a suite that must retain skip behaviour. Step and final `check()`
hooks replay against a tenant-backed `IdmHandle` at their matching response
boundaries. Suite `cleanup()` runs afterwards in a `finally`, before fixture
deletion, even if a check throws. The remote handle returns full materialized
tenant records rather than projecting them to the local mock's shape, so assert
the fields a check needs instead of whole-record equality. An AIC-only throw is
reported as a lane disagreement naming the remote hook.

Object-form queries reject values that cannot be represented faithfully as the
measured CREST `eq` filter (quotes, nulls, arrays/objects, control characters,
and non-finite numbers) before making a request. Unknown fields are not
preflighted because AIC reports them as a successful zero-row query.

Every AM write is followed by a confirming read. A process lock excludes the
same lease on one host, and an identifier-only journal makes stale owned
resources discoverable after process death. Simultaneous use of the same
`aic.id` on different hosts is unsupported because no atomic AM create
precondition has been established. Generated subject and session-minter source
can contain test seeds while a lease is open; never put credentials or real
secrets in them.

For capacity planning only, the design estimate uses `O` distinct subject
outcomes after adding `true` and `false`, and `R = 2O + 3` graph resources. For
`N` one-pass cases without managed fixtures or sessions, the base call count is
`3 + 4R + 3N`: two CLI session calls, one `serverinfo` read to discover the
session cookie's name, three calls per resource at open, one delete per
resource at close, and three calls per case. At the smallest `R = 9`, that is
an estimated `39 + 3N`, or 69 calls for ten cases. (The `38 + 3N` measured on
2026-09-14 predates the cookie-name discovery.)
Each `IdmHandle` read, encodable query, or delete in checks/cleanup adds one REST
call. Each additional outcome adds an estimated eight file-lifetime calls.
Step chains, managed fixtures, session minting, and credential refresh add
calls too.

`runAicChain()` remains the one-shot compatibility facade for external callers.
It provisions and deletes a throwaway graph per call. `useLease()` uses the
pre-opened file runner and does not route through that facade. The one-shot
facade refuses `given.libraries` before contacting the tenant; use `useLease()`
for library-backed suites.

`npm run generate` reads the captured JSON (offline; it does not call the
tenant) and overwrites the two artefacts. Completeness tests fail if the
artefacts drift from the JSON, if a method is dropped, or if you forget to
regenerate after changing the emitter.

## The `.cjs` is AM-safe JavaScript

The generated mock is **not** Node. AM's Rhino 1.7.14 rejects or silently
miscompiles a large slice of ES2015; the rules are runtime-verified in
`docs/api/12-script-bindings-matrix.md` and encoded in
`src/scripts/templates/am/eslint.config.js`. The emitter stays inside that
subset:

- `var` at top level; **no `let`**; **no top-level `const`** (parses, then
  reads back `undefined` in a scripted-decision script)
- **no `for...of`**, object shorthand, destructuring, or default parameters
- **no `Map` / `Set` / `Symbol` / `Promise` / `WeakMap` / `WeakSet`**
- `function () { … arguments … }` for methods (no rest parameters)
- `const` _inside a function_ is allowed; this file just does not need it

`npm run lint:am` runs those Rhino rules over `generated/*.cjs`. The generator
itself is ordinary TypeScript and has no such limits.

## What the JSON does not name

Empty-`elements` bindings (`requestHeaders`, `requestParameters`,
`requestCookies`, `locales`) are emitted as `{}` — a seam a test case seeds
directly. Scalar bindings (`realm`, `scriptName`, `cookieName`) start as
`"__rhino-local-unseeded__"`; `resumedFromSuspend` starts as `false`.

### Experimental journey emulator core (J1/J2)

`src/journey/index.ts` exposes `JourneyConfig`, `JourneyEmulator` and
`journeyHttp` inside the checkout. They are not package exports yet: the
framework-free core now includes Page, collectors and identity nodes; harness
selection follows in later slices. Use the mandatory Vitest setup/runner/reporter
described above when testing it. Its requests, runner evaluations and shared IDM
calls use the same append-only emulation audit and verdict guard as IDM.

```ts
const config = new JourneyConfig({
  realm: "alpha",
  trees: { Example: treeDocument },
  nodes: { ScriptedDecisionNode: { [nodeId]: nodeDocument } },
  scripts: { [scriptId]: scriptDocument }, // script is REST base64, evaluatorVersion "2.0"
});
const journey = new JourneyEmulator(config, runner, idm, {
  declaredOutcomes: { [unimplementedNodeId]: "true" },
  // wire.bodyText is the body as AIC would send it: `1` goes out as `1.0`.
  http: async (url, options, wire) => mockResponse(url, options, wire.bodyText),
});
const result = await journey.request({
  method: "POST", path: "authenticate",
  query: { authIndexType: "service", authIndexValue: "Example" },
  body: {},
});
```

The caller supplies one IDM emulator per test, shared with the journey, so
script `openidm` calls execute managed hooks in that instance. `fork()` shares
frozen config and the runner, forks IDM records, copies authentication snapshots,
and shares failure history. The caller closes the runner. `journeyHttp(journey)`
implements `AicIo.http` for the existing `invokeJourney`/`driveJourney` transport;
no parallel callback driver or verdict implementation is introduced.

The REST result uses `{status, body, rawBody?, provenance, emulation?}` just like
IDM. Callbacks use the tenant `{authId, callbacks}` output/input envelopes;
`aic/callbacks.ts` fills their inputs unchanged. Queued callbacks beat `goTo`;
the node runs again on resume. Shared state survives, transient survives
callback-free edges and is dropped at a round trip. Inner trees share shared
state, isolate their transient state and restore outer transient state. Inner
exceptions, callbacks and nested inner trees remain missing. Legacy scripts,
unsupported bindings, session-property actions, `noSession`, unsupported tree
flags and unmocked HTTP are fatal missing behavior, even if script catches or
`finally` tries to hide the failure. So is every `rhino-local:` diagnostic the
binding runtime raises for behaviour it does not model (an argument count whose
range is a guess, `new JsonValue(...)`, and so on): in a journey the runtime's
diagnostic constructor, `__rhinoLocalFail`, is a Java function that fails the
run, so the decision never depends on an error's text. Errors the runtime
emulates from AIC (`keySet()` on a request map, a `consentMappingCallback`
field refusal, an argument count AIC itself refuses) stay catchable in a
journey, as they are on the tenant. `utils` members measured in `docs/api/12` run locally and record
that provenance; any other `utils` member is refused.

Node effects are serialized in Java from the values themselves, never through a
script-reachable `JSON.stringify` or `toJSON`. Shared and transient state, queued
callback arguments and `openidm`/`httpClient` arguments must be plain JSON: a
custom or null prototype, an accessor, a non-enumerable property, an array
expando or hole, a cycle, `undefined`, a non-finite number, a function or a host
object is missing, never rewritten or thrown as a catchable error. The measured
exceptions (`docs/api/12`, 2026-11-04): `undefined` anywhere in an
`httpClient.send` body, including `body: undefined` itself, goes out as `null`;
and a top-level `undefined` field of `openidm.create` content is sent as `null`,
so a schema `default` does not land and a `NOT_NULL` policy refuses it.
Nested `undefined` in `openidm` content, and `undefined` in any
other `openidm` call, stay missing. The HTTP mock receives the body as parsed
JSON and, as its third argument, `{ bodyText }`: the text AIC would send, where
every JS number is a Java double (`1` is `1.0`), a boxed `java.lang.Integer` or
`Long` is written as is, and no `body` key is `""`. Strings and keys are
escaped as measured: in printable ASCII only `"` and `\` (no `\/`), other BMP
characters raw (U+2028 included), `\n` and `\t` short, other C0 controls and
every astral character or lone high surrogate as upper-case `\uXXXX`. That is
measured for an object body sent with `Content-Type: application/json`. Each
other shape is rendered the same way and labelled assumed with its reason: no
such header, a string or top-level non-object body, `\r`/`\b`/`\f`, a C1
control, a lone low surrogate, a
fraction or an integer past 1e6, or a number the script engine holds as a Java
Integer (`s.length`, `indexOf`, `JSON.parse` results). A `form` request is
missing, because its body is not the JSON text. `objectAttributes` that a node
wrote, or that crossed a callback round trip, reaches the next script as a Java
map (`get`/`put` work); its printed form is measured, the methods are inferred
and labelled assumed.

The guarantee is against accidental false passes from realistic scripts, not a
sandbox. AM's global scope cannot be made read-only, so a script that
deliberately rebinds the runtime's own globals — `__rhinoLocalFail`,
`__rhinoLocal`, built-ins such as `Error.prototype` or `Object.prototype` — can
still change what the local runtime does. The cheap paths are closed: refusals
and diagnostics are classified in Java by which function ran, and the harvest
reads the runtime object handed to Java before author code, not a global.

AuthIds use signed immutable local snapshot references (assumed format).
Suspended snapshots may be replayed; tampering returns the measured 400. A fork
accepts snapshots present when it forked; a parent's later snapshots are not
present there. Replay after a previous submission minted a session is explicitly
missing: doc 25 lists that case as unmeasured. Session tokenIds are local UUIDs;
principal existence and session properties are not modeled. Success is currently
measured only for realm alpha. `existingSession` can be supplied as complete flat
string-map fixtures under `sessions: { [cookieValue]: properties }` with an
explicit `cookieName`; unknown cookies fail missing instead of inventing session
properties. Newly issued tokenIds do not synthesize such a session fixture.

Declared outcomes are assumptions, not implementations of the named node. They
carry `assumed: declared by test: <tree>/<node> -> <outcome>`. Known type
vocabularies come from doc 25, including Page's last stored child's outcomes.
Undeclared types are missing. Basic Name/Password/TextInput/TextOutput/HiddenValue
callback wire shapes reuse the AM binding runtime; other wire types remain
missing until implemented. Input defaults, local Java collection facsimiles,
initial ambient state values, HTTP response fixtures and AM host-error adapters
retain explicit assumptions. The request's provenance distinguishes these from
the measurement agent's doc 25 rows; those rows were not measured by this agent.

`timeoutMs` bounds each script job (default 10 seconds); `maxTransitions` bounds
a callback-free request (default 1000 transitions). Exhaustion is an audited
emulation failure. Malformed JSON and callback shapes, callback metadata that
no longer matches a signed snapshot, and non-finite/negative-zero host values
remain missing rather than becoming plausible answers. The ambient duration
key's initial value/bucket is explicitly assumed; ordinary caller-written shared
state is preserved even when its key has an ambient name.

Page executes children in config order, merges their callbacks inline, and routes
by the last child's outcome. Each scripted child receives only its own answers.
A non-last child with multiple known outcomes is rejected by `JourneyConfig`
with a `RangeError` carrying the measured `400 Invalid config: Only the last
node in a page can have more than one outcome` diagnostic. Construction is the
local config-loading boundary; this core does not expose node PUT. Declaring an
outcome for any emulated node (including a Page child) throws `TypeError` when
constructing `JourneyEmulator`, so declarations cannot bypass its implementation.

Username/Password collectors write shared username/transient password.
AttributeCollector (`validateInputs: false`) uses string-property titles from
the shared IDM schema, emits the two-input attribute callback, and writes shared
objectAttributes. Empty required fields re-send the Page with empty
failedPolicies: attribute callbacks echo their submitted value (measured), while
an accepted sibling's other callbacks go out as first sent — a PasswordCallback
never carries the password (measured), other types are labelled assumed. A
scripted child after a sibling that rejected the submission is missing before it
runs, since whether AM runs it is unmeasured. `ValidatedPasswordNode` (`validateInput: false`) writes
transient password and transient objectAttributes. `nodeState.get` returns the
whole transient value when present, otherwise shared; objectAttributes maps are
never merged across these buckets. Extending that shadowing to other caller keys
is labelled assumed. The shared `pageNodeCallbacks` value is assumed to be the
merged wire array; doc 25 measures its presence/lifetime, not its contents.
Page discovery evaluates every script child once for real and retains its state
effects, ignoring its goTo (measured); submission evaluates each child again.
`withErrorMessage` in a discovery run, or alongside queued callbacks, is missing:
AM discards that Action, and whether the message survives is unmeasured.

IdentifyExistingUser queries the same IDM emulator and writes shared _id,
username and the identifier in shared objectAttributes. When
`objectAttributes[identityAttribute]` is present it is the only lookup value:
a hit overwrites `username`, a miss is `false` with `username` untouched and no
fallback; `username` is used only when that key is absent or `null`, and an
empty string, as a blank optional AttributeCollector answer writes it, is
looked up and fails (measured 2026-11-04). The IDM emulator's `eq` ignores
letter case and leading/trailing spaces on `alpha_user` strings, and
`gt`/`ge`/`lt` on `userName` ignore case (measured on the probed properties;
the rest of `alpha_user`, `le` and ordering on other properties are assumed),
so Identify finds a mixed-case or space-padded input and `username` takes the
stored case. A query whose answer for a record turns on an unmeasured case or
space rule (another managed type, `sw`, `co`, non-ASCII folding, repeated
interior spaces), on Unicode normalisation, on an unmeasured type mismatch (a
quoted integer is decided only on `alpha_user`, and a quoted literal against a
number only on a type `repo` declares generic), on a decimal literal against a
number, on `pr` of an explicitly empty string, array or object, on a pointer
through an array, or on `telephoneNumber`, a date or an integer attribute beyond
what the 2026-11-06 probe measured on `alpha_user` (`docs/rhino-local-gaps.md`) is the
`openidm-query-filter` fault, since either answer could be a false pass; one
whose answer does not turn on it, in any operand order, is not. CreateObject uses that
emulator's create path (including managed hooks and the caller-supplied IDM
policy evaluator) and writes no journey state. Missing-required policy rejection
routes to FAILURE; the broader mapping of IDM 400/403 to FAILURE is assumed.
PatchObject uses the identity attribute or username, ignores _id for lookup,
and aborts with 401 when an identifier names nobody. Identify/Create do not add
an extra principal-existence check at Success: doc 25 does not establish one.
Create alone leaves username absent and therefore cannot produce a session.

Attribute policy evaluation/validateOnly, validated-password validation,
split objectAttributes passed to Create/Patch/Identify, conflicting identity
sources for Patch, a null identity source, `httpClient.send` with no URL, Identify with no identity value at all, Patch with neither an
identity value nor objectAttributes, a non-object shared objectAttributes under
AttributeCollector or Identify, `ValidatedPasswordNode` over an existing
transient objectAttributes, a `ValidatedUsernameNode` Page child, callbacks
posted without an authId to an entry node other than a scripted decision, a
non-array `outcomes` configuration, Patch patchAsObject/ignoredFields, Page
headers/descriptions and nested Pages remain fatal missing behavior when
reached. Policy evaluation is not copied
into the journey layer: managed writes use the existing IDM policy seam.
