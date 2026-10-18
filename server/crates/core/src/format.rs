//! Post formats: what each platform accepts for text and media.

use serde::{Deserialize, Serialize};

use crate::{AspectRatio, MediaKind, Message, TextMeasure};

/// A kind of post a platform can publish. The set is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FormatId {
    /// Text only.
    Text,
    /// Text with a link card.
    Link,
    /// One image with text.
    Image,
    /// Several images or mixed media.
    Carousel,
    /// One regular video.
    Video,
    /// A short vertical video.
    ShortVideo,
    /// An ephemeral story.
    Story,
    /// A reel.
    Reel,
    /// A rich HTML email.
    HtmlEmail,
}

/// Limits on the text of a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TextRules {
    /// How the platform counts length.
    pub measure: TextMeasure,
    /// Longest text allowed, in `measure` units.
    pub max: u32,
    /// Whether the text may not be empty.
    pub required: bool,
}

/// A range of acceptable aspect ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AspectRange {
    /// Narrowest ratio (width over height) allowed.
    pub min: AspectRatio,
    /// Widest ratio allowed.
    pub max: AspectRatio,
    /// The ratio a fit should aim for.
    pub preferred: AspectRatio,
}

/// A part of the frame the platform's interface covers. Fractions of the frame, 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SafeZone {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// What the zone is for, e.g. "keep captions inside".
    pub label: Message,
}

/// Limits on the media of a format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MediaRules {
    /// Fewest items allowed.
    pub min_count: u32,
    /// Most items allowed.
    pub max_count: u32,
    /// Allowed media kinds.
    pub kinds: Vec<MediaKind>,
    /// Allowed MIME types; empty allows any of the kinds.
    pub mime_types: Vec<String>,
    /// Largest file size per item.
    pub max_bytes: Option<u64>,
    /// Narrowest width in pixels.
    pub min_width: Option<u32>,
    /// Widest width in pixels.
    pub max_width: Option<u32>,
    /// Shortest height in pixels.
    pub min_height: Option<u32>,
    /// Tallest height in pixels.
    pub max_height: Option<u32>,
    /// Allowed aspect ratios; empty allows any.
    pub aspect_ratios: Vec<AspectRange>,
    /// Shortest duration in milliseconds.
    pub min_duration_ms: Option<u64>,
    /// Longest duration in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Regions the interface covers.
    pub safe_zones: Vec<SafeZone>,
}

impl MediaRules {
    /// Rules for a format with no media at all.
    #[must_use]
    pub fn none() -> Self {
        Self {
            min_count: 0,
            max_count: 0,
            kinds: Vec::new(),
            mime_types: Vec::new(),
            max_bytes: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            aspect_ratios: Vec::new(),
            min_duration_ms: None,
            max_duration_ms: None,
            safe_zones: Vec::new(),
        }
    }
}

/// Picks a format explicitly: the format applies when the target's option equals the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FormatSelector {
    /// Name of the option in the agent's options schema.
    pub option: String,
    /// The option value that selects this format.
    pub value: String,
}

/// One format an agent can publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FormatSpec {
    /// Which format this is.
    pub id: FormatId,
    /// Text limits.
    pub text: TextRules,
    /// Media limits.
    pub media: MediaRules,
    /// Option that selects this format over content-based inference.
    pub selected_by: Option<FormatSelector>,
}
