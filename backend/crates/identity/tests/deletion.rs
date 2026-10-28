use std::sync::{Arc, Mutex};

use postit_core::{AccountId, DelegationId, IdGenerator, SystemClock, SystemIdGenerator, UserId};
use postit_data::audit::AuditContext;
use postit_data::pseudonym::pseudonym_for;
use postit_data::users::{UserStatus, UsersRepo};
use postit_data::{Level, OwnerScope};
use postit_identity::IdentityError;
use postit_identity::admin::UserAdminService;
use postit_identity::delegation::{AccountsInput, DelegationService, GrantRequest};
use postit_identity::deletion::{DeleteUser, DeleteUserHandler, UserDeletionStep};
use postit_jobs::{JobContext, JobId, JobQueue};
use postit_mail::MailOutbox;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &str = "deletion-test-pseudonym-key";

fn service(pool: &PgPool) -> UserAdminService {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let jobs = JobQueue::new(
        pool.clone(),
        Arc::clone(&ids),
        Arc::new(postit_core::SystemClock),
    );
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
        .delete_user(admin, member, AuditContext::default())
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
        .delete_self(member, AuditContext::default())
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
                service(&pool)
                    .delete_self(target, AuditContext::default())
                    .await
            } else {
                service(&pool)
                    .delete_user(admin, target, AuditContext::default())
                    .await
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
        .approve(first_admin, member, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    service(&pool)
        .delete_user(second_admin, first_admin, AuditContext::default())
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
        svc.delete_user(admin, admin, AuditContext::default()).await,
        Err(IdentityError::CannotDeleteSelf)
    ));
    assert!(matches!(
        svc.delete_self(admin, AuditContext::default()).await,
        Err(IdentityError::LastAdmin)
    ));

    svc.delete_user(admin, member, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    assert!(matches!(
        svc.delete_user(admin, member, AuditContext::default())
            .await,
        Err(IdentityError::UserDeleting)
    ));
    assert!(matches!(
        svc.delete_self(member, AuditContext::default()).await,
        Err(IdentityError::UserDeleting)
    ));

    let other_admin = user(&pool, "admin-2", "admin", "active").await;
    svc.delete_user(other_admin, admin, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("delete admin: {e}"));
    assert!(matches!(
        svc.delete_self(other_admin, AuditContext::default()).await,
        Err(IdentityError::LastAdmin)
    ));
}

/// A `send_email` job can lock the still-`deleting` row and write a raw-ID audit event
/// (`EmailDropped`/`EmailFailed`) in the gap between step 2's commit and step 3's delete.
/// Step 3's re-scrub must catch it before the row is deleted.
#[sqlx::test(migrations = "../data/migrations")]
async fn a_raw_id_written_after_pseudonymization_is_scrubbed_before_delete(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    let member = user(&pool, "member", "member", "active").await;

    service(&pool)
        .delete_user(admin, member, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));

    let crashed = handler(&pool)
        .failing_after(2)
        .handle(
            DeleteUser {
                user_id: member.as_uuid(),
            },
            ctx(),
        )
        .await;
    assert!(crashed.is_err(), "step 2 should have failed");

    // Simulate a concurrent send_email job writing a raw-ID audit row in the gap.
    let audit_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO audit_events (id, subject_user_id, kind, details)
         VALUES ($1, $2, 'email_dropped', '{}'::jsonb)",
    )
    .bind(audit_id)
    .bind(member.as_uuid())
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert gap audit: {e}"));

    handler(&pool)
        .handle(
            DeleteUser {
                user_id: member.as_uuid(),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("resume: {e}"));

    assert_fully_deleted(&pool, member).await;
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

/// A deletion step that records its name each time it runs.
struct Recording {
    name: &'static str,
    log: Arc<Mutex<Vec<(&'static str, UserId)>>>,
}

#[async_trait::async_trait]
impl UserDeletionStep for Recording {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run(&self, user: UserId) -> Result<(), postit_jobs::JobError> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((self.name, user));
        Ok(())
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deletion_steps_run_in_order_and_resume(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    let member = user(&pool, "member", "member", "active").await;
    service(&pool)
        .delete_user(admin, member, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    let log = Arc::new(Mutex::new(Vec::new()));
    let steps = || -> Vec<Arc<dyn UserDeletionStep>> {
        vec![
            Arc::new(Recording {
                name: "first",
                log: Arc::clone(&log),
            }),
            Arc::new(Recording {
                name: "second",
                log: Arc::clone(&log),
            }),
        ]
    };
    let job = || DeleteUser {
        user_id: member.as_uuid(),
    };
    let ran = || {
        log.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    };

    // Checkpoint 1 fires before the steps, so failing after step 2 proves they ran first.
    let crashed = handler(&pool)
        .with_steps(steps())
        .failing_after(2)
        .handle(job(), ctx())
        .await;
    assert!(crashed.is_err());
    assert_eq!(ran(), vec![("first", member), ("second", member)]);
    assert_eq!(status(&pool, member).await, Some(UserStatus::Deleting));

    handler(&pool)
        .with_steps(steps())
        .handle(job(), ctx())
        .await
        .unwrap_or_else(|e| unreachable!("resume: {e}"));
    assert_eq!(
        ran(),
        vec![
            ("first", member),
            ("second", member),
            ("first", member),
            ("second", member)
        ]
    );
    assert_fully_deleted(&pool, member).await;
}

async fn connected_account(pool: &PgPool, owner: UserId) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("platform: {e}"));
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, $3)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(id.to_string())
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("account: {e}"));
    AccountId::from(id)
}

async fn grant_rows(pool: &PgPool, id: DelegationId) -> (i64, i64) {
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM delegations WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("grants: {e}"));
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM delegation_accounts WHERE delegation_id = $1")
            .bind(id.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| unreachable!("links: {e}"));
    (grants, links)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_user_removes_grants_in_both_directions(pool: PgPool) {
    let admin = user(&pool, "admin", "admin", "active").await;
    let doomed = user(&pool, "doomed", "member", "active").await;
    let bob = user(&pool, "bob", "member", "active").await;
    let carol = user(&pool, "carol", "member", "active").await;
    let doomed_account = connected_account(&pool, doomed).await;
    let bob_account = connected_account(&pool, bob).await;
    let carol_account = connected_account(&pool, carol).await;

    let delegations = DelegationService::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(SystemClock),
    );
    let grant = |owner: UserId, delegate: UserId, account: AccountId| {
        let delegations = &delegations;
        async move {
            delegations
                .grant(
                    &OwnerScope::own(owner),
                    GrantRequest {
                        delegate_id: delegate,
                        level: Level::Edit,
                        accounts: AccountsInput::Only(vec![account]),
                        expires_at: None,
                    },
                    AuditContext::default(),
                )
                .await
                .unwrap_or_else(|e| unreachable!("grant: {e}"))
                .delegation
                .id
        }
    };
    let given = grant(doomed, bob, doomed_account).await;
    let received = grant(carol, doomed, carol_account).await;
    let carol_to_bob = grant(carol, bob, carol_account).await;
    let bob_to_carol = grant(bob, carol, bob_account).await;

    service(&pool)
        .delete_user(admin, doomed, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    handler(&pool)
        .handle(
            DeleteUser {
                user_id: doomed.as_uuid(),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("job: {e}"));
    assert_fully_deleted(&pool, doomed).await;

    assert_eq!(grant_rows(&pool, given).await, (0, 0));
    assert_eq!(grant_rows(&pool, received).await, (0, 0));
    assert_eq!(grant_rows(&pool, carol_to_bob).await, (1, 1));
    assert_eq!(grant_rows(&pool, bob_to_carol).await, (1, 1));
}
