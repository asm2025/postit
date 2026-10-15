use std::sync::Arc;

use postit_core::{SystemIdGenerator, UserId};
use postit_data::audit::AuditContext;
use postit_data::users::{UserRole, UserStatus, UsersRepo};
use postit_identity::IdentityError;
use postit_identity::admin::UserAdminService;
use sqlx::PgPool;

async fn provisioned_pending_user(pool: &PgPool, sub: &str) -> UserId {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://issuer.test", sub, "Name")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

fn service(pool: PgPool) -> UserAdminService {
    let ids: Arc<dyn postit_core::IdGenerator> = Arc::new(SystemIdGenerator);
    let jobs = postit_jobs::JobQueue::new(
        pool.clone(),
        Arc::clone(&ids),
        Arc::new(postit_core::SystemClock),
    );
    UserAdminService::new(pool, ids, jobs.clone(), postit_mail::MailOutbox::new(jobs))
}

async fn set_deleting(pool: &PgPool, id: UserId) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::set_status(&mut conn, id, UserStatus::Deleting, None)
        .await
        .unwrap_or_else(|e| unreachable!("set deleting: {e}"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn approve_moves_pending_to_active(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let user = service(pool)
        .approve(admin_actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(user.approved_by, Some(admin_actor));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn approving_a_non_pending_user_is_rejected(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;
    let svc = service(pool);
    svc.approve(admin_actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("first approve: {e}"));

    let Err(err) = svc
        .approve(admin_actor, target, AuditContext::default())
        .await
    else {
        unreachable!("second approve unexpectedly succeeded");
    };
    assert!(matches!(
        err,
        IdentityError::InvalidTransition("active", "active")
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disable_then_enable_round_trips(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;
    let svc = service(pool);
    svc.approve(admin_actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    let disabled = svc
        .disable(admin_actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("disable: {e}"));
    assert_eq!(disabled.status, UserStatus::Disabled);

    let enabled = svc
        .enable(admin_actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("enable: {e}"));
    assert_eq!(enabled.status, UserStatus::Active);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabling_the_last_active_admin_is_refused(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut conn,
        admin_id,
        "https://issuer.test",
        "admin-sub",
        "Admin",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, admin_id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    let Err(err) = service(pool)
        .disable(admin_id, admin_id, AuditContext::default())
        .await
    else {
        unreachable!("disable unexpectedly succeeded");
    };
    assert!(matches!(err, IdentityError::LastAdmin));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn demoting_the_last_active_admin_is_refused(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut conn,
        admin_id,
        "https://issuer.test",
        "admin-sub",
        "Admin",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, admin_id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    let Err(err) = service(pool)
        .change_role(
            admin_id,
            admin_id,
            UserRole::Member,
            AuditContext::default(),
        )
        .await
    else {
        unreachable!("change_role unexpectedly succeeded");
    };
    assert!(matches!(err, IdentityError::LastAdmin));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn demoting_a_second_admin_is_allowed(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let first_admin = UserId::from(uuid::Uuid::now_v7());
    let second_admin = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut conn,
        first_admin,
        "https://issuer.test",
        "first-sub",
        "First",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision first: {e}"));
    UsersRepo::grant_admin(&mut conn, first_admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant first: {e}"));
    UsersRepo::provision(
        &mut conn,
        second_admin,
        "https://issuer.test",
        "second-sub",
        "Second",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision second: {e}"));
    UsersRepo::grant_admin(&mut conn, second_admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant second: {e}"));

    let updated = service(pool)
        .change_role(
            first_admin,
            second_admin,
            UserRole::Member,
            AuditContext::default(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("change_role: {e}"));

    assert_eq!(updated.role, UserRole::Member);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn changing_role_of_a_pending_user_is_rejected(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let Err(err) = service(pool)
        .change_role(
            admin_actor,
            target,
            UserRole::Admin,
            AuditContext::default(),
        )
        .await
    else {
        unreachable!("change_role unexpectedly succeeded");
    };
    assert!(matches!(
        err,
        IdentityError::InvalidTransition("pending", "admin")
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn approve_enqueues_the_user_approved_email(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let actor = provisioned_pending_user(&pool, "sub-admin").await;
    service(pool.clone())
        .approve(actor, target, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    let payloads: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM job_outbox WHERE job_type = 'send_email'")
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["kind"], "user_approved");
    assert_eq!(payloads[0]["recipient"], target.as_uuid().to_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn every_status_change_on_a_deleting_user_returns_user_deleting(pool: PgPool) {
    let actor = provisioned_pending_user(&pool, "sub-admin").await;
    let svc = service(pool.clone());
    for (n, op) in ["approve", "disable", "enable", "change_role"]
        .into_iter()
        .enumerate()
    {
        let target = provisioned_pending_user(&pool, &format!("sub-{n}")).await;
        set_deleting(&pool, target).await;
        let result = match op {
            "approve" => svc.approve(actor, target, AuditContext::default()).await,
            "disable" => svc.disable(actor, target, AuditContext::default()).await,
            "enable" => svc.enable(actor, target, AuditContext::default()).await,
            _ => {
                svc.change_role(actor, target, UserRole::Admin, AuditContext::default())
                    .await
            }
        };
        let Err(err) = result else {
            unreachable!("{op} on a deleting user unexpectedly succeeded");
        };
        assert!(matches!(err, IdentityError::UserDeleting), "{op}: {err:?}");
    }
}

async fn active_admin(pool: &PgPool, sub: &str) -> UserId {
    let id = provisioned_pending_user(pool, sub).await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    // `grant_admin` makes the user an active admin, as the bootstrap grant does.
    UsersRepo::grant_admin(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"));
    id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_mutual_disable_leaves_one_admin(pool: PgPool) {
    for round in 0..20 {
        let svc = service(pool.clone());
        let a = active_admin(&pool, &format!("a{round}")).await;
        let b = active_admin(&pool, &format!("b{round}")).await;
        // Earlier rounds' admins are disabled below, so each round starts with exactly two.
        let (ra, rb) = tokio::join!(
            svc.disable(a, b, AuditContext::default()),
            svc.disable(b, a, AuditContext::default()),
        );
        let outcomes = [ra.map(|_| ()), rb.map(|_| ())];
        let ok = outcomes.iter().filter(|r| r.is_ok()).count();
        let last = outcomes
            .iter()
            .filter(|r| matches!(r, Err(IdentityError::LastAdmin)))
            .count();
        assert_eq!((ok, last), (1, 1), "round {round}: {outcomes:?}");
        // Leave no active admin behind for the next round.
        let survivor = if outcomes[0].is_ok() { a } else { b };
        sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
            .bind(survivor.as_uuid())
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
}
