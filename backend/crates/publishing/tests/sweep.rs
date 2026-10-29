//! The delivery-due sweep (Task 10): every duty of `run_sweep`, against the `mock` agent, with
//! the jobs it enqueues read back from the ledger.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use chrono::{DateTime, Utc};
use common::{
    Fixture, JobSpec, audits, connect, create_job, fixture, notices, publishing_settings, queued,
    row, run, service_full, state,
};
use postit_core::{DeliveryId, DeliveryState, Notice, ParkReason};
use postit_data::publishing::Dispatch;
use postit_publishing::poll::run_poll;
use postit_publishing::reconcile::run_reconcile;
use postit_publishing::sweep::{SweepReport, run_sweep};
use postit_publishing::{PollDeliveryStatus, PublishingService, ReconcileDelivery};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn kinds(all: &[Notice]) -> Vec<&'static str> {
    all.iter().map(|n| n.kind().as_str()).collect()
}

fn count(all: &[Notice], kind: &str) -> usize {
    all.iter().filter(|n| n.kind().as_str() == kind).count()
}

async fn sweep(service: &PublishingService) -> SweepReport {
    run_sweep(service).await.unwrap()
}

fn publish_payload(delivery: DeliveryId, dispatch_gen: i32) -> Value {
    json!({"delivery": delivery.as_uuid(), "dispatch_gen": dispatch_gen})
}

/// The `publish_delivery` payloads for `delivery` in the ledger.
async fn publishes(pool: &PgPool, delivery: DeliveryId) -> Vec<Value> {
    queued(pool, "publish_delivery")
        .await
        .into_iter()
        .map(|(payload, _)| payload)
        .filter(|payload| payload["delivery"] == json!(delivery.as_uuid()))
        .collect()
}

/// Makes a pending delivery look like its queued job was lost `minutes` ago: due and untouched
/// since then.
async fn overdue(pool: &PgPool, id: DeliveryId, minutes: i32) {
    sqlx::query(
        "UPDATE deliveries SET next_attempt_at = now() - make_interval(mins => $2), \
         updated_at = now() - make_interval(mins => $2) WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(minutes)
    .execute(pool)
    .await
    .unwrap();
}

/// Forces a delivery into `state` under a lease that ran out a minute ago.
async fn lease_ran_out(pool: &PgPool, id: DeliveryId, state: &str) {
    sqlx::query(
        "UPDATE deliveries SET state = $2, lease_expires_at = now() - interval '1 minute' \
         WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(state)
    .execute(pool)
    .await
    .unwrap();
}

/// Forces a delivery to `parked` for `reason`, due `minutes_ago`.
async fn park(pool: &PgPool, id: DeliveryId, reason: ParkReason, minutes_ago: i32) {
    sqlx::query(
        "UPDATE deliveries SET state = 'parked', parked_reason = $2, \
         next_attempt_at = now() - make_interval(mins => $3), \
         parked_until = CASE WHEN $2 = 'quota_exceeded' \
                             THEN now() - interval '1 minute' END \
         WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(reason.as_str())
    .bind(minutes_ago)
    .execute(pool)
    .await
    .unwrap();
}

/// The six reasons a sweep can clear.
const CLEARABLE: [ParkReason; 6] = [
    ParkReason::OwnerDisabled,
    ParkReason::AccountNeedsReconnect,
    ParkReason::PlatformUnavailable,
    ParkReason::PlatformDisabled,
    ParkReason::LivePublishDisabled,
    ParkReason::QuotaExceeded,
];

/// One live job per call, each with one delivery to the fixture's account.
async fn deliveries(fx: &Fixture, n: usize) -> Vec<DeliveryId> {
    let mut ids = Vec::new();
    for _ in 0..n {
        ids.push(
            create_job(&fx.env.pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0]
                .delivery,
        );
    }
    ids
}

async fn check_columns(pool: &PgPool, id: DeliveryId) -> (i32, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT check_seq, check_due_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn state_changed_at(pool: &PgPool, id: DeliveryId) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT state_changed_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Deletes the queued job of `job_type` whose payload is `payload` from the ledger and the
/// outbox, as if the queue lost it.
async fn lose(pool: &PgPool, job_type: &str, payload: &Value) {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("DELETE FROM jobs WHERE job_type = $1 AND payload = $2 RETURNING id")
            .bind(job_type)
            .bind(payload)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(ids.len(), 1, "one {job_type} job {payload} to lose");
    sqlx::query("DELETE FROM job_outbox WHERE id = ANY($1)")
        .bind(&ids)
        .execute(pool)
        .await
        .unwrap();
}

/// Moves the chain's backstop past the sweep's grace.
async fn backstop_passed(pool: &PgPool, id: DeliveryId) {
    sqlx::query("UPDATE deliveries SET check_due_at = now() - interval '3 minutes' WHERE id = $1")
        .bind(id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

fn job_payloads(all: &[(Value, DateTime<Utc>)], delivery: DeliveryId) -> Vec<Value> {
    all.iter()
        .map(|(payload, _)| payload.clone())
        .filter(|payload| payload["delivery"] == json!(delivery.as_uuid()))
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn overdue_pending_is_requeued_with_new_generation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    overdue(&pool, dispatch.delivery, 10).await;

    let report = sweep(&fx.service).await;

    assert_eq!(report.requeued, 1);
    assert_eq!(row(&pool, dispatch.delivery).await.dispatch_gen, 1);
    assert_eq!(
        publishes(&pool, dispatch.delivery).await,
        [publish_payload(dispatch.delivery, 1)]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_claim_returns_to_pending_and_requeues(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    lease_ran_out(&pool, dispatch.delivery, "claimed").await;

    let report = sweep(&fx.service).await;

    assert_eq!(report.reclaimed, 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    assert_eq!(
        publishes(&pool, dispatch.delivery).await,
        [publish_payload(dispatch.delivery, 1)]
    );
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
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_in_flight_becomes_unknown_and_reconciles(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    lease_ran_out(&pool, dispatch.delivery, "in_flight").await;

    let report = sweep(&fx.service).await;

    assert_eq!(report.unknown, 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    let reconciles = queued(&pool, "reconcile_delivery").await;
    assert_eq!(
        job_payloads(&reconciles, dispatch.delivery),
        [json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 0, "attempt": 0})]
    );
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_outcome_unknown"]
    );
    assert_eq!(audits(&pool, "delivery_outcome_unknown").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_in_flight_reconcile_carries_the_generation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    sqlx::query("UPDATE deliveries SET dispatch_gen = 3 WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    lease_ran_out(&pool, dispatch.delivery, "in_flight").await;

    sweep(&fx.service).await;

    let reconciles = queued(&pool, "reconcile_delivery").await;
    let payloads = job_payloads(&reconciles, dispatch.delivery);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["dispatch_gen"], 3);
    // The job the sweep enqueued is the one the delivery's chain admits.
    let job: ReconcileDelivery = serde_json::from_value(payloads[0].clone()).unwrap();
    run_reconcile(&fx.service, job).await.unwrap();
    assert_ne!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown,
        "the reconcile round settled it"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_in_flight_counts_an_attempt(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    lease_ran_out(&pool, dispatch.delivery, "in_flight").await;
    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 0);

    sweep(&fx.service).await;

    assert_eq!(row(&pool, dispatch.delivery).await.attempt_count, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn orphaned_direct_batch_is_requeued_once_per_grace(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    sqlx::query(
        "UPDATE deliveries SET operation = 'direct', state = 'processing', \
         updated_at = now() - interval '10 minutes', next_attempt_at = NULL WHERE id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(fx.env.owner.as_uuid())
    .bind(dispatch.delivery.as_uuid())
    .bind(Uuid::now_v7().as_bytes().to_vec())
    .execute(&pool)
    .await
    .unwrap();

    let first = sweep(&fx.service).await;
    let second = sweep(&fx.service).await;

    assert_eq!(first.orphaned_direct, 1);
    assert_eq!(second.orphaned_direct, 0, "listed once per grace");
    assert_eq!(second.restarted, 0, "a direct delivery has no poll chain");
    assert_eq!(
        job_payloads(&queued(&pool, "direct_send_batch").await, dispatch.delivery),
        [json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 0})]
    );
}

/// Review Focus 2: for every reason a sweep clears, a delivery cleared inside `park_grace` of
/// its due time resumes; one cleared outside it stays parked and asks the owner once.
#[sqlx::test(migrations = "../data/migrations")]
async fn unpark_respects_park_grace_for_every_reason(pool: PgPool) {
    let fx = fixture(&pool).await;
    let ids = deliveries(&fx, CLEARABLE.len() * 2).await;
    let (inside, outside) = ids.split_at(CLEARABLE.len());
    for (reason, (fresh, old)) in CLEARABLE.iter().zip(inside.iter().zip(outside)) {
        park(&pool, *fresh, *reason, 1).await;
        park(&pool, *old, *reason, 30).await;
    }

    let first = sweep(&fx.service).await;
    let second = sweep(&fx.service).await;

    assert_eq!(first.unparked, CLEARABLE.len());
    assert_eq!(first.stale, CLEARABLE.len());
    assert_eq!(second.unparked, 0);
    assert_eq!(second.stale, 0, "the owner is asked once");
    for (reason, (fresh, old)) in CLEARABLE.iter().zip(inside.iter().zip(outside)) {
        assert_eq!(
            state(&pool, *fresh).await,
            DeliveryState::Pending,
            "{reason:?} inside the grace resumes"
        );
        assert_eq!(
            publishes(&pool, *fresh).await,
            [publish_payload(*fresh, 1)],
            "{reason:?}"
        );
        assert_eq!(
            state(&pool, *old).await,
            DeliveryState::Parked(*reason),
            "{reason:?} outside the grace stays parked"
        );
        assert!(publishes(&pool, *old).await.is_empty(), "{reason:?}");
    }
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(count(&all, "delivery_stale"), CLEARABLE.len());
    assert_eq!(
        all.len(),
        CLEARABLE.len(),
        "nothing else: {:?}",
        kinds(&all)
    );
    let mut reasons: Vec<_> = all
        .iter()
        .map(|notice| match notice {
            Notice::DeliveryStale(n) => n.park_reason.unwrap(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    reasons.sort_by_key(ParkReason::as_str);
    let mut want = CLEARABLE.to_vec();
    want.sort_by_key(ParkReason::as_str);
    assert_eq!(reasons, want);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn outcome_unresolvable_never_unparks(pool: PgPool) {
    let fx = fixture(&pool).await;
    let ids = deliveries(&fx, 2).await;
    park(&pool, ids[0], ParkReason::OutcomeUnresolvable, 1).await;
    park(&pool, ids[1], ParkReason::OutcomeUnresolvable, 30).await;

    let first = sweep(&fx.service).await;
    let second = sweep(&fx.service).await;

    assert_eq!(first, SweepReport::default());
    assert_eq!(second, SweepReport::default());
    for id in ids {
        assert_eq!(
            state(&pool, id).await,
            DeliveryState::Parked(ParkReason::OutcomeUnresolvable)
        );
        assert!(publishes(&pool, id).await.is_empty());
    }
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

/// Review Focus 9: while a park's cause is still there, sweep after sweep leaves the delivery
/// parked, records nothing, and enqueues nothing, inside the grace or past it.
#[sqlx::test(migrations = "../data/migrations")]
async fn parked_delivery_stays_parked_while_its_cause_persists(pool: PgPool) {
    let fx = fixture(&pool).await;
    let ids = deliveries(&fx, CLEARABLE.len() * 2).await;
    let (inside, outside) = ids.split_at(CLEARABLE.len());
    for (reason, (fresh, old)) in CLEARABLE.iter().zip(inside.iter().zip(outside)) {
        park(&pool, *fresh, *reason, 1).await;
        park(&pool, *old, *reason, 30).await;
    }
    // Every cause stays in place.
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(fx.env.owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE connected_accounts SET status = 'needs_reconnect' WHERE id = $1")
        .bind(fx.account.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE platforms SET enabled = FALSE WHERE id = 'mock'")
        .execute(&pool)
        .await
        .unwrap();
    common::set_live(&pool, false).await;
    sqlx::query(
        "UPDATE deliveries SET parked_until = now() + interval '1 hour' \
         WHERE parked_reason = 'quota_exceeded'",
    )
    .execute(&pool)
    .await
    .unwrap();
    // A build without the mock agent.
    let service = service_full(
        &fx.env,
        fx.accounts.clone(),
        Vec::new(),
        publishing_settings(),
        CancellationToken::new(),
    );

    let first = sweep(&service).await;
    let second = sweep(&service).await;

    assert_eq!(first, SweepReport::default());
    assert_eq!(second, SweepReport::default());
    for (reason, (fresh, old)) in CLEARABLE.iter().zip(inside.iter().zip(outside)) {
        for id in [*fresh, *old] {
            assert_eq!(state(&pool, id).await, DeliveryState::Parked(*reason));
        }
    }
    assert!(queued(&pool, "publish_delivery").await.is_empty());
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

/// Review Focus 9: a quota park whose `parked_until` comes more than `park_grace` after the
/// delivery was due stays parked and asks the owner once.
#[sqlx::test(migrations = "../data/migrations")]
async fn quota_park_cleared_past_grace_asks_the_owner(pool: PgPool) {
    let fx = fixture(&pool).await;
    let id = deliveries(&fx, 1).await[0];
    // Due 20 minutes ago; the bucket's period rolled over a minute ago.
    park(&pool, id, ParkReason::QuotaExceeded, 20).await;

    let first = sweep(&fx.service).await;
    let second = sweep(&fx.service).await;

    assert_eq!(first.stale, 1);
    assert_eq!(first.unparked, 0);
    assert_eq!(second, SweepReport::default());
    assert_eq!(
        state(&pool, id).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
    assert!(publishes(&pool, id).await.is_empty());
    assert_eq!(
        kinds(&notices(&pool, fx.env.owner).await),
        ["delivery_stale"]
    );
}

/// Review Focus 10: a poll chain, and separately a reconcile chain, whose next job vanished
/// after `take_check` is restarted once from `check_due_at`; the chain continues, a second
/// sweep inside the grace enqueues nothing, and the late original job and the sweep's job run
/// the step once between them.
#[sqlx::test(migrations = "../data/migrations")]
async fn stalled_check_chain_is_restarted_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    fx.script.keep_processing(true);

    // The poll chain.
    let polled = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("processing"),
    )
    .await[0];
    run(&fx.service, polled).await;
    assert_eq!(
        state(&pool, polled.delivery).await,
        DeliveryState::Processing
    );
    let poll = |n: i32| PollDeliveryStatus {
        delivery: polled.delivery.as_uuid(),
        dispatch_gen: polled.dispatch_gen,
        poll: n,
    };
    run_poll(&fx.service, poll(0)).await.unwrap();
    assert_eq!(check_columns(&pool, polled.delivery).await.0, 1);
    lose(
        &pool,
        "poll_delivery_status",
        &serde_json::to_value(poll(1)).unwrap(),
    )
    .await;
    backstop_passed(&pool, polled.delivery).await;
    let changed = state_changed_at(&pool, polled.delivery).await;

    // The reconcile chain.
    let unknown = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_lost"),
    )
    .await[0];
    run(&fx.service, unknown).await;
    assert_eq!(
        state(&pool, unknown.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    fx.script.fail_reconcile(true);
    let round = |n: i32| ReconcileDelivery {
        delivery: unknown.delivery.as_uuid(),
        dispatch_gen: unknown.dispatch_gen,
        attempt: n,
    };
    run_reconcile(&fx.service, round(0)).await.unwrap();
    assert_eq!(check_columns(&pool, unknown.delivery).await.0, 1);
    lose(
        &pool,
        "reconcile_delivery",
        &serde_json::to_value(round(1)).unwrap(),
    )
    .await;
    backstop_passed(&pool, unknown.delivery).await;

    let report = sweep(&fx.service).await;
    assert_eq!(report.restarted, 2);
    assert_eq!(
        state_changed_at(&pool, polled.delivery).await,
        changed,
        "a restart is not a state change"
    );
    let polls = job_payloads(
        &queued(&pool, "poll_delivery_status").await,
        polled.delivery,
    );
    assert_eq!(
        polls
            .iter()
            .filter(|p| **p == serde_json::to_value(poll(1)).unwrap())
            .count(),
        1,
        "the sweep enqueued step 1 once"
    );
    let rounds = job_payloads(&queued(&pool, "reconcile_delivery").await, unknown.delivery);
    assert_eq!(
        rounds
            .iter()
            .filter(|p| **p == serde_json::to_value(round(1)).unwrap())
            .count(),
        1
    );

    // A second sweep inside the grace enqueues nothing.
    let again = sweep(&fx.service).await;
    assert_eq!(again.restarted, 0);

    // The sweep's jobs run and the chains continue.
    let calls = fx.script.remote_calls();
    run_poll(&fx.service, poll(1)).await.unwrap();
    run_reconcile(&fx.service, round(1)).await.unwrap();
    assert_eq!(fx.script.remote_calls(), calls + 2, "one call per chain");
    assert_eq!(check_columns(&pool, polled.delivery).await.0, 2);
    assert_eq!(check_columns(&pool, unknown.delivery).await.0, 2);
    assert!(
        job_payloads(
            &queued(&pool, "poll_delivery_status").await,
            polled.delivery
        )
        .contains(&serde_json::to_value(poll(2)).unwrap()),
        "the poll chain goes on"
    );
    assert!(
        job_payloads(&queued(&pool, "reconcile_delivery").await, unknown.delivery)
            .contains(&serde_json::to_value(round(2)).unwrap()),
        "the reconcile chain goes on"
    );

    // The original jobs were only late: they run now and do nothing.
    run_poll(&fx.service, poll(1)).await.unwrap();
    run_reconcile(&fx.service, round(1)).await.unwrap();
    assert_eq!(
        fx.script.remote_calls(),
        calls + 2,
        "no second call for a step"
    );
    assert_eq!(check_columns(&pool, polled.delivery).await.0, 2);
    assert_eq!(check_columns(&pool, unknown.delivery).await.0, 2);
}

/// A pending delivery whose queued job was lost is published late, once.
#[sqlx::test(migrations = "../data/migrations")]
async fn lost_queue_entry_delays_but_never_duplicates(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    overdue(&pool, dispatch.delivery, 10).await;

    let first = sweep(&fx.service).await;
    let second = sweep(&fx.service).await;
    assert_eq!(first.requeued, 1);
    assert_eq!(
        second.requeued, 0,
        "a delivery just re-dispatched is not overdue"
    );
    assert_eq!(
        publishes(&pool, dispatch.delivery).await,
        [publish_payload(dispatch.delivery, 1)]
    );

    // The lost job turns up after all: its generation is retired.
    run(&fx.service, dispatch).await;
    assert!(fx.script.posts().is_empty());
    let current = Dispatch {
        dispatch_gen: 1,
        ..dispatch
    };
    run(&fx.service, current).await;
    run(&fx.service, current).await;
    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

/// A sweep write that ends a job's last open delivery and makes a live job `partial` records
/// `job_partial` once, through the worker's notice path.
#[sqlx::test(migrations = "../data/migrations")]
async fn sweep_that_makes_a_job_partial_notifies_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], JobSpec::live()).await;
    run(&fx.service, dispatches[0]).await;
    assert_eq!(
        state(&pool, dispatches[0].delivery).await,
        DeliveryState::Published
    );
    // The other delivery's claim ran out after its job was cancelled.
    lease_ran_out(&pool, dispatches[1].delivery, "claimed").await;
    sqlx::query("UPDATE publish_jobs SET cancelled_at = now() WHERE id = (SELECT job_id FROM deliveries WHERE id = $1)")
        .bind(dispatches[1].delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let first = sweep(&fx.service).await;
    let again = sweep(&fx.service).await;

    assert_eq!(
        first.reclaimed, 0,
        "a cancelled job's delivery is not requeued"
    );
    assert_eq!(again, SweepReport::default());
    assert_eq!(
        state(&pool, dispatches[1].delivery).await,
        DeliveryState::Cancelled
    );
    assert!(publishes(&pool, dispatches[1].delivery).await.is_empty());
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(count(&all, "job_partial"), 1, "{:?}", kinds(&all));
}
