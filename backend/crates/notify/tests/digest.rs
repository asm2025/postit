//! The daily comment digest: when it is enqueued (local hour 8, once per local day, DST-safe)
//! and what it says.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use postit_core::{
    AccountId, AccountLabel, CommentsNotice, Notice, PlatformId, SystemClock, SystemIdGenerator,
    UserId,
};
use postit_data::preferences::{PreferencesUpdate, UserPreferencesRepo};
use postit_data::testkit::seed_user;
use postit_jobs::{JobContext, JobError, JobId, JobQueue};
use postit_mail::{
    MailContent, MailError, MailKind, MailLoaders, MailOutbox, MailParams, Mailer, MemoryMailer,
    RenderedMessage, SendEmail, SendEmailDeps, SendEmailHandler, render,
};
use postit_notify::{DIGEST_HOUR, Notifier, NotifyEvent, Recipients, enqueue_due_digests};
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

async fn account(pool: &PgPool, owner: UserId, name: &str) -> AccountId {
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
         VALUES ($1, $2, 'mastodon', $3, $4)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(id.to_string())
    .bind(name)
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

async fn comments(pool: &PgPool, owner: UserId, account: AccountId, name: &str, count: u32) {
    let notifier = Notifier::for_tests(pool.clone());
    let mut tx = pool.begin().await.unwrap();
    notifier
        .record(
            &mut tx,
            NotifyEvent {
                recipients: Recipients::Workspace { owner, actor: None },
                notice: Notice::NewComments(CommentsNotice {
                    account: label(account, name),
                    count,
                }),
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn prefs(pool: &PgPool, user: UserId, timezone: &str, digest: bool, email: bool) {
    let mut conn = pool.acquire().await.unwrap();
    UserPreferencesRepo::upsert(
        &mut conn,
        user,
        &PreferencesUpdate {
            timezone: Some(timezone.to_owned()),
            comment_digest: Some(digest),
            email_notifications: Some(email),
            ..PreferencesUpdate::default()
        },
    )
    .await
    .unwrap();
}

fn outbox(pool: &PgPool) -> MailOutbox {
    MailOutbox::new(JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(SystemClock),
    ))
}

async fn digest_job_count(pool: &PgPool) -> usize {
    let rows: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE job_type = 'send_email'")
            .fetch_all(pool)
            .await
            .unwrap();
    rows.into_iter()
        .map(|p| serde_json::from_value::<SendEmail>(p).unwrap())
        .filter(|j| j.kind == MailKind::CommentDigest)
        .count()
}

fn handler(pool: &PgPool, mailer: Arc<dyn Mailer>) -> SendEmailHandler {
    let mut loaders = MailLoaders::default();
    postit_notify::register_loaders(&mut loaders).unwrap();
    SendEmailHandler::new(SendEmailDeps {
        pool: pool.clone(),
        ids: Arc::new(SystemIdGenerator),
        mailer,
        loaders,
        outbox: outbox(pool),
        app_url: Url::parse("https://app.postit.test").unwrap(),
        max_attempts: 3,
    })
}

async fn run(handler: &SendEmailHandler, job: &SendEmail) -> Result<(), JobError> {
    handler
        .handle(
            job.clone(),
            JobContext {
                job_id: JobId(Uuid::now_v7()),
                attempt: 1,
                max_attempts: Some(3),
            },
        )
        .await
}

fn job_for(user: UserId) -> SendEmail {
    SendEmail {
        kind: MailKind::CommentDigest,
        recipient: user.as_uuid(),
        params: MailParams::None,
    }
}

/// Runs the cron at minute 0 UTC of every hour in `[from, from + hours)`, as the worker would,
/// and returns the ticks that enqueued a digest. `mark_sent` is simulated at each such tick.
async fn simulate(
    pool: &PgPool,
    user: UserId,
    from: DateTime<Utc>,
    hours: i64,
) -> Vec<DateTime<Utc>> {
    let outbox = outbox(pool);
    let mut fired = Vec::new();
    for h in 0..hours {
        let tick = from + Duration::hours(h);
        let n = enqueue_due_digests(pool, &outbox, tick).await.unwrap();
        if n > 0 {
            fired.push(tick);
            let mut conn = pool.acquire().await.unwrap();
            UserPreferencesRepo::set_last_comment_digest_at(&mut conn, user, tick)
                .await
                .unwrap();
        }
    }
    fired
}

fn local(tick: DateTime<Utc>, zone: &str) -> (String, u32, u32) {
    let tz: Tz = zone.parse().unwrap();
    let t = tick.with_timezone(&tz);
    (t.format("%Y-%m-%d").to_string(), t.hour(), t.minute())
}

async fn check_zone(pool: &PgPool, zone: &str, from: DateTime<Utc>, hours: i64, minute: u32) {
    let user = verified_user(pool).await;
    prefs(pool, user, zone, true, true).await;
    let fired = simulate(pool, user, from, hours).await;
    let days: Vec<_> = fired.iter().map(|t| local(*t, zone)).collect();
    assert_eq!(days.len(), usize::try_from(hours / 24).unwrap(), "{days:?}");
    let mut dates: Vec<_> = days.iter().map(|d| d.0.clone()).collect();
    dates.dedup();
    assert_eq!(dates.len(), days.len(), "twice in one local day: {days:?}");
    for (date, hour, min) in &days {
        assert_eq!((*hour, *min), (DIGEST_HOUR, minute), "{date} in {zone}");
    }
    // Leave the next zone a clean slate.
    sqlx::query("UPDATE user_preferences SET comment_digest = FALSE WHERE user_id = $1")
        .bind(user.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn digest_runs_at_local_eight_for_offset_zones(pool: PgPool) {
    assert_eq!(DIGEST_HOUR, 8);
    let start = |m, d| Utc.with_ymd_and_hms(2026, m, d, 0, 0, 0).unwrap();
    // 08:00 in Riyadh (no DST) and 08:30 in Kolkata (the cron runs at minute 0 UTC and
    // Kolkata is +05:30), over four days each.
    check_zone(&pool, "Asia/Riyadh", start(6, 1), 96, 0).await;
    check_zone(&pool, "Asia/Kolkata", start(6, 1), 96, 30).await;
    // Spring forward (2026-03-08) and fall back (2026-11-01) in New York.
    check_zone(&pool, "America/New_York", start(3, 6), 96, 0).await;
    check_zone(&pool, "America/New_York", start(10, 30), 96, 0).await;
    // An unknown zone falls back to UTC instead of failing the run.
    let user = verified_user(&pool).await;
    prefs(&pool, user, "Not/AZone", true, true).await;
    let fired = simulate(&pool, user, start(7, 1), 48).await;
    assert_eq!(
        fired,
        vec![
            Utc.with_ymd_and_hms(2026, 7, 1, 8, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 2, 8, 0, 0).unwrap(),
        ]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn digest_off_sends_nothing(pool: PgPool) {
    let user = verified_user(&pool).await;
    let tick = Utc.with_ymd_and_hms(2026, 6, 1, 8, 0, 0).unwrap();
    let enqueue = || async {
        enqueue_due_digests(&pool, &outbox(&pool), tick)
            .await
            .unwrap()
    };
    prefs(&pool, user, "UTC", false, true).await;
    assert_eq!(enqueue().await, 0);
    // The master switch off beats the digest switch.
    prefs(&pool, user, "UTC", true, false).await;
    assert_eq!(enqueue().await, 0);
    assert_eq!(digest_job_count(&pool).await, 0);
    // A user who is not active gets none either.
    prefs(&pool, user, "UTC", true, true).await;
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(user.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(enqueue().await, 0);
    sqlx::query("UPDATE users SET status = 'active' WHERE id = $1")
        .bind(user.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(enqueue().await, 1);
}

async fn last_digest_at(pool: &PgPool, user: UserId) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT last_comment_digest_at FROM user_preferences WHERE user_id = $1")
        .bind(user.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn counts_are_summed_per_account(pool: PgPool) {
    let owner = verified_user(&pool).await;
    prefs(&pool, owner, "UTC", true, true).await;
    let shop = account(&pool, owner, "Shop").await;
    let blog = account(&pool, owner, "Blog").await;
    comments(&pool, owner, shop, "Shop", 2).await;
    comments(&pool, owner, blog, "Blog", 1).await;
    comments(&pool, owner, shop, "Shop", 3).await;
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &job_for(owner))
        .await
        .unwrap();
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, ADDRESS);
    assert_eq!(sent[0].subject, "You have 6 new comments on postit");
    assert!(
        sent[0]
            .text
            .contains("Shop on mastodon has 5 new comments."),
        "{}",
        sent[0].text
    );
    assert!(
        sent[0].text.contains("Blog on mastodon has a new comment."),
        "{}",
        sent[0].text
    );
    assert_eq!(sent[0].text.matches("Shop").count(), 1, "{}", sent[0].text);
    assert!(last_digest_at(&pool, owner).await.is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_comments_skips(pool: PgPool) {
    let owner = verified_user(&pool).await;
    prefs(&pool, owner, "UTC", true, true).await;
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &job_for(owner))
        .await
        .unwrap();
    assert!(mailer.sent().is_empty());
    assert!(last_digest_at(&pool, owner).await.is_none());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn comments_recorded_with_digest_off_are_never_digested(pool: PgPool) {
    let owner = verified_user(&pool).await;
    let shop = account(&pool, owner, "Shop").await;
    prefs(&pool, owner, "UTC", false, true).await;
    comments(&pool, owner, shop, "Shop", 4).await;
    prefs(&pool, owner, "UTC", true, true).await;
    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &job_for(owner))
        .await
        .unwrap();
    assert!(mailer.sent().is_empty());
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
async fn late_committed_comment_is_in_the_next_digest(pool: PgPool) {
    let owner = verified_user(&pool).await;
    prefs(&pool, owner, "UTC", true, true).await;
    let shop = account(&pool, owner, "Shop").await;
    comments(&pool, owner, shop, "Shop", 1).await;
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
    let job = job_for(owner);
    let running = tokio::spawn(async move { run(&gated, &job).await });
    entered.notified().await;
    // The first job holds its transaction (and the row it took) while this row commits.
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        comments(&pool, owner, shop, "Late Shop", 7),
    )
    .await
    .unwrap();
    release.notify_one();
    running.await.unwrap().unwrap();
    let sent = inner.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("a new comment"), "{}", sent[0].text);
    assert!(!sent[0].text.contains("Late Shop"));

    let mailer = MemoryMailer::default();
    run(&handler(&pool, Arc::new(mailer.clone())), &job_for(owner))
        .await
        .unwrap();
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("Late Shop"), "{}", sent[0].text);
    assert!(sent[0].text.contains("7 new comments"));
}

fn digest(locale: postit_i18n::Locale, names: &[&str]) -> String {
    let comments = names
        .iter()
        .enumerate()
        .map(|(i, name)| CommentsNotice {
            account: label(AccountId::from(Uuid::from_u128(i as u128 + 1)), name),
            count: u32::try_from(i).unwrap() + 2,
        })
        .collect();
    let rendered = render(
        &MailContent::CommentDigest { comments },
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
fn digest_en() {
    let out = digest(postit_i18n::Locale::EnUs, &["Corner Shop", "Blog"]);
    assert!(out.contains("dir=\"ltr\""));
    insta::assert_snapshot!("digest_en", out);
}

#[test]
fn digest_ar() {
    let out = digest(postit_i18n::Locale::Ar, &["Corner Shop", "Blog"]);
    assert!(out.contains("dir=\"rtl\""));
    assert!(out.contains("<bdi>Corner Shop</bdi>"), "{out}");
    insta::assert_snapshot!("digest_ar", out);
}

#[test]
fn hostile_label_is_escaped() {
    for locale in postit_i18n::Locale::ALL {
        let out = digest(locale, &["<script>&\"'"]);
        let (_, out) = out.split_once("--- html ---").unwrap();
        assert!(!out.contains("<script>"), "{out}");
        assert!(out.contains("&#60;script&#62;&#38;"), "{out}");
    }
}

#[test]
fn long_digest_is_capped_with_a_more_line() {
    let names: Vec<String> = (0..25).map(|i| format!("Shop{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let out = digest(postit_i18n::Locale::EnUs, &refs);
    assert!(out.contains("Shop19"), "{out}");
    assert!(!out.contains("Shop20"), "{out}");
    assert!(out.contains("...and 5 more."), "{out}");
    let ar = digest(postit_i18n::Locale::Ar, &refs);
    assert!(!ar.contains("Shop20"), "{ar}");
    assert!(!ar.contains("...and 5 more."), "{ar}");
}
