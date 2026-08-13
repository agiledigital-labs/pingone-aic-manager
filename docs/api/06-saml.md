# 06 — SAML 2.0

Implemented in: —

## Purpose

Manage SAML 2.0 hosted (this tenant is the IdP/SP) and remote (another party is
the IdP/SP) entity providers, plus the circles of trust that bind them. Feature
3 of pingone-aic-manager ("manage OIDC and SAML config") is partly built on this
API.

## Authentication

Service-account bearer. Scope: `fr:am:*`.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`). Always
send `Accept-API-Version: protocol=2.1,resource=1.0`.

### Entity providers

| Op                  | Method   | Path                                                                   | Notes                                         |
| ------------------- | -------- | ---------------------------------------------------------------------- | --------------------------------------------- |
| List                | `GET`    | `/am/json{realm-path}/realm-config/saml2?_queryFilter=true`            | Stubs only — see shape below.                 |
| Filter by entityId  | `GET`    | `/am/json{realm-path}/realm-config/saml2?_queryFilter=entityId+eq+"…"` | **Not yet exercised.**                        |
| Read full           | `GET`    | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | `location` ∈ `hosted` \| `remote`.            |
| Create hosted       | `POST`   | `/am/json{realm-path}/realm-config/saml2/hosted/?_action=create`       | **Not yet exercised.**                        |
| Import remote       | `POST`   | `/am/json{realm-path}/realm-config/saml2/remote/?_action=importEntity` | **Not yet exercised.** Body has XML metadata. |
| Update              | `PUT`    | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | **Not yet exercised.**                        |
| Delete              | `DELETE` | `/am/json{realm-path}/realm-config/saml2/{location}/{entityId64}`      | **Not yet exercised.**                        |
| Export metadata XML | `GET`    | `/am/saml2/jsp/exportmetadata.jsp?entityid={entityId}&realm=/{realm}`  | **Not yet exercised.** Raw XML, not JSON.     |

`{entityId64}` is the entity ID **base64url-encoded without padding** — verified
2026-08-12 by re-deriving the `_id` of two live entities from their `entityId`
(exact match, both an `https://host` form and a form with a trailing `/`).

### Circles of Trust

| Op     | Method   | Path                                                                            | Notes                                          |
| ------ | -------- | ------------------------------------------------------------------------------- | ---------------------------------------------- |
| List   | `GET`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust?_queryFilter=true` | Returns full documents, not stubs.             |
| Read   | `GET`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | `{id}` is the plain CoT name, **not** base64.  |
| Create | `POST`   | `/am/json{realm-path}/realm-config/federation/circlesoftrust/?_action=create`   | **Not yet exercised.**                         |
| Update | `PUT`    | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | **Not yet exercised — see the warning below.** |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/federation/circlesoftrust/{id}`              | **Not yet exercised.**                         |

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
- **Treat `PUT` on a CoT as suspect until verified.** Writing `trustedProviders`
  directly may update the CoT document without syncing the entities' `cotlist`,
  manufacturing exactly the split above. The console's CoT editor does sync both
  (that is what `COTUtils`/`COTManager` are for). Until someone verifies the
  REST path syncs too, do CoT membership changes in the console.
- **Re-importing an entity's metadata is a hazard**, for the same reason:
  `importEntity` rewrites extended metadata and can drop a `cotlist` that was
  added afterwards, breaking a federation that was working, with no visible
  change to the CoT document.

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

`location` ∈ `hosted` | `remote`. `roles` is an array — `serviceProvider` /
`identityProvider`.

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
  suffix is `saml2` (AM also understands `wsfed`).

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
  SP) and an unset CoT `description` are omitted keys, not `null` values.
- **`/am/AuthConsumer/…` is not audited** in `am-access`.

## Verified against

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
- Still **not exercised**: create/update/delete of either resource,
  `importEntity`, `exportmetadata.jsp`, `_queryFilter=entityId eq …`. Everything
  in this file about writes is inference from the read shapes and from AM
  behaviour, and is marked as such in the tables.
- **Still unconfirmed: whether recreating a CoT in the console repairs the
  entity-side `cotlist`.** It is the expected fix and the CoT document did
  change, but every login attempt since has failed at gate 2 (replayed
  `AuthnRequest`), so no run has reached the trust check. Do not record this as
  verified until a fresh login produces a `LIBCOT` lookup for `/bravo//client-b`.

## Source citations

- frodo-lib: `src/api/Saml2Api.ts`, `src/api/CirclesOfTrustApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/saml.js`,
  `packages/fr-config-push/src/scripts/update-saml.js`.
- Ping docs: <https://apidocs.id.forgerock.io/> (SAML2 section).

Per CLAUDE.md §2, none of these are trusted without a live call — the
`serviceProvider: null` / `attributeQueryProvider` shape corrected above came
from exactly this reading and was wrong.

## Open questions

- **Does `PUT` on a circle of trust sync the entities' `cotlist`?** Determines
  whether we can ever offer CoT editing over REST, or must send users to the
  console. Needs a throwaway CoT + entity in the sandbox: write
  `trustedProviders` by REST, then test whether a real assertion verifies.
- **Is the entity-side `cotlist` readable at all over REST?** Not present in any
  response we have seen. If it genuinely is not, our tooling can never fully
  validate a federation, and any CoT view we build must say so.
- **Which side's `cotlist` does the runtime need** — hosted only, or both hosted
  and remote? The trust check we traced reads the hosted SP's. The remote IdP's
  may matter for IdP-initiated flows and SLO.
- Exact body for `?_action=importEntity` — JSON wrapper around the XML, or
  multipart?
- Full shape of `identityProvider` on a hosted IdP (UAT `bravo` has only remote
  IdPs, so the hosted-IdP shape is still undocumented).
