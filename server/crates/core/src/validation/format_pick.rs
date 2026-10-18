//! Choosing which of an agent's formats a variant is published as.

use serde_json::Value;

use crate::{
    AgentDescriptor, FormatId, FormatSpec, MediaAsset, MediaKind, Variant, Violation, ViolationCode,
};

/// Picks the format `variant` is published as.
///
/// A declared format whose `selected_by` option matches the variant's options wins. Otherwise
/// the format is inferred from the content (see the candidate lists below) and the first
/// candidate the agent declares is used. `media` holds the variant's media that could be
/// resolved, in order.
///
/// # Errors
/// Returns a `FormatUnsupported` violation carrying the inferred format when the agent
/// declares none of the candidates.
pub fn pick_format<'a>(
    descriptor: &'a AgentDescriptor,
    variant: &Variant,
    media: &[MediaAsset],
) -> Result<&'a FormatSpec, Violation> {
    if let Some(format) = descriptor.formats.iter().find(|f| selected(f, variant)) {
        return Ok(format);
    }
    let candidates = candidates(descriptor, variant, media);
    candidates
        .iter()
        .find_map(|id| {
            descriptor.formats.iter().find(|f| {
                f.id == *id && (*id != FormatId::Text || media.is_empty() || text_takes(f, media))
            })
        })
        .ok_or_else(|| {
            let inferred = candidates.first().copied().unwrap_or(FormatId::Text);
            Violation::new(ViolationCode::FormatUnsupported).param(
                "inferred",
                serde_json::to_value(inferred).unwrap_or_default(),
            )
        })
}

fn selected(format: &FormatSpec, variant: &Variant) -> bool {
    let Some(selector) = &format.selected_by else {
        return false;
    };
    match variant.options.get(&selector.option) {
        Some(Value::String(s)) => *s == selector.value,
        Some(Value::Null) | None => false,
        Some(other) => *other == selector.value,
    }
}

/// Whether the `Text` format's media rules allow all of `media`.
fn text_takes(format: &FormatSpec, media: &[MediaAsset]) -> bool {
    let rules = &format.media;
    let count = u32::try_from(media.len()).unwrap_or(u32::MAX);
    count >= rules.min_count
        && count <= rules.max_count
        && media.iter().all(|m| rules.kinds.contains(&m.kind))
}

/// Formats the content could be, most specific first.
fn candidates(
    descriptor: &AgentDescriptor,
    variant: &Variant,
    media: &[MediaAsset],
) -> Vec<FormatId> {
    use FormatId::{Carousel, HtmlEmail, Image, Link, Reel, ShortVideo, Text, Video};

    if variant.email.is_some() {
        return vec![HtmlEmail];
    }
    match media {
        [] if variant.link.is_some() => vec![Link, Text],
        [] => vec![Text, Link],
        [one] if one.kind == MediaKind::Image => vec![Image, Text, Carousel],
        [one] if one.kind == MediaKind::Video => {
            if one.is_vertical() && fits_short_video(descriptor, one) {
                vec![ShortVideo, Reel, Video, Text]
            } else {
                vec![Video, Text]
            }
        }
        many if many.iter().all(|m| m.kind == MediaKind::Image) => vec![Carousel, Text],
        _ => vec![Carousel],
    }
}

/// Whether a declared `ShortVideo` format takes a clip of this length.
fn fits_short_video(descriptor: &AgentDescriptor, clip: &MediaAsset) -> bool {
    descriptor
        .formats
        .iter()
        .find(|f| f.id == FormatId::ShortVideo)
        .is_some_and(|f| match (f.media.max_duration_ms, clip.duration_ms) {
            (Some(max), Some(actual)) => actual <= max,
            _ => true,
        })
}
