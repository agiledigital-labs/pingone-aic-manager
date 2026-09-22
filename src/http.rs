//! Shared HTTP client defaults for requests sent by `aic`.

/// Stable product identifier plus the running binary's package version.
pub(crate) const USER_AGENT: &str = concat!("aic/", env!("CARGO_PKG_VERSION"));
pub(crate) const TRANSACTION_ID_HEADER: &str = "x-forgerock-transactionid";

/// Start a reqwest client builder with the defaults common to every `aic`
/// transport. Callers may add transport-specific settings before building it.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(USER_AGENT)
}

/// Add a fresh, recognisably `aic` transaction id to one outbound request.
pub(crate) trait RequestBuilderExt {
    fn aic_transaction_id(self) -> Self;
}

impl RequestBuilderExt for reqwest::RequestBuilder {
    fn aic_transaction_id(self) -> Self {
        self.header(
            TRANSACTION_ID_HEADER,
            format!("aic-{}", uuid::Uuid::new_v4()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_identifies_aic_and_its_version() {
        assert_eq!(USER_AGENT, format!("aic/{}", env!("CARGO_PKG_VERSION")));
        client_builder().build().unwrap();
    }

    #[test]
    fn transaction_ids_identify_aic_and_are_unique_per_request() {
        let client = client_builder().build().unwrap();
        let build = || {
            client
                .get("https://tenant.example/resource")
                .aic_transaction_id()
                .build()
                .unwrap()
        };
        let first = build();
        let second = build();
        let value = first.headers()[TRANSACTION_ID_HEADER].to_str().unwrap();

        assert_ne!(
            first.headers()[TRANSACTION_ID_HEADER],
            second.headers()[TRANSACTION_ID_HEADER]
        );
        assert_eq!(value.len(), "aic-".len() + uuid::fmt::Hyphenated::LENGTH);
        assert!(uuid::Uuid::parse_str(value.trim_start_matches("aic-")).is_ok());
    }
}
