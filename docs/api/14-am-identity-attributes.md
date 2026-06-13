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
| `reports` | `manager` | **doc (swap — unverified)** |
| `manager` | `fr-idm-managed-user-manager` | **doc (swap — unverified)** |
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

### Still to verify (no sample data)
The **`reports` ↔ `manager` swap** is the one surprising row and remains
unconfirmed — no sampled `alpha_user` has a `manager`/`reports` relationship
set. Confirm by probing a user with a manager before relying on it. The address
block (`l`/`st`/`co`/`postalCode`/`street`), `displayName`, `roles`,
`applications`, etc. are unset on available test users — names taken from the
Ping reference.

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
