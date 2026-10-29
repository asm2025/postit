//! The publishing step of user deletion (Task 13): it cancels the user's deliveries, deletes
//! native uploads through their agents, and asks for a retry while a worker still holds one.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use chrono::Utc;
use common::{
    JobSpec, create_job, fixture, force, native_upload, notices, service_without_native_scheduler,
    state,
};
use postit_core::{DeliveryId, DeliveryState};
use postit_identity::deletion::UserDeletionStep;
use postit_jobs::JobError;
use postit_publishing::PublishingDeletionStep;
use sqlx::PgPool;

async fn cancel_requested(pool: &PgPool, id: DeliveryId) -> bool {
    sqlx::query_scalar("SELECT cancel_requested_at IS NOT NULL FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn job_version(pool: &PgPool, id: DeliveryId) -> i32 {
    sqlx::query_scalar(
        "SELECT j.version FROM publish_jobs j JOIN deliveries d ON d.job_id = j.id \
         WHERE d.id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_user_with_pending_in_flight_and_native_deliveries(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let pending = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let running = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    force(&pool, running.delivery, "in_flight").await;
    let step = PublishingDeletionStep::new(fx.service.clone());
    assert_eq!(step.name(), "publishing");

    let first = step.run(fx.env.owner).await;

    assert!(matches!(first, Err(JobError::Retry(_))), "{first:?}");
    assert_eq!(
        state(&pool, pending.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Cancelled
    );
    assert!(
        fx.script.posts().is_empty(),
        "the native upload was deleted"
    );
    assert_eq!(
        state(&pool, running.delivery).await,
        DeliveryState::InFlight
    );
    assert!(cancel_requested(&pool, running.delivery).await);

    // Still running: the step keeps asking for a retry.
    assert!(matches!(
        step.run(fx.env.owner).await,
        Err(JobError::Retry(_))
    ));

    // The worker finishes.
    sqlx::query("UPDATE deliveries SET state = 'published', lease_expires_at = NULL WHERE id = $1")
        .bind(running.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    step.run(fx.env.owner).await.unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publishing_step_is_idempotent(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let pending = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let step = PublishingDeletionStep::new(fx.service.clone());

    step.run(fx.env.owner).await.unwrap();
    let calls = fx.script.mutating_calls();
    let versions = (
        job_version(&pool, native.delivery).await,
        job_version(&pool, pending.delivery).await,
    );
    let notified = notices(&pool, fx.env.owner).await.len();

    step.run(fx.env.owner).await.unwrap();

    assert_eq!(
        state(&pool, pending.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(fx.script.mutating_calls(), calls, "no second platform call");
    assert_eq!(
        (
            job_version(&pool, native.delivery).await,
            job_version(&pool, pending.delivery).await,
        ),
        versions,
        "nothing changed"
    );
    assert_eq!(notices(&pool, fx.env.owner).await.len(), notified);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn transient_native_delete_failure_retries_then_completes(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let step = PublishingDeletionStep::new(fx.service.clone());
    fx.script.fail_next_deletes(1);

    let first = step.run(fx.env.owner).await;

    assert!(matches!(first, Err(JobError::Retry(_))), "{first:?}");
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(fx.script.posts().len(), 1, "the platform still holds it");

    // The platform answers again.
    step.run(fx.env.owner).await.unwrap();

    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Cancelled
    );
    assert!(fx.script.posts().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_upload_the_agent_cannot_delete_does_not_block_deletion(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let step = PublishingDeletionStep::new(service_without_native_scheduler(&fx));

    step.run(fx.env.owner).await.unwrap();

    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(
        fx.script.posts().len(),
        1,
        "left on the platform, and logged"
    );
}
