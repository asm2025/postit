//! Credential values split into plain and secret parts.

use std::{collections::BTreeMap, fmt};

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};

use super::OptionsSchema;
use crate::Violation;

/// One credential value: plain data, or a secret that never prints.
#[derive(Clone)]
pub enum CredentialValue {
    /// A non-secret value (e.g. a server URL or chat ID).
    Plain(Value),
    /// A secret (e.g. a bot token); sealed in the vault.
    Secret(SecretString),
}

impl fmt::Debug for CredentialValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(v) => v.fmt(f),
            Self::Secret(_) => f.write_str("[redacted]"),
        }
    }
}

/// Validated credential values in schema order.
#[derive(Clone, Default)]
pub struct CredentialFields {
    entries: Vec<(String, CredentialValue)>,
}

impl CredentialFields {
    /// Rebuilds fields from stored parts without validating them: plain entries
    /// first in map order, then secrets.
    ///
    /// A schema change between releases surfaces when an agent uses the fields,
    /// not as a load failure.
    #[must_use]
    pub fn from_parts(plain: Map<String, Value>, secret: BTreeMap<String, SecretString>) -> Self {
        let entries = plain
            .into_iter()
            .map(|(k, v)| (k, CredentialValue::Plain(v)))
            .chain(
                secret
                    .into_iter()
                    .map(|(k, v)| (k, CredentialValue::Secret(v))),
            )
            .collect();
        Self { entries }
    }

    /// Splits the fields into plain values and exposed secrets.
    ///
    /// For the credential store only: `Vault::seal` serializes plain strings,
    /// so the caller seals the secret map and drops it right away.
    #[must_use]
    pub fn split(&self) -> (Map<String, Value>, BTreeMap<String, String>) {
        let mut plain = Map::new();
        let mut secret = BTreeMap::new();
        for (name, value) in &self.entries {
            match value {
                CredentialValue::Plain(v) => {
                    plain.insert(name.clone(), v.clone());
                }
                CredentialValue::Secret(s) => {
                    secret.insert(name.clone(), s.expose_secret().to_owned());
                }
            }
        }
        (plain, secret)
    }

    /// The plain value named `name`, if present and not secret.
    #[must_use]
    pub fn get_plain(&self, name: &str) -> Option<&Value> {
        match self.get(name)? {
            CredentialValue::Plain(v) => Some(v),
            CredentialValue::Secret(_) => None,
        }
    }

    /// The secret named `name`, if present and secret.
    #[must_use]
    pub fn get_secret(&self, name: &str) -> Option<&SecretString> {
        match self.get(name)? {
            CredentialValue::Secret(s) => Some(s),
            CredentialValue::Plain(_) => None,
        }
    }

    /// The entries in schema order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &CredentialValue)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    fn get(&self, name: &str) -> Option<&CredentialValue> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

impl fmt::Debug for CredentialFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

impl OptionsSchema {
    /// Validates `values` and splits them into plain and secret credentials.
    ///
    /// Fields hidden by `depends_on` and values the schema does not list are left out.
    ///
    /// # Errors
    /// Returns every [`Violation`] when the values do not satisfy the schema.
    #[allow(clippy::needless_pass_by_value)] // takes ownership so callers drop the plaintext map
    pub fn parse_credentials(
        &self,
        values: Map<String, Value>,
    ) -> Result<CredentialFields, Vec<Violation>> {
        let violations = self.validate(&values);
        if !violations.is_empty() {
            return Err(violations);
        }
        let mut entries = Vec::new();
        for field in self.fields() {
            if !self.applies(field, &values) {
                continue;
            }
            let Some(value) = values.get(&field.name) else {
                continue;
            };
            let entry = if field.secret {
                let text = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                CredentialValue::Secret(SecretString::from(text))
            } else {
                CredentialValue::Plain(value.clone())
            };
            entries.push((field.name.clone(), entry));
        }
        Ok(CredentialFields { entries })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    const LBL: &str = r#"{"key":"k","en":"E"}"#;

    fn parsed() -> CredentialFields {
        let src = r#"{"type":"object","properties":{
            "server_url":{"type":"string","x-postit-label":LBL},
            "token":{"type":"string","x-postit-secret":true,"x-postit-label":LBL}},
            "required":["server_url","token"],"additionalProperties":false}"#
            .replace("LBL", LBL);
        let schema = OptionsSchema::from_json_str(&src).unwrap();
        let Value::Object(values) = serde_json::json!({
            "server_url": "https://example.test", "token": "s3cret-token"
        }) else {
            unreachable!()
        };
        schema.parse_credentials(values).unwrap()
    }

    #[test]
    fn from_parts_round_trips_split() {
        let (plain, secret) = parsed().split();
        let secret = secret
            .into_iter()
            .map(|(k, v)| (k, SecretString::from(v)))
            .collect();
        let rebuilt = CredentialFields::from_parts(plain, secret);
        assert_eq!(
            rebuilt.get_plain("server_url"),
            Some(&Value::String("https://example.test".to_owned()))
        );
        assert_eq!(
            rebuilt.get_secret("token").unwrap().expose_secret(),
            "s3cret-token"
        );
    }

    #[test]
    fn debug_of_from_parts_redacts() {
        let (plain, secret) = parsed().split();
        let secret = secret
            .into_iter()
            .map(|(k, v)| (k, SecretString::from(v)))
            .collect();
        let shown = format!("{:?}", CredentialFields::from_parts(plain, secret));
        assert!(shown.contains("[redacted]"));
        assert!(!shown.contains("s3cret-token"));
    }
}
