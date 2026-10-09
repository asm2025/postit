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

/// Setting names whose values are secrets. A figment type error normally quotes the
/// offending value, so an error at one of these keys is rendered without it.
const SECRET_KEYS: &[&str] = &["password", "pseudonym_key"];

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        let secret_leaf = err
            .path
            .last()
            .is_some_and(|leaf| SECRET_KEYS.contains(&leaf.as_str()));
        if secret_leaf {
            let source = err
                .metadata
                .as_ref()
                .map_or_else(|| "unknown source".to_string(), |m| m.name.to_string());
            return Self::Load(format!(
                "invalid value (not shown) for key \"{}\" in {source}",
                err.path.join(".")
            ));
        }
        Self::Load(err.to_string())
    }
}
