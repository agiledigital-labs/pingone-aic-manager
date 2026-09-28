# rhino-local gaps — real scripts vs the local overlay

What happens when the local harness is pointed at the scripted-decision scripts
this repo actually has, instead of the three toy cases.

Cases live under `packages/rhino-local/cases/real/`. Overlay:
`packages/rhino-local/src/bindings/rhino/runtime.cjs`. Measured 2026-09-12
against the JVM runner (`VERSION_DEFAULT` + `ScriptContextScope`) after
`callbacks.isEmpty`, next-gen `require()`, and legacy `Action.send`.

## What runs today

Re-measured 2026-09-28 against the sandbox (`run-probes.sh`) and the host-JVM
runner. 47 cases, and every one is green — by asserting what is true, not by
matching the overlay:

| Count | Status                                                                                             |
| ----- | -------------------------------------------------------------------------------------------------- |
| 27    | pass the live payload exactly                                                                      |
| 8     | **known gap** — live values committed, the differing keys pinned                                   |
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
| `identity-resolve-diag`       | `getIdentity` resolves a UUID only                                                           | also resolves a userName                                           |
| `identity-attr-mapping`       | `uid` is mapped (count 1)                                                                    | not mapped (count 0)                                               |
| `identity-getattribute-shape` | `getAttributeValues` is a Java collection: `toArray` yes, `includes` no, prints `[{}]`       | JS array: `includes` yes, `toArray` no                             |
| `identity-enum-attrs`         | **not measured** — every count came back `err`; the probe's IDM setup users are gone         | —                                                                  |

Two of these are dangerous rather than cosmetic: `.includes()` on
`getAttributeValues` and a `getClass()` on a request map both behave better
locally than on AIC, so a script relying on them passes here and fails live.

Two request cases are also **not comparable** beyond their `getClass` row: the
live run was a curl with its own headers and parameters, not the case's
`given`, and its payload carries the tenant host and client-certificate
headers, so it is not committed.

What the 2026-09-12 ranking called the shutter gap is closed:
`java-class-shutter`, `for-each-java-collection` and
`lib-java-collections-consumer` now equal the live payload exactly, as does
`logger-placeholders` (its `E0-shape` row was the only miss, and it was the
expectation that was incomplete). The five legacy cases were dropped on
2026-09-12 (`7bb0195`); legacy scripts are not supported.

## Where the real scripts are

| Source                                                                                | What it is                                                                | Verdict                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/rhino-script-tester/fixtures/*.script.js`                                    | Next-gen scripted-decision probes, run live through `AIC-Rhino-Let-Probe` | **This is the in-repo corpus.** Recorded outcomes live in `docs/api/12-script-bindings-matrix.md` and `docs/api/14-am-identity-attributes.md`. `tmp/rhino-script-tester/probe-results.json` is gitignored and was not in this checkout. |
| `scripts/rhino-script-tester/fixtures-legacy/*.script.js`                             | Legacy (`evaluatorVersion` 1.0) scripted-decision probes                  | Same kind, different engine. Converted with `given.engine = "legacy"`.                                                                                                                                                                  |
| `scripts/rhino-script-tester/scripts/*.script.js`                                     | Earlier `let` / `var` probes, same journey                                | Converted.                                                                                                                                                                                                                              |
| `scripts/rhino-script-tester/fixtures/*.lib.js`                                       | LIBRARY bodies, not decision-node scripts                                 | Not cases. Consumers `require()` them; library source is seeded next to the case (the case type has no `libraries` field).                                                                                                              |
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

47 cases in `packages/rhino-local/cases/real/index.ts`, each running its
origin fixture under `scripts/rhino-script-tester/`:

- 45 next-gen (43 `fixtures/*.script.js` + 2 `scripts/*.script.js`)
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
- A known gap is not coverage. Eleven of 47 differ from AIC on at least one key,
  and one of those (`identity-enum-attrs`) has no valid live measurement.
- `require()` is a CommonJS eval of seeded source, not AM's wrap factory. The
  library probes in this corpus do not need the shutter to load; they need it
  for the Java names they then construct (`lib-java-collections-consumer`).
