use postit_data::app_settings::AppSettingsRepo;
use postit_data::testkit::seed_user;
use sqlx::PgPool;

#[sqlx::test]
async fn seed_does_not_overwrite(pool: PgPool) {
    let admin = seed_user(&pool, "active").await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));

    AppSettingsRepo::seed_defaults(&mut conn, true)
        .await
        .unwrap_or_else(|e| unreachable!("seed: {e}"));
    assert!(
        AppSettingsRepo::live_publishing(&mut conn)
            .await
            .unwrap_or_else(|e| unreachable!("read: {e}"))
    );

    AppSettingsRepo::set_live_publishing(&mut conn, false, admin)
        .await
        .unwrap_or_else(|e| unreachable!("set: {e}"));
    AppSettingsRepo::seed_defaults(&mut conn, true)
        .await
        .unwrap_or_else(|e| unreachable!("seed again: {e}"));
    assert!(
        !AppSettingsRepo::live_publishing(&mut conn)
            .await
            .unwrap_or_else(|e| unreachable!("read: {e}"))
    );
}

#[sqlx::test]
async fn unseeded_setting_is_not_found(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let result = AppSettingsRepo::live_publishing(&mut conn).await;
    assert!(matches!(result, Err(postit_data::DataError::NotFound)));
}

#[sqlx::test]
async fn set_records_the_admin(pool: PgPool) {
    let admin = seed_user(&pool, "active").await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    AppSettingsRepo::set_live_publishing(&mut conn, true, admin)
        .await
        .unwrap_or_else(|e| unreachable!("set: {e}"));

    let by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT updated_by FROM app_settings WHERE key = 'publishing.live'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("read: {e}"));
    assert_eq!(by, Some(admin.as_uuid()));
}
