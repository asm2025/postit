#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{account, at, create_job, post, within};
use postit_core::{DeliveryEvent, DeliveryId, DeliveryState, ParkReason, PlatformId, UserId};
use postit_data::OwnerScope;
use postit_data::publishing::sweep::{StalledCheck, Sweep, UnparkInput};
use postit_data::publishing::{Applied, DeliveryRepo, Dispatch, Fence, TransitionData};
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

const GRACE: Duration = Duration::minutes(5);

fn ago(minutes: i64) -> DateTime<Utc> {
    at(Utc::now() - Duration::minutes(minutes))
}

/// A due, live job for a new owner with `accounts` deliveries. Returns the owner and deliveries.
async fn fixture(pool: &PgPool, accounts: usize) -> (UserId, Vec<DeliveryId>) {
    let owner = seed_user(pool, "active").await;
    let scope = OwnerScope::own(owner);
    let post = post(pool, owner, "hi").await;
    let mut ids = Vec::new();
    for n in 0..accounts {
        ids.push(account(pool, owner, &format!("acct{n}")).await);
    }
    let created = create_job(pool, &scope, post, &ids, ago(10)).await;
    let deliveries: Vec<DeliveryId> = created.deliveries.iter().map(|d| d.delivery).collect();
    // The rows were last touched when the job was due, not just now.
    sqlx::query("UPDATE deliveries SET updated_at = $1 WHERE owner_id = $2")
        .bind(ago(10))
        .bind(owner.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    (owner, deliveries)
}

/// Forces a delivery into `state` with the given columns, bypassing the transition table.
async fn force(
    pool: &PgPool,
    id: DeliveryId,
    state: &str,
    reason: Option<&str>,
    lease: Option<DateTime<Utc>>,
    due: Option<DateTime<Utc>>,
) {
    sqlx::query(
        "UPDATE deliveries SET state = $2, parked_reason = $3, lease_expires_at = $4, \
         next_attempt_at = $5 WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(state)
    .bind(reason)
    .bind(lease)
    .bind(due)
    .execute(pool)
    .await
    .unwrap();
}

async fn state(pool: &PgPool, id: DeliveryId) -> DeliveryState {
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::get(&mut conn, &OwnerScope::own(owner_of(pool, id).await), id)
        .await
        .unwrap()
        .state
}

async fn owner_of(pool: &PgPool, id: DeliveryId) -> UserId {
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    UserId::from_uuid(owner)
}

async fn generation(pool: &PgPool, id: DeliveryId) -> i32 {
    sqlx::query_scalar("SELECT dispatch_gen FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn platforms(ids: &[&str]) -> Vec<PlatformId> {
    ids.iter().map(|id| PlatformId::new(id).unwrap()).collect()
}

async fn unpark(
    pool: &PgPool,
    now: DateTime<Utc>,
    compiled: &[PlatformId],
) -> postit_data::publishing::sweep::UnparkResult {
    let mut conn = pool.acquire().await.unwrap();
    Sweep::unpark(
        &mut conn,
        UnparkInput {
            now,
            park_grace: GRACE,
            compiled_platforms: compiled,
            limit: 100,
        },
    )
    .await
    .unwrap()
}

#[sqlx::test]
async fn overdue_pending_redispatched_once(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 1).await;
    let id = deliveries[0];
    let before = generation(&pool, id).await;
    let now = Utc::now();
    // A sweeps and stays open; B sweeps while A holds the row and must skip it, not wait.
    let mut a = pool.begin().await.unwrap();
    let mut b = pool.begin().await.unwrap();
    let first = Sweep::overdue_pending(&mut a, now, GRACE, 100)
        .await
        .unwrap();
    let second = within(Sweep::overdue_pending(&mut b, now, GRACE, 100))
        .await
        .unwrap();
    a.commit().await.unwrap();
    b.rollback().await.unwrap();
    let all: Vec<_> = first.into_iter().chain(second).collect();
    assert_eq!(all.len(), 1, "one sweep wins the delivery");
    assert_eq!(all[0].delivery, id);
    assert_eq!(all[0].dispatch_gen, before + 1);
    assert_eq!(generation(&pool, id).await, before + 1);

    let mut conn = pool.acquire().await.unwrap();
    let again = Sweep::overdue_pending(&mut conn, now, GRACE, 100)
        .await
        .unwrap();
    assert!(
        again.is_empty(),
        "a delivery just re-dispatched is not overdue"
    );
}

#[sqlx::test]
async fn overdue_pending_skips_deliveries_not_yet_overdue(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 1).await;
    // Due 10 minutes ago and touched just now: inside the grace window.
    let mut conn = pool.acquire().await.unwrap();
    let swept = Sweep::overdue_pending(&mut conn, Utc::now() - Duration::minutes(8), GRACE, 100)
        .await
        .unwrap();
    assert!(swept.is_empty());
    assert_eq!(generation(&pool, deliveries[0]).await, 0);
}

#[sqlx::test]
async fn expired_claim_returns_to_pending(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 2).await;
    let (expired, live) = (deliveries[0], deliveries[1]);
    let now = Utc::now();
    force(&pool, expired, "claimed", None, Some(ago(1)), Some(ago(10))).await;
    force(
        &pool,
        live,
        "claimed",
        None,
        Some(at(now + Duration::minutes(1))),
        Some(ago(10)),
    )
    .await;

    let mut conn = pool.acquire().await.unwrap();
    let swept = Sweep::expired_claims(&mut conn, now, 100).await.unwrap();
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].delivery, expired);
    assert_eq!(swept[0].dispatch_gen, 1);
    assert_eq!(state(&pool, expired).await, DeliveryState::Pending);
    assert_eq!(state(&pool, live).await, DeliveryState::Claimed);
}

#[sqlx::test]
async fn expired_in_flight_becomes_outcome_unknown(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 2).await;
    let (expired, live) = (deliveries[0], deliveries[1]);
    let now = Utc::now();
    force(
        &pool,
        expired,
        "in_flight",
        None,
        Some(ago(1)),
        Some(ago(10)),
    )
    .await;
    force(
        &pool,
        live,
        "in_flight",
        None,
        Some(at(now + Duration::minutes(1))),
        Some(ago(10)),
    )
    .await;

    let mut conn = pool.acquire().await.unwrap();
    let swept = Sweep::expired_in_flight(&mut conn, now, 100).await.unwrap();
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].delivery, expired);
    assert_eq!(
        swept[0].dispatch(),
        Dispatch {
            delivery: expired,
            dispatch_gen: 0,
            run_at: now,
        },
        "the reconcile job carries the generation"
    );
    assert_eq!(swept[0].state, DeliveryState::OutcomeUnknown);
    assert_eq!(state(&pool, expired).await, DeliveryState::OutcomeUnknown);
    assert_eq!(state(&pool, live).await, DeliveryState::InFlight);
    let attempts: i32 = sqlx::query_scalar("SELECT attempt_count FROM deliveries WHERE id = $1")
        .bind(expired.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempts, 1, "a lease lost in flight counts an attempt");
}

/// Forces a delivery into `state` with its chain's backstop at `due` and its chain at `seq`.
async fn force_check(
    pool: &PgPool,
    id: DeliveryId,
    state: &str,
    operation: &str,
    seq: i32,
    due: DateTime<Utc>,
) {
    sqlx::query(
        "UPDATE deliveries SET state = $2, operation = $3, check_seq = $4, check_due_at = $5,          state_changed_at = $6 WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(state)
    .bind(operation)
    .bind(seq)
    .bind(due)
    .bind(ago(30))
    .execute(pool)
    .await
    .unwrap();
}

async fn state_changed_at(pool: &PgPool, id: DeliveryId) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT state_changed_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn stalled_checks_skips_direct_and_lists_each_once_per_grace(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 4).await;
    let (polled, direct, unknown, fresh) =
        (deliveries[0], deliveries[1], deliveries[2], deliveries[3]);
    force_check(&pool, polled, "processing", "publish", 3, ago(10)).await;
    force_check(&pool, direct, "processing", "direct", 0, ago(10)).await;
    force_check(&pool, unknown, "outcome_unknown", "publish", 1, ago(10)).await;
    // Due a minute ago: inside the grace, its job may only be late.
    force_check(&pool, fresh, "processing", "publish", 0, ago(1)).await;
    let changed = state_changed_at(&pool, polled).await;

    let now = Utc::now();
    let mut conn = pool.acquire().await.unwrap();
    let mut listed = Sweep::stalled_checks(&mut conn, now, GRACE, 100)
        .await
        .unwrap();
    listed.sort_by_key(|check| check.seq);
    assert_eq!(
        listed,
        vec![
            StalledCheck {
                delivery: unknown,
                dispatch_gen: 0,
                seq: 1,
                state: DeliveryState::OutcomeUnknown,
            },
            StalledCheck {
                delivery: polled,
                dispatch_gen: 0,
                seq: 3,
                state: DeliveryState::Processing,
            },
        ]
    );
    assert_eq!(
        state_changed_at(&pool, polled).await,
        changed,
        "the restart is not a state change"
    );

    let again = Sweep::stalled_checks(&mut conn, now, GRACE, 100)
        .await
        .unwrap();
    assert!(again.is_empty(), "stamped by the first listing");

    let later = now + GRACE + Duration::seconds(1);
    let mut listed: Vec<DeliveryId> = Sweep::stalled_checks(&mut conn, later, GRACE, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|check| check.delivery)
        .collect();
    listed.sort();
    let mut want = vec![polled, unknown, fresh];
    want.sort();
    assert_eq!(
        listed, want,
        "listed again once a whole grace has passed; the direct delivery never"
    );
}

#[sqlx::test]
async fn orphaned_direct_detected(pool: PgPool) {
    let (owner, deliveries) = fixture(&pool, 2).await;
    let (orphan, healthy) = (deliveries[0], deliveries[1]);
    let now = Utc::now();
    for id in [orphan, healthy] {
        sqlx::query(
            "UPDATE deliveries SET operation = 'direct', state = 'processing' WHERE id = $1",
        )
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    }
    // The orphan: queued rows, every lease expired or never taken. The healthy one: one live lease.
    let rows = [
        (orphan, Some(ago(2))),
        (orphan, None),
        (healthy, Some(ago(2))),
        (healthy, Some(at(now + Duration::minutes(1)))),
    ];
    for (delivery, lease) in rows {
        sqlx::query(
            "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash, lease_expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(owner.as_uuid())
        .bind(delivery.as_uuid())
        .bind(Uuid::now_v7().as_bytes().to_vec())
        .bind(lease)
        .execute(&pool)
        .await
        .unwrap();
    }

    let mut conn = pool.acquire().await.unwrap();
    let found = Sweep::orphaned_direct(&mut conn, now, GRACE, 100)
        .await
        .unwrap();
    assert_eq!(
        found,
        vec![Dispatch {
            delivery: orphan,
            dispatch_gen: generation(&pool, orphan).await,
            run_at: now,
        }],
        "the dispatch the next batch job carries"
    );
}

/// The deliveries `orphaned_direct` lists at `now`.
async fn orphans(conn: &mut sqlx::PgConnection, now: DateTime<Utc>) -> Vec<DeliveryId> {
    Sweep::orphaned_direct(conn, now, GRACE, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|dispatch| dispatch.delivery)
        .collect()
}

/// A `processing` direct delivery with one queued row and no lease, last touched `idle` minutes ago.
async fn orphan(pool: &PgPool, idle: i64) -> DeliveryId {
    let (owner, deliveries) = fixture(pool, 1).await;
    let id = deliveries[0];
    sqlx::query(
        "UPDATE deliveries SET operation = 'direct', state = 'processing', updated_at = $2 \n         WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(ago(idle))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(id.as_uuid())
    .bind(Uuid::now_v7().as_bytes().to_vec())
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test]
async fn orphaned_direct_skips_a_delivery_inside_the_grace(pool: PgPool) {
    let fresh = orphan(&pool, 1).await;
    let mut conn = pool.acquire().await.unwrap();
    let found = orphans(&mut conn, Utc::now()).await;
    assert!(
        found.is_empty(),
        "reached processing a minute ago: {found:?}"
    );

    sqlx::query("UPDATE deliveries SET updated_at = $2 WHERE id = $1")
        .bind(fresh.as_uuid())
        .bind(ago(6))
        .execute(&pool)
        .await
        .unwrap();
    let found = orphans(&mut conn, Utc::now()).await;
    assert_eq!(found, vec![fresh]);
}

#[sqlx::test]
async fn orphaned_direct_lists_once_per_grace(pool: PgPool) {
    let id = orphan(&pool, 10).await;
    let mut conn = pool.acquire().await.unwrap();
    let now = Utc::now();
    assert_eq!(orphans(&mut conn, now).await, vec![id]);
    assert!(
        orphans(&mut conn, now).await.is_empty(),
        "stamped by the first listing"
    );
    let later = now + GRACE + Duration::seconds(1);
    assert_eq!(orphans(&mut conn, later).await, vec![id]);
}

#[sqlx::test]
async fn two_sweeps_never_list_one_orphan_twice(pool: PgPool) {
    let id = orphan(&pool, 10).await;
    let now = Utc::now();
    let mut first = pool.begin().await.unwrap();
    let mut second = pool.begin().await.unwrap();
    let a = orphans(&mut first, now).await;
    let b = within(orphans(&mut second, now)).await;
    assert_eq!(a, vec![id]);
    assert!(b.is_empty(), "the second sweep skips the locked row");
    first.commit().await.unwrap();
    second.rollback().await.unwrap();
}

#[sqlx::test]
async fn orphaned_direct_ignores_deliveries_with_nothing_queued(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 1).await;
    sqlx::query("UPDATE deliveries SET operation = 'direct', state = 'processing' WHERE id = $1")
        .bind(deliveries[0].as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(orphans(&mut conn, Utc::now()).await.is_empty());
}

/// The six reasons a sweep can clear, each with the change that clears it.
#[sqlx::test]
async fn unpark_each_reason_when_cleared(pool: PgPool) {
    let (owner, deliveries) = fixture(&pool, 6).await;
    let now = Utc::now();
    let due = Some(at(now - Duration::minutes(1)));
    let reasons = [
        "owner_disabled",
        "account_needs_reconnect",
        "platform_unavailable",
        "platform_disabled",
        "live_publish_disabled",
        "quota_exceeded",
    ];
    for (id, reason) in deliveries.iter().zip(reasons) {
        force(&pool, *id, "parked", Some(reason), None, due).await;
    }
    // Every blocker is still in place except where noted below.
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE connected_accounts SET status = 'needs_reconnect' WHERE owner_id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE platforms SET enabled = FALSE WHERE id = 'mastodon'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('publishing.live', 'false') \
         ON CONFLICT (key) DO UPDATE SET value = 'false'",
    )
    .execute(&pool)
    .await
    .unwrap();
    // quota_exceeded waits until its bucket's next period.
    sqlx::query("UPDATE deliveries SET parked_until = $2 WHERE id = $1")
        .bind(deliveries[5].as_uuid())
        .bind(at(now + Duration::minutes(10)))
        .execute(&pool)
        .await
        .unwrap();

    let result = unpark(&pool, now, &[]).await;
    assert!(result.resumed.is_empty(), "nothing is cleared yet");
    assert!(result.needs_owner.is_empty());

    // Clear one blocker at a time and see exactly that delivery resume.
    let clears: [(usize, &str); 5] = [
        (0, "UPDATE users SET status = 'active' WHERE id = $1"),
        (
            1,
            "UPDATE connected_accounts SET status = 'active' WHERE owner_id = $1",
        ),
        (
            3,
            "UPDATE platforms SET enabled = TRUE WHERE id = 'mastodon' AND $1::uuid IS NOT NULL",
        ),
        (
            4,
            "UPDATE app_settings SET value = 'true' WHERE key = 'publishing.live' AND $1::uuid IS NOT NULL",
        ),
        (
            5,
            "UPDATE deliveries SET parked_until = now() - interval '1 minute' WHERE owner_id = $1 AND parked_reason = 'quota_exceeded'",
        ),
    ];
    for (index, sql) in clears {
        sqlx::query(sql)
            .bind(owner.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let result = unpark(&pool, Utc::now(), &[]).await;
        let resumed: Vec<DeliveryId> = result.resumed.iter().map(|d| d.delivery).collect();
        assert_eq!(
            resumed,
            vec![deliveries[index]],
            "reason {}",
            reasons[index]
        );
        assert_eq!(
            state(&pool, deliveries[index]).await,
            DeliveryState::Pending
        );
        assert_eq!(result.resumed[0].dispatch_gen, 1);
    }

    // platform_unavailable clears once the platform is compiled in.
    let result = unpark(&pool, Utc::now(), &platforms(&["mastodon"])).await;
    let resumed: Vec<DeliveryId> = result.resumed.iter().map(|d| d.delivery).collect();
    assert_eq!(resumed, vec![deliveries[2]]);
    assert_eq!(state(&pool, deliveries[2]).await, DeliveryState::Pending);
}

/// Review Focus 9: a quota park waits for `parked_until`, and `park_grace` still counts from
/// the delivery's due time.
#[sqlx::test]
async fn quota_park_resumes_at_parked_until_and_ages_from_due_time(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 2).await;
    let (soon, late) = (deliveries[0], deliveries[1]);
    let now = Utc::now();
    let due = at(now - Duration::minutes(1));
    let mut conn = pool.acquire().await.unwrap();
    for (id, until) in [
        (soon, at(now + Duration::minutes(2))),
        (late, at(now + Duration::minutes(10))),
    ] {
        force(
            &pool,
            id,
            "claimed",
            None,
            Some(at(now + Duration::minutes(5))),
            Some(due),
        )
        .await;
        let applied = DeliveryRepo::apply(
            &mut conn,
            Fence {
                id,
                dispatch_gen: 0,
                expected: DeliveryState::Claimed,
            },
            DeliveryEvent::Park(ParkReason::QuotaExceeded),
            TransitionData {
                parked_until: Some(until),
                ..TransitionData::default()
            },
            now,
        )
        .await
        .unwrap();
        assert!(matches!(applied, Applied::Done { .. }));
        let (parked_until, next_attempt_at) = park_times(&pool, id).await;
        assert_eq!(parked_until, Some(until));
        assert_eq!(next_attempt_at, Some(due), "the due time is untouched");
    }

    // Before either period rolls over, nothing resumes.
    let result = unpark(&pool, now, &[]).await;
    assert!(result.resumed.is_empty());
    assert!(result.needs_owner.is_empty());

    // `soon` clears 4 minutes after it was due, inside the 5-minute grace: it resumes.
    let result = unpark(&pool, now + Duration::minutes(3), &[]).await;
    let resumed: Vec<DeliveryId> = result.resumed.iter().map(|d| d.delivery).collect();
    assert_eq!(resumed, vec![soon]);
    assert_eq!(state(&pool, soon).await, DeliveryState::Pending);
    assert_eq!(
        park_times(&pool, soon).await.0,
        None,
        "leaving parked clears it"
    );

    // `late` clears 11 minutes after it was due: it stays parked and asks the owner once.
    let result = unpark(&pool, now + Duration::minutes(11), &[]).await;
    assert!(result.resumed.is_empty());
    assert_eq!(result.needs_owner.len(), 1);
    assert_eq!(result.needs_owner[0].1, late);
    assert_eq!(
        state(&pool, late).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
    let again = unpark(&pool, now + Duration::minutes(12), &[]).await;
    assert!(again.resumed.is_empty());
    assert!(again.needs_owner.is_empty());
}

async fn park_times(
    pool: &PgPool,
    id: DeliveryId,
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT parked_until, next_attempt_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn outcome_unresolvable_never_unparks(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 1).await;
    force(
        &pool,
        deliveries[0],
        "parked",
        Some("outcome_unresolvable"),
        None,
        Some(ago(1)),
    )
    .await;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('publishing.live', 'true') \
         ON CONFLICT (key) DO UPDATE SET value = 'true'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let result = unpark(
        &pool,
        Utc::now() + Duration::days(30),
        &platforms(&["mastodon"]),
    )
    .await;
    assert!(result.resumed.is_empty());
    assert!(result.needs_owner.is_empty());
    assert_eq!(
        state(&pool, deliveries[0]).await,
        DeliveryState::Parked(ParkReason::OutcomeUnresolvable)
    );
}

#[sqlx::test]
async fn stale_parked_notifies_owner_once(pool: PgPool) {
    let (owner, deliveries) = fixture(&pool, 1).await;
    // Parked for an owner who is active again, due an hour ago: cleared but too old to send.
    force(
        &pool,
        deliveries[0],
        "parked",
        Some("owner_disabled"),
        None,
        Some(ago(60)),
    )
    .await;

    let first = unpark(&pool, Utc::now(), &[]).await;
    assert!(first.resumed.is_empty());
    assert_eq!(first.needs_owner, vec![(owner, deliveries[0])]);
    assert_eq!(
        state(&pool, deliveries[0]).await,
        DeliveryState::Parked(ParkReason::OwnerDisabled)
    );

    let second = unpark(&pool, Utc::now(), &[]).await;
    assert!(second.resumed.is_empty());
    assert!(second.needs_owner.is_empty(), "the owner is told once");
}

#[sqlx::test]
async fn unpark_respects_platform_not_compiled(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 1).await;
    force(
        &pool,
        deliveries[0],
        "parked",
        Some("platform_unavailable"),
        None,
        Some(ago(1)),
    )
    .await;
    let result = unpark(&pool, Utc::now(), &platforms(&["bluesky"])).await;
    assert!(result.resumed.is_empty());
    assert_eq!(
        state(&pool, deliveries[0]).await,
        DeliveryState::Parked(ParkReason::PlatformUnavailable)
    );
}

#[sqlx::test]
async fn native_scheduled_counts_group_by_platform(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 2).await;
    sqlx::query("UPDATE publish_jobs SET native_schedule = TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE deliveries SET state = 'published', remote_id = 'r1', \
         remote_publish_at = now() + interval '1 day' WHERE id = $1",
    )
    .bind(deliveries[0].as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let counts = Sweep::native_scheduled_counts(&mut conn).await.unwrap();
    assert_eq!(counts, vec![(PlatformId::new("mastodon").unwrap(), 1)]);
}

#[sqlx::test]
async fn native_scheduled_count_for_account_counts_only_that_account(pool: PgPool) {
    let (_, deliveries) = fixture(&pool, 2).await;
    sqlx::query("UPDATE publish_jobs SET native_schedule = TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE deliveries SET state = 'published', remote_id = 'r1', \
         remote_publish_at = now() + interval '1 day'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let first: uuid::Uuid = sqlx::query_scalar("SELECT account_id FROM deliveries WHERE id = $1")
        .bind(deliveries[0].as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let count = Sweep::native_scheduled_count_for_account(&mut conn, first.into())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn orphaned_direct_skips_inactive_accounts(pool: PgPool) {
    let id = orphan(&pool, 10).await;
    sqlx::query(
        "UPDATE connected_accounts SET status = 'needs_reconnect' \
         WHERE id = (SELECT account_id FROM deliveries WHERE id = $1)",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        orphans(&mut conn, Utc::now()).await.is_empty(),
        "an account that cannot send is not handed a batch"
    );
    sqlx::query(
        "UPDATE connected_accounts SET status = 'active' \
         WHERE id = (SELECT account_id FROM deliveries WHERE id = $1)",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(orphans(&mut conn, Utc::now()).await, vec![id]);
}
