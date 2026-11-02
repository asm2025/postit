//! A smoke test against the real Telegram Bot API.
//!
//! THIS POSTS FOR REAL. It sends a text, an image, and a two-item album to the chat named by
//! `POSTIT_TELEGRAM_CHAT`, as the bot whose token is in `POSTIT_TELEGRAM_TOKEN`. It runs only
//! with `POSTIT_LIVE_TESTS=1`, and only when asked for: `cargo test -p postit-agent-telegram
//! --test live -- --ignored`.
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
    Agent, AgentError, CredentialFields, Credentials, DeliveryId, DiscoveredAccount, MediaAsset,
    MediaAssetId, MediaAssetState, MediaItem, MediaKind, PostKind, PublishMode, PublishOutcome,
    PublishProgress, PublishRequest, UserId, Variant,
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

fn png(shade: u8) -> Vec<u8> {
    // Large enough for Telegram to take as a photo; the color differs per item.
    let img = image::RgbaImage::from_pixel(320, 240, image::Rgba([shade, 120, 200, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
#[ignore = "posts to a real Telegram chat; set POSTIT_LIVE_TESTS=1"]
async fn live_text_image_and_album() {
    if std::env::var("POSTIT_LIVE_TESTS").as_deref() != Ok("1") {
        return;
    }
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

    let mut assets = Vec::new();
    for shade in [10_u8, 200] {
        let bytes = png(shade);
        let a = asset(bytes.len() as u64);
        test.media_store
            .insert_owned(a.owner_id, a.id, Bytes::from(bytes));
        assets.push(a);
    }
    let send = |text: &str, media: &[MediaAsset]| Variant {
        target: account(&target),
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
        options: Map::new(),
    };
    for (what, variant, media) in [
        ("text", send("postit live test: text", &[]), &assets[..0]),
        (
            "image",
            send("postit live test: image", &assets[..1]),
            &assets[..1],
        ),
        (
            "album",
            send("postit live test: album", &assets),
            &assets[..],
        ),
    ] {
        let outcome = client
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
            .unwrap();
        let PublishOutcome::Published(receipt) = outcome else {
            panic!("{what}: {outcome:?}")
        };
        assert!(receipt.remote_id.is_some(), "{what}: no message ID");
        println!("{what}: {:?}", receipt.remote_url.map(|u| u.to_string()));
    }
}

fn asset(bytes: u64) -> MediaAsset {
    MediaAsset {
        id: MediaAssetId::from_uuid(Uuid::now_v7()),
        owner_id: UserId::from_uuid(Uuid::now_v7()),
        kind: MediaKind::Image,
        mime: "image/png".to_owned(),
        bytes,
        width: Some(320),
        height: Some(240),
        duration_ms: None,
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}
