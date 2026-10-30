//! No endpoint URL, subscription key, or VAPID private key reaches a log, a job error, or a job
//! payload, whatever the push service answers. Its own binary because it captures logs.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use postit_config::HttpSettings;
use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, PlatformId, PostId, PublishJobId,
};
use postit_data::push_endpoints::PushPlatform;
use postit_data::testkit::seed_user;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_http::{AddressPolicy, build_public_client};
use postit_jobs::{JobContext, JobId};
use postit_notify::ece::TestSubscriber;
use postit_notify::vapid::VapidKeys;
use postit_notify::{
    NewDevice, Notifier, NotifyEvent, PushEndpoints, PushEndpointsDeps, PushSend, PushSender,
    PushSenderDeps, Recipients,
};
use postit_vault::Vault;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VAPID_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const TOKEN: &str = "very-secret-endpoint-token-4f2a9c";

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap()
}

fn keys() -> Arc<VapidKeys> {
    Arc::new(VapidKeys::new(VAPID_PRIVATE, "mailto:ops@postit.test").unwrap())
}

fn sender(pool: &PgPool, allow_private: bool) -> PushSender {
    let settings = HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-notify-tests/0".to_owned(),
        extra_ca_files: Vec::new(),
    };
    PushSender::new(PushSenderDeps {
        pool: pool.clone(),
        vault: vault(),
        keys: Some(keys()),
        client: build_public_client(&settings, AddressPolicy { allow_private }).unwrap(),
        clock: Arc::new(postit_core::SystemClock),
    })
}

#[sqlx::test(migrations = "../data/migrations")]
async fn push_never_leaks_the_endpoint_or_keys(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let server = MockServer::start().await;
    let mut secrets: Vec<String> = vec![VAPID_PRIVATE.to_owned(), TOKEN.to_owned()];
    let user = seed_user(&pool, "active").await;
    let service = PushEndpoints::new(PushEndpointsDeps {
        pool: pool.clone(),
        vault: vault(),
        ids: Arc::new(postit_core::SystemIdGenerator),
        keys: Some(keys()),
        allow_private: true,
    });
    for (status, name) in [
        (201, "ok"),
        (410, "gone"),
        (429, "busy"),
        (400, "bad"),
        (500, "down"),
    ] {
        let at = format!("/{name}/{TOKEN}");
        Mock::given(method("POST"))
            .and(path(at.clone()))
            .respond_with(ResponseTemplate::new(status).set_body_string(format!("echo {at}")))
            .mount(&server)
            .await;
        let device = TestSubscriber::generate();
        let url = format!("{}{at}", server.uri());
        secrets.extend([url.clone(), device.public_b64url(), device.auth_b64url()]);
        service
            .register(
                user,
                NewDevice {
                    endpoint: url,
                    p256dh: device.public_b64url(),
                    auth: device.auth_b64url(),
                    platform: PushPlatform::Web,
                    device_label: name.to_owned(),
                },
            )
            .await
            .unwrap();
    }

    let mut tx = pool.begin().await.unwrap();
    Notifier::for_tests_with_push(pool.clone())
        .record(
            &mut tx,
            NotifyEvent {
                recipients: Recipients::User {
                    user,
                    owner: Some(user),
                },
                notice: Notice::DeliveryFailed(DeliveryNotice {
                    delivery_id: DeliveryId::from(Uuid::now_v7()),
                    job_id: PublishJobId::from(Uuid::now_v7()),
                    post_id: PostId::from(Uuid::now_v7()),
                    account: AccountLabel {
                        account_id: AccountId::from(Uuid::now_v7()),
                        account_name: "Shop".to_owned(),
                        platform: PlatformId::new("mastodon").unwrap(),
                    },
                    park_reason: None,
                }),
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let payloads: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE job_type = 'push_send'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(payloads.len(), 5);
    let mut errors = String::new();
    for payload in &payloads {
        let job: PushSend = serde_json::from_value(payload.clone()).unwrap();
        for allow_private in [false, true] {
            let ctx = JobContext {
                job_id: JobId(Uuid::now_v7()),
                attempt: 1,
                max_attempts: Some(5),
            };
            if let Err(err) = sender(&pool, allow_private).handle(job.clone(), ctx).await {
                writeln!(errors, "{err} | {err:?}").unwrap();
            }
        }
    }
    assert!(!errors.is_empty(), "expected some failed sends");

    let payloads = serde_json::to_string(&payloads).unwrap();
    let logs = logs.text();
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
    assert_no_leak(
        &secrets,
        &[
            ("job payloads", &payloads),
            ("job errors", &errors),
            ("logs", &logs),
            ("keys debug", &format!("{:?}", keys())),
        ],
    );
}
