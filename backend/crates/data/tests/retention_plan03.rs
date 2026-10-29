//! The plan 03 retention purges: one old and one fresh row per table.
#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::seed_owned_rows;
use postit_data::retention;
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

async fn count(pool: &PgPool, table: &str, owner: Uuid) -> i64 {
    let column = if table == "notifications" {
        "user_id"
    } else {
        "owner_id"
    };
    let sql = format!("SELECT count(*) FROM {table} WHERE {column} = $1");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn run(pool: &PgPool, purge: &str, cutoff: DateTime<Utc>) -> u64 {
    let mut conn = pool.acquire().await.unwrap();
    match purge {
        "notifications" => retention::purge_read_notifications(&mut conn, cutoff).await,
        "delivery_attempts" => retention::purge_delivery_attempts(&mut conn, cutoff).await,
        "ai_generations" => retention::purge_ai_generations(&mut conn, cutoff).await,
        "inbox_items" => retention::purge_inbox_items(&mut conn, cutoff).await,
        "direct_sends" => retention::purge_direct_sends(&mut conn, cutoff).await,
        "email_events" => retention::purge_email_events(&mut conn, cutoff).await,
        "contact_imports" => retention::purge_contact_imports(&mut conn, cutoff).await,
        other => unreachable!("{other}"),
    }
    .unwrap()
}

#[sqlx::test]
async fn each_retention_purge_deletes_only_expired(pool: PgPool) {
    let (old, fresh) = (
        seed_user(&pool, "active").await,
        seed_user(&pool, "active").await,
    );
    seed_owned_rows(&pool, old, old).await;
    seed_owned_rows(&pool, fresh, fresh).await;
    let cases = [
        ("notifications", "read_at"),
        ("delivery_attempts", "started_at"),
        ("ai_generations", "created_at"),
        ("inbox_items", "created_at"),
        // Events first: deleting a direct send takes its events with it.
        ("email_events", "at"),
        ("direct_sends", "created_at"),
        ("contact_imports", "created_at"),
    ];
    let cutoff = Utc::now() - Duration::days(5);
    for (table, column) in cases {
        let owner_column = if table == "notifications" {
            "user_id"
        } else {
            "owner_id"
        };
        let sql = format!(
            "UPDATE {table} SET {column} = now() - interval '10 days' WHERE {owner_column} = $1"
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(old.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(run(&pool, table, cutoff).await, 1, "{table}");
        assert_eq!(
            count(&pool, table, old.as_uuid()).await,
            0,
            "{table} old row kept"
        );
        assert_eq!(
            count(&pool, table, fresh.as_uuid()).await,
            1,
            "{table} fresh row lost"
        );
    }
}

#[sqlx::test]
async fn unread_notifications_kept(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, created_at) \
         VALUES ($1, $2, 'user_pending', now() - interval '400 days')",
    )
    .bind(Uuid::now_v7())
    .bind(user.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(run(&pool, "notifications", Utc::now()).await, 0);
    assert_eq!(count(&pool, "notifications", user.as_uuid()).await, 1);
}

#[sqlx::test]
async fn queued_direct_sends_and_running_imports_kept(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    seed_owned_rows(&pool, owner, owner).await;
    sqlx::query("UPDATE direct_sends SET state = 'queued', created_at = now() - interval '9 days'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE contact_imports SET status = 'processing', created_at = now() - interval '9 days'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let cutoff = Utc::now() - Duration::days(1);
    assert_eq!(run(&pool, "direct_sends", cutoff).await, 0);
    assert_eq!(run(&pool, "contact_imports", cutoff).await, 0);
}

#[sqlx::test]
async fn ai_prompt_cleared_before_row_deleted(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    seed_owned_rows(&pool, owner, owner).await;
    sqlx::query("UPDATE ai_generations SET created_at = now() - interval '10 days'")
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let cleared = retention::clear_ai_prompts(&mut conn, Utc::now() - Duration::days(5))
        .await
        .unwrap();
    assert_eq!(cleared, 1);
    drop(conn);
    let prompt: Option<String> = sqlx::query_scalar("SELECT prompt FROM ai_generations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(prompt, None);
    assert_eq!(
        count(&pool, "ai_generations", owner.as_uuid()).await,
        1,
        "row kept"
    );

    // The row goes only once its own, longer window passes.
    assert_eq!(
        run(&pool, "ai_generations", Utc::now() - Duration::days(20)).await,
        0
    );
    assert_eq!(
        run(&pool, "ai_generations", Utc::now() - Duration::days(5)).await,
        1
    );
}

#[sqlx::test]
async fn expired_connections_purged(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    common::account(&pool, user, "a").await; // creates the mastodon platform row
    for (offset, marker) in [("-1 hour", 0u8), ("+1 hour", 1u8)] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO oauth_states (id, state_hash, user_id, platform_id, expires_at) \
             VALUES ($1, $2, $3, 'mastodon', now() + interval '{offset}')"
        )))
        .bind(Uuid::now_v7())
        .bind(vec![marker; 32])
        .bind(user.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO pending_connections \
             (id, owner_id, connect_id_hash, platform_id, credential, expires_at) \
             VALUES ($1, $2, $3, 'mastodon', $4, now() + interval '{offset}')"
        )))
        .bind(Uuid::now_v7())
        .bind(user.as_uuid())
        .bind(vec![marker; 32])
        .bind(vec![0u8; 4])
        .execute(&pool)
        .await
        .unwrap();
    }
    let mut conn = pool.acquire().await.unwrap();
    let purged = retention::purge_expired_connections(&mut conn, Utc::now())
        .await
        .unwrap();
    assert_eq!(purged, (1, 1));
    drop(conn);
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM oauth_states) + (SELECT count(*) FROM pending_connections)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, 2);
}

#[sqlx::test]
async fn expired_oauth_state_holding_tokens_is_left_for_the_revoking_purge(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    common::account(&pool, user, "a").await; // creates the mastodon platform row
    sqlx::query(
        "INSERT INTO oauth_states (id, state_hash, user_id, platform_id, expires_at, grant_tokens) \
         VALUES ($1, $2, $3, 'mastodon', now() - interval '1 hour', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(vec![9u8; 32])
    .bind(user.as_uuid())
    .bind(vec![0u8; 4])
    .execute(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let purged = retention::purge_expired_connections(&mut conn, Utc::now())
        .await
        .unwrap();
    assert_eq!(purged, (0, 0));
}

#[sqlx::test]
async fn purge_loops_past_one_batch(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, read_at) \
         SELECT uuidv7(), $1, 'user_pending', now() - interval '10 days' FROM generate_series(1, 5001)",
    )
    .bind(user.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(run(&pool, "notifications", Utc::now()).await, 5001);
}

#[sqlx::test]
async fn connections_expiring_exactly_now_are_purged(pool: PgPool) {
    // One boundary for "expired": `expires_at <= now`, as the OAuth purge job and the
    // live-result checks (`expires_at > now`) use.
    let user = seed_user(&pool, "active").await;
    common::account(&pool, user, "a").await; // creates the mastodon platform row
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT date_trunc('second', now())")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO oauth_states (id, state_hash, user_id, platform_id, expires_at) \
         VALUES ($1, $2, $3, 'mastodon', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(vec![7u8; 32])
    .bind(user.as_uuid())
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pending_connections \
         (id, owner_id, connect_id_hash, platform_id, credential, expires_at) \
         VALUES ($1, $2, $3, 'mastodon', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user.as_uuid())
    .bind(vec![7u8; 32])
    .bind(vec![0u8; 4])
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let purged = retention::purge_expired_connections(&mut conn, now)
        .await
        .unwrap();
    assert_eq!(purged, (1, 1));
}
