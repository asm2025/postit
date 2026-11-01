//! What the Telegram agent tells the system about itself: limits, formats, the credentials
//! form, and the per-post options.

use std::collections::BTreeSet;

use postit_agent_kit::{media_format, message, schema, text_rules};
use postit_core::{
    AgentDescriptor, AgentKind, ConnectMethod, FormatId, FormatSpec, MediaKind, MediaRules,
    OptionsSchema, PlatformId, TextMeasure, TextRules,
};
use serde_json::{Value, json};

/// The platform ID.
pub const TELEGRAM: &str = "telegram";

/// Largest photo sent as a photo, in decimal bytes.
pub const PHOTO_MAX_BYTES: u64 = 10_000_000;
/// Largest file of any other kind, in decimal bytes.
pub const FILE_MAX_BYTES: u64 = 50_000_000;
/// Most a photo's width plus height may be, in pixels.
pub const PHOTO_MAX_SIDES: u64 = 10_000;
/// Most a photo's longer side may be over its shorter.
pub const PHOTO_MAX_RATIO: u64 = 20;
/// Longest message text, in UTF-16 code units.
pub const TEXT_MAX: u32 = 4_096;
/// Longest caption, in UTF-16 code units.
pub const CAPTION_MAX: u32 = 1_024;
/// Fewest items in an album.
pub const ALBUM_MIN: u32 = 2;
/// Most items in an album.
pub const ALBUM_MAX: u32 = 10;

const BRAND_COLOR: &str = "#26A5E4";
/// A small paper plane drawn for postit; not Telegram's logo.
static ICON: &[u8] = include_bytes!("../assets/icon.png");

fn platform_id() -> PlatformId {
    PlatformId::new(TELEGRAM)
        .unwrap_or_else(|e| unreachable!("the Telegram platform ID is valid: {e}"))
}

fn parse(platform: &PlatformId, json: &Value) -> OptionsSchema {
    schema(platform, &json.to_string()).unwrap_or_else(|e| unreachable!("{e}"))
}

/// The credentials form: the bot token, and chats to add by hand.
#[must_use]
pub fn credentials_schema(platform: &PlatformId) -> OptionsSchema {
    let m = |key: &str, en: &str| message(platform, key, en);
    parse(
        platform,
        &json!({
            "type": "object",
            "properties": {
                "bot_token": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 200,
                    "x-postit-secret": true,
                    "x-postit-label": m("credentials.bot_token.label", "Bot token"),
                    "x-postit-help": m(
                        "credentials.bot_token.help",
                        "The token BotFather gave you for a bot made only for postit.",
                    ),
                },
                "chats": {
                    "type": "string",
                    "maxLength": 2000,
                    "x-postit-label": m("credentials.chats.label", "Channels and groups"),
                    "x-postit-help": m(
                        "credentials.chats.help",
                        "Optional. Add chats by hand, separated by commas: @name, a t.me/name link, or a numeric chat ID. A private invite link (t.me/+...) cannot be used; use the numeric ID instead.",
                    ),
                },
            },
            "required": ["bot_token"],
            "additionalProperties": false,
        }),
    )
}

/// The per-post options.
#[must_use]
pub fn options_schema(platform: &PlatformId) -> OptionsSchema {
    let m = |key: &str, en: &str| message(platform, key, en);
    let flag = |name: &str, label: &str, help: &str| {
        json!({
            "type": "boolean",
            "default": false,
            "x-postit-label": m(&format!("options.{name}.label"), label),
            "x-postit-help": m(&format!("options.{name}.help"), help),
        })
    };
    parse(
        platform,
        &json!({
            "type": "object",
            "properties": {
                "silent": flag(
                    "silent",
                    "Send silently",
                    "People get the post without a notification sound.",
                ),
                "protect_content": flag(
                    "protect_content",
                    "Protect content",
                    "Stops people from forwarding or saving the post.",
                ),
                "no_link_preview": flag(
                    "no_link_preview",
                    "No link preview",
                    "Does not show a preview card for links in the text.",
                ),
                "as_file": flag(
                    "as_file",
                    "Send images and videos as files",
                    "Sends the original file without compression. Allows larger images.",
                ),
            },
            "additionalProperties": false,
        }),
    )
}

fn text(max: u32, required: bool) -> TextRules {
    text_rules(TextMeasure::Utf16Units, max, required)
}

fn media(kinds: &[MediaKind], min: u32, max: u32, mimes: &[&str]) -> MediaRules {
    MediaRules {
        min_count: min,
        max_count: max,
        kinds: kinds.to_vec(),
        mime_types: mimes.iter().map(|&m| m.to_owned()).collect(),
        max_bytes: Some(FILE_MAX_BYTES),
        ..MediaRules::none()
    }
}

fn formats() -> Vec<FormatSpec> {
    vec![
        media_format(FormatId::Text, text(TEXT_MAX, true), MediaRules::none()),
        media_format(
            FormatId::Image,
            text(CAPTION_MAX, false),
            media(
                &[MediaKind::Image],
                1,
                1,
                &["image/jpeg", "image/png", "image/webp", "image/gif"],
            ),
        ),
        media_format(
            FormatId::Video,
            text(CAPTION_MAX, false),
            media(&[MediaKind::Video], 1, 1, &["video/mp4", "video/quicktime"]),
        ),
        media_format(
            FormatId::Carousel,
            text(CAPTION_MAX, false),
            media(
                &[MediaKind::Image, MediaKind::Video],
                ALBUM_MIN,
                ALBUM_MAX,
                &["image/jpeg", "image/png", "image/webp", "video/mp4"],
            ),
        ),
    ]
}

/// The agent's descriptor.
#[must_use]
pub fn descriptor() -> AgentDescriptor {
    let id = platform_id();
    AgentDescriptor {
        options_schema: options_schema(&id),
        connect: ConnectMethod::Credentials(credentials_schema(&id)),
        platform: id,
        display_name: "Telegram".to_owned(),
        icon_png: ICON,
        brand_color: BRAND_COLOR.to_owned(),
        kind: AgentKind::Broadcast,
        formats: formats(),
        capabilities: BTreeSet::new(),
        config_schema_version: 1,
        native_schedule: None,
        quota_buckets: Vec::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_valid() {
        let d = descriptor();
        d.validate().unwrap();
        assert_eq!(d.kind, AgentKind::Broadcast);
        assert!(d.capabilities.is_empty());
        assert!(d.quota_buckets.is_empty());
        let ids: Vec<_> = d.formats.iter().map(|f| f.id).collect();
        assert_eq!(
            ids,
            [
                FormatId::Text,
                FormatId::Image,
                FormatId::Video,
                FormatId::Carousel
            ]
        );
        let carousel = &d.formats[3];
        assert_eq!(
            (carousel.media.min_count, carousel.media.max_count),
            (2, 10)
        );
        assert_eq!(d.formats[0].text.max, 4_096);
        assert_eq!(d.formats[1].text.max, 1_024);
        assert!(
            d.formats
                .iter()
                .all(|f| f.text.measure == TextMeasure::Utf16Units)
        );
    }

    #[test]
    fn options_are_four_boolean_flags_defaulting_to_false() {
        let schema = format!("{:?}", descriptor().options_schema);
        for name in ["silent", "protect_content", "no_link_preview", "as_file"] {
            assert!(schema.contains(name), "{name} missing");
        }
    }
}
