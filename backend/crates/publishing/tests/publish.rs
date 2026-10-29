//! The remote call and what its result does to the delivery (Task 8): outcomes, retries, the
//! lease renewer, container references, late receipts, and the notices and audit events they
//! record. Every test runs the `publish_delivery` handler against the `mock` agent.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use common::{
    Fixture, FixtureOptions, JobSpec, account_status, account_units, allow_notifications, attempts,
    audits, break_notifications, connect, create_job, fixture, fixture_with, last_error, make_due,
    notices, park_times, publishing_settings, queued, row, run, service_full, short_lease, state,
};
use postit_config::{JobSchedules, PublishingSettings};
use postit_core::Capability;
use postit_core::{DeliveryEvent, DeliveryState, Notice, ParkReason};
use postit_data::publishing::sweep::Sweep;
use postit_data::publishing::{CancelOutcome, DeliveryRepo, Dispatch, Fence, TransitionData};
use postit_jobs::testkit::{RunningWorker, wait_until_async};
use postit_jobs::{JobQueue, JobRegistry};
use postit_publishing::reconcile::run_reconcile;
use postit_publishing::worker::run_publish;
use postit_publishing::{
    DirectSendBatch, PollDeliveryStatus, PublishDelivery, PublishingService, ReconcileDelivery,
    register,
};
use serde_json::json;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

async fn with_settings(pool: &PgPool, settings: PublishingSettings) -> Fixture {
    fixture_with(
        pool,
        FixtureOptions {
            settings: Some(settings),
            ..FixtureOptions::default()
        },
    )
    .await
}

/// A second service over the fixture's accounts, with `settings` and `shutdown`.
fn other_service(
    fx: &Fixture,
    settings: PublishingSettings,
    shutdown: CancellationToken,
) -> PublishingService {
    service_full(
        &fx.env,
        fx.accounts.clone(),
        vec![common::mock()],
        settings,
        shutdown,
    )
}

/// Moves an `in_flight` delivery to `outcome_unknown`, as the sweep does once its lease ran out.
async fn take_over(pool: &PgPool, dispatch: Dispatch) {
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::apply(
        &mut conn,
        Fence {
            id: dispatch.delivery,
            dispatch_gen: dispatch.dispatch_gen,
            expected: DeliveryState::InFlight,
        },
        DeliveryEvent::LeaseExpired,
        TransitionData::default(),
        Utc::now(),
    )
    .await
    .unwrap();
}

fn micros(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(time.timestamp_micros()).unwrap()
}

fn kinds(all: &[Notice]) -> Vec<&'static str> {
    all.iter().map(|n| n.kind().as_str()).collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn published_records_receipt_and_notifies_info(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    let posts = fx.script.posts();
    assert_eq!(posts.len(), 1);
    let stored = row(&pool, dispatch.delivery).await;
    assert_eq!(
        stored.remote_id.as_deref(),
        Some(posts[0].remote_id.as_str())
    );
    assert_eq!(
        stored.remote_url,
        posts[0]
            .receipt
            .remote_url
            .as_ref()
            .map(ToString::to_string)
    );
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(kinds(&all), ["delivery_published"]);
    assert!(matches!(&all[0], Notice::DeliveryPublished(n) if n.delivery_id == dispatch.delivery));
    assert_eq!(attempts(&pool, dispatch.delivery).await, 1);
    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM delivery_attempts WHERE delivery_id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outcome.as_deref(), Some("published"));
    assert_eq!(audits(&pool, "delivery_published").await, 1);
    assert_eq!(account_units(&pool, fx.account).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn processing_schedules_a_poll(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("processing"),
    )
    .await[0];
    let before = Utc::now();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    let polls = queued(&pool, "poll_delivery_status").await;
    assert_eq!(polls.len(), 1);
    assert_eq!(
        polls[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 0, "poll": 0})
    );
    let wait = polls[0].1 - before;
    assert!(
        wait >= chrono::Duration::seconds(14) && wait <= chrono::Duration::seconds(20),
        "{wait}"
    );
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retryable_failure_backs_off_with_new_generation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("retryable_failure"),
    )
    .await[0];
    let before = Utc::now();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let stored = row(&pool, dispatch.delivery).await;
    assert_eq!(stored.dispatch_gen, 1);
    assert_eq!(stored.attempt_count, 1);
    let next = stored.next_attempt_at.unwrap();
    assert!(next >= micros(before) && next <= Utc::now() + chrono::Duration::seconds(31));
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "remote", "class": "retryable"})
    );
    let redispatched = queued(&pool, "publish_delivery").await;
    assert_eq!(redispatched.len(), 1);
    assert_eq!(
        redispatched[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 1})
    );
    assert_eq!(micros(redispatched[0].1), next);
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_budget_ends_in_dead_letter(pool: PgPool) {
    let fx = with_settings(
        &pool,
        PublishingSettings {
            max_attempts: 2,
            ..publishing_settings()
        },
    )
    .await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("retryable_failure"),
    )
    .await[0];

    run(&fx.service, dispatch).await;
    make_due(&pool, dispatch.delivery).await;
    run(
        &fx.service,
        Dispatch {
            dispatch_gen: 1,
            ..dispatch
        },
    )
    .await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::DeadLetter
    );
    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 2);
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_dead_lettered"]
    );
    assert_eq!(audits(&pool, "delivery_dead_lettered").await, 1);
    // No third run is queued.
    assert_eq!(queued(&pool, "publish_delivery").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn permanent_failure_fails(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("permanent_failure"),
    )
    .await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    let error = last_error(&pool, dispatch.delivery).await.unwrap();
    assert_eq!(error, json!({"code": "remote", "class": "permanent"}));
    assert!(!error.to_string().contains("refused"), "no platform text");
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_failed"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn needs_reconnect_parks_and_marks_the_grant(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("needs_reconnect"),
    )
    .await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::AccountNeedsReconnect)
    );
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("needs_reconnect".to_owned(), "needs_reconnect".to_owned())
    );
    let all = notices(&pool, fx.env.owner).await;
    // Marking the grant tells the owner about the account too (`AccountService`).
    let mut found = kinds(&all);
    found.sort_unstable();
    assert_eq!(found, ["account_needs_reconnect", "delivery_parked"]);
    assert!(all.iter().any(
        |n| matches!(n, Notice::DeliveryParked(n) if n.park_reason == Some(ParkReason::AccountNeedsReconnect))
    ));
    assert_eq!(audits(&pool, "account_needs_reconnect").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_mark_commits_with_the_park(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("needs_reconnect"),
    )
    .await[0];
    // The parked notice is the last write of the park's transaction: failing it rolls back
    // the park and the mark together.
    break_notifications(&pool).await;

    let result = run_publish(&fx.service, PublishDelivery::from(dispatch)).await;

    assert!(result.is_err());
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::InFlight
    );
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("active".to_owned(), "active".to_owned())
    );
    allow_notifications(&pool).await;
    // Nothing of the park survived: no notice, no reconnect audit, no parked error.
    assert!(notices(&pool, fx.env.owner).await.is_empty());
    assert_eq!(audits(&pool, "account_needs_reconnect").await, 0);
    assert_eq!(last_error(&pool, dispatch.delivery).await, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn quota_error_parks_until_resets_at(pool: PgPool) {
    let fx = fixture(&pool).await;
    let spec = JobSpec::outcome("quota_exhausted");
    let due = spec.scheduled_at;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];
    let before = Utc::now();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
    let (parked_until, next_attempt_at) = park_times(&pool, dispatch.delivery).await;
    // The mock's daily `posts` bucket resets at the next midnight UTC.
    let midnight = (before.date_naive() + chrono::Days::new(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let after_midnight = (Utc::now().date_naive() + chrono::Days::new(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    assert!(
        parked_until == Some(midnight) || parked_until == Some(after_midnight),
        "{parked_until:?}"
    );
    assert_eq!(next_attempt_at, Some(micros(due)), "still the due time");
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_parked"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rate_limited_uses_retry_after(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("rate_limited"),
    )
    .await[0];
    let before = Utc::now();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let next = row(&pool, dispatch.delivery).await.next_attempt_at.unwrap();
    let wait = next - before;
    // The mock asks for 30 seconds.
    assert!(
        wait >= chrono::Duration::seconds(29) && wait <= chrono::Duration::seconds(35),
        "{wait}"
    );
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "rate_limited", "class": "retry_after"})
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn outcome_unknown_enqueues_reconcile(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_lost"),
    )
    .await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    let reconciles = queued(&pool, "reconcile_delivery").await;
    assert_eq!(reconciles.len(), 1);
    assert_eq!(
        reconciles[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 0, "attempt": 0})
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_outcome_unknown"]
    );
    assert_eq!(audits(&pool, "delivery_outcome_unknown").await, 1);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "outcome_unknown", "class": "ambiguous"})
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn ambiguous_counts_an_attempt(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_landed"),
    )
    .await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn renewal_reports_cancel_request(pool: PgPool) {
    let fx = with_settings(&pool, short_lease()).await;
    fx.script.set_publish_steps(2);
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);

    let mut conn = pool.acquire().await.unwrap();
    let outcome = DeliveryRepo::cancel(&mut conn, &fx.env.scope, dispatch.delivery, Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, CancelOutcome::Requested);
    // The renewer runs every 200 ms and sees the request.
    tokio::time::sleep(Duration::from_millis(700)).await;
    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Cancelled
    );
    assert!(fx.script.posts().is_empty());
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dry_run_simulates_and_leaves_post_status(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::dry_run()).await[0];
    let post_status = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT p.status FROM posts p JOIN deliveries d ON d.post_id = p.id WHERE d.id = $1",
        )
        .bind(dispatch.delivery.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let before = post_status().await;

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Simulated
    );
    assert_eq!(post_status().await, before);
    let (outcome, summary): (Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT outcome, request_summary FROM delivery_attempts WHERE delivery_id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outcome.as_deref(), Some("simulated"));
    assert_eq!(summary.unwrap()["planned"], json!(["create the post"]));
    assert!(fx.script.posts().is_empty());
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn comment_post_goes_through_commenter(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    run(&fx.service, post).await;
    let answered = fx.script.posts()[0].remote_id.clone();
    let spec = JobSpec {
        text: "thanks!".to_owned(),
        comment_on: Some(answered.clone()),
        ..JobSpec::live()
    };
    let comment = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];

    run(&fx.service, comment).await;

    assert_eq!(
        state(&pool, comment.delivery).await,
        DeliveryState::Published
    );
    let comments = fx.script.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].post_remote_id, answered);
    assert_eq!(comments[0].text, "thanks!");
    assert_eq!(fx.script.posts().len(), 1, "no second post");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn container_ref_saved_before_next_step_and_reused_on_retry(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.set_publish_steps(3);
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("retryable_failure"),
    )
    .await[0];

    run(&fx.service, dispatch).await;
    assert_eq!(
        row(&pool, dispatch.delivery).await.container_ref.as_deref(),
        Some("chunks:3")
    );
    assert_eq!(fx.script.chunk_uploads(), 3);

    make_due(&pool, dispatch.delivery).await;
    run(
        &fx.service,
        Dispatch {
            dispatch_gen: 1,
            ..dispatch
        },
    )
    .await;

    assert_eq!(
        fx.script.seen_container_refs(),
        vec![None, Some("chunks:3".to_owned())]
    );
    assert_eq!(fx.script.chunk_uploads(), 3, "no chunk uploaded twice");
}

/// Review Focus 1a.
#[sqlx::test(migrations = "../data/migrations")]
async fn stalled_worker_late_success_is_recorded_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);

    // The worker stalls past its lease (five minutes; its renewer never ran), and the sweep
    // marks the delivery `outcome_unknown`.
    sqlx::query(
        "UPDATE deliveries SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let moved = Sweep::expired_in_flight(&mut conn, Utc::now(), 10)
        .await
        .unwrap();
    let moved: Vec<_> = moved.iter().map(|swept| swept.delivery).collect();
    assert_eq!(moved, vec![dispatch.delivery]);

    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    let posts = fx.script.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        row(&pool, dispatch.delivery).await.remote_id.as_deref(),
        Some(posts[0].remote_id.as_str())
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_published"]
    );
    assert_eq!(audits(&pool, "delivery_published").await, 1);

    // The reconcile job the sweep enqueued for the takeover finds the delivery published and
    // exits: no platform call, no write, no notice.
    let calls = fx.script.remote_calls();
    run_reconcile(
        &fx.service,
        ReconcileDelivery {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            attempt: 0,
        },
    )
    .await
    .unwrap();
    assert_eq!(fx.script.remote_calls(), calls);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_published"]
    );
    assert_eq!(audits(&pool, "delivery_published").await, 1);
}

/// Review Focus 1b.
#[sqlx::test(migrations = "../data/migrations")]
async fn late_success_after_reconcile_absent_publishes_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let stalled = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    take_over(&pool, dispatch).await;

    // Reconcile finds nothing (the held call has not landed) and re-queues the delivery.
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::apply(
        &mut conn,
        Fence {
            id: dispatch.delivery,
            dispatch_gen: 0,
            expected: DeliveryState::OutcomeUnknown,
        },
        DeliveryEvent::ReconcileAbsent,
        TransitionData::default(),
        Utc::now(),
    )
    .await
    .unwrap();
    gate.stop_holding();
    run(
        &fx.service,
        Dispatch {
            dispatch_gen: 1,
            ..dispatch
        },
    )
    .await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );

    // The held call lands with the same key: the platform returns the one post, and the
    // stalled worker's write is stale.
    gate.release();
    stalled.await.unwrap().unwrap();

    let posts = fx.script.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        row(&pool, dispatch.delivery).await.remote_id.as_deref(),
        Some(posts[0].remote_id.as_str())
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_published"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn late_processing_receipt_is_not_recorded_as_published(pool: PgPool) {
    let fx = fixture(&pool).await;
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("processing"),
    )
    .await[0];
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    take_over(&pool, dispatch).await;

    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert_eq!(row(&pool, dispatch.delivery).await.remote_id, None);
    assert!(queued(&pool, "poll_delivery_status").await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lost_lease_stops_at_next_checkpoint(pool: PgPool) {
    let fx = with_settings(&pool, short_lease()).await;
    fx.script.set_publish_steps(2);
    let gate = fx.script.hold_publishes();
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    take_over(&pool, dispatch).await;
    // The renewer runs every 200 ms and finds the lease gone.
    tokio::time::sleep(Duration::from_millis(700)).await;

    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert!(fx.script.posts().is_empty(), "stopped before the create");
    assert_eq!(last_error(&pool, dispatch.delivery).await, None);
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_apalis_job_publishes_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    let (a, b) = tokio::join!(
        run_publish(&fx.service, PublishDelivery::from(dispatch)),
        run_publish(&fx.service, PublishDelivery::from(dispatch)),
    );
    a.unwrap();
    b.unwrap();

    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_workers_publish_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    let runs: Vec<_> = (0..10)
        .map(|_| {
            let service = fx.service.clone();
            tokio::spawn(
                async move { run_publish(&service, PublishDelivery::from(dispatch)).await },
            )
        })
        .collect();
    for handle in runs {
        handle.await.unwrap().unwrap();
    }

    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(fx.script.mutating_calls(), 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn console_retry_of_every_publishing_type_publishes_nothing_twice(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(fx.script.posts().len(), 1);

    let every = "0 0 0 1 1 *".to_owned();
    let schedules = JobSchedules {
        job_history_purge: every.clone(),
        purge_pending_users: every.clone(),
        audit_retention: every.clone(),
        data_retention: every.clone(),
        media_gc: every.clone(),
        delegation_expiry: every.clone(),
        refresh_credentials: every.clone(),
        oauth_state_purge: every.clone(),
        delivery_due_sweep: every.clone(),
        account_preflight: every.clone(),
        notification_digest: every,
    };
    let mut registry = JobRegistry::default();
    register(&mut registry, fx.service.clone(), &schedules).unwrap();
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(postit_core::SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    // Each payload the delivery's life produced, enqueued again as a console retry would.
    let ids = [
        queue
            .enqueue(&PublishDelivery::from(dispatch))
            .await
            .unwrap(),
        queue
            .enqueue(&PollDeliveryStatus::from(dispatch))
            .await
            .unwrap(),
        queue
            .enqueue(&ReconcileDelivery::from(dispatch))
            .await
            .unwrap(),
        queue
            .enqueue(&DirectSendBatch::from(dispatch))
            .await
            .unwrap(),
    ];
    let done = wait_until_async(Duration::from_secs(30), || {
        let pool = pool.clone();
        let ids: Vec<uuid::Uuid> = ids.iter().map(|id| id.0).collect();
        async move {
            let finished: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM jobs WHERE id = ANY($1) AND state = 'succeeded'",
            )
            .bind(&ids)
            .fetch_one(&pool)
            .await
            .unwrap();
            finished == 4
        }
    })
    .await;
    worker.stop().await;

    assert!(done, "every re-enqueued job ran");
    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(fx.script.mutating_calls(), 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_after_edit_publishes_the_snapshot(pool: PgPool) {
    let fx = fixture(&pool).await;
    let spec = JobSpec {
        text: "as scheduled".to_owned(),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];
    // A first run uploads its one chunk, is stopped by a shutdown before the create, and is
    // sent back to pending.
    fx.script.set_publish_steps(1);
    let gate = fx.script.hold_publishes();
    let stopping = CancellationToken::new();
    let worker = {
        let service = other_service(&fx, publishing_settings(), stopping.clone());
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    stopping.cancel();
    gate.release();
    worker.await.unwrap().unwrap();
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    sqlx::query(
        "UPDATE posts SET text = 'edited later' WHERE id = \
         (SELECT post_id FROM deliveries WHERE id = $1)",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    make_due(&pool, dispatch.delivery).await;
    run(
        &fx.service,
        Dispatch {
            dispatch_gen: 1,
            ..dispatch
        },
    )
    .await;

    let posts = fx.script.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].text, "as scheduled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn partial_job_notifies(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let spec = JobSpec {
        outcomes: vec!["published", "permanent_failure"],
        ..JobSpec::live()
    };
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], spec).await;

    run(&fx.service, dispatches[0]).await;
    run(&fx.service, dispatches[1]).await;

    let all = notices(&pool, fx.env.owner).await;
    let partial: Vec<_> = all
        .iter()
        .filter_map(|n| match n {
            Notice::JobPartial(job) => Some(job),
            _ => None,
        })
        .collect();
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].published, 1);
    assert_eq!(partial[0].failed, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn partial_job_notifies_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let spec = JobSpec {
        outcomes: vec!["published", "permanent_failure"],
        ..JobSpec::live()
    };
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], spec).await;
    run(&fx.service, dispatches[0]).await;
    let gate = fx.script.hold_publishes();
    let worker = {
        let service = fx.service.clone();
        let dispatch = dispatches[1];
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    // While the call is out, the job comes to read `partial` (as if an earlier round had got
    // there), so the failure below is a `partial` to `partial` write.
    sqlx::query(
        "UPDATE publish_jobs SET status = 'partial' WHERE id =          (SELECT job_id FROM deliveries WHERE id = $1)",
    )
    .bind(dispatches[1].delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatches[1].delivery).await,
        DeliveryState::Failed
    );
    let all = notices(&pool, fx.env.owner).await;
    assert!(
        !all.iter().any(|n| matches!(n, Notice::JobPartial(_))),
        "{:?}",
        kinds(&all)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn quota_released_when_no_call_was_made(pool: PgPool) {
    let fx = fixture(&pool).await;
    // A rejected credential: the platform did not act.
    let rejected = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("needs_reconnect"),
    )
    .await[0];
    run(&fx.service, rejected).await;
    assert_eq!(account_units(&pool, fx.account).await, 0);

    // A run that stops before the first mutating call: the owner's cancel request was already
    // recorded when it claimed the delivery (a shutdown had sent it back to pending).
    let other = connect(&fx.accounts, &fx.env.scope, "other").await;
    let dispatch = create_job(&pool, fx.env.owner, &[other], JobSpec::live()).await[0];
    sqlx::query("UPDATE deliveries SET cancel_requested_at = now() WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    run(&fx.service, dispatch).await;

    assert_eq!(account_units(&pool, other).await, 0);
    assert_eq!(fx.script.mutating_calls(), 1, "only the rejected call");
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnected_account_fails_instead_of_looping(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    // An active account without a grant: `bound_client` reports it disconnected.
    sqlx::query("UPDATE connected_accounts SET grant_id = NULL WHERE id = $1")
        .bind(fx.account.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "account_disconnected"})
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn failed_fail_gives_quota_back(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            config: Some(json!({"quota": {"posts": 1}})),
            ..FixtureOptions::default()
        },
    )
    .await;
    let invalid = JobSpec {
        text: "x".repeat(600),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], invalid).await[0];
    // The failure's transaction rolls back at its notice.
    break_notifications(&pool).await;

    let result = run_publish(&fx.service, PublishDelivery::from(dispatch)).await;

    assert!(result.is_err());
    assert_eq!(account_units(&pool, fx.account).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn comment_without_commenter_fails_as_unsupported(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            without: vec![Capability::Commenter],
            ..FixtureOptions::default()
        },
    )
    .await;
    let spec = JobSpec {
        comment_on: Some("mock-post-1".to_owned()),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "capability_unsupported", "class": "permanent"})
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_failed"]
    );
    assert_eq!(fx.script.mutating_calls(), 0);
    assert_eq!(
        account_units(&pool, fx.account).await,
        0,
        "no call, no quota"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn failed_record_gives_quota_back_when_no_call_was_made(pool: PgPool) {
    let fx = fixture(&pool).await;
    // A rejected credential parks the delivery; the park's notice fails, rolling back the
    // whole write including the quota release.
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("needs_reconnect"),
    )
    .await[0];
    break_notifications(&pool).await;

    let result = run_publish(&fx.service, PublishDelivery::from(dispatch)).await;

    assert!(result.is_err());
    assert_eq!(account_units(&pool, fx.account).await, 0);
}
