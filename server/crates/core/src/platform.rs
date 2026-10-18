//! Platform identity: a validated lowercase string, never a closed enum.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 32;

/// Why a string is not a valid [`PlatformId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformIdError {
    /// The string is empty.
    #[error("platform id is empty")]
    Empty,
    /// The string is longer than 32 characters.
    #[error("platform id is longer than {MAX_LEN} characters")]
    TooLong,
    /// The first character is not `a-z`, or a later one is not `a-z`, `0-9`, or `_`.
    #[error("platform id must match [a-z][a-z0-9_]*")]
    InvalidChar,
}

/// Identifier of a platform agent, e.g. `email` or `telegram`.
///
/// ASCII, first character `a-z`, the rest `a-z0-9_`, 1 to 32 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlatformId(String);

impl PlatformId {
    /// Validates and wraps `id`.
    ///
    /// # Errors
    /// Returns a [`PlatformIdError`] naming the first rule the string breaks.
    pub fn new(id: &str) -> Result<Self, PlatformIdError> {
        if id.is_empty() {
            return Err(PlatformIdError::Empty);
        }
        if id.len() > MAX_LEN {
            return Err(PlatformIdError::TooLong);
        }
        let mut chars = id.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if first_ok && rest_ok {
            Ok(Self(id.to_owned()))
        } else {
            Err(PlatformIdError::InvalidChar)
        }
    }

    /// The identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlatformId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PlatformId {
    type Err = PlatformIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for PlatformId {
    type Error = PlatformIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl Serialize for PlatformId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PlatformId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_lowercase_ids() {
        for id in ["email", "telegram", "x", "google_business"] {
            assert!(PlatformId::new(id).is_ok(), "{id}");
        }
    }

    #[test]
    fn rejects_invalid_ids() {
        for id in [
            "",
            "Email",
            "1x",
            "tik-tok",
            "tik tok",
            "ü",
            &"a".repeat(33),
        ] {
            assert!(PlatformId::new(id).is_err(), "{id}");
        }
    }

    #[test]
    fn deserialize_validates() {
        assert!(serde_json::from_str::<PlatformId>("\"TikTok\"").is_err());
        assert_eq!(
            serde_json::from_str::<PlatformId>("\"tiktok\"")
                .unwrap()
                .as_str(),
            "tiktok"
        );
    }

    #[test]
    fn serializes_as_plain_string() {
        assert_eq!(
            serde_json::to_value(PlatformId::new("email").unwrap()).unwrap(),
            json!("email")
        );
    }
}
