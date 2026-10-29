#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, create_job, later, post, snapshot};
use postit_core::{
    AccountId, DeliveryId, DeliveryState, JobStatus, MediaAssetId, PostId, PublishJobId,
    ResolvedOutcome, UserId,
};
use postit_data::media::{MediaRepo, MediaStatus, NewMedia};
use postit_data::posts::PostRepo;
use postit_data::publishing::deliveries::{CancelOutcome, DeliveryRepo};
use postit_data::publishing::{Dispatch, JobRepo, NewDelivery, NewJob, Operation};
use postit_data::testkit::{Callers, callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    owner: UserId,
    accounts: Vec<AccountId>,
    post: PostId,
    job: PublishJobId,
    deliveries: Vec<Dispatch>,
    who: Callers,
}

/// A live job that is already due, one pending delivery per account.
async fn fixture(pool: &PgPool, count: usize, visible: usize) -> Fixture {
    let owner = seed_user(pool, "active").await;
    let mut accounts = Vec::new();
    for n in 0..count {
        accounts.push(account(pool, owner, &format!("acct{n}")).await);
    }
    let post = post(pool, owner, "hi").await;
    let who = callers(pool, owner, &accounts[..visible]).await;
    let created = create_job(
        pool,
        &who.owner,
        post,
        &accounts,
        at(Utc::now() - Duration::seconds(30)),
    )
    .await;
    Fixture {
        owner,
        accounts,
        post,
        job: created.job,
        deliveries: created.deliveries,
        who,
    }
}

async fn set_state(pool: &PgPool, id: DeliveryId, state: &str, reason: Option<&str>) {
    sqlx::query("UPDATE deliveries SET state = $2, parked_reason = $3 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(state)
        .bind(reason)
        .execute(pool)
        .await
        .unwrap();
}

async fn state_of(pool: &PgPool, id: DeliveryId) -> String {
    sqlx::query_scalar("SELECT state FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn job_status(pool: &PgPool, scope: &OwnerScope, job: PublishJobId) -> JobStatus {
    let mut conn = pool.acquire().await.unwrap();
    JobRepo::get(&mut conn, scope, job).await.unwrap().status
}

fn outcomes(list: Vec<(DeliveryId, CancelOutcome)>) -> HashMap<DeliveryId, CancelOutcome> {
    list.into_iter().collect()
}

#[sqlx::test]
async fn cancel_job_with_in_flight_delivery(pool: PgPool) {
    let f = fixture(&pool, 2, 2).await;
    let (d1, d2) = (f.deliveries[0].delivery, f.deliveries[1].delivery);
    set_state(&pool, d2, "in_flight", None).await;
    let mut conn = pool.acquire().await.unwrap();

    let result = outcomes(
        JobRepo::cancel(&mut conn, &f.who.owner, f.job, Utc::now())
            .await
            .unwrap(),
    );

    assert_eq!(result[&d1], CancelOutcome::Cancelled);
    assert_eq!(result[&d2], CancelOutcome::Requested);
    assert_eq!(state_of(&pool, d1).await, "cancelled");
    assert_eq!(state_of(&pool, d2).await, "in_flight");
    let requested: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT cancel_requested_at FROM deliveries WHERE id = $1")
            .bind(d2.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(requested.is_some());
    let job = JobRepo::get(&mut conn, &f.who.owner, f.job).await.unwrap();
    assert_eq!(job.status, JobStatus::Publishing);
    assert!(job.cancelled_at.is_some());
}

#[sqlx::test]
async fn cancel_native_scheduled_upload_needs_the_agent(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    let publish_at = at(Utc::now() + Duration::hours(2));
    sqlx::query("UPDATE deliveries SET state = 'processing', remote_publish_at = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(publish_at)
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let outcome = DeliveryRepo::cancel(&mut conn, &f.who.owner, id, Utc::now())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        CancelOutcome::NeedsAgent {
            remote_publish_at: publish_at
        }
    );
    assert_eq!(state_of(&pool, id).await, "processing");
}

#[sqlx::test]
async fn cancel_bumps_generation_of_pending(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    let mut conn = pool.acquire().await.unwrap();

    DeliveryRepo::cancel(&mut conn, &f.who.owner, id, Utc::now())
        .await
        .unwrap();

    let generation: i32 = sqlx::query_scalar("SELECT dispatch_gen FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(generation, 1);
    assert_eq!(
        job_status(&pool, &f.who.owner, f.job).await,
        JobStatus::Cancelled
    );
}

#[sqlx::test]
async fn limited_delegate_cannot_cancel_job_with_hidden_delivery(pool: PgPool) {
    let f = fixture(&pool, 2, 1).await;
    let (visible, hidden) = (f.deliveries[0].delivery, f.deliveries[1].delivery);
    let mut conn = pool.acquire().await.unwrap();

    let err = JobRepo::cancel(&mut conn, &f.who.limited, f.job, Utc::now())
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::AccountNotInScope));
    assert_eq!(state_of(&pool, visible).await, "pending");

    assert_eq!(
        DeliveryRepo::cancel(&mut conn, &f.who.limited, visible, Utc::now())
            .await
            .unwrap(),
        CancelOutcome::Cancelled
    );
    assert!(matches!(
        DeliveryRepo::cancel(&mut conn, &f.who.limited, hidden, Utc::now()).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn retry_from_outcome_unknown_needs_ack(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    set_state(&pool, id, "outcome_unknown", None).await;
    let mut conn = pool.acquire().await.unwrap();

    let err = DeliveryRepo::retry(&mut conn, &f.who.owner, id, false, Utc::now())
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::Conflict(code) if code == "acknowledgement_required"));
    assert_eq!(state_of(&pool, id).await, "outcome_unknown");

    let dispatch = DeliveryRepo::retry(&mut conn, &f.who.owner, id, true, Utc::now())
        .await
        .unwrap();
    assert_eq!(dispatch.delivery, id);
    assert_eq!(dispatch.dispatch_gen, 1);
    assert_eq!(state_of(&pool, id).await, "pending");
}

#[sqlx::test]
async fn retry_resets_attempts_and_redispatches(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    sqlx::query("UPDATE deliveries SET state = 'failed', attempt_count = 3 WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE publish_jobs SET status = 'failed' WHERE id = $1")
        .bind(f.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let now = at(Utc::now());

    let dispatch = DeliveryRepo::retry(&mut conn, &f.who.owner, id, false, now)
        .await
        .unwrap();

    assert_eq!(dispatch.dispatch_gen, 1);
    assert_eq!(dispatch.run_at, now);
    let row: (i32, Option<Uuid>) =
        sqlx::query_as("SELECT attempt_count, updated_by FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, (0, Some(f.owner.as_uuid())));
    assert_eq!(
        job_status(&pool, &f.who.owner, f.job).await,
        JobStatus::Publishing
    );
}

#[sqlx::test]
async fn retry_refused_while_another_live_job_active(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    set_state(&pool, id, "failed", None).await;
    sqlx::query("UPDATE publish_jobs SET status = 'failed' WHERE id = $1")
        .bind(f.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    create_job(&pool, &f.who.owner, f.post, &f.accounts, later()).await;
    let mut conn = pool.acquire().await.unwrap();

    let err = DeliveryRepo::retry(&mut conn, &f.who.owner, id, false, Utc::now())
        .await
        .unwrap_err();

    assert!(matches!(err, DataError::JobActive));
    assert_eq!(state_of(&pool, id).await, "failed");
}

#[sqlx::test]
async fn reschedule_refused_when_one_in_flight(pool: PgPool) {
    let f = fixture(&pool, 2, 2).await;
    set_state(&pool, f.deliveries[1].delivery, "in_flight", None).await;
    let before: Vec<(Uuid, i32, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, dispatch_gen, next_attempt_at FROM deliveries WHERE job_id = $1 ORDER BY id",
    )
    .bind(f.job.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let err = JobRepo::reschedule_pending(&mut conn, &f.who.owner, f.job, later(), Utc::now())
        .await
        .unwrap_err();

    assert!(matches!(err, DataError::Conflict(code) if code == "not_reschedulable"));
    let after: Vec<(Uuid, i32, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, dispatch_gen, next_attempt_at FROM deliveries WHERE job_id = $1 ORDER BY id",
    )
    .bind(f.job.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
}

#[sqlx::test]
async fn reschedule_moves_pending_with_new_gen(pool: PgPool) {
    let f = fixture(&pool, 2, 2).await;
    let when = later();
    let mut conn = pool.acquire().await.unwrap();

    let moved = JobRepo::reschedule_pending(&mut conn, &f.who.owner, f.job, when, Utc::now())
        .await
        .unwrap();

    assert_eq!(moved.dispatches.len(), 2);
    for dispatch in &moved.dispatches {
        assert_eq!(dispatch.dispatch_gen, 1);
        assert_eq!(dispatch.run_at, when);
    }
    assert!(moved.needs_agent.is_empty());
    let job = JobRepo::get(&mut conn, &f.who.owner, f.job).await.unwrap();
    assert_eq!(job.scheduled_at, when);
    assert_eq!(job.status, JobStatus::Scheduled);
}

#[sqlx::test]
async fn reschedule_hands_native_uploads_to_the_agent(pool: PgPool) {
    let f = fixture(&pool, 2, 2).await;
    let native = f.deliveries[1].delivery;
    sqlx::query(
        "UPDATE deliveries SET state = 'processing', remote_publish_at = now() + interval '3 hours' \
         WHERE id = $1",
    )
    .bind(native.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let moved = JobRepo::reschedule_pending(&mut conn, &f.who.owner, f.job, later(), Utc::now())
        .await
        .unwrap();

    assert_eq!(moved.dispatches.len(), 1);
    assert_eq!(moved.dispatches[0].delivery, f.deliveries[0].delivery);
    assert_eq!(moved.needs_agent, vec![native]);
    assert_eq!(state_of(&pool, native).await, "processing");
}

#[sqlx::test]
async fn resolve_dead_letter_to_published(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    set_state(&pool, id, "dead_letter", None).await;
    let mut conn = pool.acquire().await.unwrap();

    let state = DeliveryRepo::resolve(
        &mut conn,
        &f.who.owner,
        id,
        ResolvedOutcome::Published,
        Utc::now(),
    )
    .await
    .unwrap();

    assert_eq!(state, DeliveryState::Published);
    assert_eq!(state_of(&pool, id).await, "published");
    assert_eq!(
        job_status(&pool, &f.who.owner, f.job).await,
        JobStatus::Published
    );
}

#[sqlx::test]
async fn media_referenced_by_active_snapshot_in_use(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let asset = MediaAssetId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    MediaRepo::insert(
        &mut conn,
        &scope,
        NewMedia {
            id: asset,
            storage_key: format!("media/{asset}"),
            original_filename: None,
            mime: "image/png".to_string(),
            bytes: 1,
            status: MediaStatus::Ready,
            declared_bytes: None,
            upload_expires_at: None,
            derived_from: None,
            fit_spec: None,
        },
    )
    .await
    .unwrap();
    let mut snap = snapshot(p, 1, &[a]);
    snap.variants[0].media.push(postit_core::MediaItem {
        asset,
        alt_text: None,
    });
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, p, None).await.unwrap();
    let job = NewJob {
        id: PublishJobId::from_uuid(Uuid::now_v7()),
        snapshot: snap,
        targets: vec![NewDelivery {
            id: DeliveryId::from_uuid(Uuid::now_v7()),
            account: a,
            operation: Operation::Publish,
            due_at: None,
        }],
        scheduled_at: later(),
        native_schedule: false,
        dry_run: false,
        recurrence_id: None,
    };
    let job_id = job.id;
    JobRepo::create(&mut tx, &scope, &locked, job)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let err = MediaRepo::delete(&mut conn, &scope, asset)
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::Conflict(code) if code == "media_in_use"));

    sqlx::query("UPDATE publish_jobs SET status = 'published' WHERE id = $1")
        .bind(job_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    MediaRepo::delete(&mut conn, &scope, asset).await.unwrap();
}

#[sqlx::test]
async fn cancel_for_account_cancels_pending_and_parked_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let scope = OwnerScope::own(owner);
    let mut ids = Vec::new();
    for _ in 0..3 {
        let p = post(&pool, owner, "hi").await;
        let created = create_job(&pool, &scope, p, &[a], later()).await;
        ids.push(created.deliveries[0].delivery);
    }
    set_state(&pool, ids[1], "parked", Some("quota_exceeded")).await;
    set_state(&pool, ids[2], "in_flight", None).await;
    let mut conn = pool.acquire().await.unwrap();

    let result = outcomes(
        DeliveryRepo::cancel_for_account(&mut conn, a, Utc::now())
            .await
            .unwrap(),
    );

    assert_eq!(result.len(), 2);
    assert_eq!(result[&ids[0]], CancelOutcome::Cancelled);
    assert_eq!(result[&ids[1]], CancelOutcome::Cancelled);
    assert_eq!(state_of(&pool, ids[2]).await, "in_flight");
}

#[sqlx::test]
async fn cancel_for_account_cancels_running_direct_delivery(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let scope = OwnerScope::own(owner);
    let p = post(&pool, owner, "hi").await;
    let created = create_job(&pool, &scope, p, &[a], at(Utc::now())).await;
    let direct = created.deliveries[0].delivery;
    sqlx::query("UPDATE deliveries SET operation = 'direct', state = 'processing' WHERE id = $1")
        .bind(direct.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    for (n, state) in ["sent", "queued", "queued"].into_iter().enumerate() {
        let mut hash = format!("r{n}@example.com").into_bytes();
        hash.resize(32, 0);
        sqlx::query(
            "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash, state, sent_at)
             VALUES ($1, $2, $3, $4, $5, CASE WHEN $5 = 'sent' THEN now() END)",
        )
        .bind(Uuid::now_v7())
        .bind(owner.as_uuid())
        .bind(direct.as_uuid())
        .bind(hash)
        .bind(state)
        .execute(&pool)
        .await
        .unwrap();
    }
    let mut conn = pool.acquire().await.unwrap();

    let result = outcomes(
        DeliveryRepo::cancel_for_account(&mut conn, a, Utc::now())
            .await
            .unwrap(),
    );

    assert_eq!(result[&direct], CancelOutcome::Cancelled);
    assert_eq!(state_of(&pool, direct).await, "cancelled");
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT state, error_code FROM direct_sends WHERE delivery_id = $1 ORDER BY id",
    )
    .bind(direct.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("sent".to_owned(), None),
            ("failed".to_owned(), Some("cancelled".to_owned())),
            ("failed".to_owned(), Some("cancelled".to_owned())),
        ]
    );
    let counts: serde_json::Value =
        sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
            .bind(direct.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(counts["sent"], 1);
    assert_eq!(counts["failed"], 2);
    assert_eq!(counts["queued"], 0);
}

#[sqlx::test]
async fn view_and_edit_delegates_cannot_act(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    let mut conn = pool.acquire().await.unwrap();
    for scope in [&f.who.view, &f.who.edit] {
        let insufficient =
            |err: DataError| matches!(err, DataError::Scope(ScopeError::InsufficientAccess));
        assert!(insufficient(
            DeliveryRepo::cancel(&mut conn, scope, id, Utc::now())
                .await
                .unwrap_err()
        ));
        assert!(insufficient(
            DeliveryRepo::retry(&mut conn, scope, id, true, Utc::now())
                .await
                .unwrap_err()
        ));
        assert!(insufficient(
            DeliveryRepo::resolve(&mut conn, scope, id, ResolvedOutcome::Failed, Utc::now())
                .await
                .unwrap_err()
        ));
        assert!(insufficient(
            JobRepo::cancel(&mut conn, scope, f.job, Utc::now())
                .await
                .unwrap_err()
        ));
        assert!(insufficient(
            JobRepo::reschedule_pending(&mut conn, scope, f.job, later(), Utc::now())
                .await
                .unwrap_err()
        ));
    }
    assert_eq!(state_of(&pool, id).await, "pending");
}

#[sqlx::test]
async fn reads_are_filtered_by_the_account_limit(pool: PgPool) {
    let f = fixture(&pool, 2, 1).await;
    let (visible, hidden) = (f.deliveries[0].delivery, f.deliveries[1].delivery);
    let mut conn = pool.acquire().await.unwrap();

    let delivery = DeliveryRepo::get(&mut conn, &f.who.limited, visible)
        .await
        .unwrap();
    assert_eq!(delivery.id, visible);
    assert_eq!(delivery.state, DeliveryState::Pending);
    assert_eq!(delivery.account_id, f.accounts[0]);
    assert!(matches!(
        DeliveryRepo::get(&mut conn, &f.who.limited, hidden).await,
        Err(DataError::NotFound)
    ));
    let listed = DeliveryRepo::list_for_job(&mut conn, &f.who.limited, f.job)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        DeliveryRepo::list_for_job(&mut conn, &f.who.view, f.job)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        DeliveryRepo::get(&mut conn, &f.who.stranger, visible).await,
        Err(DataError::NotFound)
    ));
}

/// Makes `id` a direct delivery that ended `failed` with `rows` recipient rows (all stopped).
async fn failed_direct(pool: &PgPool, f: &Fixture, id: DeliveryId, rows: usize) {
    sqlx::query(
        "UPDATE deliveries SET operation = 'direct', state = 'failed', \
         direct_counts = '{\"audience\": 0}' WHERE id = $1",
    )
    .bind(id.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE publish_jobs SET status = 'failed' WHERE id = $1")
        .bind(f.job.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    for n in 0..rows {
        let contact = Uuid::now_v7();
        let hash = vec![u8::try_from(n).unwrap(); 32];
        sqlx::query(
            "INSERT INTO contacts (id, owner_id, email, email_hash) VALUES ($1, $2, $3, $4)",
        )
        .bind(contact)
        .bind(f.owner.as_uuid())
        .bind(format!("c{n}@example.test"))
        .bind(&hash)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO direct_sends (id, owner_id, delivery_id, contact_id, email_hash, state, \
             error_code) VALUES ($1, $2, $3, $4, $5, 'failed', 'cancelled')",
        )
        .bind(Uuid::now_v7())
        .bind(f.owner.as_uuid())
        .bind(id.as_uuid())
        .bind(contact)
        .bind(&hash)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn retry_refuses_a_direct_delivery_that_materialized_its_audience(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    failed_direct(&pool, &f, id, 2).await;
    let mut conn = pool.acquire().await.unwrap();

    let err = DeliveryRepo::retry(&mut conn, &f.who.owner, id, false, Utc::now())
        .await
        .unwrap_err();

    assert!(matches!(err, DataError::Conflict(code) if code == "not_retryable"));
    assert_eq!(state_of(&pool, id).await, "failed");
}

#[sqlx::test]
async fn retry_of_a_direct_delivery_without_rows_materializes_afresh(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    failed_direct(&pool, &f, id, 0).await;
    let mut conn = pool.acquire().await.unwrap();

    DeliveryRepo::retry(&mut conn, &f.who.owner, id, false, Utc::now())
        .await
        .unwrap();

    assert_eq!(state_of(&pool, id).await, "pending");
    let counts: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(counts, None, "the next run materializes the audience again");
}

#[sqlx::test]
async fn native_publish_time_write_is_fenced(pool: PgPool) {
    let f = fixture(&pool, 1, 1).await;
    let id = f.deliveries[0].delivery;
    let first = later();
    sqlx::query(
        "UPDATE deliveries SET state = 'processing', remote_publish_at = $2, dispatch_gen = 1 \
         WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(first)
    .execute(&pool)
    .await
    .unwrap();
    let moved = at(first + Duration::hours(1));
    let mut conn = pool.acquire().await.unwrap();
    let fence = |dispatch_gen| postit_data::publishing::Fence {
        id,
        dispatch_gen,
        expected: DeliveryState::Processing,
    };

    let stale = DeliveryRepo::set_remote_publish_at(&mut conn, fence(0), moved, Utc::now())
        .await
        .unwrap();
    assert!(stale.is_none());
    let written = DeliveryRepo::set_remote_publish_at(&mut conn, fence(1), moved, Utc::now())
        .await
        .unwrap();
    assert!(written.is_some());

    let stored: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT remote_publish_at FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, Some(moved));
}

#[sqlx::test]
async fn cancel_with_status_reports_the_job_change(pool: PgPool) {
    let f = fixture(&pool, 2, 2).await;
    let (d1, d2) = (f.deliveries[0].delivery, f.deliveries[1].delivery);
    set_state(&pool, d1, "published", None).await;
    let mut conn = pool.acquire().await.unwrap();

    let (outcome, change) =
        DeliveryRepo::cancel_with_status(&mut conn, &f.who.owner, d2, Utc::now())
            .await
            .unwrap();

    assert_eq!(outcome, CancelOutcome::Cancelled);
    let change = change.unwrap();
    assert!(change.became_partial(), "{change:?}");
    let (outcomes, change) =
        JobRepo::cancel_with_status(&mut conn, &f.who.owner, f.job, Utc::now())
            .await
            .unwrap();
    assert!(outcomes.is_empty());
    assert!(!change.became_partial());
}
