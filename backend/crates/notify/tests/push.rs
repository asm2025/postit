//! Web Push: the RFC 8291 body, the RFC 8292 VAPID header, the `push_send` job, and the push
//! fan-out of `Notifier::record`.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use postit_config::{HttpSettings, PushSettings};
use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, NotificationKind, PlatformId,
    PostId, PublishJobId, UserId,
};
use postit_data::preferences::{
    ChannelSwitch, NotificationSwitches, PreferencesUpdate, UserPreferencesRepo,
};
use postit_data::push_endpoints::{PushEndpointRepo, PushPlatform};
use postit_data::testkit::seed_user;
use postit_http::{AddressPolicy, build_public_client};
use postit_jobs::{JobContext, JobError, JobId};
use postit_notify::ece::{self, TestSubscriber};
use postit_notify::vapid::VapidKeys;
use postit_notify::{
    NewDevice, Notifier, NotifyEvent, PushEndpoints, PushEndpointsDeps, PushSend, PushSender,
    PushSenderDeps, Recipients, push_topic,
};
use postit_vault::Vault;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A fixed VAPID private key (the RFC 8291 example's sender key), so the tests need no vault.
const VAPID_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const SUBJECT: &str = "mailto:ops@postit.test";

fn b64(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(s).unwrap()
}

#[test]
fn rfc8291_test_vector() {
    // RFC 8291 section 5.
    let plaintext = b"When I grow up, I want to be a watermelon";
    let ua_public = b64(
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
    );
    let ua_private = b64("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94");
    let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
    let as_private = b64("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw");
    let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
    let expected = b64(
        "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
    );

    let body = ece::encrypt_with(plaintext, &ua_public, &auth, &as_private, salt).unwrap();
    assert_eq!(body, expected);
    assert_eq!(ece::decrypt(&body, &ua_private, &auth).unwrap(), plaintext);

    // The random-key path round-trips too.
    let random = ece::encrypt(plaintext, &ua_public, &auth).unwrap();
    assert_ne!(random, expected);
    assert_eq!(
        ece::decrypt(&random, &ua_private, &auth).unwrap(),
        plaintext
    );
}

#[test]
fn vapid_header_shape() {
    let keys = VapidKeys::new(VAPID_PRIVATE, SUBJECT).unwrap();
    // The RFC 8291 sender key's public half.
    assert_eq!(
        keys.public_key_b64url(),
        "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
    );
    let now = Utc::now();
    let endpoint = url::Url::parse("https://push.example.test:8443/send/abc?x=1").unwrap();
    let header = keys.authorization(&endpoint, now).unwrap();

    let rest = header.strip_prefix("vapid t=").unwrap();
    let (token, key) = rest.split_once(", k=").unwrap();
    assert_eq!(key, keys.public_key_b64url());

    let point = b64(key);
    assert_eq!(point.len(), 65);
    let decoding = jsonwebtoken::DecodingKey::from_ec_components(
        &URL_SAFE_NO_PAD.encode(&point[1..33]),
        &URL_SAFE_NO_PAD.encode(&point[33..]),
    )
    .unwrap();
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::ES256);
    validation.set_audience(&["https://push.example.test:8443"]);
    let claims = jsonwebtoken::decode::<Value>(token, &decoding, &validation)
        .unwrap()
        .claims;
    assert_eq!(claims["aud"], "https://push.example.test:8443");
    assert_eq!(claims["sub"], SUBJECT);
    let exp = claims["exp"].as_i64().unwrap();
    assert!(exp > now.timestamp());
    assert!(exp - now.timestamp() <= 24 * 3600);

    // The default port is not part of the origin.
    let default_port = url::Url::parse("https://fcm.example.test/x").unwrap();
    let header = keys.authorization(&default_port, now).unwrap();
    let token = header
        .strip_prefix("vapid t=")
        .unwrap()
        .split_once(", k=")
        .unwrap()
        .0;
    validation.set_audience(&["https://fcm.example.test"]);
    assert!(jsonwebtoken::decode::<Value>(token, &decoding, &validation).is_ok());
}

#[test]
fn vapid_keys_follow_the_settings() {
    let off = PushSettings {
        enabled: false,
        vapid_private_key: None,
        vapid_subject: None,
        allow_private_endpoints: false,
    };
    assert!(VapidKeys::from_settings(&off).unwrap().is_none());
    let on: PushSettings = serde_json::from_value(serde_json::json!({
        "enabled": true,
        "vapid_private_key": VAPID_PRIVATE,
        "vapid_subject": SUBJECT,
        "allow_private_endpoints": false,
    }))
    .unwrap();
    let keys = VapidKeys::from_settings(&on).unwrap().unwrap();
    assert!(!format!("{keys:?}").contains(VAPID_PRIVATE));
    let bad: PushSettings = serde_json::from_value(serde_json::json!({
        "enabled": true,
        "vapid_private_key": "not-a-key",
        "vapid_subject": SUBJECT,
        "allow_private_endpoints": false,
    }))
    .unwrap();
    let err = VapidKeys::from_settings(&bad).unwrap_err().to_string();
    assert!(err.contains("push.vapid_private_key"), "{err}");
    assert!(!err.contains("not-a-key"), "{err}");
}

#[test]
fn topic_is_the_tag_bytes() {
    let tag = Uuid::now_v7();
    let topic = push_topic(tag);
    assert_eq!(topic.len(), 22);
    assert_eq!(b64(&topic), tag.as_bytes());
}

// ----- the job -----

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap()
}

fn keys() -> Arc<VapidKeys> {
    Arc::new(VapidKeys::new(VAPID_PRIVATE, SUBJECT).unwrap())
}

fn http_settings() -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-notify-tests/0".to_owned(),
        extra_ca_files: Vec::new(),
    }
}

fn endpoints(pool: &PgPool) -> PushEndpoints {
    PushEndpoints::new(PushEndpointsDeps {
        pool: pool.clone(),
        vault: vault(),
        ids: Arc::new(postit_core::SystemIdGenerator),
        keys: Some(keys()),
        allow_private: true,
    })
}

fn sender(pool: &PgPool, allow_private: bool) -> PushSender {
    PushSender::new(PushSenderDeps {
        pool: pool.clone(),
        vault: vault(),
        keys: Some(keys()),
        client: build_public_client(&http_settings(), AddressPolicy { allow_private }).unwrap(),
        clock: Arc::new(postit_core::SystemClock),
    })
}

fn ctx() -> JobContext {
    JobContext {
        job_id: JobId(Uuid::now_v7()),
        attempt: 1,
        max_attempts: Some(5),
    }
}

async fn register(pool: &PgPool, user: UserId, url: &str, device: &TestSubscriber) -> Uuid {
    endpoints(pool)
        .register(
            user,
            NewDevice {
                endpoint: url.to_owned(),
                p256dh: device.public_b64url(),
                auth: device.auth_b64url(),
                platform: PushPlatform::Web,
                device_label: "Laptop".to_owned(),
            },
        )
        .await
        .unwrap()
        .as_uuid()
}

fn delivery(job: PublishJobId) -> DeliveryNotice {
    DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: job,
        post_id: PostId::from(Uuid::now_v7()),
        account: AccountLabel {
            account_id: AccountId::from(Uuid::now_v7()),
            account_name: "Shop".to_owned(),
            platform: PlatformId::new("mastodon").unwrap(),
        },
        park_reason: None,
    }
}

async fn record(pool: &PgPool, notifier: &Notifier, user: UserId, notice: Notice) -> Vec<Uuid> {
    let mut tx = pool.begin().await.unwrap();
    let ids = notifier
        .record(
            &mut tx,
            NotifyEvent {
                recipients: Recipients::User {
                    user,
                    owner: Some(user),
                },
                notice,
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    ids.into_iter().map(|id| id.as_uuid()).collect()
}

/// Every `push_send` job payload, oldest first.
async fn push_jobs(pool: &PgPool) -> Vec<PushSend> {
    sqlx::query_scalar::<_, Value>(
        "SELECT payload FROM jobs WHERE job_type = 'push_send' ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|p| serde_json::from_value(p).unwrap())
    .collect()
}

async fn mount(server: &MockServer, at: &str, status: u16) {
    Mock::given(method("POST"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

async fn received(server: &MockServer, at: &str) -> Vec<wiremock::Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == at)
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn push_delivers_payload_without_text(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/one", 201).await;
    let user = seed_user(&pool, "active").await;
    let device = TestSubscriber::generate();
    let url = format!("{}/push/one", server.uri());
    let endpoint = register(&pool, user, &url, &device).await;

    let notifier = Notifier::for_tests_with_push(pool.clone());
    let job = PublishJobId::from(Uuid::now_v7());
    let ids = record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(job)),
    )
    .await;
    let jobs = push_jobs(&pool).await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].notification, ids[0]);
    assert_eq!(jobs[0].endpoint, endpoint);

    sender(&pool, true)
        .handle(jobs[0].clone(), ctx())
        .await
        .unwrap();

    let requests = received(&server, "/push/one").await;
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    let header = |name: &str| {
        req.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(header("ttl"), "86400");
    assert_eq!(header("urgency"), "high");
    assert_eq!(header("content-encoding"), "aes128gcm");
    assert_eq!(header("topic"), push_topic(job.as_uuid()));
    assert!(header("authorization").starts_with("vapid t="));

    let plain = device.decrypt(&req.body).unwrap();
    let payload: Value = serde_json::from_slice(&plain).unwrap();
    let keys: Vec<&String> = payload.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["body", "id", "kind", "tag", "title"]);
    assert_eq!(payload["id"], ids[0].to_string());
    assert_eq!(payload["kind"], "delivery_failed");
    assert_eq!(payload["tag"], job.as_uuid().to_string());
    assert!(!payload["title"].as_str().unwrap().is_empty());
    assert!(payload["body"].as_str().unwrap().contains("Shop"));
    let text = String::from_utf8(plain).unwrap();
    assert!(!text.contains(&url), "the endpoint URL is in the payload");

    let mut conn = pool.acquire().await.unwrap();
    let row = PushEndpointRepo::get(&mut conn, endpoint.into())
        .await
        .unwrap()
        .unwrap();
    assert!(row.last_success_at.is_some());
    let pushed: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT pushed_at FROM notifications WHERE id = $1")
            .bind(ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(pushed.is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn push_is_in_the_recipients_language_with_normal_urgency_for_info(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/ar", 201).await;
    let user = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    UserPreferencesRepo::upsert(
        &mut conn,
        user,
        &PreferencesUpdate {
            locale: Some("ar".to_owned()),
            ..PreferencesUpdate::default()
        },
    )
    .await
    .unwrap();
    // Push is off by default for published posts; turn it on.
    UserPreferencesRepo::upsert(
        &mut conn,
        user,
        &PreferencesUpdate {
            switches: Some(NotificationSwitches(BTreeMap::from([(
                NotificationKind::DeliveryPublished,
                ChannelSwitch {
                    push: Some(true),
                    email: None,
                },
            )]))),
            ..PreferencesUpdate::default()
        },
    )
    .await
    .unwrap();
    drop(conn);
    let device = TestSubscriber::generate();
    register(&pool, user, &format!("{}/push/ar", server.uri()), &device).await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    let ids = record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryPublished(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    let job = push_jobs(&pool).await.remove(0);
    sender(&pool, true).handle(job, ctx()).await.unwrap();

    let req = received(&server, "/push/ar").await.remove(0);
    assert_eq!(
        req.headers.get("urgency").unwrap().to_str().unwrap(),
        "normal"
    );
    let payload: Value = serde_json::from_slice(&device.decrypt(&req.body).unwrap()).unwrap();
    let title = payload["title"].as_str().unwrap();
    assert!(
        title.chars().any(|c| ('\u{600}'..='\u{6ff}').contains(&c)),
        "{title}"
    );
    // A delivery kind's tag is its job; this one is the only push of its job.
    assert_ne!(payload["tag"], ids[0].to_string());
}

/// Review Focus 3: two delivery notices of one job share one tag and one Topic, so the push
/// service and the device keep only the latest.
#[sqlx::test(migrations = "../data/migrations")]
async fn job_pushes_share_a_topic(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/job", 201).await;
    let user = seed_user(&pool, "active").await;
    let device = TestSubscriber::generate();
    register(&pool, user, &format!("{}/push/job", server.uri()), &device).await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    let job = PublishJobId::from(Uuid::now_v7());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(job)),
    )
    .await;
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryParked(delivery(job)),
    )
    .await;
    let sender = sender(&pool, true);
    for job in push_jobs(&pool).await {
        sender.handle(job, ctx()).await.unwrap();
    }

    let requests = received(&server, "/push/job").await;
    assert_eq!(requests.len(), 2);
    let topics: Vec<String> = requests
        .iter()
        .map(|r| r.headers.get("topic").unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(topics[0], topics[1]);
    assert_eq!(topics[0], push_topic(job.as_uuid()));
    let tags: Vec<Value> = requests
        .iter()
        .map(|r| {
            let p: Value = serde_json::from_slice(&device.decrypt(&r.body).unwrap()).unwrap();
            p["tag"].clone()
        })
        .collect();
    assert_eq!(tags[0], tags[1]);
    assert_eq!(tags[0], job.as_uuid().to_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_notice_without_a_job_is_tagged_with_its_own_id(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/own", 201).await;
    let user = seed_user(&pool, "active").await;
    let device = TestSubscriber::generate();
    register(&pool, user, &format!("{}/push/own", server.uri()), &device).await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    let label = delivery(PublishJobId::from(Uuid::now_v7())).account;
    let ids = record(
        &pool,
        &notifier,
        user,
        Notice::AccountNeedsReconnect(postit_core::AccountNotice { account: label }),
    )
    .await;
    let job = push_jobs(&pool).await.remove(0);
    sender(&pool, true).handle(job, ctx()).await.unwrap();
    let req = received(&server, "/push/own").await.remove(0);
    let payload: Value = serde_json::from_slice(&device.decrypt(&req.body).unwrap()).unwrap();
    assert_eq!(payload["tag"], ids[0].to_string());
    assert_eq!(
        req.headers.get("topic").unwrap().to_str().unwrap(),
        push_topic(ids[0])
    );
}

/// Review Focus 5: the first `410` deletes that endpoint; the user's other endpoint still
/// receives, and later notices go to it alone.
#[sqlx::test(migrations = "../data/migrations")]
async fn gone_endpoint_is_deleted_and_others_still_receive(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/gone", 410).await;
    mount(&server, "/push/kept", 201).await;
    let user = seed_user(&pool, "active").await;
    let gone = register(
        &pool,
        user,
        &format!("{}/push/gone", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    let kept = register(
        &pool,
        user,
        &format!("{}/push/kept", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    let notice = || Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7())));
    record(&pool, &notifier, user, notice()).await;
    let sender = sender(&pool, true);
    let jobs = push_jobs(&pool).await;
    assert_eq!(jobs.len(), 2);
    for job in jobs {
        sender.handle(job, ctx()).await.unwrap();
    }

    let mut conn = pool.acquire().await.unwrap();
    assert!(
        PushEndpointRepo::get(&mut conn, gone.into())
            .await
            .unwrap()
            .is_none()
    );
    let row = PushEndpointRepo::get(&mut conn, kept.into())
        .await
        .unwrap()
        .unwrap();
    assert!(row.last_success_at.is_some());
    drop(conn);
    assert_eq!(received(&server, "/push/kept").await.len(), 1);

    record(&pool, &notifier, user, notice()).await;
    let jobs = push_jobs(&pool).await;
    assert_eq!(jobs.len(), 3);
    assert_eq!(jobs[2].endpoint, kept);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rate_limited_endpoint_retries(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/busy", 429).await;
    mount(&server, "/push/down", 503).await;
    let user = seed_user(&pool, "active").await;
    let busy = register(
        &pool,
        user,
        &format!("{}/push/busy", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    register(
        &pool,
        user,
        &format!("{}/push/down", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    let sender = sender(&pool, true);
    for job in push_jobs(&pool).await {
        let err = sender.handle(job, ctx()).await.unwrap_err();
        assert!(matches!(err, JobError::Retry(_)), "{err:?}");
    }
    let mut conn = pool.acquire().await.unwrap();
    let row = PushEndpointRepo::get(&mut conn, busy.into())
        .await
        .unwrap()
        .unwrap();
    assert!(row.last_success_at.is_none());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn other_client_errors_are_dropped_and_keep_the_endpoint(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/bad", 400).await;
    let user = seed_user(&pool, "active").await;
    let endpoint = register(
        &pool,
        user,
        &format!("{}/push/bad", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    let job = push_jobs(&pool).await.remove(0);
    sender(&pool, true).handle(job, ctx()).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        PushEndpointRepo::get(&mut conn, endpoint.into())
            .await
            .unwrap()
            .is_some()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn private_endpoint_refused_unless_allowed(pool: PgPool) {
    let server = MockServer::start().await;
    mount(&server, "/push/lan", 201).await;
    let user = seed_user(&pool, "active").await;
    // The mock listens on loopback, a private address.
    register(
        &pool,
        user,
        &format!("{}/push/lan", server.uri()),
        &TestSubscriber::generate(),
    )
    .await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    let job = push_jobs(&pool).await.remove(0);

    let err = sender(&pool, false)
        .handle(job.clone(), ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, JobError::Fatal(_)), "{err:?}");
    assert!(received(&server, "/push/lan").await.is_empty());

    sender(&pool, true).handle(job, ctx()).await.unwrap();
    assert_eq!(received(&server, "/push/lan").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gone_rows_end_the_job_quietly(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let sender = sender(&pool, true);
    // Neither the endpoint nor the notification exists.
    sender
        .handle(
            PushSend {
                notification: Uuid::now_v7(),
                endpoint: Uuid::now_v7(),
            },
            ctx(),
        )
        .await
        .unwrap();
    // The endpoint exists, the notification does not.
    let endpoint = register(
        &pool,
        user,
        "https://push.example.test/x",
        &TestSubscriber::generate(),
    )
    .await;
    sender
        .handle(
            PushSend {
                notification: Uuid::now_v7(),
                endpoint,
            },
            ctx(),
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn push_switch_off_enqueues_nothing(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    register(
        &pool,
        user,
        "https://push.example.test/off",
        &TestSubscriber::generate(),
    )
    .await;
    let mut conn = pool.acquire().await.unwrap();
    UserPreferencesRepo::upsert(
        &mut conn,
        user,
        &PreferencesUpdate {
            switches: Some(NotificationSwitches(BTreeMap::from([(
                NotificationKind::DeliveryFailed,
                ChannelSwitch {
                    push: Some(false),
                    email: None,
                },
            )]))),
            ..PreferencesUpdate::default()
        },
    )
    .await
    .unwrap();
    drop(conn);
    let notifier = Notifier::for_tests_with_push(pool.clone());
    let n = record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    assert_eq!(n.len(), 1);
    assert!(push_jobs(&pool).await.is_empty());
    // Info kinds are not pushed by default either.
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryPublished(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    assert!(push_jobs(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn push_disabled_enqueues_nothing(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    register(
        &pool,
        user,
        "https://push.example.test/disabled",
        &TestSubscriber::generate(),
    )
    .await;
    let notifier = Notifier::for_tests(pool.clone());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    assert!(push_jobs(&pool).await.is_empty());

    // With push off in the deployment, a job left over from before ends quietly.
    let off = PushSender::new(PushSenderDeps {
        pool: pool.clone(),
        vault: vault(),
        keys: None,
        client: build_public_client(&http_settings(), AddressPolicy::strict()).unwrap(),
        clock: Arc::new(postit_core::SystemClock),
    });
    off.handle(
        PushSend {
            notification: Uuid::now_v7(),
            endpoint: Uuid::now_v7(),
        },
        ctx(),
    )
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_user_without_devices_gets_no_jobs(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let notifier = Notifier::for_tests_with_push(pool.clone());
    record(
        &pool,
        &notifier,
        user,
        Notice::DeliveryFailed(delivery(PublishJobId::from(Uuid::now_v7()))),
    )
    .await;
    assert!(push_jobs(&pool).await.is_empty());
}

#[test]
fn every_kind_has_a_push_title_and_body() {
    for locale in postit_i18n::Locale::ALL {
        let ids = postit_i18n::message_ids(locale);
        for kind in NotificationKind::ALL {
            for prefix in ["push-title-", "push-body-"] {
                let id = format!("{prefix}{}", kind.as_str());
                assert!(ids.contains(&id), "{locale:?} lacks {id}");
            }
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn devices_can_be_removed_with_push_off(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let id = register(
        &pool,
        user,
        "https://push.example.test/cleanup",
        &TestSubscriber::generate(),
    )
    .await;
    let off = PushEndpoints::new(PushEndpointsDeps {
        pool: pool.clone(),
        vault: vault(),
        ids: Arc::new(postit_core::SystemIdGenerator),
        keys: None,
        allow_private: false,
    });
    assert!(off.vapid_public_key().is_none());
    let err = off
        .register(
            user,
            NewDevice {
                endpoint: "https://push.example.test/new".to_owned(),
                p256dh: TestSubscriber::generate().public_b64url(),
                auth: TestSubscriber::generate().auth_b64url(),
                platform: PushPlatform::Web,
                device_label: String::new(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, postit_notify::PushEndpointError::Disabled));
    off.delete(user, id.into()).await.unwrap();
    assert!(matches!(
        off.delete(user, id.into()).await.unwrap_err(),
        postit_notify::PushEndpointError::NotFound
    ));
}
