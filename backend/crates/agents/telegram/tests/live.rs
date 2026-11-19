//! A smoke test against the real Telegram Bot API.
//!
//! THIS POSTS FOR REAL. It sends a text, an image, and a two-item album to the chat named by
//! `POSTIT_TELEGRAM_CHAT`, as the bot whose token is in `POSTIT_TELEGRAM_TOKEN`. It runs only
//! with `POSTIT_LIVE_TESTS=1`, and only when asked for: `cargo test -p postit-agent-telegram
//! --test live -- --ignored --nocapture`.
//!
//! `live_text_image_and_album` is the quick check. `live_formats` sends every format the agent
//! accepts, one post each, and prints each receipt link: a right-to-left caption, a PNG, a WebP,
//! an animated GIF, an MP4, a MOV sent as a file, and a ten-item album. It reports every case
//! that failed, not only the first. A person judges the result in Telegram on a phone and on
//! the desktop app.
//!
//! `live_text_limits` and `live_large_media` probe Telegram's limits and print what it accepted;
//! they do not fail on a refusal. `live_large_media` uploads about 540 MB by default.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use async_trait::async_trait;
use bytes::Bytes;
use common::account;
use postit_agent_kit::testkit::TestContext;
use postit_agent_telegram::credentials::{self, ChatRef};
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_core::{
    Agent, AgentClient, AgentError, CredentialFields, Credentials, DeliveryId, DiscoveredAccount,
    MediaAsset, MediaAssetId, MediaAssetState, MediaItem, MediaKind, PostKind, PublishMode,
    PublishOutcome, PublishProgress, PublishRequest, UserId, Variant,
};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// Progress for a publish nobody cancels.
struct NoCancel;

#[async_trait]
impl PublishProgress for NoCancel {
    async fn save_container_ref(&self, _container_ref: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        false
    }
}

/// The account for the chat the user named, and no other: discovery also lists the chats the
/// bot's updates show, and a live test must never post to one of those by accident.
fn named_account<'a>(
    accounts: &'a [DiscoveredAccount],
    chat: &ChatRef,
) -> Option<&'a DiscoveredAccount> {
    accounts.iter().find(|account| match chat {
        ChatRef::Id(id) => account.remote_id == id.to_string(),
        ChatRef::Username(name) => account
            .handle
            .as_deref()
            .and_then(|handle| handle.strip_prefix('@'))
            .is_some_and(|handle| handle.eq_ignore_ascii_case(name)),
    })
}

#[test]
fn named_account_never_falls_back_to_another_chat() {
    let found = |remote_id: &str, handle: Option<&str>| DiscoveredAccount {
        remote_id: remote_id.to_owned(),
        display_name: "A chat".to_owned(),
        handle: handle.map(str::to_owned),
        avatar_url: None,
    };
    let accounts = [
        found("-1001", Some("@other")),
        found("-1002", Some("@Mine")),
    ];

    let pick = |chat: ChatRef| named_account(&accounts, &chat).map(|a| a.remote_id.as_str());
    assert_eq!(pick(ChatRef::Username("mine".to_owned())), Some("-1002"));
    assert_eq!(pick(ChatRef::Id(-1002)), Some("-1002"));
    assert_eq!(pick(ChatRef::Username("absent".to_owned())), None);
    assert_eq!(pick(ChatRef::Id(-1003)), None);
}

const MP4: &[u8] = include_bytes!("../../../media/tests/fixtures/landscape.mp4");
const MOV: &[u8] = include_bytes!("../../../media/tests/fixtures/portrait.mov");

fn gradient(shade: u8) -> image::RgbaImage {
    // Large enough for Telegram to take as a photo; the color differs per item.
    image::RgbaImage::from_fn(320, 240, |x, y| {
        image::Rgba([
            shade,
            u8::try_from(x % 256).unwrap(),
            u8::try_from(y % 256).unwrap(),
            255,
        ])
    })
}

fn encode(img: &image::RgbaImage, format: image::ImageFormat) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn png(shade: u8) -> Vec<u8> {
    encode(&gradient(shade), image::ImageFormat::Png)
}

/// A GIF of a few frames, so Telegram has something to play.
fn animated_gif() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for shade in [0_u8, 60, 120, 180, 240] {
            encoder
                .encode_frame(image::Frame::new(gradient(shade)))
                .unwrap();
        }
    }
    out
}

fn asset(kind: MediaKind, mime: &str, bytes: u64, dims: (u32, u32), ms: Option<u64>) -> MediaAsset {
    MediaAsset {
        id: MediaAssetId::from_uuid(Uuid::now_v7()),
        owner_id: UserId::from_uuid(Uuid::now_v7()),
        kind,
        mime: mime.to_owned(),
        bytes,
        width: Some(dims.0),
        height: Some(dims.1),
        duration_ms: ms,
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

/// The live tests share one chat, so they take turns: interleaved posts are hard to judge.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A post to send: what it shows in the log, its text, its files, and its options.
type Case = (&'static str, String, Vec<MediaAsset>, Map<String, Value>);

/// A connected bot and the one chat it posts to.
struct Live {
    _turn: tokio::sync::MutexGuard<'static, ()>,
    test: TestContext,
    client: Box<dyn AgentClient>,
    target: String,
}

impl Live {
    /// Connects as the bot in `POSTIT_TELEGRAM_TOKEN` to the chat in `POSTIT_TELEGRAM_CHAT`.
    /// `None` unless `POSTIT_LIVE_TESTS=1`.
    async fn connect() -> Option<Self> {
        if std::env::var("POSTIT_LIVE_TESTS").as_deref() != Ok("1") {
            return None;
        }
        let turn = ONE_AT_A_TIME.lock().await;
        let token = std::env::var("POSTIT_TELEGRAM_TOKEN").expect("POSTIT_TELEGRAM_TOKEN");
        let chat = std::env::var("POSTIT_TELEGRAM_CHAT").expect("POSTIT_TELEGRAM_CHAT");

        let secret = BTreeMap::from([("bot_token".to_owned(), SecretString::from(token))]);
        let mut plain = Map::new();
        plain.insert("chats".to_owned(), json!(chat));
        let fields = CredentialFields::from_parts(plain, secret);
        let test = TestContext::new().with_credentials(Credentials::Fields(fields.clone()));
        let agent = TelegramAgent::new(TelegramConfig::default());

        // Connect: the named chat is found and the bot may post there.
        let found = agent
            .verify_credentials(&test.check_context(Value::Null), &fields)
            .await
            .unwrap();
        let named = credentials::parse(&fields).unwrap().chats;
        let [named] = named.as_slice() else {
            panic!("POSTIT_TELEGRAM_CHAT must name exactly one chat");
        };
        let target = named_account(&found.accounts, named)
            .expect("the named chat is found")
            .remote_id
            .clone();
        let client = agent
            .create_client(test.agent_context(Value::Null))
            .unwrap();
        client.account_info(&account(&target)).await.unwrap();
        Some(Self {
            _turn: turn,
            test,
            client,
            target,
        })
    }

    /// Puts the bytes in the media store and returns their asset.
    fn store(
        &self,
        kind: MediaKind,
        mime: &str,
        dims: (u32, u32),
        ms: Option<u64>,
        bytes: &[u8],
    ) -> MediaAsset {
        self.store_shared(kind, mime, dims, ms, &Bytes::copy_from_slice(bytes))
    }

    /// Like `store`, for bytes already shared, so ten large assets can hold one buffer.
    fn store_shared(
        &self,
        kind: MediaKind,
        mime: &str,
        dims: (u32, u32),
        ms: Option<u64>,
        bytes: &Bytes,
    ) -> MediaAsset {
        let a = asset(kind, mime, bytes.len() as u64, dims, ms);
        self.test
            .media_store
            .insert_owned(a.owner_id, a.id, bytes.clone());
        a
    }

    fn image(&self, mime: &str, bytes: &[u8]) -> MediaAsset {
        self.store(MediaKind::Image, mime, (320, 240), None, bytes)
    }

    /// Publishes one post live and returns its receipt link.
    async fn send(
        &self,
        text: &str,
        media: &[MediaAsset],
        options: Map<String, Value>,
    ) -> Result<Option<String>, String> {
        let variant = Variant {
            target: account(&self.target),
            kind: PostKind::Post,
            text: text.to_owned(),
            media: media
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
        };
        let outcome = self
            .client
            .publish(
                PublishRequest {
                    variant: &variant,
                    assets: media,
                    mode: PublishMode::Live,
                    native_publish_at: None,
                    container_ref: None,
                    idempotency_key: DeliveryId::from_uuid(Uuid::now_v7()),
                },
                &NoCancel,
            )
            .await
            .map_err(|err| format!("{err:?}"))?;
        let PublishOutcome::Published(receipt) = outcome else {
            return Err(format!("{outcome:?}"));
        };
        if receipt.remote_id.is_none() {
            return Err("no message ID".to_owned());
        }
        Ok(receipt.remote_url.map(|u| u.to_string()))
    }
}

#[tokio::test]
#[ignore = "posts to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_text_image_and_album() {
    let Some(live) = Live::connect().await else {
        return;
    };
    let assets: Vec<_> = [10_u8, 200]
        .into_iter()
        .map(|shade| live.image("image/png", &png(shade)))
        .collect();
    for (what, media) in [
        ("text", &assets[..0]),
        ("image", &assets[..1]),
        ("album", &assets[..]),
    ] {
        let link = live
            .send(&format!("postit live test: {what}"), media, Map::new())
            .await
            .unwrap_or_else(|err| panic!("{what}: {err}"));
        println!("{what}: {link:?}");
    }
}

/// Every format the agent accepts, one post each. Check each on a phone and on the desktop app.
#[tokio::test]
#[ignore = "posts to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_formats() {
    let Some(live) = Live::connect().await else {
        return;
    };
    let as_file = Map::from_iter([("as_file".to_owned(), json!(true))]);
    let webp = live.image(
        "image/webp",
        &encode(&gradient(90), image::ImageFormat::WebP),
    );
    let gif = live.image("image/gif", &animated_gif());
    let mp4 = live.store(MediaKind::Video, "video/mp4", (320, 240), Some(1_000), MP4);
    let mov = live.store(
        MediaKind::Video,
        "video/quicktime",
        (240, 320),
        Some(1_000),
        MOV,
    );
    let ten: Vec<_> = (0..10_u8)
        .map(|i| live.image("image/png", &png(i * 25)))
        .collect();

    let cases: Vec<Case> = vec![
        (
            "right-to-left caption",
            "postit live test: \u{627}\u{644}\u{633}\u{644}\u{627}\u{645} \u{5E9}\u{5DC}\u{5D5}\u{5DD} rtl"
                .to_owned(),
            vec![],
            Map::new(),
        ),
        (
            "png",
            "postit live test: png".to_owned(),
            vec![live.image("image/png", &png(30))],
            Map::new(),
        ),
        (
            "webp",
            "postit live test: webp".to_owned(),
            vec![webp],
            Map::new(),
        ),
        (
            "gif (plays as an animation)",
            "postit live test: gif".to_owned(),
            vec![gif],
            Map::new(),
        ),
        (
            "mp4 (plays as a video)",
            "postit live test: mp4".to_owned(),
            vec![mp4],
            Map::new(),
        ),
        (
            "mov sent as a file",
            "postit live test: mov as a file".to_owned(),
            vec![mov],
            as_file,
        ),
        (
            "album of 10",
            "postit live test: album of 10".to_owned(),
            ten,
            Map::new(),
        ),
    ];

    let mut failed = Vec::new();
    for (what, text, media, options) in cases {
        match live.send(&text, &media, options).await {
            Ok(link) => println!("ok   {what}: {link:?}"),
            Err(err) => {
                println!("FAIL {what}: {err}");
                failed.push(what);
            }
        }
    }
    assert!(failed.is_empty(), "cases that failed: {failed:?}");
}

/// What a probe of Telegram's limits found, for the log.
fn verdict(result: &Result<Option<String>, String>, took: std::time::Duration) -> String {
    match result {
        Ok(link) => format!("accepted in {took:.1?}: {link:?}"),
        Err(err) => format!("not accepted after {took:.1?}: {err}"),
    }
}

/// A valid MP4 grown to `total` bytes by a trailing `free` box, which players skip. Real video
/// at this size would only slow the test; Telegram's limits count bytes.
fn padded_mp4(total: usize) -> Bytes {
    let pad = total
        .checked_sub(MP4.len())
        .filter(|pad| *pad >= 8)
        .expect("the size is larger than the base MP4");
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(MP4);
    out.extend_from_slice(&u32::try_from(pad).expect("under 4 GB").to_be_bytes());
    out.extend_from_slice(b"free");
    out.resize(total, 0);
    Bytes::from(out)
}

/// The size in megabytes from the environment, or `default`.
fn megabytes(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Text at the 4,096 limit: does Telegram count UTF-16 units (the agent's rule) or characters?
/// A probe, not a pass or fail: it prints what Telegram accepted. Each emoji is one character
/// and two UTF-16 units, so 4,096 of them are 8,192 units.
#[tokio::test]
#[ignore = "posts to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_text_limits() {
    let Some(live) = Live::connect().await else {
        return;
    };
    // The agent's own check is skipped on purpose: `publish` sends what it is given.
    let emoji = |n: usize| "\u{1F600}".repeat(n);
    for (what, text) in [
        ("4096 ASCII characters (the baseline)", "a".repeat(4_096)),
        ("4096 emoji, 8192 UTF-16 units", emoji(4_096)),
        ("4097 emoji, one over in characters", emoji(4_097)),
    ] {
        let started = std::time::Instant::now();
        let result = live.send(&text, &[], Map::new()).await;
        println!("{what}: {}", verdict(&result, started.elapsed()));
    }
}

/// A video near the 50 MB file limit and an album of ten. Sizes are in decimal megabytes:
/// `POSTIT_LIVE_VIDEO_MB` (default 49) and `POSTIT_LIVE_ALBUM_VIDEO_MB` (default 49, so the
/// album uploads ten times that; lower it on a slow uplink). A probe, not a pass or fail.
#[tokio::test]
#[ignore = "uploads hundreds of megabytes to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_large_media() {
    let Some(live) = Live::connect().await else {
        return;
    };
    let video = |bytes: &Bytes| {
        live.store_shared(
            MediaKind::Video,
            "video/mp4",
            (320, 240),
            Some(1_000),
            bytes,
        )
    };

    let single_mb = megabytes("POSTIT_LIVE_VIDEO_MB", 49);
    let single = padded_mp4(single_mb * 1_000_000);
    let started = std::time::Instant::now();
    let result = live
        .send(
            &format!("postit live test: {single_mb} MB video"),
            &[video(&single)],
            Map::new(),
        )
        .await;
    println!(
        "{single_mb} MB video: {}",
        verdict(&result, started.elapsed())
    );
    drop(single);

    let each_mb = megabytes("POSTIT_LIVE_ALBUM_VIDEO_MB", 49);
    let one = padded_mp4(each_mb * 1_000_000);
    // One buffer, shared by ten assets.
    let ten: Vec<_> = (0..10).map(|_| video(&one)).collect();
    let started = std::time::Instant::now();
    let result = live
        .send(
            &format!("postit live test: album of ten {each_mb} MB videos"),
            &ten,
            Map::new(),
        )
        .await;
    println!(
        "album of ten {each_mb} MB videos ({} MB in one request): {}",
        each_mb * 10,
        verdict(&result, started.elapsed())
    );
}

/// Where Telegram's limit on one album request lies. It refused ten 49 MB videos at once with
/// `413`, before reading the body. Tries nine items, then eight, and so on down, and stops at
/// the first it accepts, so a refusal costs nothing and only that one album uploads. Each item
/// is `POSTIT_LIVE_ALBUM_VIDEO_MB` (default 49). A probe, not a pass or fail.
#[tokio::test]
#[ignore = "uploads up to hundreds of megabytes to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_album_total_limit() {
    let Some(live) = Live::connect().await else {
        return;
    };
    let each_mb = megabytes("POSTIT_LIVE_ALBUM_VIDEO_MB", 49);
    let one = padded_mp4(each_mb * 1_000_000);
    for count in (2..=9).rev() {
        let items: Vec<_> = (0..count)
            .map(|_| {
                live.store_shared(MediaKind::Video, "video/mp4", (320, 240), Some(1_000), &one)
            })
            .collect();
        let started = std::time::Instant::now();
        let result = live
            .send(
                &format!("postit live test: album of {count} {each_mb} MB videos"),
                &items,
                Map::new(),
            )
            .await;
        println!(
            "album of {count} x {each_mb} MB ({} MB in one request): {}",
            count * each_mb,
            verdict(&result, started.elapsed())
        );
        if result.is_ok() {
            return;
        }
    }
}
