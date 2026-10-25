//! Translatable human text: a message key plus the English default.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// A message key and its English text. Clients translate known keys and fall back to `en`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Message {
    /// Stable key, namespaced by platform ID for agent text (e.g. `tiktok.privacy.label`).
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub key: Cow<'static, str>,
    /// English default shown when no translation exists.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub en: Cow<'static, str>,
}

impl Message {
    /// Builds a message from static strings, usable in `const` and `static` items.
    #[must_use]
    pub const fn new_static(key: &'static str, en: &'static str) -> Self {
        Self {
            key: Cow::Borrowed(key),
            en: Cow::Borrowed(en),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn message_serializes_as_key_and_en() {
        let m = Message::new_static("tiktok.privacy.label", "Who can view");
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"key": "tiktok.privacy.label", "en": "Who can view"})
        );
        assert_eq!(
            serde_json::from_value::<Message>(json!({"key": "a", "en": "b"}))
                .unwrap()
                .en,
            "b"
        );
    }
}
