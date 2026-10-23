//! Regressions found by the whole-branch review of B2b.
#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, create_job, post, wait_for_blocked_statement};
use postit_core::{DeliveryEvent, DeliveryState, JobStatus, UserId};
use postit_data::OwnerScope;
use postit_data::publishing::deliveries::{Applied, DeliveryRepo, Fence, TransitionData};
use postit_data::publishing::sweep::{Sweep, UnparkInput};
use postit_data::publishing::{Dispatch, JobRepo};
use postit_data::testkit::seed_user;
use sqlx::PgPool;

const LEASE: Duration = Duration::minutes(5);

fn ago(minutes: i64) -> DateTime<Utc> {
    at(Utc::now() - Duration::minutes(minutes))
}

/// A due live job with `accounts` deliveries.
async fn fixture(
    pool: &PgPool,
    accounts: usize,
) -> (UserId, postit_core::PublishJobId, Vec<Dispatch>) {
    let owner = seed_user(pool, "active").await;
    let post = post(pool, owner, "hi").await;
    let mut ids = Vec::new();
    for n in 0..accounts {
        ids.push(account(pool, owner, &format!("acct{n}")).await);
    }
    let created = create_job(pool, &OwnerScope::own(owner), post, &ids, ago(10)).await;
    (owner, created.job, created.deliveries)
}

/// A job-wide cancel and a worker write on one delivery lock the same two rows. Both must take
/// the delivery before the job, or the pair can deadlock.
#[sqlx::test]
async fn cancel_does_not_deadlock_with_a_worker_write(pool: PgPool) {
    let (owner, job, deliveries) = fixture(&pool, 1).await;
    let delivery = deliveries[0].delivery;

    // The worker holds the delivery row, as `apply` does before it touches the job.
    let mut worker = pool.begin().await.unwrap();
    sqlx::query("UPDATE deliveries SET updated_at = updated_at WHERE id = $1")
        .bind(delivery.as_uuid())
        .execute(&mut *worker)
        .await
        .unwrap();

    let canceller = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            JobRepo::cancel(&mut tx, &OwnerScope::own(owner), job, Utc::now())
                .await
                .unwrap();
            tx.commit().await.unwrap();
        })
    };
    wait_for_blocked_statement(&pool).await;

    // Now the worker reaches for the job row, as `touch_job` does.
    sqlx::query("UPDATE publish_jobs SET version = version + 1 WHERE id = $1")
        .bind(job.as_uuid())
        .execute(&mut *worker)
        .await
        .unwrap_or_else(|e| unreachable!("the worker deadlocked with the cancel: {e}"));
    worker.commit().await.unwrap();
    canceller.await.unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM publish_jobs WHERE id = $1")
        .bind(job.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");
}

/// Rows whose owner was already told never block newer ones from resuming.
#[sqlx::test]
async fn unpark_not_starved_by_notified_stale_rows(pool: PgPool) {
    let (owner, _, deliveries) = fixture(&pool, 2).await;
    let (stale, fresh) = (deliveries[0].delivery, deliveries[1].delivery);
    // Both blockers cleared (the owner is active). `stale` was due an hour ago and the owner has
    // been told; `fresh` was due a minute ago.
    for (id, due, notified) in [(stale, ago(60), true), (fresh, ago(1), false)] {
        sqlx::query(
            "UPDATE deliveries SET state = 'parked', parked_reason = 'owner_disabled', \
             next_attempt_at = $2, parked_notified_at = CASE WHEN $3 THEN now() END WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(due)
        .bind(notified)
        .execute(&pool)
        .await
        .unwrap();
    }
    let mut conn = pool.acquire().await.unwrap();
    let result = Sweep::unpark(
        &mut conn,
        UnparkInput {
            now: Utc::now(),
            park_grace: Duration::minutes(5),
            compiled_platforms: &[],
            limit: 1,
        },
    )
    .await
    .unwrap();
    let resumed: Vec<_> = result.resumed.iter().map(|d| d.delivery).collect();
    assert_eq!(
        resumed,
        vec![fresh],
        "the stale row used up the whole limit"
    );
    let _ = owner;
}

/// A delivery that fails and would retry after its job was cancelled ends cancelled, so the job
/// can finish and free the post.
#[sqlx::test]
async fn retry_after_cancel_during_flight_ends_cancelled(pool: PgPool) {
    let (owner, job, deliveries) = fixture(&pool, 1).await;
    let Dispatch {
        delivery,
        dispatch_gen,
        ..
    } = deliveries[0];
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::claim(&mut conn, delivery, dispatch_gen, Utc::now(), LEASE)
        .await
        .unwrap()
        .unwrap();
    let fence = |expected| Fence {
        id: delivery,
        dispatch_gen,
        expected,
    };
    DeliveryRepo::apply(
        &mut conn,
        fence(DeliveryState::Claimed),
        DeliveryEvent::StartRemote,
        TransitionData::default(),
        Utc::now(),
    )
    .await
    .unwrap();

    JobRepo::cancel(&mut conn, &OwnerScope::own(owner), job, Utc::now())
        .await
        .unwrap();

    let applied = DeliveryRepo::apply(
        &mut conn,
        fence(DeliveryState::InFlight),
        DeliveryEvent::RetryableFailure,
        TransitionData::default(),
        Utc::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        applied,
        Applied::Done {
            state: DeliveryState::Cancelled,
            redispatch: None,
        }
    );
    let status = JobRepo::get(&mut conn, &OwnerScope::own(owner), job)
        .await
        .unwrap()
        .status;
    assert_eq!(status, JobStatus::Cancelled);
}
