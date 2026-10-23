#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use common::{connect_all, found, platforms};
use emixdb::dto::Pagination;
use postit_core::{AccountId, UserId, ViolationCode};
use postit_data::accounts::{AccountStatus, GrantStatus};
use postit_data::audit::AuditContext;
use postit_data::testkit::{Callers, callers, seed_user};
use postit_data::{OwnerScope, ScopeError};
use postit_oauth::testkit::{
    InfoScript, Script, TestCredentialAgent, VerifyScript, account_service, test_accounts_settings,
};
use postit_oauth::{AccountError, AccountService, ConnectKind};
use sqlx::PgPool;
use uuid::Uuid;

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

struct Fixture {
    service: AccountService,
    agent: Arc<TestCredentialAgent>,
    owner: UserId,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool, accounts: &[&str]) -> Fixture {
    platforms(pool, &["fake"]).await;
    let agent = TestCredentialAgent::new("fake");
    agent.set_script(Script {
        verify: VerifyScript::Accounts(accounts.iter().map(|id| found(id)).collect()),
        ..Script::default()
    });
    let service = account_service(pool.clone(), vec![agent.clone()], test_accounts_settings());
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        agent,
        owner,
        scope: OwnerScope::own(owner),
    }
}

async fn who(pool: &PgPool, f: &Fixture, visible: &[AccountId]) -> Callers {
    callers(pool, f.owner, visible).await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_hides_disconnected_by_default(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    let listed = f.service.list(&f.scope, None, false, page()).await.unwrap();

    assert_eq!(listed.total, 1);
    assert_eq!(listed.data[0].account.id, accounts[1].id);
    assert_eq!(listed.data[0].connect, Some(ConnectKind::Credentials));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_include_disconnected(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    let listed = f.service.list(&f.scope, None, true, page()).await.unwrap();

    assert_eq!(listed.total, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_and_get_respect_account_limit(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let who = who(&pool, &f, &[accounts[0].id]).await;

    for (name, scope) in [
        ("view", &who.view),
        ("edit", &who.edit),
        ("publish", &who.publish),
    ] {
        let listed = f.service.list(scope, None, false, page()).await.unwrap();
        assert_eq!(listed.total, 2, "{name}");
    }
    let limited = f
        .service
        .list(&who.limited, None, false, page())
        .await
        .unwrap();
    assert_eq!(limited.total, 1);
    assert_eq!(limited.data[0].account.id, accounts[0].id);
    assert!(matches!(
        f.service.get(&who.limited, accounts[1].id).await,
        Err(AccountError::NotFound)
    ));
    let stranger = f
        .service
        .list(&who.stranger, None, false, page())
        .await
        .unwrap();
    assert_eq!(stranger.total, 0);
    assert!(matches!(
        f.service.get(&who.stranger, accounts[0].id).await,
        Err(AccountError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_owner_only(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let who = who(&pool, &f, &[]).await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .update(scope, accounts[0].id, Some(true), None)
            .await;
        assert!(matches!(
            result,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let renamed = f
        .service
        .update(
            &f.scope,
            accounts[0].id,
            Some(true),
            Some("  Mine ".to_owned()),
        )
        .await
        .unwrap();
    assert!(renamed.account.test_account);
    assert_eq!(renamed.account.display_name, "Mine");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_rejects_blank_name(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;

    let blank = f
        .service
        .update(&f.scope, accounts[0].id, None, Some("   ".to_owned()))
        .await;
    let long = f
        .service
        .update(&f.scope, accounts[0].id, None, Some("x".repeat(101)))
        .await;

    let Err(AccountError::Validation(blank)) = blank else {
        panic!("expected validation");
    };
    assert_eq!(blank[0].code, ViolationCode::OptionTooShort);
    assert_eq!(blank[0].path.as_deref(), Some("display_name"));
    let Err(AccountError::Validation(long)) = long else {
        panic!("expected validation");
    };
    assert_eq!(long[0].code, ViolationCode::OptionTooLong);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn check_marks_checked_and_passes_remote_id(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    assert!(accounts[0].last_checked_at.is_none());

    let checked = f
        .service
        .check(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(checked.account.status, AccountStatus::Active);
    assert!(checked.account.last_checked_at.is_some());
    assert_eq!(f.agent.seen_remote_ids(), vec!["a".to_owned()]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn check_auth_failure_moves_grant_and_all_its_accounts_to_needs_reconnect(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.agent.set_script(Script {
        account_info: InfoScript::AuthFailed,
        ..Script::default()
    });

    let checked = f
        .service
        .check(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(checked.account.status, AccountStatus::NeedsReconnect);
    let other = f.service.get(&f.scope, accounts[1].id).await.unwrap();
    assert_eq!(other.account.status, AccountStatus::NeedsReconnect);
    let grant: String = sqlx::query_scalar("SELECT status FROM account_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(grant, GrantStatus::NeedsReconnect.as_str());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn check_disconnected_refused(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    let result = f
        .service
        .check(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::AccountDisconnected)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn check_owner_only(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let who = who(&pool, &f, &[]).await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .check(scope, accounts[0].id, AuditContext::default())
            .await;
        assert!(matches!(
            result,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let stranger = f
        .service
        .check(&who.stranger, accounts[0].id, AuditContext::default())
        .await;
    assert!(matches!(stranger, Err(AccountError::NotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn mark_needs_reconnect_writes_one_event_per_account(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let grant = accounts[0].grant_id.unwrap();

    f.service
        .mark_needs_reconnect(&f.scope, grant, AuditContext::default())
        .await
        .unwrap();

    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind = 'account_needs_reconnect'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 2);
    let missing = f
        .service
        .mark_needs_reconnect(
            &f.scope,
            postit_core::GrantId::from(Uuid::now_v7()),
            AuditContext::default(),
        )
        .await;
    assert!(matches!(missing, Err(AccountError::NotFound)));
}
