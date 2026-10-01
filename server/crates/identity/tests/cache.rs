use std::time::Duration;

use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus, UsersRepo};
use postit_identity::cache::{LISTENER_APPLICATION_NAME, PrincipalCache, run_one_listen_session};
use postit_identity::principal::Principal;
use sqlx::PgPool;

fn principal(user_id: UserId) -> Principal {
    Principal {
        user_id,
        role: UserRole::Member,
        status: UserStatus::Pending,
    }
}

#[test]
fn insert_then_get_round_trips() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let user_id = UserId::from(uuid::Uuid::now_v7());

    cache.insert("https://issuer.test", "sub-1", principal(user_id));

    let found = cache.get("https://issuer.test", "sub-1");
    assert!(matches!(found, Some(p) if p.user_id == user_id));
}

#[test]
fn get_misses_for_an_unknown_identity() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    assert!(cache.get("https://issuer.test", "no-such-sub").is_none());
}

#[test]
fn invalidate_user_evicts_only_that_user() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let a = UserId::from(uuid::Uuid::now_v7());
    let b = UserId::from(uuid::Uuid::now_v7());
    cache.insert("https://issuer.test", "sub-a", principal(a));
    cache.insert("https://issuer.test", "sub-b", principal(b));

    cache.invalidate_user(a);

    assert!(cache.get("https://issuer.test", "sub-a").is_none());
    assert!(cache.get("https://issuer.test", "sub-b").is_some());
}

#[test]
fn insert_if_current_drops_a_write_that_raced_an_invalidation() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    let before = cache.generation();
    cache.invalidate_user(user_id); // a concurrent admin change lands mid-miss
    assert!(!cache.insert_if_current(before, "https://issuer.test", "sub-1", principal(user_id)));
    assert!(cache.get("https://issuer.test", "sub-1").is_none());

    let now = cache.generation();
    assert!(cache.insert_if_current(now, "https://issuer.test", "sub-1", principal(user_id)));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_status_change_notification_evicts_that_user_in_a_second_process(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let bystander = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, bystander, "https://issuer.test", "sub-2", "Bob")
        .await
        .unwrap_or_else(|e| unreachable!("provision bystander: {e}"));

    let cache = PrincipalCache::new(Duration::from_secs(60));
    let before_listen = cache.generation();
    let listener_task = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));

    // Wait until the session is LISTENing: its backend is visible with our application_name,
    // and its post-listen invalidate_all() has advanced the generation. Only then is an
    // inserted entry safe from that initial clear, so the eviction below is the NOTIFY's.
    let listening = postit_jobs::testkit::wait_until(Duration::from_secs(10), || {
        cache.generation() > before_listen
    })
    .await;
    assert!(listening, "listener never reached LISTEN");
    // Scoped to this test's database: #[sqlx::test] runs tests in parallel, one database
    // each, on one cluster, so other tests' listeners share the application_name.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_stat_activity WHERE application_name = $1 AND datname = current_database()",
    )
    .bind(LISTENER_APPLICATION_NAME)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("pg_stat_activity: {e}"));
    assert_eq!(count, 1);

    cache.insert("https://issuer.test", "sub-1", principal(user_id));
    cache.insert("https://issuer.test", "sub-2", principal(bystander));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());

    UsersRepo::set_status(&mut conn, user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    let evicted = postit_jobs::testkit::wait_until(Duration::from_secs(5), || {
        cache.get("https://issuer.test", "sub-1").is_none()
    })
    .await;
    assert!(evicted, "NOTIFY did not evict the user");
    // Targeted, not a fallback invalidate_all (which a killed listener would trigger).
    assert!(
        cache.get("https://issuer.test", "sub-2").is_some(),
        "the eviction must be the NOTIFY for that user, not a full clear"
    );
    assert!(
        !listener_task.is_finished(),
        "the listen session must still be running"
    );
    listener_task.abort();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn connection_drop_falls_back_to_invalidate_all(pool: PgPool) {
    // Review Focus: the listener's own connection dying must clear every cached principal,
    // not just leave staleness to the TTL.
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_a = UserId::from(uuid::Uuid::now_v7());
    let user_b = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_a, "https://issuer.test", "sub-a", "A")
        .await
        .unwrap_or_else(|e| unreachable!("provision a: {e}"));
    UsersRepo::provision(&mut conn, user_b, "https://issuer.test", "sub-b", "B")
        .await
        .unwrap_or_else(|e| unreachable!("provision b: {e}"));

    let cache = PrincipalCache::new(Duration::from_secs(60));
    let before_listen = cache.generation();
    let session = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));
    let listening = postit_jobs::testkit::wait_until(Duration::from_secs(10), || {
        cache.generation() > before_listen
    })
    .await;
    assert!(listening, "listener never reached LISTEN");
    // Inserted after the session's initial clear, so only the drop path can remove them.
    cache.insert("https://issuer.test", "sub-a", principal(user_a));
    cache.insert("https://issuer.test", "sub-b", principal(user_b));

    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE application_name = $1 AND datname = current_database()",
    )
    .bind(LISTENER_APPLICATION_NAME)
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("terminate backend: {e}"));

    // PgListener transparently reconnects after a kill, so the session does not end; it
    // reports the gap (try_recv -> Ok(None)) and the session answers with invalidate_all().
    // Entries inserted after the initial clear can only vanish through that path.
    let cleared = postit_jobs::testkit::wait_until(Duration::from_secs(10), || {
        cache.get("https://issuer.test", "sub-a").is_none()
            && cache.get("https://issuer.test", "sub-b").is_none()
    })
    .await;
    assert!(cleared, "the dropped connection did not clear the cache");
    session.abort();

    assert!(cache.get("https://issuer.test", "sub-a").is_none());
    assert!(cache.get("https://issuer.test", "sub-b").is_none());
}
