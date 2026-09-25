use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Development,
    Qa,
    Production,
}

impl Environment {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Qa => "qa",
            Self::Production => "production",
        }
    }

    /// Resolves `POSTIT_ENV`. `is_debug_build` selects the default-to-`development`
    /// fallback that only a debug build gets when the variable is unset.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingEnvironment`] when `POSTIT_ENV` is unset and
    /// `is_debug_build` is `false`, and [`ConfigError::InvalidEnvironment`] when it is set
    /// to anything other than `development`, `qa`, or `production`.
    pub fn resolve(is_debug_build: bool) -> Result<Self, ConfigError> {
        match std::env::var("POSTIT_ENV") {
            Ok(raw) => raw.parse(),
            Err(std::env::VarError::NotPresent) if is_debug_build => Ok(Self::Development),
            Err(_) => Err(ConfigError::MissingEnvironment),
        }
    }

    /// # Errors
    ///
    /// See [`Self::resolve`].
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::resolve(cfg!(debug_assertions))
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Environment {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "development" => Ok(Self::Development),
            "qa" => Ok(Self::Qa),
            "production" => Ok(Self::Production),
            other => Err(ConfigError::InvalidEnvironment(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_defaults_to_development_in_debug_when_unset() {
        // SAFETY-equivalent: tests run single-threaded within this module's env mutations
        // would race other tests; instead this test only asserts the debug-fallback branch
        // by calling resolve() directly and relying on POSTIT_ENV being unset in CI runners
        // that don't set it. When it *is* set, this assertion is skipped.
        if std::env::var("POSTIT_ENV").is_err() {
            assert_eq!(Environment::resolve(true), Ok(Environment::Development));
        }
    }

    #[test]
    fn resolve_rejects_missing_env_in_release() {
        if std::env::var("POSTIT_ENV").is_err() {
            assert_eq!(
                Environment::resolve(false),
                Err(ConfigError::MissingEnvironment)
            );
        }
    }

    #[test]
    fn parses_known_values() {
        assert_eq!("development".parse(), Ok(Environment::Development));
        assert_eq!("qa".parse(), Ok(Environment::Qa));
        assert_eq!("production".parse(), Ok(Environment::Production));
    }

    #[test]
    fn rejects_unknown_value() {
        let err: Result<Environment, _> = "staging".parse();
        assert_eq!(err, Err(ConfigError::InvalidEnvironment("staging".into())));
    }
}
