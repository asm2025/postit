#![allow(clippy::unwrap_used)]

mod common;

use chrono::Utc;
use common::{
    account, create_job, later, new_job, post, snapshot, wait_for_blocked_statement, within,
};
use emixdb::dto::Pagination;
use postit_core::{AccountId, JobStatus, PublishJobId};
use postit_data::posts::{PostContent, PostRepo};
use postit_data::publishing::{JobFilter, JobRepo, delivery_idempotency_key};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

#[test]
fn idempotency_key_is_stable_and_order_sensitive() {
    let job = PublishJobId::from_uuid(Uuid::from_u128(1));
    let acct = AccountId::from_uuid(Uuid::from_u128(2));
    let swapped_job = PublishJobId::from_uuid(Uuid::from_u128(2));
    let swapped_acct = AccountId::from_uuid(Uuid::from_u128(1));
    assert_eq!(
        delivery_idempotency_key(job, acct),
        delivery_idempotency_key(job, acct)
    );
    assert_ne!(
        delivery_idempotency_key(job, acct),
        delivery_idempotency_key(swapped_job, swapped_acct)
    );
}

#[sqlx::test]
async fn create_inserts_job_and_pending_deliveries(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let p = post(&pool, owner, "hi").await;
    let at = later();

    let created = create_job(&pool, &scope, p, &[a, b], at).await;

    assert_eq!(created.deliveries.len(), 2);
    for dispatch in &created.deliveries {
        assert_eq!(dispatch.dispatch_gen, 0);
        assert_eq!(dispatch.run_at, at);
    }
    let rows: Vec<(String, Option<Uuid>, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
        "SELECT state, created_by, next_attempt_at FROM deliveries WHERE job_id = $1",
    )
    .bind(created.job.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    for (state, created_by, next) in rows {
        assert_eq!(state, "pending");
        assert_eq!(created_by, Some(owner.as_uuid()));
        assert_eq!(next, Some(at));
    }
    let job = JobRepo::get(&mut pool.acquire().await.unwrap(), &scope, created.job)
        .await
        .unwrap();
    assert_eq!(job.status, JobStatus::Scheduled);
    assert_eq!(job.post_version, 1);
}

#[sqlx::test]
async fn due_job_starts_publishing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;

    let at = Utc::now() - chrono::Duration::seconds(5);
    let created = create_job(&pool, &scope, p, &[a], at).await;

    let job = JobRepo::get(&mut pool.acquire().await.unwrap(), &scope, created.job)
        .await
        .unwrap();
    assert_eq!(job.status, JobStatus::Publishing);
}

#[sqlx::test]
async fn duplicate_delivery_rejected_by_idempotency_key(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let created = create_job(&pool, &scope, p, &[a], later()).await;

    let err = sqlx::query(
        "INSERT INTO deliveries (id, owner_id, job_id, post_id, account_id, operation, idempotency_key) \
         VALUES ($1, $2, $3, $4, $5, 'publish', $6)",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(created.job.as_uuid())
    .bind(p.as_uuid())
    .bind(a.as_uuid())
    .bind(delivery_idempotency_key(created.job, a).to_vec())
    .execute(&pool)
    .await
    .unwrap_err();

    assert!(err.as_database_error().unwrap().is_unique_violation());
}

#[sqlx::test]
async fn second_live_job_is_refused(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    create_job(&pool, &scope, p, &[a], later()).await;

    let mut conn = pool.acquire().await.unwrap();
    let err = PostRepo::lock(&mut conn, &scope, p, None)
        .await
        .unwrap_err();

    assert!(matches!(err, DataError::PostLocked));
}

#[sqlx::test]
async fn dry_run_not_covered_by_one_live_job_index(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let p = post(&pool, owner, "hi").await;
    for dry_run in [false, true] {
        sqlx::query(
            "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, status, dry_run) \
             VALUES ($1, $2, $3, 1, '{}', now(), 'publishing', $4)",
        )
        .bind(Uuid::now_v7())
        .bind(owner.as_uuid())
        .bind(p.as_uuid())
        .bind(dry_run)
        .execute(&pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn republish_same_version_after_terminal(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let first = create_job(&pool, &scope, p, &[a], later()).await;
    sqlx::query("UPDATE publish_jobs SET status = 'published' WHERE id = $1")
        .bind(first.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let second = create_job(&pool, &scope, p, &[a], later()).await;

    assert_ne!(first.job, second.job);
}

#[sqlx::test]
async fn snapshot_unchanged_by_later_edit(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let created = create_job(&pool, &scope, p, &[a], later()).await;
    let mut conn = pool.acquire().await.unwrap();
    let before = JobRepo::snapshot(&mut conn, created.job).await.unwrap();
    sqlx::query("UPDATE publish_jobs SET status = 'published' WHERE id = $1")
        .bind(created.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let locked = PostRepo::lock(&mut conn, &scope, p, None).await.unwrap();
    PostRepo::update_content(
        &mut conn,
        &locked,
        PostContent {
            reply_to: None,
            campaign_id: None,
            text: "edited".to_string(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();

    let after = JobRepo::snapshot(&mut conn, created.job).await.unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(before, snapshot(p, 1, &[a]));
}

#[sqlx::test]
async fn edit_delegate_cannot_create_live_job(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &who.edit, p, None).await.unwrap();

    let live = new_job(p, 1, &[a], later());
    let err = JobRepo::create(&mut tx, &who.edit, &locked, live)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        DataError::Scope(ScopeError::InsufficientAccess)
    ));

    let mut dry = new_job(p, 1, &[a], later());
    dry.dry_run = true;
    JobRepo::create(&mut tx, &who.edit, &locked, dry)
        .await
        .unwrap();
}

#[sqlx::test]
async fn limited_delegate_cannot_target_hidden_account(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let hidden = account(&pool, owner, "h").await;
    let p = post(&pool, owner, "hi").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &who.limited, p, None)
        .await
        .unwrap();

    let job = new_job(p, 1, &[a, hidden], later());
    let err = JobRepo::create(&mut tx, &who.limited, &locked, job)
        .await
        .unwrap_err();

    assert!(matches!(err, DataError::AccountNotInScope));
}

#[sqlx::test]
async fn limited_delegate_sees_only_its_jobs(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let p = post(&pool, owner, "p").await;
    let q = post(&pool, owner, "q").await;
    let who = callers(&pool, owner, &[a]).await;
    let both = create_job(&pool, &who.owner, p, &[a, b], later()).await;
    let only_b = create_job(&pool, &who.owner, q, &[b], later()).await;
    let mut conn = pool.acquire().await.unwrap();

    assert!(
        JobRepo::get(&mut conn, &who.limited, both.job)
            .await
            .is_ok()
    );
    assert!(matches!(
        JobRepo::get(&mut conn, &who.limited, only_b.job).await,
        Err(DataError::NotFound)
    ));
    let listed = JobRepo::list(&mut conn, &who.limited, page(), JobFilter::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert!(matches!(
        JobRepo::get(&mut conn, &who.stranger, both.job).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn edit_racing_job_creation(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;

    let mut tx1 = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx1, &scope, p, None).await.unwrap();
    JobRepo::create(&mut tx1, &scope, &locked, new_job(p, 1, &[a], later()))
        .await
        .unwrap();

    let other_pool = pool.clone();
    let other_scope = scope.clone();
    let editor = tokio::spawn(async move {
        let mut tx2 = other_pool.begin().await.unwrap();
        PostRepo::lock(&mut tx2, &other_scope, p, Some(1)).await
    });
    // Commit only once the editor is waiting on the post row.
    within(wait_for_blocked_statement(&pool)).await;
    tx1.commit().await.unwrap();

    let err = within(editor).await.unwrap().unwrap_err();
    assert!(matches!(err, DataError::PostLocked));
}
