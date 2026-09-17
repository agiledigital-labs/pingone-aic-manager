# SAML test harness

Local Keycloak peer for the `aic saml` CLI. Two realms:

- `aic-idp` — Keycloak as SAML IdP (AIC-as-SP)
- `aic-sp` — Keycloak as SAML SP via identity brokering (AIC-as-IdP)

```bash
scripts/saml-harness/harness.sh up
scripts/saml-harness/harness.sh status
scripts/saml-harness/harness.sh metadata aic-idp
scripts/saml-harness/harness.sh verify-rotate aic-idp
scripts/saml-harness/harness.sh down     # keeps the volume
scripts/saml-harness/harness.sh reset    # wipes the volume
```

`verify-rotate` adds a signing key, removes the old one, and asserts the
surviving `KeyDescriptor` **is** the certificate it just published and **is
not** a pre-rotation one. Counts cannot tell those apart: deleting either key
leaves the same `1 -> 2 -> 1` sequence. Keys are compared by the SHA-256 of the
DER out of `<ds:X509Certificate>`, because AIC publishes no `<ds:KeyName>` to
compare instead.

Rotation spans two products, and they work nothing alike. Keycloak's signing key
is a realm `rsa-generated` key provider, rotated here. **AIC's is a _version_ of
an ESV secret behind an AM secret-store label** — not a metadata upload, not a
field on the SAML entity. The paired procedure for both is in the knowledge
base.

Admin console: <http://localhost:18080/admin> (`admin` / `admin`, local-dev).

This directory never talks to an AIC tenant. Metadata exchange is file/paste.

The knowledge base — measured Admin REST shapes, a sanitised descriptor, the
two-sided rotation procedure, end-to-end recipes for running a federation in
either direction, and what is still unproven — is
[docs/saml-test-harness.md](../../docs/saml-test-harness.md).

`scripts/shellcheck-all.sh` lints this script and runs in CI.
