//! Disconnecting an account with the publishing engine's hook (Task 13): native uploads are
//! deleted through the agent first, running campaigns stop in the disconnect transaction, and
//! history stays.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use chrono::Utc;
use common::{
    Fixture, JobSpec, attempts, connect, create_job, fixture, force, native_upload, notices, run,
    service_without_native_scheduler, state,
};
use postit_core::{AccountId, DeliveryId, DeliveryState, Notice};
use postit_data::audit::AuditContext;
use postit_oauth::{AccountError, AccountService, DisconnectOutcome};
use postit_publishing::{PublishingDisconnect, PublishingService};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// The fixture's account service with `engine`'s hook on it.
fn hooked(fx: &Fixture, engine: &PublishingService) -> AccountService {
    fx.accounts
        .with_disconnect_hook(Arc::new(PublishingDisconnect::new(engine.clone())))
}

async fn disconnect(
    accounts: &AccountService,
    fx: &Fixture,
    account: AccountId,
) -> Result<DisconnectOutcome, AccountError> {
    accounts
        .disconnect(&fx.env.scope, account, AuditContext::default())
        .await
}

async fn account_status(pool: &PgPool, account: AccountId) -> String {
    sqlx::query_scalar("SELECT status FROM connected_accounts WHERE id = $1")
        .bind(account.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn partials(all: &[Notice]) -> usize {
    all.iter()
        .filter(|n| matches!(n, Notice::JobPartial(_)))
        .count()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_deletes_native_uploads_through_the_agent(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    assert_eq!(fx.script.posts().len(), 1);

    let outcome = disconnect(&hooked(&fx, &fx.service), &fx, fx.account)
        .await
        .unwrap();

    assert_eq!(outcome.native_scheduled_remaining, 0);
    assert!(fx.script.posts().is_empty(), "deleted on the platform");
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(account_status(&pool, fx.account).await, "disconnected");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_lists_uploads_the_agent_cannot_delete(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let without = service_without_native_scheduler(&fx);

    let outcome = disconnect(&hooked(&fx, &without), &fx, fx.account)
        .await
        .unwrap();

    assert_eq!(outcome.native_scheduled_remaining, 1);
    assert_eq!(fx.script.posts().len(), 1, "the platform still holds it");
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(account_status(&pool, fx.account).await, "disconnected");
}

/// Makes `delivery` a running direct campaign with one sent and two queued rows.
async fn running_campaign(pool: &PgPool, fx: &Fixture, delivery: DeliveryId) {
    sqlx::query("UPDATE deliveries SET operation = 'direct', state = 'processing' WHERE id = $1")
        .bind(delivery.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    for (n, row_state) in ["sent", "queued", "queued"].into_iter().enumerate() {
        let mut hash = format!("r{n}@example.com").into_bytes();
        hash.resize(32, 0);
        sqlx::query(
            "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash, state, sent_at) \
             VALUES ($1, $2, $3, $4, $5, CASE WHEN $5 = 'sent' THEN now() END)",
        )
        .bind(Uuid::now_v7())
        .bind(fx.env.owner.as_uuid())
        .bind(delivery.as_uuid())
        .bind(hash)
        .bind(row_state)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_stops_a_running_campaign(pool: PgPool) {
    let fx = fixture(&pool).await;
    let delivery =
        create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0].delivery;
    running_campaign(&pool, &fx, delivery).await;

    let outcome = disconnect(&hooked(&fx, &fx.service), &fx, fx.account)
        .await
        .unwrap();

    assert_eq!(outcome.cancelled_deliveries, 1);
    assert_eq!(state(&pool, delivery).await, DeliveryState::Cancelled);
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT state, error_code FROM direct_sends WHERE delivery_id = $1 ORDER BY id",
    )
    .bind(delivery.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("sent".to_owned(), None),
            ("failed".to_owned(), Some("cancelled".to_owned())),
            ("failed".to_owned(), Some("cancelled".to_owned())),
        ]
    );
    let counts: Value = sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
        .bind(delivery.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(counts["sent"], 1);
    assert_eq!(counts["failed"], 2);
    assert_eq!(counts["queued"], 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_records_job_partial_for_live_jobs_only(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let accounts = hooked(&fx, &fx.service);
    // A live job that published to the first account and still waits for the second.
    let live = create_job(&pool, fx.env.owner, &[fx.account, second], JobSpec::live()).await;
    run(&fx.service, live[0]).await;
    // The same in a dry run.
    let dry = create_job(
        &pool,
        fx.env.owner,
        &[fx.account, second],
        JobSpec::dry_run(),
    )
    .await;
    run(&fx.service, dry[0]).await;
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 0);

    disconnect(&accounts, &fx, second).await.unwrap();

    assert_eq!(
        state(&pool, live[1].delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(
        state(&pool, dry[1].delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_refused_while_in_flight_leaves_the_platform_untouched(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let native = native_upload(&fx.service, &pool, fx.env.owner, fx.account, at).await;
    let running = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    force(&pool, running.delivery, "in_flight").await;
    let calls = fx.script.mutating_calls();

    let result = disconnect(&hooked(&fx, &fx.service), &fx, fx.account).await;

    assert!(matches!(result, Err(AccountError::JobActive)));
    assert_eq!(
        fx.script.mutating_calls(),
        calls,
        "no delete reached the platform"
    );
    assert_eq!(fx.script.posts().len(), 1);
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(account_status(&pool, fx.account).await, "active");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_keeps_history(pool: PgPool) {
    let fx = fixture(&pool).await;
    let done = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    run(&fx.service, done).await;
    assert_eq!(state(&pool, done.delivery).await, DeliveryState::Published);

    disconnect(&hooked(&fx, &fx.service), &fx, fx.account)
        .await
        .unwrap();

    assert_eq!(state(&pool, done.delivery).await, DeliveryState::Published);
    assert_eq!(attempts(&pool, done.delivery).await, 1);
    assert_eq!(
        fx.script.posts().len(),
        1,
        "a published post stays published"
    );
    assert_eq!(account_status(&pool, fx.account).await, "disconnected");
}
