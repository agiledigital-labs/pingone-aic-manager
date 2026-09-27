//! The only module in `saml::cert` that talks to a tenant.
//!
//! Two reads and one write. The reads are the realm's entity list (is it
//! remote, which roles) and the unauthenticated metadata export (what it
//! publishes); the write is `api::update_certificates`. What this module owns
//! is the order: re-read immediately before the write and refuse on any
//! difference, send, then read the export again and compare the exact
//! `(role, use, sha256)` set — `.ai/core.md` §5's confirm-from-the-read.

use std::collections::BTreeSet;
use std::time::Duration;

use crate::config::tenant::Tenant;
use crate::saml::cert::spec::{self, CertKey, CertPermit, CertPlan, Target};
use crate::saml::rotate::spec::WriteFailure;
use crate::saml::spec::ExportOutcome;
use crate::saml::{api, spec as saml_spec};
use crate::{Error, Result};

/// How long to wait for the export to show the update, and how often to
/// look. Every update measured appeared on the first read afterwards
/// (`docs/api/06-saml.md`); the wait is for a node that has not caught up.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);
const SETTLE_INTERVAL: Duration = Duration::from_secs(2);

/// The entity's exported metadata, classified. The JSP answers failure with
/// HTTP 200, so the classification is the only thing that makes this
/// metadata.
pub async fn read_export(tenant: &Tenant, realm: &str, entity_id: &str) -> Result<Vec<u8>> {
    let body = api::export_metadata(tenant, realm, entity_id).await?;
    match saml_spec::classify_export(body.as_bytes()) {
        ExportOutcome::Metadata => Ok(body.into_bytes()),
        ExportOutcome::TenantError(message) => Err(Error::Config(format!(
            "tenant {} refused to export {entity_id:?} from realm {realm}: {message}",
            tenant.name
        ))),
        ExportOutcome::Unrecognised(excerpt) => Err(Error::Config(format!(
            "tenant {} answered the metadata export for {entity_id:?} in realm {realm} with \
             something that is not SAML metadata: {excerpt}",
            tenant.name
        ))),
    }
}

/// The entity, confirmed remote from the realm's list.
pub async fn read_target(tenant: &str, realm: &str, entity_id: &str) -> Result<Target> {
    let stubs = api::list(tenant, realm).await?;
    spec::target_ok(entity_id, &stubs, realm)
}

/// Recheck, send, confirm.
///
/// Every failure says which side of the send it is on ([`WriteFailure`] has
/// no `From<Error>` for that reason): a recheck refusal was never sent, a
/// failed send is unknown unless proven otherwise, and a failed or
/// mismatching read afterwards is a write that **landed** unverified.
pub async fn apply(
    tenant: &Tenant,
    plan: &CertPlan,
    confirmed_prod: bool,
    permit: &CertPermit,
) -> std::result::Result<BTreeSet<CertKey>, WriteFailure> {
    let fresh_target = read_target(&tenant.name, &plan.realm, &plan.entity_id)
        .await
        .map_err(WriteFailure::before_send)?;
    let fresh_export = read_export(tenant, &plan.realm, &plan.entity_id)
        .await
        .map_err(WriteFailure::before_send)?;
    spec::recheck_ok(plan, &fresh_target, &fresh_export).map_err(WriteFailure::before_send)?;

    // No measured refusal applies: every failure of this endpoint seen so far
    // is the same opaque 500, which cannot be told from a write that landed.
    api::update_certificates(
        &tenant.name,
        &plan.realm,
        &plan.document,
        confirmed_prod,
        permit,
    )
    .await
    .map_err(|error| WriteFailure::from_send(error, &[]))?;

    let deadline = std::time::Instant::now() + SETTLE_TIMEOUT;
    loop {
        let export = read_export(tenant, &plan.realm, &plan.entity_id)
            .await
            .map_err(WriteFailure::unverified)?;
        let seen = spec::published(&export).map_err(WriteFailure::unverified)?;
        if spec::settled(&seen, &plan.expected) {
            return Ok(seen);
        }
        if std::time::Instant::now() >= deadline {
            return Err(WriteFailure::unverified(Error::Config(format!(
                "after {}s the export of {} does not publish the planned certificates — {}",
                SETTLE_TIMEOUT.as_secs(),
                plan.entity_id,
                spec::settlement_gap(&seen, &plan.expected)
            ))));
        }
        tokio::time::sleep(SETTLE_INTERVAL).await;
    }
}
