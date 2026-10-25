use postit_core::UserId;
use postit_data::limits::{UserLimitDefaults, UserLimits, UserLimitsRepo};
use postit_data::testkit::seed_user;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provision(pool: &PgPool, subject: &str, limits: UserLimitDefaults) -> UserId {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let (_, user) = UsersRepo::provision(
        &mut conn,
        UserId::from(uuid::Uuid::now_v7()),
        "https://issuer.test",
        subject,
        "Ada",
        limits,
    )
    .await
    .unwrap_or_else(|e| unreachable!("provision: {e}"));
    user.id
}

#[sqlx::test]
async fn provision_creates_limits_from_defaults(pool: PgPool) {
    let defaults = UserLimitDefaults {
        email_daily_cap: 500,
        contacts_cap: 10_000,
        storage_quota_bytes: 21_474_836_480,
    };
    let user = provision(&pool, "sub-1", defaults).await;

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let limits = UserLimitsRepo::get(&mut conn, user)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(limits.email_daily_cap, defaults.email_daily_cap);
    assert_eq!(limits.contacts_cap, defaults.contacts_cap);
    assert_eq!(limits.storage_quota_bytes, defaults.storage_quota_bytes);
    assert_eq!(limits.updated_by, None);
}

#[sqlx::test]
async fn provision_twice_keeps_first_limits(pool: PgPool) {
    let first = UserLimitDefaults {
        email_daily_cap: 1,
        contacts_cap: 2,
        storage_quota_bytes: 3,
    };
    let user = provision(&pool, "sub-1", first).await;
    let again = provision(&pool, "sub-1", UserLimitDefaults::default()).await;
    assert_eq!(user, again);

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let limits = UserLimitsRepo::get(&mut conn, user)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(
        (
            limits.email_daily_cap,
            limits.contacts_cap,
            limits.storage_quota_bytes
        ),
        (1, 2, 3)
    );
}

#[sqlx::test]
async fn limits_update_records_admin(pool: PgPool) {
    let user = provision(&pool, "sub-1", UserLimitDefaults::default()).await;
    let admin = seed_user(&pool, "active").await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));

    let current = UserLimitsRepo::get(&mut conn, user)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    let changed = UserLimits {
        email_daily_cap: 0,
        contacts_cap: 50,
        storage_quota_bytes: 1_000,
        ..current
    };
    UserLimitsRepo::update(&mut conn, user, changed, admin)
        .await
        .unwrap_or_else(|e| unreachable!("update: {e}"));

    let stored = UserLimitsRepo::get(&mut conn, user)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(stored.email_daily_cap, 0);
    assert_eq!(stored.contacts_cap, 50);
    assert_eq!(stored.storage_quota_bytes, 1_000);
    assert_eq!(stored.updated_by, Some(admin));
    assert!(stored.updated_at >= current.updated_at);
}

#[sqlx::test]
async fn update_of_a_user_without_limits_is_not_found(pool: PgPool) {
    let admin = seed_user(&pool, "active").await;
    // Seeded directly, so no limits row exists.
    let user = seed_user(&pool, "active").await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));

    let missing = UserLimitsRepo::get(&mut conn, user).await;
    assert!(matches!(missing, Err(postit_data::DataError::NotFound)));

    let limits = UserLimits {
        email_daily_cap: 1,
        contacts_cap: 1,
        storage_quota_bytes: 1,
        updated_at: chrono::Utc::now(),
        updated_by: None,
    };
    let result = UserLimitsRepo::update(&mut conn, user, limits, admin).await;
    assert!(matches!(result, Err(postit_data::DataError::NotFound)));
}
