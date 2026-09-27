//! An entity's exported metadata: reading it as metadata, and waiting for it
//! to show a write.
//!
//! Shared by `rotate` and `cert`, the two verticals that prove a write by
//! re-exporting. The export is the only place either change is visible — a
//! remote entity's certificates are not in its JSON, and a hosted one's come
//! from the secret store — so both verbs need the same two things: a read
//! that never mistakes the JSP's HTTP-200 error page for an empty entity, and
//! a bounded poll.

use std::time::Duration;

use crate::config::tenant::Tenant;
use crate::saml::spec::{self, ExportOutcome};
use crate::saml::{api, metadata};
use crate::{Error, Result};

/// How long to wait for the export to show a change, and how often to look.
#[derive(Debug, Clone, Copy)]
pub struct Settle {
    pub timeout: Duration,
    pub interval: Duration,
}

/// Every change measured on the sandbox appeared within **single-digit
/// seconds** (`docs/api/06-saml.md`) — a certificate update on the first read
/// after it — so this is generous rather than hopeful. A timeout is reported,
/// never treated as a failure of the write: the write landed either way, and
/// telling an operator it failed is how it gets sent twice.
pub const SETTLE: Settle = Settle {
    timeout: Duration::from_secs(45),
    interval: Duration::from_secs(3),
};

/// The entity's exported metadata, classified.
///
/// **The export endpoint answers 200 for failure too**, so the body is
/// classified before anything parses it — otherwise `ERROR : No metadata for
/// entity …` would fingerprint to nothing and read as "this entity publishes
/// no certificates", which is the same shape as a successful roleless export.
pub async fn read(tenant: &Tenant, realm: &str, entity_id: &str) -> Result<Vec<u8>> {
    let body = api::export_metadata(tenant, realm, entity_id).await?;
    match spec::classify_export(body.as_bytes()) {
        ExportOutcome::Metadata => Ok(body.into_bytes()),
        ExportOutcome::TenantError(message) => Err(Error::Config(format!(
            "tenant {} refused to export {entity_id:?} from realm {realm}, so what it publishes \
             is unknown: {message}",
            tenant.name
        ))),
        ExportOutcome::Unrecognised(excerpt) => Err(Error::Config(format!(
            "tenant {} answered the metadata export for {entity_id:?} in realm {realm} with \
             something that is not SAML metadata: {excerpt}",
            tenant.name
        ))),
    }
}

/// Every certificate the export publishes.
pub async fn cert_refs(
    tenant: &Tenant,
    realm: &str,
    entity_id: &str,
) -> Result<Vec<metadata::CertRef>> {
    Ok(metadata::cert_refs(&read(tenant, realm, entity_id).await?)?)
}

/// Call `read` until `settled` accepts what it returns or `settle.timeout`
/// passes, and return the last value read — which on a timeout is what the
/// caller has to report, not an error of the write that preceded it. A read
/// that fails ends the poll with that error.
pub async fn poll<T>(
    settle: Settle,
    mut read: impl AsyncFnMut() -> Result<T>,
    settled: impl Fn(&T) -> bool,
) -> Result<T> {
    let deadline = std::time::Instant::now() + settle.timeout;
    loop {
        let seen = read().await?;
        if settled(&seen) || std::time::Instant::now() >= deadline {
            return Ok(seen);
        }
        tokio::time::sleep(settle.interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUICK: Settle = Settle {
        timeout: Duration::from_millis(50),
        interval: Duration::from_millis(1),
    };

    #[tokio::test]
    async fn the_poll_stops_at_the_first_settled_read_and_returns_the_last_on_timeout() {
        let mut reads = 0;
        let seen = poll(
            QUICK,
            async || {
                reads += 1;
                Ok(reads)
            },
            |value| *value == 3,
        )
        .await
        .unwrap();
        assert_eq!((seen, reads), (3, 3));

        let last = poll(QUICK, async || Ok(0), |_| false).await.unwrap();
        assert_eq!(last, 0, "a timeout returns what was read, not an error");

        let error = poll(
            QUICK,
            async || -> Result<u8> { Err(Error::Config("export unreadable".into())) },
            |_| true,
        )
        .await;
        assert!(error.is_err(), "a failed read ends the poll");
    }
}
