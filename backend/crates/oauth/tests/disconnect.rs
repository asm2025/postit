#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{connect_all, force_state, found, job_for, mock_agent, platforms};
use postit_agent_mock::{MockScript, VerifyScript};
use postit_core::UserId;
use postit_data::audit::AuditContext;
use postit_data::testkit::{callers, seed_user};
use postit_data::{OwnerScope, ScopeError};
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountError, AccountService};
use sqlx::PgPool;

struct Fixture {
    service: AccountService,
    script: MockScript,
    owner: UserId,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool, accounts: &[&str]) -> Fixture {
    platforms(pool, &["fake"]).await;
    let (agent, script) = mock_agent("fake");
    script.set_verify(VerifyScript::Accounts(
        accounts.iter().map(|id| found(id)).collect(),
    ));
    let service = account_service(pool.clone(), vec![agent], test_accounts_settings());
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_cancels_pending_and_parked_and_keeps_row(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let job = job_for(&pool, f.owner, &accounts[0]).await;
    assert_eq!(job.deliveries.len(), 1);

    let outcome = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(outcome.cancelled_deliveries, 1);
    assert_eq!(outcome.native_scheduled_remaining, 0);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM deliveries WHERE state = 'cancelled'"
        )
        .await,
        1
    );
    let kept: String = sqlx::query_scalar("SELECT status FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, "disconnected");
    let events = count(
        &pool,
        "SELECT count(*) FROM audit_events WHERE kind = 'account_disconnected'",
    )
    .await;
    assert_eq!(events, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_refused_while_in_flight(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    job_for(&pool, f.owner, &accounts[0]).await;
    force_state(&pool, accounts[0].id, "claimed").await;

    let result = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::JobActive)));
    let status: String = sqlx::query_scalar("SELECT status FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_frees_remote_key_for_another_owner(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();
    let other = OwnerScope::own(seed_user(&pool, "active").await);

    let theirs = connect_all(&f.service, &other, "fake").await;

    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].remote_account_id, "a");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn last_account_revokes_and_deletes_grant(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;

    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(f.script.revoke_calls(), 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn one_of_two_accounts_keeps_grant(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;

    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_failure_still_deletes_grant(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.script.fail_revoke(true);

    let outcome = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(outcome.is_ok());
    assert_eq!(f.script.revoke_calls(), 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_owner_only(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let who = callers(&pool, f.owner, &[accounts[0].id]).await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .disconnect(scope, accounts[0].id, AuditContext::default())
            .await;
        assert!(matches!(
            result,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
    assert_eq!(f.script.revoke_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stranger_gets_not_found(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);

    let result = f
        .service
        .disconnect(&stranger, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::NotFound)));
}
