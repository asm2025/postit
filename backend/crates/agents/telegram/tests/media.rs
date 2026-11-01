//! Publishing images, video, and albums.
#![allow(
    clippy::unwrap_used,
    clippy::needless_pass_by_value,
    clippy::cloned_ref_to_slice_refs,
    clippy::assigning_clones,
    clippy::panic
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use common::{Bot, TOKEN, account};
use futures::StreamExt;
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_core::{
    Agent, AgentError, ByteStream, DeliveryId, MediaAsset, MediaAssetId, MediaAssetState,
    MediaError, MediaItem, MediaKind, MediaStore, PostKind, PublishMode, PublishOutcome,
    PublishProgress, PublishRequest, RetryClass, UserId, Variant, ViolationCode,
};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, Request, ResponseTemplate};

struct NoCancel(AtomicBool);

#[async_trait]
impl PublishProgress for NoCancel {
    async fn save_container_ref(&self, _container_ref: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A part of a recorded multipart body.
struct Field {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    data: Vec<u8>,
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn multipart(request: &Request) -> Vec<Field> {
    let content_type = request
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let boundary = content_type.split("boundary=").nth(1).unwrap();
    let delimiter = format!("--{boundary}").into_bytes();
    let body = &request.body;
    let mut fields = Vec::new();
    let mut at = find(body, &delimiter, 0).unwrap() + delimiter.len();
    loop {
        if body[at..].starts_with(b"--") {
            break;
        }
        let head_start = at + 2; // after the CRLF
        let head_end = find(body, b"\r\n\r\n", head_start).unwrap();
        let head = String::from_utf8_lossy(&body[head_start..head_end]).into_owned();
        let data_start = head_end + 4;
        let next = find(body, &delimiter, data_start).unwrap();
        let data = body[data_start..next - 2].to_vec();
        let mut field = Field {
            name: String::new(),
            filename: None,
            content_type: None,
            data,
        };
        for line in head.lines() {
            let lower = line.to_lowercase();
            if lower.starts_with("content-disposition") {
                for piece in line.split(';').map(str::trim) {
                    if let Some(v) = piece.strip_prefix("name=") {
                        field.name = v.trim_matches('"').to_owned();
                    } else if let Some(v) = piece.strip_prefix("filename=") {
                        field.filename = Some(v.trim_matches('"').to_owned());
                    }
                }
            } else if lower.starts_with("content-type") {
                field.content_type = line.split(':').nth(1).map(|v| v.trim().to_owned());
            }
        }
        fields.push(field);
        at = next + delimiter.len();
    }
    fields
}

fn text<'a>(fields: &'a [Field], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|f| f.name == name && f.filename.is_none())
        .map(|f| std::str::from_utf8(&f.data).unwrap())
}

fn file<'a>(fields: &'a [Field], name: &str) -> &'a Field {
    fields
        .iter()
        .find(|f| f.name == name && f.filename.is_some())
        .unwrap_or_else(|| panic!("no file field {name}"))
}

fn asset(kind: MediaKind, mime: &str, bytes: u64, dims: Option<(u32, u32)>) -> MediaAsset {
    MediaAsset {
        id: MediaAssetId::from_uuid(Uuid::now_v7()),
        owner_id: UserId::from_uuid(Uuid::now_v7()),
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

/// Stores `data` for `asset` in the bot's media store.
fn stored(bot: &Bot, asset: &MediaAsset, data: &[u8]) {
    bot.ctx
        .media_store
        .insert_owned(asset.owner_id, asset.id, Bytes::copy_from_slice(data));
}

fn variant(text: &str, assets: &[MediaAsset], options: Value) -> Variant {
    Variant {
        target: account("-1001234567890"),
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
        options: options.as_object().cloned().unwrap_or_default(),
    }
}

fn sent(id: i64) -> Value {
    json!({
        "message_id": id,
        "chat": {"id": -1_001_234_567_890_i64, "type": "channel", "username": "newsroom"},
        "date": 1
    })
}

async fn publish(
    bot: &Bot,
    variant: &Variant,
    assets: &[MediaAsset],
) -> Result<PublishOutcome, AgentError> {
    publish_with(bot.client(), variant, assets).await
}

async fn publish_with(
    client: Box<dyn postit_core::AgentClient>,
    variant: &Variant,
    assets: &[MediaAsset],
) -> Result<PublishOutcome, AgentError> {
    client
        .publish(
            PublishRequest {
                variant,
                assets,
                mode: PublishMode::Live,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: DeliveryId::from_uuid(Uuid::nil()),
            },
            &NoCancel(AtomicBool::new(false)),
        )
        .await
}

async fn the_request(bot: &Bot, method_name: &str) -> Vec<Field> {
    let requests = bot.requests(method_name).await;
    assert_eq!(requests.len(), 1, "{method_name}");
    multipart(&requests[0])
}

#[tokio::test]
async fn photo_goes_to_send_photo() {
    let bot = Bot::start().await;
    bot.ok("sendPhoto", sent(11)).await;
    let a = asset(MediaKind::Image, "image/jpeg", 6, Some((10, 10)));
    stored(&bot, &a, b"\xff\xd8abcd");

    let outcome = publish(&bot, &variant("Look", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();

    assert!(matches!(outcome, PublishOutcome::Published(_)));
    let fields = the_request(&bot, "sendPhoto").await;
    let photo = file(&fields, "photo");
    assert_eq!(photo.data, b"\xff\xd8abcd");
    assert_eq!(photo.content_type.as_deref(), Some("image/jpeg"));
    assert_eq!(photo.filename.as_deref(), Some("file0.jpg"));
    assert_eq!(text(&fields, "caption"), Some("Look"));
    assert_eq!(text(&fields, "chat_id"), Some("-1001234567890"));
}

#[tokio::test]
async fn gif_goes_to_send_animation() {
    let bot = Bot::start().await;
    bot.ok("sendAnimation", sent(1)).await;
    let a = asset(MediaKind::Image, "image/gif", 3, None);
    stored(&bot, &a, b"GIF");
    publish(&bot, &variant("", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();
    let fields = the_request(&bot, "sendAnimation").await;
    assert_eq!(file(&fields, "animation").data, b"GIF");
    assert!(text(&fields, "caption").is_none());
}

#[tokio::test]
async fn as_file_goes_to_send_document() {
    let bot = Bot::start().await;
    bot.ok("sendDocument", sent(1)).await;
    let a = asset(MediaKind::Image, "image/png", 3, Some((10, 10)));
    stored(&bot, &a, b"PNG");
    publish(
        &bot,
        &variant("", &[a.clone()], json!({"as_file": true})),
        &[a],
    )
    .await
    .unwrap();
    let fields = the_request(&bot, "sendDocument").await;
    assert_eq!(
        file(&fields, "document").filename.as_deref(),
        Some("file0.png")
    );
}

#[tokio::test]
async fn quicktime_goes_to_send_document() {
    let bot = Bot::start().await;
    bot.ok("sendDocument", sent(1)).await;
    let a = asset(MediaKind::Video, "video/quicktime", 3, None);
    stored(&bot, &a, b"MOV");
    publish(&bot, &variant("", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();
    let fields = the_request(&bot, "sendDocument").await;
    assert_eq!(
        file(&fields, "document").filename.as_deref(),
        Some("file0.mov")
    );
}

#[tokio::test]
async fn video_carries_probed_fields() {
    let bot = Bot::start().await;
    bot.ok("sendVideo", sent(1)).await;
    let mut a = asset(MediaKind::Video, "video/mp4", 3, Some((1280, 720)));
    a.duration_ms = Some(12_001);
    stored(&bot, &a, b"MP4");
    publish(&bot, &variant("", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();
    let fields = the_request(&bot, "sendVideo").await;
    assert_eq!(text(&fields, "supports_streaming"), Some("true"));
    assert_eq!(text(&fields, "width"), Some("1280"));
    assert_eq!(text(&fields, "height"), Some("720"));
    assert_eq!(text(&fields, "duration"), Some("13"));
    assert_eq!(file(&fields, "video").data, b"MP4");
}

#[tokio::test]
async fn video_without_probe_omits_fields() {
    let bot = Bot::start().await;
    bot.ok("sendVideo", sent(1)).await;
    let a = asset(MediaKind::Video, "video/mp4", 3, None);
    stored(&bot, &a, b"MP4");
    publish(&bot, &variant("", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();
    let fields = the_request(&bot, "sendVideo").await;
    for name in ["width", "height", "duration"] {
        assert!(text(&fields, name).is_none(), "{name}");
    }
}

fn album_assets(bot: &Bot) -> Vec<MediaAsset> {
    let image = asset(MediaKind::Image, "image/jpeg", 3, Some((10, 10)));
    let video = asset(MediaKind::Video, "video/mp4", 3, Some((640, 360)));
    let png = asset(MediaKind::Image, "image/png", 3, Some((10, 10)));
    stored(bot, &image, b"JPG");
    stored(bot, &video, b"MP4");
    stored(bot, &png, b"PNG");
    vec![image, video, png]
}

#[tokio::test]
async fn album_shape() {
    let bot = Bot::start().await;
    bot.ok("sendMediaGroup", json!([sent(21), sent(22), sent(23)]))
        .await;
    let assets = album_assets(&bot);

    let outcome = publish(&bot, &variant("Caption", &assets, json!({})), &assets)
        .await
        .unwrap();

    let PublishOutcome::Published(receipt) = outcome else {
        panic!()
    };
    assert_eq!(receipt.remote_id.as_deref(), Some("21"));
    let fields = the_request(&bot, "sendMediaGroup").await;
    let media: Value = serde_json::from_str(text(&fields, "media").unwrap()).unwrap();
    let items = media.as_array().unwrap();
    assert_eq!(items.len(), 3);
    let kinds: Vec<_> = items.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["photo", "video", "photo"]);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(item["media"], format!("attach://file{i}"));
        assert_eq!(item.get("caption").is_some(), i == 0, "{i}");
    }
    assert_eq!(items[0]["caption"], "Caption");
    assert_eq!(items[1]["supports_streaming"], true);
    assert_eq!(file(&fields, "file0").data, b"JPG");
    assert_eq!(file(&fields, "file1").data, b"MP4");
    assert_eq!(file(&fields, "file2").data, b"PNG");
}

#[tokio::test]
async fn album_as_file_is_all_documents() {
    let bot = Bot::start().await;
    bot.ok("sendMediaGroup", json!([sent(1), sent(2), sent(3)]))
        .await;
    let assets = album_assets(&bot);
    publish(
        &bot,
        &variant("", &assets, json!({"as_file": true})),
        &assets,
    )
    .await
    .unwrap();
    let fields = the_request(&bot, "sendMediaGroup").await;
    let media: Value = serde_json::from_str(text(&fields, "media").unwrap()).unwrap();
    assert!(
        media
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["type"] == "document")
    );
}

#[tokio::test]
async fn migrated_album_reopens_media() {
    let bot = Bot::start().await;
    // The first request is refused as migrated; the second succeeds.
    Mock::given(method("POST"))
        .and(path(format!("/bot{TOKEN}/sendMediaGroup")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "ok": false,
            "error_code": 400,
            "description": "Bad Request: group chat was upgraded to a supergroup chat",
            "parameters": {"migrate_to_chat_id": -1009}
        })))
        .up_to_n_times(1)
        .mount(&bot.server)
        .await;
    bot.ok("sendMediaGroup", json!([sent(1), sent(2), sent(3)]))
        .await;
    let assets = album_assets(&bot);

    publish(&bot, &variant("", &assets, json!({})), &assets)
        .await
        .unwrap();

    let requests = bot.requests("sendMediaGroup").await;
    assert_eq!(requests.len(), 2);
    let second = multipart(&requests[1]);
    assert_eq!(text(&second, "chat_id"), Some("-1009"));
    assert_eq!(file(&second, "file0").data, b"JPG");
    assert_eq!(file(&second, "file1").data, b"MP4");
    assert_eq!(file(&second, "file2").data, b"PNG");
}

#[tokio::test]
async fn missing_media_stops_before_sending() {
    let bot = Bot::start().await;
    bot.ok("sendMediaGroup", json!([sent(1), sent(2), sent(3)]))
        .await;
    let assets = album_assets(&bot);
    let vanished = asset(MediaKind::Image, "image/jpeg", 3, None);
    let mut all = assets;
    all[2] = vanished; // never stored

    let err = publish(&bot, &variant("", &all, json!({})), &all)
        .await
        .unwrap_err();

    let AgentError::Validation(violations) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(violations[0].code, ViolationCode::MediaMissing);
    assert_eq!(violations[0].path.as_deref(), Some("media[2]"));
    assert_eq!(err.retry_class(), RetryClass::Permanent);
    assert!(bot.requests("sendMediaGroup").await.is_empty());
}

const SIZE: u64 = 40 * 1024 * 1024;

/// A store that makes `size` bytes on demand, one piece at a time, and counts the pieces.
struct Generated {
    size: u64,
    pulled: Arc<AtomicUsize>,
}

#[async_trait]
impl MediaStore for Generated {
    async fn open(&self, _asset: &MediaAsset) -> Result<ByteStream, MediaError> {
        const PIECE: u64 = 64 * 1024;
        let (size, pulled) = (self.size, Arc::clone(&self.pulled));
        Ok(futures::stream::unfold(0_u64, move |sent| {
            let pulled = Arc::clone(&pulled);
            async move {
                if sent >= size {
                    return None;
                }
                let n = PIECE.min(size - sent);
                pulled.fetch_add(1, Ordering::SeqCst);
                let piece = Bytes::from(vec![7_u8; usize::try_from(n).unwrap()]);
                Some((Ok(piece), sent + n))
            }
        })
        .boxed())
    }
}

#[tokio::test]
async fn upload_stays_streamed() {
    let bot = Bot::start().await;
    bot.ok("sendVideo", sent(1)).await;
    let a = asset(MediaKind::Video, "video/mp4", SIZE, Some((1920, 1080)));
    let pulled = Arc::new(AtomicUsize::new(0));
    let mut ctx = bot.ctx.agent_context(Value::Null);
    ctx.media_store = Arc::new(Generated {
        size: SIZE,
        pulled: Arc::clone(&pulled),
    });
    let client = bot.agent().create_client(ctx).unwrap();

    publish_with(client, &variant("", &[a.clone()], json!({})), &[a])
        .await
        .unwrap();

    // The whole file went out, pulled a piece at a time rather than built up front.
    let request = &bot.requests("sendVideo").await[0];
    let fields = multipart(request);
    assert_eq!(file(&fields, "video").data.len() as u64, SIZE);
    assert_eq!(pulled.load(Ordering::SeqCst) as u64, SIZE / (64 * 1024));
    assert!(request.headers.get("content-length").is_some());
}

fn slow_agent(bot: &Bot, upload_timeout: Duration) -> Box<dyn postit_core::AgentClient> {
    TelegramAgent::new(TelegramConfig {
        api_base: Url::parse(&bot.server.uri()).unwrap(),
        upload_timeout,
    })
    .create_client(bot.ctx.agent_context(Value::Null))
    .unwrap()
}

async fn delayed(bot: &Bot, method_name: &str, result: Value, delay: Duration) {
    bot.respond(
        method_name,
        ResponseTemplate::new(200)
            .set_body_json(json!({"ok": true, "result": result}))
            .set_delay(delay),
    )
    .await;
}

#[tokio::test]
async fn upload_timeout_applies() {
    let bot = Bot::start().await;
    delayed(&bot, "sendPhoto", sent(1), Duration::from_secs(3)).await;
    let a = asset(MediaKind::Image, "image/jpeg", 3, Some((10, 10)));
    stored(&bot, &a, b"JPG");

    let started = Instant::now();
    let err = publish_with(
        slow_agent(&bot, Duration::from_millis(500)),
        &variant("", &[a.clone()], json!({})),
        &[a],
    )
    .await
    .unwrap_err();

    assert_eq!(err.retry_class(), RetryClass::Ambiguous, "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn album_timeout_scales() {
    let bot = Bot::start().await;
    delayed(
        &bot,
        "sendMediaGroup",
        json!([sent(1), sent(2)]),
        Duration::from_millis(1_200),
    )
    .await;
    let assets: Vec<_> = album_assets(&bot).into_iter().take(2).collect();

    // Longer than one upload timeout, shorter than two.
    publish_with(
        slow_agent(&bot, Duration::from_millis(800)),
        &variant("", &assets, json!({})),
        &assets,
    )
    .await
    .unwrap();
}
