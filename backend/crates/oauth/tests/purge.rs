//! The `oauth_state_purge` job: expired states and pending connections are deleted, and the
//! tokens of discarded results are revoked at the provider first, under the revoke-skip rule.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::Utc;
use common::{
    SECRET, mock_agent, oauth_agent_with, oauth_config, oauth_provider, platform, platforms,
    query_param, sha256_hex, values,
};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::{ExchangeScript, MockScript};
use postit_core::{OAuthProviderSpec, OAuthStateId};
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::oauth_states::OAuthStateRepo;
use postit_data::testkit::seed_user;
use postit_oauth::jobs::{PurgeReport, run_oauth_state_purge};
use postit_oauth::testkit::{
    TestOptions, account_service_with, test_accounts_settings, test_vault,
};
use postit_oauth::{AccountError, AccountService, CallbackParams, Confirmed};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";
const DONE: &str = "https://postit.local:44315/oauth-done";

struct Fixture {
    service: AccountService,
    provider: MockOAuthProvider,
    script: MockScript,
    scope: OwnerScope,
}

fn options() -> TestOptions {
    let mut configs = BTreeMap::new();
    configs.insert(platform(OAUTH), Some(oauth_config()));
    TestOptions {
        configs,
        ..TestOptions::default()
    }
}

async fn fixture_with(pool: &PgPool, edit: impl FnOnce(&mut OAuthProviderSpec)) -> Fixture {
    platforms(pool, &[OAUTH, "mock"]).await;
    let provider = oauth_provider().await;
    let (agent, script) = oauth_agent_with(&provider, OAUTH, edit);
    let (credentials, _) = mock_agent("mock");
    let service = account_service_with(
        pool.clone(),
        vec![agent, credentials],
        test_accounts_settings(),
        options(),
    );
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        provider,
        script,
        scope: OwnerScope::own(owner),
    }
}

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, |_| {}).await
}

/// `scope` starts a connection, the provider approves it, and the callback runs: where the
/// browser is sent.
async fn callback_by(f: &Fixture, scope: &OwnerScope) -> Url {
    let start = f
        .service
        .start_oauth(scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    let code = f.provider.approve(&start.authorize_url);
    let state = query_param(&start.authorize_url, "state").unwrap();
    f.service
        .oauth_callback(
            OAUTH,
            CallbackParams {
                code: Some(SecretString::from(code)),
                state: Some(state),
                error: None,
            },
        )
        .await
}

async fn connected_by(f: &Fixture, scope: &OwnerScope) -> String {
    let target = callback_by(f, scope).await;
    query_param(&target, "connect_id").unwrap_or_else(|| panic!("callback failed: {target}"))
}

async fn confirm(
    f: &Fixture,
    scope: &OwnerScope,
    connect_id: &str,
    ids: Option<&[&str]>,
) -> Result<Confirmed, AccountError> {
    f.service
        .confirm(
            scope,
            connect_id,
            ids.map(|ids| ids.iter().map(|id| (*id).to_owned()).collect()),
            AuditContext::default(),
        )
        .await
}

/// Connects and confirms nothing: a discarded result.
async fn discarded_by(f: &Fixture, scope: &OwnerScope) {
    let connect_id = connected_by(f, scope).await;
    let confirmed = confirm(f, scope, &connect_id, Some(&[])).await.unwrap();
    assert!(confirmed.connected.is_empty());
}

#[derive(sqlx::FromRow)]
struct StateRow {
    id: Uuid,
    connect_id_hash: Option<Vec<u8>>,
    grant_tokens: Option<Vec<u8>>,
}

async fn state_rows(pool: &PgPool) -> Vec<StateRow> {
    sqlx::query_as("SELECT id, connect_id_hash, grant_tokens FROM oauth_states ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

/// The refresh token sealed in each state row that holds tokens.
async fn refresh_tokens(pool: &PgPool) -> Vec<String> {
    state_rows(pool)
        .await
        .iter()
        .filter_map(|row| {
            let sealed =
                postit_vault::Sealed::<Value>::from_bytes(row.grant_tokens.clone()?).unwrap();
            let opened: Value = test_vault()
                .open(
                    &sealed,
                    &OAuthStateRepo::seal_context(OAuthStateId::from(row.id), "grant_tokens"),
                )
                .unwrap();
            Some(opened["refresh_token"].as_str().unwrap().to_owned())
        })
        .collect()
}

async fn purge(f: &Fixture) -> PurgeReport {
    run_oauth_state_purge(&f.service, Utc::now()).await.unwrap()
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_job_deletes_expired_rows(pool: PgPool) {
    let f = fixture(&pool).await;
    // An abandoned authorization (no tokens) and an abandoned credentials check, both expired.
    f.service
        .start_oauth(&f.scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    f.service
        .start_credentials(&f.scope, &platform("mock"), values(SECRET))
        .await
        .unwrap();
    sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE pending_connections SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    // Live ones stay.
    f.service
        .start_oauth(&f.scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    let live = connected_by(&f, &f.scope).await;

    let report = purge(&f).await;
    assert_eq!(report.purged, 2);
    assert_eq!(report.revoked, 0);
    assert_eq!(count(&pool, "pending_connections").await, 0);
    assert_eq!(count(&pool, "oauth_states").await, 2);
    assert!(f.provider.revoked().is_empty());
    // The live result can still be confirmed.
    assert_eq!(
        confirm(&f, &f.scope, &live, None)
            .await
            .unwrap()
            .connected
            .len(),
        1
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_revokes_discarded_results(pool: PgPool) {
    let f = fixture(&pool).await;
    // A callback that found no accounts.
    f.script.set_oauth_accounts(0);
    let target = callback_by(&f, &f.scope).await;
    assert_eq!(
        query_param(&target, "code").as_deref(),
        Some("no_accounts"),
        "{target}"
    );
    // A confirm that chose nothing.
    f.script.set_oauth_accounts(1);
    discarded_by(&f, &f.scope).await;
    // A result nobody confirmed in time.
    connected_by(&f, &f.scope).await;
    sqlx::query(
        "UPDATE oauth_states SET expires_at = now() - interval '1 second'
         WHERE connect_id_hash IS NOT NULL",
    )
    .execute(&pool)
    .await
    .unwrap();
    let tokens = refresh_tokens(&pool).await;
    assert_eq!(tokens.len(), 3);

    let report = purge(&f).await;
    assert_eq!(report.revoked, 3);
    assert_eq!(report.skipped, 0);
    let revoked = f.provider.revoked();
    for token in &tokens {
        assert!(revoked.contains(&sha256_hex(token)), "{token} not revoked");
    }
    assert!(state_rows(&pool).await.is_empty());
    // A second run finds nothing left to revoke.
    assert_eq!(purge(&f).await, PurgeReport::default());
    assert_eq!(f.provider.revoked().len(), 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_skips_results_whose_remote_user_is_in_use(pool: PgPool) {
    let f = fixture(&pool).await;
    // Another owner holds an active grant of `mock-user`.
    let other = OwnerScope::own(seed_user(&pool, "active").await);
    let theirs = connected_by(&f, &other).await;
    confirm(&f, &other, &theirs, None).await.unwrap();
    discarded_by(&f, &f.scope).await;

    let report = purge(&f).await;
    assert_eq!(report.revoked, 0);
    assert_eq!(report.skipped, 1);
    assert!(f.provider.revoked().is_empty());
    assert!(state_rows(&pool).await.is_empty());
    let active: String = sqlx::query_scalar("SELECT status FROM account_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(active, "active");

    // The grant is gone, but `mock-user` has another live pending result.
    sqlx::query("UPDATE account_grants SET status = 'revoked'")
        .execute(&pool)
        .await
        .unwrap();
    let live = connected_by(&f, &f.scope).await;
    discarded_by(&f, &f.scope).await;

    let report = purge(&f).await;
    assert_eq!(report.revoked, 0);
    assert_eq!(report.skipped, 1);
    assert!(f.provider.revoked().is_empty());
    let rows = state_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].connect_id_hash.is_some());
    // The live result is still the owner's to confirm.
    confirm(&f, &f.scope, &live, None).await.unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_skips_results_without_remote_user_id(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_exchange(ExchangeScript {
        remote_user_id: None,
        ..ExchangeScript::default()
    });
    discarded_by(&f, &f.scope).await;

    let report = purge(&f).await;
    assert_eq!(report.revoked, 0);
    assert_eq!(report.skipped, 1);
    assert!(f.provider.revoked().is_empty());
    assert_eq!(f.script.revoke_calls(), 0);
    assert!(state_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_drops_results_of_an_unavailable_platform(pool: PgPool) {
    let f = fixture(&pool).await;
    discarded_by(&f, &f.scope).await;
    discarded_by(&f, &f.scope).await;
    postit_data::platforms::PlatformRepo::set_enabled(&pool, &platform(OAUTH), false)
        .await
        .unwrap();

    let report = purge(&f).await;
    assert_eq!(report.revoked, 0);
    assert_eq!(report.skipped, 2);
    assert!(f.provider.revoked().is_empty());
    assert_eq!(f.script.revoke_calls(), 0);
    assert!(state_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn purge_uses_the_agent_revoke_without_revoke_url(pool: PgPool) {
    let f = fixture_with(&pool, |spec| spec.revoke_url = None).await;
    discarded_by(&f, &f.scope).await;

    let report = purge(&f).await;
    assert_eq!(report.revoked, 1);
    assert_eq!(f.script.revoke_calls(), 1);
    assert!(f.provider.revoked().is_empty());
    assert!(state_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn attacker_result_is_revoked_by_the_purge(pool: PgPool) {
    let f = fixture(&pool).await;
    let attacker = OwnerScope::own(seed_user(&pool, "active").await);
    let victim = f.scope.clone();
    // The attacker starts; the victim's browser completes the callback and confirms.
    let connect_id = connected_by(&f, &attacker).await;
    assert!(matches!(
        confirm(&f, &victim, &connect_id, None).await,
        Err(AccountError::NotFound)
    ));
    let tokens = refresh_tokens(&pool).await;
    assert_eq!(tokens.len(), 1);

    let report = purge(&f).await;
    assert_eq!(report.revoked, 1);
    assert!(f.provider.revoked().contains(&sha256_hex(&tokens[0])));
    assert!(state_rows(&pool).await.is_empty());
    for user in [attacker.owner, victim.owner] {
        let grants: i64 =
            sqlx::query_scalar("SELECT count(*) FROM account_grants WHERE owner_id = $1")
                .bind(user.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(grants, 0);
    }
}
