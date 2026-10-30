//! Claim, the pre-send checks, quota, revalidation, and parking. Every test runs the
//! `publish_delivery` handler against the `mock` agent through `bound_client`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use chrono::{DateTime, Days, TimeZone, Utc};
use common::{
    FixtureOptions, JobSpec, account_units, attempts, connect, create_job, fixture, fixture_with,
    last_error, mock, notices, park_times, run, set_live, state, upload,
};
use postit_agent_mock::MockAgent;
use postit_core::{
    AccountId, Agent, DeliveryState, Notice, ParkReason, PlatformId, QuotaBucket, QuotaCosts,
    QuotaPeriod, QuotaScope,
};
use postit_data::OwnerScope;
use postit_data::testkit::seed_user;
use postit_publishing::PublishDelivery;
use postit_publishing::worker::run_publish;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn bucket(id: &str, scope: QuotaScope, zone: &str, limit: u32) -> QuotaBucket {
    QuotaBucket {
        id: id.to_owned(),
        scope,
        period: QuotaPeriod::Day,
        default_limit: limit,
        reset_time_zone: zone.to_owned(),
        costs: QuotaCosts {
            publish: 1,
            comment: 0,
            inbox_poll: 0,
            metrics: 0,
        },
    }
}

fn parked_notices(notices: &[Notice]) -> Vec<&Notice> {
    notices
        .iter()
        .filter(|n| matches!(n, Notice::DeliveryParked(_)))
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stale_dispatch_finds_nothing(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    run_publish(
        &fx.service,
        PublishDelivery {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen + 1,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    assert_untouched(&pool, &fx, dispatch.delivery).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn not_due_finds_nothing(pool: PgPool) {
    let fx = fixture(&pool).await;
    let spec = JobSpec {
        scheduled_at: Utc::now() + chrono::Duration::hours(1),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Pending
    );
    assert_untouched(&pool, &fx, dispatch.delivery).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_owner_parks_owner_disabled(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(fx.env.owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::OwnerDisabled)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn needs_reconnect_parks(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    sqlx::query("UPDATE connected_accounts SET status = 'needs_reconnect' WHERE id = $1")
        .bind(fx.account.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::AccountNeedsReconnect)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn uncompiled_platform_parks_platform_unavailable(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            compiled: Some(vec![PlatformId::new("bluesky").unwrap()]),
            ..FixtureOptions::default()
        },
    )
    .await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::PlatformUnavailable)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_platform_parks(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    disable_mock(&pool).await;

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::PlatformDisabled)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn kill_switch_parks_live_but_not_dry_run(pool: PgPool) {
    let fx = fixture(&pool).await;
    let live = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let dry = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::dry_run()).await[0];
    set_live(&pool, false).await;

    run(&fx.service, live).await;
    run(&fx.service, dry).await;

    assert_eq!(
        state(&pool, live.delivery).await,
        DeliveryState::Parked(ParkReason::LivePublishDisabled)
    );
    // The dry run passed every check and was simulated.
    assert_eq!(state(&pool, dry.delivery).await, DeliveryState::Simulated);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn parked_live_delivery_notifies_owner_once(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    disable_mock(&pool).await;

    run(&fx.service, dispatch).await;
    // A duplicate of the same job finds nothing to claim.
    run(&fx.service, dispatch).await;

    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(all.len(), 1);
    let Notice::DeliveryParked(notice) = &all[0] else {
        panic!("expected delivery_parked, got {:?}", all[0]);
    };
    assert_eq!(notice.delivery_id, dispatch.delivery);
    assert_eq!(notice.account.account_id, fx.account);
    assert_eq!(notice.account.platform, mock());
    assert_eq!(notice.account.account_name, "main");
    assert_eq!(notice.park_reason, Some(ParkReason::PlatformDisabled));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn exhausted_bucket_parks_quota_exceeded(pool: PgPool) {
    // The deployment lowers the mock's `posts` bucket to one a day.
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            config: Some(json!({"quota": {"posts": 1}})),
            ..FixtureOptions::default()
        },
    )
    .await;
    let first = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let second = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    run(&fx.service, first).await;
    run(&fx.service, second).await;

    assert_eq!(state(&pool, first.delivery).await, DeliveryState::Published);
    assert_eq!(
        state(&pool, second.delivery).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
    assert_eq!(account_units(&pool, fx.account).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn app_bucket_shared_across_two_owners(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            buckets: Some(vec![bucket("app_posts", QuotaScope::App, "UTC", 1)]),
            ..FixtureOptions::default()
        },
    )
    .await;
    let other = seed_user(&pool, "active").await;
    let other_account = connect(&fx.accounts, &OwnerScope::own(other), "other").await;
    let first = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let second = create_job(&pool, other, &[other_account], JobSpec::live()).await[0];

    run(&fx.service, first).await;
    run(&fx.service, second).await;

    assert_eq!(state(&pool, first.delivery).await, DeliveryState::Published);
    assert_eq!(
        state(&pool, second.delivery).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revalidation_failure_fails_with_codes_only(pool: PgPool) {
    let fx = fixture(&pool).await;
    // Over the mock's 500-grapheme limit.
    let spec = JobSpec {
        text: "x".repeat(600),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    let error = last_error(&pool, dispatch.delivery).await.unwrap();
    assert_eq!(
        error,
        json!({
            "code": "revalidation_failed",
            "violations": [{"code": "text_too_long", "path": "text"}],
        })
    );
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(all.len(), 1);
    assert!(matches!(&all[0], Notice::DeliveryFailed(n) if n.delivery_id == dispatch.delivery));
    assert_eq!(attempts(&pool, dispatch.delivery).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deleted_media_fails_revalidation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let asset = upload(&fx.env, "a.png").await;
    // A running job's snapshot keeps its media from being deleted, so delete it first: the
    // snapshot then names an asset that is gone.
    fx.env.media.delete(&fx.env.scope, asset).await.unwrap();
    let spec = JobSpec {
        media: vec![asset],
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({
            "code": "revalidation_failed",
            "violations": [{"code": "media_missing", "path": "media[0]"}],
        })
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn parking_never_degrades_to_dry_run(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    set_live(&pool, false).await;

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::LivePublishDisabled)
    );
    assert_eq!(attempts(&pool, dispatch.delivery).await, 0);
    assert!(fx.script.posts().is_empty());
    assert_eq!(fx.script.mutating_calls(), 0);
}

/// Review Focus 9.
#[sqlx::test(migrations = "../data/migrations")]
async fn quota_park_waits_for_the_period_and_notifies_once(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            buckets: Some(vec![bucket("posts", QuotaScope::Account, "Asia/Riyadh", 1)]),
            ..FixtureOptions::default()
        },
    )
    .await;
    let first = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    let spec = JobSpec::live();
    let due = spec.scheduled_at;
    let second = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];
    // Taken before the runs: a run that crosses 21:00 UTC (midnight in Riyadh) parks until the
    // midnight after.
    let midnight_before = next_riyadh_midnight(Utc::now());

    run(&fx.service, first).await;
    run(&fx.service, second).await;
    run(&fx.service, second).await;

    assert_eq!(
        state(&pool, second.delivery).await,
        DeliveryState::Parked(ParkReason::QuotaExceeded)
    );
    let (parked_until, next_attempt_at) = park_times(&pool, second.delivery).await;
    let midnight_after = next_riyadh_midnight(Utc::now());
    assert!(
        parked_until == Some(midnight_before) || parked_until == Some(midnight_after),
        "{parked_until:?}"
    );
    assert_eq!(next_attempt_at, Some(micros(due)), "still the due time");
    let all = notices(&pool, fx.env.owner).await;
    let parked = parked_notices(&all);
    assert_eq!(parked.len(), 1);
    assert!(
        matches!(parked[0], Notice::DeliveryParked(n) if n.park_reason == Some(ParkReason::QuotaExceeded))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revalidation_failure_releases_quota(pool: PgPool) {
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
    let failing = create_job(&pool, fx.env.owner, &[fx.account], invalid).await[0];
    let valid = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];

    run(&fx.service, failing).await;
    assert_eq!(state(&pool, failing.delivery).await, DeliveryState::Failed);
    assert_eq!(account_units(&pool, fx.account).await, 0);

    // The released unit is there for the next delivery.
    run(&fx.service, valid).await;
    assert_eq!(state(&pool, valid.delivery).await, DeliveryState::Published);
    assert_eq!(account_units(&pool, fx.account).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unconfigured_platform_fails_without_parking(pool: PgPool) {
    let provider = url::Url::parse("https://example.test/").unwrap();
    let oauth: Arc<dyn Agent> = Arc::new(MockAgent::oauth(&provider));
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            // Compiled in and switched on, but the deployment has no `[platforms.mock_oauth]`.
            extra: vec![(oauth, None)],
            compiled: Some(vec![mock(), PlatformId::new("mock_oauth").unwrap()]),
            ..FixtureOptions::default()
        },
    )
    .await;
    let account = AccountId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, \
         display_name) VALUES ($1, $2, 'mock_oauth', 'remote-oauth', 'OAuth account')",
    )
    .bind(account.as_uuid())
    .bind(fx.env.owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let spec = JobSpec {
        platform: PlatformId::new("mock_oauth").unwrap(),
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[account], spec).await[0];

    run(&fx.service, dispatch).await;

    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    assert_eq!(
        last_error(&pool, dispatch.delivery).await.unwrap(),
        json!({"code": "platform_unavailable"})
    );
    let all = notices(&pool, fx.env.owner).await;
    assert_eq!(all.len(), 1);
    assert!(matches!(&all[0], Notice::DeliveryFailed(_)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dry_run_records_no_notifications(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::dry_run()).await[0];
    disable_mock(&pool).await;

    run(&fx.service, dispatch).await;

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::PlatformDisabled)
    );
    assert!(notices(&pool, fx.env.owner).await.is_empty());
}

/// Nothing happened to the delivery: no notice, and no lease taken.
async fn assert_untouched(pool: &PgPool, fx: &common::Fixture, delivery: postit_core::DeliveryId) {
    assert!(notices(pool, fx.env.owner).await.is_empty());
    let lease: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT lease_expires_at FROM deliveries WHERE id = $1")
            .bind(delivery.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(lease, None);
}

async fn disable_mock(pool: &PgPool) {
    sqlx::query("UPDATE platforms SET enabled = FALSE WHERE id = 'mock'")
        .execute(pool)
        .await
        .unwrap();
}

/// The midnight in Riyadh (UTC+3 all year) after `now`, in UTC.
fn next_riyadh_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    let zone: chrono_tz::Tz = "Asia/Riyadh".parse().unwrap();
    let tomorrow = now.with_timezone(&zone).date_naive() + Days::new(1);
    zone.from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// `time` cut to the microseconds Postgres stores.
fn micros(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(time.timestamp_micros()).unwrap()
}
