# Keycloak SAML test harness

A local Keycloak peer for the forthcoming `aic saml` CLI. It exercises SAML in
both directions (AIC-as-SP and AIC-as-IdP) and can rotate signing certificates
so we can watch AIC's metadata import either **replace** or **add** a cert — the
question the CLI has to answer.

This file is the knowledge base. Claims are marked **measured** (a live call
against the container this slice brought up) or **inferred**. An inferred claim
stated as measured is the worst outcome of this slice.

It never talks to an AIC tenant. Another agent is doing that concurrently.

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
`status: "ACTIVE"` — "active" here means usable for verification; the priority
picks which one **signs**. **Measured.**

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
| `KeyDescriptor use="signing"`                   | The cert AIC must trust. `KeyName` is the Keycloak kid.                 | After rotation there are two of these. Whether AIC **replaces** or **adds** is the question this harness exists to answer.                                                                                                                                                        |
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

`rotate-rm` refuses if only one `rsa-generated` provider remains.

### What the descriptor does — counted, not inferred

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

This is the fixture `aic saml` will use to ask AIC: after you upload metadata
that now has two signing `KeyDescriptor`s, do you replace the stored cert or add
a second? Then after metadata shrinks to one, do you drop the old cert? That AIC
behaviour is **not measured here**.

## Things that will trip AIC's importer

These are Keycloak facts. Whether AIC actually trips is for the other agent; do
not treat this list as a verified AIC bug list.

1. **Fat descriptor.** Artifact, SOAP, HTTP-Artifact, four NameID formats,
   `WantAuthnRequestsSigned`. Same shape of problem as Entra's
   `federationmetadata.xml` (the task's third motivation). **Inferred** that a
   strip-before-import helper will be part of `aic saml`, the way the Entra
   import already is by hand.
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
6. **Re-importing metadata on AIC drops `cotlist`.** That is an AIC fact from
   `docs/api/06-saml.md`, not a Keycloak one. Rotation tests that re-import AIC
   entities will hit it. **Not re-measured here.**
7. **No encryption `KeyDescriptor`.** An importer that requires one will not
   find it. **Measured** absence.
8. **Client GET contains a private key** if `saml.client.signature` was true at
   create. Do not paste that JSON into docs or commits. Gitleaks will fire.
   **Measured.**

## Measured vs inferred

| Claim                                                                                       | Status             |
| ------------------------------------------------------------------------------------------- | ------------------ |
| Two realms, two entity IDs, IdP descriptor vs broker SP descriptor                          | measured           |
| Default descriptor has 1 signing `KeyDescriptor`, 0 encryption                              | measured           |
| Rotation 1 → 2 → 1 signing `KeyDescriptor`s; new key listed first                           | measured           |
| Inverting `rotate-rm` leaves that count sequence unchanged (so counts cannot discriminate)  | measured           |
| `WantAuthnRequestsSigned="true"` with `saml.client.signature=false` on the only SAML client | measured           |
| `entityId` is the SP; `idpEntityId` is the remote IdP                                       | measured           |
| `import-config` takes the first `NameIDFormat` (persistent)                                 | measured           |
| `import-config` from multipart file 200; fromUrl 500; raw XML 415                           | measured           |
| Converter accepts SPSSODescriptor (JSON content-type, XML body), 500 on IDPSSODescriptor    | measured           |
| `saml.client.signature=false` on create does not mint a client private key                  | measured           |
| Component `config` values are string arrays; `parentId` is the realm UUID                   | measured           |
| POST create 201 empty body; PUT 204; DELETE 204                                             | measured           |
| Image `26.4.7`, ready in 14s on a cold volume, port 18080                                   | measured           |
| Fat metadata will be rejected by AIC the way Entra's is                                     | inferred           |
| AIC metadata-upload-for-cert-update replaces vs adds                                        | **not this slice** |
| Completing a browser SAML login either direction                                            | not exercised      |
| First-broker-login prompting / account linking                                              | not exercised      |
| nixpkgs `keycloak-26.7.3` as a substitute for the container                                 | not exercised      |

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

## Open questions this slice does not settle

- Does AIC's `importEntity` accept this descriptor as-is, or does it need the
  same kind of strip as Entra? That is the other agent's live AIC verification,
  plus the first thing `aic saml` should measure.
- When AIC is given metadata with two signing `KeyDescriptor`s, does the stored
  cert list grow or get replaced? Same for shrinking back to one. The harness
  can now produce those two documents on demand.
- Does AIC sign `AuthnRequest`s? If not, either Keycloak's
  `WantAuthnRequestsSigned` must be ignored at runtime (client-level
  `saml.client.signature=false` already is), or AIC must be told to sign.
- Hosted-IdP and hosted-SP shapes on AIC are still "not yet exercised" in
  `docs/api/06-saml.md`. This harness does not change that.
