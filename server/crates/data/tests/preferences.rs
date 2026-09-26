use postit_core::UserId;
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provisioned_user(conn: &mut sqlx::PgConnection) -> UserId {
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(conn, id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

#[sqlx::test]
async fn create_default_sets_timezone_and_defaults(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;

    let prefs = UserPreferencesRepo::create_default(&mut conn, user_id, "America/New_York")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    assert_eq!(prefs.timezone, "America/New_York");
    assert!(prefs.email_notifications);
    assert!(!prefs.store_ai_prompts);
    assert!(prefs.last_approval_email_at.is_none());
}

#[sqlx::test]
async fn update_changes_fields(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    UserPreferencesRepo::create_default(&mut conn, user_id, "UTC")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    let prefs = UserPreferencesRepo::update(&mut conn, user_id, false, "Europe/London", true)
        .await
        .unwrap_or_else(|e| unreachable!("update: {e}"));

    assert!(!prefs.email_notifications);
    assert_eq!(prefs.timezone, "Europe/London");
    assert!(prefs.store_ai_prompts);
}

#[sqlx::test]
async fn get_returns_none_for_a_user_with_no_preferences_row(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;

    let prefs = UserPreferencesRepo::get(&mut conn, user_id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));

    assert!(prefs.is_none());
}
