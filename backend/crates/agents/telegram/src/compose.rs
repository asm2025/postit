//! What to send for a variant: the Bot API method, the options, and the rules the descriptor
//! cannot express.

use postit_agent_kit::post_text;
use postit_core::{
    MediaAsset, MediaKind, SuggestedFix, TextMeasure, Variant, Violation, ViolationCode, measure,
};

use crate::descriptor::{CAPTION_MAX, PHOTO_MAX_BYTES, PHOTO_MAX_RATIO, PHOTO_MAX_SIDES, TEXT_MAX};

/// The Bot API method a variant goes out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    /// `sendMessage`.
    Message,
    /// `sendPhoto`.
    Photo,
    /// `sendAnimation`.
    Animation,
    /// `sendVideo`.
    Video,
    /// `sendDocument`.
    Document,
    /// `sendMediaGroup`.
    Album,
}

impl Method {
    /// The Bot API method name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Message => "sendMessage",
            Self::Photo => "sendPhoto",
            Self::Animation => "sendAnimation",
            Self::Video => "sendVideo",
            Self::Document => "sendDocument",
            Self::Album => "sendMediaGroup",
        }
    }
}

/// The per-post options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Options {
    /// No notification sound.
    pub silent: bool,
    /// No forwarding or saving.
    pub protect_content: bool,
    /// No link preview card.
    pub no_link_preview: bool,
    /// Send images and videos as files.
    pub as_file: bool,
}

impl Options {
    pub(crate) fn read(variant: &Variant) -> Self {
        let flag = |name: &str| {
            variant
                .options
                .get(name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        Self {
            silent: flag("silent"),
            protect_content: flag("protect_content"),
            no_link_preview: flag("no_link_preview"),
            as_file: flag("as_file"),
        }
    }
}

/// Whether the asset is a GIF, which Telegram plays as an animation.
fn is_gif(asset: &MediaAsset) -> bool {
    asset.mime == "image/gif"
}

/// The method for a variant with these assets.
pub(crate) fn choose(variant: &Variant, assets: &[MediaAsset]) -> Method {
    let as_file = Options::read(variant).as_file;
    match assets {
        [] => Method::Message,
        [_, _, ..] => Method::Album,
        [_] if as_file => Method::Document,
        [asset] => match asset.kind {
            MediaKind::Image if is_gif(asset) => Method::Animation,
            MediaKind::Image => Method::Photo,
            MediaKind::Video if asset.mime == "video/mp4" => Method::Video,
            MediaKind::Video => Method::Document,
        },
    }
}

/// Whether the asset is sent as a photo, where Telegram's photo limits apply.
fn sent_as_photo(asset: &MediaAsset, as_file: bool) -> bool {
    !as_file && asset.kind == MediaKind::Image && !is_gif(asset)
}

fn photo_violations(asset: &MediaAsset, path: &str) -> Vec<Violation> {
    let mut found = Vec::new();
    let named = |violation: Violation| violation.at(path).param("option", "as_file");
    if asset.bytes > PHOTO_MAX_BYTES {
        found.push(named(
            Violation::new(ViolationCode::MediaTooLarge)
                .param("max_bytes", PHOTO_MAX_BYTES)
                .param("actual_bytes", asset.bytes),
        ));
    }
    if let (Some(width), Some(height)) = (asset.width, asset.height) {
        let (width, height) = (u64::from(width), u64::from(height));
        if width + height > PHOTO_MAX_SIDES {
            found.push(named(
                Violation::new(ViolationCode::MediaDimensions)
                    .param("width", width)
                    .param("height", height)
                    .param("max_sum", PHOTO_MAX_SIDES),
            ));
        }
        let (long, short) = (width.max(height), width.min(height));
        if short > 0 && long > short * PHOTO_MAX_RATIO {
            found.push(named(
                Violation::new(ViolationCode::MediaAspectRatio)
                    .param("min", format!("1:{PHOTO_MAX_RATIO}"))
                    .param("max", format!("{PHOTO_MAX_RATIO}:1"))
                    .param("actual", format!("{width}:{height}")),
            ));
        }
    }
    found
}

/// The rules only this agent knows: photo limits when the photo is not sent as a file, and
/// the text limit once the link and tags are added.
pub(crate) fn violations(variant: &Variant, assets: &[MediaAsset]) -> Vec<Violation> {
    let options = Options::read(variant);
    let mut found = Vec::new();
    for (i, asset) in assets.iter().enumerate() {
        if sent_as_photo(asset, options.as_file) {
            found.extend(photo_violations(asset, &format!("media[{i}]")));
        }
    }
    let max = if variant.media.is_empty() {
        TEXT_MAX
    } else {
        CAPTION_MAX
    };
    let own = measure(&variant.text, TextMeasure::Utf16Units);
    let composed = measure(&post_text(variant), TextMeasure::Utf16Units);
    // The engine reports a text that is over the limit by itself; this is the extra the
    // link and tags add.
    if own <= max && composed > max {
        found.push(
            Violation::new(ViolationCode::TextTooLong)
                .at("text")
                .param("max", max)
                .param("actual", composed)
                .param("measure", "utf16_units")
                .fix(SuggestedFix::ShortenText { max }),
        );
    }
    found
}

/// What a dry run reports it would do.
pub(crate) fn plan(variant: &Variant, assets: &[MediaAsset], method: Method) -> Vec<String> {
    let mut steps = Vec::new();
    for (i, asset) in assets.iter().enumerate() {
        steps.push(format!(
            "Upload media {} ({}, {} bytes)",
            i + 1,
            asset.mime,
            asset.bytes
        ));
    }
    steps.push(format!("Send the post to the chat with {}", method.name()));
    if variant.media.iter().any(|item| item.alt_text.is_some()) {
        steps.push("Alt text is not sent: Telegram has no field for it".to_owned());
    }
    steps
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{
        AccountId, AccountRef, MediaAssetId, MediaAssetState, MediaItem, PlatformId, PostKind,
        UserId,
    };
    use serde_json::{Map, json};
    use url::Url;
    use uuid::Uuid;

    use super::*;

    fn asset(kind: MediaKind, mime: &str, bytes: u64, dims: Option<(u32, u32)>) -> MediaAsset {
        MediaAsset {
            id: MediaAssetId::from_uuid(Uuid::now_v7()),
            owner_id: UserId::from_uuid(Uuid::nil()),
            kind,
            mime: mime.to_owned(),
            bytes,
            width: dims.map(|d| d.0),
            height: dims.map(|d| d.1),
            duration_ms: None,
            sha256: None,
            state: MediaAssetState::Ready,
            derived_from: None,
        }
    }

    fn variant(text: &str, assets: &[MediaAsset], options: serde_json::Value) -> Variant {
        let options: Map<String, serde_json::Value> =
            options.as_object().cloned().unwrap_or_default();
        Variant {
            target: AccountRef {
                platform: PlatformId::new("telegram").unwrap(),
                account: AccountId::from_uuid(Uuid::nil()),
                remote_id: "-1001".to_owned(),
            },
            kind: PostKind::Post,
            text: text.to_owned(),
            media: assets
                .iter()
                .map(|a| MediaItem {
                    asset: a.id,
                    alt_text: None,
                })
                .collect(),
            link: None,
            tags: Vec::new(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options,
        }
    }

    fn pick(assets: &[MediaAsset], options: serde_json::Value) -> Method {
        choose(&variant("x", assets, options), assets)
    }

    #[test]
    fn choose_table() {
        let jpeg = asset(MediaKind::Image, "image/jpeg", 1, None);
        let gif = asset(MediaKind::Image, "image/gif", 1, None);
        let mp4 = asset(MediaKind::Video, "video/mp4", 1, None);
        let mov = asset(MediaKind::Video, "video/quicktime", 1, None);
        let none = json!({});
        let file = json!({"as_file": true});

        assert_eq!(pick(&[], none.clone()), Method::Message);
        assert_eq!(pick(&[jpeg.clone()], none.clone()), Method::Photo);
        assert_eq!(pick(&[jpeg.clone()], file.clone()), Method::Document);
        assert_eq!(pick(&[gif.clone()], none.clone()), Method::Animation);
        assert_eq!(pick(&[gif], file.clone()), Method::Document);
        assert_eq!(pick(&[mp4.clone()], none.clone()), Method::Video);
        assert_eq!(pick(&[mp4.clone()], file), Method::Document);
        assert_eq!(pick(&[mov], none.clone()), Method::Document);
        assert_eq!(pick(&[jpeg, mp4], none), Method::Album);
    }

    fn photo_violations_of(a: MediaAsset, options: serde_json::Value) -> Vec<Violation> {
        violations(&variant("x", &[a.clone()], options), &[a])
    }

    #[test]
    fn photo_size_is_a_violation_unless_sent_as_a_file() {
        let big = asset(MediaKind::Image, "image/jpeg", 10_000_001, None);
        let found = photo_violations_of(big.clone(), json!({}));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, ViolationCode::MediaTooLarge);
        assert_eq!(found[0].path.as_deref(), Some("media[0]"));
        assert_eq!(found[0].params["option"], "as_file");
        assert!(photo_violations_of(big, json!({"as_file": true})).is_empty());

        let exact = asset(MediaKind::Image, "image/jpeg", 10_000_000, None);
        assert!(photo_violations_of(exact, json!({})).is_empty());
    }

    #[test]
    fn a_gif_goes_to_send_animation_and_skips_photo_limits() {
        let gif = asset(MediaKind::Image, "image/gif", 20_000_000, None);
        assert!(photo_violations_of(gif, json!({})).is_empty());
    }

    #[test]
    fn photo_dimensions_and_ratio() {
        let wide = asset(MediaKind::Image, "image/png", 1, Some((6_000, 4_001)));
        let found = photo_violations_of(wide, json!({}));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, ViolationCode::MediaDimensions);

        for dims in [(2_100, 100), (100, 2_100)] {
            let odd = asset(MediaKind::Image, "image/png", 1, Some(dims));
            let found = photo_violations_of(odd, json!({}));
            assert_eq!(found.len(), 1, "{dims:?}");
            assert_eq!(found[0].code, ViolationCode::MediaAspectRatio);
        }
        let ok = asset(MediaKind::Image, "image/png", 1, Some((2_000, 100)));
        assert!(photo_violations_of(ok, json!({})).is_empty());

        // No probed dimensions: nothing to judge.
        let unknown = asset(MediaKind::Image, "image/png", 1, None);
        assert!(photo_violations_of(unknown, json!({})).is_empty());
    }

    #[test]
    fn text_over_the_limit_after_composition() {
        let mut v = variant(&"a".repeat(4_090), &[], json!({}));
        v.tags = vec!["longtag".to_owned()];
        let found = violations(&v, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, ViolationCode::TextTooLong);
        assert_eq!(found[0].params["max"], 4_096);
        assert_eq!(found[0].fix, Some(SuggestedFix::ShortenText { max: 4_096 }));

        // Already over by itself: the engine reports it, not the agent.
        let over = variant(&"a".repeat(5_000), &[], json!({}));
        assert!(violations(&over, &[]).is_empty());
    }

    #[test]
    fn a_caption_uses_the_caption_limit() {
        let image = asset(MediaKind::Image, "image/jpeg", 1, None);
        let mut v = variant(&"a".repeat(1_020), &[image.clone()], json!({}));
        v.link = Some(Url::parse("https://a.example/x").unwrap());
        let found = violations(&v, &[image]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].params["max"], 1_024);
    }
}
