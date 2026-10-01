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
an empty collection (`null`, matching AIC), an undeclared one throws naming
the environment and the pull date. Without a profile the pre-existing
behaviour is unchanged: any unseeded collection is a missing fixture.

Only the SET of declared type names crosses into the sandbox. Every schema
rule — properties, `required`, `enum` — is checked in the Node layer, so each
rule has one implementation rather than an AM-safe second copy that could
drift from it.

### A deliberate divergence, in the safe direction

An undeclared type throwing is **stricter than AIC**, which returns `null`.
That is a false fail, not a false pass, and it is the point: the alternative
is a test that reads nothing and passes. `lib-openidm-miss-consumer` encodes
the live `null` and therefore cannot pass with a profile loaded. Retire or
re-scope that case rather than relaxing the check.

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
| `lib-openidm-miss-consumer`   | read of a nonexistent managed **type** → `null`                                              | throws `no given.managed entry` (deliberate — see above)           |
| `identity-resolve-diag`       | `getIdentity` resolves a UUID only; payload committed abbreviated                            | same resolution; emits the full `attr-error: <message>` text       |
| `identity-getattribute-shape` | Java collection: no `includes` member (`Cannot find function`), prints `[{}]`                | `toArray`/`contains`/`[{}]` match; shadowed `includes` (`Cannot call property`) |
| `identity-enum-attrs`         | **not measured** — every count came back `err`; the probe's IDM setup users are gone         | —                                                                  |

`.includes()` on `getAttributeValues` and `getClass()` on a request map were
once dangerous — they worked locally and failed live. Both now throw on both
lanes and differ only in the message. `identity-attr-mapping` left this table
when the local identity binding gained AM's `uid` mapping; it now matches.

Two request cases are also **not comparable** beyond their `getClass` row: the
live run was a curl with its own headers and parameters, not the case's
`given`, and its payload carries the tenant host and client-certificate
headers, so it is not committed.

`identity.store()` follows the layouts measured 2026-10-01
(`live-identity-store-families`, `docs/api/14` "What `store()` writes"): every
`fr-attr-*` family by one representative, plus `givenName`, `sn`, `mail` and
`telephoneNumber`, including AM's own `IdentityUpdateException` for the
refusals DS makes. What remains:

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
| `binding-utils-interop`     | 17/20        | AIC wraps as `JavaException ScriptCryptoException`; arity and `(name#line)` wording                                                 |
| `binding-services`          | 57/80        | tenant-backed JWT and policy results; absent-user `getIdentity` wrapper's Java members; secret `JavaException`s; tenant script id in `getName`     |
| `binding-openidm-writes`    | 18/36        | AIC throws `JavaException` wrappers carrying the LDAP DN; `validateObject` answers from tenant policy; unsupported query filters     |

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
