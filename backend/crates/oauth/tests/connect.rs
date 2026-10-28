#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use chrono::{Duration, Utc};
use common::{SECRET, found, mock_agent, mock_oauth_agent, platform, platforms, values};
use postit_agent_mock::{MockScript, VerifyScript};
use postit_core::{PendingConnectionId, UserId, ViolationCode};
use postit_data::OwnerScope;
use postit_data::ScopeError;
use postit_data::accounts::{AccountRepo, AccountStatus, GrantRepo};
use postit_data::audit::AuditContext;
use postit_data::pending_connections::{NewPendingConnection, PendingConnectionRepo};
use postit_data::platforms::PlatformRepo;
use postit_data::testkit::{callers, seed_user};
use postit_oauth::testkit::{account_service, test_accounts_settings, test_vault};
use postit_oauth::{AccountChange, AccountError, AccountService, ConnectId};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    service: AccountService,
    script: MockScript,
    owner: UserId,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool, platform_id: &str, accounts: &[&str]) -> Fixture {
    platforms(pool, &[platform_id]).await;
    let (agent, script) = mock_agent(platform_id);
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

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn start(f: &Fixture, platform_id: &str) -> String {
    let check = f
        .service
        .start_credentials(&f.scope, &platform(platform_id), values(SECRET))
        .await
        .unwrap();
    check.pending.unwrap().0.as_str().to_owned()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_returns_connect_id_and_discovered(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a", "b"]).await;

    let check = f
        .service
        .start_credentials(&f.scope, &platform("fake"), values(SECRET))
        .await
        .unwrap();

    let (connect_id, expires_at) = check.pending.unwrap();
    assert!(connect_id.as_str().len() >= 43);
    assert!(expires_at > Utc::now() + Duration::minutes(9));
    assert_eq!(check.discovered.len(), 2);
    assert_eq!(count(&pool, "pending_connections").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_invalid_fields_returns_every_violation(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let Value::Object(bad) = json!({"extra": "x"}) else {
        unreachable!()
    };

    let result = f
        .service
        .start_credentials(&f.scope, &platform("fake"), bad)
        .await;

    let Err(AccountError::Validation(violations)) = result else {
        panic!("expected validation");
    };
    // Both required fields are missing and the unknown one is named.
    assert!(violations.len() >= 3, "{violations:?}");
    assert!(
        violations
            .iter()
            .any(|v| v.code == ViolationCode::OptionRequired
                && v.path.as_deref() == Some("options.token"))
    );
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_invalid_credentials_names_field(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    f.script
        .set_verify(VerifyScript::Invalid(Some("token".to_owned())));

    let result = f
        .service
        .start_credentials(&f.scope, &platform("fake"), values(SECRET))
        .await;

    let Err(AccountError::CredentialsInvalid {
        field,
        missing_scopes,
    }) = result
    else {
        panic!("expected credentials_invalid");
    };
    assert_eq!(field.as_deref(), Some("token"));
    assert!(missing_scopes.is_empty());
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_empty_discovery_returns_no_connect_id_and_stores_nothing(pool: PgPool) {
    let f = fixture(&pool, "fake", &[]).await;

    let check = f
        .service
        .start_credentials(&f.scope, &platform("fake"), values(SECRET))
        .await
        .unwrap();

    assert!(check.pending.is_none());
    assert!(check.discovered.is_empty());
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_oauth_platform_refused(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    // Switched on and with a config table, so only the connect method refuses it.
    let agent = mock_oauth_agent("fake");
    let service = account_service(pool.clone(), vec![agent], test_accounts_settings());
    let owner = seed_user(&pool, "active").await;

    let result = service
        .start_credentials(&OwnerScope::own(owner), &platform("fake"), values(SECRET))
        .await;

    // Without a table the catalog answers first; either way nothing connects.
    assert!(matches!(
        result,
        Err(AccountError::ConnectMethodUnsupported | AccountError::PlatformUnavailable)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_creates_grant_and_all_accounts(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a", "b"]).await;
    let connect_id = start(&f, "fake").await;

    let confirmed = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(confirmed.connected.len(), 2);
    assert!(
        confirmed
            .connected
            .iter()
            .all(|(a, change)| a.status == AccountStatus::Active
                && *change == AccountChange::Created)
    );
    assert_eq!(confirmed.skipped_elsewhere, 0);
    assert!(confirmed.unknown.is_empty());
    assert_eq!(count(&pool, "account_grants").await, 1);
    assert_eq!(count(&pool, "connected_accounts").await, 2);
    assert_eq!(count(&pool, "pending_connections").await, 0);
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind = 'account_connected' AND owner_id = $1",
    )
    .bind(f.owner.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_subset_and_unknown_ids(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a", "b"]).await;
    let connect_id = start(&f, "fake").await;

    let confirmed = f
        .service
        .confirm(
            &f.scope,
            &connect_id,
            Some(vec!["b".to_owned(), "zzz".to_owned()]),
            AuditContext::default(),
        )
        .await
        .unwrap();

    assert_eq!(confirmed.connected.len(), 1);
    assert_eq!(confirmed.connected[0].0.remote_account_id, "b");
    assert_eq!(confirmed.unknown, vec!["zzz".to_owned()]);
    assert_eq!(count(&pool, "connected_accounts").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_reaching_several_accounts_shares_one_credential(pool: PgPool) {
    use postit_core::{CredentialStore, Credentials};
    use postit_oauth::PgCredentialStore;
    use secrecy::ExposeSecret;

    let f = fixture(&pool, "fake", &["a", "b"]).await;
    let connect_id = start(&f, "fake").await;

    let confirmed = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await
        .unwrap();

    let grants: Vec<_> = confirmed
        .connected
        .iter()
        .map(|(a, _)| a.grant_id.unwrap())
        .collect();
    assert_eq!(grants[0], grants[1]);
    assert_eq!(count(&pool, "account_grants").await, 1);
    let store = PgCredentialStore::new(pool.clone(), test_vault(), f.owner, grants[0]);
    let Credentials::Fields(fields) = store.credentials().await.unwrap() else {
        panic!("expected fields");
    };
    assert_eq!(fields.get_secret("token").unwrap().expose_secret(), SECRET);
    assert_eq!(
        fields.get_plain("account_name"),
        Some(&json!("test-account"))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_by_other_user_is_not_found_and_deletes_pending(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let connect_id = start(&f, "fake").await;
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);

    let theirs = f
        .service
        .confirm(&stranger, &connect_id, None, AuditContext::default())
        .await;
    let mine = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await;

    assert!(matches!(theirs, Err(AccountError::NotFound)));
    assert!(matches!(mine, Err(AccountError::NotFound)));
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(count(&pool, "account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_expired_creates_nothing(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let id = PendingConnectionId::from(Uuid::now_v7());
    let presented = "an-expired-connect-id";
    let mut conn = pool.acquire().await.unwrap();
    PendingConnectionRepo::insert(
        &mut conn,
        &f.scope,
        NewPendingConnection {
            id,
            platform: platform("fake"),
            connect_id_hash: ConnectId::hash_of(presented),
            credential: test_vault()
                .seal(
                    &json!({"token": SECRET}),
                    &PendingConnectionRepo::seal_context(id),
                )
                .unwrap(),
            connection_config: json!({"account_name": "test-account"}),
            discovered: json!([found("a")]),
            expires_at: Utc::now() - Duration::minutes(1),
        },
    )
    .await
    .unwrap();

    let result = f
        .service
        .confirm(&f.scope, presented, None, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::NotFound)));
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_twice_second_not_found(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let connect_id = start(&f, "fake").await;

    let first = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await;
    let second = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await;

    assert!(first.is_ok());
    assert!(matches!(second, Err(AccountError::NotFound)));
    assert_eq!(count(&pool, "connected_accounts").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_confirms_one_wins(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a", "b"]).await;
    let connect_id = start(&f, "fake").await;

    let (one, two) = tokio::join!(
        f.service
            .confirm(&f.scope, &connect_id, None, AuditContext::default()),
        f.service
            .confirm(&f.scope, &connect_id, None, AuditContext::default()),
    );

    let wins = [&one, &two].iter().filter(|r| r.is_ok()).count();
    let misses = [&one, &two]
        .iter()
        .filter(|r| matches!(r, Err(AccountError::NotFound)))
        .count();
    assert_eq!((wins, misses), (1, 1));
    assert_eq!(count(&pool, "connected_accounts").await, 2);
    assert_eq!(count(&pool, "account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn connected_elsewhere_skipped_without_owner(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a", "b"]).await;
    // Another owner already has "a".
    let other = fixture_for(&pool, &f, "other").await;
    let other_id = start(&other, "fake").await;
    other
        .service
        .confirm(
            &other.scope,
            &other_id,
            Some(vec!["a".to_owned()]),
            AuditContext::default(),
        )
        .await
        .unwrap();
    let connect_id = start(&f, "fake").await;

    let confirmed = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(confirmed.connected.len(), 1);
    assert_eq!(confirmed.connected[0].0.remote_account_id, "b");
    assert_eq!(confirmed.skipped_elsewhere, 1);
    let other_text = other.owner.as_uuid().to_string();
    assert!(!format!("{confirmed:?}").contains(&other_text));
    let details: Vec<Value> =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE owner_id = $1")
            .bind(f.owner.as_uuid())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!format!("{details:?}").contains(&other_text));
}

/// A second owner against the same service and platform.
async fn fixture_for(pool: &PgPool, like: &Fixture, _name: &str) -> Fixture {
    let owner = seed_user(pool, "active").await;
    Fixture {
        service: like.service.clone(),
        script: like.script.clone(),
        owner,
        scope: OwnerScope::own(owner),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn all_skipped_leaves_no_grant(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let other = fixture_for(&pool, &f, "other").await;
    let other_id = start(&other, "fake").await;
    other
        .service
        .confirm(&other.scope, &other_id, None, AuditContext::default())
        .await
        .unwrap();
    let grants_before = count(&pool, "account_grants").await;
    let connect_id = start(&f, "fake").await;

    let confirmed = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await
        .unwrap();

    assert!(confirmed.connected.is_empty());
    assert_eq!(confirmed.skipped_elsewhere, 1);
    assert_eq!(count(&pool, "account_grants").await, grants_before);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconnect_moves_account_and_deletes_old_grant(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let first_id = start(&f, "fake").await;
    let first = f
        .service
        .confirm(&f.scope, &first_id, None, AuditContext::default())
        .await
        .unwrap();
    let old_grant = first.connected[0].0.grant_id.unwrap();
    let again_id = start(&f, "fake").await;

    let again = f
        .service
        .confirm(&f.scope, &again_id, None, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(again.connected[0].1, AccountChange::Moved);
    assert_eq!(again.connected[0].0.id, first.connected[0].0.id);
    assert_ne!(again.connected[0].0.grant_id.unwrap(), old_grant);
    assert_eq!(count(&pool, "account_grants").await, 1);
    let mut conn = pool.acquire().await.unwrap();
    assert!(matches!(
        GrantRepo::get(&mut conn, &f.scope, old_grant).await,
        Err(postit_data::DataError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn second_email_method_is_sending_method_exists(pool: PgPool) {
    let f = fixture(&pool, "email", &["me@example.test", "you@example.test"]).await;
    let first_id = start(&f, "email").await;
    f.service
        .confirm(
            &f.scope,
            &first_id,
            Some(vec!["me@example.test".to_owned()]),
            AuditContext::default(),
        )
        .await
        .unwrap();
    let second_id = start(&f, "email").await;

    let result = f
        .service
        .confirm(
            &f.scope,
            &second_id,
            Some(vec!["you@example.test".to_owned()]),
            AuditContext::default(),
        )
        .await;

    assert!(matches!(result, Err(AccountError::SendingMethodExists)));
    assert_eq!(count(&pool, "connected_accounts").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 1);
    // The failed connect rolled back with the take, so the user can confirm again after
    // removing the existing method.
    assert_eq!(count(&pool, "pending_connections").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn platform_disabled_between_start_and_confirm(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let connect_id = start(&f, "fake").await;
    PlatformRepo::set_enabled(&pool, &platform("fake"), false)
        .await
        .unwrap();

    let result = f
        .service
        .confirm(&f.scope, &connect_id, None, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::PlatformDisabled)));
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(count(&pool, "account_grants").await, 0);
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_and_confirm_are_owner_only(pool: PgPool) {
    let f = fixture(&pool, "fake", &["a"]).await;
    let who = callers(&pool, f.owner, &[]).await;
    let connect_id = start(&f, "fake").await;

    for (name, scope) in [
        ("view", &who.view),
        ("edit", &who.edit),
        ("publish", &who.publish),
        ("limited", &who.limited),
    ] {
        let started = f
            .service
            .start_credentials(scope, &platform("fake"), values(SECRET))
            .await;
        assert!(
            matches!(started, Err(AccountError::Scope(ScopeError::OwnerOnly))),
            "{name} start"
        );
        let confirmed = f
            .service
            .confirm(scope, &connect_id, None, AuditContext::default())
            .await;
        assert!(
            matches!(confirmed, Err(AccountError::Scope(ScopeError::OwnerOnly))),
            "{name} confirm"
        );
    }
    // A refused delegate did not consume the pending row.
    assert!(
        f.service
            .confirm(&f.scope, &connect_id, None, AuditContext::default())
            .await
            .is_ok()
    );
    let mut conn = pool.acquire().await.unwrap();
    let listed = AccountRepo::list(
        &mut conn,
        &f.scope,
        emixdb::dto::Pagination {
            page: 1,
            page_size: 10,
        },
        None,
        false,
    )
    .await
    .unwrap();
    assert_eq!(listed.total, 1);
}
