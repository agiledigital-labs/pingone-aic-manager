# 06 — SAML 2.0

Implemented in: —

## Purpose

Manage SAML 2.0 hosted (this tenant is the IdP/SP) and remote (another party is
the IdP/SP) entity providers, plus the circles of trust that bind them. Feature
3 of pingone-aic-manager ("manage OIDC and SAML config") is partly built on this
API.

## Authentication

Service-account bearer. Scope: `fr:am:*`.

`Accept-API-Version` is **optional** on this family — omitting the header
entirely still returns 200, and `protocol=2.1,resource=1.0` is what the console
sends. There is no `resource=2.0`: asking for it returns `404 Resource '' not
found`, which reads like a bad path rather than a bad version. Send
`resource=1.0` (with or without the protocol part) and nothing else.

**The metadata-export JSP is not authenticated at all.** See
"Exporting metadata" below; it is the one endpoint in this file that takes no
bearer.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`). Send
`Accept-API-Version: protocol=2.1,resource=1.0` — optional here (see
"Authentication"), but it is what the console sends and costs nothing.

### Entity providers

| Op                   | Method   | Path                                                                   | Notes                                                                            |
| -------------------- | -------- | ---------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| List                 | `GET`    | `/am/json{realm-path}/realm-config/saml2?_queryFilter=true`            | Stubs only — see shape below. The **only** list endpoint; see the row below.      |
| List one location    | `GET`    | `/am/json{realm-path}/realm-config/saml2/{location}?_queryFilter=true` | **400 `Query not supported`.** Filter the parent collection on `location` instead. |
| Filter by entityId   | `GET`    | `/am/json{realm-path}/realm-config/saml2?_queryFilter=entityId+eq+"…"` | 200, same stub shape. Works.                                                      |
| Read full            | `GET`    | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | `location` ∈ `hosted` \| `remote`.                                                |
| Schema               | `POST`   | `/am/json{realm-path}/realm-config/saml2/{location}?_action=schema`    | Draft-07 JSON Schema of the whole entity, ~100 KB. See "The schema endpoint".     |
| Create hosted        | `POST`   | `/am/json{realm-path}/realm-config/saml2/hosted/?_action=create`       | 201. **Nothing is required** — see "Creating a hosted entity".                    |
| Create remote        | `POST`   | `/am/json{realm-path}/realm-config/saml2/remote/?_action=create`       | **400 `Create not supported`.** Remote entities arrive only via `importEntity`.   |
| Import remote        | `POST`   | `/am/json{realm-path}/realm-config/saml2/remote/?_action=importEntity` | 200. `{"standardMetadata":"<base64url>"}` — **url**-safe alphabet; see below. |
| Import hosted        | `POST`   | `/am/json{realm-path}/realm-config/saml2/hosted/?_action=importEntity` | **501 `importEntity not supported`.** Hosted entities are built from JSON only.   |
| Update               | `PUT`    | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | 200. **Full replace**, no `If-Match`. 404 on an unknown id — no create-by-`PUT`.  |
| Delete               | `DELETE` | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | 200, echoes the deleted document. **Cascades into every CoT that listed it.**     |
| Export metadata XML  | `GET`    | `/am/saml2/jsp/exportmetadata.jsp?entityid={entityId}&realm=/{realm}`  | 200 `text/xml`. **No authentication.** Errors are also 200 — see below.           |

An unsupported `_action` on these collections answers **`403 No privilege
mapping for requested action`**, not 404 or 501. It looks like the
service-account is missing a scope; it is not. A CLI must not report it as a
permissions problem.

`{entityId64}` is the entity ID **base64url-encoded without padding** — verified
2026-08-12 by re-deriving the `_id` of two live entities from their `entityId`
(exact match, both an `https://host` form and a form with a trailing `/`).

### Circles of Trust

| Op            | Method   | Path                                                                            | Notes                                                                    |
| ------------- | -------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| List          | `GET`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust?_queryFilter=true` | Returns full documents, not stubs.                                       |
| Read          | `GET`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | `{id}` is the plain CoT name, **not** base64.                            |
| Schema        | `POST`   | `/am/json{realm-path}/realm-config/federation/circlesoftrust?_action=schema`    | `status`, `description`, `trustedProviders`, `saml2{Reader,Writer}ServiceUrl`. No `required` list, and **no `_id`** — the id comes from the URL or the body. |
| Create        | `POST`   | `/am/json{realm-path}/realm-config/federation/circlesoftrust/?_action=create`   | 201. `_id` in the body names it; omit `_id` and AM mints a **UUID**.     |
| Create/Update | `PUT`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | 201 on a new id, 200 on an existing one. **Merges** — omitted keys survive. |
| Delete        | `DELETE` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | 200, echoes the deleted document.                                        |

The two families do **not** behave alike, and the differences are the ones a
shared abstraction would paper over: entity `PUT` replaces and 404s on an
unknown id; CoT `PUT` merges and creates. Don't write one helper for both.

## Circle-of-trust membership is stored in TWO places — and REST shows only one

This is the single most important thing in this file. Read it before writing any
CoT code, and before diagnosing any "trust"-flavoured federation failure.

AM records that entity E belongs to circle of trust C in **two independent
places**:

1. The **CoT document's `trustedProviders`** — `<entityId>|<protocol>` strings.
   This is what the REST API above returns.
2. The **`cotlist` attribute in each entity's extended metadata** (`SPSSOConfig`
   / `IDPSSOConfig`). **The REST API never exposes this** — it is absent from
   both the list stub and the full entity read.

**Runtime trust checks read direction 2, not direction 1.** `SAML2Utils`
resolves the hosted entity's `cotlist`, then looks up each named CoT _by name_
to test membership. It does **not** scan all CoTs for the entity. So a CoT
document that lists both providers proves nothing: if the entity's `cotlist` is
empty, every assertion from that peer is rejected and the REST API shows a
perfectly healthy configuration.

Verified 2026-08-12 by comparing a failing and a succeeding SP-initiated login
in the same tenant, realm and time window — see "Diagnosing a rejected
assertion" below for the log signature of each.

Consequences for this project:

- **Never present REST `trustedProviders` as "the" CoT membership** in a TUI or
  CLI. It is one of two sources and the one the runtime ignores. If we surface
  CoT membership at all, label it as the CoT document's view and say the
  entity-side `cotlist` is not visible over REST.
- **`PUT` on a CoT does drive the entity-side membership too — and it can fail
  half-way.** Corrected 2026-09-16; this file previously said the REST path was
  "suspect until verified" and to make membership changes in the console. Two
  live observations say the REST write goes through the same `COTManager`
  machinery the console uses:

  1. A `trustedProviders` entry that **names a real entity but an unresolvable
     protocol** (`https://idp-a.example.com|wsfed` against an entity with only
     SAML2 metadata) answers **`500 An error occurred while updating the COT
     memberships`** — in both directions, adding it and later removing it. An
     entry naming an entity that does not exist fails the same way on **add**
     and succeeds on **remove**. An entry with no `|protocol` suffix at all
     (`garbage-no-pipe`) is stored and removed with a cheerful 200, because
     nothing can be resolved from it to update.
  2. **Deleting an entity removes it from every CoT document that listed it.**
     A throwaway CoT holding `https://idp-c.example.com|saml2` came back with
     `trustedProviders: []` immediately after that entity was `DELETE`d, with
     no CoT write of our own in between.

  Neither observation reads a `cotlist` directly — REST still never exposes it
  — so this is strong circumstantial evidence, not a proof. The falsifying case
  would have been a 200 for the `|wsfed` entry: a write that only touched the
  CoT document has nothing to resolve and no reason to fail. **The remaining
  open question is narrower than it was**: not "does REST sync at all" but "does
  a real assertion verify after a REST-only membership change".

- **A `500` from a CoT `PUT` still writes the CoT document.** Every 500 above
  left `trustedProviders` exactly as submitted. So the failure mode is precisely
  the split this section warns about — document updated, entity side not — and
  a CLI must **re-read and re-check**, never treat the 500 as "nothing
  happened".
- **Re-importing an entity's metadata is a hazard**, for the same reason:
  `importEntity` rewrites extended metadata and can drop a `cotlist` that was
  added afterwards, breaking a federation that was working, with no visible
  change to the CoT document.

## Creating a hosted entity

`POST …/realm-config/saml2/hosted/?_action=create`, `Content-Type:
application/json`. There is effectively **no minimum viable body**: the probe
that produced this section sent `{}` and got **201**, with AM minting a random
UUID as the `entityId`:

```json
{
  "_id": "YTg4MzAxNDQtZTY1MC00OTJjLThhYTEtYjFiOWE3YWQ3YWMx",
  "_rev": "-1282901691",
  "entityId": "a8830144-e650-492c-8aa1-b1b9a7ad7ac1"
}
```

The schema lists `entityId` as the only `required` property and AM does not
enforce even that. **A CLI must supply `entityId` itself and refuse an empty
one** — otherwise a mistyped command leaves a UUID-named entity in the realm
that nothing will ever reference.

The 201 response is a **stub** (`_id`, `_rev`, `entityId`), not the created
document — do not snapshot it. `GET` the entity afterwards.

### The one 4xx you will actually hit is a 500

Field-level validation is thin and the failures are opaque. Measured:

| Body                                                        | Result                                                            |
| ----------------------------------------------------------- | ----------------------------------------------------------------- |
| `{}`                                                        | **201**, UUID `entityId`, no roles                                |
| `{"entityId":"https://sp.example.com"}`                     | **201**, roleless entity                                          |
| `{"entityId":…,"serviceProvider":{}}`                       | **500 `Exception from invocation expected to be handled by promise`**, nothing created |
| `{"entityId":…,"serviceProvider":{"services":{"metaAlias":"/bravo/x"}}}` | **201**, full SP role                                 |

So **`services.metaAlias` is the field that makes a role block legal** — the
last two rows differ only in that key — and omitting it is a 500 with a message
that names no field. (That pair shows `metaAlias` is *sufficient*; no probe
tried to find a second required field, so treat "the only one" as unproven.) A CLI has to validate
`metaAlias` client-side; there is nothing in the response to translate. The
alias must start with `/<realm>/` — every endpoint AM publishes for the entity
embeds it (`/am/AuthConsumer/metaAlias/bravo/x`, `/am/SPSloRedirect/…`).

A roleless entity is legal and inert: the list stub omits `roles` entirely (not
`[]`), and its exported metadata is a bare self-closing `<EntityDescriptor/>`.

### Roles are derived, not declared

`roles` is computed from which role blocks are present. Sending both
`identityProvider` and `serviceProvider` (each with its own `metaAlias`) gives
a stub with `"roles": ["identityProvider","serviceProvider"]` and a metadata
export carrying both an `IDPSSODescriptor` and an `SPSSODescriptor`. There is no
`roles` field to write.

### `PUT` is a full replace — this is the sharp edge

`PUT …/saml2/{location}/{entityId64}` with `{"entityId": "<same id>"}` returns
**200** and silently deletes the entire `serviceProvider` block, `metaAlias` and
all. The entity survives as a roleless shell and its metadata export collapses
to `<EntityDescriptor/>`.

- No `If-Match` support: a stale `If-Match` header and a bogus `_rev` in the
  body were both accepted with 200. Optimistic concurrency does not exist here.
  Use a content snapshot (`.ai/core.md` §5).
- `_id` and `_rev` may be present in the body and are ignored.
- A repeat `PUT` of identical content returns 200 with the **same** `_rev`, so
  `_rev` is a content hash, not a counter.
- `PUT` to an id that does not exist is **404 `Cannot find SAML2 entity`** —
  there is no create-by-`PUT` for entities (unlike CoTs, which do create).

**So `aic saml push` must read-modify-write the whole document.** Anything that
sends a partial body is a destructive operation wearing an update's clothes.

## Importing a remote entity

This is the shape the whole `aic saml import` command hangs off, so it is worth
stating exactly.

```
POST /am/json{realm-path}/realm-config/saml2/remote/?_action=importEntity
Content-Type: application/json

{"standardMetadata": "PD94bWwgdmVyc2lvbj0iMS4wIiBlbmNvZGluZz0iVVRGLTgiPz4KPEVudGl0..."}
```

- **JSON envelope, not multipart, not a raw XML body.** One field,
  `standardMetadata`.
- **The value is base64url — the URL-safe alphabet is mandatory.** This is the
  finding that will cost someone a day if it is not written down. The *same
  bytes* encoded three ways:

  | Encoding of the identical XML                         | Result                                                        |
  | ----------------------------------------------------- | ------------------------------------------------------------- |
  | standard base64 (`+` `/` `=`)                         | **400 `Invalid standard metadata value in request`**          |
  | standard base64, padding stripped                     | **400** (same message)                                        |
  | base64**url** (`-` `_`), no padding                   | **200**                                                       |
  | base64**url**, padding kept                           | **200**                                                       |

  Padding is optional; the alphabet is not. The test document happened to
  contain five `+` characters and no `/`. In Rust that is
  `base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(xml)`; in Node,
  `Buffer.from(xml).toString('base64url')`. The same encoding as `{entityId64}`,
  which is at least easy to remember.
- **The 400 is indistinguishable from an empty body.** `{}` returns the exact
  same `Invalid standard metadata value in request`. A CLI cannot tell "you sent
  the wrong base64 alphabet" from "you sent nothing" from "that XML is not
  metadata" — so validate and encode locally and say so in the error text.
- **Response:** `{"importedEntities": ["<entityId>", …]}` and HTTP 200 — note
  **200, not 201**, even though it creates.
- **An `EntitiesDescriptor` aggregate imports every entity it contains in one
  call.** A two-entity aggregate returned both ids in `importedEntities`. So
  `aic saml import <file>` must handle 1..n created entities and report them
  all; one file is not one entity.
- **Re-importing an entity that already exists is `500 Unable to import SAML2
  entity provider`.** `importEntity` is create-only — there is no upsert. To
  re-import, `DELETE` first, which is exactly the `cotlist`-destroying hazard
  this file warns about. A CLI should refuse and say so rather than deleting.
- **`cot` at import time does nothing.** `{"standardMetadata": …, "cot":
  "<name>"}` returns 200 and leaves the named CoT's `trustedProviders`
  unchanged; a `cot` naming a CoT that does not exist is *also* accepted with
  200 and creates nothing. Unknown body fields are simply discarded (contrast
  an unknown `_action`, which is a 403). **Do not offer `--cot` on import** —
  it would silently do nothing. Add membership with a separate CoT `PUT`.
- `importEntity` on the **hosted** collection is `501 importEntity not
  supported`. Metadata import builds remote entities only.

The imported entity's JSON carries the metadata's `singleSignOnService` /
`singleLogoutService` endpoints and `nameIdFormatList`, but **not the signing
certificate** — see below.

## Exporting metadata

```
GET /am/saml2/jsp/exportmetadata.jsp?entityid=<url-encoded entityId>&realm=/<realm>
```

- **Content type `text/xml;charset=utf-8`; root element `<EntityDescriptor>`**
  (singular — a single entity, never an `EntitiesDescriptor`).
- **It takes no authentication.** Fetched with a bearer and with no
  `Authorization` header at all, the two bodies were byte-identical. This is
  correct for SAML — metadata is meant to be public — but it has two
  consequences for us: `aic saml export` needs no token and should not require
  an unlocked agent, and a hosted entity's metadata is readable by anyone who
  knows the tenant host and the entity id.
- **Works for `remote` entities too**, and round-trips the imported signing
  certificate byte-for-byte (verified by comparing the exported
  `<ds:X509Certificate>` against the one in the imported document).
- **Errors are HTTP 200.** An unknown entity id returns status **200**, *no*
  `Content-Type` header, and a plain-text, HTML-escaped body:

  ```
  ERROR : No metadata for entity &quot;https&amp;#x3a;…&quot; under realm &quot;&amp;#x2f;bravo&quot; found.
  ```

  **Never branch on the status code here.** Parse the body as XML, or test the
  `ERROR : ` prefix; a status check will report success for every failure.
- Omitting `realm` defaults to the **root** realm, not the current one — so a
  forgotten `realm=/bravo` produces the `ERROR :` body above rather than the
  entity you meant.

## Where the signing certificate lives

Short answer: **not in the SAML entity.** The entity holds only a *label
identifier*; the key material lives in AM's secret store and is bound to a
secret label.

The field is:

```
<identityProvider|serviceProvider>.assertionContent.signingAndEncryption.secretIdAndAlgorithms.secretIdIdentifier
```

a plain string, whose schema description spells out the mechanism: setting it to
`demo` makes the entity resolve
`am.applications.federation.entity.providers.saml2.demo.signing` and
`…demo.encryption`. Left unset (it is `{}` on every live entity in the sandbox
`bravo`, hosted or remote), AM falls back to the role-wide defaults
`am.default.applications.federation.entity.providers.saml2.{sp,idp}.{signing,encryption,mtls}`.

Verified by construction rather than by reading: a throwaway hosted SP was
`PUT` with `secretIdIdentifier: "probe3key"`, after which the realm's
**secret-mapping schema enum** (`…/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV/mappings?_action=schema`,
see `docs/api/15-secret-mappings.md`) gained exactly three new labels —
`am.applications.federation.entity.providers.saml2.probe3key.{signing,encryption,mtls}`
— which vanished again when the entity was deleted. Nothing but that one string
was changed.

So a SAML signing-key rotation in `aic` is **an ESV secret-mapping operation,
not a SAML operation**:

1. `PUT` the entity with a `secretIdIdentifier` (this creates the labels).
2. Map an ESV secret onto `am.applications.federation.entity.providers.saml2.<id>.signing`
   via `src/secretmap/` (`docs/api/15-secret-mappings.md`).
3. Re-export the metadata and hand it to the peer.

Setting `secretIdIdentifier` alone changes nothing visible: the exported
metadata kept the stock `rsajwtsigningkey` signing certificate and the `test`
encryption certificate, because the new labels resolve to the same defaults
until a mapping exists. **A rotation was not carried through** — steps 2 and 3
are documented from the object model, not measured.

For a **remote** entity the peer's certificate is not in the JSON at all. It is
held with the standard metadata and comes back only through
`exportmetadata.jsp`. There is no REST field to compare, so any "has my peer's
cert changed?" check has to diff exported XML.

## The schema endpoint

`POST …/realm-config/saml2/{hosted,remote}?_action=schema` returns a ~100 KB
draft-07 JSON Schema titled *SAML2 Hosted/Remote Entity Provider*. It is the
authoritative field list — richer than anything in this file — and it carries
the console's own titles, help text, enums (signing/digest/encryption
algorithms) and `attributePath` mappers. Two practical uses:

- generating or validating an entity body without hard-coding AM's four-level
  group nesting;
- answering "what is this field for" without the console. The
  `secretIdIdentifier` explanation above is quoted from it.

`?_action=template` and `?_action=getAllTypes` are **501** on this family, so
`schema` is the only introspection available.

Top-level group names differ by role, which is worth knowing before writing a
renderer:

| Role                  | `assertionContent`                                                                       | `assertionProcessing`                                                                              | `services`                                                | `advanced`                                                                                                              |
| --------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `identityProvider`    | `assertionCache`, `assertionTime`, `authenticationContext`, `basicAuthentication`, `nameIdFormat`, `signingAndEncryption` | `accountMapper`, `attributeMapper`, `localConfiguration`                                           | `assertionIdRequest`, `metaAlias`, `nameIdMapping`, `serviceAttributes` | `applicationContext`, `authnRequestStorage`, `ecpConfiguration`, `idpAdapter`, `idpFinderImplementation`, `relayStateUrlList`, `saeConfiguration`, `sessionSynchronization` |
| `serviceProvider`     | `assertionTimeSkew`, `authenticationContext`, `basicAuthentication`, `clientAuthentication`, `nameIdFormat`, `signingAndEncryption` | `accountMapping`, `adapter`, `attributeMapper`, `autoFederation`, `defaultRelayState`, `redirectTreeConfiguration`, `responseArtifactMessageEncoding`, `url` | `metaAlias`, `serviceAttributes`                          | `ecpConfiguration`, `idpProxy`, `relayStateUrlList`, `saeConfiguration`, `spSessionSyncEnabled`                          |

## Object shapes

### Entity provider — list stub

```json
{
  "_id": "<entityId64>",
  "_rev": "1725473215",
  "entityId": "https://sp-b.example.com",
  "location": "hosted",
  "roles": ["serviceProvider"]
}
```

`location` ∈ `hosted` | `remote`. `roles` is an array; the values observed live
are exactly `serviceProvider` and `identityProvider`, and a dual-role entity
lists both, in that alphabetical order:
`"roles": ["identityProvider","serviceProvider"]`.

**`roles` is absent, not `[]`, on an entity with no role blocks** (verified
2026-09-16 against a freshly created roleless entity). `location` is always
present. A deserialiser must default `roles` to empty rather than requiring it.

### Entity provider — full read (hosted SP)

```json
{
  "_id": "…base64url-without-padding…",
  "_rev": "-168223540",
  "entityId": "https://sp-b.example.com",
  "serviceProvider": {
    "assertionContent":    { "signingAndEncryption": {…}, "nameIdFormat": {…},
                             "authenticationContext": {…}, "assertionTimeSkew": 300,
                             "basicAuthentication": {…} },
    "assertionProcessing": { "attributeMapper": {…}, "accountMapping": {…},
                             "responseArtifactMessageEncoding": {…},
                             "redirectTreeConfiguration": {…}, "adapter": {…} },
    "services":            { "metaAlias": "/bravo/client-b-sp",
                             "serviceAttributes": { "assertionConsumerService": [ … ],
                                                    "singleLogoutService": [ … ],
                                                    "nameIdService": [ … ] } },
    "advanced":            { "saeConfiguration": {…}, "ecpConfiguration": {…},
                             "idpProxy": {…}, "spSessionSyncEnabled": false }
  }
}
```

A pure SP has **no `identityProvider` key at all** — it is absent, not `null`.
(The 2026-05-17 version of this file predicted `"serviceProvider": null` for the
unused role, along with `attributeQueryProvider` and
`xacmlPolicyEnforcementPoint` keys. None of that appears in a real UAT entity;
the unused role and those two keys are simply absent. That prediction came from
library research, not observation — corrected 2026-08-12.)

Notable leaf values seen live:

- `services.metaAlias` — `/bravo/client-b-sp`. This is the SP's routing key; every
  `serviceAttributes` endpoint URL embeds it
  (`/am/AuthConsumer/metaAlias/bravo/client-b-sp`, `/am/SPSloRedirect/…`,
  `/am/SPMniRedirect/…`, `/am/spsaehandler/…`).
- Unset script/plugin slots read as the **string `"[Empty]"`**, not `null` and
  not `""` (`spAccountMapperScript`, `spAdapterScript`, `redirectTreeName`).
  Don't treat `"[Empty]"` as a configured value.
- **Group objects are present but empty when nothing in them is set.** A hosted
  SP created by REST with nothing but a `metaAlias` reads back with every group
  key present and `{}` inside — `signingAndEncryption.requestResponseSigning`,
  `nameIdFormat`, `advanced.idpProxy` and the rest. So "the key exists" says
  nothing about whether the feature is configured; only the leaves do. Two of
  the sandbox's real hosted SPs likewise carry `"encryption": {}` and
  `"secretIdAndAlgorithms": {}`.

### Entity provider — full read (hosted IdP)

The 2026-08-12 version of this file listed the hosted-IdP shape as an open
question (UAT `bravo` had only remote IdPs). Measured 2026-09-16 by giving a
throwaway hosted entity an `identityProvider` block:

```json
"identityProvider": {
  "assertionContent":    { "signingAndEncryption": {…}, "nameIdFormat": {…},
                           "authenticationContext": {…}, "assertionTime": {…},
                           "basicAuthentication": {…}, "assertionCache": {…} },
  "assertionProcessing": { "attributeMapper": {…}, "accountMapper": {…},
                           "localConfiguration": {…} },
  "services":            { "metaAlias": "/bravo/probe3-idp",
                           "serviceAttributes": {…} },
  "advanced":            { "saeConfiguration": {…}, "ecpConfiguration": {…},
                           "sessionSynchronization": {…},
                           "idpFinderImplementation": {…}, "relayStateUrlList": {…},
                           "idpAdapter": {…}, "applicationContext": {…},
                           "authnRequestStorage": {…} }
}
```

The four top-level groups are the same as the SP's; everything below them
differs. The full per-role group list is in "The schema endpoint" above.

### Entity provider — full read (remote IdP, freshly imported)

An entity created by `importEntity` from minimal IdP metadata reads back as:

```json
{
  "_id": "<entityId64>", "_rev": "-695104615",
  "entityId": "https://idp-a.example.com",
  "identityProvider": {
    "assertionContent": {
      "signingAndEncryption": { "requestResponseSigning": { "authenticationRequest": false },
                                "encryption": {}, "secretIdAndAlgorithms": {} },
      "nameIdFormat": { "nameIdFormatList": ["urn:oasis:names:tc:SAML:2.0:nameid-format:persistent"] },
      "secrets": {}, "basicAuthentication": {}, "clientAuthentication": {} },
    "services": { "serviceAttributes": {
      "singleLogoutService": [ { "binding": "…HTTP-Redirect", "location": "https://idp-a.example.com/slo" } ],
      "singleSignOnService": [ { "binding": "…HTTP-Redirect", "location": "https://idp-a.example.com/sso" },
                               { "binding": "…HTTP-POST",     "location": "https://idp-a.example.com/sso" } ] } }
  }
}
```

Three things to notice:

- **No `advanced` key and no `services.metaAlias`** — a remote entity has
  neither. So `advanced` is not a fixed part of the entity shape.
- `assertionContent` gains a `secrets` group the hosted IdP does not have.
- **The signing certificate from the imported metadata is nowhere in the JSON.**
  It is only visible through `exportmetadata.jsp`.

### Circle of Trust

```json
{
  "_id": "client-b",
  "_rev": "-1000217909",
  "status": "active",
  "trustedProviders": [
    "https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2",
    "https://sp-b.example.com|saml2"
  ],
  "_type": {
    "_id": "circlesoftrust",
    "name": "Circle of Trust",
    "collection": true
  }
}
```

- `status` is **`active`**, not the `enabled` this file claimed before
  2026-08-12.
- **`description` is optional and absent when unset** — of the three CoTs in UAT
  `bravo`, one has it and two omit the key entirely. Same trap as
  `config/access` `actions` (CLAUDE.md §8): a round-trip through a typed struct
  would hand two CoTs a `description` they never had. Mutate the parsed `Value`
  in place.
- `trustedProviders` entries are `<entityId>|<protocol>` strings; the protocol
  suffix is `saml2` (AM also understands `wsfed`). `[]` is common — four of the
  five CoTs in sandbox `bravo` have no members at all.
- **Clear `description` by sending `""` or `null`; the key then disappears.**
  Both were accepted with 200 and the subsequent read omitted the key. There is
  no way to store an empty-string description, so `Some("")` and `None` are the
  same state and a type that distinguishes them will drift.
- `_type` is server-supplied decoration (`{"_id":"circlesoftrust","name":"Circle
  of Trust","collection":true}`), identical on every CoT. Strip it from write
  bodies.

## Diagnosing a rejected assertion

The verification that rejects an assertion does **not** happen in the
authentication tree's transaction. Getting this wrong costs an hour, so:

In an SDK-driven tree (`x-requested-with: forgerock-sdk` posting to
`/am/json{realm-path}/authenticate?authIndexType=service&authIndexValue=<tree>`),
the browser POSTs the assertion to `/am/AuthConsumer/metaAlias/{realm}/{alias}`
as a **separate HTTP request with its own transactionId**. That request does the
real work; the tree's `Saml2Node` only reads back the stored outcome and
re-reports it:

```
Saml2Node: AuthConsumer endpoint reported error code samlVerify and message: Issuer%20in%20Response%20is%20invalid.
```

That message is second-hand and **URL-encoded**. The detail lives in the ACS
transaction. To find it:

1. `aic logs tx <id>` on the tree's transactionId gives you only the tree side.
   It is an **exact match**, not a prefix match, so it will not reach the ACS
   transaction even though the ids look related.
2. `aic logs range` a few seconds either side, source `am-core`, then group by
   `payload.transactionId` and keep the groups whose `payload.logger` contains
   `saml2`. The ACS transaction is the one that starts with
   `SAMLUtils: HttpRequest content length=` and contains `SPACSUtils`.
3. `/am/AuthConsumer/…` is **not** logged in `am-access` — it is outside the
   CREST audit filter. Don't look for it by path.

### `verifyResponse` gate order — read this before concluding anything

`SAML2Utils.verifyResponse` applies its checks in a fixed order and stops at the
first failure. Knowing the order matters because **fixing a later gate cannot be
confirmed by a run that fails at an earlier one** — the log simply stops sooner
and looks like "still broken".

Order observed live (2026-08-11/12, UAT `bravo`):

| #   | Gate                                                                          | Log signature when it passes                                                                                        | …when it fails                                                                                                           |
| --- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| 1   | ACS location matches the entity's `assertionConsumerService`                  | `verifyAssertionConsumerServiceLocation: requestUrl = … / acsEndpoint = …`                                          | —                                                                                                                        |
| 2   | `InResponseTo` matches an `AuthnRequest` AM issued and still holds in the CTS | `CTS: Token read` then `verifyResponse:AuthnRequestInfoCopy retrieved from SAML2 repository for inResponseTo: <id>` | `CTS: Token did not exist` then `ERROR … InResponseTo attribute in Response is invalid: <id>, SAML2 failover is enabled` |
| 3   | Issuer is a trusted provider (the `cotlist` check)                            | `LIBCOT` / `COTCache` / `COTUtils` lines                                                                            | `verifyResponse:Issuer in Response is not valid.`                                                                        |
| 4   | Binding / signature                                                           | `verifyResponse:binding is :…HTTP-POST`, `responseIsSigned is :…`                                                   | —                                                                                                                        |

Gate 2 precedes gate 3 — verified from a pre-fix capture where `CTS: Token read`
and `AuthnRequestInfoCopy retrieved` both succeed and the rejection is
nonetheless `Issuer in Response is not valid.`

### Gate 2: you cannot replay a captured `AuthnRequest`

When AM issues an `AuthnRequest` it stores an `AuthnRequestInfo` in the CTS
keyed by the request `ID`, and gate 2 requires that record to still exist. It is
short-lived. So **a SAML capture is not a reusable test fixture**: re-POSTing a
saved `AuthnRequest` gets a freshly minted, perfectly valid `Response` from the
IdP that echoes the _old_ request `ID`, and AM rejects it because it has no
record of that request any more.

Verified 2026-08-12: a request `ID` first issued at 2026-08-11T23:13:44Z was
replayed at 06:53 the next morning. Azure returned a new `Response`
(`IssueInstant` 06:53:12Z, valid signature) whose `InResponseTo` was the
7½-hour-old ID; AM logged `CTS: Token did not exist` and rejected at gate 2,
never reaching the trust check.

The tell is an `InResponseTo` whose ID you can find in an _older_ capture.
Always compare it against the `AuthnRequest ID` of the run you think you are
looking at.

**To test a federation change, start a fresh login from the application** and
let it generate a new `AuthnRequest`.

### Failing at gate 2 also breaks the error path

A gate-2 failure is followed by:

```
Saml2Proxy: An error occurred while verifying the SAML response
Saml2Proxy: getUrlWithError: Unable to determine AuthURL
```

`Saml2Proxy` recovers the URL it should bounce the browser back to from the same
per-request state that just turned out to be missing, so it cannot build the
error redirect either. Practically this changes the _symptom_: a gate-3 failure
redirects back into the tree, which fails and — under an SDK-driven journey —
gets retried, so you see the tree failing repeatedly (15 executions in 20
seconds, in the pre-fix capture). A gate-2 failure never returns to the tree at
all, so **no tree execution is recorded** and the retry loop stops.

Do not read "it stopped looping" as progress on its own. Check which gate the
ACS transaction reached.

### Log signature: trusted vs not trusted

Both flows read the hosted entity's config, then diverge. This is the
fingerprint that distinguishes a `cotlist` problem from anything else:

**Succeeding** — AM resolves a CoT _by name_ out of the entity's `cotlist`:

```
SAML2MetaManager.getEntityConfig: got entity config from SAML2MetaCache: https://sp-a.example.com
ConfigurationInstanceImpl.getAllConfigurationNames: realm = /bravo, componentName = LIBCOT
COTCache:getCircleOfTrust:cacheKey = /bravo//client-a, found = false
ConfigurationInstanceImpl.getConfiguration: componentName = LIBCOT, realm = /bravo, configName = client-a
COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2
COTUtils.setToPrototolMap: check https://sp-a.example.com|saml2
SAML2Utils.verifyResponse:binding is :urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST
```

**Failing** — the same entity-config read, then **no `LIBCOT` lookup at all**:

```
SAML2MetaManager.getEntityConfig: got entity config from SAML2MetaCache: https://sp-b.example.com
SAML2MetaManager.getEntityConfig: got entity config from SAML2MetaCache: https://sp-b.example.com
SAML2Utils.verifyResponse:Issuer in Response is not valid.
```

**The absence of the `LIBCOT` / `COTCache` / `COTUtils` lines is the
diagnosis.** AM had no CoT _name_ to resolve, so the entity's `cotlist` was
empty. If instead you see a `LIBCOT` lookup followed by the rejection, the
`cotlist` is fine and the peer really is missing from that CoT's
`trustedProviders` — a different fault with a different fix.

### Other things that produce a similar message

`SAML2Utils.isSourceSiteValid` also rejects an `Issuer` carrying a `Format`
attribute that is anything other than
`urn:oasis:names:tc:SAML:2.0:nameid-format:entity`. Azure AD omits `Format`
entirely, which is valid. Check the raw `Issuer` element before assuming
`cotlist`.

Entity IDs are compared **exactly** after `trim()`. Azure AD issues
`https://sts.windows.net/<tenant-guid>/` **with** the trailing slash, and it
must be registered that way.

### What the messages cannot tell you

Capturing the `AuthnRequest`/`Response` pair (SAML-tracer or equivalent) is the
usual first move and it is worth doing — but for this fault class it only
excludes suspects. A failing UAT pair and a working one were compared field by
field on 2026-08-12 and were equivalent on every input the trust check reads:
same `NameIDPolicy`, `Issuer` with no `Format` attribute on either,
`Destination` matching the SP's ACS URL, `Status: Success`, `Audience` equal to
the SP entity ID, assertion-signed-but-response-unsigned in both, `InResponseTo`
matching the request `ID`. The only differences were the entity names, the Azure
tenant GUID and the attribute payload.

So: when the messages check out, the fault is server-side state — go to the ACS
transaction's logs. A message capture is still useful for the timestamp, which
is how you locate that transaction.

## Examples

```sh
# List SAML entities in bravo
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/saml2?_queryFilter=true" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"

# Read one entity in full (id is base64url-no-pad of the entityId)
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/saml2/hosted/<entityId64>" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"

# Circles of trust (full documents, not stubs)
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/federation/circlesoftrust?_queryFilter=true" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"

# The field list AM itself publishes (~100 KB; pipe it through jq)
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/saml2/hosted?_action=schema" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0" -X POST

# Create a hosted SP. metaAlias is what makes the role block legal.
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/saml2/hosted/?_action=create" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0" \
  -X POST -H "Content-Type: application/json" \
  --data '{"entityId":"https://sp-a.example.com",
           "serviceProvider":{"services":{"metaAlias":"/bravo/sp-a"}}}'

# Import remote metadata. base64URL — plain base64 is a 400.
b64url=$(base64 -w0 < idp-metadata.xml | tr '+/' '-_' | tr -d '=')
scripts/verify-endpoint.sh \
  "/am/json/realms/root/realms/bravo/realm-config/saml2/remote/?_action=importEntity" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0" \
  -X POST -H "Content-Type: application/json" \
  --data "$(jq -n --arg x "$b64url" '{standardMetadata:$x}')"
```

Metadata export takes no bearer, so `verify-endpoint.sh` is the wrong tool for
it — plain `curl` is enough:

```sh
host=https://<your-tenant>.forgeblocks.com
curl -sS "$host/am/saml2/jsp/exportmetadata.jsp?entityid=https%3A%2F%2Fsp-a.example.com&realm=/bravo"
```

`verify-endpoint.sh` falls back to `.envrc`'s `TENANT_BASE_URL` when the var is
unset, which is the **sandbox** — while the bearer comes from whatever tenant
`aic ctx current` names. Export `TENANT_BASE_URL` explicitly when working a
non-sandbox tenant, or you will send a UAT token to the sandbox host.

## Quirks

- **CoT membership is stored twice and REST shows one side.** See the dedicated
  section above. This is the one that will bite.
- **`{entityId64}` is unpadded base64url** — verified against live ids.
  `URL_SAFE_NO_PAD.encode(...)` in Rust (`base64` crate);
  `Buffer.from(entityId).toString('base64url')` in Node.
- **CoT ids are NOT base64** — the CoT resource id is the plain name (`client-b`).
  Only entity providers are base64url-encoded. Easy to get wrong when both live
  under `realm-config`.
- **The entity list returns stubs**, not full configs — `_id`, `_rev`,
  `entityId`, `location`, `roles`. Follow up with the `/{location}/{entityId64}`
  GET. The CoT list, by contrast, returns full documents.
- **`_rev` differs between the list stub and the full read of the same entity**
  — `1725473215` from the list vs `-168223540` from the read, both stable across
  repeated GETs (verified 2026-08-12, three reads each). So `_rev` is
  per-representation, not a resource version. Never carry a stub's `_rev` into a
  write, and never use `_rev` for drift detection here: use a content snapshot,
  per CLAUDE.md §5.
- **Unset script slots read as `"[Empty]"`**, the literal string.
- **Absent, not `null`.** An unused entity role (`identityProvider` on a pure
  SP), `roles` on a roleless entity, and an unset CoT `description` are omitted
  keys, not `null` values. `advanced` is absent on remote entities too.
- **`importEntity` wants base64URL, and says "invalid" for everything.**
  Standard base64 of a perfectly good document is rejected with the same 400 as
  an empty body. The single most expensive quirk in this file.
- **Entity `PUT` replaces; CoT `PUT` merges.** And CoT `PUT` creates on an
  unknown id (201) while entity `PUT` 404s. One helper cannot serve both.
- **`DELETE` on an entity silently edits every CoT that listed it.** Verified
  2026-09-16. Anything that offers undo has to restore the CoT documents too,
  and a "delete entity" confirmation should name the CoTs it will change.
- **A CoT `PUT` can return 500 and still have written the document.** See the
  CoT-membership section. Re-read after a failure; do not assume a rollback.
- **An unknown `_action` is a 403, not a 404.** `No privilege mapping for
  requested action`. Do not surface it as a permissions error.
- **`exportmetadata.jsp` reports failure with HTTP 200** and no `Content-Type`.
  Detect the `ERROR : ` body, never the status.
- **`exportmetadata.jsp` needs no bearer at all** — the response is identical
  with and without one.
- **`/am/AuthConsumer/…` is not audited** in `am-access`.

## Verified against

Two independent passes. The 2026-08-12 pass was read-only against a UAT tenant
and is the basis for the diagnosis sections; the 2026-09-16 pass exercised the
write surface against the sandbox.

### 2026-08-12 — read-only, UAT `bravo`

- Tenant: `tenant.example.com` (UAT), realm `bravo`
- Date: 2026-08-12
- Calls (all `GET`, all 200):
  - `…/realm-config/saml2?_queryFilter=true` → 5 entities (3 hosted SPs, 2
    remote Azure AD IdPs); stub shape as documented above.
  - `…/realm-config/saml2/hosted/{id64}` for
    `https://sp-b.example.com` and
    `https://sp-a.example.com` → full SP configs, diffed against
    each other (identical but for names/metaAlias).
  - `…/realm-config/saml2/remote/{id64}` for both
    `https://sts.windows.net/{guid}/` IdPs → full IdP configs, diffed against
    each other (identical but for the tenant GUID in endpoint URLs).
  - `…/realm-config/federation/circlesoftrust?_queryFilter=true` → 3 CoTs
    (`servicedesk`, `client-a`, `client-b`), all `status: active`.
  - `…/realm-config/federation/circlesoftrust/client-b` → single document, read
    twice to confirm `_rev` stability.
  - The hosted client-b entity read three times to confirm `_rev` stability and
    compare against the list stub.
- Log evidence (`aic logs range`, sources `am-core` / `am-authentication`,
  2026-08-11 22:00–24:00Z): 15 failing `ClientBLogin` tree executions, all
  rejecting with no `LIBCOT` lookup, against **two independent successful
  `ClientALogin` flows** (ACS transactions at 23:36:18 and 23:40:52), both resolving
  `/bravo//client-a` by name — `found = false` on the first, `found = true` on the
  second off the warm `COTCache`. Full ACS transactions dumped and compared line
  by line. This is the basis for the `cotlist` finding and the log-signature
  section.
- Message-level control: the failing and working `AuthnRequest`/`Response` pairs
  compared field by field (see "What the messages cannot tell you"). Equivalent
  on every input the trust check reads, which is what excludes the message-level
  explanations and leaves server-side state.
- Gate order and the gate-2 replay behaviour: from a further ACS transaction at
  2026-08-12T06:53:12Z, after the `client-b` CoT was recreated in the console. The
  CoT document changed (`_rev` `-1000217909` → `-2115263780`, and it gained a
  `description` key it did not have before) with `trustedProviders` unchanged.
- **Still unconfirmed: whether recreating a CoT in the console repairs the
  entity-side `cotlist`.** It is the expected fix and the CoT document did
  change, but every login attempt since has failed at gate 2 (replayed
  `AuthnRequest`), so no run has reached the trust check. Do not record this as
  verified until a fresh login produces a `LIBCOT` lookup for `/bravo//client-b`.

### 2026-09-16 — the write surface, sandbox `bravo`

- Tenant: `<your-tenant>.forgeblocks.com` (**sandbox**, `aic ctx current` =
  `sandbox`), realm `bravo`. AM `9.0.0-SNAPSHOT`, build
  `2026-September-02 09:17` (`GET /am/json/serverinfo/version`).
- Everything from here down was produced by live calls made on this date. The
  2026-08-12 block above is UAT read-only evidence and is unchanged.
- **Starting and ending state, both confirmed by
  `…/realm-config/saml2?_queryFilter=true` and
  `…/federation/circlesoftrust?_queryFilter=true`:** 10 entity providers (7
  hosted SPs, 3 remote IdPs) and 5 CoTs. Six throwaway entities and three
  throwaway CoTs were created and all were deleted; the closing list matches the
  opening one entity for entity and CoT for CoT, `trustedProviders` included. No
  pre-existing entity or CoT was written to at any point.
- Throwaway names used: hosted `https://sp-probe{,3}.example.com` plus one
  UUID-named entity AM minted from `{}`; remote `https://idp-{a,b,c}.example.com`
  imported from a self-signed descriptor generated locally with `openssl
  req -x509`; CoTs `aic-probe-cot`, `aic-probe-cot-2`, and one UUID-named CoT.

Calls made, and what each one settles:

| Call                                                                   | Result                                    | What it establishes                                          |
| ---------------------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------ |
| `GET …/saml2?_queryFilter=entityId eq "…"`                             | 200, one stub                             | The filter row, previously unexercised                        |
| `GET …/saml2/{hosted,remote}?_queryFilter=true`                        | 400 `Query not supported`                 | Listing exists only on the parent collection                  |
| `POST …/saml2/hosted/?_action=create` `{}`                             | **201**, UUID `entityId`                  | Nothing is required — the CLI must impose `entityId` itself   |
| … `{"entityId":…}`                                                     | 201, roleless                             | `roles` is absent, not `[]`                                   |
| … `{"entityId":…,"serviceProvider":{}}`                                | **500**, nothing created                  | The missing-`metaAlias` failure, and that it names no field   |
| … `{…,"serviceProvider":{"services":{"metaAlias":"/bravo/…"}}}`        | 201                                       | `metaAlias` is the discriminator — the control for the row above |
| `POST …/saml2/remote/?_action=create`                                  | 400 `Create not supported`                | Remote is import-only                                         |
| `POST …/saml2/{hosted,remote}?_action=schema`                          | 200, ~100 KB draft-07                     | `required: ["entityId"]`; the `secretIdIdentifier` semantics   |
| `POST …?_action={template,getAllTypes}`                                | 501                                       | `schema` is the only introspection                            |
| `POST …/saml2/remote/?_action=importEntity` `{}`                       | 400 `Invalid standard metadata value…`    | The field name is `standardMetadata`                          |
| … raw XML string                                                       | 400 (same message)                        | Not a raw-XML envelope                                        |
| … **standard base64**, padded                                          | 400 (same message)                        | ← the falsifier: identical bytes, wrong alphabet              |
| … standard base64, unpadded                                            | 400 (same message)                        | Padding is not the variable                                   |
| … **base64url**, unpadded                                              | **200** `{"importedEntities":[…]}`        | base64url is the requirement                                  |
| … base64url, padded (on a clean realm)                                 | 200                                       | Padding is optional — retested after a `DELETE`, because the first attempt hit the duplicate-import 500 and would have read as a padding failure |
| … repeat import of an existing entity                                  | **500 `Unable to import SAML2 entity provider`** | `importEntity` is create-only                          |
| … `EntitiesDescriptor` wrapping two IdPs                               | 200, **two** ids in `importedEntities`    | One file can be n entities                                    |
| … `{"standardMetadata":…,"cot":"aic-probe-cot"}`                       | 200, CoT `trustedProviders` **unchanged** | `cot` is ignored at import                                    |
| … `{"standardMetadata":…,"cot":"no-such-cot-xyz"}`                     | 200, no CoT created                       | Control: the field is not validated either, so it is not read |
| `POST …/saml2/hosted/?_action=importEntity`                            | 501                                       | Import is remote-only                                         |
| `POST …?_action=bogus`                                                 | **403 `No privilege mapping…`**           | Unknown actions masquerade as permission errors               |
| `PUT …/saml2/hosted/{id64}` full document                              | 200                                       | Update works, no `If-Match` needed                            |
| … with a bogus `_rev` in the body                                      | 200                                       | `_rev` in the body is ignored                                 |
| … with a stale `If-Match` header                                       | 200                                       | No optimistic concurrency — use content snapshots             |
| … `{"entityId":"<same>"}` only                                         | 200, **`serviceProvider` gone**           | `PUT` is a full replace                                       |
| … to an id that does not exist                                         | 404 `Cannot find SAML2 entity`            | No create-by-`PUT` for entities                               |
| … both role blocks with two `metaAlias`es                              | 200, `roles: ["identityProvider","serviceProvider"]` | Dual-role shape, and the hosted-IdP group list      |
| `PUT` with `secretIdIdentifier: "probe3key"`, then mappings `?_action=schema` | enum gains `…saml2.probe3key.{signing,encryption,mtls}` | Where the cert lives; the labels vanished again on entity delete |
| `GET /am/saml2/jsp/exportmetadata.jsp` with a bearer                   | 200, `text/xml`, `<EntityDescriptor>`     | The export row, previously unexercised                        |
| … **with no `Authorization` header**                                   | 200, **byte-identical body**              | The endpoint is unauthenticated                               |
| … for an unknown entity                                                | **200**, no `Content-Type`, `ERROR : …`   | Status codes are useless here                                 |
| … with `realm` omitted                                                 | `ERROR :` for realm `/`                   | `realm` is not defaulted to the current realm                 |
| … for a `remote` entity                                                | 200; `<ds:X509Certificate>` byte-equal to the imported one | Peer certs are readable only this way        |
| … for a roleless entity                                                | 200, `<EntityDescriptor/>` self-closing   | A roleless entity is inert, not broken                        |
| `POST …/circlesoftrust/?_action=create` `{"status":"active"}`          | 201, UUID `_id`                           | `_id` is optional and AM mints one                            |
| … with `_id` in the body                                               | 201, named                                | How to name a CoT on create                                   |
| `PUT …/circlesoftrust/<new id>`                                        | **201**                                   | CoTs create by `PUT`; entities do not                         |
| `PUT` omitting `description`                                           | 200, `description` **survives**           | CoT `PUT` merges                                              |
| `PUT` `{"description":""}` / `{"description":null}`                    | 200, key absent on read                   | Empty string and absent are the same state                    |
| `PUT` `["https://idp-c.example.com\|saml2"]` (entity exists)            | 200                                       | Positive control for the four rows below                      |
| `PUT` `["https://idp-c.example.com\|wsfed"]` (entity exists, no wsfed)  | **500 `…updating the COT memberships`**, document **still written** | The REST write touches the entity side |
| `PUT` `["https://nonexistent.example.com\|saml2"]` **as an addition**   | 500, document still written               | Same, for an unresolvable entity                              |
| … the same entry **as a removal**                                      | 200                                       | Removing an unresolvable member has nothing to do             |
| `PUT` `["garbage-no-pipe"]` add and remove                             | 200 both ways                             | An entry with no protocol is never resolved, so never fails — the discriminator that rules out "AM just validates strings" |
| `DELETE …/saml2/remote/{id64}` of a CoT member                         | 200; the CoT's `trustedProviders` → `[]`  | Entity delete cascades into CoT documents                     |
| `DELETE …/saml2/{location}/{id64}`, `DELETE …/circlesoftrust/{id}`     | 200, echoes the deleted document          | Delete semantics; also the cleanup                            |
| `DELETE` of an id that does not exist                                  | 404 `Cannot find SAML2 entity`            | —                                                             |
| `GET …/saml2?_queryFilter=true` with no `Accept-API-Version`           | 200                                       | The header is optional                                        |
| … with `resource=2.0`                                                  | 404 `Resource '' not found`               | There is no v2 of this resource                               |

- **Not established**, and stated as such above: whether a real assertion
  verifies after a REST-only CoT membership change. Nothing here reads a
  `cotlist`; the evidence is that REST CoT writes *fail in the places a
  cotlist-updating write would fail* and that entity deletes cascade. A
  federation test is still the only proof.
- **Not established:** a signing-key rotation end to end. `secretIdIdentifier`
  was set and the resulting secret labels observed, but no ESV secret was mapped
  onto them and no re-signed assertion was produced.

## Source citations

- frodo-lib: `src/api/Saml2Api.ts`, `src/api/CirclesOfTrustApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/saml.js`,
  `packages/fr-config-push/src/scripts/update-saml.js`.
- Ping docs: <https://apidocs.id.forgerock.io/> (SAML2 section).

Per CLAUDE.md §2, none of these are trusted without a live call — the
`serviceProvider: null` / `attributeQueryProvider` shape corrected above came
from exactly this reading and was wrong.

## Open questions

- **Does a federation actually work after a REST-only CoT membership change?**
  Narrowed 2026-09-16 from "does REST sync the `cotlist` at all" — the evidence
  above says the REST write does drive the entity side. What is left is the
  end-to-end proof: put a hosted SP and a remote IdP in a CoT purely over REST,
  then run a fresh SP-initiated login and look for the `LIBCOT` lookup.
- **Is the entity-side `cotlist` readable at all over REST?** Not present in any
  response we have seen. If it genuinely is not, our tooling can never fully
  validate a federation, and any CoT view we build must say so.
- **Which side's `cotlist` does the runtime need** — hosted only, or both hosted
  and remote? The trust check we traced reads the hosted SP's. The remote IdP's
  may matter for IdP-initiated flows and SLO.
- **How does an entity get into a CoT at creation time?** `cot` on
  `importEntity` is ignored and `?_action=create` has no equivalent, so every
  path is a second `PUT` on the CoT — which is the call that can 500 half-way.
  There may be a console-only flow that does both atomically.
- **Does mapping an ESV secret onto
  `am.applications.federation.entity.providers.saml2.<id>.signing` actually
  change the exported certificate**, and is a restart or cache flush needed? The
  labels appear the moment `secretIdIdentifier` is set; nothing beyond that was
  measured.
- **Is `assertionContent.secrets`** (present on a remote IdP, absent on a hosted
  one) the peer certificate in some form? It reads as `{}` on a freshly imported
  entity, so it may be where a per-entity override would land.
