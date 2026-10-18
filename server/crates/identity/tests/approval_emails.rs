use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::{UserRecord, UserStatus, UsersRepo};
use postit_identity::mail::PendingApprovalLoader;
use postit_mail::{LoadOutcome, MailContent, MailContextLoader, MailParams, PendingUser};
use sqlx::PgPool;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);

async fn insert_user(
    pool: &PgPool,
    role: &str,
    status: &str,
    name: &str,
    created_at: DateTime<Utc>,
) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name, role, status, approved_at, created_at)
         VALUES ($1, 'https://i.test', $1::text, $2, TRUE, $3, $4, $5,
                 CASE WHEN $5 = 'active' THEN $6 ELSE NULL END, $6)",
    )
    .bind(id)
    .bind(format!("{}@example.com", name.to_lowercase()))
    .bind(name)
    .bind(role)
    .bind(status)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UserPreferencesRepo::create_default(&mut conn, UserId::from(id), "UTC", "en-US")
        .await
        .unwrap_or_else(|e| unreachable!("prefs: {e}"));
    UserId::from(id)
}

async fn record(pool: &PgPool, id: UserId) -> UserRecord {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::find_by_id(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("user missing"))
}

async fn load(pool: &PgPool, admin: UserId, now: DateTime<Utc>) -> LoadOutcome {
    let admin = record(pool, admin).await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    PendingApprovalLoader::new(HOUR)
        .load(&mut conn, &admin, &MailParams::None, now)
        .await
        .unwrap_or_else(|e| unreachable!("load: {e}"))
}

async fn mark(pool: &PgPool, admin: UserId, at: DateTime<Utc>) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    PendingApprovalLoader::new(HOUR)
        .mark_sent(&mut conn, admin, at)
        .await
        .unwrap_or_else(|e| unreachable!("mark: {e}"));
}

fn names(outcome: &LoadOutcome) -> Vec<String> {
    match outcome {
        LoadOutcome::Send(MailContent::UserPendingApproval { pending, .. }) => pending
            .iter()
            .map(|p: &PendingUser| p.display_name.clone())
            .collect(),
        other => unreachable!("expected a pending-approval Send, got {other:?}"),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn five_sign_ups_in_one_interval_make_one_email_then_skips(pool: PgPool) {
    let t0 = Utc::now() - chrono::Duration::minutes(30);
    let admin = insert_user(&pool, "admin", "active", "Admin", t0).await;
    for n in 1..=5 {
        insert_user(
            &pool,
            "member",
            "pending",
            &format!("User{n}"),
            t0 + chrono::Duration::minutes(n),
        )
        .await;
    }
    let now = Utc::now();

    let first = load(&pool, admin, now).await;
    assert_eq!(
        names(&first),
        vec!["User1", "User2", "User3", "User4", "User5"]
    );
    mark(&pool, admin, now).await;

    // The four other queued jobs for this admin find nothing new.
    assert_eq!(load(&pool, admin, now).await, LoadOutcome::Skip);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_sign_up_just_after_an_email_is_deferred_then_listed_alone(pool: PgPool) {
    let t0 = Utc::now() - chrono::Duration::minutes(30);
    let admin = insert_user(&pool, "admin", "active", "Admin", t0).await;
    insert_user(
        &pool,
        "member",
        "pending",
        "Early",
        t0 + chrono::Duration::minutes(1),
    )
    .await;
    let sent_at = Utc::now() - chrono::Duration::minutes(5);
    mark(&pool, admin, sent_at).await;
    insert_user(&pool, "member", "pending", "Late", Utc::now()).await;

    let now = Utc::now();
    let deferred = load(&pool, admin, now).await;
    let expected_at = sent_at + chrono::Duration::hours(1);
    let LoadOutcome::Defer(at) = deferred else {
        unreachable!("expected Defer, got {deferred:?}");
    };
    assert!((at - expected_at).num_milliseconds().abs() < 1);

    let later = load(&pool, admin, expected_at + chrono::Duration::seconds(1)).await;
    assert_eq!(names(&later), vec!["Late"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_user_approved_before_the_email_is_not_listed(pool: PgPool) {
    let t0 = Utc::now() - chrono::Duration::minutes(30);
    let admin = insert_user(&pool, "admin", "active", "Admin", t0).await;
    let approved = insert_user(
        &pool,
        "member",
        "pending",
        "Approved",
        t0 + chrono::Duration::minutes(1),
    )
    .await;
    insert_user(
        &pool,
        "member",
        "pending",
        "Waiting",
        t0 + chrono::Duration::minutes(2),
    )
    .await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::set_status(&mut conn, approved, UserStatus::Active, Some(admin))
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    assert_eq!(
        names(&load(&pool, admin, Utc::now()).await),
        vec!["Waiting"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_new_admin_is_not_sent_the_whole_history(pool: PgPool) {
    let t0 = Utc::now() - chrono::Duration::days(10);
    insert_user(&pool, "member", "pending", "Ancient", t0).await;
    let admin = insert_user(
        &pool,
        "admin",
        "active",
        "Admin",
        Utc::now() - chrono::Duration::days(1),
    )
    .await;
    insert_user(&pool, "member", "pending", "Fresh", Utc::now()).await;

    assert_eq!(names(&load(&pool, admin, Utc::now()).await), vec!["Fresh"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_recipient_no_longer_admin_is_skipped(pool: PgPool) {
    let t0 = Utc::now() - chrono::Duration::minutes(30);
    let former = insert_user(&pool, "member", "active", "Former", t0).await;
    insert_user(&pool, "member", "pending", "Waiting", Utc::now()).await;
    assert_eq!(load(&pool, former, Utc::now()).await, LoadOutcome::Skip);
}

mod send_locale {
    use std::sync::Arc;

    use postit_core::{SystemClock, SystemIdGenerator};
    use postit_i18n::{Args, Locale, t};
    use postit_identity::mail::ApprovedLoader;
    use postit_jobs::{JobContext, JobId, JobQueue};
    use postit_mail::{
        MailKind, MailLoaders, MailOutbox, MemoryMailer, SendEmail, SendEmailDeps, SendEmailHandler,
    };
    use sqlx::PgPool;
    use url::Url;
    use uuid::Uuid;

    use super::{MailParams, UserId};

    /// An active, verified member with an email; no preferences row.
    async fn bare_user(pool: &PgPool) -> UserId {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name, role, status)
             VALUES ($1, 'https://i.test', $1::text, 'ada@example.com', TRUE, 'Ada', 'member', 'active')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
        UserId::from(id)
    }

    async fn set_locale(pool: &PgPool, id: UserId, tag: &str) {
        sqlx::query(
            "INSERT INTO user_preferences (user_id, timezone, locale) VALUES ($1, 'UTC', $2)",
        )
        .bind(id.as_uuid())
        .bind(tag)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("prefs: {e}"));
    }

    async fn send_approved(pool: &PgPool, id: UserId) -> String {
        let mailer = MemoryMailer::default();
        let queue = JobQueue::new(
            pool.clone(),
            Arc::new(SystemIdGenerator),
            Arc::new(SystemClock),
        );
        let mut loaders = MailLoaders::default();
        loaders
            .register(MailKind::UserApproved, Arc::new(ApprovedLoader))
            .unwrap_or_else(|e| unreachable!("register: {e}"));
        let handler = SendEmailHandler::new(SendEmailDeps {
            pool: pool.clone(),
            ids: Arc::new(SystemIdGenerator),
            mailer: Arc::new(mailer.clone()),
            loaders,
            outbox: MailOutbox::new(queue),
            app_url: Url::parse("https://app.postit.test")
                .unwrap_or_else(|e| unreachable!("url: {e}")),
            max_attempts: 3,
        });
        handler
            .handle(
                SendEmail {
                    kind: MailKind::UserApproved,
                    recipient: id.as_uuid(),
                    params: MailParams::None,
                },
                JobContext {
                    job_id: JobId(Uuid::now_v7()),
                    attempt: 1,
                    max_attempts: Some(3),
                },
            )
            .await
            .unwrap_or_else(|e| unreachable!("handle: {e}"));
        let sent = mailer.sent();
        assert_eq!(sent.len(), 1);
        sent[0].subject.clone()
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn approved_email_uses_recipient_locale(pool: PgPool) {
        let id = bare_user(&pool).await;
        set_locale(&pool, id, "ar").await;
        let subject = send_approved(&pool, id).await;
        assert_eq!(subject, t(Locale::Ar, "approved-subject", &Args::new()));
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn missing_preferences_sends_english(pool: PgPool) {
        let id = bare_user(&pool).await;
        let subject = send_approved(&pool, id).await;
        assert_eq!(subject, t(Locale::EnUs, "approved-subject", &Args::new()));
    }
}
