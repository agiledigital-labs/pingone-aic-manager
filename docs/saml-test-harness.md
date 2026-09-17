# Keycloak SAML test harness

A local Keycloak peer for the `aic saml` CLI. It exercises SAML in both
directions (AIC-as-SP and AIC-as-IdP) and can rotate signing certificates, so
one side of a federation can be changed while the other is watched.

This file is the knowledge base, and it is now a **paired** procedure rather
than the Keycloak half of one. The Keycloak side is measured here; the AIC side
was measured against a sandbox tenant by a different pass and lives in
`docs/api/`.
["Running one direction end to end"](#running-one-direction-end-to-end) puts the
two halves in order, and [certificate rotation](#certificate-rotation) does the
same for a key roll.

Claims carry one of three labels:

- **measured** — a live call against the Keycloak container, made by this
  document. Every one of these is Keycloak-side.
- **attributed** — measured against a tenant by another pass and recorded in the
  `docs/api/` file named with the claim. This document did not make the call.
  The citation is the audit trail, and a claim here that contradicts its
  citation is a bug in this file.
- **inferred** — neither. A plausible reading, labelled so a later reader can
  tell.

An inferred or attributed claim restated as measured is the worst outcome
available here.

`harness.sh` itself still never talks to an AIC tenant: it holds no tenant
credential and has no AIC code path. The `aic` commands in the paired procedures
below do.

## The question this harness was built for has moved

The original framing was: hand AIC metadata carrying two signing certificates
and watch whether the import **replaces** the stored certificate or **adds** to
it. `docs/api/06-saml.md` has since measured the import surface, and the
question does not have that shape — **attributed**:

- A remote peer's certificate is not a field on the AIC entity at all. It lives
  inside the stored `standardMetadata` and comes back only through
  `exportmetadata.jsp`, so there is nothing to compare over REST.
- `?_action=importEntity` is **create-only**: re-importing an entity that
  already exists answers 500, not an upsert. There is no "update this peer's
  metadata" call. The only route is `DELETE` then import — and the `DELETE`
  silently edits every circle of trust that listed the entity.

The two-certificate document this harness produces on demand is still the
fixture that matters, but for a different question: whether a rollover
**window** works — whether a peer keeps verifying while both certificates are
published, and whether `aic saml` can ever refresh a peer's metadata without
destroying its CoT membership.

## Why Keycloak, and why two realms

Keycloak is both a SAML IdP (every realm publishes
`/realms/<realm>/protocol/saml/descriptor`) and, via identity brokering, a SAML
SP (per-alias metadata at `/realms/<realm>/broker/<alias>/endpoint/descriptor`).
That covers both directions without a second product.

The two-realm split is the right shape, **measured**:

| Realm     | Role in the test                   | Metadata you hand AIC                              | Entity ID (default)                     |
| --------- | ---------------------------------- | -------------------------------------------------- | --------------------------------------- |
| `aic-idp` | Keycloak is the IdP; AIC is the SP | Realm IdP descriptor                               | `http://localhost:18080/realms/aic-idp` |
| `aic-sp`  | Keycloak is the SP; AIC is the IdP | Broker SP descriptor, **not** the realm descriptor | `http://localhost:18080/realms/aic-sp`  |

The realm descriptor is always an `IDPSSODescriptor`. The broker export is
always an `SPSSODescriptor`. Mixing them in one realm would still work
technically — a realm can be an IdP and a broker at once — but the two entity
IDs would collide on the same URL, and rotating IdP signing keys would also
rotate the SP's. Two realms keep the two AIC directions from sharing a signing
key or an entity ID.

Keycloak has no standalone "I am only an SP" mode. Identity brokering **is** how
it acts as a SAML SP. **Measured** by creating a SAML identity provider and
exporting SP metadata from it; **inferred** that there is no other SP surface
worth using.

## What this is not

- **Not a replay fixture.** `docs/api/06-saml.md` records that AM's
  `verifyResponse` gate 2 requires a short-lived `AuthnRequestInfo` in the CTS
  keyed by the request ID. Replaying a saved `AuthnRequest` always fails at that
  gate. Every login this harness is used for must start fresh in a browser. The
  harness does not save assertions.
- **Not an AIC client.** It does not call the tenant, does not know the sandbox
  hostname, and cannot fetch AIC metadata by URL. Every exchange is file/paste
  (`register-sp`, `register-idp`).
- **Not a production Keycloak.** `start-dev`, H2 on a docker volume,
  `admin`/`admin`. Local-dev credentials, said plainly.

## Bring it up, tear it down

```bash
scripts/saml-harness/harness.sh up
scripts/saml-harness/harness.sh status
scripts/saml-harness/harness.sh metadata aic-idp    # IdP descriptor (paste into AIC)
scripts/saml-harness/harness.sh sp-metadata         # SP descriptor  (paste into AIC)
scripts/saml-harness/harness.sh down                # container gone, volume kept
scripts/saml-harness/harness.sh reset               # wipe volume, start clean
```

`up` is idempotent: a running container is left running and bootstrap skips
resources that already exist. **Measured.**

Every command but `help` checks its prerequisites before doing anything —
`docker`, `curl`, `jq`, `python3` — so a machine missing one gets this script's
own message rather than a parse error from somewhere inside a pipeline. Scratch
files all live in one `mktemp -d` removed by a single `EXIT` trap, which is the
only reason a `die` half way through a command does not leak them; the script's
own comment records 37 survivors from before that trap existed.

`scripts/shellcheck-all.sh` lints every tracked shell script in the repo,
`harness.sh` included, and runs both in CI and in `scripts/release-check.sh`.
Change this script and run it.

Admin console: <http://localhost:18080/admin> — user `admin`, password `admin`.
That is a local-dev credential for this throwaway container.

Test user in both realms: `samluser` / `samluser-pass`, email
`samluser@example.com`. NameID is forced to email on the placeholder SAML client
and on the broker (`saml_force_name_id_format=true`, `nameIDPolicyFormat` =
`urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress`). **Measured** on the
client GET and the IdP GET.

### Defaults this machine forced

| Knob             | Value                                    | Why                                                                                                                                                                                                                                               |
| ---------------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Image            | `quay.io/keycloak/keycloak:26.4.7`       | The task named `:26.4`. That floating tag is not published. Official getting-started currently pins `26.4.7`. **Measured** (`GET /admin/serverinfo` → `26.4.7`).                                                                                  |
| Host port        | `18080` (env `KEYCLOAK_PORT`)            | `:8080` on this machine is already bound (Hasura). **Measured.**                                                                                                                                                                                  |
| Bind             | `127.0.0.1`                              | Local-dev; SAML is front-channel so the browser on this box is the only client that needs to reach it.                                                                                                                                            |
| Hostname in XML  | `http://localhost:18080` (`KC_HOSTNAME`) | Without this, entity IDs and ACS URLs advertise the container-internal `:8080`. **Measured** (`entityID` on the descriptor).                                                                                                                      |
| Compose          | not used                                 | `docker compose` **is** present (v5.1.4). The task said use `docker run` if compose was absent; it is not absent, but a compose file would be a dependency the rest of the repo does not have. The harness is a shell script around `docker run`. |
| nixpkgs Keycloak | not used                                 | The task noted `keycloak-26.7.3` in nixpkgs. The container is the peer we actually ran.                                                                                                                                                           |

Volume: `aic-saml-keycloak-data` → `/opt/keycloak/data` (H2). Container name:
`aic-saml-keycloak`.

Ready means the admin-cli password grant returns a token, not merely that
`GET /realms/master` is 200. On a cold volume that was **measured** at 14s; a
second `up` against an already-running container was 0s.

## Topology

```text
AIC-as-SP (the common direction)
  browser  →  AIC hosted SP  →  Keycloak aic-idp  (SAML IdP)
  paste: harness.sh metadata aic-idp  →  AIC remote IdP import
  paste: AIC hosted-SP metadata       →  harness.sh register-sp <file>

AIC-as-IdP
  browser  →  Keycloak aic-sp account console  →  AIC hosted IdP
  paste: AIC hosted-IdP metadata      →  harness.sh register-idp <file>
  paste: harness.sh sp-metadata       →  AIC remote SP import
```

Until those pastes happen, `aic-idp` has a placeholder SAML client
(`https://sp.example.com`, ACS `https://sp.example.com/acs`) and `aic-sp` has a
placeholder SAML identity provider (`idpEntityId` `https://idp.example.com`).
The placeholders exist so the realms are complete and the metadata endpoints
already serve XML; they are not a working federation with AIC.

`wire-loopback` points `aic-sp`'s broker at `aic-idp` and registers `aic-sp` as
a SAML client of `aic-idp`. That is a Keycloak-only smoke, not the AIC path.
**Measured:** after `wire-loopback`, `idpEntityId` became
`http://localhost:18080/realms/aic-idp` and the SP descriptor gained a signing
`KeyDescriptor`.

A SAML capture is not a test fixture (see above). To actually log in, open the
account console and start a fresh login:

- IdP side: <http://localhost:18080/realms/aic-idp/account/>
- SP side: <http://localhost:18080/realms/aic-sp/account/>

Those URLs return 200. **Measured.** Completing a browser login was **not
exercised** in this slice.

## What the AIC side does — attributed, not measured here

Everything in this section was measured against a sandbox tenant by a different
pass and recorded in `docs/api/`. It is repeated here because it is what a
reader of _this_ file needs in order to act, and because two of the rows change
what "rotate a certificate" even means. This document made none of these calls.

| Claim                                                                                                                                                                                                                                                                                                                                                                                                                                       | Recorded in                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| **AIC's signing certificate is not in the SAML entity.** The entity carries `…signingAndEncryption.secretIdAndAlgorithms.secretIdIdentifier`, a _label_ into AM's secret store (`am.applications.federation.entity.providers.saml2.<id>.{signing,encryption,mtls}`); left unset it falls through to a realm default. Rotating an AIC certificate is an **ESV secret-version** operation with one SAML field in front of it, not a SAML one. | `06-saml.md`                          |
| **Replace by default; two certificates only during a rollover.** The published set is the mapped ESV secret's **ENABLED versions**: one enabled version → one `<KeyDescriptor use="signing">`; `add-version` → two, the newest first; disable the old one → back to one within ~5s.                                                                                                                                                         | `06-saml.md`, `03-esvs.md`            |
| **Create the ESV secret with placeholders off** (`--no-placeholders`) and **no restart is needed** — it reads `loaded: true` on creation. Created _with_ placeholders it reads `loaded: false`, which is the class that does need a tenant restart.                                                                                                                                                                                         | `06-saml.md`, `03-esvs.md`            |
| **AIC emits no `<ds:KeyName>`.** A peer holding two-certificate metadata can only tell the two apart by the certificates. This is exactly why the harness compares fingerprints.                                                                                                                                                                                                                                                            | `06-saml.md`                          |
| **`exportmetadata.jsp` needs no authentication**, and a failed export is **HTTP 200** with a plain-text `ERROR : …` body. Never branch on the status code.                                                                                                                                                                                                                                                                                  | `06-saml.md`                          |
| **`importEntity` is create-only** (500 on re-import, not an upsert) and takes `{"standardMetadata": "<base64url, unpadded>"}` — the standard base64 alphabet is refused with the same 400 as sending `{}`. A `cot` key in the body is silently ignored.                                                                                                                                                                                     | `06-saml.md`                          |
| **`DELETE` on an entity silently edits every circle of trust that listed it.**                                                                                                                                                                                                                                                                                                                                                              | `06-saml.md`                          |
| **CoT membership lives in two places**: `trustedProviders` on the CoT document (visible over REST) _and_ each entity's `cotlist` in extended metadata (**not** exposed over REST). The runtime reads `cotlist`.                                                                                                                                                                                                                             | `06-saml.md`                          |
| **One alias per mapping on AIC** — a mapping `PUT` carrying two aliases is a `400 … Only a single alias per mapping is allowed for this secret store type`. Versions of one ESV secret are the only way to publish two certificates for one purpose.                                                                                                                                                                                        | `06-saml.md`, `15-secret-mappings.md` |

### What `aic` can do today

As of 2026-09-18, `aic saml` reads **and writes** (`docs/CLI.md` is the full
reference; check it before assuming this table is still complete):

| Verb                                                         | Unlocked agent?                  |
| ------------------------------------------------------------ | -------------------------------- |
| `aic saml list [--location …] [--role …] [--realm …]`        | yes                              |
| `aic saml show <ENTITY-ID> [--location …] [--realm …]`       | yes                              |
| `aic saml metadata export <ENTITY-ID> --realm <r> [--out P]` | **no** — the JSP takes no bearer |
| `aic saml metadata inspect <FILE>`                           | no — local file                  |
| `aic saml metadata sanitise <FILE> [--out P]`                | no — local file                  |
| `aic saml cot list \| show [--realm …]`                      | yes                              |
| `aic saml create-hosted <ENTITY-ID> --role … --meta-alias …` | yes                              |
| `aic saml import <FILE> [--dry-run]`                         | yes                              |
| `aic saml delete <ENTITY-ID> [--force]`                      | yes                              |
| `aic saml rotate status \| init \| stage \| complete`         | yes                              |

The only thing still missing is a circle-of-trust **write** verb: `cot list`
and `cot show` read the CoT document, but nothing here creates a circle or
edits its membership, so a step below that needs one gives the REST call from
`docs/api/06-saml.md` instead. That gap is deliberate — membership is stored
twice and REST exposes only half of it, so a write verb could not confirm its
own result. `scripts/verify-endpoint.sh` is a `GET` helper and will not make
that write; the console or a hand-rolled `curl` with the agent's bearer will.

Run every `aic` invocation with `--no-prompt`, so a locked daemon fails fast
instead of waiting for a master password (`.ai/core.md` §8).

The two sides agree on fingerprints, which is what makes them pairable.
`aic saml metadata inspect` on the document `harness.sh metadata aic-idp`
reports the same SHA-256 that `harness.sh status` prints for that key:

```text
harness.sh status:          IDPSSODescriptor  d84954d3…  KeyName=ZC7tnJ0k…
aic saml metadata inspect:  {"key_use":"signing","key_name":"ZC7tnJ0k…","sha256":"d84954d3…"}
```

**Measured 2026-09-17** against the running container, Keycloak 26.4.7. The same
`inspect` reported `"signed": false` and `"would_remove": []` — Keycloak's realm
descriptor carries neither an enveloped signature nor a WS-Federation
`RoleDescriptor`, so `sanitise` has nothing to strip. The Entra strip is not
needed for this peer.

## Running one direction end to end

Two recipes. Run **one direction at a time**: `aic-idp` and `aic-sp` are
separate realms precisely so the two directions share no entity ID and no
signing key, and a half-wired federation in the other direction is one more
thing to rule out when a login fails.

Entity IDs below are the reserved placeholders from `.ai/core.md` §3. Substitute
your own — and never paste tenant output into this file: the sandbox `bravo`
realm holds real client entity IDs.

### Direction A — AIC is the SP, Keycloak is the IdP

1. **Bring the peer up.**

   ```bash
   scripts/saml-harness/harness.sh up
   scripts/saml-harness/harness.sh status
   ```

   `status` names the entity ID, the descriptor URL and the signing fingerprint
   per role descriptor. That fingerprint is the one AIC must end up holding for
   this peer, and the one to re-check after any rotation.

2. **Take Keycloak's IdP metadata and read it before sending it.**

   ```bash
   scripts/saml-harness/harness.sh metadata aic-idp > kc-idp.xml
   aic saml metadata inspect kc-idp.xml --no-prompt
   ```

   `inspect` is a local file read — no tenant, no agent. It prints the entity
   ID, the roles, the endpoints, and the certificate fingerprints, and says what
   `sanitise` would remove. On this document that is nothing (**measured**), so
   there is no sanitise step in this direction.

3. **Import it into AIC as a remote IdP.** No CLI verb yet. The call
   (`docs/api/06-saml.md`):

   ```text
   POST /am/json/realms/root/realms/<realm>/realm-config/saml2/remote/?_action=importEntity
   Content-Type: application/json

   {"standardMetadata": "<kc-idp.xml, base64url, unpadded>"}
   ```

   200 (not 201) with
   `{"importedEntities": ["http://localhost:18080/realms/aic-idp"]}`. The
   **url-safe** alphabet is mandatory: standard base64 is refused with the same
   400 as an empty body, so a CLI cannot tell the two apart and must encode
   locally and say so. Confirm with
   `aic saml list --location remote --realm <realm> --no-prompt`.

   Re-running this after a change to the Keycloak descriptor is **not** how you
   update the entity — see "The question this harness was built for has moved".

4. **Have a hosted SP on AIC.** If one does not exist, create it with
   `?_action=create`; `serviceProvider.services.metaAlias` is the field that
   makes the role block legal and nothing else is required (`06-saml.md`). Read
   it back with
   `aic saml show "https://sp-a.example.com" --realm <realm> --no-prompt` — the
   summary prints the `metaAlias` and the signing secret identifier, which is
   the field the rotation section turns on.

5. **Put both entities in one circle of trust.** `PUT` the CoT document with
   `trustedProviders: ["https://sp-a.example.com|saml2", "http://localhost:18080/realms/aic-idp|saml2"]`.
   Two attributed traps: a `500` from that `PUT` **still writes** the document,
   so re-read rather than assuming nothing happened; and membership also lives
   in each entity's `cotlist`, which REST never shows. See "Still unproven".

6. **Export AIC's hosted-SP metadata and register it with Keycloak.**

   ```bash
   aic saml metadata export "https://sp-a.example.com" \
     --realm <realm> --out aic-sp.xml --no-prompt
   scripts/saml-harness/harness.sh register-sp aic-sp.xml
   ```

   `export` reaches the tenant over an endpoint that takes no bearer, so it
   works against a locked daemon (**attributed**) — and a failure arrives as
   HTTP 200 with an `ERROR :` body, which is why the CLI classifies the body
   rather than the status. `register-sp` runs the XML through Keycloak's
   `client-description-converter` and creates, or updates, a SAML client whose
   `clientId` is the entity ID, then forces email NameID so the NameID stays
   predictable whatever the metadata listed first. **Measured.** Feed it an
   `IDPSSODescriptor` by mistake and the converter answers 500.

7. **Log in — fresh, in a browser.** AIC-as-SP is driven from a journey holding
   a `Saml2Node`; that is the shape `06-saml.md` diagnosed a live failure in.
   Sign in at the Keycloak prompt as `samluser` / `samluser-pass`. Do **not**
   replay a captured `AuthnRequest`: AM requires an `AuthnRequestInfo` still in
   the CTS keyed by that request ID, so a replay always fails at gate 2
   (**attributed**, and the reason this harness saves no assertions).

8. **When it fails, read the ACS transaction, not the tree's.** The browser
   POSTs the assertion to `/am/AuthConsumer/metaAlias/<realm>/<alias>` as a
   separate HTTP request with its own transaction id, and that path is not in
   `am-access` at all. `aic logs range` a few seconds either side with
   `--source am-core`, group by `payload.transactionId`, keep the groups whose
   `payload.logger` mentions `saml2`. The gate-order table in `06-saml.md` says
   which failure you are looking at — and fixing a later gate cannot be
   confirmed by a run that still fails at an earlier one. All **attributed**.

### Direction B — AIC is the IdP, Keycloak is the SP

1. **Bring the peer up**, as above. Keycloak's SP surface is identity brokering:
   the `aic` alias in realm `aic-sp`.

2. **Export AIC's hosted-IdP metadata.**

   ```bash
   aic saml metadata export "https://idp-a.example.com" \
     --realm <realm> --out aic-idp.xml --no-prompt
   aic saml metadata inspect aic-idp.xml --no-prompt
   ```

   The `certs` array is where a rotation lands. There will be no key name to
   read: AM emits no `<ds:KeyName>` (**attributed**), so the fingerprint is the
   only discriminator on this side.

3. **Register it as the brokered IdP.**

   ```bash
   scripts/saml-harness/harness.sh register-idp aic-idp.xml
   ```

   That posts the file to `identity-provider/import-config` as multipart, merges
   the returned **config map** over whatever the alias already had, and
   `POST`s/`PUT`s the instance. It fixes two things for you, both **measured**:
   `import-config` returns `idpEntityId` (the remote IdP) and never `entityId`
   (this SP), so the harness pins `entityId` to
   `http://localhost:18080/realms/aic-sp`; and it takes the **first**
   `NameIDFormat` in the document, so the harness pins `nameIDPolicyFormat` back
   to email.

4. **Take the broker's SP metadata.**

   ```bash
   scripts/saml-harness/harness.sh sp-metadata > kc-sp.xml
   ```

   This is the **broker** descriptor. `/realms/aic-sp/protocol/saml/descriptor`
   is the `aic-sp` realm's own _IdP_ descriptor and is the wrong document to
   hand AIC. The ACS AIC must POST the assertion to is
   `http://localhost:18080/realms/aic-sp/broker/aic/endpoint`, not the realm's
   `/protocol/saml`. Whether this document publishes a signing `KeyDescriptor`
   follows `wantAuthnRequestsSigned` on the broker, which step 3 copies out of
   AIC's metadata. Before any import the placeholder publishes none — **measured
   2026-09-17**:

   ```xml
   <md:SPSSODescriptor protocolSupportEnumeration="…"
       AuthnRequestsSigned="false" WantAssertionsSigned="true">
   ```

5. **Import `kc-sp.xml` into AIC as a remote SP** — the same `importEntity` call
   as direction A, step 3, and create-only in the same way.

6. **Circle of trust**, as direction A, step 5.

7. **Log in — fresh, in a browser.** Start at
   <http://localhost:18080/realms/aic-sp/account/> and pick the `aic` broker.
   Keycloak issues the `AuthnRequest` to AIC and consumes the assertion at its
   broker ACS. First-broker-login prompting and account linking are **not
   exercised**; the harness sets `trustEmail: true` on the alias (**measured**),
   which is the knob that usually decides whether Keycloak stops to ask
   (**inferred**).

8. **Keycloak-side failures land in the container log**:
   `docker logs -f aic-saml-keycloak`. **Inferred** — not exercised; no
   assertion has been through this path.

## Admin REST shapes

Base URL from the host: `http://127.0.0.1:18080`. There is **no** `/auth` prefix
(Keycloak 17+). All admin calls send `Authorization: Bearer <token>` and, for
JSON bodies, `Content-Type: application/json`.

Creates return **201 + `Location` + empty body**. Updates return **204**.
Deletes return **204**. Missing realms return **404**. **Measured.**

### Token

```http
POST /realms/master/protocol/openid-connect/token
Content-Type: application/x-www-form-urlencoded

client_id=admin-cli&username=admin&password=admin&grant_type=password
```

Returns a JSON access token. **Measured.** `kcadm.sh` inside the container works
too; this harness uses curl so the shapes below are the ones a later agent can
copy.

### Create realm

```http
POST /admin/realms
```

```json
{
  "realm": "aic-idp",
  "enabled": true,
  "displayName": "AIC-as-SP peer (Keycloak IdP)"
}
```

`GET /admin/realms/aic-idp` then returns (trimmed, **measured**):

```json
{
  "id": "<realm-uuid>",
  "realm": "aic-idp",
  "enabled": true,
  "displayName": "AIC-as-SP peer (Keycloak IdP)",
  "sslRequired": "external",
  "defaultSignatureAlgorithm": "RS256",
  "loginWithEmailAllowed": true
}
```

`id` is a UUID. It is the `parentId` of every key provider. It is **not** the
realm name. `sslRequired: external` is why HTTP on localhost works.

No SAML-specific field on the realm object controls `WantAuthnRequestsSigned`.
Realm `attributes` after create are CIBA / device code / PAR keys only.
**Measured.**

### Create user

```http
POST /admin/realms/{realm}/users
```

```json
{
  "username": "samluser",
  "enabled": true,
  "email": "samluser@example.com",
  "emailVerified": true,
  "firstName": "SAML",
  "lastName": "User",
  "credentials": [
    { "type": "password", "value": "samluser-pass", "temporary": false }
  ]
}
```

Lookup: `GET /admin/realms/{realm}/users?username=samluser&exact=true` returns
`[]` if missing, or a one-element array. `requiredActions` is `[]` when email is
pre-verified. **Measured.**

### Create SAML client (Keycloak-as-IdP side)

```http
POST /admin/realms/aic-idp/clients
```

A **minimal** body `{clientId, protocol: "saml", enabled: true}` is enough to
get a 201. Keycloak then fills in defaults that **will** trip a later AIC import
if you leave them. **Measured** defaults on that minimal create:

| Attribute                   | Default      | Why it matters                                                       |
| --------------------------- | ------------ | -------------------------------------------------------------------- |
| `saml.client.signature`     | `"true"`     | Keycloak requires the SP to sign `AuthnRequest`s. AIC may not.       |
| `saml_name_id_format`       | `"username"` | Not email.                                                           |
| `saml_force_name_id_format` | `"false"`    | NameID follows the request, so it is not predictable.                |
| `saml.assertion.signature`  | absent       | Response is signed (`saml.server.signature=true`); assertion is not. |
| `saml.signing.private.key`  | **present**  | Auto-minted. Never commit a GET of this client.                      |

The harness therefore POSTs the attributes it wants on create, including
`saml.client.signature=false`. **Measured:** that create does **not** mint a
client private key (`has("saml.signing.private.key") == false`).

Body the harness actually sends:

```json
{
  "clientId": "https://sp.example.com",
  "name": "Placeholder AIC SP (replace via register-sp)",
  "protocol": "saml",
  "enabled": true,
  "redirectUris": ["https://sp.example.com/acs", "https://sp.example.com/*"],
  "attributes": {
    "saml.client.signature": "false",
    "saml_name_id_format": "email",
    "saml_force_name_id_format": "true",
    "saml.assertion.signature": "true",
    "saml.server.signature": "true",
    "saml.force.post.binding": "true",
    "saml.authnstatement": "true",
    "saml_assertion_consumer_url_post": "https://sp.example.com/acs"
  },
  "protocolMappers": [
    {
      "name": "email",
      "protocol": "saml",
      "protocolMapper": "saml-user-property-mapper",
      "consentRequired": false,
      "config": {
        "user.attribute": "email",
        "friendly.name": "email",
        "attribute.name": "email",
        "attribute.nameformat": "urn:oasis:names:tc:SAML:2.0:attrname-format:basic"
      }
    }
  ]
}
```

`register-sp <file>` is the real path once AIC hosted-SP metadata exists:

```http
POST /admin/realms/aic-idp/client-description-converter
Content-Type: application/json
```

Body is the **raw XML**, not a JSON wrapper. **Measured:**

- SPSSODescriptor → 200, a `ClientRepresentation` (`clientId` = entityID,
  `protocol` = `saml`, ACS URLs in `attributes` and `redirectUris`).
- IDPSSODescriptor → 500 `unknown_error`. Do not feed IdP metadata in here.

Then `POST /admin/realms/aic-idp/clients` with that representation, or
`PUT /admin/realms/aic-idp/clients/{uuid}` if a client with that `clientId`
already exists.

### Create identity provider (Keycloak-as-SP side)

```http
POST /admin/realms/aic-sp/identity-provider/instances
```

```json
{
  "alias": "aic",
  "displayName": "AIC (placeholder — replace via register-idp)",
  "providerId": "saml",
  "enabled": true,
  "trustEmail": true,
  "config": {
    "entityId": "http://localhost:18080/realms/aic-sp",
    "idpEntityId": "https://idp.example.com",
    "singleSignOnServiceUrl": "https://idp.example.com/sso",
    "nameIDPolicyFormat": "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress",
    "principalType": "SUBJECT",
    "postBindingResponse": "true",
    "postBindingAuthnRequest": "true",
    "wantAuthnRequestsSigned": "false",
    "wantAssertionsSigned": "true",
    "wantAssertionsEncrypted": "false",
    "validateSignature": "false"
  }
}
```

**The `entityId` trap, measured.** In this config:

- `entityId` is **this SP's** entity ID. It is what the exported SPSSODescriptor
  puts in `entityID=`.
- `idpEntityId` is the **remote IdP's** entity ID.

`import-config` (below) returns `idpEntityId` and does **not** return
`entityId`. Feeding the remote IdP's entity ID into `entityId` makes the SP
metadata claim to _be_ the IdP. The first attempt in this slice did exactly
that; the SPSSODescriptor's `entityID` became `https://idp.example.com`. The
harness now sets `entityId` to `http://localhost:18080/realms/aic-sp` and never
lets import overwrite it.

Keycloak also fills `syncMode: "LEGACY"` if omitted. **Measured** on GET after
create.

### Import IdP metadata from a file

```http
POST /admin/realms/aic-sp/identity-provider/import-config
Content-Type: multipart/form-data
```

Fields: `providerId=saml`, `file=@descriptor.xml;type=application/xml`.

Returns 200 and a **config map**, not an identity-provider object. You merge it
into `config` and POST/PUT `/identity-provider/instances`. **Measured** against
the `aic-idp` descriptor (so: Keycloak importing Keycloak):

```json
{
  "validateSignature": "true",
  "signingCertificate": "…redacted…",
  "postBindingLogout": "true",
  "singleLogoutServiceUrl": "http://localhost:18080/realms/aic-idp/protocol/saml",
  "postBindingResponse": "true",
  "nameIDPolicyFormat": "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent",
  "idpEntityId": "http://localhost:18080/realms/aic-idp",
  "loginHint": "false",
  "enabledFromMetadata": "true",
  "postBindingAuthnRequest": "true",
  "syncMode": "LEGACY",
  "singleSignOnServiceUrl": "http://localhost:18080/realms/aic-idp/protocol/saml",
  "wantAuthnRequestsSigned": "true",
  "artifactResolutionServiceUrl": "http://localhost:18080/realms/aic-idp/protocol/saml/resolve",
  "addExtensionsElementWithKeyInfo": "false",
  "artifactBindingResponse": "false"
}
```

Two things in that map will bite if you apply it verbatim:

1. `nameIDPolicyFormat` is **persistent** — Keycloak's IdP descriptor lists
   persistent first, and import-config takes the first `NameIDFormat`. The
   harness overrides it back to email after import. **Measured.**
2. `wantAuthnRequestsSigned` is `"true"` because the IdP descriptor advertises
   `WantAuthnRequestsSigned="true"`. After this merge the SP descriptor
   **gains** a signing `KeyDescriptor` and `AuthnRequestsSigned="true"`.
   **Measured.** Before the merge, with the placeholder's
   `wantAuthnRequestsSigned=false`, the SP descriptor had **zero**
   `KeyDescriptor`s.

`fromUrl` JSON import
(`{"providerId":"saml","fromUrl":"http://127.0.0.1:18080/…"}`) returned **500**
`unknown_error`. **Measured.** The Keycloak process is inside the container;
host port 18080 is not where it listens. AIC cannot fetch localhost either.
File/paste is the only path that matters.

Raw XML with `Content-Type: application/xml` returned **415**. **Measured.**

### Export SP metadata

Public, no auth:

```http
GET /realms/aic-sp/broker/aic/endpoint/descriptor
```

Same bytes as the admin export
`GET /admin/realms/aic-sp/identity-provider/instances/aic/export`
(`Content-Type: application/xml`). **Measured** (1524 bytes on the placeholder,
larger once a signing key is advertised).

`GET /realms/aic-sp/protocol/saml/descriptor` is the **IdP** descriptor for the
`aic-sp` realm. It is the wrong document to hand AIC as a remote SP.

### Keys

```http
GET /admin/realms/{realm}/keys
GET /admin/realms/{realm}/components?type=org.keycloak.keys.KeyProvider
POST /admin/realms/{realm}/components
DELETE /admin/realms/{realm}/components/{id}
```

A fresh realm has four providers. **Measured:**

| name                   | providerId          | use (from `/keys`) | in SAML descriptor?  |
| ---------------------- | ------------------- | ------------------ | -------------------- |
| `rsa-generated`        | `rsa-generated`     | SIG / RS256        | yes, `use="signing"` |
| `rsa-enc-generated`    | `rsa-enc-generated` | ENC / RSA-OAEP     | **no**               |
| `hmac-generated-hs512` | `hmac-generated`    | SIG / HS512        | no                   |
| `aes-generated`        | `aes-generated`     | ENC / AES          | no                   |

So a default realm's SAML descriptor has **one** `KeyDescriptor`, not two. The
encryption RSA key is not published in SAML metadata. **Measured.**

`config` values on components are **arrays of strings**: `"priority": ["100"]`.
`parentId` is the realm UUID. The default `rsa-generated` component's config is
only `{priority: ["100"]}` — `algorithm` / `enabled` / `active` are omitted and
default. **Measured.**

Add a second signing key:

```http
POST /admin/realms/aic-idp/components
```

```json
{
  "name": "rsa-generated-rotation-200",
  "providerId": "rsa-generated",
  "providerType": "org.keycloak.keys.KeyProvider",
  "parentId": "<realm-uuid>",
  "config": {
    "priority": ["200"],
    "enabled": ["true"],
    "active": ["true"],
    "algorithm": ["RS256"]
  }
}
```

Highest priority becomes `active.RS256` on `GET /keys`. Both keys have
`status: "ACTIVE"` — "active" here means usable for verification. **Measured**
that the field moves with priority. That the same field decides which key
actually **signs** is the natural reading of `active`, but no `<ds:Signature>`
has been read here to confirm it, so treat that half as **inferred**. AIC has
the identical open question during its own two-key window — see
[which certificate actually signs](#which-certificate-actually-signs-is-unproven).

## What Keycloak's SAML metadata actually looks like

Served at `GET /realms/aic-idp/protocol/saml/descriptor`, one line, no XML
declaration. The harness pretty-prints it. Certificate and KeyName redacted;
everything else is a live descriptor from `aic-idp` after `harness.sh up`.
**Measured 2026-09-16**, Keycloak 26.4.7.

```xml
<md:EntityDescriptor
    xmlns:ds="http://www.w3.org/2000/09/xmldsig#"
    xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata"
    entityID="http://localhost:18080/realms/aic-idp">
  <md:IDPSSODescriptor
      WantAuthnRequestsSigned="true"
      protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <md:KeyDescriptor use="signing">
      <ds:KeyInfo>
        <ds:KeyName>…kid…</ds:KeyName>
        <ds:X509Data>
          <ds:X509Certificate>…redacted…</ds:X509Certificate>
        </ds:X509Data>
      </ds:KeyInfo>
    </md:KeyDescriptor>
    <md:ArtifactResolutionService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:SOAP"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml/resolve"
        index="0" />
    <md:SingleLogoutService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleLogoutService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleLogoutService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Artifact"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleLogoutService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:SOAP"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:NameIDFormat>urn:oasis:names:tc:SAML:2.0:nameid-format:persistent</md:NameIDFormat>
    <md:NameIDFormat>urn:oasis:names:tc:SAML:2.0:nameid-format:transient</md:NameIDFormat>
    <md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified</md:NameIDFormat>
    <md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat>
    <md:SingleSignOnService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleSignOnService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleSignOnService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:SOAP"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
    <md:SingleSignOnService
        Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Artifact"
        Location="http://localhost:18080/realms/aic-idp/protocol/saml" />
  </md:IDPSSODescriptor>
</md:EntityDescriptor>
```

### What matters for an AIC import

| Element                                         | Role for AIC                                                            | Hazard                                                                                                                                                                                                                                                                            |
| ----------------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `entityID`                                      | Remote IdP entity id. Compared **exactly** after trim (`06-saml.md`).   | Includes the port. If AIC later reaches this IdP through a different host, the entity ID will not match.                                                                                                                                                                          |
| `KeyDescriptor use="signing"`                   | The cert AIC must trust. `KeyName` is the Keycloak kid.                 | After rotation there are two of these, and there is no `KeyName` on the AIC side to tell them apart. AIC has no update path for a remote peer's metadata at all — see "The question this harness was built for has moved".                                                        |
| `SingleSignOnService` HTTP-POST / HTTP-Redirect | The only two bindings a browser federation needs. Same `Location`.      | SOAP and HTTP-Artifact are also advertised. Entra's `federationmetadata.xml` is rejected by AIC for being similarly fat — **inferred** that Keycloak's extras are in the same class.                                                                                              |
| `NameIDFormat` × 4, persistent first            | AIC / the SP request picks one. The harness forces email on the client. | An importer that takes the first format gets persistent. Keycloak's own `import-config` does exactly that. **Measured.**                                                                                                                                                          |
| `WantAuthnRequestsSigned="true"`                | Advertises that this IdP wants signed `AuthnRequest`s.                  | Stayed `true` after the only SAML client was set to `saml.client.signature=false`. No realm field flips it. **Measured.** If AIC's importer honours this flag, AIC will be told to sign requests. Whether AIC actually signs is **not measured** (this slice does not touch AIC). |
| `ArtifactResolutionService`                     | SOAP artifact resolve.                                                  | Another Entra-like extra. **Inferred** trip for a strict importer.                                                                                                                                                                                                                |
| No `SPSSODescriptor`                            | This document is IdP-only.                                              | Hand the broker export to AIC when Keycloak is the SP.                                                                                                                                                                                                                            |

The signing cert is 10-year self-signed, CN = realm name (`aic-idp`).
**Measured** (the CN is visible as ASCII in the DER). Do not commit the cert;
`aic saml` should treat it as opaque base64.

### SP descriptor (broker), after `wire-loopback`

Placeholder (no signed requests) has **no** `KeyDescriptor`, one email
`NameIDFormat`, ACS at
`http://localhost:18080/realms/aic-sp/broker/aic/endpoint` for POST, Redirect,
and Artifact. **Measured.**

After importing the IdP descriptor, `AuthnRequestsSigned="true"` and a signing
`KeyDescriptor` appear. ACS URL does not change. **Measured.**

That ACS is the URL AIC (as IdP) must POST the assertion to. It is **not** the
realm's `/protocol/saml`.

## Certificate rotation

Commands:

```bash
scripts/saml-harness/harness.sh rotate-add aic-idp   # higher-priority rsa-generated
scripts/saml-harness/harness.sh rotate-rm  aic-idp   # drop the lowest-priority one
scripts/saml-harness/harness.sh verify-rotate aic-idp
```

`rotate-rm` refuses if only one `rsa-generated` provider remains. `rotate-add`
names the key it published — the identity that is present in the descriptor
after the add and absent before it — so it says what it did rather than only
that a count went up. **Measured.**

### What the descriptor does — measured, not inferred

On a freshly bootstrapped `aic-idp`, `verify-rotate` fetched the descriptor
before, after add, and after remove, and counted
`<KeyDescriptor use="signing">`:

```text
signing KeyDescriptors: 1 -> 2 -> 1
```

**Measured 2026-09-16**, Keycloak 26.4.7, three independent runs (manual
add/delete while exploring; `verify-rotate` on a dirty volume; `verify-rotate`
on a `reset`). Same counts each time. `encryption` stayed 0. Total
`KeyDescriptor` count equalled the signing count.

During the two-key window, both descriptors have `use="signing"`. The new key
(higher priority) is listed **first** and is `active.RS256` on `GET /keys`. The
old key stays `status: "ACTIVE"` so verifiers that still have it can check
signatures it produced. **Measured.**

After `rotate-rm` the remaining `KeyName` is the **new** kid, not the original.
A completed rotation replaces the signing cert in metadata, it does not revert
to the old one. **Measured.**

### Why the count sequence is not the assertion

`1 -> 2 -> 1` is necessary and nowhere near sufficient. `rotate-add` inserts at
the **highest** priority; `rotate-rm` deletes the **lowest**. So a `rotate-rm`
that deleted the _new_ provider instead of the old one is a no-op rollback that
produces exactly the same three numbers. A harness asserting on counts alone
would certify a completed rotation having rotated nothing — which is the one
failure it exists to catch.

`verify-rotate` therefore asserts **identity**, in both directions:

- the signing key left standing must **be** the one `rotate-add` published;
- no key that predated the rotation may still be published.

Counts stay, as a secondary assertion.

Keys are compared by the **SHA-256 of the DER** decoded from
`<ds:X509Certificate>`, not by `<ds:KeyName>`. Keycloak publishes a `KeyName`;
AIC publishes none at all (`docs/api/06-saml.md`), so a `KeyName` comparison
would be vacuous against the peer this harness exists to test. `KeyName` is the
fallback for a `KeyDescriptor` with no certificate, and a key with neither makes
`verify-rotate` refuse rather than match unknown against unknown.

The guard has been seen to fail. `rotate-rm` was inverted to delete the
highest-priority provider (`min_by` → `max_by`) and `verify-rotate` re-run
against the same live realm:

```text
signing KeyDescriptors: 1 -> 2 -> 1
verify-rotate aic-idp: rotate-rm removed the key rotate-add published
  (IDPSSODescriptor/<sha256-of-new-cert>); the descriptor is back to its
  pre-rotation certificate and nothing rotated
```

The count line is unchanged — that is the point. **Measured 2026-09-17**,
Keycloak 26.4.7; the unmodified script passed against the same realm immediately
before and after, so the red came from the mutation and not from the state.

### Counts are per role descriptor

`status` and `verify-rotate` count `KeyDescriptor`s **within each role
descriptor** and never across the document, and key identities are
role-qualified for the same reason. The distinction is invisible on Keycloak — a
realm publishes one `IDPSSODescriptor`, a broker alias one `SPSSODescriptor` —
but an AIC entity can hold the `identityProvider` and `serviceProvider` roles at
once, and a whole-document count would silently add the two roles' keys
together. `status` prints one line per role:

```text
  KeyDescriptor IDPSSODescriptor: total=1 signing=1 encryption=0 unspecified=0
  IDPSSODescriptor  <sha256-of-cert>  KeyName=<kid>
```

A `KeyDescriptor` sitting outside every role descriptor is reported in its own
`(entity)` bucket rather than folded into a neighbouring role, so a malformed
document cannot inflate a real role's count.

This is the fixture for the rollover question: a document with two signing
`KeyDescriptor`s, on demand, and one with a single new one after it. What that
fixture is _not_ for is asking AIC to update a stored peer — `importEntity` is
create-only and there is no update path (**attributed**, `docs/api/06-saml.md`).
What it can still establish is whether a peer keeps verifying across the window,
and what `aic saml` would have to do to refresh a peer without destroying its
CoT membership. Neither is **measured here**.

### Rotating the other side: AIC's certificate is an ESV secret version

**Attributed** throughout to `docs/api/06-saml.md`, which measured this end to
end against a sandbox tenant, plus `03-esvs.md` and `15-secret-mappings.md`. The
harness plays no part in these steps; it is the peer that has to notice.

The AIC entity holds a _label_, not a key, so there is no certificate to upload
and no metadata to push. The rotation is:

1. **`PUT` the entity with a `secretIdIdentifier`** (any string; it namespaces
   the labels). That creates
   `am.applications.federation.entity.providers.saml2.<id>.{signing,encryption,mtls}`
   in the realm's secret-label enum and **changes the exported metadata not at
   all** — the labels are unmapped, so resolution still falls through to the
   realm default. Entity `PUT` is a full replace. No CLI verb; REST.

2. **Create an ESV secret holding the private key PEM and the certificate PEM
   concatenated.**

   ```bash
   cat key.pem cert.pem > pair.pem
   aic esv secret create esv-saml-sp-a-signing \
     --encoding pem --no-placeholders --value-file pair.pem --no-prompt
   ```

   `--no-placeholders` is the restart-free class: the secret reads
   `loaded: true` immediately. With placeholders on it reads `loaded: false`,
   and you have bought a tenant restart.

3. **Map the label at the secret.**

   ```bash
   aic secretmap set \
     am.applications.federation.entity.providers.saml2.<id>.signing \
     esv-saml-sp-a-signing --realm <realm> --no-prompt
   ```

4. **Re-export and check the fingerprint.**

   ```bash
   aic saml metadata export "https://sp-a.example.com" \
     --realm <realm> --out aic-sp.xml --no-prompt
   aic saml metadata inspect aic-sp.xml --no-prompt
   ```

   One `<KeyDescriptor use="signing">`, carrying the ESV secret's certificate,
   within single-digit seconds and with no restart. Mapping the label
   **replaces** the tenant default; it does not add to it.

Then hand the re-exported metadata to the peer again — `register-sp` for
direction A, `register-idp` for direction B. Neither is a rotation-specific
path: the peer simply re-reads metadata.

Three cleanup traps, all attributed and all of them things a future
`aic saml rotate` has to handle: changing `secretIdIdentifier` does **not**
remove the old mapping; deleting the entity does not either; and an orphaned
mapping cannot be removed with `aic secretmap remove`, because the CLI validates
the label against an enum the delete has just emptied. Delete the mapping
_before_ the identifier changes or the entity goes.

### The two-key window, side by side

|                            | Keycloak (`harness.sh`, measured)                 | AIC (`docs/api/06-saml.md`, attributed)                     |
| -------------------------- | ------------------------------------------------- | ----------------------------------------------------------- |
| What holds the key         | a realm `rsa-generated` key provider              | a **version** of an ESV secret, behind a secret-store label |
| Publish a second           | `rotate-add <realm>` (a provider at priority+100) | `aic esv secret add-version <id>`                           |
| Order in metadata          | new (highest priority) first                      | newest ENABLED version first                                |
| Retire the old             | `rotate-rm <realm>` (deletes the lowest priority) | `aic esv secret disable <id> <v>`, then `destroy`           |
| When the change is live    | the next descriptor fetch                         | single-digit seconds, no restart                            |
| `<ds:KeyName>` published   | yes — the Keycloak kid                            | **no** — nothing but the certificate                        |
| Encryption `KeyDescriptor` | none at all                                       | present, on its own label, untouched by a signing roll      |

So both sides publish exactly two `<KeyDescriptor use="signing">` during the
window and one either side of it, and on both sides the newest is listed first.
The row that matters to a tool is the second from last: against Keycloak you
_may_ compare `KeyName`s, against AIC you have nothing but the certificate.
Compare SHA-256 of the DER in both directions and one implementation covers both
— which is what `key_report` does.

A rollover is therefore the same five moves on either side: publish the second
certificate → let the peer re-read metadata → confirm the peer now holds both →
retire the first → confirm the peer holds only the new one. `verify-rotate`
automates exactly that on the Keycloak side, for the descriptor rather than for
a peer.

### Which certificate actually signs is unproven

During a two-certificate window, **which of the two does the signer use?**

`docs/api/06-saml.md` is explicit that this is not established on the AIC side:
the metadata lists the active ESV secret version first, and ordering is
suggestive, not proof. The same gap exists on the Keycloak side of this file —
the highest-priority provider is `active.RS256` on `GET /keys`, which is not the
same statement as "this is the key that signed that assertion".

Settling it needs a live federation and a real `<ds:Signature>`, which is
exactly what a wired direction gives you. The harness can do it — vary one thing
at a time, and run the controls:

1. **Positive control.** Wire **direction B** (AIC is the IdP) with a single
   ENABLED ESV secret version carrying certificate **A**, and get one successful
   login. Read `<ds:Signature>/<ds:KeyInfo>/<ds:X509Certificate>` off the
   assertion Keycloak received and fingerprint it: it must be A. That proves the
   setup can see which certificate signed at all — without it, every later
   answer is a coin flip.
2. `aic esv secret add-version` with certificate **B**. The export now carries
   two signing `KeyDescriptor`s, B first. Note both fingerprints with
   `aic saml metadata inspect`.
3. `harness.sh register-idp aic-idp.xml` so Keycloak holds the two-certificate
   metadata.
4. **The measurement.** Log in again, fresh, and fingerprint the signing
   certificate. A or B is the answer.
5. **The staleness control.** `aic esv secret disable <id> 1` — the _old_
   version; the latest cannot be disabled (`docs/api/03-esvs.md`) — so the
   export drops back to B alone. Re-register and log in once more. If step 4
   said A and this says B, step 4's answer was real. If this also says A,
   something is caching a key and neither run measured what it looked like it
   measured.

Note what the experiment cannot separate: the newest ENABLED version is _both_
the active version and the first `KeyDescriptor` in the document, so a result of
"B" does not distinguish "AM signs with the active version" from "AM signs with
whatever is listed first". Only a result of **A** discriminates — and that is
the answer worth knowing, because it is the one that breaks a rollover.

Whether Keycloak _accepts_ two-certificate metadata at all is a separate
unexercised question — it has no `KeyName` to pick by, so it has to try both.

## Things that will trip AIC's importer

These are Keycloak facts. Whether AIC actually trips is for the other agent; do
not treat this list as a verified AIC bug list.

1. **Fat descriptor.** Artifact, SOAP, HTTP-Artifact, four NameID formats,
   `WantAuthnRequestsSigned`. Same shape of problem as Entra's
   `federationmetadata.xml` (the task's third motivation). The strip helper this
   predicted now exists — `aic saml metadata sanitise` — and on Keycloak's
   descriptor it is a **no-op**: `inspect` reports `would_remove: []`, because
   what it cuts by default is a WS-Federation `RoleDescriptor` and an enveloped
   signature, and Keycloak emits neither. **Measured 2026-09-17.** Whether AIC's
   importer minds the extras nothing strips is still untested.
2. **`WantAuthnRequestsSigned="true"` is sticky.** Not obviously configurable on
   the realm. If AIC honours it, AIC must sign `AuthnRequest`s, or the IdP will
   reject them at runtime even if import succeeded. Runtime of that rejection is
   **inferred**; the flag on the XML is **measured**.
3. **Default SAML client requires signed requests and uses username NameID.**
   The harness overrides both. `register-sp` from AIC metadata will restore
   whatever the converter produced (`saml.client.signature` follows
   `AuthnRequestsSigned` on the SPSSODescriptor). **Measured** on the converter
   output of Keycloak's own SP metadata (`saml.client.signature: "true"`).
4. **`entityId` vs `idpEntityId`.** Getting this wrong on the broker makes
   Keycloak publish the IdP's entity ID as its own. AIC would then see a remote
   SP whose entity ID equals the hosted IdP. **Measured.**
5. **First `NameIDFormat` wins on import-config.** Persistent, not email.
6. **You cannot re-import a peer's metadata on AIC at all.** `importEntity` is
   create-only — a second import of the same entity id is a 500 — and the
   `DELETE`-then-import workaround both rewrites extended metadata (dropping a
   `cotlist` added afterwards) and removes the entity from every CoT document
   that listed it. **Attributed** (`docs/api/06-saml.md`). This file previously
   said only that re-import "drops `cotlist`", which understated it: there is no
   supported update path for a remote entity's metadata.
7. **No encryption `KeyDescriptor`.** An importer that requires one will not
   find it. **Measured** absence.
8. **Client GET contains a private key** if `saml.client.signature` was true at
   create. Do not paste that JSON into docs or commits. Gitleaks will fire.
   **Measured.**

## Measured, attributed, inferred

Keycloak-side rows were measured here. AIC-side rows were measured elsewhere and
are cited; this document made none of those calls.

| Claim                                                                                         | Status                                  |
| --------------------------------------------------------------------------------------------- | --------------------------------------- |
| Two realms, two entity IDs, IdP descriptor vs broker SP descriptor                            | measured                                |
| Default descriptor has 1 signing `KeyDescriptor`, 0 encryption                                | measured                                |
| Rotation 1 → 2 → 1 signing `KeyDescriptor`s; new key listed first                             | measured                                |
| Inverting `rotate-rm` leaves that count sequence unchanged (so counts cannot discriminate)    | measured                                |
| `WantAuthnRequestsSigned="true"` with `saml.client.signature=false` on the only SAML client   | measured                                |
| `entityId` is the SP; `idpEntityId` is the remote IdP                                         | measured                                |
| `import-config` takes the first `NameIDFormat` (persistent)                                   | measured                                |
| `import-config` from multipart file 200; fromUrl 500; raw XML 415                             | measured                                |
| Converter accepts SPSSODescriptor (JSON content-type, XML body), 500 on IDPSSODescriptor      | measured                                |
| `saml.client.signature=false` on create does not mint a client private key                    | measured                                |
| Component `config` values are string arrays; `parentId` is the realm UUID                     | measured                                |
| POST create 201 empty body; PUT 204; DELETE 204                                               | measured                                |
| Image `26.4.7`, ready in 14s on a cold volume, port 18080                                     | measured                                |
| `harness.sh status` and `aic saml metadata inspect` report the same SHA-256 for one key       | measured 2026-09-17                     |
| Keycloak's realm descriptor has no enveloped signature and no WS-Fed `RoleDescriptor`         | measured 2026-09-17                     |
| The un-imported broker SP descriptor is `AuthnRequestsSigned="false"` with no `KeyDescriptor` | measured 2026-09-17                     |
| AIC's signing certificate is an ESV secret version behind a secret-store label                | attributed (`06-saml.md`)               |
| The published certificate set is the ESV secret's ENABLED versions, newest first              | attributed (`06-saml.md`)               |
| `--no-placeholders` makes the rotation restart-free                                           | attributed (`06-saml.md`, `03-esvs.md`) |
| AIC emits no `<ds:KeyName>`                                                                   | attributed (`06-saml.md`)               |
| `exportmetadata.jsp` takes no bearer; a failure is HTTP 200 with an `ERROR :` body            | attributed (`06-saml.md`)               |
| `importEntity` is create-only, base64url only, and ignores `cot`                              | attributed (`06-saml.md`)               |
| Deleting an entity edits every CoT that listed it                                             | attributed (`06-saml.md`)               |
| CoT membership lives in two places and the runtime reads the one REST hides                   | attributed (`06-saml.md`)               |
| Priority decides which Keycloak key **signs** (as opposed to which is `active`)               | inferred                                |
| Fat metadata will be rejected by AIC the way Entra's is                                       | inferred                                |
| Which published certificate AM signs with during a rollover                                   | **unproven**                            |
| Whether a federation authenticates after a REST-only CoT membership change                    | **unproven**                            |
| Completing a browser SAML login either direction                                              | not exercised                           |
| First-broker-login prompting / account linking                                                | not exercised                           |
| nixpkgs `keycloak-26.7.3` as a substitute for the container                                   | not exercised                           |

## Divergences from the task prompt

The prompt asked to use the truth and say so.

- **`docker compose` is present** (v5.1.4). `docker-compose` (hyphen) and
  `podman` are not. The harness still uses `docker run` so it does not add a
  compose file.
- **Image tag `26.4` is not published.** Ran `26.4.7`.
- **Port 8080 is not free** on this machine. Default is 18080.
- **Two-realm split is correct**; not pushed back.
- **Keycloak is the right peer for both directions**, with the caveat that
  "Keycloak as SP" means identity brokering, not a SAML client. A SAML client in
  Keycloak is an SP that Keycloak-as-IdP serves — the other direction.

## Still unproven

Two of these are load-bearing — a tool built on either assumption would be
confidently wrong — and the harness can settle both.

- **Does a SAML federation actually authenticate after a REST-only change to
  circle-of-trust membership?** **Nobody has read a `cotlist`.** REST never
  exposes it, and the evidence that a CoT `PUT` drives the entity side is
  indirect: the write fails in exactly the places a `cotlist`-updating write
  would fail, and deleting an entity cascades into the CoT documents
  (`docs/api/06-saml.md`, which carries this as an open question). The
  end-to-end proof is one fresh login in either direction above, with membership
  made purely over REST, and the gate-3 `LIBCOT` / `COTCache` lines present in
  the **ACS** transaction. Until that run happens, no surface may present
  `trustedProviders` as "the" membership.
- **Which of the two published certificates does AM sign with during a
  rollover?** Metadata order is suggestive, not proof, on either side. The
  procedure — including the discriminating case — is in
  [which certificate actually signs](#which-certificate-actually-signs-is-unproven).
- **Does AIC's `importEntity` accept a Keycloak descriptor as-is?** Our own
  `sanitise` finds nothing to strip in it (**measured**), which is not the same
  as AIC accepting the extras: no Keycloak descriptor has been imported into a
  tenant.
- **Does AIC sign `AuthnRequest`s?** If not, either Keycloak's
  `WantAuthnRequestsSigned` must be ignored at runtime (the client-level
  `saml.client.signature=false` already is), or AIC must be told to sign.
- **Does Keycloak accept two-certificate metadata from AIC?** It has no
  `KeyName` to pick by, so it has to try both. Not exercised.
- **Has anyone completed a browser SAML login in either direction?** No. Step 7
  of each procedure above is assembled from the two halves and the cited docs,
  not from a login that happened. Treat it as the least-tested part of this
  file.

Hosted-IdP and hosted-SP shapes on AIC are no longer open: `docs/api/06-saml.md`
now carries full reads of both and the create procedure. That bullet used to say
otherwise and was stale.
