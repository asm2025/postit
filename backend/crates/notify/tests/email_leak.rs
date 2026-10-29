//! The alert email, its job payload, and the logs carry no address beyond the recipient's own
//! `To` header. Its own binary because it captures logs.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, PlatformId, PostId, PublishJobId,
    UserId,
};
use postit_data::testkit::seed_user;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_jobs::{JobContext, JobId};
use postit_mail::{
    MailLoaders, MailOutbox, MemoryMailer, SendEmail, SendEmailDeps, SendEmailHandler,
};
use postit_notify::{Notifier, NotifyEvent, Recipients};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const ADDRESS: &str = "leak-owner@example.test";
const REMOTE_ID: &str = "remote-account-7f3a9c";

async fn owner(pool: &PgPool) -> UserId {
    let user = seed_user(pool, "active").await;
    sqlx::query("UPDATE users SET email = $2, email_verified = TRUE WHERE id = $1")
        .bind(user.as_uuid())
        .bind(ADDRESS)
        .execute(pool)
        .await
        .unwrap();
    user
}

async fn account(pool: &PgPool, owner: UserId) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, 'Shop')",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(REMOTE_ID)
    .execute(pool)
    .await
    .unwrap();
    AccountId::from(id)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn alert_carries_no_address_in_body_payload_or_logs(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let owner = owner(&pool).await;
    let acct = account(&pool, owner).await;
    let notice = Notice::DeliveryFailed(DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: PublishJobId::from(Uuid::now_v7()),
        post_id: PostId::from(Uuid::now_v7()),
        account: AccountLabel {
            account_id: acct,
            account_name: "Shop".to_owned(),
            platform: PlatformId::new("mastodon").unwrap(),
        },
        park_reason: None,
    });
    let mut tx = pool.begin().await.unwrap();
    Notifier::for_tests(pool.clone())
        .record(
            &mut tx,
            NotifyEvent {
                recipients: Recipients::Workspace { owner, actor: None },
                notice,
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE job_type = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let job: SendEmail = serde_json::from_value(payload.clone()).unwrap();

    let mailer = MemoryMailer::default();
    let queue = postit_jobs::JobQueue::new(
        pool.clone(),
        Arc::new(postit_core::SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut loaders = MailLoaders::default();
    postit_notify::register_loaders(&mut loaders).unwrap();
    SendEmailHandler::new(SendEmailDeps {
        pool: pool.clone(),
        ids: Arc::new(postit_core::SystemIdGenerator),
        mailer: Arc::new(mailer.clone()),
        loaders,
        outbox: MailOutbox::new(queue),
        app_url: Url::parse("https://app.postit.test").unwrap(),
        max_attempts: 3,
    })
    .handle(
        job,
        JobContext {
            job_id: JobId(Uuid::now_v7()),
            attempt: 1,
            max_attempts: Some(3),
        },
    )
    .await
    .unwrap();

    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, ADDRESS);
    let payload = payload.to_string();
    let logs = logs.text();
    assert_no_leak(
        &[ADDRESS, REMOTE_ID],
        &[
            ("subject", &sent[0].subject),
            ("text", &sent[0].text),
            ("html", &sent[0].html),
            ("job payload", &payload),
            ("logs", &logs),
        ],
    );
}
