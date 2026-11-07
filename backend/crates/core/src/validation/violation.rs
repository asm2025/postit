//! Structured validation failures. Clients translate them by code and parameters.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::Message;

/// Why a string is not a valid [`AspectRatio`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AspectRatioError {
    /// The string is not `W:H` with two unsigned integers.
    #[error("aspect ratio must look like 9:16")]
    Malformed,
    /// Width or height is zero.
    #[error("aspect ratio sides must be greater than zero")]
    Zero,
}

/// A width-to-height ratio. Serializes as the string `"9:16"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    /// Width part of the ratio.
    pub width: u32,
    /// Height part of the ratio.
    pub height: u32,
}

impl AspectRatio {
    /// Builds a ratio.
    ///
    /// # Errors
    /// Returns [`AspectRatioError::Zero`] when either side is zero.
    pub const fn new(width: u32, height: u32) -> Result<Self, AspectRatioError> {
        if width == 0 || height == 0 {
            Err(AspectRatioError::Zero)
        } else {
            Ok(Self { width, height })
        }
    }

    /// The ratio as `width / height`.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.width) / f64::from(self.height)
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.width, self.height)
    }
}

impl FromStr for AspectRatio {
    type Err = AspectRatioError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (w, h) = s.split_once(':').ok_or(AspectRatioError::Malformed)?;
        let width = w.parse().map_err(|_| AspectRatioError::Malformed)?;
        let height = h.parse().map_err(|_| AspectRatioError::Malformed)?;
        Self::new(width, height)
    }
}

impl Serialize for AspectRatio {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AspectRatio {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for AspectRatio {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .examples([serde_json::json!("9:16")])
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for AspectRatio {}

macro_rules! violation_codes {
    ($($(#[$meta:meta])* $name:ident => $wire:literal,)+) => {
        /// What rule a post broke. The parameters each code carries are listed on the variant;
        /// clients translate by code and fill the text from those parameters.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[serde(rename_all = "snake_case")]
        pub enum ViolationCode {
            $($(#[$meta])* $name,)+
        }

        impl ViolationCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [ViolationCode] = &[$(ViolationCode::$name,)+];

            /// The code's wire name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(ViolationCode::$name => $wire,)+
                }
            }
        }
    };
}

violation_codes! {
    /// The scheduled time is more than a minute in the past. Params: none.
    ScheduleInPast => "schedule_in_past",
    /// The local time falls in a daylight-saving gap and never happens. Params: `local` (string), `zone` (string).
    ScheduleTimeNonexistent => "schedule_time_nonexistent",
    /// The local time happens twice in a daylight-saving overlap.
    /// Params: `local` (string), `zone` (string), `earliest_offset`, `latest_offset` (strings, `+HH:MM`).
    ScheduleTimeAmbiguous => "schedule_time_ambiguous",
    /// The zone is not an IANA zone name. Params: `zone` (string).
    ScheduleZoneUnknown => "schedule_zone_unknown",
    /// The platform's native scheduling window does not allow this time.
    /// Params: `min_ms` (u64, earliest offset), `max_ms` (u64, latest offset).
    NativeScheduleOutsideWindow => "native_schedule_outside_window",
    /// The platform has no format that fits this post. Params: `inferred` (string, the format the content looked like).
    FormatUnsupported => "format_unsupported",
    /// The format needs text and there is none. Params: none.
    TextRequired => "text_required",
    /// The text is over the limit. Params: `max` (u32), `actual` (u32), `measure` (string).
    TextTooLong => "text_too_long",
    /// The format needs media and there is none. Params: none.
    MediaRequired => "media_required",
    /// The media count is outside the allowed range. Params: `min` (u32), `max` (u32), `actual` (u32).
    MediaCount => "media_count",
    /// A referenced media asset does not exist or is not the owner's. Params: `id` (string).
    MediaMissing => "media_missing",
    /// The media kind (image, video, audio) is not allowed. Params: `kind` (string), `allowed` (string list).
    MediaKindNotAllowed => "media_kind_not_allowed",
    /// The media type is not allowed. Params: `content_type` (string), `allowed` (string list).
    MediaTypeNotAllowed => "media_type_not_allowed",
    /// The media file is over the size limit. Params: `max_bytes` (u64), `actual_bytes` (u64).
    MediaTooLarge => "media_too_large",
    /// The media dimensions are outside the allowed range. Params: `min_width`, `min_height`, `max_width`, `max_height` (u32, each present only when the format sets it), `width`, `height` (u32).
    MediaDimensions => "media_dimensions",
    /// The media aspect ratio is outside the allowed range. Params: `min` (string), `max` (string), `actual` (string).
    MediaAspectRatio => "media_aspect_ratio",
    /// The media duration is outside the allowed range. Params: `min_ms` (u64), `max_ms` (u64), `actual_ms` (u64).
    MediaDuration => "media_duration",
    /// The media is still being processed or failed processing. Params: `id` (string).
    MediaNotReady => "media_not_ready",
    /// The format needs a reply target and there is none. Params: none.
    ReplyToRequired => "reply_to_required",
    /// The format does not accept a reply target. Params: none.
    ReplyToNotAllowed => "reply_to_not_allowed",
    /// A per-platform override uses media that the post does not carry. Params: `id` (string).
    OverrideMediaNotInPost => "override_media_not_in_post",
    /// A per-platform override lists the same media twice. Params: `id` (string).
    OverrideMediaDuplicate => "override_media_duplicate",
    /// An email post has no email document. Params: none.
    EmailDocumentRequired => "email_document_required",
    /// An email post has no subject. Params: none.
    EmailSubjectRequired => "email_subject_required",
    /// The rendered email, with every link at its signed length, is over the size limit.
    /// Params: `max_bytes` (u64), `actual_bytes` (u64).
    EmailTooLarge => "email_too_large",
    /// The sending account has no postal address for the footer. Params: none.
    EmailPostalAddressRequired => "email_postal_address_required",
    /// A merge tag does not parse, or sits inside a URL. Params: none.
    EmailMergeTagInvalid => "email_merge_tag_invalid",
    /// An option is not in the agent's schema. Params: `name` (string).
    OptionUnknown => "option_unknown",
    /// A required option is missing. Params: `name` (string).
    OptionRequired => "option_required",
    /// An option has the wrong JSON type. Params: `name` (string), `expected` (string).
    OptionType => "option_type",
    /// An option value is not one of the allowed values. Params: `name` (string), `allowed` (string list).
    OptionNotAllowed => "option_not_allowed",
    /// An option text is shorter than allowed. Params: `name` (string), `min` (u32).
    OptionTooShort => "option_too_short",
    /// An option text is longer than allowed. Params: `name` (string), `max` (u32).
    OptionTooLong => "option_too_long",
    /// An integer option is outside its range. Params: `name` (string), `min` (i64, optional), `max` (i64, optional).
    OptionOutOfRange => "option_out_of_range",
    /// A consent option is not accepted. Params: `name` (string).
    OptionConsentRequired => "option_consent_required",
    /// A rule only the agent knows. Params: `message` (`{key, en}`).
    AgentRule => "agent_rule",
}

/// A change that would make a violating post valid, offered to the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SuggestedFix {
    /// Crop or pad the media to this aspect ratio.
    Fit {
        /// Target ratio.
        aspect: AspectRatio,
    },
    /// Trim the video to at most this many milliseconds.
    Trim {
        /// Maximum duration in milliseconds.
        max_ms: u64,
    },
    /// Remove the media item at `index`, or all media when `None`.
    RemoveMedia {
        /// Zero-based position, or `None` for every item.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    /// Shorten the text to at most this many units of the format's measure.
    ShortenText {
        /// Maximum length.
        max: u32,
    },
}

#[cfg(feature = "openapi")]
mod fix_schema {
    //! The [`SuggestedFix`] schema. Each variant is a named object whose `type` is its tag, and
    //! the fix is a `oneOf` over them with a discriminator mapping (client generators need one;
    //! the derived inline `oneOf` gave the Dart client a type it never defined). The variant
    //! structs only describe the wire format; a test checks them against `SuggestedFix`'s
    //! serialization.
    #![allow(dead_code)]

    use utoipa::openapi::RefOr;
    use utoipa::openapi::schema::{Discriminator, OneOfBuilder, Ref, Schema};
    use utoipa::{PartialSchema, ToSchema};

    use super::{AspectRatio, SuggestedFix};

    /// Crop or pad the media to this aspect ratio.
    #[derive(ToSchema)]
    pub(super) struct FitFix {
        #[schema(rename = "type", inline)]
        tag: FitTag,
        /// Target ratio.
        aspect: AspectRatio,
    }

    /// Trim the video to at most this many milliseconds.
    #[derive(ToSchema)]
    pub(super) struct TrimFix {
        #[schema(rename = "type", inline)]
        tag: TrimTag,
        /// Maximum duration in milliseconds.
        max_ms: u64,
    }

    /// Remove the media item at `index`, or all media when it is absent.
    #[derive(ToSchema)]
    pub(super) struct RemoveMediaFix {
        #[schema(rename = "type", inline)]
        tag: RemoveMediaTag,
        /// Zero-based position; absent for every item.
        index: Option<usize>,
    }

    /// Shorten the text to at most this many units of the format's measure.
    #[derive(ToSchema)]
    pub(super) struct ShortenTextFix {
        #[schema(rename = "type", inline)]
        tag: ShortenTextTag,
        /// Maximum length.
        max: u32,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum FitTag {
        Fit,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum TrimTag {
        Trim,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum RemoveMediaTag {
        RemoveMedia,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum ShortenTextTag {
        ShortenText,
    }

    /// Each tag and the name of its variant's schema.
    pub(super) fn variants() -> [(&'static str, std::borrow::Cow<'static, str>); 4] {
        [
            ("fit", FitFix::name()),
            ("trim", TrimFix::name()),
            ("remove_media", RemoveMediaFix::name()),
            ("shorten_text", ShortenTextFix::name()),
        ]
    }

    impl PartialSchema for SuggestedFix {
        fn schema() -> RefOr<Schema> {
            let mut discriminator = Discriminator::new("type");
            let mut one_of = OneOfBuilder::new().description(Some(
                "A change that would make a violating post valid, offered to the client.",
            ));
            for (tag, name) in variants() {
                discriminator
                    .mapping
                    .insert(tag.to_owned(), format!("#/components/schemas/{name}"));
                one_of = one_of.item(Ref::from_schema_name(name));
            }
            Schema::OneOf(one_of.discriminator(Some(discriminator)).build()).into()
        }
    }

    impl ToSchema for SuggestedFix {
        fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
            schemas.push((FitFix::name().into(), FitFix::schema()));
            <FitFix as ToSchema>::schemas(schemas);
            schemas.push((TrimFix::name().into(), TrimFix::schema()));
            <TrimFix as ToSchema>::schemas(schemas);
            schemas.push((RemoveMediaFix::name().into(), RemoveMediaFix::schema()));
            <RemoveMediaFix as ToSchema>::schemas(schemas);
            schemas.push((ShortenTextFix::name().into(), ShortenTextFix::schema()));
            <ShortenTextFix as ToSchema>::schemas(schemas);
        }
    }
}

/// One rule a post broke, with the data a client needs to explain and fix it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Violation {
    /// Which rule.
    pub code: ViolationCode,
    /// Where in the post, e.g. `text`, `media[1]`, `options.privacy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Code-specific parameters (see [`ViolationCode`]).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub params: Map<String, Value>,
    /// A change that would resolve it, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<SuggestedFix>,
}

impl Violation {
    /// A violation with no path, parameters, or fix.
    #[must_use]
    pub fn new(code: ViolationCode) -> Self {
        Self {
            code,
            path: None,
            params: Map::new(),
            fix: None,
        }
    }

    /// Sets where in the post the violation is.
    #[must_use]
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Adds a parameter.
    #[must_use]
    pub fn param(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }

    /// Attaches a suggested fix.
    #[must_use]
    pub fn fix(mut self, fix: SuggestedFix) -> Self {
        self.fix = Some(fix);
        self
    }

    /// A rule only the agent knows, carrying its own message.
    #[must_use]
    pub fn agent_rule(message: Message) -> Self {
        Self::new(ViolationCode::AgentRule).param(
            "message",
            serde_json::json!({"key": message.key.into_owned(), "en": message.en.into_owned()}),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;

    /// The fix schema is a `oneOf` over named variants with a `type` discriminator (client
    /// generators need one), and each variant's properties are exactly the keys the fix
    /// serializes with every field set, so the hand-written schema cannot drift from the wire.
    #[cfg(feature = "openapi")]
    #[test]
    fn suggested_fix_schema_is_discriminated_and_matches_the_wire() {
        use utoipa::{PartialSchema as _, ToSchema as _};

        let schema = serde_json::to_value(SuggestedFix::schema()).unwrap();
        let mut components = Vec::new();
        SuggestedFix::schemas(&mut components);
        let named: std::collections::BTreeMap<String, Value> = components
            .into_iter()
            .map(|(name, schema)| (name, serde_json::to_value(schema).unwrap()))
            .collect();
        assert_eq!(schema["discriminator"]["propertyName"], "type");
        let mapping = schema["discriminator"]["mapping"].as_object().unwrap();
        let samples = [
            SuggestedFix::Fit {
                aspect: AspectRatio {
                    width: 9,
                    height: 16,
                },
            },
            SuggestedFix::Trim { max_ms: 1 },
            SuggestedFix::RemoveMedia { index: Some(0) },
            SuggestedFix::ShortenText { max: 1 },
        ];
        // A new variant stops this match compiling; its arm and `variants` then ask for a
        // sample above.
        let slot = |fix: &SuggestedFix| match fix {
            SuggestedFix::Fit { .. } => 0,
            SuggestedFix::Trim { .. } => 1,
            SuggestedFix::RemoveMedia { .. } => 2,
            SuggestedFix::ShortenText { .. } => 3,
        };
        let mut covered: Vec<usize> = samples.iter().map(slot).collect();
        covered.sort_unstable();
        assert_eq!(
            covered,
            (0..4).collect::<Vec<_>>(),
            "one sample per variant"
        );
        assert_eq!(mapping.len(), samples.len());
        assert_eq!(schema["oneOf"].as_array().unwrap().len(), samples.len());
        for sample in samples {
            let wire = serde_json::to_value(&sample).unwrap();
            let tag = wire["type"].as_str().unwrap();
            let name = mapping[tag]
                .as_str()
                .unwrap()
                .trim_start_matches("#/components/schemas/");
            let variant = &named[name];
            let mut properties: Vec<&String> =
                variant["properties"].as_object().unwrap().keys().collect();
            let mut keys: Vec<&String> = wire.as_object().unwrap().keys().collect();
            properties.sort();
            keys.sort();
            assert_eq!(properties, keys, "{name}");
            assert_eq!(
                variant["properties"]["type"]["enum"],
                json!([tag]),
                "{name}"
            );
        }
    }

    #[test]
    fn violation_wire_format() {
        let v = Violation::new(ViolationCode::TextTooLong)
            .at("text")
            .param("max", 280)
            .param("actual", 301)
            .param("measure", "x_weighted")
            .fix(SuggestedFix::ShortenText { max: 280 });
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            json!({"code":"text_too_long","path":"text","params":{"max":280,"actual":301,"measure":"x_weighted"},"fix":{"type":"shorten_text","max":280}})
        );
    }

    #[test]
    fn bare_violation_omits_empty_fields() {
        let v = Violation::new(ViolationCode::MediaRequired);
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            json!({"code": "media_required"})
        );
    }

    #[test]
    fn agent_rule_carries_message() {
        let v = Violation::agent_rule(Message::new_static("tiktok.rule", "Nope"));
        assert_eq!(v.code, ViolationCode::AgentRule);
        assert_eq!(
            v.params["message"],
            json!({"key": "tiktok.rule", "en": "Nope"})
        );
    }

    #[test]
    fn aspect_ratio_round_trips() {
        let a: AspectRatio = serde_json::from_str("\"9:16\"").unwrap();
        assert_eq!((a.width, a.height), (9, 16));
        assert_eq!(serde_json::to_string(&a).unwrap(), "\"9:16\"");
        assert!((a.as_f64() - 0.5625).abs() < 1e-9);
        assert!(serde_json::from_str::<AspectRatio>("\"9x16\"").is_err());
        assert!(serde_json::from_str::<AspectRatio>("\"0:1\"").is_err());
        assert!(serde_json::from_str::<AspectRatio>("\"9:\"").is_err());
    }

    #[test]
    fn codes_are_unique_snake_case() {
        assert_eq!(ViolationCode::ALL.len(), 36);
        for wire in [
            "email_too_large",
            "email_postal_address_required",
            "email_merge_tag_invalid",
        ] {
            assert!(
                ViolationCode::ALL.iter().any(|code| code.as_str() == wire),
                "{wire}"
            );
        }
        let mut seen = HashSet::new();
        for code in ViolationCode::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(code.as_str()));
            assert!(
                code.as_str()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_')
            );
            assert!(seen.insert(code.as_str()), "{}", code.as_str());
        }
    }

    #[test]
    fn fixes_serialize_with_type_tag() {
        assert_eq!(
            serde_json::to_value(SuggestedFix::RemoveMedia { index: None }).unwrap(),
            json!({"type": "remove_media"})
        );
        assert_eq!(
            serde_json::to_value(SuggestedFix::Fit {
                aspect: AspectRatio::new(9, 16).unwrap()
            })
            .unwrap(),
            json!({"type": "fit", "aspect": "9:16"})
        );
    }
}
