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

    let cache = PrincipalCache::new(Duration::from_secs(60));
    cache.insert("https://issuer.test", "sub-1", principal(user_id));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());

    let listener_task = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));
    // Give the spawned task time to connect and LISTEN before the notification fires.
    tokio::time::sleep(Duration::from_millis(500)).await;

    UsersRepo::set_status(&mut conn, user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    // Give the notification time to be delivered and processed.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(cache.get("https://issuer.test", "sub-1").is_none());

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
    cache.insert("https://issuer.test", "sub-a", principal(user_a));
    cache.insert("https://issuer.test", "sub-b", principal(user_b));

    let session = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));
    // Give the session time to connect, set its application_name, and start listening.
    tokio::time::sleep(Duration::from_millis(200)).await;

    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
    )
    .bind(LISTENER_APPLICATION_NAME)
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("terminate backend: {e}"));

    // run_one_listen_session ends on its own once its connection is killed — no abort()
    // needed, and awaiting the handle proves it actually reached the invalidate_all() path
    // rather than the test just winning a race against a still-running task.
    session
        .await
        .unwrap_or_else(|e| unreachable!("listener task panicked: {e}"));

    assert!(cache.get("https://issuer.test", "sub-a").is_none());
    assert!(cache.get("https://issuer.test", "sub-b").is_none());
}
