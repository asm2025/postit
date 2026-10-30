//! Graceful shutdown (Review Focus 6): a delivery stops at its next safe checkpoint,
//! goes back to `pending` under the next generation, and publishes once after the restart; one
//! past its final call finishes; and a stopping server starts no new claim.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{
    Fixture, JobSpec, create_job, fixture, mock, publishing_settings, queued, row, run,
    service_full, state,
};
use postit_core::DeliveryState;
use postit_data::publishing::Dispatch;
use postit_publishing::worker::run_publish;
use postit_publishing::{PublishDelivery, PublishingService};
use serde_json::json;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

/// A service over the fixture's accounts that stops when `shutdown` is cancelled.
fn stoppable(fx: &Fixture, shutdown: CancellationToken) -> PublishingService {
    service_full(
        &fx.env,
        fx.accounts.clone(),
        vec![mock()],
        publishing_settings(),
        shutdown,
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn shutdown_at_checkpoint_requeues_and_publishes_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.set_publish_steps(2);
    let gate = fx.script.hold_after_chunk(1);
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let shutdown = CancellationToken::new();
    let stopping = stoppable(&fx, shutdown.clone());
    let worker = {
        let service = stopping.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);

    shutdown.cancel();
    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let stored = row(&pool, dispatch.delivery).await;
    assert_eq!(stored.dispatch_gen, dispatch.dispatch_gen + 1);
    assert_eq!(stored.attempt_count, 0, "a shutdown is no attempt");
    assert_eq!(stored.container_ref.as_deref(), Some("chunks:1"));
    assert!(fx.script.posts().is_empty());
    let next = Dispatch {
        dispatch_gen: stored.dispatch_gen,
        ..dispatch
    };
    let jobs = queued(&pool, "publish_delivery").await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": next.dispatch_gen})
    );

    // While the server is still stopping, the re-queued job claims nothing.
    run_publish(&stopping, PublishDelivery::from(next))
        .await
        .unwrap();
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );

    // After the restart it resumes after the uploaded chunk and publishes once.
    run(&fx.service, next).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(fx.script.chunk_uploads(), 2, "chunk 1 was not sent again");
    assert_eq!(
        fx.script.seen_container_refs(),
        vec![None, Some("chunks:1".to_owned())]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn shutdown_after_final_call_finishes_within_grace(pool: PgPool) {
    let fx = fixture(&pool).await;
    // One step: past its first checkpoint, the next call is the final create.
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let shutdown = CancellationToken::new();
    let worker = {
        let service = stoppable(&fx, shutdown.clone());
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);

    shutdown.cancel();
    gate.release();
    // Well inside `server.shutdown_timeout` (30 s by default).
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap_or_else(|_| panic!("the delivery did not finish within the grace period"))
        .unwrap()
        .unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(fx.script.posts().len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_new_claim_after_shutdown_begins(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let shutdown = CancellationToken::new();
    shutdown.cancel();

    run(&stoppable(&fx, shutdown), dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let stored = row(&pool, dispatch.delivery).await;
    assert_eq!(stored.dispatch_gen, dispatch.dispatch_gen, "never claimed");
    assert_eq!(fx.script.mutating_calls(), 0);
    // The delivery stays due, so the sweep hands it on after the restart.
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}
