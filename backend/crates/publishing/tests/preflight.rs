//! The account preflight (Task 10): accounts with a delivery due within the hour are checked
//! with the platform unless they were checked within the hour.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use chrono::{Duration, Utc};
use common::{JobSpec, account_status, create_job, fixture, run, state};
use postit_agent_mock::InfoScript;
use postit_core::{AccountId, DeliveryState, ParkReason};
use postit_publishing::preflight::run_preflight;
use sqlx::PgPool;

/// Sets the account's last good check `minutes` ago, or never.
async fn checked(pool: &PgPool, account: AccountId, minutes: Option<i64>) {
    sqlx::query("UPDATE connected_accounts SET last_checked_at = $2 WHERE id = $1")
        .bind(account.as_uuid())
        .bind(minutes.map(|m| Utc::now() - Duration::minutes(m)))
        .execute(pool)
        .await
        .unwrap();
}

async fn last_checked(pool: &PgPool, account: AccountId) -> Option<chrono::DateTime<Utc>> {
    sqlx::query_scalar("SELECT last_checked_at FROM connected_accounts WHERE id = $1")
        .bind(account.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn preflight_detects_revoked_credential(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    checked(&pool, fx.account, Some(120)).await;
    // The platform no longer accepts the stored token.
    fx.script.set_account_info(InfoScript::AuthFailed);

    let asked = run_preflight(&fx.service).await.unwrap();

    assert_eq!(asked, 1);
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("needs_reconnect".to_owned(), "needs_reconnect".to_owned())
    );
    // The delivery parks when it comes due instead of reaching the platform.
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Parked(ParkReason::AccountNeedsReconnect)
    );
    assert!(fx.script.posts().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn preflight_stamps_a_good_account(pool: PgPool) {
    let fx = fixture(&pool).await;
    create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await;
    checked(&pool, fx.account, None).await;

    let asked = run_preflight(&fx.service).await.unwrap();

    assert_eq!(asked, 1);
    assert!(last_checked(&pool, fx.account).await.is_some());
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("active".to_owned(), "active".to_owned())
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn preflight_skips_recently_checked_accounts(pool: PgPool) {
    let fx = fixture(&pool).await;
    create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await;
    checked(&pool, fx.account, Some(10)).await;
    fx.script.set_account_info(InfoScript::AuthFailed);

    let asked = run_preflight(&fx.service).await.unwrap();

    assert_eq!(asked, 0);
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("active".to_owned(), "active".to_owned())
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn preflight_skips_accounts_with_nothing_due_soon(pool: PgPool) {
    let fx = fixture(&pool).await;
    // Due in three hours, and a dry run due now: neither needs the account checked yet.
    create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec {
            scheduled_at: Utc::now() + Duration::hours(3),
            ..JobSpec::live()
        },
    )
    .await;
    create_job(&pool, fx.env.owner, &[fx.account], JobSpec::dry_run()).await;
    checked(&pool, fx.account, None).await;
    fx.script.set_account_info(InfoScript::AuthFailed);

    let asked = run_preflight(&fx.service).await.unwrap();

    assert_eq!(asked, 0);
    assert_eq!(
        account_status(&pool, fx.account).await,
        ("active".to_owned(), "active".to_owned())
    );
}
