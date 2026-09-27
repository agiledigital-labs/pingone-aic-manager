//! The only module in `saml::cert` that talks to a tenant.
//!
//! Two reads and one write. The reads are the realm's entity list (is it
//! remote, which roles) and the unauthenticated metadata export (what it
//! publishes, via [`crate::saml::export`]); the write is
//! `api::update_certificates`. What this module owns is the order: re-read
//! immediately before the write and refuse on any difference, send, then read
//! the export again and compare the exact `(role, use, sha256)` set —
//! `.ai/core.md` §5's confirm-from-the-read. [`apply_with`] is that order
//! with the three calls injected, so a test drives the real sequence.

use std::collections::BTreeSet;

use crate::config::tenant::Tenant;
use crate::saml::api;
use crate::saml::cert::spec::{self, CertKey, CertPermit, CertPlan, Target};
use crate::saml::export::{self, Settle};
use crate::saml::write::WriteFailure;
use crate::{Error, Result};

/// The entity's exported metadata, classified.
pub async fn read_export(tenant: &Tenant, realm: &str, entity_id: &str) -> Result<Vec<u8>> {
    export::read(tenant, realm, entity_id).await
}

/// The entity, confirmed remote from the realm's list.
pub async fn read_target(tenant: &str, realm: &str, entity_id: &str) -> Result<Target> {
    let stubs = api::list(tenant, realm).await?;
    spec::target_ok(entity_id, &stubs, realm)
}

/// Recheck, send, confirm — against the tenant.
pub async fn apply(
    tenant: &Tenant,
    plan: &CertPlan,
    confirmed_prod: bool,
    permit: &CertPermit,
) -> std::result::Result<BTreeSet<CertKey>, WriteFailure> {
    apply_with(
        plan,
        export::SETTLE,
        async || read_target(&tenant.name, &plan.realm, &plan.entity_id).await,
        async || read_export(tenant, &plan.realm, &plan.entity_id).await,
        async |document: &[u8]| {
            api::update_certificates(&tenant.name, &plan.realm, document, confirmed_prod, permit)
                .await
                .map(|_| ())
        },
    )
    .await
}

/// Recheck, send, confirm, with the list read, the export read and the write
/// injected.
///
/// Every failure says which side of the send it is on ([`WriteFailure`] has
/// no `From<Error>` for that reason): a recheck refusal was never sent, a
/// failed send is unknown unless proven otherwise, and a failed or
/// mismatching read afterwards is a write that **landed** unverified.
pub async fn apply_with(
    plan: &CertPlan,
    settle: Settle,
    mut read_target: impl AsyncFnMut() -> Result<Target>,
    mut read_export: impl AsyncFnMut() -> Result<Vec<u8>>,
    mut send: impl AsyncFnMut(&[u8]) -> Result<()>,
) -> std::result::Result<BTreeSet<CertKey>, WriteFailure> {
    let fresh_target = read_target().await.map_err(WriteFailure::before_send)?;
    let fresh_export = read_export().await.map_err(WriteFailure::before_send)?;
    spec::recheck_ok(plan, &fresh_target, &fresh_export).map_err(WriteFailure::before_send)?;

    // No measured refusal applies: every failure of this endpoint seen so far
    // is the same opaque 500, which cannot be told from a write that landed.
    send(&plan.document)
        .await
        .map_err(|error| WriteFailure::from_send(error, &[]))?;

    let seen = export::poll(
        settle,
        async || spec::published(&read_export().await?),
        |seen| spec::settled(seen, &plan.expected),
    )
    .await
    .map_err(WriteFailure::unverified)?;
    if spec::settled(&seen, &plan.expected) {
        return Ok(seen);
    }
    Err(WriteFailure::unverified(Error::Config(format!(
        "after {}s the export of {} does not publish the planned certificates — {}",
        settle.timeout.as_secs(),
        plan.entity_id,
        spec::settlement_gap(&seen, &plan.expected)
    ))))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Duration;

    use super::*;
    use crate::saml::spec::Role;
    use crate::saml::write::WriteStatus;

    const QUICK: Settle = Settle {
        timeout: Duration::from_millis(30),
        interval: Duration::from_millis(1),
    };
    const ENTITY: &str = "https://sp-a.example.com";

    /// The one certificate this test adds, its SHA-256 written out by hand
    /// (`openssl x509 -in src/saml/fixtures/cert-ec.crt -noout -fingerprint
    /// -sha256`), not computed by the scanner under test.
    const EC_PEM: &[u8] = include_bytes!("../fixtures/cert-ec.crt");
    const EC_SHA: &str = "7f3ce2e437673b639c20bc54bc9a152a248b36280a3a32f29a02a6d2066600d9";
    /// And the one it already publishes (`cert-rsa.der`).
    const RSA_SHA: &str = "353f92d3903677e8efd96420323c35de7f7961c088e9b6f2c1828fc3c3ba8447";

    /// A remote IdP as AM exports it, with the RSA fixture as its one signing
    /// certificate — written out literally, so the scanner has to find it.
    fn export() -> Vec<u8> {
        let rsa = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            include_bytes!("../fixtures/cert-rsa.der"),
        );
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\"\n    \
             entityID=\"{ENTITY}\">\n    \
             <IDPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\n        \
             <KeyDescriptor use=\"signing\">\n            \
             <KeyInfo xmlns=\"http://www.w3.org/2000/09/xmldsig#\"><X509Data><X509Certificate>{rsa}</X509Certificate></X509Data></KeyInfo>\n        \
             </KeyDescriptor>\n    \
             </IDPSSODescriptor>\n\
             </EntityDescriptor>\n"
        )
        .into_bytes()
    }

    fn target() -> Target {
        Target {
            entity_id: ENTITY.into(),
            realm: "alpha".into(),
            roles: vec![Role::Idp],
        }
    }

    fn plan() -> CertPlan {
        let cert = crate::saml::pem::read_certificate(EC_PEM).unwrap();
        match spec::plan_add(&target(), &export(), &cert.der, &cert.sha256, None).unwrap() {
            spec::Planned::Change(plan) => plan,
            spec::Planned::NoChange(sentence) => panic!("{sentence}"),
        }
    }

    /// What the fake tenant saw, in order.
    #[derive(Debug, Default)]
    struct Tenant {
        calls: Vec<&'static str>,
        stored: Vec<u8>,
        sent: Option<Vec<u8>>,
    }

    /// Drive the real sequence against a fake tenant whose export becomes
    /// whatever was sent, then assert the order and the published identity.
    async fn run(
        tenant: &RefCell<Tenant>,
        roles: Vec<Role>,
        send_result: Result<()>,
        land: bool,
    ) -> std::result::Result<BTreeSet<CertKey>, WriteFailure> {
        let plan = plan();
        let mut send_result = Some(send_result);
        apply_with(
            &plan,
            QUICK,
            async || {
                tenant.borrow_mut().calls.push("list");
                Ok(Target {
                    roles: roles.clone(),
                    ..target()
                })
            },
            async || {
                let mut state = tenant.borrow_mut();
                state.calls.push("export");
                Ok(state.stored.clone())
            },
            async |document: &[u8]| {
                let mut state = tenant.borrow_mut();
                state.calls.push("send");
                state.sent = Some(document.to_vec());
                if land {
                    state.stored = document.to_vec();
                }
                send_result.take().expect("one send")
            },
        )
        .await
    }

    fn fresh() -> RefCell<Tenant> {
        RefCell::new(Tenant {
            stored: export(),
            ..Tenant::default()
        })
    }

    #[tokio::test]
    async fn the_sequence_rechecks_sends_the_plan_and_confirms_from_a_fresh_export() {
        let tenant = fresh();
        let seen = run(&tenant, vec![Role::Idp], Ok(()), true).await.unwrap();
        let state = tenant.borrow();
        assert_eq!(state.calls, ["list", "export", "send", "export"]);
        assert_eq!(state.sent.as_deref(), Some(plan().document.as_slice()));
        let signing = seen
            .iter()
            .filter(|key| key.descriptor == "IDPSSODescriptor")
            .map(|key| (key.key_use.as_deref(), key.sha256.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            signing,
            [(Some("signing"), RSA_SHA), (Some("signing"), EC_SHA)]
        );
    }

    /// Discriminating for review finding 1: the export is byte-identical, so
    /// only the roles can refuse this — and the send must not happen.
    #[tokio::test]
    async fn a_role_the_list_no_longer_shows_refuses_before_the_send() {
        let tenant = fresh();
        let failure = run(&tenant, vec![Role::Sp], Ok(()), true)
            .await
            .unwrap_err();
        assert_eq!(failure.status, WriteStatus::Refused);
        assert_eq!(tenant.borrow().calls, ["list", "export"]);
        assert!(
            failure.source.to_string().contains("roles"),
            "{}",
            failure.source
        );
    }

    #[tokio::test]
    async fn an_export_that_moved_refuses_before_the_send() {
        let tenant = fresh();
        tenant.borrow_mut().stored.push(b'\n');
        let failure = run(&tenant, vec![Role::Idp], Ok(()), true)
            .await
            .unwrap_err();
        assert_eq!(failure.status, WriteStatus::Refused);
        assert!(tenant.borrow().sent.is_none());
    }

    #[tokio::test]
    async fn a_failed_send_is_unknown_and_an_unchanged_export_is_unverified() {
        let tenant = fresh();
        let failure = run(
            &tenant,
            vec![Role::Idp],
            Err(Error::Api {
                status: 500,
                body: "{}".into(),
            }),
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(failure.status, WriteStatus::Unknown);

        // A 200 whose effect never shows: the write landed as far as anyone
        // can say, and the export disagrees with the plan.
        let tenant = fresh();
        let failure = run(&tenant, vec![Role::Idp], Ok(()), false)
            .await
            .unwrap_err();
        assert_eq!(failure.status, WriteStatus::AcceptedUnverified);
        let message = failure.source.to_string();
        assert!(message.contains(EC_SHA), "{message}");
    }
}
