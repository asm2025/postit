//! Building the multipart request for a post with media.
//!
//! Every file is opened before anything is sent, so a file that vanished stops the post whole
//! and no album goes out short one item. A stream cannot be replayed, so each attempt builds
//! its form fresh.

use postit_core::{
    AgentError, MediaAsset, MediaAssetId, MediaError, MediaStore, Variant, Violation, ViolationCode,
};
use postit_http::stream_body;
use reqwest::multipart::{Form, Part};
use serde_json::{Map, Value, json};

use crate::compose::{Method, Options, album_type, extension};

/// A violation at `media[i]` that stops the post.
fn media_error(i: usize, code: ViolationCode, id: MediaAssetId) -> AgentError {
    AgentError::Validation(vec![
        Violation::new(code)
            .at(format!("media[{i}]"))
            .param("id", id.to_string()),
    ])
}

/// The variant's assets, in `variant.media` order.
fn ordered<'a>(
    variant: &Variant,
    assets: &'a [MediaAsset],
) -> Result<Vec<&'a MediaAsset>, AgentError> {
    variant
        .media
        .iter()
        .enumerate()
        .map(|(i, item)| {
            assets
                .iter()
                .find(|asset| asset.id == item.asset)
                .ok_or_else(|| media_error(i, ViolationCode::MediaMissing, item.asset))
        })
        .collect()
}

/// A part streaming the asset's bytes, with the asset's stored type and a generated name.
async fn part(
    store: &dyn MediaStore,
    asset: &MediaAsset,
    index: usize,
) -> Result<Part, AgentError> {
    let stream = store.open(asset).await.map_err(|err| match err {
        MediaError::NotFound => media_error(index, ViolationCode::MediaMissing, asset.id),
        MediaError::NotReady => media_error(index, ViolationCode::MediaNotReady, asset.id),
        _ => AgentError::Transport {
            message: "a media file could not be read".to_owned(),
        },
    })?;
    Part::stream_with_length(stream_body(stream), asset.bytes)
        .file_name(format!("file{index}.{}", extension(&asset.mime)))
        .mime_str(&asset.mime)
        .map_err(|_| {
            AgentError::Validation(vec![
                Violation::new(ViolationCode::MediaTypeNotAllowed)
                    .at(format!("media[{index}]"))
                    .param("content_type", asset.mime.clone()),
            ])
        })
}

/// What a video says about itself, left out when not probed (never sent as zero).
fn video_fields(asset: &MediaAsset) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("supports_streaming".to_owned(), json!(true));
    if let (Some(width), Some(height)) = (asset.width, asset.height) {
        fields.insert("width".to_owned(), json!(width));
        fields.insert("height".to_owned(), json!(height));
    }
    if let Some(ms) = asset.duration_ms {
        fields.insert("duration".to_owned(), json!(ms.div_ceil(1000)));
    }
    fields
}

fn field_name(method: Method) -> &'static str {
    match method {
        Method::Photo => "photo",
        Method::Animation => "animation",
        Method::Video => "video",
        _ => "document",
    }
}

/// The form for `method` (not `sendMessage`) to `chat_id`, with `caption` on the first item.
///
/// # Errors
/// A permanent violation at `media[i]` for a file that is missing or not ready.
pub(crate) async fn form(
    store: &dyn MediaStore,
    method: Method,
    chat_id: i64,
    variant: &Variant,
    assets: &[MediaAsset],
    caption: &str,
    options: Options,
) -> Result<Form, AgentError> {
    let assets = ordered(variant, assets)?;
    let mut parts = Vec::with_capacity(assets.len());
    for (i, asset) in assets.iter().enumerate() {
        parts.push(part(store, asset, i).await?);
    }
    let mut form = Form::new().text("chat_id", chat_id.to_string());
    if options.silent {
        form = form.text("disable_notification", "true");
    }
    if options.protect_content {
        form = form.text("protect_content", "true");
    }
    if method == Method::Album {
        let items: Vec<Value> = assets
            .iter()
            .enumerate()
            .map(|(i, asset)| {
                let kind = album_type(asset, options.as_file);
                let mut item = if kind == "video" {
                    video_fields(asset)
                } else {
                    Map::new()
                };
                item.insert("type".to_owned(), json!(kind));
                item.insert("media".to_owned(), json!(format!("attach://file{i}")));
                if i == 0 && !caption.is_empty() {
                    item.insert("caption".to_owned(), json!(caption));
                }
                Value::Object(item)
            })
            .collect();
        form = form.text("media", Value::Array(items).to_string());
        for (i, part) in parts.into_iter().enumerate() {
            form = form.part(format!("file{i}"), part);
        }
        return Ok(form);
    }
    if !caption.is_empty() {
        form = form.text("caption", caption.to_owned());
    }
    if let (Method::Video, Some(asset)) = (method, assets.first()) {
        for (name, value) in video_fields(asset) {
            form = form.text(name, value.to_string());
        }
    }
    if let Some(part) = parts.into_iter().next() {
        form = form.part(field_name(method), part);
    }
    Ok(form)
}
