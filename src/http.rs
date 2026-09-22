//! Shared HTTP client defaults for requests sent by `aic`.

/// Stable product identifier plus the running binary's package version.
pub(crate) const USER_AGENT: &str = concat!("aic/", env!("CARGO_PKG_VERSION"));

/// Start a reqwest client builder with the defaults common to every `aic`
/// transport. Callers may add transport-specific settings before building it.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(USER_AGENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_identifies_aic_and_its_version() {
        assert_eq!(USER_AGENT, format!("aic/{}", env!("CARGO_PKG_VERSION")));
        client_builder().build().unwrap();
    }
}
