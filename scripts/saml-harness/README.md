# SAML test harness

Local Keycloak peer for the forthcoming `aic saml` CLI. Two realms:

- `aic-idp` — Keycloak as SAML IdP (AIC-as-SP)
- `aic-sp` — Keycloak as SAML SP via identity brokering (AIC-as-IdP)

```bash
scripts/saml-harness/harness.sh up
scripts/saml-harness/harness.sh status
scripts/saml-harness/harness.sh metadata aic-idp
scripts/saml-harness/harness.sh down     # keeps the volume
scripts/saml-harness/harness.sh reset    # wipes the volume
```

Admin console: <http://localhost:18080/admin> (`admin` / `admin`, local-dev).

This directory never talks to an AIC tenant. Metadata exchange is file/paste.

The knowledge base — measured Admin REST shapes, a sanitised descriptor, key
rotation, and what will trip AIC's importer — is
[docs/saml-test-harness.md](../../docs/saml-test-harness.md).
