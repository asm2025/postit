#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::seed_owned_rows;
use postit_core::UserId;
use postit_data::deletion::{OWNED_BATCH_TABLES, delete_owned_batch};
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

/// Every table with an `owner_id` column.
async fn owned_tables(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND column_name = 'owner_id' ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// How many rows `owner` has in each table that has an `owner_id`, zeros left out.
async fn counts(pool: &PgPool, owner: UserId) -> BTreeMap<String, i64> {
    let mut found = BTreeMap::new();
    for table in owned_tables(pool).await {
        let sql = format!("SELECT count(*) FROM {table} WHERE owner_id = $1");
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(owner.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
        if count > 0 {
            found.insert(table, count);
        }
    }
    found
}

async fn count_in(pool: &PgPool, table: &str, owner: UserId) -> i64 {
    let sql = format!("SELECT count(*) FROM {table} WHERE owner_id = $1");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(owner.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn deleting_owner_cascades_everything_and_spares_others(pool: PgPool) {
    let (a, b, delegate) = (
        seed_user(&pool, "active").await,
        seed_user(&pool, "active").await,
        seed_user(&pool, "active").await,
    );
    seed_owned_rows(&pool, a, delegate).await;
    seed_owned_rows(&pool, b, delegate).await;
    let before_a = counts(&pool, a).await;
    let before_b = counts(&pool, b).await;
    for table in OWNED_BATCH_TABLES {
        assert!(before_a.contains_key(*table), "{table} was not seeded");
    }

    // Empty the big tables a few rows at a time, then delete the user.
    let mut conn = pool.acquire().await.unwrap();
    for table in OWNED_BATCH_TABLES {
        while delete_owned_batch(&mut conn, a, table, 1).await.unwrap() == 1 {}
    }
    drop(conn);
    for table in OWNED_BATCH_TABLES {
        assert_eq!(count_in(&pool, table, a).await, 0, "{table} not emptied");
    }
    assert_eq!(
        counts(&pool, b).await,
        before_b,
        "B untouched by A's batches"
    );

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(a.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert!(counts(&pool, a).await.is_empty(), "rows left for A");
    assert_eq!(
        counts(&pool, b).await,
        before_b,
        "B untouched by A's deletion"
    );

    // Deleting the editor keeps B's content and clears the actor columns.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(delegate.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(counts(&pool, b).await, before_b);
    for table in [
        "posts",
        "publish_jobs",
        "deliveries",
        "contacts",
        "suppressions",
    ] {
        let sql =
            format!("SELECT count(*) FROM {table} WHERE owner_id = $1 AND updated_by IS NOT NULL");
        let left: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(b.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "{table}.updated_by not cleared");
    }
}

#[sqlx::test]
async fn delete_owned_batch_respects_limit(pool: PgPool) {
    let (owner, other) = (
        seed_user(&pool, "active").await,
        seed_user(&pool, "active").await,
    );
    seed_owned_rows(&pool, owner, owner).await;
    seed_owned_rows(&pool, other, other).await;
    // Three more contacts for the owner.
    for n in 0..3 {
        sqlx::query(
            "INSERT INTO contacts (id, owner_id, email, email_hash) VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(owner.as_uuid())
        .bind(format!("extra{n}@example.com"))
        .bind(vec![9u8; 32])
        .execute(&pool)
        .await
        .unwrap();
    }
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        delete_owned_batch(&mut conn, owner, "contacts", 3)
            .await
            .unwrap(),
        3
    );
    assert_eq!(count_in(&pool, "contacts", owner).await, 1);
    assert_eq!(
        delete_owned_batch(&mut conn, owner, "contacts", 3)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        count_in(&pool, "contacts", other).await,
        1,
        "other owner kept"
    );
}

#[sqlx::test]
#[should_panic(expected = "not a batch-deletable table")]
async fn delete_owned_batch_refuses_other_tables(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let _ = delete_owned_batch(&mut conn, owner, "users", 10).await;
}
