//! Telegram through the engine: an account connected through `AccountService`, posts created
//! and published through `PublishingService`, real files in the real media store, against a
//! fake Bot API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value
)]

mod campaign;

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_config::{Environment, Settings};
use postit_core::{
    AccountId, AgentRegistry, MediaAssetId, PlatformId, PostId, PostKind, PostVersion, UserId,
};
use postit_data::OwnerScope;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::posts::{NewPost, PostMediaItem, PostRepo};
use postit_data::publishing::Dispatch;
use postit_data::testkit::seed_user;
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use postit_oauth::AccountService;
use postit_publishing::sweep::run_sweep;
use postit_publishing::{PublishDelivery, PublishInput, PublishingService, run_publish};
use postit_server::compose;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const TOKEN: &str = "123456:ABC-def_1";
const BOT_ID: i64 = 1000;

fn telegram() -> PlatformId {
    PlatformId::new("telegram").unwrap()
}

fn sent(id: i64) -> Value {
    json!({
        "message_id": id,
        "chat": {"id": -1_001_234_567_890_i64, "type": "channel", "username": "newsroom"},
        "date": 1
    })
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap_or(Value::Null)
}

/// Answers `getChat` for whatever chat is asked about: a channel with that ID.
struct AnyChat;

impl Respond for AnyChat {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let id = body_of(request)["chat_id"].clone();
        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"id": id, "type": "channel", "title": format!("Channel {id}")}
        }))
    }
}

fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": result}))
}

/// A Bot API that knows the bot and accepts every chat the bot is asked about.
async fn bot_api() -> MockServer {
    let server = MockServer::start().await;
    bot_api_mocks(&server).await;
    server
}

/// Mounts the read calls: the bot, no updates, and any chat with the bot as its admin.
async fn bot_api_mocks(server: &MockServer) {
    on_send(
        server,
        "getMe",
        ok(json!({"id": BOT_ID, "is_bot": true, "username": "postit_bot"})),
    )
    .await;
    on_send(server, "getUpdates", ok(json!([]))).await;
    on_send(
        server,
        "getChatMember",
        ok(json!({"status": "administrator", "can_post_messages": true})),
    )
    .await;
    Mock::given(method("POST"))
        .and(path_regex("^/bot[^/]+/getChat$"))
        .respond_with(AnyChat)
        .mount(server)
        .await;
}

async fn on_send(server: &MockServer, name: &str, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path_regex(format!("^/bot[^/]+/{name}$")))
        .respond_with(template)
        .mount(server)
        .await;
}

async fn requests(server: &MockServer, name: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().ends_with(&format!("/{name}")))
        .collect()
}

struct Fx {
    pool: PgPool,
    accounts: AccountService,
    publishing: PublishingService,
    media: compose::Media,
    scope: OwnerScope,
    owner: UserId,
    _dir: tempfile::TempDir,
    _media: tempfile::TempDir,
}

async fn fixture(pool: &PgPool, api_base: &str, owner: Option<UserId>) -> Fx {
    let media_dir = tempfile::tempdir().unwrap();
    let dir = campaign::config_dir(media_dir.path());
    let settings: Settings = campaign::settings(&dir);

    let mut agents = AgentRegistry::new();
    agents
        .register(Arc::new(TelegramAgent::new(TelegramConfig {
            api_base: Url::parse(api_base).unwrap(),
            upload_timeout: Duration::from_secs(30),
        })))
        .unwrap();
    let registered = vec![(telegram(), "Telegram".to_owned())];
    PlatformRepo::upsert_registered(pool, &registered)
        .await
        .unwrap();
    let configs = postit_server::platforms::resolve_platform_configs(&settings, &agents).unwrap();
    let ids = compose::system_ids();
    let clock = compose::system_clock();
    let media = compose::media_service(&settings, pool, &ids, &clock).unwrap();
    let http = postit_http::build_client(&settings.http).unwrap();
    let vault = postit_vault::Vault::from_settings(&settings.vault).unwrap();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
    let outbox = MailOutbox::new(jobs.clone());
    let notifier = compose::notifier(&settings, &ids, &clock, &jobs, &outbox);
    let accounts = compose::account_service(
        &settings,
        Environment::Development,
        pool,
        &http,
        &vault,
        Arc::new(postit_oauth::PlatformCatalog::new(agents, configs)),
        (
            Arc::clone(&media.store) as Arc<dyn postit_core::MediaStore>,
            Arc::clone(&media.host) as Arc<dyn postit_core::MediaHost>,
        ),
        &compose::email_links(&settings).unwrap(),
        &notifier,
        &ids,
        &clock,
    )
    .unwrap();
    {
        let mut conn = pool.acquire().await.unwrap();
        AppSettingsRepo::seed_defaults(&mut conn, true)
            .await
            .unwrap();
    }
    let owner = match owner {
        Some(owner) => owner,
        None => seed_user(pool, "active").await,
    };
    let publishing = compose::publishing(compose::PublishingParts {
        settings: &settings,
        pool,
        accounts: &accounts,
        media: &media,
        notifier: &notifier,
        jobs: &jobs,
        ids: &ids,
        clock: &clock,
        shutdown: tokio_util::sync::CancellationToken::new(),
        compiled_platforms: vec![telegram()],
    });
    Fx {
        pool: pool.clone(),
        accounts,
        publishing,
        media,
        scope: OwnerScope::own(owner),
        owner,
        _dir: dir,
        _media: media_dir,
    }
}

impl Fx {
    /// Connects the bot with these chats, through check then confirm; one account per chat.
    async fn connect(&self, chats: &str) -> Vec<AccountId> {
        let Value::Object(values) = json!({"bot_token": TOKEN, "chats": chats}) else {
            unreachable!()
        };
        let check = self
            .accounts
            .start_credentials(&self.scope, &telegram(), values)
            .await
            .unwrap();
        let confirmed = self
            .accounts
            .confirm(
                &self.scope,
                check.pending.unwrap().0.as_str(),
                None,
                AuditContext::default(),
            )
            .await
            .unwrap();
        confirmed.connected.iter().map(|(a, _)| a.id).collect()
    }

    /// A real PNG uploaded through the media service.
    async fn upload(&self, name: &str, shade: u8) -> MediaAssetId {
        let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([shade, 200, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let bytes = out.into_inner();
        let stream =
            futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(bytes.clone()))])
                .boxed();
        self.media
            .service
            .upload(&self.scope, name, bytes.len() as u64, stream)
            .await
            .unwrap()
            .id
    }

    /// A post with `text` and `media`, published now to `accounts`: one delivery each.
    async fn publish_job(
        &self,
        text: &str,
        media: &[MediaAssetId],
        accounts: &[AccountId],
    ) -> Vec<Dispatch> {
        let post = PostId::from_uuid(Uuid::now_v7());
        let mut tx = self.pool.begin().await.unwrap();
        PostRepo::create(
            &mut tx,
            &self.scope,
            NewPost {
                id: post,
                kind: PostKind::Post,
                reply_to: None,
                campaign_id: None,
                text: text.to_owned(),
                link: None,
                tags: vec![],
                disclose_ai_generated: false,
            },
        )
        .await
        .unwrap();
        let locked = PostRepo::lock(&mut tx, &self.scope, post, None)
            .await
            .unwrap();
        let mut version = locked.post().version;
        if !media.is_empty() {
            let items: Vec<PostMediaItem> = media
                .iter()
                .enumerate()
                .map(|(i, id)| PostMediaItem {
                    media_id: *id,
                    position: i32::try_from(i).unwrap(),
                    alt_text: None,
                })
                .collect();
            PostRepo::set_media(&mut tx, &locked, &items).await.unwrap();
            version += 1;
        }
        tx.commit().await.unwrap();
        self.publishing
            .create_job(
                &self.scope,
                post,
                PublishInput {
                    targets: accounts.to_vec(),
                    at: None,
                    native_schedule: false,
                    dry_run: false,
                    confirm: true,
                    if_match: PostVersion(version),
                },
                AuditContext::default(),
            )
            .await
            .unwrap()
            .deliveries
    }

    async fn run(&self, dispatch: Dispatch) {
        run_publish(&self.publishing, PublishDelivery::from(dispatch))
            .await
            .unwrap();
    }

    async fn delivery(&self, dispatch: Dispatch) -> (String, Option<String>, Option<String>) {
        sqlx::query_as("SELECT state, parked_reason, remote_url FROM deliveries WHERE id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn account_status(&self, account: AccountId) -> (String, String) {
        sqlx::query_as(
            "SELECT a.status, g.status FROM connected_accounts a \
             JOIN account_grants g ON g.id = a.grant_id WHERE a.id = $1",
        )
        .bind(account.as_uuid())
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn text_post_reaches_the_channel(pool: PgPool) {
    let server = bot_api().await;
    on_send(&server, "sendMessage", ok(sent(42))).await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001234567890").await;
    assert_eq!(accounts.len(), 1);

    let dispatch = fx.publish_job("Fresh bread today", &[], &accounts).await[0];
    fx.run(dispatch).await;

    let (state, _, url) = fx.delivery(dispatch).await;
    assert_eq!(state, "published");
    assert_eq!(url.as_deref(), Some("https://t.me/newsroom/42"));
    let sends = requests(&server, "sendMessage").await;
    assert_eq!(sends.len(), 1);
    assert_eq!(body_of(&sends[0])["text"], "Fresh bread today");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn image_is_read_from_the_real_store(pool: PgPool) {
    let server = bot_api().await;
    on_send(&server, "sendPhoto", ok(sent(61))).await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001234567890").await;
    let picture = fx.upload("one.png", 10).await;

    let dispatch = fx.publish_job("One picture", &[picture], &accounts).await[0];
    fx.run(dispatch).await;

    let (state, _, url) = fx.delivery(dispatch).await;
    assert_eq!(state, "published");
    assert_eq!(url.as_deref(), Some("https://t.me/newsroom/61"));
    let sends = requests(&server, "sendPhoto").await;
    assert_eq!(sends.len(), 1);
    let body = String::from_utf8_lossy(&sends[0].body).into_owned();
    for part in ["name=\"photo\"", "file0.png", "One picture"] {
        assert!(body.contains(part), "{part}");
    }
    assert!(sends[0].body.windows(8).any(|w| w == b"\x89PNG\r\n\x1a\n"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn album_is_read_from_the_real_store(pool: PgPool) {
    let server = bot_api().await;
    on_send(&server, "sendMediaGroup", ok(json!([sent(51), sent(52)]))).await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001234567890").await;
    let first = fx.upload("one.png", 10).await;
    let second = fx.upload("two.png", 120).await;

    let dispatch = fx
        .publish_job("Two pictures", &[first, second], &accounts)
        .await[0];
    fx.run(dispatch).await;

    assert_eq!(fx.delivery(dispatch).await.0, "published");
    let sends = requests(&server, "sendMediaGroup").await;
    assert_eq!(sends.len(), 1);
    let body = String::from_utf8_lossy(&sends[0].body).into_owned();
    for name in ["file0.png", "file1.png", "attach://file0", "attach://file1"] {
        assert!(body.contains(name), "{name}");
    }
    // The bytes at the Bot API are the files in the store: each PNG starts with its magic.
    let magic = sends[0]
        .body
        .windows(8)
        .filter(|w| *w == b"\x89PNG\r\n\x1a\n")
        .count();
    assert_eq!(magic, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lost_send_parks_unresolvable(pool: PgPool) {
    let server = bot_api().await;
    // The post lands, then the response is lost.
    on_send(
        &server,
        "sendMessage",
        ok(sent(1)).set_delay(Duration::from_secs(40)),
    )
    .await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001234567890").await;

    let dispatch = fx.publish_job("Maybe posted", &[], &accounts).await[0];
    fx.run(dispatch).await;

    let (state, reason, _) = fx.delivery(dispatch).await;
    assert_eq!(state, "outcome_unknown");
    // The reconcile finds nothing it can ask, so it parks; run the reconcile the engine queued.
    let _ = reason;
    postit_publishing::reconcile::run_reconcile(&fx.publishing, dispatch.into())
        .await
        .unwrap();
    let (state, reason, _) = fx.delivery(dispatch).await;
    assert_eq!(state, "parked");
    assert_eq!(reason.as_deref(), Some("outcome_unresolvable"));
    assert_eq!(requests(&server, "sendMessage").await.len(), 1);

    // The sweep does not send it again.
    run_sweep(&fx.publishing).await.unwrap();
    assert_eq!(requests(&server, "sendMessage").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn refused_connection_retries(pool: PgPool) {
    let server = bot_api().await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001234567890").await;
    // The Bot API is unreachable when the post comes due: a port nothing listens on.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let down = fixture(&pool, &format!("http://127.0.0.1:{closed}"), Some(fx.owner)).await;

    let dispatch = down.publish_job("Nobody home", &[], &accounts).await[0];
    down.run(dispatch).await;

    let (state, reason, _) = down.delivery(dispatch).await;
    let last: Option<Value> = sqlx::query_scalar("SELECT last_error FROM deliveries WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "pending", "{last:?}");
    assert!(reason.is_none());
    let next: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT next_attempt_at FROM deliveries WHERE id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(next.unwrap() > postit_core::Clock::now(compose::system_clock().as_ref()));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoked_token_parks_every_channel(pool: PgPool) {
    let server = bot_api().await;
    on_send(
        &server,
        "sendMessage",
        ResponseTemplate::new(401)
            .set_body_json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"})),
    )
    .await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001, -1002, -1003").await;
    assert_eq!(accounts.len(), 3);
    let dispatches = fx.publish_job("Three channels", &[], &accounts).await;
    assert_eq!(dispatches.len(), 3);

    for dispatch in &dispatches {
        fx.run(*dispatch).await;
    }

    for dispatch in &dispatches {
        let (state, reason, _) = fx.delivery(*dispatch).await;
        assert_eq!(state, "parked");
        assert_eq!(reason.as_deref(), Some("account_needs_reconnect"));
    }
    for account in &accounts {
        let (account_status, grant_status) = fx.account_status(*account).await;
        assert_eq!(account_status, "needs_reconnect");
        assert_eq!(grant_status, "needs_reconnect");
    }
    // The first 401 marked the grant; the others parked before calling the platform.
    assert_eq!(requests(&server, "sendMessage").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn removed_bot_parks_one_channel(pool: PgPool) {
    let server = bot_api().await;
    // The bot was removed from -1001 only.
    Mock::given(method("POST"))
        .and(path_regex("^/bot[^/]+/sendMessage$"))
        .and(wiremock::matchers::body_partial_json(
            json!({"chat_id": -1001}),
        ))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "ok": false,
            "error_code": 403,
            "description": "Forbidden: bot was kicked from the channel chat"
        })))
        .mount(&server)
        .await;
    on_send(&server, "sendMessage", ok(sent(9))).await;
    let fx = fixture(&pool, &server.uri(), None).await;
    let accounts = fx.connect("-1001, -1002").await;
    assert_eq!(accounts.len(), 2);
    let dispatches = fx.publish_job("Two channels", &[], &accounts).await;

    // Which delivery targets which chat: read the accounts' remote IDs.
    let mut by_chat = Vec::new();
    for (account, dispatch) in accounts.iter().zip(&dispatches) {
        let remote: String =
            sqlx::query_scalar("SELECT remote_account_id FROM connected_accounts WHERE id = $1")
                .bind(account.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        by_chat.push((remote, *account, *dispatch));
    }
    for (_, _, dispatch) in &by_chat {
        fx.run(*dispatch).await;
    }

    let removed = by_chat.iter().find(|(chat, ..)| chat == "-1001").unwrap();
    let healthy = by_chat.iter().find(|(chat, ..)| chat == "-1002").unwrap();
    let (state, reason, _) = fx.delivery(removed.2).await;
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("parked", Some("account_needs_reconnect"))
    );
    assert_eq!(fx.delivery(healthy.2).await.0, "published");
    assert_eq!(
        fx.account_status(removed.1).await,
        ("needs_reconnect".to_owned(), "active".to_owned())
    );
    assert_eq!(
        fx.account_status(healthy.1).await,
        ("active".to_owned(), "active".to_owned())
    );

    // The bot is put back; the owner presses Check, and the sweep unparks within the grace.
    server.reset().await;
    bot_api_mocks(&server).await;
    on_send(&server, "sendMessage", ok(sent(10))).await;
    fx.accounts
        .check(&fx.scope, removed.1, AuditContext::default())
        .await
        .unwrap();
    assert_eq!(fx.account_status(removed.1).await.0, "active");
    let report = run_sweep(&fx.publishing).await.unwrap();
    assert_eq!(report.unparked, 1);
    assert_eq!(fx.delivery(removed.2).await.0, "pending");
}
