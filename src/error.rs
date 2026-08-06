use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Auth error: {0}")]
    Auth(String),

    #[error(
        "agent is locked and no terminal is available to prompt.\n  Run `aic session login` first, or pipe the password:\n  printf '%s\\n' \"$PASSWORD\" | aic session login --password-stdin"
    )]
    AuthRequired,

    #[error("no {kind} stored for tenant {tenant}")]
    SecretMissing { kind: String, tenant: String },

    #[error("AIC API error: {status} {body}")]
    Api { status: u16, body: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("RSA error: {0}")]
    Rsa(String),

    #[error("Prod write confirmation required")]
    ProdConfirmRequired,

    #[error("Onboard cancelled")]
    OnboardCancelled,
}

impl Error {
    /// Process exit status for this error. Unknown and general failures retain
    /// the conventional status 1; callers may branch on an unavailable unlock.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::AuthRequired => 3,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_required_has_a_distinct_exit_code() {
        assert_eq!(Error::AuthRequired.exit_code(), 3);
        assert_eq!(Error::Auth("wrong password".into()).exit_code(), 1);
        assert_eq!(Error::Config("other".into()).exit_code(), 1);
    }
}
