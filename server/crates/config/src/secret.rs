use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A config value that must never appear in a log, error, or redacted dump.
///
/// Deserializes like a plain string but always serializes as `"[redacted]"`, so the
/// `Settings` tree can derive `Serialize` and be logged or dumped directly without a
/// second, hand-maintained redaction pass.
#[derive(Clone)]
pub struct RedactedSecret(SecretString);

impl RedactedSecret {
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl From<String> for RedactedSecret {
    fn from(value: String) -> Self {
        Self(SecretString::from(value))
    }
}

impl fmt::Debug for RedactedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl Serialize for RedactedSecret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("[redacted]")
    }
}

impl<'de> Deserialize<'de> for RedactedSecret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_serialize_redact() {
        let secret = RedactedSecret::from("super-secret".to_string());
        assert_eq!(format!("{secret:?}"), "[redacted]");
        let json = serde_json::to_string(&secret).unwrap_or_default();
        assert_eq!(json, "\"[redacted]\"");
        assert!(!json.contains("super-secret"));
    }

    #[test]
    fn expose_returns_original_value() {
        let secret = RedactedSecret::from("super-secret".to_string());
        assert_eq!(secret.expose(), "super-secret");
    }
}
