# 14 — AM identity attributes (the `identity` / `idRepository` binding)

Implemented in: `src/scripts/templates/`

## Purpose
AM-side scripts (OIDC claims, SAML mappers, scripted decision nodes) read
managed-user profile data through an identity binding keyed by **AM attribute
names**, which differ from the IDM managed-object property names. This file is
the verified IDM-property → AM-attribute mapping that drives a typed `identity`
in the script workspace (Phase 3 of `docs/schema-driven-types-plan.md`).

## Authentication
N/A — this is script-runtime behavior, probed in-tenant via the
`scripts/rhino-script-tester/` harness, not a REST endpoint.

Verified against the sandbox 2026-06-13 with
`fixtures/identity-attr-mapping.script.js` + `fixtures/identity-resolve-diag.script.js`
(a next-gen scripted decision node, `evaluatorVersion: 2.0`).

Return-container shapes verified 2026-09-10 with
`fixtures/identity-getattribute-shape.script.js` (same next-gen node) and
`fixtures-atm/identity-getattribute-shape.script.js` (legacy access-token
modification, `evaluatorVersion: 1.0`, `client_credentials`). Both call every
candidate member rather than `typeof`-ing it.

Legacy scripted decision `idRepository` method presence verified 2026-07-06
with `fixtures-legacy/legacy-idrepository-methods.script.js`
(`evaluatorVersion: 1.0`).

## Runtime facts (verified)

- **Resolution key is the managed-object UUID, not `userName`.** In a scripted
  decision, `idRepository.getIdentity(<fr-idm-uuid>)` returns a working
  `ScriptedIdentity`; passing the `userName`/`uid` (or `amadmin`) returns a stub
  whose `amIdentity` is `null` and every `getAttributeValues(...)` throws
  `InternalError: … this.amIdentity is null`. Use the managed `_id` (uuid).
- **Attribute access is by AM attribute name.** `getAttributeValues(<amName>)`
  returns an **indexable Java collection**: `length` is a number, and `size()`,
  `get(0)`, `[0]`, `toArray()` and `contains()` all resolve (size 0 when unset
  or when the name is wrong). Values always come back as strings regardless of
  the IDM property's declared type — so a typed binding's value is
  `JavaArray<string>`; the win is validating/autocompleting the **name**, not
  narrowing the return.

  It is **not** a `java.util.Set`, which this file claimed until 2026-09-10 —
  a Set has no `length` and no index access. Nor is it the `String[]` the AM
  8.1.1 class file declares: `String(v)` is `[{}]`, the bracketed `toString` of
  a collection, where a Java array stringifies as `[Ljava.lang.String;@…`.
  `includes()` is the one array-ish member that does **not** exist here, so
  `getAttributeValues(n).includes(x)` type-checks against `JavaArray` and throws
  — use `contains()`.

- **The next-gen binding has no `getAttribute` at all** (verified 2026-09-10).
  `idRepository.getIdentity(uuid).getAttribute("fr-idm-custom-attrs")` throws
  `TypeError: Cannot find function getAttribute in object
  …ScriptedIdentityScriptWrapper`. The AM 8.1.1 `ScriptedIdentity` class does
  declare `getAttribute(String) -> java.util.Set` as public, so this is the
  **wrapper** withholding it, not the class lacking it — an on-prem class file
  is evidence about what AM could expose, never about what AIC does.
  `getAttribute` lives on the classic `AMIdentity` bindings instead (legacy OIDC
  claims, legacy access-token modification), where it returns a
  `java.util.HashSet` — see
  [12-script-bindings-matrix.md](12-script-bindings-matrix.md#amidentitygetattribute-returns-a-hashset-verified-2026-09-10).
- **Negative controls** `frGivenName` / `givenNameXYZ` returned size 0 (no
  throw) — wrong names are silently empty, so a 0 alone can't distinguish
  "unset" from "invalid". Positive hits on populated fields are the proof.
- **Rhino sandbox:** `Set.iterator()` is blocked
  (`java.util.ArrayList$Itr … prohibited`); use `.toArray()[0]` to read a value
  in a probe. This is **not** a quirk of identity attributes — it is a
  general per-class, per-context class shutter, and the same error appears on
  bindings with no identity anywhere near them. `getClass()` and, in some
  contexts, `JSON.stringify` are blocked by the same mechanism, which makes the
  two obvious "what is this object" probes the two that fail. See
  [The Java class shutter](./12-script-bindings-matrix.md#the-java-class-shutter-verified-2026-09-08) for the measured boundary, the two
  distinct failure shapes, and which iterators are actually allowed
  (`HashSet`'s is).
- **Legacy decision nodes also expose `idRepository.getIdentity`.** The legacy
  engine reports `getIdentity`, `getAttribute`, `setAttribute`, and
  `addAttribute` as functions. The direct methods remain legacy-specific; the
  `ScriptedIdentity` object returned by `getIdentity` is shared.

## IDM-property → AM-attribute mapping

Status column: **✓live** = positively confirmed on a populated test user
(`getAttributeValues` returned the value under this AM name); **doc** = from the
Ping [user identity properties reference][ref], not locally confirmed because no
sampled user had the field populated. Source `frodo`/Ping docs claims are
trusted only where marked ✓live (CLAUDE.md §2).

[ref]: https://docs.pingidentity.com/pingoneaic/identities/user-identity-properties-attributes-reference.html

| IDM property | AM attribute | Status |
|---|---|---|
| `userName` | `uid` | ✓live |
| `cn` | `cn` | ✓live |
| `givenName` | `givenName` | ✓live |
| `sn` | `sn` | ✓live |
| `mail` | `mail` | ✓live |
| `telephoneNumber` | `telephoneNumber` | ✓live |
| `accountStatus` | `inetUserStatus` | ✓live |
| `_id` | `fr-idm-uuid` | ✓live |
| (custom attrs bag) | `fr-idm-custom-attrs` | ✓live (object; `{}` on this tenant) |
| `displayName` | `displayName` | ✓live store (2026-10-01, one value → string) |
| `description` | `description` | doc |
| `password` | `userPassword` | doc |
| `postalAddress` | `street` | doc |
| `city` | `l` | doc |
| `stateProvince` | `st` | doc |
| `postalCode` | `postalCode` | doc |
| `country` | `co` | doc |
| `aliasList` | `iplanet-am-user-alias-list` | doc |
| `applications` | `fr-idm-managed-application-member` | doc |
| `ownerOfApp` | `fr-idm-managed-application-owner` | doc |
| `assignedDashboard` | `assignedDashboard` | doc |
| `assignments` | `fr-idm-managed-assignment-member` | doc |
| `consentedMappings` | `fr-idm-consentedMapping` | doc |
| `reports` | `manager` | **not exposed here** (see below) |
| `manager` | `fr-idm-managed-user-manager` | **not exposed here** (see below) |
| `passwordLastChangedTime` | `pwdChangedTime` | doc |
| `passwordExpirationTime` | `pwdExpirationTime` | doc |
| `groups` | `fr-idm-managed-user-groups` | doc |
| `roles` | `fr-idm-managed-user-roles` | doc |
| `kbaInfo` | `fr-idm-kbaInfo` | doc |
| `preferences` | `fr-idm-preferences` | doc |
| `profileImage` | `labeledURI` | doc |
| `adminOfOrg` | `fr-idm-managed-organization-admin` | doc |
| `ownerOfOrg` | `fr-idm-managed-organization-owner` | doc |
| `memberOfOrg` | `fr-idm-managed-organization-member` | doc |
| `memberOfOrgIDs` | `fr-idm-managed-user-memberoforgid` | doc |
| `taskPrincipals` | `fr-idm-managed-user-task-principals` | doc |
| `_notifications` | `fr-idm-managed-user-notifications` | doc |
| `_rev` | `etag` | doc |
| `_meta` | `fr-idm-managed-user-meta` | doc |

General-purpose extension attributes from the Ping reference. "✓live store"
means `identity.store()` was measured writing the representative named, with
the layout in [What `store()` writes](#what-store-writes-measured-2026-10-01);
the other members of the family are assumed to behave like it, not measured.

| IDM property | AM attribute | Status |
|---|---|---|
| `frIndexedString1` … `frIndexedString20` | `fr-attr-istr1` … `fr-attr-istr20` | ✓live store (measured: istr1; istr2..20 assumed by family) |
| `frUnindexedString1` … `frUnindexedString5` | `fr-attr-str1` … `fr-attr-str5` | ✓live store (measured: str1, str2; str3..5 assumed by family) |
| `frIndexedMultivalued1` … `frIndexedMultivalued5` | `fr-attr-imulti1` … `fr-attr-imulti5` | ✓live store (measured: imulti1; imulti2..5 assumed by family) |
| `frUnindexedMultivalued1` … `frUnindexedMultivalued5` | `fr-attr-multi1` … `fr-attr-multi5` | ✓live store (measured: multi1, multi2; multi3..5 assumed by family) |
| `frIndexedDate1` … `frIndexedDate5` | `fr-attr-idate1` … `fr-attr-idate5` | ✓live store (measured: idate1; idate2..5 assumed by family) |
| `frUnindexedDate1` … `frUnindexedDate5` | `fr-attr-date1` … `fr-attr-date5` | ✓live store (measured: date1; date2..5 assumed by family) |
| `frIndexedInteger1` … `frIndexedInteger5` | `fr-attr-iint1` … `fr-attr-iint5` | ✓live store (measured: iint1; iint2..5 assumed by family) |
| `frUnindexedInteger1` … `frUnindexedInteger5` | `fr-attr-int1` … `fr-attr-int5` | ✓live store (measured: int1, int2; int3..5 assumed by family) |

Multivalue 2FA profile attributes from the Ping reference:

| IDM property | AM attribute | Status |
|---|---|---|
| `deviceProfiles` | `deviceProfiles` | doc |
| `devicePrintProfiles` | `devicePrintProfiles` | doc |
| `webauthnDeviceProfiles` | `webauthnDeviceProfiles` | doc |
| `oathDeviceProfiles` | `oathDeviceProfiles` | doc |
| `pushDeviceProfiles` | `pushDeviceProfiles` | doc |

### `fr-idm-custom-attrs`
An AM attribute holding the custom-property bag as persisted JSON text.
Custom fields are nested inside it, not exposed as separate AM attributes.
The earlier no-custom-properties measurement returned one element `{}` (the
Phase-1 schema was all OOTB). A stored bag can contain any valid JSON, and a
`[]` clear leaves no element. Per-field typing can join object bags with the
managed schema's custom properties.

**The getter does not return a string.** It returns the same container every
other attribute does. Check its size before reading an element; when present,
the element is JSON text, so use `JSON.parse(String(v.toArray()[0]))`, never
`JSON.parse(v)`. Measured
2026-09-10 both ways: through the next-gen `getAttributeValues` (count 1,
`String(v)` is `[{}]`, `charAt`/`substring` throw) and through the classic
`AMIdentity.getAttribute` (a `HashSet`, count 0 on an agent identity that has
no such attribute). The attribute's JSON content does not change the
container — the shape is a property of the method, and the populated control
(`com.forgerock.openam.oauth2provider.clientType`, count 1) reports the same
members as the empty case.

`fixtures/identity-attr-mapping.script.js` could not have caught a string: its
`sizeOf` helper reads `.size()` **or** `.length` **or** `.toArray().length`, so
it agrees with itself on a Set, a List and a bare string alike, and its
key-extraction has a bracket-stripping fallback that parses either. The counts
it reported are evidence about which NAMES are populated and not about the
container. `fixtures/identity-getattribute-shape.script.js` is the
discriminating half: it calls each candidate member and reports which throw.

**Populated bag and whole-bag writes — measured 2026-10-02 by the maintainer**
with `scripts/rhino-script-tester/fixtures/identity-custom-attrs.script.js` at
`42a34b8`, next-gen journey, `getIdentity(<fr-idm-uuid>)` on a user seeded with
temporary `custom_rlProbe` (string) and `custom_rlProbeFlag` (boolean):

- The read was size 1, one compact JSON element
  `{"custom_rlProbe":"seeded","custom_rlProbeFlag":true}`. Booleans remain JSON
  booleans. Key order varied across reads; compare parsed JSON, never its text.
  Reading the IDM name `custom_rlProbe` returned size 0.
- Before `store()`, the same wrapper read the persisted bag, ignoring staged
  values. A successful store became visible on the same and fresh wrappers.
- One JSON object **replaces the entire custom bag**. Writing only
  `custom_rlProbe` deleted `custom_rlProbeFlag`; writing both kept exactly both.
  No coercion: `{"custom_rlProbeFlag":"false"}` stored the string `"false"`,
  deleting `custom_rlProbe` despite the boolean schema property.
- Unknown `custom_rlNope` persisted and appeared on a fresh bag read. A later
  IDM patch of the declared properties merged them alongside that unknown key.
- `["{}"]` removed every custom property and read back size 1 with `{}`.
  `[]` also removed every custom property but read back **size 0**.
- Two valid JSON elements threw the existing `JavaException: …IdentityUpdateException`
  with `ldap errorcode=65`; a single `"not json"` threw the same shape with
  `errorcode=21`. Neither failure changed anything. Mixed malformed/valid text
  was measured separately below.

**Bag edges — measured 2026-10-02 by the maintainer** with
`scripts/rhino-script-tester/fixtures/identity-custom-attrs-edges.script.js` at
`8f4ff66`, next-gen journey, a throwaway managed user with no custom schema
properties:

- `["not json", "{}"]` and `["{}", "not json"]` both failed with errorcode 21,
  leaving the bag unchanged. Malformed text wins in either position; 65 applies
  to multiple elements only when every element is valid JSON.
- Every single-element write below succeeded, and a fresh AM wrapper returned
  exactly the text written. The bag is AM's source of truth; object-bag keys
  appear as IDM record properties, including unprefixed keys.

| Bag element text | Fresh AM read-back | IDM observation |
| --- | --- | --- |
| `"x"` | `"x"` | full read throws “Response is not application/json”; `_id`-only filtered query still returns the row |
| `1` | `1` | same full-read failure; `_id` query works |
| `[]` | `[]` | same full-read failure; `_id` query works |
| `null` | `null` | full read works; no `custom_*` keys or `plain` key |
| `{"plain":"x"}` | `{"plain":"x"}` | full read exposes top-level `plain: "x"` |
| `{"custom_rlNested":{"a":1}}` | as written | full read exposes the nested `custom_rlNested` property |

The full-read error is
`JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Response is not application/json`.
Persisting string, number or array JSON in this attribute can break subsequent
IDM reads, even though the AM write and read-back succeed.

After a `[]` clear (AM bag size 0), an IDM patch of the ordinary `givenName`
property restored `["{}"]` (size 1). Updating the whole record read back from
IDM did the same. Create and delete/recreate were not measured.

**Boolean read and string delete — measured 2026-10-02 by the maintainer**
with `scripts/rhino-script-tester/fixtures/identity-custom-attrs-nonobject.script.js`
at `f7d684b`, a throwaway managed user with no custom schema properties:

- `["true"]` persisted, and AM read back the text `true`. A full `openidm.read`
  threw the same `ResourceExceptionScriptAdapter: Response is not application/json`.
  Boolean bags therefore join strings, numbers and arrays in the full-read
  failure category; JSON `null` still permits a full read.
- `openidm.delete` of a user with the string bag `["\"x\""]` threw that same
  adapter error. An `_id`-only filtered query confirmed the user still existed.
  An external REST DELETE returned HTTP 500. Writing `["{}"]` through AM
  restored deletability; the subsequent REST DELETE returned HTTP 200.
- Delete with number, array, boolean or null bags was **not measured**. Neither
  was a boolean-bag query. Full-read failure alone establishes no delete or
  query outcome for those categories.

The local script binding uses the following measured categories and refusals:

| Persisted JSON category | Full `openidm.read` | `_queryFilter` with `["_id"]` | `openidm.delete` |
| --- | --- | --- | --- |
| object, or zero-element bag | existing object behavior | existing object behavior | existing object behavior |
| string | measured adapter error | row returned | measured adapter error; record retained |
| number, array | measured adapter error | row returned | unmeasured refusal |
| boolean | measured adapter error | unmeasured refusal | unmeasured refusal |
| null | full read works | row returned | unmeasured refusal |

Projected reads, other query shapes, patch/update and create against an existing
non-object-bag resource refuse as unmeasured before changing that record.
A local refusal names the JSON category and directs the script to restore an
object bag through AM.

The local harness derives an initial bag from seeded IDM `custom_*` properties
only; ordinary seeded properties cannot be distinguished from bag keys. After
`store()`, optional resource-keyed `identityCustomAttrs` metadata preserves the
persisted AM values, while `identityCustomAttrsOwnedKeys` separately retains
historical ownership of object keys such as `plain`. Clearing a declared alias,
an object bag or the whole bag removes values without revoking ownership. A
later declared re-add restores the key in both record and bag; whole-bag
replacement removes owned keys as well as all `custom_*` keys. Local successful
create/delete discards ownership history along with values. This bookkeeping is
validated against the current managed store when carried to another pass: dense
unique ownership keys identify existing resources, and a currently present
owned property must project from the bag. Properties currently projected from a
proven bag bypass profile property/enum checks; ordinary properties remain
strict. Both metadata fields are optional; older values-only metadata infers
present keys but cannot reconstruct cleared historical ownership. These are
local consistency rules, not additional tenant measurements. No
`identityAttributes` declaration is needed for the default bag layout.

An explicit legacy `"fr-idm-custom-attrs"` seed is accepted only if its parsed
object equals the custom_* derivation (key order ignored), then removed from
the canonical seed. Conflicts fail with a remedy pointing at `custom_*`.
A declared layout overrides the measured default **and exempts its seeds from
this normalization**, just as other declared layouts override measurements.

**Local collision policy, not a tenant measurement:** before persisting, the
harness refuses bag keys beginning with `_` (including `_id` and `_rev`) and
known ordinary/OOTB identity fields such as `givenName`. It refuses a whole-bag
replacement and another AM attribute write targeting the same property in one
`store()`, in either pending order. A replacement touches the entire `custom_*`
set, including omitted keys; use separate stores for overlapping writes.
Distinct targets may share a store. Reserved metadata collisions are
intentionally not probed on a tenant.

Declared layouts retain precedence over inferred seed contents and bag-written
values. A declared layout targeting a `custom_*` property replaces just that
key in the persisted object bag while updating the record; other bag keys are
preserved. After an absent `[]` clear, a declared key write restores one object
element. A single-valued clear removes its key; a multi-valued clear retains
`[]` as the key's value. Declared writes to persisted unprefixed bag keys follow
the same rule. Unused declarations permit whole-bag clearing, and a declaration
of `fr-idm-custom-attrs` itself overrides the measured layout and seed contract.

The complete store is preflighted against the current record and bag before
any mutation or effect is recorded, including changes after staging. A key
replacement within a non-object bag (string, number, array, boolean or JSON
null) is refused as unmeasured; first restore an object bag in a separate store.
Changing the identity resource through a declared `_id` layout is also refused
as unmeasured. These are local consistency boundaries, not additional tenant
measurements.

Public harness `check(idm)`/cleanup handles receive the same bag metadata in
step and final checks. External materialized read/query behavior for non-object
bags is not established by the AM adapter measurements, so these handles refuse
as unmeasured, including JSON null. String-bag delete reports the measured REST
500 failure; the other non-object deletes refuse as unmeasured. Object-bag
checks retain their ordinary behavior. Final check store/metadata clones keep
housekeeping out of recorded effects.

Successful local create/delete clears old bag metadata, and fresh recreated
records derive their own bag. That recreation policy remains unmeasured;
retained wrappers after delete/recreate refuse and require a fresh wrapper.
Update refreshes wrappers from the current record. The measured failed
string-bag delete leaves both record and metadata unchanged.

### Relationship attributes are NOT exposed via scripted-decision `getAttributeValues` (verified)
Probed with two purpose-built users — A (`probe-rpt-a`) with `manager` → B
(`probe-mgr-b`), so B.reports = [A] (relationship confirmed in IDM). Yet on the
`idRepository.getIdentity(uuid)` `ScriptedIdentity`, **all** of `manager`,
`fr-idm-managed-user-manager`, `reports`, `fr-idm-managed-user-reports` returned
size 0 for both users. So IDM relationship-typed fields (`manager`, `reports`,
`roles`, `assignments`, `applications`, `groups`, `authzRoles`) do **not**
surface through this binding — they're IDM-managed relationships, not
materialised AM identity attributes here. The `reports`↔`manager` swap is
therefore moot for scripted decision (neither side returns data); it may differ
in the OIDC-claims `AMIdentity` context (not yet probed — needs an OIDC flow).
`ScriptedIdentity` also exposes only `getAttributeValues` (no
`getAttributes`/`getAttributeNames`/`asMap` enumerator).

Also observed: `dn` returns the user DN (size 1) though it isn't in the Ping
mapping. The address block (`l`/`st`/`co`/`postalCode`/`street`),
`displayName`, etc. were unset on the test user — names taken from the Ping
reference (status `doc`).

**Typing consequence:** the typed `identity` should advertise the scalar/profile
names (the ✓live set + the `doc` scalars) and `fr-idm-custom-attrs`; the
relationship names are kept in the union for completeness/other contexts but
will return empty in scripted decision.

## Typing implication (Phase 3)
The AM attribute-name set is **fixed/OOTB** (this table) plus the single
`fr-idm-custom-attrs` — it is NOT per-tenant-schema-driven (custom fields nest
inside `fr-idm-custom-attrs`). So a typed `identity` is a **static** workspace
template improvement, not a generated-per-tenant artifact: give
`getAttribute`/`getAttributeValues` an overload accepting the AM-name union
(returning `JavaArray<string>`) plus a `string` fallback for non-user
attributes. Applies to the contexts that expose the binding: OIDC claims
(`identity: AMIdentity`), oidc-claims-ng / SAML mappers (`identity: Identity`),
and scripted decision (`idRepository.getIdentity(uuid): Identity`).

## Identity writes (live AIC-lane probe, 2026-10-01)

On a throwaway managed user, `setAttribute("fr-attr-str1", ["new"])`
succeeded, but `getAttributeValues("fr-attr-str1")` returned the persisted
"old" value **before** `store()`. The local tester now keeps staged writes
invisible to that getter until `store()`. The live result also recorded
successful `addAttribute("fr-attr-multi1", "second")` and `store()` calls.
On a later callback pass, the getter returned "new" for the string attribute,
and the multivalue collection had size 2 and contained both "first" and
"second". The wrong IDM field name returned size 0. These values matched the
local model after the pre-store read was corrected.

The AIC result did not directly prove the managed-record shape after
`store()`; the external between-pass IDM assertion was removed during
diagnosis so it could not mask the script-visible results. Whether an
`addAttribute` change is visible before `store()` remains unmeasured.

Because that shape is unproven, the local script tester does not model
`store()` as an IDM write. It records an `identityWrites` effect per stored
attribute, named by the AM attribute the script used, and keeps it out of the
`openidm` channel and its failure stubs (AM persists through the identity
repository, not the `openidm` binding). It refuses an IDM field name passed to
`setAttribute`/`addAttribute`: a wrong-name *read* returning no values is
measured above, but what `store()` does with a wrong-name *write* is not.
`live-identity-writes.e2e.test.ts` retains per-call diagnostics, and its
fixture cleanup deletes the user.

**Measured 2026-10-01** (`live-identity-store-visibility.e2e.test.ts`, one
pass, then a callback round trip):

- After `store()`, the **same** wrapper and a **fresh** `getIdentity` both read
  the new value immediately.
- The IDM record (read with `openidm.read` in the same pass and the next)
  holds `setAttribute("fr-attr-multi1", ["only"])` as `["only"]` — an array even
  with one value — while `fr-attr-str1` and `mail` hold a plain string.
- A `setAttribute` with **no** `store()` on another wrapper never reaches IDM
  or a later `getIdentity`, even across the callback round trip.

The local lane stores by the layouts below and refuses a write to any other
attribute name unless the suite declares one (`identityAttributes`).

### What `store()` writes (measured 2026-10-01)

`live-identity-store-families.e2e.test.ts`, `live-identity-cn.e2e.test.ts` and
`live-identity-declared.e2e.test.ts`, alpha realm, next-gen scripted decision.
Each row was its own `store()` through a fresh `getIdentity` wrapper; the IDM
side was read with `openidm.read(path, null, [field])` in the same pass.

| AM attribute (representative) | 1 value in IDM | 2+ values | `[]` | Value the script passes |
|---|---|---|---|---|
| `fr-attr-str1`, `fr-attr-istr1` | string | **throws** errorcode 65 | property removed (reads `null`) | any string |
| `fr-attr-multi1`, `fr-attr-imulti1` | **array** (`["only"]`) | array | `[]` | any string |
| `fr-attr-int1`, `fr-attr-iint1` | **number** (`"42"` → `42`) | **throws** 65 | removed | decimal integer string; `"abc"`, `"4.5"` **throw** errorcode 21 |
| `fr-attr-date1`, `fr-attr-idate1` | ISO string (`"20261001120000Z"` → `"2026-10-01T12:00:00Z"`) | **throws** 65 | removed | LDAP GeneralizedTime `YYYYMMDDHHMMSSZ`; an ISO timestamp, a bare `2026-10-03` and free text **throw** 21 |
| `givenName`, `telephoneNumber`, `mail` | string | **array** | removed | any string |
| `sn` | string | array | **throws** 65 (required) | any string |
| `fr-idm-custom-attrs` (2026-10-02) | whole-bag replace; object keys exposed in IDM, including unprefixed keys | **throws** 65 if all JSON is valid, otherwise 21 | all bag-owned properties removed; AM bag size 0 | one valid JSON string; malformed text **throws** 21; `["{}"]` reads one `{}`; string/number/array/boolean JSON breaks full IDM reads; string bags also block delete (see above) |
| `displayName` | string | unmeasured | unmeasured | any string |
| `cn` | not in IDM at all | unmeasured | **throws** 65 (required) | any string |

- **The thrown error** is a `JavaException` whose text is
  `org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo encountered a ldap exception.  ldap errorcode=<n>`
  — 21 is LDAP invalidAttributeSyntax, 65 objectClassViolation. A script can
  catch it.
- **A refused store applies nothing.** One `store()` with a good
  `fr-attr-str3` and a bad `fr-attr-int2` threw 21 and left `fr-attr-str3`
  unset. After any refused row, the previous value was still in place in both
  IDM and `getAttributeValues`.
- **`getAttributeValues` returns what the script wrote**, as strings: an
  integer reads `"42"`, a date reads back as GeneralizedTime, not as the ISO
  string IDM holds.
- **Standard attributes are multi-valued in DS**, so the IDM type is not fixed:
  `givenName` is a string with one value and an array with two. A reader of the
  managed object has to accept both.
- **`cn` lives outside the managed record.** It starts as `"<givenName> <sn>"`
  (`"Identity Probe"` for a user created through IDM with those names),
  `store()` changes it and AM reads the new value back, but `openidm.read` of
  the full record has no `cn` key before or after.
- `getAttributeValues(n).toArray()` supports `Array.prototype` methods:
  `.toArray().map(String)` ran in every row.
- Not measured: several values on `displayName` or `cn`; `[]` on
  `displayName`; signed or zero-padded integers; GeneralizedTime with
  fractions, offsets or no seconds; what a second `store()` on a wrapper whose
  first `store()` threw sends.

The local lane follows this table. It throws the same `JavaException` for
the measured refusals, refuses the unmeasured cases, and refuses a `cn` write
— AM keeps it outside the IDM record, which is all the local lane holds.
`displayName` is not given a layout locally on one measurement; the declared
probe shows a suite declaring it agrees with the tenant.
