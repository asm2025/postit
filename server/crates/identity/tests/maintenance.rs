use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use postit_config::{AuditSettings, JobSchedules, RedactedSecret};
use postit_core::{IdGenerator, SystemIdGenerator, UserId};
use postit_data::users::{UserStatus, UsersRepo};
use postit_identity::jobs::{IdentityJobs, register};
use postit_identity::maintenance::{audit_retention, purge_pending_users};
use postit_jobs::{JobQueue, JobRegistry};
use postit_mail::{MailKind, MailLoaders};
use sqlx::PgPool;
use uuid::Uuid;

async fn user(pool: &PgPool, sub: &str, status: &str, age_days: i32) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status, created_at)
         VALUES ($1, 'https://i.test', $2, $2, 'member', $3, now() - make_interval(days => $4))",
    )
    .bind(id)
    .bind(sub)
    .bind(status)
    .bind(age_days)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    UserId::from(id)
}

async fn status(pool: &PgPool, id: UserId) -> Option<UserStatus> {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::find_by_id(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .map(|u| u.status)
}

fn schedules() -> JobSchedules {
    postit_jobs::testkit::jobs_settings().schedules
}

fn audit_settings() -> AuditSettings {
    AuditSettings {
        retention: Duration::from_hours(365 * 24),
        ip_retention: Duration::from_hours(90 * 24),
        pseudonym_key: RedactedSecret::from("k".to_string()),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_pending_users_deletes_only_stale_pending_users(pool: PgPool) {
    let stale = user(&pool, "stale", "pending", 40).await;
    let fresh = user(&pool, "fresh", "pending", 1).await;
    let old_active = user(&pool, "old-active", "active", 400).await;
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));

    let purged = purge_pending_users(
        &pool,
        ids.as_ref(),
        &jobs,
        Duration::from_hours(30 * 24),
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("purge: {e}"));
    assert_eq!(purged, 1);

    assert_eq!(status(&pool, stale).await, Some(UserStatus::Deleting));
    assert_eq!(status(&pool, fresh).await, Some(UserStatus::Pending));
    assert_eq!(status(&pool, old_active).await, Some(UserStatus::Active));

    let queued: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM job_outbox WHERE job_type = 'delete_user'")
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0]["user_id"], stale.as_uuid().to_string());
    let actor: Option<Uuid> =
        sqlx::query_scalar("SELECT actor_user_id FROM audit_events WHERE kind = 'user_deleted'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("audit: {e}"));
    assert_eq!(actor, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn audit_retention_applies_both_windows(pool: PgPool) {
    for (days, ip) in [
        (10, Some("203.0.113.1")),
        (100, Some("203.0.113.2")),
        (400, None),
    ] {
        sqlx::query(
            "INSERT INTO audit_events (id, at, kind, ip) VALUES ($1, now() - make_interval(days => $2), 'user_enabled', $3)",
        )
        .bind(Uuid::now_v7())
        .bind(days)
        .bind(ip)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    }

    audit_retention(&pool, &audit_settings())
        .await
        .unwrap_or_else(|e| unreachable!("retention: {e}"));

    let rows: Vec<Option<String>> =
        sqlx::query_scalar("SELECT ip FROM audit_events ORDER BY at DESC")
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert_eq!(rows, vec![Some("203.0.113.1".to_string()), None]);
}

// `PgPool::connect_lazy` needs a Tokio context to construct its lazy connector in this
// sqlx version, even though it never actually connects, so this runs under `#[tokio::test]`
// rather than the brief's plain `#[test]`.
#[tokio::test]
async fn register_adds_every_identity_job_and_both_loaders() {
    let pool = PgPool::connect_lazy("postgres://localhost/unused")
        .unwrap_or_else(|e| unreachable!("pool: {e}"));
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    register(
        &mut registry,
        &mut loaders,
        IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(&ids),
            jobs: JobQueue::new(pool, ids),
            pending_ttl: Duration::from_hours(30 * 24),
            approval_email_interval: Duration::from_hours(1),
            audit: audit_settings(),
            schedules: schedules(),
        },
    )
    .unwrap_or_else(|e| unreachable!("register: {e}"));

    assert_eq!(
        registry.job_types(),
        vec![
            "audit_retention".to_string(),
            "delete_user".to_string(),
            "purge_pending_users".to_string()
        ]
    );
    assert_eq!(
        loaders.kinds(),
        vec![MailKind::UserApproved, MailKind::UserPendingApproval]
    );
}
