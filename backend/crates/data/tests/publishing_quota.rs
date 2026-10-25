#![allow(clippy::unwrap_used)]

mod common;

use chrono::NaiveDate;
use common::account;
use postit_core::{AccountId, PlatformId, UserId};
use postit_data::publishing::quota::{
    AccountQuotaKey, AppQuotaKey, EMAIL_CAP_BUCKET, PlatformAppQuotaRepo, QuotaRepo,
};
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use tokio::task::JoinSet;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

fn mastodon() -> PlatformId {
    PlatformId::new("mastodon").unwrap()
}

async fn key(pool: &PgPool, period: NaiveDate) -> (UserId, AccountId, AccountQuotaKey) {
    let owner = seed_user(pool, "active").await;
    let account = account(pool, owner, "a").await;
    let key = AccountQuotaKey {
        owner,
        account,
        platform: mastodon(),
        bucket: "posts".to_string(),
        period_start: period,
    };
    (owner, account, key)
}

async fn used(pool: &PgPool, key: &AccountQuotaKey) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT units_used FROM platform_quota_usage \
         WHERE account_id = $1 AND bucket = $2 AND period_start = $3",
    )
    .bind(key.account.as_uuid())
    .bind(&key.bucket)
    .bind(key.period_start)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn reserve_within_limit_then_refuse(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(QuotaRepo::reserve(&mut conn, &key, 2, 3).await.unwrap());
    assert!(!QuotaRepo::reserve(&mut conn, &key, 2, 3).await.unwrap());
    assert_eq!(used(&pool, &key).await, Some(2));
    QuotaRepo::release(&mut conn, &key, 2).await.unwrap();
    assert!(QuotaRepo::reserve(&mut conn, &key, 2, 3).await.unwrap());
}

#[sqlx::test]
async fn first_reservation_over_the_limit_inserts_nothing(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(!QuotaRepo::reserve(&mut conn, &key, 5, 3).await.unwrap());
    assert_eq!(used(&pool, &key).await, None);
}

#[sqlx::test]
async fn concurrent_reservations_never_exceed_limit(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut tasks = JoinSet::new();
    for _ in 0..20 {
        let (pool, key) = (pool.clone(), key.clone());
        tasks.spawn(async move {
            let mut conn = pool.acquire().await.unwrap();
            QuotaRepo::reserve(&mut conn, &key, 1, 7).await.unwrap()
        });
    }
    let mut granted = 0;
    while let Some(result) = tasks.join_next().await {
        granted += i32::from(result.unwrap());
    }
    assert_eq!(granted, 7);
    assert_eq!(used(&pool, &key).await, Some(7));
}

#[sqlx::test]
async fn reserve_up_to_grants_remainder(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(QuotaRepo::reserve(&mut conn, &key, 8, 10).await.unwrap());
    assert_eq!(
        QuotaRepo::reserve_up_to(&mut conn, &key, 5, 10)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        QuotaRepo::reserve_up_to(&mut conn, &key, 5, 10)
            .await
            .unwrap(),
        0
    );
    assert_eq!(used(&pool, &key).await, Some(10));
}

#[sqlx::test]
async fn reserve_up_to_on_a_new_bucket_grants_up_to_the_limit(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        QuotaRepo::reserve_up_to(&mut conn, &key, 4, 10)
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        QuotaRepo::reserve_up_to(&mut conn, &key, 50, 10)
            .await
            .unwrap(),
        6
    );
}

#[sqlx::test]
async fn concurrent_reserve_up_to_never_exceeds_limit(pool: PgPool) {
    let (_, _, mut key) = key(&pool, day(1)).await;
    key.bucket = EMAIL_CAP_BUCKET.to_string();
    let mut tasks = JoinSet::new();
    for _ in 0..12 {
        let (pool, key) = (pool.clone(), key.clone());
        tasks.spawn(async move {
            let mut conn = pool.acquire().await.unwrap();
            QuotaRepo::reserve_up_to(&mut conn, &key, 4, 25)
                .await
                .unwrap()
        });
    }
    let mut total = 0;
    while let Some(result) = tasks.join_next().await {
        total += result.unwrap();
    }
    assert_eq!(total, 25);
    assert_eq!(used(&pool, &key).await, Some(25));
}

#[sqlx::test]
async fn reserve_up_to_unlimited_writes_nothing(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        QuotaRepo::reserve_up_to(&mut conn, &key, 9, 0)
            .await
            .unwrap(),
        9
    );
    assert!(QuotaRepo::reserve(&mut conn, &key, 9, 0).await.unwrap());
    assert_eq!(used(&pool, &key).await, None);
}

#[sqlx::test]
async fn periods_are_separate_rows(pool: PgPool) {
    let (_, _, first) = key(&pool, day(1)).await;
    let second = AccountQuotaKey {
        period_start: day(2),
        ..first.clone()
    };
    let mut conn = pool.acquire().await.unwrap();
    assert!(QuotaRepo::reserve(&mut conn, &first, 3, 3).await.unwrap());
    assert!(QuotaRepo::reserve(&mut conn, &second, 3, 3).await.unwrap());
    assert_eq!(used(&pool, &first).await, Some(3));
    assert_eq!(used(&pool, &second).await, Some(3));
}

#[sqlx::test]
async fn release_never_goes_negative(pool: PgPool) {
    let (_, _, key) = key(&pool, day(1)).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(QuotaRepo::reserve(&mut conn, &key, 2, 5).await.unwrap());
    QuotaRepo::release(&mut conn, &key, 10).await.unwrap();
    assert_eq!(used(&pool, &key).await, Some(0));
    // Releasing a bucket that was never reserved is a no-op.
    let other = AccountQuotaKey {
        bucket: "other".to_string(),
        ..key
    };
    QuotaRepo::release(&mut conn, &other, 1).await.unwrap();
    assert_eq!(used(&pool, &other).await, None);
}

#[sqlx::test]
async fn app_quota_concurrent_never_exceeds(pool: PgPool) {
    let _ = account(&pool, seed_user(&pool, "active").await, "a").await;
    let key = AppQuotaKey {
        platform: mastodon(),
        bucket: "app_posts".to_string(),
        period_start: day(1),
    };
    let mut tasks = JoinSet::new();
    for _ in 0..20 {
        let (pool, key) = (pool.clone(), key.clone());
        tasks.spawn(async move {
            let mut conn = pool.acquire().await.unwrap();
            PlatformAppQuotaRepo::reserve(&mut conn, &key, 1, 7)
                .await
                .unwrap()
        });
    }
    let mut granted = 0;
    while let Some(result) = tasks.join_next().await {
        granted += i32::from(result.unwrap());
    }
    assert_eq!(granted, 7);

    let mut conn = pool.acquire().await.unwrap();
    PlatformAppQuotaRepo::release(&mut conn, &key, 2)
        .await
        .unwrap();
    let usage = PlatformAppQuotaRepo::usage(&mut conn).await.unwrap();
    assert_eq!(
        usage,
        vec![(mastodon(), "app_posts".to_string(), day(1), 5)]
    );
}
