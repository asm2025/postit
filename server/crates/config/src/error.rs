use thiserror::Error;

/// Why configuration could not be loaded or accepted.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    /// `POSTIT_ENV` is unset in a build that has no development default.
    #[error("POSTIT_ENV is required outside a debug build")]
    MissingEnvironment,

    /// `POSTIT_ENV` holds a value that is not a known environment.
    #[error("invalid POSTIT_ENV value: {0}")]
    InvalidEnvironment(String),

    /// Layering or deserialising the sources failed.
    #[error("failed to load configuration: {0}")]
    Load(String),

    /// The loaded settings break a rule that serde cannot express.
    #[error("configuration validation failed: {0}")]
    Validation(String),

    /// A secrets file could not be read or parsed.
    #[error("failed to read secret file {path}: {reason}")]
    SecretFile {
        /// Path of the secrets file.
        path: String,
        /// What went wrong reading it.
        reason: String,
    },
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
