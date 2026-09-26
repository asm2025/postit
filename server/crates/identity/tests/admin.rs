use std::sync::Arc;

use postit_core::{SystemIdGenerator, UserId};
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
    UserAdminService::new(pool, Arc::new(SystemIdGenerator))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn approve_moves_pending_to_active(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let user = service(pool)
        .approve(admin_actor, target)
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
    svc.approve(admin_actor, target)
        .await
        .unwrap_or_else(|e| unreachable!("first approve: {e}"));

    let Err(err) = svc.approve(admin_actor, target).await else {
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
    svc.approve(admin_actor, target)
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    let disabled = svc
        .disable(admin_actor, target)
        .await
        .unwrap_or_else(|e| unreachable!("disable: {e}"));
    assert_eq!(disabled.status, UserStatus::Disabled);

    let enabled = svc
        .enable(admin_actor, target)
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

    let Err(err) = service(pool).disable(admin_id, admin_id).await else {
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
        .change_role(admin_id, admin_id, UserRole::Member)
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
        .change_role(first_admin, second_admin, UserRole::Member)
        .await
        .unwrap_or_else(|e| unreachable!("change_role: {e}"));

    assert_eq!(updated.role, UserRole::Member);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn changing_role_of_a_pending_user_is_rejected(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let Err(err) = service(pool)
        .change_role(admin_actor, target, UserRole::Admin)
        .await
    else {
        unreachable!("change_role unexpectedly succeeded");
    };
    assert!(matches!(
        err,
        IdentityError::InvalidTransition("pending", "admin")
    ));
}
