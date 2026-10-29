//! The coalesced critical email: one `send_email` job per wanted critical row, and the first
//! job to run takes every unemailed critical row of the recipient.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use postit_core::{
    AccountId, AccountLabel, AccountNotice, DelegationEvent, DelegationId, DelegationNotice,
    DeliveryId, DeliveryNotice, GrantId, JobNotice, Notice, NotificationKind, ParkReason,
    PlatformId, PostId, PublishJobId, ReconnectNotice, RecurrenceId, RecurrenceNotice, SkipReason,
    UserId,
};
use postit_data::preferences::{
    ChannelSwitch, NotificationSwitches, PreferencesUpdate, UserPreferencesRepo,
};
use postit_data::testkit::seed_user;
use postit_jobs::{JobContext, JobError, JobId};
use postit_mail::{
    MailContent, MailError, MailKind, MailLoaders, MailOutbox, Mailer, MemoryMailer,
    RenderedMessage, SendEmail, SendEmailDeps, SendEmailHandler, render,
};
use postit_notify::{ALERT_DELAY, Notifier, NotifyEvent, Recipients, notifications_for};
use sqlx::PgPool;
use tokio::sync::Notify;
use url::Url;
use uuid::Uuid;

const ADDRESS: &str = "owner@example.test";

async fn verified_user(pool: &PgPool) -> UserId {
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
    .bind(id.to_string())
    .execute(pool)
    .await
    .unwrap();
    AccountId::from(id)
}

fn label(account: AccountId, name: &str) -> AccountLabel {
    AccountLabel {
        account_id: account,
        account_name: name.to_owned(),
        platform: PlatformId::new("mastodon").unwrap(),
    }
}

fn delivery(account: AccountId, name: &str, park_reason: Option<ParkReason>) -> DeliveryNotice {
    DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: PublishJobId::from(Uuid::now_v7()),
        post_id: PostId::from(Uuid::now_v7()),
        account: label(account, name),
        park_reason,
    }
}

fn failed(account: AccountId, name: &str) -> Notice {
    Notice::DeliveryFailed(delivery(account, name, None))
}

fn parked(account: AccountId, name: &str) -> Notice {
    Notice::DeliveryParked(delivery(account, name, Some(ParkReason::QuotaExceeded)))
}

async fn record(pool: &PgPool, owner: UserId, notice: Notice) {
    let notifier = Notifier::for_tests(pool.clone());
    let mut tx = pool.begin().await.unwrap();
    notifier
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
}

async fn set_prefs(pool: &PgPool, user: UserId, update: PreferencesUpdate) {
    let mut conn = pool.acquire().await.unwrap();
    UserPreferencesRepo::upsert(&mut conn, user, &update)
        .await
        .unwrap();
}

fn email_off(kind: NotificationKind) -> PreferencesUpdate {
    let mut switches = BTreeMap::new();
    switches.insert(
        kind,
        ChannelSwitch {
            push: None,
            email: Some(false),
        },
    );
    PreferencesUpdate {
        switches: Some(NotificationSwitches(switches)),
        ..PreferencesUpdate::default()
    }
}

/// The `send_email` jobs enqueued so far, oldest first, with their run times.
async fn alert_jobs(pool: &PgPool) -> Vec<(SendEmail, DateTime<Utc>, DateTime<Utc>)> {
    let rows: Vec<(serde_json::Value, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT payload, run_at, created_at FROM jobs WHERE job_type = 'send_email' \
         ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(payload, run_at, created_at)| {
            (serde_json::from_value(payload).unwrap(), run_at, created_at)
        })
        .collect()
}

fn handler(pool: &PgPool, mailer: Arc<dyn Mailer>) -> SendEmailHandler {
    let notifier_pool = pool.clone();
    let queue = postit_jobs::JobQueue::new(
        notifier_pool,
        Arc::new(postit_core::SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut loaders = MailLoaders::default();
    postit_notify::register_loaders(&mut loaders).unwrap();
    SendEmailHandler::new(SendEmailDeps {
        pool: pool.clone(),
        ids: Arc::new(postit_core::SystemIdGenerator),
        mailer,
        loaders,
        outbox: MailOutbox::new(queue),
        app_url: Url::parse("https://app.postit.test").unwrap(),
        max_attempts: 3,
    })
}

fn ctx() -> JobContext {
    JobContext {
        job_id: JobId(Uuid::now_v7()),
        attempt: 1,
        max_attempts: Some(3),
    }
}

async fn run(handler: &SendEmailHandler, job: &SendEmail) -> Result<(), JobError> {
    handler.handle(job.clone(), ctx()).await
}

async fn unemailed(pool: &PgPool, user: UserId) -> usize {
    let mut conn = pool.acquire().await.unwrap();
    notifications_for(&mut conn, user)
        .await
        .iter()
        .filter(|n| n.emailed_at.is_none())
        .count()
}

async fn sent_audits(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = 'email_sent'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn ten_failures_send_one_email(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    for i in 0..10 {
        record(&pool, owner, failed(acct, &format!("Shop {i}"))).await;
    }
    let jobs = alert_jobs(&pool).await;
    assert_eq!(jobs.len(), 10);
    for (job, run_at, created_at) in &jobs {
        assert_eq!(job.kind, MailKind::NotificationAlert);
        assert_eq!(job.recipient, owner.as_uuid());
        let delay = *run_at - *created_at;
        assert!(
            (delay - ALERT_DELAY).num_seconds().abs() <= 5,
            "run_at is {delay} after enqueue"
        );
    }
    let mailer = MemoryMailer::default();
    let handler = handler(&pool, Arc::new(mailer.clone()));
    run(&handler, &jobs[0].0).await.unwrap();
    assert_eq!(unemailed(&pool, owner).await, 0);
    for (job, _, _) in &jobs[1..] {
        run(&handler, job).await.unwrap();
    }
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, ADDRESS);
    assert_eq!(sent[0].subject, "10 things need your attention on postit");
    for i in 0..10 {
        assert!(
            sent[0].text.contains(&format!("Shop {i}")),
            "{}",
            sent[0].text
        );
    }
    assert!(sent[0].text.contains("https://app.postit.test/"));
    assert_eq!(sent_audits(&pool).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn opted_out_kind_never_blocks_later_alerts(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    set_prefs(&pool, owner, email_off(NotificationKind::DeliveryParked)).await;
    record(&pool, owner, parked(acct, "Parked Shop")).await;
    assert!(alert_jobs(&pool).await.is_empty());
    assert_eq!(unemailed(&pool, owner).await, 0);
    record(&pool, owner, failed(acct, "Failed Shop")).await;
    let jobs = alert_jobs(&pool).await;
    assert_eq!(jobs.len(), 1);
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &jobs[0].0)
        .await
        .unwrap();
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("Failed Shop"), "{}", sent[0].text);
    assert!(!sent[0].text.contains("Parked Shop"), "{}", sent[0].text);
    assert_eq!(sent[0].subject, "Something needs your attention on postit");
    assert_eq!(unemailed(&pool, owner).await, 0);
}

/// Sends nothing until released, and says when a send started.
struct GatedMailer {
    inner: MemoryMailer,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl Mailer for GatedMailer {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError> {
        self.entered.notify_one();
        self.release.notified().await;
        self.inner.send(message).await
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn row_recorded_during_a_send_is_emailed_next(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    record(&pool, owner, failed(acct, "First Shop")).await;
    let first = alert_jobs(&pool).await.remove(0).0;
    let inner = MemoryMailer::default();
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let gated = handler(
        &pool,
        Arc::new(GatedMailer {
            inner: inner.clone(),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
    );
    let running = tokio::spawn(async move { run(&gated, &first).await });
    entered.notified().await;
    // The first job holds its transaction (and the row it took) while this row commits.
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        record(&pool, owner, failed(acct, "Second Shop")),
    )
    .await
    .unwrap();
    release.notify_one();
    running.await.unwrap().unwrap();
    let sent = inner.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("First Shop"));
    assert!(!sent[0].text.contains("Second Shop"));
    assert_eq!(unemailed(&pool, owner).await, 1);

    let jobs = alert_jobs(&pool).await;
    assert_eq!(jobs.len(), 2);
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &jobs[1].0)
        .await
        .unwrap();
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("Second Shop"));
    assert!(!sent[0].text.contains("First Shop"));
    assert_eq!(unemailed(&pool, owner).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn email_switch_off_sends_nothing(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    set_prefs(&pool, owner, email_off(NotificationKind::DeliveryFailed)).await;
    record(&pool, owner, failed(acct, "Shop")).await;
    assert!(alert_jobs(&pool).await.is_empty());
    let mut conn = pool.acquire().await.unwrap();
    let rows = notifications_for(&mut conn, owner).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].emailed_at, Some(rows[0].created_at));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn master_switch_off_sends_nothing(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    set_prefs(
        &pool,
        owner,
        PreferencesUpdate {
            email_notifications: Some(false),
            comment_digest: Some(true),
            ..PreferencesUpdate::default()
        },
    )
    .await;
    record(&pool, owner, failed(acct, "Shop")).await;
    record(
        &pool,
        owner,
        Notice::NewComments(postit_core::CommentsNotice {
            account: label(acct, "Shop"),
            count: 2,
        }),
    )
    .await;
    assert!(alert_jobs(&pool).await.is_empty());
    assert_eq!(unemailed(&pool, owner).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn info_kinds_never_email(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    // Even switched on explicitly, an info kind is never emailed.
    let mut switches = BTreeMap::new();
    switches.insert(
        NotificationKind::DeliveryPublished,
        ChannelSwitch {
            push: None,
            email: Some(true),
        },
    );
    set_prefs(
        &pool,
        owner,
        PreferencesUpdate {
            switches: Some(NotificationSwitches(switches)),
            ..PreferencesUpdate::default()
        },
    )
    .await;
    record(
        &pool,
        owner,
        Notice::DeliveryPublished(delivery(acct, "Shop", None)),
    )
    .await;
    record(
        &pool,
        owner,
        Notice::DelegationChanged(DelegationNotice {
            delegation_id: DelegationId::from(Uuid::now_v7()),
            event: DelegationEvent::Granted,
            party_name: "Sam".to_owned(),
        }),
    )
    .await;
    // Comments without the digest are settled at once.
    record(
        &pool,
        owner,
        Notice::NewComments(postit_core::CommentsNotice {
            account: label(acct, "Shop"),
            count: 2,
        }),
    )
    .await;
    assert!(alert_jobs(&pool).await.is_empty());
    assert_eq!(unemailed(&pool, owner).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn comments_wait_for_the_digest_without_an_alert(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    set_prefs(
        &pool,
        owner,
        PreferencesUpdate {
            comment_digest: Some(true),
            ..PreferencesUpdate::default()
        },
    )
    .await;
    record(
        &pool,
        owner,
        Notice::NewComments(postit_core::CommentsNotice {
            account: label(acct, "Shop"),
            count: 2,
        }),
    )
    .await;
    assert!(alert_jobs(&pool).await.is_empty());
    assert_eq!(unemailed(&pool, owner).await, 1);
}

struct FailingMailer;

#[async_trait]
impl Mailer for FailingMailer {
    async fn send(&self, _message: RenderedMessage) -> Result<(), MailError> {
        Err(MailError::Transient("connection refused".into()))
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn smtp_failure_leaves_rows_unemailed(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    record(&pool, owner, failed(acct, "Shop")).await;
    record(&pool, owner, failed(acct, "Shop")).await;
    let jobs = alert_jobs(&pool).await;
    let result = run(&handler(&pool, Arc::new(FailingMailer)), &jobs[0].0).await;
    assert!(matches!(result, Err(JobError::Retry(_))), "{result:?}");
    assert_eq!(unemailed(&pool, owner).await, 2);
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &jobs[0].0)
        .await
        .unwrap();
    assert_eq!(mailer.sent().len(), 1);
    assert_eq!(unemailed(&pool, owner).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn later_job_skips_when_nothing_left(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let acct = account(&pool, owner).await;
    record(&pool, owner, failed(acct, "Shop")).await;
    record(&pool, owner, parked(acct, "Shop")).await;
    let jobs = alert_jobs(&pool).await;
    assert_eq!(jobs.len(), 2);
    let mailer = MemoryMailer::default();
    let handler = handler(&pool, Arc::new(mailer.clone()));
    run(&handler, &jobs[0].0).await.unwrap();
    run(&handler, &jobs[1].0).await.unwrap();
    assert_eq!(mailer.sent().len(), 1);
    assert_eq!(sent_audits(&pool).await, 1);
}

fn fixed_id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// One notice of every critical kind, with fixed IDs and labels.
fn every_critical_kind() -> Vec<Notice> {
    let acct = AccountId::from(fixed_id(1));
    let delivery = |reason| DeliveryNotice {
        delivery_id: DeliveryId::from(fixed_id(2)),
        job_id: PublishJobId::from(fixed_id(3)),
        post_id: PostId::from(fixed_id(4)),
        account: label(acct, "Corner Shop"),
        park_reason: reason,
    };
    let reconnect = ReconnectNotice {
        grant_id: GrantId::from(fixed_id(5)),
        account: label(acct, "Corner Shop"),
        reconnect_by: Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
        stage: None,
    };
    vec![
        Notice::DeliveryDeadLettered(delivery(None)),
        Notice::DeliveryFailed(delivery(None)),
        Notice::DeliveryOutcomeUnknown(delivery(None)),
        Notice::DeliveryParked(delivery(Some(ParkReason::QuotaExceeded))),
        Notice::DeliveryStale(delivery(Some(ParkReason::AccountNeedsReconnect))),
        Notice::JobPartial(JobNotice {
            job_id: PublishJobId::from(fixed_id(3)),
            post_id: PostId::from(fixed_id(4)),
            published: 2,
            failed: 3,
        }),
        Notice::AccountNeedsReconnect(AccountNotice {
            account: label(acct, "Corner Shop"),
        }),
        Notice::AccountRevokedByPlatform(AccountNotice {
            account: label(acct, "Corner Shop"),
        }),
        Notice::ReconnectByPassed(reconnect),
        Notice::RecurrenceSkipped(RecurrenceNotice {
            recurrence_id: RecurrenceId::from(fixed_id(6)),
            post_id: PostId::from(fixed_id(4)),
            reason: SkipReason::JobActive,
        }),
    ]
}

fn render_alert(locale: postit_i18n::Locale) -> String {
    let notices = every_critical_kind();
    assert!(
        notices
            .iter()
            .all(|n| n.kind().severity() == postit_core::Severity::Critical)
    );
    let rendered = render(
        &MailContent::NotificationAlert { notices },
        &Url::parse("https://app.postit.test/app").unwrap(),
        locale,
    )
    .unwrap();
    format!(
        "Subject: {}\n\n--- text ---\n{}\n--- html ---\n{}",
        rendered.subject, rendered.text, rendered.html
    )
}

#[test]
fn alert_en() {
    let out = render_alert(postit_i18n::Locale::EnUs);
    assert!(out.contains("dir=\"ltr\""));
    assert!(out.contains("https://app.postit.test/app/"));
    insta::assert_snapshot!("alert_en", out);
}

#[test]
fn alert_ar() {
    let out = render_alert(postit_i18n::Locale::Ar);
    assert!(out.contains("dir=\"rtl\""));
    assert!(out.contains("<bdi>Corner Shop</bdi>"), "{out}");
    insta::assert_snapshot!("alert_ar", out);
}

#[test]
fn every_critical_line_is_translated() {
    for locale in postit_i18n::Locale::ALL {
        let out = render_alert(locale);
        assert!(!out.contains("notice-"), "{out}");
        assert!(!out.contains("park-reason-"), "{out}");
        assert!(!out.contains("skip-reason-"), "{out}");
    }
}
