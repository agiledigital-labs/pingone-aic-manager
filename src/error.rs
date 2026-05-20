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

    #[error("Keychain error: {0}")]
    Keychain(String),

    #[error("Auth error: {0}")]
    Auth(String),

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

pub type Result<T> = std::result::Result<T, Error>;
