use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("POSTIT_ENV is required outside a debug build")]
    MissingEnvironment,

    #[error("invalid POSTIT_ENV value: {0}")]
    InvalidEnvironment(String),

    #[error("failed to load configuration: {0}")]
    Load(String),

    #[error("configuration validation failed: {0}")]
    Validation(String),

    #[error("failed to read secret file {path}: {reason}")]
    SecretFile { path: String, reason: String },
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Load(err.to_string())
    }
}
