//! Helpers an agent uses to declare its formats, options schema, and human text.

use std::borrow::Cow;

use postit_core::{
    FormatId, FormatSpec, MediaRules, Message, OptionsSchema, PlatformId, SchemaError, TextMeasure,
    TextRules,
};

/// An agent's options schema is outside the supported subset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("schema for platform {platform} is outside the subset: {source}")]
pub struct DeclareError {
    /// The platform whose schema failed.
    pub platform: PlatformId,
    /// Why the schema was rejected.
    pub source: SchemaError,
}

/// Text limits counted with `measure`, at most `max` units.
#[must_use]
pub fn text_rules(measure: TextMeasure, max: u32, required: bool) -> TextRules {
    TextRules {
        measure,
        max,
        required,
    }
}

/// A text-only format with required text, no media, and no selecting option.
#[must_use]
pub fn text_format(measure: TextMeasure, max: u32) -> FormatSpec {
    media_format(
        FormatId::Text,
        text_rules(measure, max, true),
        MediaRules::none(),
    )
}

/// A format with the given text and media rules and no selecting option.
#[must_use]
pub fn media_format(id: FormatId, text: TextRules, media: MediaRules) -> FormatSpec {
    FormatSpec {
        id,
        text,
        media,
        selected_by: None,
    }
}

/// Parses an agent's options schema.
///
/// # Errors
/// Returns a [`DeclareError`] naming `platform` when the schema leaves the supported subset.
pub fn schema(platform: &PlatformId, json: &str) -> Result<OptionsSchema, DeclareError> {
    OptionsSchema::from_json_str(json).map_err(|source| DeclareError {
        platform: platform.clone(),
        source,
    })
}

/// A message keyed `"{platform}.{key}"` with its English text.
#[must_use]
pub fn message(platform: &PlatformId, key: &str, en: &str) -> Message {
    Message {
        key: Cow::Owned(format!("{platform}.{key}")),
        en: Cow::Owned(en.to_owned()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{FormatId, MediaRules, PlatformId, TextMeasure};

    use super::*;

    fn mock() -> PlatformId {
        PlatformId::new("mock").unwrap()
    }

    #[test]
    fn text_format_shape() {
        let f = text_format(TextMeasure::Graphemes, 300);
        assert_eq!(f.id, FormatId::Text);
        assert_eq!(f.text, text_rules(TextMeasure::Graphemes, 300, true));
        assert!(f.text.required);
        assert_eq!(f.media, MediaRules::none());
        assert!(f.selected_by.is_none());
    }

    #[test]
    fn schema_names_platform_on_error() {
        let err = schema(
            &mock(),
            r#"{"type":"object","properties":{"a":{"type":"string","pattern":"x"}}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("mock"), "{err}");
        assert!(schema(&mock(), r#"{"type":"object","properties":{}}"#).is_ok());
    }

    #[test]
    fn message_prefixes_key() {
        let m = message(&mock(), "outcome.label", "Outcome");
        assert_eq!(m.key, "mock.outcome.label");
        assert_eq!(m.en, "Outcome");
    }
}
