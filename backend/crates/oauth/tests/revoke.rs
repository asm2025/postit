//! Revoking OAuth grants at the provider: on disconnect, under the revoke-skip rule, under the
//! refresh lock, and through the OAuth user deletion step.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use chrono::Utc;
use common::{
    oauth_agent_with, oauth_config, oauth_grant, oauth_provider, platform, platforms, query_param,
    sha256_hex,
};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::{ExchangeScript, MockScript};
use postit_core::{AccountId, GrantId, OAuthProviderSpec, OAuthStateId, UserId};
use postit_data::OwnerScope;
use postit_data::accounts::GrantRepo;
use postit_data::audit::AuditContext;
use postit_data::oauth_states::OAuthStateRepo;
use postit_data::testkit::seed_user;
use postit_identity::deletion::UserDeletionStep;
use postit_oauth::testkit::{
    TestOptions, account_service_with, test_accounts_settings, test_vault,
};
use postit_oauth::{AccountService, CallbackParams, OAuthDeletionStep};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";
const DONE: &str = "https://postit.local:44315/oauth-done";

struct Fixture {
    pool: PgPool,
    service: AccountService,
    provider: MockOAuthProvider,
    script: MockScript,
    owner: UserId,
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
    platforms(pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let (agent, script) = oauth_agent_with(&provider, OAUTH, edit);
    let service = account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        options(),
    );
    let owner = seed_user(pool, "active").await;
    Fixture {
        pool: pool.clone(),
        service,
        provider,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, |_| {}).await
}

async fn disconnect(f: &Fixture, scope: &OwnerScope, account: AccountId) {
    f.service
        .disconnect(scope, account, AuditContext::default())
        .await
        .unwrap();
}

async fn grant_count(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM account_grants")
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

/// The grant's status and whether it still holds a credential.
async fn grant_state(f: &Fixture, grant: GrantId) -> (String, bool) {
    sqlx::query_as("SELECT status, credential IS NOT NULL FROM account_grants WHERE id = $1")
        .bind(grant.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

/// `scope` starts a connection, the provider approves it, and the callback runs; returns the
/// `connect_id` of the live result.
async fn connected_by(f: &Fixture, scope: &OwnerScope) -> String {
    let start = f
        .service
        .start_oauth(scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    let code = f.provider.approve(&start.authorize_url);
    let state = query_param(&start.authorize_url, "state").unwrap();
    let target: Url = f
        .service
        .oauth_callback(
            OAUTH,
            CallbackParams {
                code: Some(SecretString::from(code)),
                state: Some(state),
                error: None,
            },
        )
        .await;
    query_param(&target, "connect_id").unwrap_or_else(|| panic!("callback failed: {target}"))
}

/// The remote user and refresh token of each state row that holds tokens.
async fn result_tokens(pool: &PgPool) -> BTreeMap<String, String> {
    let rows: Vec<(Uuid, Option<String>, Vec<u8>)> = sqlx::query_as(
        "SELECT id, remote_user_id, grant_tokens FROM oauth_states
         WHERE grant_tokens IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(id, remote, sealed)| {
            let sealed = postit_vault::Sealed::<Value>::from_bytes(sealed).unwrap();
            let opened: Value = test_vault()
                .open(
                    &sealed,
                    &OAuthStateRepo::seal_context(OAuthStateId::from(id), "grant_tokens"),
                )
                .unwrap();
            (
                remote.unwrap(),
                opened["refresh_token"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnecting_one_of_two_accounts_keeps_the_grant_unrevoked(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, accounts) = oauth_grant(
        &pool,
        f.owner,
        OAUTH,
        Some("mock-user"),
        "r-shared",
        &["a1", "a2"],
    )
    .await;

    disconnect(&f, &f.scope, accounts[0]).await;

    assert!(f.provider.revoked().is_empty());
    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(grant_state(&f, grant).await, ("active".to_owned(), true));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnecting_the_last_account_revokes_at_revoke_url(pool: PgPool) {
    let f = fixture(&pool).await;
    let (_, accounts) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-last", &["a1"]).await;

    disconnect(&f, &f.scope, accounts[0]).await;

    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-last")]);
    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(grant_count(&f).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_skipped_without_remote_user_id(pool: PgPool) {
    let f = fixture(&pool).await;
    let (_, accounts) = oauth_grant(&pool, f.owner, OAUTH, None, "r-anon", &["a1"]).await;

    disconnect(&f, &f.scope, accounts[0]).await;

    assert!(f.provider.revoked().is_empty());
    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(grant_count(&f).await, 0, "the grant is still deleted");
}

/// Review Focus 4: owner A's old grant lingers while owner B holds an active grant of the same
/// remote user. Revoking A's grant would end B's authorization on a provider that revokes per
/// user and app pair, so it is not sent.
#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_skipped_when_another_owner_holds_the_remote_user(pool: PgPool) {
    let f = fixture(&pool).await;
    let (_, a_accounts) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-a", &["a1"]).await;
    let b = seed_user(&pool, "active").await;
    let (b_grant, b_accounts) =
        oauth_grant(&pool, b, OAUTH, Some("mock-user"), "r-b", &["b1"]).await;

    disconnect(&f, &f.scope, a_accounts[0]).await;

    assert!(f.provider.revoked().is_empty());
    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(grant_count(&f).await, 1, "A's grant is deleted");
    assert_eq!(grant_state(&f, b_grant).await, ("active".to_owned(), true));

    // Once B disconnects too, nothing else holds `mock-user`: B's grant is revoked.
    disconnect(&f, &OwnerScope::own(b), b_accounts[0]).await;
    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-b")]);
    assert_eq!(grant_count(&f).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_skipped_while_a_pending_result_holds_the_remote_user(pool: PgPool) {
    let f = fixture(&pool).await;
    let (_, accounts) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-own", &["a1"]).await;
    // The same user connects `mock-user` again in another tab and has not confirmed yet.
    let live = connected_by(&f, &f.scope).await;

    disconnect(&f, &f.scope, accounts[0]).await;

    assert!(f.provider.revoked().is_empty());
    assert_eq!(grant_count(&f).await, 0);
    // The other tab's result is untouched and still confirms.
    let confirmed = f
        .service
        .confirm(&f.scope, &live, None, AuditContext::default())
        .await
        .unwrap();
    assert_eq!(confirmed.connected.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn agent_revoke_used_when_spec_has_no_revoke_url(pool: PgPool) {
    let f = fixture_with(&pool, |spec| spec.revoke_url = None).await;
    let (_, accounts) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-agent", &["a1"]).await;

    disconnect(&f, &f.scope, accounts[0]).await;

    assert_eq!(f.script.revoke_calls(), 1);
    assert!(f.provider.revoked().is_empty());
    assert_eq!(grant_count(&f).await, 0);
}

/// A refresh holding the grant's lock has stored rotated tokens but not committed yet. A
/// revoke started meanwhile waits for it, then sends the rotated refresh token, not the old
/// one the provider has already retired.
#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_waits_for_a_running_refresh(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) = oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-old", &["a1"]).await;
    let row = {
        let mut conn = pool.acquire().await.unwrap();
        GrantRepo::get(&mut conn, &f.scope, grant).await.unwrap()
    };

    let mut refresh = pool.begin().await.unwrap();
    GrantRepo::lock_for_refresh(&mut refresh, grant)
        .await
        .unwrap()
        .unwrap();
    let rotated = test_vault()
        .seal(
            &json!({"access_token": "access-new", "refresh_token": "r-new"}),
            &GrantRepo::seal_context(grant),
        )
        .unwrap();
    GrantRepo::store_tokens(
        &mut refresh,
        grant,
        &rotated,
        Some(Utc::now() + chrono::Duration::hours(1)),
    )
    .await
    .unwrap();

    let service = f.service.clone();
    let owner = f.owner;
    let revoke = tokio::spawn(async move { service.revoke_grant(owner, &row).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !revoke.is_finished(),
        "the revoke must wait for the refresh"
    );
    assert!(f.provider.revoked().is_empty());

    refresh.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), revoke)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-new")]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_revokes_once(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) = oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-del", &["a1"]).await;
    let step = OAuthDeletionStep::new(f.service.clone());
    assert_eq!(step.name(), "oauth");

    step.run(f.owner).await.unwrap();
    step.run(f.owner).await.unwrap();

    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-del")]);
    assert_eq!(grant_state(&f, grant).await, ("revoked".to_owned(), false));
    let account: String = sqlx::query_scalar("SELECT status FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(account, "revoked");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_revokes_needs_reconnect_grants(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-stale", &["a1"]).await;
    // Past its `reconnect_by`, but the authorization may still be live at the provider.
    sqlx::query("UPDATE account_grants SET status = 'needs_reconnect' WHERE id = $1")
        .bind(grant.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    OAuthDeletionStep::new(f.service.clone())
        .run(f.owner)
        .await
        .unwrap();

    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-stale")]);
    assert_eq!(grant_state(&f, grant).await, ("revoked".to_owned(), false));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_revokes_pending_results(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-grant", &["a1"]).await;
    let script = |remote: &str| ExchangeScript {
        remote_user_id: Some(remote.to_owned()),
        ..ExchangeScript::default()
    };
    // A live result.
    f.script.set_exchange(script("u-live"));
    connected_by(&f, &f.scope).await;
    // A discarded one: confirmed with nothing chosen.
    f.script.set_exchange(script("u-disc"));
    let discarded = connected_by(&f, &f.scope).await;
    let confirmed = f
        .service
        .confirm(&f.scope, &discarded, Some(vec![]), AuditContext::default())
        .await
        .unwrap();
    assert!(confirmed.connected.is_empty());
    // A live result of the remote user the owner's grant also holds.
    f.script.set_exchange(script("mock-user"));
    connected_by(&f, &f.scope).await;
    let tokens = result_tokens(&pool).await;
    assert_eq!(tokens.len(), 3);

    OAuthDeletionStep::new(f.service.clone())
        .run(f.owner)
        .await
        .unwrap();

    let revoked: BTreeSet<String> = f.provider.revoked().into_iter().collect();
    let expected: BTreeSet<String> = [
        sha256_hex(&tokens["u-live"]),
        sha256_hex(&tokens["u-disc"]),
        sha256_hex("r-grant"),
    ]
    .into_iter()
    .collect();
    assert_eq!(revoked, expected);
    assert_eq!(
        f.provider.revoked().len(),
        3,
        "`mock-user` is revoked once, through the grant"
    );
    let states: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_states")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(states, 0);
    assert_eq!(grant_state(&f, grant).await, ("revoked".to_owned(), false));
}

/// Renames `table` to `to`: a stand-in for a database that fails one query mid-run.
async fn rename(pool: &PgPool, table: &str, to: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE {table} RENAME TO {to}"
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn state_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_states")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A database failure while the step plans a result's revoke rolls the take back and asks for
/// a retry; the retry revokes the result instead of dropping it.
#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_retries_a_result_on_a_database_error(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_exchange(ExchangeScript {
        remote_user_id: Some("u-live".to_owned()),
        ..ExchangeScript::default()
    });
    connected_by(&f, &f.scope).await;
    let tokens = result_tokens(&pool).await;
    let step = OAuthDeletionStep::new(f.service.clone());

    for table in ["account_grants", "platforms"] {
        rename(&pool, table, "broken_table").await;
        let result = step.run(f.owner).await;
        rename(&pool, "broken_table", table).await;
        assert!(
            matches!(result, Err(postit_jobs::JobError::Retry(_))),
            "{table}: {result:?}"
        );
        assert_eq!(state_count(&pool).await, 1, "{table}: the result is kept");
        assert!(f.provider.revoked().is_empty());
    }

    step.run(f.owner).await.unwrap();
    assert_eq!(f.provider.revoked(), vec![sha256_hex(&tokens["u-live"])]);
    assert_eq!(state_count(&pool).await, 0);
}

/// A database failure while the step revokes a grant asks for a retry before the grant is
/// marked `revoked`, so its credential survives for the retry to revoke.
#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_retries_a_grant_on_a_database_error(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-retry", &["a1"]).await;
    let step = OAuthDeletionStep::new(f.service.clone());

    rename(&pool, "platforms", "broken_table").await;
    let result = step.run(f.owner).await;
    rename(&pool, "broken_table", "platforms").await;
    assert!(
        matches!(result, Err(postit_jobs::JobError::Retry(_))),
        "{result:?}"
    );
    assert_eq!(grant_state(&f, grant).await, ("active".to_owned(), true));
    assert!(f.provider.revoked().is_empty());

    step.run(f.owner).await.unwrap();
    assert_eq!(f.provider.revoked(), vec![sha256_hex("r-retry")]);
    assert_eq!(grant_state(&f, grant).await, ("revoked".to_owned(), false));
}

/// A platform that no longer resolves (switched off) is a reason to drop, not to retry.
#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_drops_revokes_of_a_disabled_platform(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) = oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-off", &["a1"]).await;
    f.script.set_exchange(ExchangeScript {
        remote_user_id: Some("u-live".to_owned()),
        ..ExchangeScript::default()
    });
    connected_by(&f, &f.scope).await;
    postit_data::platforms::PlatformRepo::set_enabled(&pool, &platform(OAUTH), false)
        .await
        .unwrap();

    OAuthDeletionStep::new(f.service.clone())
        .run(f.owner)
        .await
        .unwrap();

    assert!(f.provider.revoked().is_empty());
    assert_eq!(state_count(&pool).await, 0);
    assert_eq!(grant_state(&f, grant).await, ("revoked".to_owned(), false));
}

/// A `needs_reconnect` grant does not hold its remote user "in use", so a result of the same
/// remote user is revoked with its own tokens and the grant with its own: two token sets, two
/// calls. On a provider that revokes per token both are needed; on one that revokes per user and
/// app pair the second RFC 7009 call is a harmless no-op.
#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_deletion_step_revokes_both_token_sets_beside_a_needs_reconnect_grant(pool: PgPool) {
    let f = fixture(&pool).await;
    let (grant, _) =
        oauth_grant(&pool, f.owner, OAUTH, Some("mock-user"), "r-stale", &["a1"]).await;
    sqlx::query("UPDATE account_grants SET status = 'needs_reconnect' WHERE id = $1")
        .bind(grant.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    connected_by(&f, &f.scope).await;
    let tokens = result_tokens(&pool).await;

    OAuthDeletionStep::new(f.service.clone())
        .run(f.owner)
        .await
        .unwrap();

    let revoked: BTreeSet<String> = f.provider.revoked().into_iter().collect();
    let expected: BTreeSet<String> = [sha256_hex(&tokens["mock-user"]), sha256_hex("r-stale")]
        .into_iter()
        .collect();
    assert_eq!(revoked, expected);
}
