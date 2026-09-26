use postit_core::UserId;
use postit_data::users::{ProvisionOutcome, UserRole, UserStatus, UsersRepo};
use sqlx::PgPool;

#[sqlx::test]
async fn provision_creates_a_pending_member(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());

    let (outcome, user) =
        UsersRepo::provision(&mut conn, id, "https://issuer.test", "sub-1", "Ada")
            .await
            .unwrap_or_else(|e| unreachable!("provision: {e}"));

    assert!(matches!(outcome, ProvisionOutcome::Created));
    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
    assert_eq!(user.display_name, "Ada");
}

#[sqlx::test]
async fn provisioning_the_same_identity_twice_returns_existing(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id_a = UserId::from(uuid::Uuid::now_v7());
    let id_b = UserId::from(uuid::Uuid::now_v7());

    let (first_outcome, first) =
        UsersRepo::provision(&mut conn, id_a, "https://issuer.test", "sub-1", "Ada")
            .await
            .unwrap_or_else(|e| unreachable!("first provision: {e}"));
    let (second_outcome, second) =
        UsersRepo::provision(&mut conn, id_b, "https://issuer.test", "sub-1", "Ignored")
            .await
            .unwrap_or_else(|e| unreachable!("second provision: {e}"));

    assert!(matches!(first_outcome, ProvisionOutcome::Created));
    assert!(matches!(second_outcome, ProvisionOutcome::Existing));
    assert_eq!(first.id, second.id);
    assert_eq!(second.display_name, "Ada");
}

#[sqlx::test]
async fn concurrent_first_sign_in_for_the_same_identity_creates_exactly_one_user(pool: PgPool) {
    // Review Focus: two concurrent provisions for a brand-new (iss, sub) must create
    // exactly one row, not two, and neither call may error.
    let iss = "https://issuer.test";
    let sub = "concurrent-sub";

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let task_a = tokio::spawn(async move {
        let mut conn = pool_a
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::provision(&mut conn, UserId::from(uuid::Uuid::now_v7()), iss, sub, "A").await
    });
    let task_b = tokio::spawn(async move {
        let mut conn = pool_b
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::provision(&mut conn, UserId::from(uuid::Uuid::now_v7()), iss, sub, "B").await
    });

    let result_a = task_a
        .await
        .unwrap_or_else(|e| unreachable!("task a panicked: {e}"));
    let result_b = task_b
        .await
        .unwrap_or_else(|e| unreachable!("task b panicked: {e}"));
    assert!(result_a.is_ok());
    assert!(result_b.is_ok());

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2",
    )
    .bind(iss)
    .bind(sub)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn grant_admin_promotes_to_active_admin(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let user = UsersRepo::grant_admin(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    assert_eq!(user.role, UserRole::Admin);
    assert_eq!(user.status, UserStatus::Active);
    assert!(user.approved_at.is_some());
}

#[sqlx::test]
async fn count_active_admins_counts_only_active_admins(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin = UserId::from(uuid::Uuid::now_v7());
    let pending = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut conn,
        admin,
        "https://issuer.test",
        "admin-sub",
        "Admin",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision admin: {e}"));
    UsersRepo::grant_admin(&mut conn, admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));
    UsersRepo::provision(
        &mut conn,
        pending,
        "https://issuer.test",
        "pending-sub",
        "Pending",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision pending: {e}"));

    let count = UsersRepo::count_active_admins(&mut conn)
        .await
        .unwrap_or_else(|e| unreachable!("count_active_admins: {e}"));

    assert_eq!(count, 1);
}

#[sqlx::test]
async fn set_status_to_active_from_pending_sets_approved_at_and_approved_by(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin = UserId::from(uuid::Uuid::now_v7());
    let member = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut conn,
        admin,
        "https://issuer.test",
        "admin-sub",
        "Admin",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision admin: {e}"));
    UsersRepo::provision(
        &mut conn,
        member,
        "https://issuer.test",
        "member-sub",
        "Member",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision member: {e}"));

    let user = UsersRepo::set_status(&mut conn, member, UserStatus::Active, Some(admin))
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    assert_eq!(user.status, UserStatus::Active);
    assert!(user.approved_at.is_some());
    assert_eq!(user.approved_by, Some(admin));
}

#[sqlx::test]
async fn set_status_notifies_postit_user_changed(pool: PgPool) {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("listener connect: {e}"));
    listener
        .listen("postit_user_changed")
        .await
        .unwrap_or_else(|e| unreachable!("listen: {e}"));

    let mut writer_conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire writer: {e}"));
    let member = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        &mut writer_conn,
        member,
        "https://issuer.test",
        "member-sub",
        "Member",
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::set_status(&mut writer_conn, member, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
        .await
        .unwrap_or_else(|_| unreachable!("timed out waiting for notification"))
        .unwrap_or_else(|e| unreachable!("recv: {e}"));

    assert_eq!(notification.payload(), member.as_uuid().to_string());
}
