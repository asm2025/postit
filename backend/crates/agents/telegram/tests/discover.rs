//! Discovery, the account check, and the Bot API's error mapping on the read calls.
#![allow(
    clippy::unwrap_used,
    clippy::needless_pass_by_value,
    clippy::cloned_ref_to_slice_refs,
    clippy::assigning_clones,
    clippy::panic
)]

mod common;

use std::time::{Duration, Instant};

use common::{Bot, TOKEN, account, chat, fields, member_update};
use postit_core::{AccountStatus, Agent, AgentError, Discovered};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

async fn verify(bot: &Bot, chats: Option<&str>) -> Result<Discovered, AgentError> {
    bot.agent()
        .verify_credentials(&bot.ctx.check_context(Value::Null), &fields(chats))
        .await
}

fn invalid(field: &str) -> AgentError {
    AgentError::InvalidCredentials {
        field: Some(field.to_owned()),
    }
}

#[tokio::test]
async fn valid_token_lists_admin_chats() {
    let bot = Bot::start().await;
    bot.bot().await;
    let channel = chat(-1001, "channel", "News", Some("newsroom"));
    bot.ok(
        "getUpdates",
        json!([
            member_update(&channel, "administrator", Some(true)),
            member_update(
                &chat(-1002, "channel", "Muted", None),
                "administrator",
                Some(false)
            ),
            member_update(&chat(7, "private", "Someone", None), "member", None),
        ]),
    )
    .await;

    let found = verify(&bot, None).await.unwrap();

    assert_eq!(found.accounts.len(), 1);
    let account = &found.accounts[0];
    assert_eq!(account.remote_id, "-1001");
    assert_eq!(account.display_name, "News");
    assert_eq!(account.handle.as_deref(), Some("@newsroom"));
    assert!(account.avatar_url.is_none());
}

#[tokio::test]
async fn removed_then_lost_chat_is_not_offered() {
    let channel = chat(-1001, "channel", "News", None);
    let added = member_update(&channel, "administrator", Some(true));
    let left = member_update(&channel, "left", None);

    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([added.clone(), left.clone()]))
        .await;
    assert!(verify(&bot, None).await.unwrap().accounts.is_empty());

    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([left, added])).await;
    assert_eq!(verify(&bot, None).await.unwrap().accounts.len(), 1);
}

#[tokio::test]
async fn peek_is_read_only() {
    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([])).await;

    verify(&bot, None).await.unwrap();

    let bodies = bot.bodies("getUpdates").await;
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].get("offset").is_none(), "{}", bodies[0]);
    assert!(bodies[0].get("allowed_updates").is_none(), "{}", bodies[0]);
    assert_eq!(bodies[0]["timeout"], 0);
    assert_eq!(bodies[0]["limit"], 100);
}

#[tokio::test]
async fn peek_conflict_falls_back_to_named_chats() {
    let bot = Bot::start().await;
    bot.bot().await;
    bot.fail(
        "getUpdates",
        409,
        "Conflict: terminated by other getUpdates request",
    )
    .await;
    bot.ok(
        "getChat",
        chat(-1003, "channel", "Named", Some("namedchat")),
    )
    .await;
    bot.ok(
        "getChatMember",
        json!({"status": "administrator", "can_post_messages": true}),
    )
    .await;

    let found = verify(&bot, Some("@namedchat")).await.unwrap();
    assert_eq!(found.accounts.len(), 1);
    assert_eq!(found.accounts[0].remote_id, "-1003");

    // With nothing named, nothing is found, and that is not an error.
    assert!(verify(&bot, None).await.unwrap().accounts.is_empty());
}

#[tokio::test]
async fn named_chat_without_rights_is_refused() {
    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([])).await;
    bot.fail("getChat", 400, "Bad Request: chat not found")
        .await;
    assert_eq!(
        verify(&bot, Some("@namedchat")).await.unwrap_err(),
        invalid("chats")
    );

    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([])).await;
    bot.ok(
        "getChat",
        chat(-1003, "channel", "Named", Some("namedchat")),
    )
    .await;
    bot.ok("getChatMember", json!({"status": "member"})).await;
    assert_eq!(
        verify(&bot, Some("@namedchat")).await.unwrap_err(),
        invalid("chats")
    );
}

#[tokio::test]
async fn named_group_member_needs_admin() {
    let group = chat(-1004, "supergroup", "Team", None);
    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([])).await;
    bot.ok("getChat", group.clone()).await;
    bot.ok("getChatMember", json!({"status": "member"})).await;
    assert_eq!(
        verify(&bot, Some("-1004")).await.unwrap_err(),
        invalid("chats")
    );

    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getUpdates", json!([])).await;
    bot.ok("getChat", group).await;
    bot.ok("getChatMember", json!({"status": "administrator"}))
        .await;
    assert_eq!(verify(&bot, Some("-1004")).await.unwrap().accounts.len(), 1);
}

#[tokio::test]
async fn bad_token_is_invalid_credentials() {
    for status in [401, 404] {
        let bot = Bot::start().await;
        bot.fail("getMe", status, "Unauthorized").await;
        assert_eq!(
            verify(&bot, None).await.unwrap_err(),
            invalid("bot_token"),
            "{status}"
        );
    }
}

#[tokio::test]
async fn rate_limit_reads_the_body() {
    let bot = Bot::start().await;
    bot.respond(
        "getMe",
        ResponseTemplate::new(429).set_body_json(json!({
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests: retry after 7",
            "parameters": {"retry_after": 7}
        })),
    )
    .await;
    assert_eq!(
        verify(&bot, None).await.unwrap_err(),
        AgentError::RateLimited {
            retry_after: Some(Duration::from_secs(7))
        }
    );

    // With no JSON, the header wins.
    let bot = Bot::start().await;
    bot.respond(
        "getMe",
        ResponseTemplate::new(429)
            .insert_header("Retry-After", "3")
            .set_body_string("slow down"),
    )
    .await;
    assert_eq!(
        verify(&bot, None).await.unwrap_err(),
        AgentError::RateLimited {
            retry_after: Some(Duration::from_secs(3))
        }
    );
}

#[tokio::test]
async fn retry_after_header_is_not_slept() {
    let bot = Bot::start().await;
    bot.respond(
        "getMe",
        ResponseTemplate::new(429).insert_header("Retry-After", "600"),
    )
    .await;

    let started = Instant::now();
    let err = verify(&bot, None).await.unwrap_err();

    assert_eq!(
        err,
        AgentError::RateLimited {
            retry_after: Some(Duration::from_secs(600))
        }
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(bot.requests("getMe").await.len(), 1);
}

async fn active_chat(bot: &Bot) {
    bot.bot().await;
    bot.ok("getChat", chat(-1001, "channel", "News", Some("newsroom")))
        .await;
    bot.ok(
        "getChatMember",
        json!({"status": "administrator", "can_post_messages": true}),
    )
    .await;
}

#[tokio::test]
async fn account_info_active() {
    let bot = Bot::start().await;
    active_chat(&bot).await;

    let info = bot.client().account_info(&account("-1001")).await.unwrap();

    assert_eq!(info.status, AccountStatus::Active);
    assert_eq!(info.remote_id, "-1001");
    assert_eq!(info.display_name, "News");
    assert_eq!(info.handle.as_deref(), Some("@newsroom"));
}

#[tokio::test]
async fn account_info_has_no_avatar() {
    let bot = Bot::start().await;
    active_chat(&bot).await;
    let info = bot.client().account_info(&account("-1001")).await.unwrap();
    assert!(info.avatar_url.is_none());
}

#[tokio::test]
async fn account_info_lost_chat() {
    let bot = Bot::start().await;
    bot.bot().await;
    bot.fail(
        "getChat",
        403,
        "Forbidden: bot was kicked from the channel chat",
    )
    .await;
    let err = bot
        .client()
        .account_info(&account("-1001"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AgentError::AccountUnreachable { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn account_info_bad_token() {
    let bot = Bot::start().await;
    bot.fail("getMe", 401, "Unauthorized").await;
    let err = bot
        .client()
        .account_info(&account("-1001"))
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
}

#[tokio::test]
async fn account_info_not_admin() {
    let bot = Bot::start().await;
    bot.bot().await;
    bot.ok("getChat", chat(-1001, "channel", "News", None))
        .await;
    bot.ok("getChatMember", json!({"status": "member"})).await;
    let err = bot
        .client()
        .account_info(&account("-1001"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AgentError::AccountUnreachable { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn account_info_follows_a_migrated_group() {
    let bot = Bot::start().await;
    bot.bot().await;
    let on = |method_name: &str, chat_id: i64, template: ResponseTemplate| {
        Mock::given(method("POST"))
            .and(path(format!("/bot{TOKEN}/{method_name}")))
            .and(body_partial_json(json!({"chat_id": chat_id})))
            .respond_with(template)
    };
    on(
        "getChat",
        -100,
        ResponseTemplate::new(400).set_body_json(json!({
            "ok": false,
            "error_code": 400,
            "description": "Bad Request: group chat was upgraded to a supergroup chat",
            "parameters": {"migrate_to_chat_id": -1009}
        })),
    )
    .mount(&bot.server)
    .await;
    on(
        "getChat",
        -1009,
        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": chat(-1009, "supergroup", "Team", None)
        })),
    )
    .mount(&bot.server)
    .await;
    on(
        "getChatMember",
        -1009,
        ResponseTemplate::new(200)
            .set_body_json(json!({"ok": true, "result": {"status": "administrator"}})),
    )
    .mount(&bot.server)
    .await;

    let info = bot.client().account_info(&account("-100")).await.unwrap();

    assert_eq!(info.status, AccountStatus::Active);
    // The stored ID stays what the account was connected with.
    assert_eq!(info.remote_id, "-100");
}

#[tokio::test]
async fn peek_skips_unknown_updates() {
    let bot = Bot::start().await;
    bot.bot().await;
    let channel = chat(-1001, "channel", "News", None);
    bot.ok(
        "getUpdates",
        json!([
            {"update_id": 1, "message": {"message_id": 1}},
            {"update_id": 2, "edited_channel_post": {"message_id": 2}},
            member_update(&chat(-1002, "channel", "Odd", None), "brand_new_status", None),
            {"update_id": 3, "my_chat_member": {"chat": "not a chat"}},
            member_update(&channel, "administrator", Some(true)),
        ]),
    )
    .await;

    let found = verify(&bot, None).await.unwrap();

    assert_eq!(found.accounts.len(), 1);
    assert_eq!(found.accounts[0].remote_id, "-1001");
}

#[tokio::test]
async fn discover_accounts_matches_verify() {
    let bot = Bot::with_chats(Some("@namedchat")).await;
    bot.bot().await;
    bot.ok(
        "getUpdates",
        json!([member_update(
            &chat(-1001, "channel", "News", Some("newsroom")),
            "administrator",
            Some(true)
        )]),
    )
    .await;
    bot.ok(
        "getChat",
        chat(-1003, "channel", "Named", Some("namedchat")),
    )
    .await;
    bot.ok(
        "getChatMember",
        json!({"status": "administrator", "can_post_messages": true}),
    )
    .await;

    let verified = verify(&bot, Some("@namedchat")).await.unwrap();
    let discovered = bot.client().discover_accounts().await.unwrap();

    assert_eq!(verified.accounts.len(), 2);
    assert_eq!(verified, discovered);
}
