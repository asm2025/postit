#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{connect_all, found, mock_agent, platforms};
use postit_agent_mock::{MockScript, VerifyScript};
use postit_core::{AccountId, UserId};
use postit_data::OwnerScope;
use postit_data::accounts::GrantStatus;
use postit_data::audit::AuditContext;
use postit_data::testkit::seed_user;
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountError, AccountService};
use sqlx::{Connection, PgPool};

struct Fixture {
    service: AccountService,
    #[allow(dead_code)]
    script: MockScript,
    owner: UserId,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool) -> Fixture {
    platforms(pool, &["fake"]).await;
    let (agent, script) = mock_agent("fake");
    script.set_verify(VerifyScript::Accounts(vec![found("a")]));
    let service = account_service(pool.clone(), vec![agent], test_accounts_settings());
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

async fn grant_status(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT status FROM account_grants")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bound_client_for_disconnected_account_is_refused(pool: PgPool) {
    let f = fixture(&pool).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    let result = f.service.bound_client(f.owner, accounts[0].id).await;

    assert!(matches!(result, Err(AccountError::AccountDisconnected)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bound_client_for_unknown_account_is_not_found(pool: PgPool) {
    let f = fixture(&pool).await;

    let result = f
        .service
        .bound_client(f.owner, AccountId::from_uuid(uuid::Uuid::now_v7()))
        .await;

    assert!(matches!(result, Err(AccountError::NotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bound_client_carries_descriptor_and_account_ref(pool: PgPool) {
    let f = fixture(&pool).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let account = &accounts[0];

    let bound = f.service.bound_client(f.owner, account.id).await.unwrap();

    assert_eq!(bound.owner, f.owner);
    assert_eq!(bound.grant, account.grant_id.unwrap());
    assert_eq!(bound.account.account, account.id);
    assert_eq!(bound.account.platform, account.platform);
    assert_eq!(bound.account.remote_id, account.remote_account_id);
    assert_eq!(bound.descriptor.platform, account.platform);
    // The client answers for the account it is bound to.
    let info = bound.client.account_info(&bound.account).await.unwrap();
    assert_eq!(info.status, postit_core::AccountStatus::Active);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn mark_needs_reconnect_in_rolls_back_with_the_callers_transaction(pool: PgPool) {
    let f = fixture(&pool).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let grant = accounts[0].grant_id.unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let mut tx = conn.begin().await.unwrap();
    f.service
        .mark_needs_reconnect_in(&mut tx, &f.scope, grant, AuditContext::default())
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(grant_status(&pool).await, GrantStatus::Active.as_str());
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind = 'account_needs_reconnect'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 0);

    // Committed, the same call marks the grant and writes the event.
    let mut conn = pool.acquire().await.unwrap();
    let mut tx = conn.begin().await.unwrap();
    f.service
        .mark_needs_reconnect_in(&mut tx, &f.scope, grant, AuditContext::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        grant_status(&pool).await,
        GrantStatus::NeedsReconnect.as_str()
    );
}
