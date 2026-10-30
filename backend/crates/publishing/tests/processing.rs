//! Processing polls, processing timeouts, native schedules, and reconciliation (Task 9). Every
//! test runs the `poll_delivery_status` and `reconcile_delivery` handlers against the `mock`
//! agent, after the `publish_delivery` handler left the delivery `processing` or
//! `outcome_unknown`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use chrono::{DateTime, TimeDelta, Utc};
use common::{
    Fixture, FixtureOptions, JobSpec, audits, connect, create_job, fixture, fixture_with,
    last_error, native_job, notices, publishing_settings, queued, row, run, state,
};
use postit_config::PublishingSettings;
use postit_core::{DeliveryId, DeliveryState, Notice, ParkReason};
use postit_data::publishing::Dispatch;
use postit_publishing::backoff::{poll_delay, retry_ceiling};
use postit_publishing::poll::run_poll;
use postit_publishing::reconcile::run_reconcile;
use postit_publishing::{PollDeliveryStatus, ReconcileDelivery};
use serde_json::json;
use sqlx::PgPool;

fn kinds(all: &[Notice]) -> Vec<&'static str> {
    all.iter().map(|n| n.kind().as_str()).collect()
}

fn micros(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(time.timestamp_micros()).unwrap()
}

async fn poll(fx: &Fixture, dispatch: Dispatch, poll: i32) {
    run_poll(
        &fx.service,
        PollDeliveryStatus {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            poll,
        },
    )
    .await
    .unwrap();
}

async fn reconcile(fx: &Fixture, dispatch: Dispatch, attempt: i32) {
    run_reconcile(
        &fx.service,
        ReconcileDelivery {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            attempt,
        },
    )
    .await
    .unwrap();
}

/// A delivery the worker left `processing`.
async fn processing(fx: &Fixture) -> Dispatch {
    let pool = &fx.env.pool;
    let dispatch = create_job(
        pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("processing"),
    )
    .await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    dispatch
}

/// A delivery the worker left `outcome_unknown`.
async fn unknown(fx: &Fixture, spec: JobSpec) -> Dispatch {
    let pool = &fx.env.pool;
    let dispatch = create_job(pool, fx.env.owner, &[fx.account], spec).await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    dispatch
}

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

async fn check_columns(pool: &PgPool, id: DeliveryId) -> (i32, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT check_seq, check_due_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Moves the delivery's attempts `hours` into the past.
async fn backdate_attempts(pool: &PgPool, id: DeliveryId, hours: i32) {
    sqlx::query(
        "UPDATE delivery_attempts SET started_at = started_at - make_interval(hours => $2) \
         WHERE delivery_id = $1",
    )
    .bind(id.as_uuid())
    .bind(hours)
    .execute(pool)
    .await
    .unwrap();
}

/// Moves the delivery's last state change `hours` into the past.
async fn backdate_state_change(pool: &PgPool, id: DeliveryId, hours: i32) {
    sqlx::query(
        "UPDATE deliveries SET state_changed_at = state_changed_at - make_interval(hours => $2) WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(hours)
    .execute(pool)
    .await
    .unwrap();
}

/// Makes the delivery a natively scheduled upload that goes live at `at`.
async fn set_remote_publish_at(pool: &PgPool, id: DeliveryId, at: DateTime<Utc>) {
    sqlx::query("UPDATE deliveries SET remote_publish_at = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
}

fn poll_payload(dispatch: Dispatch, poll: i32) -> serde_json::Value {
    json!({
        "delivery": dispatch.delivery.as_uuid(),
        "dispatch_gen": dispatch.dispatch_gen,
        "poll": poll,
    })
}

fn reconcile_payload(dispatch: Dispatch, attempt: i32) -> serde_json::Value {
    json!({
        "delivery": dispatch.delivery.as_uuid(),
        "dispatch_gen": dispatch.dispatch_gen,
        "attempt": attempt,
    })
}

#[sqlx::test(migrations = "../data/migrations")]
async fn poll_publishes_when_platform_finishes(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = processing(&fx).await;

    poll(&fx, dispatch, 0).await;

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
    assert_eq!(
        queued(&pool, "poll_delivery_status").await.len(),
        1,
        "only the worker's first poll"
    );
    assert_eq!(check_columns(&pool, dispatch.delivery).await, (0, None));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn poll_backs_off_to_fifteen_minutes(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.keep_processing(true);
    let dispatch = processing(&fx).await;

    let mut waits = Vec::new();
    for k in 0..7 {
        let before = Utc::now();
        poll(&fx, dispatch, k).await;
        let polls = queued(&pool, "poll_delivery_status").await;
        let (payload, run_at) = polls.last().unwrap().clone();
        assert_eq!(payload, poll_payload(dispatch, k + 1));
        let wait = run_at - before;
        let want = TimeDelta::from_std(poll_delay(k + 1)).unwrap();
        assert!(
            wait >= want && wait <= want + TimeDelta::seconds(5),
            "poll {k}: {wait} vs {want}"
        );
        assert_eq!(
            check_columns(&pool, dispatch.delivery).await,
            (k + 1, Some(micros(run_at))),
            "the successor's time is the backstop's"
        );
        waits.push(want.num_seconds());
    }
    assert_eq!(waits, [30, 60, 120, 240, 480, 900, 900]);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn poll_permanent_failure_fails(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = processing(&fx).await;
    // The platform no longer knows the post.
    sqlx::query("UPDATE deliveries SET remote_id = 'gone' WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    poll(&fx, dispatch, 0).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_failed"]
    );
    let error = last_error(&pool, dispatch.delivery).await.unwrap();
    assert_eq!(error["class"], "permanent");
    assert_eq!(queued(&pool, "poll_delivery_status").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn processing_timeout_moves_to_outcome_unknown(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = processing(&fx).await;
    backdate_attempts(&pool, dispatch.delivery, 25).await;
    let calls = fx.script.remote_calls();

    poll(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert_eq!(fx.script.remote_calls(), calls, "no status call");
    let reconciles = queued(&pool, "reconcile_delivery").await;
    assert_eq!(reconciles.len(), 1);
    assert_eq!(reconciles[0].0, reconcile_payload(dispatch, 0));
    assert!(reconciles[0].1 <= Utc::now());
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_outcome_unknown"]
    );
    assert_eq!(audits(&pool, "delivery_outcome_unknown").await, 1);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "processing_timeout", "class": "ambiguous"})
    );
    let (seq, due) = check_columns(&pool, dispatch.delivery).await;
    assert_eq!(seq, 0);
    assert!(
        due.is_some_and(|due| due <= Utc::now()),
        "reconcile is due now"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_schedule_timeout_counts_from_remote_publish_at(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.keep_processing(true);
    let dispatch = processing(&fx).await;
    // The upload started 25 hours ago, but the platform was to publish it an hour ago: the
    // timeout counts from the publish time, so it has not run out.
    backdate_attempts(&pool, dispatch.delivery, 25).await;
    set_remote_publish_at(&pool, dispatch.delivery, Utc::now() - TimeDelta::hours(1)).await;

    poll(&fx, dispatch, 0).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(
        queued(&pool, "poll_delivery_status")
            .await
            .last()
            .unwrap()
            .0,
        poll_payload(dispatch, 1)
    );

    // Twenty-five hours past the publish time, it has.
    set_remote_publish_at(&pool, dispatch.delivery, Utc::now() - TimeDelta::hours(25)).await;
    poll(&fx, dispatch, 1).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert_eq!(queued(&pool, "reconcile_delivery").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_upload_waits_until_publish_time(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = processing(&fx).await;
    let live_at = micros(Utc::now() + TimeDelta::hours(2));
    set_remote_publish_at(&pool, dispatch.delivery, live_at).await;
    let calls = fx.script.remote_calls();

    poll(&fx, dispatch, 0).await;

    assert_eq!(fx.script.remote_calls(), calls, "nothing to ask yet");
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    let polls = queued(&pool, "poll_delivery_status").await;
    assert_eq!(polls.len(), 2);
    assert_eq!(polls[1].0, poll_payload(dispatch, 1));
    assert_eq!(polls[1].1, live_at);
    assert_eq!(
        check_columns(&pool, dispatch.delivery).await,
        (1, Some(live_at))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_poll_job_does_not_fork_the_chain(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.keep_processing(true);
    let dispatch = processing(&fx).await;
    let calls = fx.script.remote_calls();

    tokio::join!(poll(&fx, dispatch, 0), poll(&fx, dispatch, 0));

    assert_eq!(fx.script.remote_calls(), calls + 1, "one status call");
    let successors: Vec<_> = queued(&pool, "poll_delivery_status")
        .await
        .into_iter()
        .filter(|(payload, _)| *payload == poll_payload(dispatch, 1))
        .collect();
    assert_eq!(successors.len(), 1, "one successor");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_found_publishes(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_landed")).await;

    reconcile(&fx, dispatch, 0).await;

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
        ["delivery_published", "delivery_outcome_unknown"]
    );
    assert_eq!(audits(&pool, "delivery_published").await, 1);
    assert_eq!(check_columns(&pool, dispatch.delivery).await, (0, None));
}

/// The delivery's `next_attempt_at`.
async fn next_attempt_at(pool: &PgPool, delivery: DeliveryId) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT next_attempt_at FROM deliveries WHERE id = $1")
        .bind(delivery.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Skips the rest of a backoff: the delivery is due now.
async fn make_due(pool: &PgPool, delivery: DeliveryId) {
    sqlx::query("UPDATE deliveries SET next_attempt_at = now() WHERE id = $1")
        .bind(delivery.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_absent_redispatches_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_lost")).await;

    let asked = Utc::now();
    reconcile(&fx, dispatch, 0).await;
    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let stored = row(&pool, dispatch.delivery).await;
    assert_eq!(stored.dispatch_gen, 1);
    assert_eq!(
        stored.attempt_count, 1,
        "the ambiguous round counted already"
    );
    let publishes = queued(&pool, "publish_delivery").await;
    assert_eq!(publishes.len(), 1);
    assert_eq!(
        publishes[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 1})
    );
    // The re-run is due after the retry backoff for one attempt, and the job runs then.
    let ceiling = chrono::Duration::from_std(retry_ceiling(1)).unwrap();
    let due = next_attempt_at(&pool, dispatch.delivery).await;
    assert!(due >= asked && due <= Utc::now() + ceiling, "{due}");
    assert_eq!(publishes[0].1, due);
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_outcome_unknown"]
    );
    assert_eq!(check_columns(&pool, dispatch.delivery).await, (0, None));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_absent_within_budget_redispatches(pool: PgPool) {
    let fx = with_settings(
        &pool,
        PublishingSettings {
            max_attempts: 3,
            ..publishing_settings()
        },
    )
    .await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_lost")).await;
    sqlx::query("UPDATE deliveries SET attempt_count = 2 WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 2);
    assert_eq!(queued(&pool, "publish_delivery").await.len(), 1);
    assert_eq!(audits(&pool, "delivery_dead_lettered").await, 0);
}

/// Review Focus 10.
#[sqlx::test(migrations = "../data/migrations")]
async fn ambiguous_loop_ends_in_dead_letter(pool: PgPool) {
    let fx = with_settings(
        &pool,
        PublishingSettings {
            max_attempts: 3,
            ..publishing_settings()
        },
    )
    .await;
    let mut dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_lost"),
    )
    .await[0];

    for round in 1..=3 {
        run(&fx.service, dispatch).await;
        assert_eq!(
            state(&pool, dispatch.delivery).await,
            DeliveryState::OutcomeUnknown,
            "round {round}"
        );
        assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, round);
        let asked = Utc::now();
        reconcile(&fx, dispatch, 0).await;
        if round < 3 {
            assert_eq!(
                state(&pool, dispatch.delivery).await,
                DeliveryState::Pending
            );
            // The re-run waits the retry backoff instead of following at once.
            let due = next_attempt_at(&pool, dispatch.delivery).await;
            let ceiling = chrono::Duration::from_std(retry_ceiling(round)).unwrap();
            assert!(
                due >= asked && due <= Utc::now() + ceiling,
                "round {round}: {due}"
            );
            make_due(&pool, dispatch.delivery).await;
            dispatch = Dispatch {
                dispatch_gen: round,
                ..dispatch
            };
        }
    }

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::DeadLetter
    );
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(
        kinds(&all)
            .iter()
            .filter(|k| **k == "delivery_dead_lettered")
            .count(),
        1,
        "{:?}",
        kinds(&all)
    );
    assert_eq!(audits(&pool, "delivery_dead_lettered").await, 1);
    assert!(fx.script.posts().is_empty(), "nothing went out");
    assert_eq!(queued(&pool, "publish_delivery").await.len(), 2);
    assert_eq!(check_columns(&pool, dispatch.delivery).await, (0, None));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_unresolvable_parks_and_notifies(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(
        &fx,
        JobSpec {
            reconcile: Some("unresolvable"),
            ..JobSpec::outcome("outcome_unknown_landed")
        },
    )
    .await;

    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::OutcomeUnresolvable)
    );
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(kinds(&all), ["delivery_parked", "delivery_outcome_unknown"]);
    // Newest first.
    assert!(matches!(
        &all[0],
        Notice::DeliveryParked(n) if n.park_reason == Some(ParkReason::OutcomeUnresolvable)
    ));
    assert_eq!(queued(&pool, "reconcile_delivery").await.len(), 1);
    assert_eq!(check_columns(&pool, dispatch.delivery).await, (0, None));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_unsupported_parks(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(
        &fx,
        JobSpec {
            reconcile: Some("unsupported"),
            ..JobSpec::outcome("outcome_unknown_lost")
        },
    )
    .await;

    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::OutcomeUnresolvable)
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_parked", "delivery_outcome_unknown"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn comment_reconcile_uses_reconcile_comment(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    run(&fx.service, post).await;
    let remote_post = fx.script.posts()[0].remote_id.clone();
    let dispatch = unknown(
        &fx,
        JobSpec {
            comment_on: Some(remote_post),
            ..JobSpec::outcome("outcome_unknown_landed")
        },
    )
    .await;

    reconcile(&fx, dispatch, 0).await;

    // The post lookup knows nothing under the comment's key; only `reconcile_comment` finds it.
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    let comments = fx.script.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(
        row(&pool, dispatch.delivery).await.remote_id.as_deref(),
        Some(comments[0].remote_id.as_str())
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stale_reconcile_job_exits(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_landed")).await;
    let calls = fx.script.remote_calls();

    // Another generation.
    reconcile(
        &fx,
        Dispatch {
            dispatch_gen: 1,
            ..dispatch
        },
        0,
    )
    .await;
    assert_eq!(fx.script.remote_calls(), calls);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    assert_eq!(check_columns(&pool, dispatch.delivery).await.0, 0);

    // A delivery that is no longer `outcome_unknown`.
    let published = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    run(&fx.service, published).await;
    let calls = fx.script.remote_calls();
    reconcile(&fx, published, 0).await;
    assert_eq!(fx.script.remote_calls(), calls);
    assert_eq!(
        state(&pool, published.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_reconcile_job_does_not_fork_the_chain(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_lost")).await;
    fx.script.fail_reconcile(true);
    let calls = fx.script.remote_calls();
    let before = Utc::now();

    tokio::join!(reconcile(&fx, dispatch, 0), reconcile(&fx, dispatch, 0));

    assert_eq!(fx.script.remote_calls(), calls + 1, "one reconcile call");
    let successors: Vec<_> = queued(&pool, "reconcile_delivery")
        .await
        .into_iter()
        .filter(|(payload, _)| *payload == reconcile_payload(dispatch, 1))
        .collect();
    assert_eq!(successors.len(), 1, "one successor");
    let wait = successors[0].1 - before;
    let want = TimeDelta::from_std(poll_delay(1)).unwrap();
    assert!(
        wait >= want && wait <= want + TimeDelta::seconds(5),
        "{wait}"
    );
    assert_eq!(
        check_columns(&pool, dispatch.delivery).await,
        (1, Some(micros(successors[0].1)))
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_retries_until_processing_timeout_then_parks(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = unknown(&fx, JobSpec::outcome("outcome_unknown_lost")).await;
    fx.script.fail_reconcile(true);
    // The delivery entered `outcome_unknown` 25 hours ago.
    backdate_attempts(&pool, dispatch.delivery, 25).await;
    backdate_state_change(&pool, dispatch.delivery, 25).await;

    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::OutcomeUnresolvable)
    );
    assert_eq!(queued(&pool, "reconcile_delivery").await.len(), 1);
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_parked", "delivery_outcome_unknown"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_found_that_completes_a_partial_job_notifies_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let spec = JobSpec {
        outcomes: vec!["permanent_failure", "outcome_unknown_landed"],
        ..JobSpec::live()
    };
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], spec).await;
    run(&fx.service, dispatches[0]).await;
    run(&fx.service, dispatches[1]).await;
    assert_eq!(
        state(&pool, dispatches[1].delivery).await,
        DeliveryState::OutcomeUnknown
    );

    reconcile(&fx, dispatches[1], 0).await;
    reconcile(&fx, dispatches[1], 0).await;

    assert_eq!(
        state(&pool, dispatches[1].delivery).await,
        DeliveryState::Published
    );
    let all = notices(&pool, fx.env.owner).await;
    let partial: Vec<_> = all
        .iter()
        .filter_map(|n| match n {
            Notice::JobPartial(job) => Some(job),
            _ => None,
        })
        .collect();
    assert_eq!(partial.len(), 1, "{:?}", kinds(&all));
    assert_eq!(partial[0].published, 1);
    assert_eq!(partial[0].failed, 1);
}

/// A reconcile after a processing timeout counts its deadline from when the delivery entered
/// `outcome_unknown`, not from the upload's start a day earlier, so one transient failure
/// asks again instead of parking.
#[sqlx::test(migrations = "../data/migrations")]
async fn reconcile_after_processing_timeout_retries_a_transient_error(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = processing(&fx).await;
    backdate_attempts(&pool, dispatch.delivery, 25).await;
    poll(&fx, dispatch, 0).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    fx.script.fail_reconcile(true);

    reconcile(&fx, dispatch, 0).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    let reconciles = queued(&pool, "reconcile_delivery").await;
    assert_eq!(
        reconciles.last().unwrap().0,
        reconcile_payload(dispatch, 1),
        "asks again"
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_outcome_unknown"]
    );
}

/// An agent's `RetryAfter` wins over the computed delay, for the successor and its backstop.
#[sqlx::test(migrations = "../data/migrations")]
async fn retry_after_sets_the_successor_time(pool: PgPool) {
    let fx = fixture(&pool).await;
    let hour = TimeDelta::hours(1);
    fx.script
        .set_retry_after(Some(std::time::Duration::from_secs(3600)));

    let polled = processing(&fx).await;
    let before = Utc::now();
    poll(&fx, polled, 0).await;
    let (payload, run_at) = queued(&pool, "poll_delivery_status")
        .await
        .last()
        .unwrap()
        .clone();
    assert_eq!(payload, poll_payload(polled, 1));
    let wait = run_at - before;
    assert!(
        wait >= hour && wait <= hour + TimeDelta::seconds(5),
        "{wait}"
    );
    assert_eq!(
        check_columns(&pool, polled.delivery).await,
        (1, Some(micros(run_at)))
    );

    let reconciled = unknown(&fx, JobSpec::outcome("outcome_unknown_lost")).await;
    let before = Utc::now();
    reconcile(&fx, reconciled, 0).await;
    let (payload, run_at) = queued(&pool, "reconcile_delivery")
        .await
        .last()
        .unwrap()
        .clone();
    assert_eq!(payload, reconcile_payload(reconciled, 1));
    let wait = run_at - before;
    assert!(
        wait >= hour && wait <= hour + TimeDelta::seconds(5),
        "{wait}"
    );
    assert_eq!(
        check_columns(&pool, reconciled.delivery).await,
        (1, Some(micros(run_at)))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_target_without_native_scheduling_waits_for_the_job_time(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = micros(Utc::now() + TimeDelta::hours(2));
    let dispatch = native_job(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    // A newer build's agent no longer schedules on the platform.
    let without = common::service_without_native_scheduler(&fx);

    run(&without, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    let after = row(&pool, dispatch.delivery).await;
    assert_eq!(after.dispatch_gen, dispatch.dispatch_gen + 1);
    assert_eq!(after.next_attempt_at, Some(at));
    assert_eq!(after.remote_id, None);
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM delivery_attempts WHERE delivery_id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempts, 0, "nothing went to the platform");
    let next: Vec<_> = queued(&pool, "publish_delivery")
        .await
        .into_iter()
        .filter(|(payload, _)| payload["dispatch_gen"] == json!(after.dispatch_gen))
        .collect();
    assert_eq!(next.len(), 1);
    assert_eq!(micros(next[0].1), at);
}
