//! Wire types of the workspace routes: the brand profile, writing styles, target groups, and
//! campaigns.

use chrono::{DateTime, Utc};
use postit_data::brand_profiles::{BrandProfile, BrandProfileFields};
use postit_data::writing_styles::{EmojiUse, HashtagUse, WritingStyle, WritingStyleFields};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// What the owner says about their business, for the drafter. Reads as an empty profile with
/// `id: null` until one is written.
#[derive(Debug, Serialize, ToSchema)]
pub struct BrandProfileDto {
    /// The profile, or `null` while none was written.
    pub id: Option<Uuid>,
    /// What the business is.
    pub description: String,
    /// Who it writes for.
    pub audience: String,
    /// What it sells or offers.
    pub offerings: String,
    /// Words the drafter must not use.
    pub avoid_words: Vec<String>,
    /// The language to write in by default.
    pub default_language: Option<String>,
    /// Posts that sound right, at most five.
    pub examples: Vec<String>,
    /// When it last changed, or `null` while none was written.
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<Option<BrandProfile>> for BrandProfileDto {
    fn from(profile: Option<BrandProfile>) -> Self {
        let Some(profile) = profile else {
            return Self {
                id: None,
                description: String::new(),
                audience: String::new(),
                offerings: String::new(),
                avoid_words: Vec::new(),
                default_language: None,
                examples: Vec::new(),
                updated_at: None,
            };
        };
        let f = profile.fields;
        Self {
            id: Some(profile.id.as_uuid()),
            description: f.description,
            audience: f.audience,
            offerings: f.offerings,
            avoid_words: f.avoid_words,
            default_language: f.default_language,
            examples: f.examples,
            updated_at: Some(profile.updated_at),
        }
    }
}

/// The brand profile to store; it replaces the earlier one. Absent fields are empty.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutBrandProfileRequest {
    /// What the business is.
    #[serde(default)]
    pub description: String,
    /// Who it writes for.
    #[serde(default)]
    pub audience: String,
    /// What it sells or offers.
    #[serde(default)]
    pub offerings: String,
    /// Words the drafter must not use.
    #[serde(default)]
    pub avoid_words: Vec<String>,
    /// The language to write in by default.
    #[serde(default)]
    pub default_language: Option<String>,
    /// Posts that sound right, at most five (`validation_failed` with `field: "examples"`).
    #[serde(default)]
    pub examples: Vec<String>,
}

impl From<PutBrandProfileRequest> for BrandProfileFields {
    fn from(body: PutBrandProfileRequest) -> Self {
        Self {
            description: body.description,
            audience: body.audience,
            offerings: body.offerings,
            avoid_words: body.avoid_words,
            default_language: body.default_language,
            examples: body.examples,
        }
    }
}

/// How much emoji a draft uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiUseDto {
    /// None at all.
    #[default]
    None,
    /// A little.
    Light,
    /// A lot.
    Heavy,
}

impl From<EmojiUse> for EmojiUseDto {
    fn from(value: EmojiUse) -> Self {
        match value {
            EmojiUse::None => Self::None,
            EmojiUse::Light => Self::Light,
            EmojiUse::Heavy => Self::Heavy,
        }
    }
}

impl From<EmojiUseDto> for EmojiUse {
    fn from(value: EmojiUseDto) -> Self {
        match value {
            EmojiUseDto::None => Self::None,
            EmojiUseDto::Light => Self::Light,
            EmojiUseDto::Heavy => Self::Heavy,
        }
    }
}

/// How many hashtags a draft carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HashtagUseDto {
    /// None at all.
    #[default]
    None,
    /// A few.
    Few,
    /// Many.
    Many,
}

impl From<HashtagUse> for HashtagUseDto {
    fn from(value: HashtagUse) -> Self {
        match value {
            HashtagUse::None => Self::None,
            HashtagUse::Few => Self::Few,
            HashtagUse::Many => Self::Many,
        }
    }
}

impl From<HashtagUseDto> for HashtagUse {
    fn from(value: HashtagUseDto) -> Self {
        match value {
            HashtagUseDto::None => Self::None,
            HashtagUseDto::Few => Self::Few,
            HashtagUseDto::Many => Self::Many,
        }
    }
}

/// How the drafter writes for one account. Reads as an empty style with `id: null` until one
/// is written.
#[derive(Debug, Serialize, ToSchema)]
pub struct WritingStyleDto {
    /// The style, or `null` while none was written.
    pub id: Option<Uuid>,
    /// The account it guides.
    pub account_id: Uuid,
    /// The tone, in the owner's words.
    pub tone: String,
    /// How much emoji to use.
    pub emoji: EmojiUseDto,
    /// How many hashtags to use.
    pub hashtags: HashtagUseDto,
    /// The language to write in, when it differs from the brand's default.
    pub language: Option<String>,
    /// Anything else the drafter should know.
    pub notes: String,
    /// When it last changed, or `null` while none was written.
    pub updated_at: Option<DateTime<Utc>>,
}

impl WritingStyleDto {
    /// The style of `account`, or the empty one when none was written.
    #[must_use]
    pub fn of(account: Uuid, style: Option<WritingStyle>) -> Self {
        match style {
            Some(style) => Self {
                id: Some(style.id.as_uuid()),
                account_id: account,
                tone: style.fields.tone,
                emoji: style.fields.emoji.into(),
                hashtags: style.fields.hashtags.into(),
                language: style.fields.language,
                notes: style.fields.notes,
                updated_at: Some(style.updated_at),
            },
            None => Self {
                id: None,
                account_id: account,
                tone: String::new(),
                emoji: EmojiUseDto::None,
                hashtags: HashtagUseDto::None,
                language: None,
                notes: String::new(),
                updated_at: None,
            },
        }
    }
}

/// The writing style to store; it replaces the earlier one. Absent fields are empty.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutWritingStyleRequest {
    /// The tone, in the owner's words.
    #[serde(default)]
    pub tone: String,
    /// How much emoji to use.
    #[serde(default)]
    pub emoji: EmojiUseDto,
    /// How many hashtags to use.
    #[serde(default)]
    pub hashtags: HashtagUseDto,
    /// The language to write in, when it differs from the brand's default.
    #[serde(default)]
    pub language: Option<String>,
    /// Anything else the drafter should know.
    #[serde(default)]
    pub notes: String,
}

impl From<PutWritingStyleRequest> for WritingStyleFields {
    fn from(body: PutWritingStyleRequest) -> Self {
        Self {
            tone: body.tone,
            emoji: body.emoji.into(),
            hashtags: body.hashtags.into(),
            language: body.language,
            notes: body.notes,
        }
    }
}
