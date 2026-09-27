use std::sync::Arc;

use postit_core::{IdGenerator, SystemIdGenerator, UserId};
use postit_data::pseudonym::pseudonym_for;
use postit_data::users::{UserStatus, UsersRepo};
use postit_identity::IdentityError;
use postit_identity::admin::UserAdminService;
use postit_identity::deletion::{DeleteUser, DeleteUserHandler};
use postit_jobs::{JobContext, JobId, JobQueue};
use postit_mail::MailOutbox;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &str = "deletion-test-pseudonym-key";

fn service(pool: &PgPool) -> UserAdminService {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
    UserAdminService::new(pool.clone(), ids, jobs.clone(), MailOutbox::new(jobs))
}

fn handler(pool: &PgPool) -> DeleteUserHandler {
    DeleteUserHandler::new(pool.clone(), SecretString::from(KEY.to_string()))
}

fn ctx() -> JobContext {
    JobContext {
        job_id: JobId(Uuid::now_v7()),
        attempt: 1,
        max_attempts: None,
    }
}

async fn user(pool: &PgPool, sub: &str, role: &str, status: &str) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
         VALUES ($1, 'https://i.test', $2, $2, $3, $4)",
    )
    .bind(id)
    .bind(sub)
    .bind(role)
    .bind(status)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    sqlx::query("INSERT INTO user_preferences (user_id) VALUES ($1)")
        .bind(id)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("prefs: {e}"));
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

async fn queued_deletions(pool: &PgPool) -> Vec<Uuid> {
    let payloads: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM job_outbox WHERE job_type = 'delete_user'")
            .fetch_all(pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    payloads
        .iter()
        .filter_map(|p| p["user_id"].as_str().and_then(|s| Uuid::parse_str(s).ok()))
        .collect()
}

async fn raw_references(pool: &PgPool, id: UserId) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE actor_user_id = $1 OR owner_id = $1 OR subject_user_id = $1
            OR details::text LIKE '%' || $1::text || '%'",
    )
    .bind(id.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"))
}

async fn assert_fully_deleted(pool: &PgPool, id: UserId) {
    assert_eq!(status(pool, id).await, None);
    let prefs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_preferences WHERE user_id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("prefs: {e}"));
    assert_eq!(prefs, 0);
    assert_eq!(raw_references(pool, id).await, 0);
    let pseudo = pseudonym_for(KEY.as_bytes(), id);
    let deleted_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE kind = 'user_deleted' AND subject_user_id = $1",
    )
    .bind(pseudo)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("user_deleted: {e}"));
    assert_eq!(deleted_events, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_deletion_marks_deleting_enqueues_and_the_job_finishes_it(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    let member = user(&pool, "member", "member", "active").await;

    service(&pool)
        .delete_user(admin, member)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    assert_eq!(status(&pool, member).await, Some(UserStatus::Deleting));
    assert_eq!(queued_deletions(&pool).await, vec![member.as_uuid()]);

    handler(&pool)
        .handle(
            DeleteUser {
                user_id: member.as_uuid(),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("job: {e}"));
    assert_fully_deleted(&pool, member).await;
    // The admin's own references are untouched.
    assert!(raw_references(&pool, admin).await > 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn self_deletion_runs_the_same_job(pool: PgPool) {
    let _admin = user(&pool, "admin", "admin", "active").await;
    let member = user(&pool, "member", "member", "active").await;
    service(&pool)
        .delete_self(member)
        .await
        .unwrap_or_else(|e| unreachable!("delete_self: {e}"));
    handler(&pool)
        .handle(
            DeleteUser {
                user_id: member.as_uuid(),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("job: {e}"));
    assert_fully_deleted(&pool, member).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn both_paths_resume_after_a_failure_at_every_step(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    for step in 1..=3u8 {
        for self_delete in [false, true] {
            let target = user(
                &pool,
                &format!("t-{step}-{self_delete}"),
                "member",
                "active",
            )
            .await;
            if self_delete {
                service(&pool).delete_self(target).await
            } else {
                service(&pool).delete_user(admin, target).await
            }
            .unwrap_or_else(|e| unreachable!("start deletion: {e}"));

            let job = || DeleteUser {
                user_id: target.as_uuid(),
            };
            let crashed = handler(&pool)
                .failing_after(step)
                .handle(job(), ctx())
                .await;
            assert!(crashed.is_err(), "step {step} should have failed");
            handler(&pool)
                .handle(job(), ctx())
                .await
                .unwrap_or_else(|e| unreachable!("resume after step {step}: {e}"));
            assert_fully_deleted(&pool, target).await;
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_deleted_admins_approvals_become_null(pool: PgPool) {
    let first_admin = user(&pool, "admin-1", "admin", "active").await;
    let second_admin = user(&pool, "admin-2", "admin", "active").await;
    let member = user(&pool, "member", "member", "pending").await;
    service(&pool)
        .approve(first_admin, member)
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    service(&pool)
        .delete_user(second_admin, first_admin)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    handler(&pool)
        .handle(
            DeleteUser {
                user_id: first_admin.as_uuid(),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("job: {e}"));

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let row = UsersRepo::find_by_id(&mut conn, member)
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("member vanished"));
    assert_eq!(row.approved_by, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn guards(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    let member = user(&pool, "member", "member", "active").await;
    let svc = service(&pool);

    assert!(matches!(
        svc.delete_user(admin, admin).await,
        Err(IdentityError::CannotDeleteSelf)
    ));
    assert!(matches!(
        svc.delete_self(admin).await,
        Err(IdentityError::LastAdmin)
    ));

    svc.delete_user(admin, member)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    assert!(matches!(
        svc.delete_user(admin, member).await,
        Err(IdentityError::UserDeleting)
    ));
    assert!(matches!(
        svc.delete_self(member).await,
        Err(IdentityError::UserDeleting)
    ));

    let other_admin = user(&pool, "admin-2", "admin", "active").await;
    svc.delete_user(other_admin, admin)
        .await
        .unwrap_or_else(|e| unreachable!("delete admin: {e}"));
    assert!(matches!(
        svc.delete_self(other_admin).await,
        Err(IdentityError::LastAdmin)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_job_for_a_non_deleting_user_is_fatal_and_changes_nothing(pool: PgPool) {
    let member = user(&pool, "member", "member", "active").await;
    let result = handler(&pool)
        .handle(
            DeleteUser {
                user_id: member.as_uuid(),
            },
            ctx(),
        )
        .await;
    assert!(matches!(result, Err(postit_jobs::JobError::Fatal(_))));
    assert_eq!(status(&pool, member).await, Some(UserStatus::Active));
}
