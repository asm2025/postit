//! Regressions for the minor findings of the B2b review that were fixed after the plan.
#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, create_job, new_job, post};
use postit_core::{DeliveryEvent, DeliveryState, PublishJobId, UserId};
use postit_data::posts::PostRepo;
use postit_data::publishing::deliveries::{DeliveryRepo, Fence, TransitionData};
use postit_data::publishing::sweep::Sweep;
use postit_data::publishing::{Dispatch, JobRepo};
use postit_data::testkit::seed_user;
use postit_data::{DataError, OwnerScope};
use sqlx::PgPool;

const LEASE: Duration = Duration::minutes(5);

fn ago(minutes: i64) -> DateTime<Utc> {
    at(Utc::now() - Duration::minutes(minutes))
}

/// A due live job with one delivery, last touched ten minutes ago.
async fn fixture(pool: &PgPool) -> (UserId, PublishJobId, Dispatch) {
    let owner = seed_user(pool, "active").await;
    let post = post(pool, owner, "hi").await;
    let account = account(pool, owner, "a").await;
    let created = create_job(pool, &OwnerScope::own(owner), post, &[account], ago(10)).await;
    sqlx::query("UPDATE deliveries SET updated_at = $1")
        .bind(ago(10))
        .execute(pool)
        .await
        .unwrap();
    (owner, created.job, created.deliveries[0])
}

async fn job_version(pool: &PgPool, job: PublishJobId) -> i32 {
    sqlx::query_scalar("SELECT version FROM publish_jobs WHERE id = $1")
        .bind(job.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A cancel requested while a delivery was in flight does not follow it into a user retry.
#[sqlx::test]
async fn user_retry_clears_a_cancel_request(pool: PgPool) {
    let (owner, _, dispatch) = fixture(&pool).await;
    let id = dispatch.delivery;
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::claim(&mut conn, id, dispatch.dispatch_gen, Utc::now(), LEASE)
        .await
        .unwrap()
        .unwrap();
    let fence = |expected| Fence {
        id,
        dispatch_gen: dispatch.dispatch_gen,
        expected,
    };
    for (expected, event) in [
        (DeliveryState::Claimed, DeliveryEvent::StartRemote),
        (DeliveryState::InFlight, DeliveryEvent::PermanentFailure),
    ] {
        DeliveryRepo::apply(
            &mut conn,
            fence(expected),
            event,
            TransitionData::default(),
            Utc::now(),
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE deliveries SET cancel_requested_at = now() WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let retried = DeliveryRepo::retry(&mut conn, &OwnerScope::own(owner), id, false, Utc::now())
        .await
        .unwrap();
    let claimed = DeliveryRepo::claim(&mut conn, id, retried.dispatch_gen, Utc::now(), LEASE)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !claimed.cancel_requested,
        "the retry inherited the old cancel request"
    );
}

#[sqlx::test]
async fn overdue_pending_bumps_the_job_version(pool: PgPool) {
    let (_, job, _) = fixture(&pool).await;
    let before = job_version(&pool, job).await;
    let mut conn = pool.acquire().await.unwrap();
    let swept = Sweep::overdue_pending(&mut conn, Utc::now(), Duration::minutes(5), 10)
        .await
        .unwrap();
    assert_eq!(swept.len(), 1);
    assert_eq!(job_version(&pool, job).await, before + 1);
}

#[sqlx::test]
async fn reschedule_refuses_a_cancelled_job(pool: PgPool) {
    let (owner, job, _) = fixture(&pool).await;
    let scope = OwnerScope::own(owner);
    let mut conn = pool.acquire().await.unwrap();
    JobRepo::cancel(&mut conn, &scope, job, Utc::now())
        .await
        .unwrap();
    let err = JobRepo::reschedule_pending(
        &mut conn,
        &scope,
        job,
        Utc::now() + Duration::hours(2),
        Utc::now(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, DataError::Conflict(code) if code == "job_cancelled"),
        "got {err:?}"
    );
}

#[sqlx::test]
async fn a_job_without_targets_is_refused(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let post = post(&pool, owner, "hi").await;
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, post, None).await.unwrap();
    let version = locked.post().version;
    let mut job = new_job(post, version, &[], ago(1));
    job.targets.clear();
    let err = JobRepo::create(&mut tx, &scope, &locked, job)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, DataError::Conflict(code) if code == "no_targets"),
        "got {err:?}"
    );
}
