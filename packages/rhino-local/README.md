# rhino-local TypeScript package

Two jobs, one package:

1. Generate the **scripted-decision mock binding surface** from the captured AM
   contexts metadata at `docs/api/bindings/scripted-decision-next.json`.
2. Talk to the long-lived host-JVM runner so tests can eval scripts without
   starting a JVM per case.

The generator emits presence, not behaviour: every method throws
`rhino-local: not mocked: <binding>.<method> arity=N overload=[…]` until the
handwritten overlay in `src/bindings/rhino/runtime.cjs` replaces it. A mock
that returned `undefined` would turn a missing feature into a passing test.

Implemented (still fail-loud for missing fixtures and for every method the
overlay does not replace): `nodeState` get/putShared/putTransient/isDefined,
legacy `sharedState`/`transientState` when `given.engine === "legacy"`,
request maps, `outcome` / `action.goTo`, `logger`, `openidm`, `httpClient`,
`callbacksBuilder` (the six authenticate-response types), `systemEnv`,
`idRepository.getIdentity` (reads, and `store()` as `identityWrites`), next-gen `require()` (CommonJS eval of seeded
library bodies; a missing id throws naming `given.libraries`), legacy
`JavaImporter` + `Action.send(HiddenValueCallback)`. End-to-end cases live
in `cases/`.

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
changing the record. See the maintainer's 2026-10-02
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

Optional `identityCustomAttrs` runner bookkeeping accompanies `managedStore`
across callback passes. It maps full managed resource paths to persisted AM
string arrays, preserving exact JSON text, the `[]`/`["{}"]` distinction and
object-key ownership. Entries must identify existing records, use dense arrays
and agree with their IDM projection. Proven bag-owned properties pass profile
checking on the next run; ordinary properties remain strict. It is not a judged
channel; producers from 0.1.2 can omit it. Bag writes are judged only through
`identityWrites`. Full resource paths prevent collisions across collections.

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
(**breaking for 0.2.0**), instead of inventing `{}`. The exception is the measured
managed-collection actions `patch`, `triggerSyncCheck`, `updateLastSync`, which
still return `{}` by default; an explicit reply overrides that default. Other
managed-collection actions still throw the measured refusal, even with a reply.
Action replies are local only: like HTTP replies, they make cases AIC-ineligible
and report the same observation gap (`aic.unsupported: "skip"` opts into skipping).

Use `openidmFailures` to make one local IDM call fail: each stub has
`match: { method, resource, ordinal }` and `reply: { code }`. `ordinal` is
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
`"undefined"`), so a script cannot route on one there either. The message text
is the harness's own; AIC's carries the reason phrase, not the number. These
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
  `.aic-script-tester/profiles/<tenant>.json`. It prints only counts.
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

## JVM runner client

`src/runner.ts` launches Java 25 from `AIC_SCRIPT_TESTER_JAVA_HOME` or `JAVA_HOME`
(`shell.nix` provides `temurin-bin-25`) and speaks line-delimited JSON.
`AIC_SCRIPT_TESTER_JVM` selects `host` (default), `container`, or `both`; the latter
lanes require Docker and the AM image. The cache defaults to
`~/.cache/aic-script-tester`; `AIC_SCRIPT_TESTER_CACHE` overrides it. Correlate by job `id`
— do not assume the JVM answers in order. A crashed JVM rejects every pending
job rather than hanging. `close()` ends stdin and waits for the process;
`afterAll` should call it.

Job shape (open enough to carry later mock bindings):

- `source` / `sourceName` — author's script; `sourceName` is Rhino's source
  name, so a parse error names the author's file and line, not `<eval>`
- `globals` — JSON values placed in ENGINE_SCOPE Bindings
- `preamble` — evaluated first so concatenating mocks into `source` is not
  needed (that would shift line numbers)
- `timeoutMs` — per-job; AM's `Error("Interrupt.")` past the instruction
  observer. Omit for the runner's 10s harness default; `0` is AM's no-timeout

Outcomes are machine-readable and distinct: `ok`, `compile_error`,
`runtime_error`, `timeout`.

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
