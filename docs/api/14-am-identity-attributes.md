# 14 — AM identity attributes (the `identity` / `idRepository` binding)

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

## Runtime facts (verified)

- **Resolution key is the managed-object UUID, not `userName`.** In a scripted
  decision, `idRepository.getIdentity(<fr-idm-uuid>)` returns a working
  `ScriptedIdentity`; passing the `userName`/`uid` (or `amadmin`) returns a stub
  whose `amIdentity` is `null` and every `getAttributeValues(...)` throws
  `InternalError: … this.amIdentity is null`. Use the managed `_id` (uuid).
- **Attribute access is by AM attribute name.** `getAttributeValues(<amName>)`
  returns a `java.util.Set<String>` (size 0 when unset or when the name is
  wrong). Values always come back as **string arrays**, regardless of the IDM
  property's declared type — so a typed binding's value is `JavaArray<string>`;
  the win is validating/autocompleting the **name**, not narrowing the return.
- **Negative controls** `frGivenName` / `givenNameXYZ` returned size 0 (no
  throw) — wrong names are silently empty, so a 0 alone can't distinguish
  "unset" from "invalid". Positive hits on populated fields are the proof.
- **Rhino sandbox:** `Set.iterator()` is blocked
  (`java.util.ArrayList$Itr … prohibited`); use `.toArray()[0]` to read a value
  in a probe.

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
| `displayName` | `displayName` | doc |
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
| `_rev` | `etag` | doc |
| `_meta` | `fr-idm-managed-user-meta` | doc |

### `fr-idm-custom-attrs`
A single object-valued AM attribute holding **all** custom (tenant-added)
managed-user properties — custom fields are nested inside it, not exposed as
separate AM attributes. The value is a JSON-object **string** (parse it). On
this sandbox it is `{}` (no custom user properties — matches the Phase-1
managed schema, all-OOTB fields). Per-field typing of this object is possible
where a tenant has custom props (join with the managed schema's non-OOTB
properties); here there are none.

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
