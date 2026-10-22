#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, new_job, post};
use postit_core::{AccountId, PostId, RecurrenceId, UserId};
use postit_data::DataError;
use postit_data::publishing::JobRepo;
use postit_data::recurrences::RecurrenceRepo;
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    owner: UserId,
    post: PostId,
    account: AccountId,
    recurrence: RecurrenceId,
    due: DateTime<Utc>,
}

/// A running recurrence on a post, due a minute ago.
async fn fixture(pool: &PgPool) -> Fixture {
    let owner = seed_user(pool, "active").await;
    let post = post(pool, owner, "weekly").await;
    let account = account(pool, owner, "a").await;
    let recurrence = RecurrenceId::from_uuid(Uuid::now_v7());
    let due = at(Utc::now() - Duration::minutes(1));
    sqlx::query(
        "INSERT INTO recurrences (id, owner_id, post_id, rule, timezone, next_occurrence_at, \
         confirmed_version, confirmed_by, confirmed_at) \
         VALUES ($1, $2, $3, 'FREQ=WEEKLY', 'UTC', $4, 1, $2, now())",
    )
    .bind(recurrence.as_uuid())
    .bind(owner.as_uuid())
    .bind(post.as_uuid())
    .bind(due)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO recurrence_targets (recurrence_id, account_id, owner_id) VALUES ($1, $2, $3)",
    )
    .bind(recurrence.as_uuid())
    .bind(account.as_uuid())
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    Fixture {
        owner,
        post,
        account,
        recurrence,
        due,
    }
}

fn occurrence(fx: &Fixture) -> postit_data::publishing::NewJob {
    let mut job = new_job(fx.post, 1, &[fx.account], fx.due);
    job.recurrence_id = Some(fx.recurrence);
    job
}

async fn job_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM publish_jobs")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn next_at(pool: &PgPool, id: RecurrenceId) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT next_occurrence_at FROM recurrences WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// One tick: take what is due, create each occurrence, advance. Returns the jobs it created.
async fn tick(pool: &PgPool, fx: &Fixture, next: DateTime<Utc>) -> usize {
    let mut tx = pool.begin().await.unwrap();
    let due = RecurrenceRepo::due(&mut tx, Utc::now(), 10).await.unwrap();
    let mut created = 0;
    for item in due {
        JobRepo::create_for_occurrence(&mut tx, item.owner_id, item.post_id, occurrence(fx))
            .await
            .unwrap();
        assert!(
            RecurrenceRepo::advance(&mut tx, item.id, item.next_occurrence_at, Some(next))
                .await
                .unwrap()
        );
        created += 1;
    }
    tx.commit().await.unwrap();
    created
}

#[sqlx::test]
async fn two_ticks_advance_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let next = at(Utc::now() + Duration::days(7));
    // Both ticks read the due list before either commits.
    let mut first = pool.begin().await.unwrap();
    let mut second = pool.begin().await.unwrap();
    let a = RecurrenceRepo::due(&mut first, Utc::now(), 10)
        .await
        .unwrap();
    let b = RecurrenceRepo::due(&mut second, Utc::now(), 10)
        .await
        .unwrap();
    assert_eq!(a.len(), 1);
    assert!(b.is_empty(), "the second tick skips the locked row");

    let item = &a[0];
    assert_eq!(item.targets, vec![fx.account]);
    assert_eq!(item.owner_id, fx.owner);
    JobRepo::create_for_occurrence(&mut first, item.owner_id, item.post_id, occurrence(&fx))
        .await
        .unwrap();
    assert!(
        RecurrenceRepo::advance(&mut first, item.id, item.next_occurrence_at, Some(next))
            .await
            .unwrap()
    );
    first.commit().await.unwrap();
    second.rollback().await.unwrap();

    assert_eq!(job_count(&pool).await, 1);
    assert_eq!(next_at(&pool, fx.recurrence).await, Some(next));
    let created: i32 =
        sqlx::query_scalar("SELECT occurrences_created FROM recurrences WHERE id = $1")
            .bind(fx.recurrence.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(created, 1);
    let by: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM publish_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        by,
        Some(fx.owner.as_uuid()),
        "the confirming user is the actor"
    );

    // A tick after the commit finds nothing due.
    assert_eq!(tick(&pool, &fx, next).await, 0);
}

#[sqlx::test]
async fn stale_advance_changes_nothing(pool: PgPool) {
    let fx = fixture(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    let stale = fx.due - Duration::hours(1);
    assert!(
        !RecurrenceRepo::advance(&mut conn, fx.recurrence, stale, Some(at(Utc::now())))
            .await
            .unwrap()
    );
    assert_eq!(next_at(&pool, fx.recurrence).await, Some(fx.due));
}

#[sqlx::test]
async fn paused_recurrence_not_due(pool: PgPool) {
    let _fx = fixture(&pool).await;
    sqlx::query("UPDATE recurrences SET paused = TRUE, paused_reason = 'user'")
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        RecurrenceRepo::due(&mut conn, Utc::now(), 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn future_recurrence_not_due(pool: PgPool) {
    let _fx = fixture(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    let earlier = Utc::now() - Duration::hours(1);
    assert!(
        RecurrenceRepo::due(&mut conn, earlier, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn advance_none_ends_recurrence(pool: PgPool) {
    let fx = fixture(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        RecurrenceRepo::advance(&mut conn, fx.recurrence, fx.due, None)
            .await
            .unwrap()
    );
    let (paused, reason, next): (bool, Option<String>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT paused, paused_reason, next_occurrence_at FROM recurrences WHERE id = $1",
    )
    .bind(fx.recurrence.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(paused);
    assert_eq!(reason.as_deref(), Some("ended"));
    assert_eq!(next, None);
}

#[sqlx::test]
async fn occurrence_skipped_while_previous_job_active(pool: PgPool) {
    let fx = fixture(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    JobRepo::create_for_occurrence(&mut conn, fx.owner, fx.post, occurrence(&fx))
        .await
        .unwrap();

    let err = JobRepo::create_for_occurrence(&mut conn, fx.owner, fx.post, occurrence(&fx))
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::PostLocked), "got {err:?}");
    assert_eq!(job_count(&pool).await, 1);

    RecurrenceRepo::record_skip(&mut conn, fx.recurrence, fx.due, "previous_job_active")
        .await
        .unwrap();
    let skipped: serde_json::Value =
        sqlx::query_scalar("SELECT skipped FROM recurrences WHERE id = $1")
            .bind(fx.recurrence.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(skipped.as_array().unwrap().len(), 1);
    assert_eq!(skipped[0]["reason"], "previous_job_active");
}

#[sqlx::test]
async fn skipped_keeps_newest_twenty(pool: PgPool) {
    let fx = fixture(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    for n in 0..25 {
        RecurrenceRepo::record_skip(&mut conn, fx.recurrence, fx.due, &format!("r{n}"))
            .await
            .unwrap();
    }
    let skipped: serde_json::Value =
        sqlx::query_scalar("SELECT skipped FROM recurrences WHERE id = $1")
            .bind(fx.recurrence.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    let reasons: Vec<&str> = skipped
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons.len(), 20);
    assert_eq!(reasons[0], "r5", "the oldest five are dropped, order kept");
    assert_eq!(reasons[19], "r24");
}
