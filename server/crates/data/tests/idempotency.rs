use chrono::{Duration, Utc};
use postit_core::UserId;
use postit_data::idempotency::{BeginOutcome, IdempotencyRepo};
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provisioned_user(conn: &mut sqlx::PgConnection, sub: &str) -> UserId {
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(conn, id, "https://issuer.test", sub, "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

#[sqlx::test]
async fn begin_starts_a_new_key(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);

    let outcome = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin: {e}"));

    assert!(matches!(outcome, BeginOutcome::Started(_)));
}

#[sqlx::test]
async fn begin_twice_with_the_same_key_returns_conflict_not_an_error(pool: PgPool) {
    // Review Focus: a racing begin() must be a typed Conflict, not an unhandled SQL error.
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);

    let first = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("first begin: {e}"));
    let second = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-2",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("second begin: {e}"));

    let BeginOutcome::Started(started) = first else {
        unreachable!("first begin should have started");
    };
    let BeginOutcome::Conflict(conflict) = second else {
        unreachable!("second begin should have conflicted");
    };
    assert_eq!(started.id, conflict.id);
    assert_eq!(conflict.request_hash, "hash-1");
}

#[sqlx::test]
async fn begin_racing_for_the_same_key_yields_exactly_one_started_and_one_conflict(pool: PgPool) {
    // Review Focus: two concurrent begin() calls for the same (owner_id, actor_id, key) must
    // resolve to exactly one Started and one Conflict — neither call may error, and the
    // conflict must be a typed BeginOutcome, not a raw Postgres unique-violation.
    let mut setup_conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut setup_conn, "sub-1").await;
    drop(setup_conn);
    let expires_at = Utc::now() + Duration::hours(24);

    let pool_a = pool.clone();
    let id_a = uuid::Uuid::now_v7();
    let task_a = tokio::spawn(async move {
        let mut conn = pool_a
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        IdempotencyRepo::begin(
            &mut conn,
            id_a,
            user,
            user,
            "key-race",
            "POST /posts",
            "hash-a",
            expires_at,
        )
        .await
    });

    let pool_b = pool.clone();
    let id_b = uuid::Uuid::now_v7();
    let task_b = tokio::spawn(async move {
        let mut conn = pool_b
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        IdempotencyRepo::begin(
            &mut conn,
            id_b,
            user,
            user,
            "key-race",
            "POST /posts",
            "hash-b",
            expires_at,
        )
        .await
    });

    let result_a = task_a
        .await
        .unwrap_or_else(|e| unreachable!("task a panicked: {e}"))
        .unwrap_or_else(|e| unreachable!("task a begin: {e}"));
    let result_b = task_b
        .await
        .unwrap_or_else(|e| unreachable!("task b panicked: {e}"))
        .unwrap_or_else(|e| unreachable!("task b begin: {e}"));

    let started_count = [&result_a, &result_b]
        .iter()
        .filter(|o| matches!(o, BeginOutcome::Started(_)))
        .count();
    let conflict_count = [&result_a, &result_b]
        .iter()
        .filter(|o| matches!(o, BeginOutcome::Conflict(_)))
        .count();
    assert_eq!(started_count, 1, "exactly one racer should start the key");
    assert_eq!(
        conflict_count, 1,
        "exactly one racer should observe a conflict"
    );

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM idempotency_keys WHERE owner_id = $1 AND actor_id = $2 AND key = $3",
    )
    .bind(user.as_uuid())
    .bind(user.as_uuid())
    .bind("key-race")
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn complete_sets_state_and_response(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);
    let id = uuid::Uuid::now_v7();
    IdempotencyRepo::begin(
        &mut conn,
        id,
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin: {e}"));

    IdempotencyRepo::complete(&mut conn, id, 201, serde_json::json!({"id": "abc"}))
        .await
        .unwrap_or_else(|e| unreachable!("complete: {e}"));

    let record = IdempotencyRepo::find(&mut conn, user, user, "key-1")
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("record should exist"));
    assert_eq!(record.response_status, Some(201));
    assert_eq!(
        record
            .response_body
            .as_ref()
            .and_then(|b| b.get("id"))
            .and_then(|v| v.as_str()),
        Some("abc")
    );
}

#[sqlx::test]
async fn delete_removes_an_in_progress_row(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);
    let id = uuid::Uuid::now_v7();
    IdempotencyRepo::begin(
        &mut conn,
        id,
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin: {e}"));

    IdempotencyRepo::delete(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));

    let record = IdempotencyRepo::find(&mut conn, user, user, "key-1")
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"));
    assert!(record.is_none());
}
