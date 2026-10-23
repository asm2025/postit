#![allow(clippy::unwrap_used)]

mod common;

use chrono::{Duration, NaiveDate, Utc};
use common::{account, at, create_job, post, wait_for_blocked_statement, within};
use postit_core::{
    AccountId, ContactListId, DeliveryEvent, DeliveryId, DeliveryState, DirectBatchId,
    DirectSendId, PlatformId, UserId,
};
use postit_data::OwnerScope;
use postit_data::publishing::deliveries::{Applied, ClaimedDelivery, Fence, TransitionData};
use postit_data::publishing::direct::{Audience, BatchClaim, DirectRepo, DirectResult};
use postit_data::publishing::quota::{AccountQuotaKey, EMAIL_CAP_BUCKET};
use postit_data::publishing::sweep::Sweep;
use postit_data::publishing::{CancelOutcome, DeliveryRepo, JobRepo};
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

const LEASE: Duration = Duration::minutes(5);

struct Fixture {
    owner: UserId,
    account: AccountId,
    delivery: ClaimedDelivery,
}

impl Fixture {
    /// The fence of a worker holding the delivery in `processing`.
    fn fence(&self) -> Fence {
        Fence {
            id: self.delivery.id,
            dispatch_gen: self.delivery.dispatch_gen,
            expected: DeliveryState::Processing,
        }
    }
}

/// A `processing` `direct` delivery for a new owner.
async fn fixture(pool: &PgPool) -> Fixture {
    let owner = seed_user(pool, "active").await;
    let scope = OwnerScope::own(owner);
    let post = post(pool, owner, "hi").await;
    let account = account(pool, owner, "a").await;
    let created = create_job(
        pool,
        &scope,
        post,
        &[account],
        at(Utc::now() - Duration::minutes(1)),
    )
    .await;
    let dispatch = created.deliveries[0];
    sqlx::query("UPDATE deliveries SET operation = 'direct' WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let delivery = DeliveryRepo::claim(
        &mut conn,
        dispatch.delivery,
        dispatch.dispatch_gen,
        Utc::now(),
        LEASE,
    )
    .await
    .unwrap()
    .unwrap();
    for event in [DeliveryEvent::StartRemote, DeliveryEvent::Accepted] {
        let fence = Fence {
            id: delivery.id,
            dispatch_gen: delivery.dispatch_gen,
            expected: if event == DeliveryEvent::StartRemote {
                DeliveryState::Claimed
            } else {
                DeliveryState::InFlight
            },
        };
        DeliveryRepo::apply(
            &mut conn,
            fence,
            event,
            TransitionData::default(),
            Utc::now(),
        )
        .await
        .unwrap();
    }
    Fixture {
        owner,
        account,
        delivery,
    }
}

/// A stand-in address hash: the address bytes padded to the 32 bytes a real hash has.
fn hash(email: &str) -> Vec<u8> {
    let mut bytes = email.as_bytes().to_vec();
    bytes.resize(32, 0);
    bytes
}

async fn list(pool: &PgPool, owner: UserId) -> ContactListId {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO contact_lists (id, owner_id, name) VALUES ($1, $2, 'list')")
        .bind(id)
        .bind(owner.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    ContactListId::from_uuid(id)
}

/// A contact on `lists`. The address hash is a padded copy of the address, which is enough for these tests.
async fn contact(
    pool: &PgPool,
    owner: UserId,
    email: &str,
    status: &str,
    tags: &[&str],
    lists: &[ContactListId],
) -> Uuid {
    let id = Uuid::now_v7();
    let tags: Vec<String> = tags.iter().map(|tag| (*tag).to_string()).collect();
    sqlx::query(
        "INSERT INTO contacts (id, owner_id, email, email_hash, status, tags) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(email)
    .bind(hash(email))
    .bind(status)
    .bind(tags)
    .execute(pool)
    .await
    .unwrap();
    for list in lists {
        sqlx::query(
            "INSERT INTO contact_list_members (list_id, contact_id, owner_id) VALUES ($1, $2, $3)",
        )
        .bind(list.as_uuid())
        .bind(id)
        .bind(owner.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    }
    id
}

async fn suppress(pool: &PgPool, owner: UserId, email: &str) {
    sqlx::query(
        "INSERT INTO suppressions (id, owner_id, email_hash, reason) VALUES ($1, $2, $3, 'manual')",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(hash(email))
    .execute(pool)
    .await
    .unwrap();
}

/// `count` subscribed contacts on one list, materialized.
async fn queue(pool: &PgPool, fx: &Fixture, count: usize) {
    let list = list(pool, fx.owner).await;
    for n in 0..count {
        contact(
            pool,
            fx.owner,
            &format!("c{n}@example.com"),
            "subscribed",
            &[],
            &[list],
        )
        .await;
    }
    let mut conn = pool.acquire().await.unwrap();
    let audience = Audience {
        lists: vec![list],
        tags: vec![],
    };
    let size = DirectRepo::materialize(&mut conn, &fx.delivery, &audience, Utc::now())
        .await
        .unwrap();
    assert_eq!(size, i64::try_from(count).unwrap());
}

fn cap_key(fx: &Fixture) -> AccountQuotaKey {
    AccountQuotaKey {
        owner: fx.owner,
        account: fx.account,
        platform: PlatformId::new("mastodon").unwrap(),
        bucket: EMAIL_CAP_BUCKET.to_string(),
        period_start: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
    }
}

async fn claim(
    pool: &PgPool,
    fx: &Fixture,
    size: i64,
    cap: Option<(&AccountQuotaKey, i64)>,
) -> BatchClaim {
    let mut conn = pool.acquire().await.unwrap();
    DirectRepo::claim_batch(&mut conn, fx.fence(), size, Utc::now(), LEASE, cap)
        .await
        .unwrap()
}

fn rows(claim: BatchClaim) -> Vec<DirectSendId> {
    match claim {
        BatchClaim::Rows { rows, .. } => rows.into_iter().map(|row| row.id).collect(),
        other => unreachable!("expected rows, got {other:?}"),
    }
}

fn batch_of(claim: &BatchClaim) -> DirectBatchId {
    match claim {
        BatchClaim::Rows { batch, .. } => *batch,
        other => unreachable!("expected rows, got {other:?}"),
    }
}

fn ids_of(claim: &BatchClaim) -> Vec<DirectSendId> {
    match claim {
        BatchClaim::Rows { rows, .. } => rows.iter().map(|row| row.id).collect(),
        other => unreachable!("expected rows, got {other:?}"),
    }
}

async fn expire_leases(pool: &PgPool) {
    sqlx::query("UPDATE direct_sends SET lease_expires_at = now() - interval '1 minute'")
        .execute(pool)
        .await
        .unwrap();
}

async fn queued(pool: &PgPool, delivery: DeliveryId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM direct_sends WHERE delivery_id = $1 AND state = 'queued'",
    )
    .bind(delivery.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn materialize_dedupes_and_skips_suppressed(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (l1, l2) = (list(&pool, fx.owner).await, list(&pool, fx.owner).await);
    contact(
        &pool,
        fx.owner,
        "c1@example.com",
        "subscribed",
        &[],
        &[l1, l2],
    )
    .await;
    contact(&pool, fx.owner, "c2@example.com", "subscribed", &[], &[l1]).await;
    suppress(&pool, fx.owner, "c2@example.com").await;
    contact(
        &pool,
        fx.owner,
        "c3@example.com",
        "unsubscribed",
        &[],
        &[l1],
    )
    .await;
    contact(&pool, fx.owner, "c4@example.com", "bounced", &[], &[l2]).await;

    let mut conn = pool.acquire().await.unwrap();
    let audience = Audience {
        lists: vec![l1, l2],
        tags: vec![],
    };
    assert_eq!(
        DirectRepo::materialize(&mut conn, &fx.delivery, &audience, Utc::now())
            .await
            .unwrap(),
        1
    );
    let counts = DirectRepo::progress(&mut conn, fx.delivery.id)
        .await
        .unwrap();
    assert_eq!((counts.audience, counts.queued), (1, 1));
}

#[sqlx::test]
async fn materialize_tag_filter(pool: PgPool) {
    let fx = fixture(&pool).await;
    let list = list(&pool, fx.owner).await;
    contact(
        &pool,
        fx.owner,
        "a@example.com",
        "subscribed",
        &["vip", "eu"],
        &[list],
    )
    .await;
    contact(
        &pool,
        fx.owner,
        "b@example.com",
        "subscribed",
        &["vip"],
        &[list],
    )
    .await;
    contact(&pool, fx.owner, "c@example.com", "subscribed", &[], &[list]).await;

    let mut conn = pool.acquire().await.unwrap();
    let audience = Audience {
        lists: vec![list],
        tags: vec!["vip".to_string(), "eu".to_string()],
    };
    assert_eq!(
        DirectRepo::materialize(&mut conn, &fx.delivery, &audience, Utc::now())
            .await
            .unwrap(),
        1,
        "all tags must match"
    );
}

#[sqlx::test]
async fn materialize_is_idempotent(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 3).await;
    // A contact who joins afterwards is not added by a second call.
    let list: Uuid = sqlx::query_scalar("SELECT id FROM contact_lists WHERE owner_id = $1")
        .bind(fx.owner.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    contact(
        &pool,
        fx.owner,
        "late@example.com",
        "subscribed",
        &[],
        &[ContactListId::from_uuid(list)],
    )
    .await;
    let mut conn = pool.acquire().await.unwrap();
    let audience = Audience {
        lists: vec![ContactListId::from_uuid(list)],
        tags: vec![],
    };
    assert_eq!(
        DirectRepo::materialize(&mut conn, &fx.delivery, &audience, Utc::now())
            .await
            .unwrap(),
        3
    );
    assert_eq!(queued(&pool, fx.delivery.id).await, 3);
}

#[sqlx::test]
async fn two_workers_claim_disjoint_batches(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 120).await;
    // No cap: with one, the quota row would serialize the claims of one owner. A claims and
    // stays open; B claims while A holds its rows and must skip them, not wait.
    let mut a = pool.begin().await.unwrap();
    let mut b = pool.begin().await.unwrap();
    let first = DirectRepo::claim_batch(&mut a, fx.fence(), 50, Utc::now(), LEASE, None)
        .await
        .unwrap();
    let second = within(DirectRepo::claim_batch(
        &mut b,
        fx.fence(),
        50,
        Utc::now(),
        LEASE,
        None,
    ))
    .await
    .unwrap();
    a.commit().await.unwrap();
    b.commit().await.unwrap();
    let (a, b) = (rows(first), rows(second));
    assert_eq!((a.len(), b.len()), (50, 50));
    assert!(a.iter().all(|id| !b.contains(id)), "batches overlap");
    assert_eq!(
        queued(&pool, fx.delivery.id).await,
        120,
        "leased rows stay queued"
    );
}

#[sqlx::test]
async fn cap_reached_mid_campaign_leaves_rows_queued(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 120).await;
    let key = cap_key(&fx);
    let first = claim(&pool, &fx, 50, Some((&key, 30))).await;
    assert_eq!(rows(first).len(), 30);
    let next = claim(&pool, &fx, 50, Some((&key, 30))).await;
    assert_eq!(next, BatchClaim::CapReached);
    assert_eq!(queued(&pool, fx.delivery.id).await, 120);
    let leased: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM direct_sends WHERE delivery_id = $1 AND lease_expires_at IS NULL",
    )
    .bind(fx.delivery.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leased, 90, "90 rows still wait for tomorrow's cap");
}

#[sqlx::test]
async fn unused_cap_units_are_released(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 10).await;
    let key = cap_key(&fx);
    // Ask for 50 against a cap of 100 with only 10 rows: 10 are used, 40 go back.
    assert_eq!(
        rows(claim(&pool, &fx, 50, Some((&key, 100))).await).len(),
        10
    );
    let used: i64 =
        sqlx::query_scalar("SELECT units_used FROM platform_quota_usage WHERE account_id = $1")
            .bind(fx.account.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(used, 10);
}

#[sqlx::test]
async fn unlimited_cap_claims_full_batch_without_usage_row(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 20).await;
    let key = cap_key(&fx);
    assert_eq!(rows(claim(&pool, &fx, 15, Some((&key, 0))).await).len(), 15);
    let usage: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_quota_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(usage, 0);
}

#[sqlx::test]
async fn each_claim_gets_a_new_batch_id(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 6).await;
    let first = claim(&pool, &fx, 2, None).await;
    let second = claim(&pool, &fx, 2, None).await;
    expire_leases(&pool).await;
    let third = claim(&pool, &fx, 2, None).await;
    let ids = [batch_of(&first), batch_of(&second), batch_of(&third)];
    assert!(
        ids[0] != ids[1] && ids[0] != ids[2] && ids[1] != ids[2],
        "batch IDs repeat: {ids:?}"
    );
}

#[sqlx::test]
async fn expired_batch_lease_rows_reclaimable(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 5).await;
    let first = claim(&pool, &fx, 5, None).await;
    assert_eq!(ids_of(&first).len(), 5);
    assert_eq!(
        claim(&pool, &fx, 5, None).await,
        BatchClaim::Empty,
        "leases are live"
    );

    expire_leases(&pool).await;
    let again = claim(&pool, &fx, 5, None).await;
    assert_eq!(ids_of(&again), ids_of(&first), "the same rows, new batch");
    assert_ne!(batch_of(&again), batch_of(&first));
}

#[sqlx::test]
async fn record_requires_matching_batch(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    let (id, batch) = (ids_of(&claimed)[0], batch_of(&claimed));
    let other = DirectBatchId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    let sent = DirectResult::Sent {
        provider_message_id: Some("m1".to_string()),
    };
    assert!(
        !DirectRepo::record(&mut conn, id, other, sent.clone(), Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(queued(&pool, fx.delivery.id).await, 1);
    assert!(
        DirectRepo::record(&mut conn, id, batch, sent.clone(), Utc::now())
            .await
            .unwrap()
    );
    assert!(
        !DirectRepo::record(&mut conn, id, batch, sent, Utc::now())
            .await
            .unwrap(),
        "a row is recorded once"
    );
    let counts = DirectRepo::progress(&mut conn, fx.delivery.id)
        .await
        .unwrap();
    assert_eq!((counts.sent, counts.queued), (1, 0));
}

#[sqlx::test]
async fn a_worker_that_lost_its_batch_cannot_record(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let a = claim(&pool, &fx, 1, None).await;
    expire_leases(&pool).await;
    let b = claim(&pool, &fx, 1, None).await;
    let id = ids_of(&a)[0];
    let mut conn = pool.acquire().await.unwrap();
    let sent = DirectResult::Sent {
        provider_message_id: None,
    };
    assert!(
        !DirectRepo::record(&mut conn, id, batch_of(&a), sent.clone(), Utc::now())
            .await
            .unwrap(),
        "the stale worker must not record onto a row another worker holds"
    );
    assert_eq!(queued(&pool, fx.delivery.id).await, 1);
    assert!(
        DirectRepo::record(&mut conn, id, batch_of(&b), sent, Utc::now())
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn an_expired_but_unclaimed_batch_still_records(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    expire_leases(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DirectRepo::record(
            &mut conn,
            ids_of(&claimed)[0],
            batch_of(&claimed),
            DirectResult::Sent {
                provider_message_id: None
            },
            Utc::now()
        )
        .await
        .unwrap(),
        "nobody reclaimed the row, and the mail went out"
    );
}

#[sqlx::test]
async fn claim_with_a_stale_fence_leases_nothing(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 3).await;
    let key = cap_key(&fx);
    let mut conn = pool.acquire().await.unwrap();

    let mut stale = fx.fence();
    stale.dispatch_gen += 1;
    let claim = DirectRepo::claim_batch(&mut conn, stale, 3, Utc::now(), LEASE, Some((&key, 10)))
        .await
        .unwrap();
    assert_eq!(claim, BatchClaim::Stale);

    sqlx::query("UPDATE deliveries SET state = 'cancelled' WHERE id = $1")
        .bind(fx.delivery.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let claim = DirectRepo::claim_batch(
        &mut conn,
        fx.fence(),
        3,
        Utc::now(),
        LEASE,
        Some((&key, 10)),
    )
    .await
    .unwrap();
    assert_eq!(claim, BatchClaim::Stale);

    let leased: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM direct_sends WHERE delivery_id = $1 AND lease_expires_at IS NOT NULL",
    )
    .bind(fx.delivery.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leased, 0);
    let usage: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_quota_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(usage, 0, "a stale claim reserves no cap units");
}

#[sqlx::test]
async fn renew_keeps_a_slow_batch(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 3).await;
    let claimed = claim(&pool, &fx, 3, None).await;
    let batch = batch_of(&claimed);
    let mut conn = pool.acquire().await.unwrap();

    // The lease would end in a minute; the worker renews for five more.
    sqlx::query("UPDATE direct_sends SET lease_expires_at = now() + interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    let renewed = DirectRepo::renew_batch(&mut conn, fx.delivery.id, batch, Utc::now(), LEASE)
        .await
        .unwrap();
    assert_eq!(renewed, 3);
    let later = Utc::now() + Duration::minutes(2);
    let second = DirectRepo::claim_batch(&mut conn, fx.fence(), 3, later, LEASE, None)
        .await
        .unwrap();
    assert_eq!(second, BatchClaim::Empty, "the renewed lease is still live");

    // A worker whose rows were reclaimed renews nothing.
    expire_leases(&pool).await;
    claim(&pool, &fx, 3, None).await;
    let lost = DirectRepo::renew_batch(&mut conn, fx.delivery.id, batch, Utc::now(), LEASE)
        .await
        .unwrap();
    assert_eq!(lost, 0);
}

#[sqlx::test]
async fn bounce_adds_suppression(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    let (id, batch) = (ids_of(&claimed)[0], batch_of(&claimed));
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DirectRepo::record(
            &mut conn,
            id,
            batch,
            DirectResult::Bounced {
                code: "mailbox_full".to_string()
            },
            Utc::now()
        )
        .await
        .unwrap()
    );
    let reason: String = sqlx::query_scalar(
        "SELECT reason FROM suppressions WHERE owner_id = $1 AND email_hash = $2",
    )
    .bind(fx.owner.as_uuid())
    .bind(hash("c0@example.com"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason, "bounced");
    let status: String = sqlx::query_scalar("SELECT status FROM contacts WHERE owner_id = $1")
        .bind(fx.owner.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "bounced");
}

#[sqlx::test]
async fn finish_when_all_terminal_publishes_with_counts(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 4).await;
    let claimed = claim(&pool, &fx, 4, None).await;
    let (ids, batch) = (ids_of(&claimed), batch_of(&claimed));
    let fence = fx.fence();

    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DirectRepo::finish_if_done(&mut conn, fence, Utc::now())
            .await
            .unwrap()
            .is_none(),
        "rows are still queued"
    );
    let results = [
        DirectResult::Sent {
            provider_message_id: None,
        },
        DirectResult::Sent {
            provider_message_id: None,
        },
        DirectResult::Failed {
            code: "rejected".to_string(),
        },
        DirectResult::Suppressed,
    ];
    for (id, result) in ids.iter().zip(results) {
        assert!(
            DirectRepo::record(&mut conn, *id, batch, result, Utc::now())
                .await
                .unwrap()
        );
    }
    let applied = DirectRepo::finish_if_done(&mut conn, fence, Utc::now())
        .await
        .unwrap();
    assert!(matches!(
        applied,
        Some(Applied::Done {
            state: DeliveryState::Published,
            ..
        })
    ));
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
            .bind(fx.delivery.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stored,
        serde_json::json!({
            "audience": 4, "queued": 0, "sent": 2, "failed": 1, "suppressed": 1, "bounced": 0
        })
    );
}

#[sqlx::test]
async fn cancel_queued_fails_the_waiting_rows(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 6).await;
    let claimed = claim(&pool, &fx, 2, None).await;
    let (ids, batch) = (ids_of(&claimed), batch_of(&claimed));
    let mut conn = pool.acquire().await.unwrap();
    DirectRepo::record(
        &mut conn,
        ids[0],
        batch,
        DirectResult::Sent {
            provider_message_id: None,
        },
        Utc::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        DirectRepo::cancel_queued(&mut conn, fx.delivery.id, Utc::now())
            .await
            .unwrap(),
        5
    );
    let counts = DirectRepo::progress(&mut conn, fx.delivery.id)
        .await
        .unwrap();
    assert_eq!((counts.sent, counts.failed, counts.queued), (1, 5, 0));
}

#[sqlx::test]
async fn materialize_bumps_the_job_version(pool: PgPool) {
    let fx = fixture(&pool).await;
    let before: i32 = sqlx::query_scalar("SELECT version FROM publish_jobs WHERE id = $1")
        .bind(fx.delivery.job_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    queue(&pool, &fx, 2).await;
    let after: i32 = sqlx::query_scalar("SELECT version FROM publish_jobs WHERE id = $1")
        .bind(fx.delivery.job_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, before + 1);
}

#[sqlx::test]
async fn record_and_cancel_use_the_given_time(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 2).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    let (id, batch) = (ids_of(&claimed)[0], batch_of(&claimed));
    let when = at(Utc::now() - Duration::days(3));
    let mut conn = pool.acquire().await.unwrap();
    let sent = DirectResult::Sent {
        provider_message_id: None,
    };
    assert!(
        DirectRepo::record(&mut conn, id, batch, sent, when)
            .await
            .unwrap()
    );
    let sent_at: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT sent_at FROM direct_sends WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sent_at, Some(when));

    DirectRepo::cancel_queued(&mut conn, fx.delivery.id, when)
        .await
        .unwrap();
    let touched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM direct_sends WHERE error_code = 'cancelled' AND updated_at = $1",
    )
    .bind(when)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(touched, 1);
}

#[sqlx::test]
async fn cap_reached_campaign_waits_and_resumes(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 10).await;
    let key = cap_key(&fx);
    claim(&pool, &fx, 4, Some((&key, 4))).await;
    assert_eq!(
        claim(&pool, &fx, 4, Some((&key, 4))).await,
        BatchClaim::CapReached
    );

    let now = Utc::now();
    let resume_at = at(now + Duration::hours(6));
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DirectRepo::wait_for_cap(&mut conn, fx.fence(), resume_at, now)
            .await
            .unwrap()
    );
    let (state, next): (String, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT state, next_attempt_at FROM deliveries WHERE id = $1")
            .bind(fx.delivery.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), next), ("processing", Some(resume_at)));
    assert_eq!(queued(&pool, fx.delivery.id).await, 10, "rows intact");

    // The leases lapse, but the delivery is waiting for its cap: not an orphan until it resumes.
    expire_leases(&pool).await;
    let grace = Duration::minutes(5);
    let just_before = resume_at + grace - Duration::seconds(1);
    assert!(
        Sweep::orphaned_direct(&mut conn, just_before, grace, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let after = resume_at + grace + Duration::seconds(1);
    assert_eq!(
        Sweep::orphaned_direct(&mut conn, after, grace, 10)
            .await
            .unwrap(),
        vec![fx.delivery.id]
    );
}

#[sqlx::test]
async fn wait_for_cap_is_fenced(pool: PgPool) {
    let fx = fixture(&pool).await;
    let mut stale = fx.fence();
    stale.dispatch_gen += 1;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        !DirectRepo::wait_for_cap(
            &mut conn,
            stale,
            at(Utc::now() + Duration::hours(1)),
            Utc::now()
        )
        .await
        .unwrap()
    );
    let next: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT next_attempt_at FROM deliveries WHERE id = $1")
            .bind(fx.delivery.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(
        next.map(|t| t > Utc::now() + Duration::minutes(30)),
        Some(true)
    );
}

async fn state_of(pool: &PgPool, id: DeliveryId) -> String {
    sqlx::query_scalar("SELECT state FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn stored_counts(pool: &PgPool, id: DeliveryId) -> serde_json::Value {
    sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn cancel(pool: &PgPool, fx: &Fixture) -> CancelOutcome {
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::cancel(
        &mut conn,
        &OwnerScope::own(fx.owner),
        fx.delivery.id,
        Utc::now(),
    )
    .await
    .unwrap()
}

#[sqlx::test]
async fn cancelling_a_running_campaign_stops_its_rows(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 4).await;
    assert_eq!(cancel(&pool, &fx).await, CancelOutcome::Cancelled);
    assert_eq!(state_of(&pool, fx.delivery.id).await, "cancelled");
    assert_eq!(queued(&pool, fx.delivery.id).await, 0);
    let stopped: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM direct_sends WHERE state = 'failed' AND error_code = 'cancelled'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stopped, 4);
    assert_eq!(
        stored_counts(&pool, fx.delivery.id).await,
        serde_json::json!({
            "audience": 4, "queued": 0, "sent": 0, "failed": 4, "suppressed": 0, "bounced": 0
        })
    );
}

#[sqlx::test]
async fn cancelling_a_job_stops_its_campaign_rows(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let post = post(&pool, owner, "hi").await;
    let (mail, social) = (
        account(&pool, owner, "mail").await,
        account(&pool, owner, "social").await,
    );
    let created = create_job(
        &pool,
        &scope,
        post,
        &[mail, social],
        at(Utc::now() - Duration::minutes(1)),
    )
    .await;
    let direct = created.deliveries[0].delivery;
    sqlx::query("UPDATE deliveries SET operation = 'direct', state = 'processing' WHERE id = $1")
        .bind(direct.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(direct.as_uuid())
    .bind(hash("a@example.com"))
    .execute(&pool)
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let outcomes = JobRepo::cancel(&mut conn, &scope, created.job, Utc::now())
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes
            .iter()
            .all(|(_, outcome)| *outcome == CancelOutcome::Cancelled)
    );
    assert_eq!(queued(&pool, direct).await, 0);
    assert_eq!(state_of(&pool, direct).await, "cancelled");
}

#[sqlx::test]
async fn cancel_before_processing_stops_materialized_rows(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 3).await;
    // The lease expired after materializing: the delivery is back to pending.
    sqlx::query("UPDATE deliveries SET state = 'pending' WHERE id = $1")
        .bind(fx.delivery.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(cancel(&pool, &fx).await, CancelOutcome::Cancelled);
    assert_eq!(queued(&pool, fx.delivery.id).await, 0);
}

#[sqlx::test]
async fn a_batch_in_flight_at_cancel_records_its_real_result(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 2).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    let (id, batch) = (ids_of(&claimed)[0], batch_of(&claimed));
    cancel(&pool, &fx).await;

    let mut conn = pool.acquire().await.unwrap();
    let sent = DirectResult::Sent {
        provider_message_id: Some("m1".to_string()),
    };
    assert!(
        DirectRepo::record(&mut conn, id, batch, sent.clone(), Utc::now())
            .await
            .unwrap(),
        "the mail went out before the cancel was seen"
    );
    let counts = DirectRepo::progress(&mut conn, fx.delivery.id)
        .await
        .unwrap();
    assert_eq!((counts.sent, counts.failed, counts.queued), (1, 1, 0));
    assert_eq!(stored_counts(&pool, fx.delivery.id).await["sent"], 1);
    assert!(
        !DirectRepo::record(&mut conn, id, batch, sent, Utc::now())
            .await
            .unwrap(),
        "recorded once"
    );
}

#[sqlx::test]
async fn a_bounce_after_cancel_still_suppresses(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let claimed = claim(&pool, &fx, 1, None).await;
    cancel(&pool, &fx).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        DirectRepo::record(
            &mut conn,
            ids_of(&claimed)[0],
            batch_of(&claimed),
            DirectResult::Bounced {
                code: "mailbox_full".to_string()
            },
            Utc::now()
        )
        .await
        .unwrap()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM direct_sends")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "bounced");
    let suppressed: i64 = sqlx::query_scalar("SELECT count(*) FROM suppressions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(suppressed, 1);
}

#[sqlx::test]
async fn an_unleased_cancelled_row_cannot_be_recorded(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 1).await;
    let id: Uuid = sqlx::query_scalar("SELECT id FROM direct_sends")
        .fetch_one(&pool)
        .await
        .unwrap();
    cancel(&pool, &fx).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        !DirectRepo::record(
            &mut conn,
            DirectSendId::from_uuid(id),
            DirectBatchId::from_uuid(Uuid::now_v7()),
            DirectResult::Sent {
                provider_message_id: None
            },
            Utc::now()
        )
        .await
        .unwrap()
    );
}

#[sqlx::test]
async fn a_reclaimed_batch_counts_against_the_cap_again(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 10).await;
    let key = cap_key(&fx);
    let used = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT units_used FROM platform_quota_usage WHERE account_id = $1",
        )
        .bind(fx.account.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let first = claim(&pool, &fx, 5, Some((&key, 10))).await;
    assert_eq!(used().await, 5);

    // The worker died: its lease lapses, and the next claim takes the same rows.
    expire_leases(&pool).await;
    let again = claim(&pool, &fx, 5, Some((&key, 10))).await;
    assert_eq!(ids_of(&again), ids_of(&first));
    assert_eq!(
        used().await,
        10,
        "the crashed batch's units are not given back"
    );

    assert_eq!(
        claim(&pool, &fx, 5, Some((&key, 10))).await,
        BatchClaim::CapReached
    );
    assert_eq!(queued(&pool, fx.delivery.id).await, 10);
}

#[sqlx::test]
async fn late_results_after_a_cancel_store_exact_counts(pool: PgPool) {
    let fx = fixture(&pool).await;
    queue(&pool, &fx, 2).await;
    let first = claim(&pool, &fx, 1, None).await;
    let second = claim(&pool, &fx, 1, None).await;
    cancel(&pool, &fx).await;
    let version = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i32>("SELECT version FROM publish_jobs")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let before = version(pool.clone()).await;
    let sent = || DirectResult::Sent {
        provider_message_id: None,
    };

    // The first late result holds the delivery open; the second must wait for it and then count
    // both, not overwrite the counts with a snapshot taken before the first committed.
    let mut tx = pool.begin().await.unwrap();
    assert!(
        DirectRepo::record(
            &mut tx,
            ids_of(&first)[0],
            batch_of(&first),
            sent(),
            Utc::now()
        )
        .await
        .unwrap()
    );
    let other = tokio::spawn({
        let pool = pool.clone();
        let (id, batch) = (ids_of(&second)[0], batch_of(&second));
        async move {
            let mut conn = pool.acquire().await.unwrap();
            DirectRepo::record(&mut conn, id, batch, sent(), Utc::now())
                .await
                .unwrap()
        }
    });
    within(wait_for_blocked_statement(&pool)).await;
    tx.commit().await.unwrap();
    assert!(within(other).await.unwrap());

    assert_eq!(stored_counts(&pool, fx.delivery.id).await["sent"], 2);
    assert!(
        version(pool.clone()).await > before,
        "a late result bumps the job version"
    );
}
