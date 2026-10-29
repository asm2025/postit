#![allow(clippy::unwrap_used)]

use postit_core::UserId;
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provisioned_user(conn: &mut sqlx::PgConnection) -> UserId {
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(
        conn,
        id,
        "https://issuer.test",
        "sub-1",
        "Ada",
        postit_data::limits::UserLimitDefaults::default(),
    )
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

    let prefs =
        UserPreferencesRepo::create_default(&mut conn, user_id, "America/New_York", "en-US")
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
    UserPreferencesRepo::create_default(&mut conn, user_id, "UTC", "en-US")
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

#[sqlx::test]
async fn set_last_approval_email_at_creates_a_missing_preferences_row(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    let at = chrono::DateTime::from_timestamp(1_800_000_000, 0)
        .unwrap_or_else(|| unreachable!("timestamp"));

    UserPreferencesRepo::set_last_approval_email_at(&mut conn, user_id, at)
        .await
        .unwrap_or_else(|e| unreachable!("set_last_approval_email_at: {e}"));

    let Some(prefs) = UserPreferencesRepo::get(&mut conn, user_id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"))
    else {
        unreachable!("no preferences row after the upsert");
    };
    assert_eq!(prefs.last_approval_email_at, Some(at));
    assert_eq!(prefs.timezone, "UTC");
    assert!(prefs.email_notifications);
    assert!(!prefs.store_ai_prompts);
}

#[sqlx::test]
async fn set_last_approval_email_at_updates_an_existing_row_and_keeps_other_fields(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    UserPreferencesRepo::create_default(&mut conn, user_id, "Europe/London", "en-US")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));
    let first = chrono::DateTime::from_timestamp(1_800_000_000, 0)
        .unwrap_or_else(|| unreachable!("timestamp"));
    let second = chrono::DateTime::from_timestamp(1_800_003_600, 0)
        .unwrap_or_else(|| unreachable!("timestamp"));

    for at in [first, second] {
        UserPreferencesRepo::set_last_approval_email_at(&mut conn, user_id, at)
            .await
            .unwrap_or_else(|e| unreachable!("set_last_approval_email_at: {e}"));
    }

    let Some(prefs) = UserPreferencesRepo::get(&mut conn, user_id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"))
    else {
        unreachable!("preferences row vanished");
    };
    assert_eq!(prefs.last_approval_email_at, Some(second));
    assert_eq!(prefs.timezone, "Europe/London");
}

#[sqlx::test]
async fn create_default_stores_locale(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    UserPreferencesRepo::create_default(&mut conn, user_id, "UTC", "ar")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    let Some(prefs) = UserPreferencesRepo::get(&mut conn, user_id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"))
    else {
        unreachable!("no preferences row");
    };
    assert_eq!(prefs.locale, "ar");
}

#[sqlx::test]
async fn set_locale_updates_only_locale(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    UserPreferencesRepo::create_default(&mut conn, user_id, "Europe/London", "en-US")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    let prefs = UserPreferencesRepo::set_locale(&mut conn, user_id, "ar")
        .await
        .unwrap_or_else(|e| unreachable!("set_locale: {e}"));

    assert_eq!(prefs.locale, "ar");
    assert_eq!(prefs.timezone, "Europe/London");
    assert!(prefs.email_notifications);
    assert!(!prefs.store_ai_prompts);
}

#[sqlx::test]
async fn set_locale_missing_user(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;

    let result = UserPreferencesRepo::set_locale(&mut conn, user_id, "ar").await;

    assert!(matches!(result, Err(postit_data::DataError::NotFound)));
}

mod switches {
    use postit_core::NotificationKind;
    use postit_data::preferences::{
        ChannelSwitch, NotificationSwitches, PreferencesUpdate, UserPreferencesRepo,
    };
    use postit_data::testkit::seed_user;
    use sqlx::PgPool;

    fn one(
        kind: NotificationKind,
        push: Option<bool>,
        email: Option<bool>,
    ) -> NotificationSwitches {
        NotificationSwitches([(kind, ChannelSwitch { push, email })].into())
    }

    #[test]
    fn switches_fall_back_to_kind_defaults() {
        let none = NotificationSwitches::default();
        for kind in NotificationKind::ALL {
            assert_eq!(none.push(kind), kind.default_push(), "{kind:?}");
            assert_eq!(none.email(kind), kind.default_email(), "{kind:?}");
        }
        let set = one(NotificationKind::DeliveryFailed, Some(false), Some(false));
        assert!(!set.push(NotificationKind::DeliveryFailed));
        assert!(!set.email(NotificationKind::DeliveryFailed));
        assert!(set.email(NotificationKind::DeliveryStale));
        let comments = one(NotificationKind::NewComments, Some(false), None);
        assert!(!comments.push(NotificationKind::NewComments));
    }

    #[test]
    fn email_switch_never_on_for_info_kinds() {
        let set = NotificationSwitches(
            NotificationKind::ALL
                .into_iter()
                .map(|k| {
                    (
                        k,
                        ChannelSwitch {
                            push: None,
                            email: Some(true),
                        },
                    )
                })
                .collect(),
        );
        for kind in NotificationKind::ALL {
            assert_eq!(set.email(kind), kind.email_allowed(), "{kind:?}");
        }
    }

    #[sqlx::test]
    async fn upsert_creates_missing_row_and_applies_only_given_fields(pool: PgPool) {
        let user = seed_user(&pool, "active").await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            UserPreferencesRepo::get(&mut conn, user)
                .await
                .unwrap()
                .is_none()
        );

        let created = UserPreferencesRepo::upsert(
            &mut conn,
            user,
            &PreferencesUpdate {
                timezone: Some("Asia/Riyadh".to_owned()),
                ..PreferencesUpdate::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(created.timezone, "Asia/Riyadh");
        assert_eq!(created.locale, "en-US");
        assert!(created.email_notifications);
        assert!(!created.comment_digest);
        assert!(created.notification_switches.0.is_empty());

        let changed = UserPreferencesRepo::upsert(
            &mut conn,
            user,
            &PreferencesUpdate {
                comment_digest: Some(true),
                email_notifications: Some(false),
                ..PreferencesUpdate::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(changed.timezone, "Asia/Riyadh");
        assert!(changed.comment_digest);
        assert!(!changed.email_notifications);
    }

    #[sqlx::test]
    async fn upsert_merges_switches_per_kind(pool: PgPool) {
        let user = seed_user(&pool, "active").await;
        let mut conn = pool.acquire().await.unwrap();
        let put = |s| PreferencesUpdate {
            switches: Some(s),
            ..PreferencesUpdate::default()
        };
        UserPreferencesRepo::upsert(
            &mut conn,
            user,
            &put(one(NotificationKind::DeliveryFailed, Some(false), None)),
        )
        .await
        .unwrap();

        let merged = UserPreferencesRepo::upsert(
            &mut conn,
            user,
            &put(one(NotificationKind::NewComments, None, Some(true))),
        )
        .await
        .unwrap();
        assert_eq!(merged.notification_switches.0.len(), 2);
        assert!(
            !merged
                .notification_switches
                .push(NotificationKind::DeliveryFailed)
        );

        let reset = UserPreferencesRepo::upsert(
            &mut conn,
            user,
            &put(one(NotificationKind::DeliveryFailed, None, None)),
        )
        .await
        .unwrap();
        assert_eq!(reset.notification_switches.0.len(), 1);
        assert!(
            reset
                .notification_switches
                .push(NotificationKind::DeliveryFailed)
        );
        let stored = UserPreferencesRepo::get(&mut conn, user)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.notification_switches, reset.notification_switches);
    }

    #[sqlx::test]
    async fn set_last_comment_digest_at_creates_and_updates(pool: PgPool) {
        let user = seed_user(&pool, "active").await;
        let mut conn = pool.acquire().await.unwrap();
        let at =
            chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros()).unwrap();

        UserPreferencesRepo::set_last_comment_digest_at(&mut conn, user, at)
            .await
            .unwrap();

        let prefs = UserPreferencesRepo::get(&mut conn, user)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(prefs.last_comment_digest_at, Some(at));
    }
}
