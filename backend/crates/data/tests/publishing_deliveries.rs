#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, create_job, later, new_job, post, wait_for_blocked_statement, within};
use postit_core::{
    AccountId, DeliveryEvent, DeliveryId, DeliveryState, JobStatus, ParkReason, PostId,
    PublishJobId, ResolvedOutcome, UserId, transition,
};
use postit_data::posts::{PostContent, PostRepo};
use postit_data::publishing::deliveries::{
    Applied, AttemptRepo, CancelOutcome, DeliveryRepo, Fence, JobStatusChange, LeaseRenewal,
    Receipt, TransitionData,
};
use postit_data::publishing::{Dispatch, JobRepo};
use postit_data::testkit::seed_user;
use postit_data::{DataError, OwnerScope};
use sqlx::PgPool;
use uuid::Uuid;

const LEASE: Duration = Duration::seconds(60);

fn due() -> DateTime<Utc> {
    at(Utc::now() - Duration::seconds(30))
}

struct Fixture {
    owner: UserId,
    scope: OwnerScope,
    post: PostId,
    job: PublishJobId,
    deliveries: Vec<Dispatch>,
}

/// A live job that is already due, one delivery per account.
async fn fixture(pool: &PgPool, accounts: usize) -> Fixture {
    let owner = seed_user(pool, "active").await;
    let scope = OwnerScope::own(owner);
    let post = post(pool, owner, "hi").await;
    let mut ids: Vec<AccountId> = Vec::new();
    for n in 0..accounts {
        ids.push(account(pool, owner, &format!("acct{n}")).await);
    }
    let created = create_job(pool, &scope, post, &ids, due()).await;
    Fixture {
        owner,
        scope,
        post,
        job: created.job,
        deliveries: created.deliveries,
    }
}

async fn claim(pool: &PgPool, id: DeliveryId, generation: i32) -> bool {
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::claim(&mut conn, id, generation, Utc::now(), LEASE)
        .await
        .unwrap()
        .is_some()
}

async fn apply(
    pool: &PgPool,
    id: DeliveryId,
    generation: i32,
    expected: DeliveryState,
    event: DeliveryEvent,
    data: TransitionData,
) -> Result<Applied, DataError> {
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::apply(
        &mut conn,
        Fence {
            id,
            dispatch_gen: generation,
            expected,
        },
        event,
        data,
        Utc::now(),
    )
    .await
}

/// Claims `id` at generation 0 and starts the remote call.
async fn in_flight(pool: &PgPool, id: DeliveryId) {
    assert!(claim(pool, id, 0).await);
    let applied = apply(
        pool,
        id,
        0,
        DeliveryState::Claimed,
        DeliveryEvent::StartRemote,
        TransitionData::default(),
    )
    .await
    .unwrap();
    assert!(matches!(
        applied,
        Applied::Done {
            state: DeliveryState::InFlight,
            ..
        }
    ));
}

async fn state_of(pool: &PgPool, id: DeliveryId) -> (String, Option<String>, i32) {
    sqlx::query_as("SELECT state, parked_reason, dispatch_gen FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn job_version(pool: &PgPool, job: PublishJobId) -> i32 {
    sqlx::query_scalar("SELECT version FROM publish_jobs WHERE id = $1")
        .bind(job.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn post_status(pool: &PgPool, post: PostId) -> String {
    sqlx::query_scalar("SELECT status FROM posts WHERE id = $1")
        .bind(post.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn claim_once(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;

    // A holds the claim open; B is let loose on the same row and must wait for A's lock.
    let mut a = pool.begin().await.unwrap();
    let first = DeliveryRepo::claim(&mut a, id, 0, Utc::now(), LEASE)
        .await
        .unwrap();
    assert!(first.is_some());
    let second = tokio::spawn({
        let pool = pool.clone();
        async move { claim(&pool, id, 0).await }
    });
    within(wait_for_blocked_statement(&pool)).await;
    a.commit().await.unwrap();

    assert!(
        !within(second).await.unwrap(),
        "B rechecks the state after A's commit and finds it claimed"
    );
    assert_eq!(state_of(&pool, id).await.0, "claimed");
}

#[sqlx::test]
async fn claim_returns_what_the_worker_needs(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    let mut conn = pool.acquire().await.unwrap();

    let claimed = DeliveryRepo::claim(&mut conn, id, 0, Utc::now(), LEASE)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(claimed.id, id);
    assert_eq!(claimed.job_id, f.job);
    assert_eq!(claimed.post_id, f.post);
    assert_eq!(claimed.owner_id, f.owner);
    assert!(!claimed.dry_run);
    assert!(!claimed.native_schedule);
    assert_eq!(claimed.dispatch_gen, 0);
    assert_eq!(claimed.attempt_count, 0);
    assert_eq!(claimed.container_ref, None);
    assert!(!claimed.cancel_requested);
    let lease: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT lease_expires_at FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(lease.unwrap() > Utc::now());
}

#[sqlx::test]
async fn stale_gen_claim_finds_nothing(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    let retry_at = at(Utc::now() + Duration::seconds(10));

    apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::RetryableFailure,
        TransitionData {
            next_attempt_at: Some(retry_at),
            count_attempt: true,
            ..TransitionData::default()
        },
    )
    .await
    .unwrap();

    assert!(!claim(&pool, id, 0).await);
    assert!(!claim(&pool, id, 1).await, "not due yet");
    let mut conn = pool.acquire().await.unwrap();
    let claimed = DeliveryRepo::claim(&mut conn, id, 1, retry_at + Duration::seconds(1), LEASE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.attempt_count, 1);
}

#[sqlx::test]
async fn future_delivery_not_claimable_yet(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let created = create_job(&pool, &scope, p, &[a], later()).await;

    assert!(!claim(&pool, created.deliveries[0].delivery, 0).await);
}

#[sqlx::test]
async fn cancelled_job_not_claimable(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    sqlx::query("UPDATE publish_jobs SET cancelled_at = now() WHERE id = $1")
        .bind(f.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    assert!(!claim(&pool, f.deliveries[0].delivery, 0).await);
}

const STATES: [DeliveryState; 17] = [
    DeliveryState::Pending,
    DeliveryState::Claimed,
    DeliveryState::InFlight,
    DeliveryState::Processing,
    DeliveryState::OutcomeUnknown,
    DeliveryState::Parked(ParkReason::OwnerDisabled),
    DeliveryState::Parked(ParkReason::AccountNeedsReconnect),
    DeliveryState::Parked(ParkReason::PlatformUnavailable),
    DeliveryState::Parked(ParkReason::PlatformDisabled),
    DeliveryState::Parked(ParkReason::LivePublishDisabled),
    DeliveryState::Parked(ParkReason::QuotaExceeded),
    DeliveryState::Parked(ParkReason::OutcomeUnresolvable),
    DeliveryState::Published,
    DeliveryState::Failed,
    DeliveryState::DeadLetter,
    DeliveryState::Cancelled,
    DeliveryState::Simulated,
];

fn events() -> Vec<DeliveryEvent> {
    use DeliveryEvent as E;
    let mut all = vec![
        E::Claim,
        E::Cancel,
        E::StartRemote,
        E::RevalidationFailed,
        E::LeaseExpired,
        E::Published,
        E::Accepted,
        E::RetryableFailure,
        E::Shutdown,
        E::RetryBudgetExhausted,
        E::PermanentFailure,
        E::Ambiguous,
        E::Simulated,
        E::ProcessingTimeout,
        E::ReconcileFound,
        E::ReconcileAbsent,
        E::ReconcileUnresolvable,
        E::Unpark,
        E::UserRetry {
            acknowledged_outcome_unknown: false,
        },
        E::UserRetry {
            acknowledged_outcome_unknown: true,
        },
        E::UserResolve(ResolvedOutcome::Published),
        E::UserResolve(ResolvedOutcome::Failed),
    ];
    for reason in [
        ParkReason::OwnerDisabled,
        ParkReason::AccountNeedsReconnect,
        ParkReason::PlatformUnavailable,
        ParkReason::PlatformDisabled,
        ParkReason::LivePublishDisabled,
        ParkReason::QuotaExceeded,
        ParkReason::OutcomeUnresolvable,
    ] {
        all.push(E::Park(reason));
    }
    all
}

#[sqlx::test]
async fn apply_matches_core_table(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    for state in STATES {
        for event in events() {
            let reason = match state {
                DeliveryState::Parked(r) => Some(r.as_str()),
                _ => None,
            };
            sqlx::query(
                "UPDATE deliveries SET state = $2, parked_reason = $3, dispatch_gen = 0, \
                 lease_expires_at = NULL WHERE id = $1",
            )
            .bind(id.as_uuid())
            .bind(state.as_str())
            .bind(reason)
            .execute(&pool)
            .await
            .unwrap();

            let applied = apply(&pool, id, 0, state, event, TransitionData::default()).await;

            if let Ok(to) = transition(state, event) {
                let Applied::Done { state: got, .. } = applied.unwrap() else {
                    unreachable!("{state:?} + {event:?} was stale");
                };
                assert_eq!(got, to, "{state:?} + {event:?}");
                let (text, parked, _) = state_of(&pool, id).await;
                assert_eq!(text, to.as_str());
                assert_eq!(
                    parked.as_deref(),
                    match to {
                        DeliveryState::Parked(r) => Some(r.as_str()),
                        _ => None,
                    }
                );
            } else {
                assert!(
                    matches!(applied, Err(DataError::Conflict(_))),
                    "{state:?} + {event:?} should conflict"
                );
                let (text, parked, generation) = state_of(&pool, id).await;
                assert_eq!(
                    text,
                    state.as_str(),
                    "{state:?} + {event:?} changed the row"
                );
                assert_eq!(parked.as_deref(), reason);
                assert_eq!(generation, 0);
            }
        }
    }
}

#[sqlx::test]
async fn fenced_write_after_takeover_is_stale(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    sqlx::query(
        "UPDATE deliveries SET lease_expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let swept = apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::LeaseExpired,
        TransitionData::default(),
    )
    .await
    .unwrap();
    assert!(matches!(
        swept,
        Applied::Done {
            state: DeliveryState::OutcomeUnknown,
            ..
        }
    ));

    let late = apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::Published,
        TransitionData {
            remote_id: Some("r1".to_string()),
            ..TransitionData::default()
        },
    )
    .await
    .unwrap();

    assert!(matches!(late, Applied::Stale));
    let remote: Option<String> =
        sqlx::query_scalar("SELECT remote_id FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remote, None);
}

#[sqlx::test]
async fn late_receipt_publishes_once(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::LeaseExpired,
        TransitionData::default(),
    )
    .await
    .unwrap();
    let receipt = || Receipt {
        remote_id: "r1".to_string(),
        remote_url: Some("https://example.social/r1".to_string()),
    };
    let mut conn = pool.acquire().await.unwrap();

    assert!(
        DeliveryRepo::record_late_receipt(&mut conn, id, receipt(), Utc::now())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        DeliveryRepo::record_late_receipt(
            &mut conn,
            id,
            Receipt {
                remote_id: "other".to_string(),
                remote_url: None,
            },
            Utc::now()
        )
        .await
        .unwrap()
        .is_none()
    );

    let row: (String, Option<String>) =
        sqlx::query_as("SELECT state, remote_id FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, ("published".to_string(), Some("r1".to_string())));
    let job = JobRepo::get(&mut conn, &f.scope, f.job).await.unwrap();
    assert_eq!(job.status, JobStatus::Published);
}

#[sqlx::test]
async fn retry_redispatch_increments_gen_and_sets_time(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    let retry_at = at(Utc::now() + Duration::minutes(5));

    let applied = apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::RetryableFailure,
        TransitionData {
            next_attempt_at: Some(retry_at),
            count_attempt: true,
            last_error: Some(serde_json::json!({"code": "rate_limited"})),
            ..TransitionData::default()
        },
    )
    .await
    .unwrap();

    let Applied::Done {
        state, redispatch, ..
    } = applied
    else {
        unreachable!("stale");
    };
    assert_eq!(state, DeliveryState::Pending);
    assert_eq!(
        redispatch,
        Some(Dispatch {
            delivery: id,
            dispatch_gen: 1,
            run_at: retry_at,
        })
    );
    let row: (i32, Option<DateTime<Utc>>, i32) = sqlx::query_as(
        "SELECT attempt_count, lease_expires_at, dispatch_gen FROM deliveries WHERE id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row, (1, None, 1));
}

#[sqlx::test]
async fn every_apply_bumps_job_version(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    assert!(claim(&pool, id, 0).await);
    let mut last = job_version(&pool, f.job).await;

    for (expected, event) in [
        (DeliveryState::Claimed, DeliveryEvent::StartRemote),
        (DeliveryState::InFlight, DeliveryEvent::Published),
    ] {
        apply(&pool, id, 0, expected, event, TransitionData::default())
            .await
            .unwrap();
        let now = job_version(&pool, f.job).await;
        assert_eq!(now, last + 1, "{event:?}");
        last = now;
    }
}

#[sqlx::test]
async fn job_and_post_status_follow_deliveries(pool: PgPool) {
    let f = fixture(&pool, 2).await;
    let (first, second) = (f.deliveries[0].delivery, f.deliveries[1].delivery);
    assert_eq!(post_status(&pool, f.post).await, "publishing");
    in_flight(&pool, first).await;
    in_flight(&pool, second).await;

    apply(
        &pool,
        first,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::Published,
        TransitionData::default(),
    )
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        JobRepo::get(&mut conn, &f.scope, f.job)
            .await
            .unwrap()
            .status,
        JobStatus::Publishing
    );

    apply(
        &pool,
        second,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::PermanentFailure,
        TransitionData::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        JobRepo::get(&mut conn, &f.scope, f.job)
            .await
            .unwrap()
            .status,
        JobStatus::Partial
    );
    assert_eq!(post_status(&pool, f.post).await, "partial");

    let locked = PostRepo::lock(&mut conn, &f.scope, f.post, None)
        .await
        .unwrap();
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
    DeliveryRepo::refresh_job_status(&mut conn, f.job, Utc::now())
        .await
        .unwrap();
    assert_eq!(post_status(&pool, f.post).await, "draft");
}

#[sqlx::test]
async fn dry_run_job_never_touches_post_status(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let a = account(&pool, owner, "a").await;
    let p = post(&pool, owner, "hi").await;
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, p, None).await.unwrap();
    let mut job = new_job(p, 1, &[a], due());
    job.dry_run = true;
    let created = JobRepo::create(&mut tx, &scope, &locked, job)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let id = created.deliveries[0].delivery;
    assert_eq!(post_status(&pool, p).await, "draft");

    in_flight(&pool, id).await;
    apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::Simulated,
        TransitionData::default(),
    )
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        JobRepo::get(&mut conn, &scope, created.job)
            .await
            .unwrap()
            .status,
        JobStatus::Published
    );
    assert_eq!(post_status(&pool, p).await, "draft");
}

#[sqlx::test]
async fn renew_lease_only_for_current_gen(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 0, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Lost,
        "pending has no lease"
    );
    assert!(claim(&pool, id, 0).await);

    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 0, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Held {
            cancel_requested: false
        }
    );
    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 1, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Lost
    );
}

#[sqlx::test]
async fn renew_lease_reports_lost_and_cancel_requested(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 0, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Held {
            cancel_requested: false
        }
    );

    // The owner asks to stop while the post is on its way out.
    let outcome = DeliveryRepo::cancel(&mut conn, &f.scope, id, Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, CancelOutcome::Requested);
    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 0, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Held {
            cancel_requested: true
        }
    );

    // The sweep takes the delivery over: the lease is gone.
    apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::LeaseExpired,
        TransitionData::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        DeliveryRepo::renew_lease(&mut conn, id, 0, Utc::now(), LEASE)
            .await
            .unwrap(),
        LeaseRenewal::Lost
    );
}

#[sqlx::test]
async fn applied_reports_job_status_change(pool: PgPool) {
    let f = fixture(&pool, 2).await;
    let first = f.deliveries[0].delivery;
    let second = f.deliveries[1].delivery;
    in_flight(&pool, first).await;
    in_flight(&pool, second).await;

    let published = apply(
        &pool,
        first,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::Published,
        TransitionData::default(),
    )
    .await
    .unwrap();
    let Applied::Done { job, .. } = published else {
        unreachable!("stale");
    };
    assert_eq!(
        job,
        JobStatusChange {
            before: JobStatus::Publishing,
            after: JobStatus::Publishing,
        }
    );

    let failed = apply(
        &pool,
        second,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::PermanentFailure,
        TransitionData::default(),
    )
    .await
    .unwrap();
    let Applied::Done { job, .. } = failed else {
        unreachable!("stale");
    };
    assert_eq!(
        job,
        JobStatusChange {
            before: JobStatus::Publishing,
            after: JobStatus::Partial,
        }
    );
}

#[sqlx::test]
async fn late_receipt_reports_job_status_change(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    apply(
        &pool,
        id,
        0,
        DeliveryState::InFlight,
        DeliveryEvent::LeaseExpired,
        TransitionData::default(),
    )
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let change = DeliveryRepo::record_late_receipt(
        &mut conn,
        id,
        Receipt {
            remote_id: "r1".to_string(),
            remote_url: None,
        },
        Utc::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        change,
        Some(JobStatusChange {
            before: JobStatus::Publishing,
            after: JobStatus::Published,
        })
    );
}

#[sqlx::test]
async fn container_ref_is_fenced(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    in_flight(&pool, id).await;
    let fence = |generation, expected| Fence {
        id,
        dispatch_gen: generation,
        expected,
    };
    let mut conn = pool.acquire().await.unwrap();

    assert!(
        !DeliveryRepo::set_container_ref(&mut conn, fence(1, DeliveryState::InFlight), "c1")
            .await
            .unwrap()
    );
    assert!(
        !DeliveryRepo::set_container_ref(&mut conn, fence(0, DeliveryState::Claimed), "c1")
            .await
            .unwrap()
    );
    assert!(
        DeliveryRepo::set_container_ref(&mut conn, fence(0, DeliveryState::InFlight), "c1")
            .await
            .unwrap()
    );

    let stored: Option<String> =
        sqlx::query_scalar("SELECT container_ref FROM deliveries WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("c1"));
}

#[sqlx::test]
async fn attempts_start_and_finish(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let id = f.deliveries[0].delivery;
    let attempt = postit_core::DeliveryAttemptId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();

    AttemptRepo::start(&mut conn, attempt, id, Utc::now())
        .await
        .unwrap();
    AttemptRepo::finish(
        &mut conn,
        attempt,
        "retryable",
        Some(serde_json::json!({"code": "timeout"})),
        Some(serde_json::json!({"media": 0})),
        Utc::now(),
    )
    .await
    .unwrap();

    let row: (Uuid, Option<String>, bool) = sqlx::query_as(
        "SELECT owner_id, outcome, finished_at IS NOT NULL FROM delivery_attempts WHERE id = $1",
    )
    .bind(attempt.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (f.owner.as_uuid(), Some("retryable".to_string()), true)
    );
}

async fn account_of(pool: &PgPool, delivery: DeliveryId) -> AccountId {
    let id: Uuid = sqlx::query_scalar("SELECT account_id FROM deliveries WHERE id = $1")
        .bind(delivery.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    AccountId::from(id)
}

#[sqlx::test]
async fn in_flight_detects_claimed_delivery(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let d = &f.deliveries[0];
    let account = account_of(&pool, d.delivery).await;
    assert!(claim(&pool, d.delivery, d.dispatch_gen).await);

    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DeliveryRepo::account_has_in_flight(&mut conn, account)
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn in_flight_false_for_pending_only(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let account = account_of(&pool, f.deliveries[0].delivery).await;

    let mut conn = pool.acquire().await.unwrap();
    assert!(
        !DeliveryRepo::account_has_in_flight(&mut conn, account)
            .await
            .unwrap()
    );
}
