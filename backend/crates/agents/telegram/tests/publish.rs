//! Validating and publishing text posts.
#![allow(
    clippy::unwrap_used,
    clippy::needless_pass_by_value,
    clippy::cloned_ref_to_slice_refs,
    clippy::assigning_clones,
    clippy::panic
)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use common::{Bot, TOKEN, account};
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_core::{
    Agent, AgentError, DeliveryId, PostKind, PublishMode, PublishOutcome, PublishProgress,
    PublishReceipt, PublishRequest, ReconcileRequest, RetryClass, Variant,
};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

struct Progress(AtomicBool);

#[async_trait]
impl PublishProgress for Progress {
    async fn save_container_ref(&self, _container_ref: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn progress() -> Progress {
    Progress(AtomicBool::new(false))
}

fn variant(text: &str, options: Value) -> Variant {
    Variant {
        target: account("-1001234567890"),
        kind: PostKind::Post,
        text: text.to_owned(),
        media: Vec::new(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: options.as_object().cloned().unwrap_or_default(),
    }
}

fn request(variant: &Variant, mode: PublishMode) -> PublishRequest<'_> {
    PublishRequest {
        variant,
        assets: &[],
        mode,
        native_publish_at: None,
        container_ref: None,
        idempotency_key: DeliveryId::from_uuid(Uuid::nil()),
    }
}

fn message(id: i64, chat: Value) -> Value {
    json!({"message_id": id, "chat": chat, "date": 1})
}

fn public_channel() -> Value {
    json!({"id": -1_001_234_567_890_i64, "type": "channel", "username": "newsroom"})
}

async fn publish(bot: &Bot, variant: &Variant) -> Result<PublishOutcome, AgentError> {
    bot.client()
        .publish(request(variant, PublishMode::Live), &progress())
        .await
}

fn published(outcome: PublishOutcome) -> PublishReceipt {
    match outcome {
        PublishOutcome::Published(receipt) => receipt,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn text_post_is_plain_text() {
    let bot = Bot::start().await;
    bot.ok("sendMessage", message(42, public_channel())).await;

    let receipt = published(
        publish(&bot, &variant("Hello *world*", json!({})))
            .await
            .unwrap(),
    );

    assert_eq!(receipt.remote_id.as_deref(), Some("42"));
    let bodies = bot.bodies("sendMessage").await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["chat_id"], json!(-1_001_234_567_890_i64));
    assert_eq!(bodies[0]["text"], "Hello *world*");
    assert!(bodies[0].get("parse_mode").is_none());
}

#[tokio::test]
async fn text_carries_link_and_tags() {
    let bot = Bot::start().await;
    bot.ok("sendMessage", message(1, public_channel())).await;
    let mut v = variant("Hi", json!({}));
    v.link = Some(Url::parse("https://a.example/").unwrap());
    v.tags = vec!["news".to_owned()];

    publish(&bot, &v).await.unwrap();

    assert_eq!(
        bot.bodies("sendMessage").await[0]["text"],
        "Hi\n\nhttps://a.example/\n\n#news"
    );
}

#[tokio::test]
async fn options_reach_the_call() {
    let bot = Bot::start().await;
    bot.ok("sendMessage", message(1, public_channel())).await;
    let all = json!({"silent": true, "protect_content": true, "no_link_preview": true});

    publish(&bot, &variant("a", all)).await.unwrap();
    publish(&bot, &variant("b", json!({}))).await.unwrap();

    let bodies = bot.bodies("sendMessage").await;
    assert_eq!(bodies[0]["disable_notification"], true);
    assert_eq!(bodies[0]["protect_content"], true);
    assert_eq!(bodies[0]["link_preview_options"]["is_disabled"], true);
    for key in [
        "disable_notification",
        "protect_content",
        "link_preview_options",
    ] {
        assert!(bodies[1].get(key).is_none(), "{key}");
    }
}

#[tokio::test]
async fn receipt_links() {
    let cases = [
        (public_channel(), Some("https://t.me/newsroom/42")),
        (
            json!({"id": -1_001_234_567_890_i64, "type": "channel"}),
            Some("https://t.me/c/1234567890/42"),
        ),
        (json!({"id": -555, "type": "group"}), None),
    ];
    for (chat, want) in cases {
        let bot = Bot::start().await;
        bot.ok("sendMessage", message(42, chat)).await;
        let receipt = published(publish(&bot, &variant("x", json!({}))).await.unwrap());
        assert_eq!(receipt.remote_url.as_ref().map(Url::as_str), want);
    }
}

#[tokio::test]
async fn dry_run_makes_no_call() {
    let bot = Bot::start().await;
    let v = variant("x", json!({}));

    let outcome = bot
        .client()
        .publish(request(&v, PublishMode::DryRun), &progress())
        .await
        .unwrap();

    assert!(
        matches!(outcome, PublishOutcome::Simulated { .. }),
        "{outcome:?}"
    );
    assert!(bot.server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancel_before_send_makes_no_call() {
    let bot = Bot::start().await;
    let v = variant("x", json!({}));

    let err = bot
        .client()
        .publish(
            request(&v, PublishMode::Live),
            &Progress(AtomicBool::new(true)),
        )
        .await
        .unwrap_err();

    assert_eq!(err, AgentError::Cancelled);
    assert!(bot.server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn lost_response_is_outcome_unknown() {
    let bot = Bot::start().await;
    bot.respond(
        "sendMessage",
        ResponseTemplate::new(200)
            .set_body_json(json!({"ok": true, "result": message(1, public_channel())}))
            .set_delay(Duration::from_secs(4)),
    )
    .await;

    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();

    assert_eq!(err.retry_class(), RetryClass::Ambiguous, "{err:?}");
}

#[tokio::test]
async fn server_error_after_send_is_outcome_unknown() {
    let bot = Bot::start().await;
    bot.fail("sendMessage", 502, "Bad Gateway").await;
    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Ambiguous, "{err:?}");
    assert_eq!(bot.requests("sendMessage").await.len(), 1);
}

#[tokio::test]
async fn unreadable_success_is_outcome_unknown() {
    let bot = Bot::start().await;
    bot.respond(
        "sendMessage",
        ResponseTemplate::new(200).set_body_string("<html>"),
    )
    .await;
    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();
    assert!(matches!(err, AgentError::OutcomeUnknown { .. }), "{err:?}");
}

#[tokio::test]
async fn refused_connection_stays_retryable() {
    let bot = Bot::start().await;
    // A port nothing listens on: the connection fails before anything is sent.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let agent = TelegramAgent::new(TelegramConfig {
        api_base: Url::parse(&format!("http://127.0.0.1:{closed}")).unwrap(),
        ..TelegramConfig::default()
    });
    let client = agent
        .create_client(bot.ctx.agent_context(Value::Null))
        .unwrap();
    let v = variant("x", json!({}));

    let err = client
        .publish(request(&v, PublishMode::Live), &progress())
        .await
        .unwrap_err();

    assert_eq!(err.retry_class(), RetryClass::Retryable, "{err:?}");
}

#[tokio::test]
async fn rate_limit_on_send_stays_retryable() {
    let bot = Bot::start().await;
    bot.respond(
        "sendMessage",
        ResponseTemplate::new(429).set_body_json(json!({
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests",
            "parameters": {"retry_after": 5}
        })),
    )
    .await;

    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();

    assert_eq!(
        err.retry_class(),
        RetryClass::RetryAfter(Duration::from_secs(5))
    );
    assert_eq!(bot.requests("sendMessage").await.len(), 1);
}

#[tokio::test]
async fn bad_request_is_permanent() {
    let bot = Bot::start().await;
    bot.fail("sendMessage", 400, "Bad Request: message is too long")
        .await;
    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Permanent, "{err:?}");
}

#[tokio::test]
async fn a_revoked_token_needs_a_reconnect() {
    let bot = Bot::start().await;
    bot.fail("sendMessage", 401, "Unauthorized").await;
    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
}

#[tokio::test]
async fn a_removed_bot_is_an_unreachable_account() {
    let bot = Bot::start().await;
    bot.fail(
        "sendMessage",
        403,
        "Forbidden: bot was kicked from the channel chat",
    )
    .await;
    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();
    assert!(
        matches!(err, AgentError::AccountUnreachable { .. }),
        "{err:?}"
    );
}

fn migrated(to: i64) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(json!({
        "ok": false,
        "error_code": 400,
        "description": "Bad Request: group chat was upgraded to a supergroup chat",
        "parameters": {"migrate_to_chat_id": to}
    }))
}

async fn on_chat(bot: &Bot, chat_id: i64, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("/bot{TOKEN}/sendMessage")))
        .and(body_partial_json(json!({"chat_id": chat_id})))
        .respond_with(template)
        .mount(&bot.server)
        .await;
}

#[tokio::test]
async fn migrated_group_repeats_once() {
    let bot = Bot::start().await;
    on_chat(&bot, -1_001_234_567_890, migrated(-1_009)).await;
    on_chat(
        &bot,
        -1_009,
        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": message(7, json!({"id": -1009, "type": "supergroup"}))
        })),
    )
    .await;

    let receipt = published(publish(&bot, &variant("x", json!({}))).await.unwrap());

    assert_eq!(receipt.remote_id.as_deref(), Some("7"));
    assert_eq!(bot.requests("sendMessage").await.len(), 2);
}

#[tokio::test]
async fn a_second_migration_is_permanent() {
    let bot = Bot::start().await;
    on_chat(&bot, -1_001_234_567_890, migrated(-1_009)).await;
    on_chat(&bot, -1_009, migrated(-1_010)).await;

    let err = publish(&bot, &variant("x", json!({}))).await.unwrap_err();

    assert_eq!(err.retry_class(), RetryClass::Permanent, "{err:?}");
    assert_eq!(bot.requests("sendMessage").await.len(), 2);
}

#[tokio::test]
async fn reconcile_is_unsupported() {
    let bot = Bot::start().await;
    let v = variant("x", json!({}));
    let err = bot
        .client()
        .reconcile(ReconcileRequest {
            variant: &v,
            container_ref: None,
            attempted_at: Utc::now(),
            idempotency_key: DeliveryId::from_uuid(Uuid::nil()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Unsupported { .. }), "{err:?}");
}

#[tokio::test]
async fn status_needs_no_call_and_revoke_is_unsupported() {
    let bot = Bot::start().await;
    bot.ok("sendMessage", message(42, public_channel())).await;
    let receipt = published(publish(&bot, &variant("x", json!({}))).await.unwrap());
    let client = bot.client();

    let outcome = client
        .status(&account("-1001234567890"), &receipt)
        .await
        .unwrap();
    assert_eq!(outcome, PublishOutcome::Published(receipt));
    assert_eq!(bot.server.received_requests().await.unwrap().len(), 1);

    let err = client.revoke().await.unwrap_err();
    assert!(matches!(err, AgentError::Unsupported { .. }), "{err:?}");
}

#[tokio::test]
async fn validate_reports_text_made_too_long_by_tags() {
    let bot = Bot::start().await;
    let mut v = variant(&"a".repeat(4_090), json!({}));
    v.tags = vec!["longtag".to_owned()];

    let found = bot.client().validate(&v, &[]).await.unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].params["max"], 4_096);
}
