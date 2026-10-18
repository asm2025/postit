//! Credential values split into plain and secret parts.

use std::fmt;

use secrecy::SecretString;
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
