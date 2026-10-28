//! Starting an OAuth connection, the provider's callback, and reading the pending result,
//! against the mock OAuth provider and `mock_oauth`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use common::{
    SECRET, mock_agent, oauth_agent, oauth_config, oauth_provider, platform, platforms, values,
};
use emixcrypto::HashAlgorithm as _;
use postit_agent_kit::testkit::{MockOAuthProvider, OAuthFailure};
use postit_agent_mock::{DiscoveryScript, ExchangeScript, MockScript};
use postit_config::Environment;
use postit_core::{OAuthStateId, PlatformId, UserId};
use postit_data::oauth_states::OAuthStateRepo;
use postit_data::testkit::{callers, seed_user};
use postit_data::{OwnerScope, ScopeError};
use postit_oauth::testkit::{
    TestOptions, account_service_with, test_accounts_settings, test_vault,
};
use postit_oauth::{AccountError, AccountService, CallbackParams, ConnectKind, ReturnToAllowlist};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";
const APP: &str = "https://postit.local:44315";
const DONE: &str = "https://postit.local:44315/oauth-done";

struct Fixture {
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

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, options()).await
}

async fn fixture_with(pool: &PgPool, options: TestOptions) -> Fixture {
    platforms(pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let (agent, script) = oauth_agent(&provider, OAUTH);
    let service =
        account_service_with(pool.clone(), vec![agent], test_accounts_settings(), options);
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        provider,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn sha256(text: &str) -> Vec<u8> {
    emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(text.as_bytes())
        .unwrap()
}

/// Starts a connection and has the provider approve it: the `state` and the code to call back
/// with.
async fn approved(f: &Fixture) -> (String, String) {
    let start = f
        .service
        .start_oauth(&f.scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    let code = f.provider.approve(&start.authorize_url);
    (query(&start.authorize_url, "state").unwrap(), code)
}

fn params(code: Option<&str>, state: Option<&str>, error: Option<&str>) -> CallbackParams {
    CallbackParams {
        code: code.map(|c| SecretString::from(c.to_owned())),
        state: state.map(str::to_owned),
        error: error.map(str::to_owned),
    }
}

async fn callback(f: &Fixture, state: &str, code: &str) -> Url {
    f.service
        .oauth_callback(OAUTH, params(Some(code), Some(state), None))
        .await
}

fn assert_error(target: &Url, base: &str, code: &str) {
    let mut without_query = target.clone();
    without_query.set_query(None);
    assert_eq!(without_query.as_str(), base, "redirect base of {target}");
    assert_eq!(query(target, "result").as_deref(), Some("error"));
    assert_eq!(query(target, "code").as_deref(), Some(code));
    assert!(query(target, "connect_id").is_none());
    assert_eq!(
        target.query_pairs().count(),
        2,
        "only result and code: {target}"
    );
}

async fn state_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_states")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn grant_and_account_rows(pool: &PgPool) -> (i64, i64) {
    let grants = sqlx::query_scalar("SELECT count(*) FROM account_grants")
        .fetch_one(pool)
        .await
        .unwrap();
    let accounts = sqlx::query_scalar("SELECT count(*) FROM connected_accounts")
        .fetch_one(pool)
        .await
        .unwrap();
    (grants, accounts)
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    state_hash: Vec<u8>,
    user_id: Uuid,
    pkce_verifier: Option<Vec<u8>>,
    return_to: Option<String>,
    connect_id_hash: Option<Vec<u8>>,
    grant_tokens: Option<Vec<u8>>,
    discovered: Option<Value>,
    remote_user_id: Option<String>,
    expired: bool,
}

async fn only_row(pool: &PgPool) -> Row {
    sqlx::query_as(
        "SELECT id, state_hash, user_id, pkce_verifier, return_to, connect_id_hash,                 grant_tokens, discovered, remote_user_id, expires_at <= now() AS expired          FROM oauth_states",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The row's sealed `grant_tokens` opened as `OAuthStateRepo` sealed them.
fn open_tokens(row: &Row) -> Value {
    let sealed: postit_vault::Sealed<Value> =
        postit_vault::Sealed::from_bytes(row.grant_tokens.clone().unwrap()).unwrap();
    test_vault()
        .open(
            &sealed,
            &OAuthStateRepo::seal_context(OAuthStateId::from(row.id), "grant_tokens"),
        )
        .unwrap()
}

fn bytes_contain(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_returns_authorize_url_and_stores_hashed_state(pool: PgPool) {
    let f = fixture(&pool).await;
    let before = Utc::now();
    let start = f
        .service
        .start_oauth(&f.scope, &platform(OAUTH), DONE)
        .await
        .unwrap();

    let url = &start.authorize_url;
    assert_eq!(url.path(), "/authorize");
    assert_eq!(query(url, "client_id").as_deref(), Some(common::CLIENT_ID));
    assert_eq!(
        query(url, "redirect_uri").as_deref(),
        Some("https://postit.test/api/v1/platforms/mock_oauth/oauth/callback")
    );
    assert_eq!(query(url, "code_challenge_method").as_deref(), Some("S256"));
    assert!(query(url, "client_secret").is_none());
    assert!(!url.as_str().contains(common::CLIENT_SECRET));
    let state = query(url, "state").unwrap();
    assert_eq!(URL_SAFE_NO_PAD.decode(&state).unwrap().len(), 32);
    let ttl = start.expires_at - before;
    assert!(
        ttl > chrono::Duration::minutes(9)
            && ttl <= chrono::Duration::minutes(10) + chrono::Duration::seconds(5)
    );

    let row = only_row(&pool).await;
    assert_eq!(row.state_hash, sha256(&state));
    assert_eq!(row.user_id, f.owner.as_uuid());
    assert_eq!(row.return_to.as_deref(), Some(DONE));
    assert!(row.connect_id_hash.is_none() && row.grant_tokens.is_none());
    // The state is stored only as its hash; the verifier only sealed.
    let verifier_bytes = row.pkce_verifier.clone().unwrap();
    assert!(!bytes_contain(&verifier_bytes, &state));
    let sealed: postit_vault::Sealed<Value> =
        postit_vault::Sealed::from_bytes(verifier_bytes).unwrap();
    let verifier: Value = test_vault()
        .open(
            &sealed,
            &OAuthStateRepo::seal_context(OAuthStateId::from(row.id), "pkce_verifier"),
        )
        .unwrap();
    let verifier = verifier.as_str().unwrap();
    assert!(!bytes_contain(&row.pkce_verifier.unwrap(), verifier));
    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(verifier.as_bytes())
        .unwrap();
    assert_eq!(
        query(url, "code_challenge").unwrap(),
        URL_SAFE_NO_PAD.encode(digest)
    );
    let stored_state: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_states WHERE return_to = $1 OR encode(state_hash, 'escape') = $1",
    )
    .bind(&state)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored_state, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_refuses_delegate_and_credentials_platform_and_unconfigured_platform(pool: PgPool) {
    platforms(&pool, &[OAUTH, "mock", "mock_oauth_b"]).await;
    let provider = oauth_provider().await;
    let (agent, _) = oauth_agent(&provider, OAUTH);
    let (unconfigured, _) = oauth_agent(&provider, "mock_oauth_b");
    let (credentials, _) = mock_agent("mock");
    let mut options = options();
    options.configs.insert(platform("mock_oauth_b"), None);
    let service = account_service_with(
        pool.clone(),
        vec![agent, unconfigured, credentials],
        test_accounts_settings(),
        options,
    );
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;

    for delegate in [&who.view, &who.edit, &who.publish] {
        let started = service.start_oauth(delegate, &platform(OAUTH), DONE).await;
        assert!(
            matches!(started, Err(AccountError::Scope(ScopeError::OwnerOnly))),
            "{started:?}"
        );
    }
    let started = service
        .start_oauth(&who.owner, &platform("mock"), DONE)
        .await;
    assert!(
        matches!(started, Err(AccountError::ConnectMethodUnsupported)),
        "{started:?}"
    );
    let started = service
        .start_oauth(&who.owner, &platform("mock_oauth_b"), DONE)
        .await;
    assert!(
        matches!(started, Err(AccountError::PlatformUnavailable)),
        "{started:?}"
    );
    assert_eq!(state_rows(&pool).await, 0);
}

#[test]
fn return_to_allowlist_builds_exact_entries() {
    let extra = vec![
        "https://extra.example/back".to_owned(),
        "https://Example.com/done".to_owned(),
    ];
    for app in [APP, "https://postit.local:44315/"] {
        let list =
            ReturnToAllowlist::new(&Url::parse(app).unwrap(), Environment::Development, &extra);
        assert_eq!(list.web().as_str(), DONE);
        assert!(list.allows(DONE));
        assert!(!list.allows("https://postit.local:44315//oauth-done"));
        assert!(list.allows("postit-dev://oauth-done"));
        assert!(list.allows("https://extra.example/back"));
        assert!(list.allows("https://example.com/done"));
    }
}

fn allowlist_options() -> TestOptions {
    TestOptions {
        return_to: ReturnToAllowlist::new(
            &Url::parse(APP).unwrap(),
            Environment::Development,
            &[
                "https://extra.example/back".to_owned(),
                "https://Example.com/done".to_owned(),
            ],
        ),
        ..options()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn return_to_allowlist_accepts_exact_entries(pool: PgPool) {
    let f = fixture_with(&pool, allowlist_options()).await;
    for candidate in [
        DONE,
        "postit-dev://oauth-done",
        "https://extra.example/back",
        "https://example.com/done",
    ] {
        let started = f
            .service
            .start_oauth(&f.scope, &platform(OAUTH), candidate)
            .await;
        assert!(started.is_ok(), "{candidate}: {started:?}");
    }
    assert_eq!(state_rows(&pool).await, 4);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn return_to_near_misses_are_rejected(pool: PgPool) {
    let f = fixture_with(&pool, allowlist_options()).await;
    for candidate in [
        "https://postit.local:44315/oauth-done/",
        "https://postit.local:44315/oauth-done?x=1",
        "https://postit.local:44315/oauth-done#a",
        "HTTPS://POSTIT.LOCAL:44315/oauth-done",
        "https://postit.local:44315/%2Foauth-done",
        "https://postit.local:44315%2Foauth-done",
        "postit://oauth-done",
        "postit-qa://oauth-done",
        "https://evil.example/oauth-done",
        "https://Example.com/done",
        "",
    ] {
        let started = f
            .service
            .start_oauth(&f.scope, &platform(OAUTH), candidate)
            .await;
        assert!(
            matches!(started, Err(AccountError::ReturnToInvalid)),
            "{candidate}: {started:?}"
        );
    }
    assert_eq!(state_rows(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_success_redirects_with_connect_id(pool: PgPool) {
    let f = fixture(&pool).await;
    let (state, code) = approved(&f).await;
    let target = callback(&f, &state, &code).await;

    let mut bare = target.clone();
    bare.set_query(None);
    assert_eq!(bare.as_str(), DONE);
    assert_eq!(query(&target, "result").as_deref(), Some("ok"));
    let connect_id = query(&target, "connect_id").unwrap();
    assert_eq!(connect_id.len(), 43);
    assert_eq!(target.query_pairs().count(), 2);
    assert!(!target.as_str().contains(&state));
    assert!(!target.as_str().contains(&code));

    let row = only_row(&pool).await;
    assert_eq!(
        row.connect_id_hash.as_deref(),
        Some(sha256(&connect_id).as_slice())
    );
    assert!(!row.expired);
    assert_eq!(row.remote_user_id.as_deref(), Some("mock-user"));
    let discovered = row.discovered.clone().unwrap();
    assert_eq!(discovered.as_array().unwrap().len(), 1);
    let grant_bytes = row.grant_tokens.clone().unwrap();
    let tokens = open_tokens(&row);
    let access = tokens["access_token"].as_str().unwrap();
    assert!(!access.is_empty());
    assert!(!bytes_contain(&grant_bytes, access));
    assert_eq!(tokens["remote_user_id"].as_str(), Some("mock-user"));
    assert!(tokens["refresh_token"].is_string());
    assert!(tokens["account_tokens"].is_object());
    // The provider's response named no `scope` (the mock's default): it granted what was
    // asked, so the spec's requested scopes are stored.
    assert_eq!(tokens["scopes"], serde_json::json!(["read", "write"]));
    assert_eq!(grant_and_account_rows(&pool).await, (0, 0));
    assert_eq!(f.script.exchange_calls(), 1);
    assert_eq!(f.provider.token_requests(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_unknown_expired_and_wrong_platform_state_redirect_state_invalid(pool: PgPool) {
    let f = fixture(&pool).await;

    // Unknown and missing state.
    let target = callback(&f, "not-a-state", "code").await;
    assert_error(&target, DONE, "state_invalid");
    let target = f
        .service
        .oauth_callback(OAUTH, params(Some("code"), None, None))
        .await;
    assert_error(&target, DONE, "state_invalid");

    // Expired.
    let (state, code) = approved(&f).await;
    sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    assert_error(&callback(&f, &state, &code).await, DONE, "state_invalid");
    assert_eq!(state_rows(&pool).await, 0);

    // Another platform's callback.
    let (state, code) = approved(&f).await;
    let target = f
        .service
        .oauth_callback("mock", params(Some(&code), Some(&state), None))
        .await;
    assert_error(&target, DONE, "state_invalid");
    assert_eq!(state_rows(&pool).await, 0);
    assert_eq!(f.provider.token_requests(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_replay_does_not_touch_the_pending_result(pool: PgPool) {
    let f = fixture(&pool).await;
    let (state, code) = approved(&f).await;
    let first = callback(&f, &state, &code).await;
    let connect_id = query(&first, "connect_id").unwrap();
    let before = only_row(&pool).await;

    let replay = callback(&f, &state, &code).await;
    assert_error(&replay, DONE, "state_invalid");
    let after = only_row(&pool).await;
    assert_eq!(after.connect_id_hash, before.connect_id_hash);
    assert_eq!(after.grant_tokens, before.grant_tokens);
    assert!(!after.expired);
    assert_eq!(f.provider.token_requests(), 1);
    let view = f.service.pending(&f.scope, &connect_id).await.unwrap();
    assert_eq!(view.discovered.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_with_error_and_code_never_exchanges(pool: PgPool) {
    let f = fixture(&pool).await;
    for (error, expected) in [
        ("access_denied", "access_denied"),
        ("temporarily_unavailable", "provider_error"),
    ] {
        let (state, code) = approved(&f).await;
        let target = f
            .service
            .oauth_callback(OAUTH, params(Some(&code), Some(&state), Some(error)))
            .await;
        assert_error(&target, DONE, expected);
        assert_eq!(state_rows(&pool).await, 0);
    }
    // No code and no error is a provider error too.
    let (state, _) = approved(&f).await;
    let target = f
        .service
        .oauth_callback(OAUTH, params(None, Some(&state), None))
        .await;
    assert_error(&target, DONE, "provider_error");
    assert_eq!(state_rows(&pool).await, 0);
    assert_eq!(f.provider.token_requests(), 0);
    assert_eq!(f.script.exchange_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_exchange_failure(pool: PgPool) {
    let f = fixture(&pool).await;
    let (state, code) = approved(&f).await;
    f.provider.fail_next(OAuthFailure::InvalidGrant);
    assert_error(&callback(&f, &state, &code).await, DONE, "exchange_failed");
    assert_eq!(state_rows(&pool).await, 0);
    assert_eq!(f.script.exchange_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_platform_switched_off_mid_flow(pool: PgPool) {
    let f = fixture(&pool).await;
    let (state, code) = approved(&f).await;
    postit_data::platforms::PlatformRepo::set_enabled(&pool, &platform(OAUTH), false)
        .await
        .unwrap();
    assert_error(
        &callback(&f, &state, &code).await,
        DONE,
        "platform_unavailable",
    );
    assert_eq!(state_rows(&pool).await, 0);
    assert_eq!(f.provider.token_requests(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_past_deadline_redirects(pool: PgPool) {
    let f = fixture_with(
        &pool,
        TestOptions {
            callback_deadline: Duration::from_millis(200),
            ..options()
        },
    )
    .await;
    let (state, code) = approved(&f).await;
    f.provider.set_delay(Some(Duration::from_secs(1)));
    let started = std::time::Instant::now();
    let target = callback(&f, &state, &code).await;
    assert!(started.elapsed() < Duration::from_millis(900));
    assert_error(&target, DONE, "exchange_failed");
    assert_eq!(state_rows(&pool).await, 0);
}

/// A callback that failed after a successful exchange left only a discarded row holding the
/// sealed tokens, for the purge to revoke.
async fn assert_discarded(f: &Fixture, pool: &PgPool, remote_user_id: Option<&str>) {
    let row = only_row(pool).await;
    assert!(row.connect_id_hash.is_none());
    assert!(row.expired);
    assert!(row.discovered.is_none());
    assert_eq!(row.remote_user_id.as_deref(), remote_user_id);
    let tokens = open_tokens(&row);
    assert!(tokens["access_token"].is_string());
    assert_eq!(tokens["remote_user_id"].as_str(), remote_user_id);
    assert_eq!(grant_and_account_rows(pool).await, (0, 0));
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_scope_missing(pool: PgPool) {
    let f = fixture(&pool).await;
    f.provider.set_granted_scopes(Some(vec!["read".to_owned()]));
    let (state, code) = approved(&f).await;
    assert_error(&callback(&f, &state, &code).await, DONE, "scope_missing");
    assert_discarded(&f, &pool, Some("mock-user")).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_discovery_failure(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_exchange(ExchangeScript {
        fail: true,
        ..ExchangeScript::default()
    });
    let (state, code) = approved(&f).await;
    assert_error(&callback(&f, &state, &code).await, DONE, "discovery_failed");
    // The hook failed, so no remote user ID came back.
    assert_discarded(&f, &pool, None).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_no_accounts(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(0);
    let (state, code) = approved(&f).await;
    assert_error(&callback(&f, &state, &code).await, DONE, "no_accounts");
    assert_discarded(&f, &pool, Some("mock-user")).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_errors_redirect_to_the_mobile_return_to(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(0);
    let start = f
        .service
        .start_oauth(&f.scope, &platform(OAUTH), "postit-dev://oauth-done")
        .await
        .unwrap();
    let code = f.provider.approve(&start.authorize_url);
    let state = query(&start.authorize_url, "state").unwrap();
    let target = callback(&f, &state, &code).await;
    assert_error(&target, "postit-dev://oauth-done", "no_accounts");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_discovery_fails_after_the_hook_keeps_refreshed_tokens(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_discovery(DiscoveryScript {
        refresh_to: Some("access-refreshed".to_owned()),
        fail: true,
        ..DiscoveryScript::default()
    });
    let (state, code) = approved(&f).await;
    assert_error(&callback(&f, &state, &code).await, DONE, "discovery_failed");
    assert_discarded(&f, &pool, Some("mock-user")).await;
    let row = only_row(&pool).await;
    assert_eq!(
        open_tokens(&row)["access_token"].as_str(),
        Some("access-refreshed")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_deadline_mid_discovery_keeps_the_refreshed_tokens(pool: PgPool) {
    let f = fixture_with(
        &pool,
        TestOptions {
            callback_deadline: Duration::from_millis(300),
            ..options()
        },
    )
    .await;
    // The agent refreshed (and the provider may have rotated the old tokens away) before the
    // deadline fired: the tokens kept for the purge must be the refreshed ones.
    f.script.set_oauth_discovery(DiscoveryScript {
        refresh_to: Some("access-refreshed".to_owned()),
        delay: Some(Duration::from_secs(5)),
        fail: false,
    });
    let (state, code) = approved(&f).await;
    let started = std::time::Instant::now();
    let target = callback(&f, &state, &code).await;
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_error(&target, DONE, "discovery_failed");
    assert_discarded(&f, &pool, Some("mock-user")).await;
    let row = only_row(&pool).await;
    assert_eq!(
        open_tokens(&row)["access_token"].as_str(),
        Some("access-refreshed")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_dropped_mid_discovery_still_discards_the_tokens(pool: PgPool) {
    let f = fixture_with(
        &pool,
        TestOptions {
            callback_deadline: Duration::from_millis(800),
            ..options()
        },
    )
    .await;
    f.script.set_oauth_discovery(DiscoveryScript {
        refresh_to: Some("access-refreshed".to_owned()),
        delay: Some(Duration::from_secs(5)),
        fail: false,
    });
    let (state, code) = approved(&f).await;
    // The request goes away (client disconnect, request timeout) while discovery runs: the
    // callback future is dropped after the exchange.
    let dropped =
        tokio::time::timeout(Duration::from_millis(300), callback(&f, &state, &code)).await;
    assert!(dropped.is_err(), "the callback was still running");
    assert_eq!(f.provider.token_requests(), 1);

    // The work goes on without it and reaches the deadline: the tokens are discarded for the
    // purge to revoke, never dropped with the request.
    let started = std::time::Instant::now();
    loop {
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM oauth_states WHERE grant_tokens IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        if held == 1 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "no discarded row after the callback was dropped"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_discarded(&f, &pool, Some("mock-user")).await;
    let row = only_row(&pool).await;
    assert_eq!(
        open_tokens(&row)["access_token"].as_str(),
        Some("access-refreshed")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_outliving_the_state_ttl_keeps_its_row_from_the_purge(pool: PgPool) {
    let f = fixture(&pool).await;
    let (state, code) = approved(&f).await;
    // The state has a second left when the callback consumes it, and the exchange takes
    // longer than that.
    sqlx::query("UPDATE oauth_states SET expires_at = now() + interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    f.provider.set_delay(Some(Duration::from_millis(2500)));
    let purge = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let mut c = pool.acquire().await.unwrap();
        OAuthStateRepo::purge_expired(&mut c, Utc::now())
            .await
            .unwrap()
    };
    let (target, purged) = tokio::join!(callback(&f, &state, &code), purge);
    assert_eq!(purged, 0, "the consumed row outlives its callback");
    assert_eq!(query(&target, "result").as_deref(), Some("ok"), "{target}");
    let row = only_row(&pool).await;
    assert!(row.grant_tokens.is_some());
}

async fn connected(f: &Fixture) -> String {
    let (state, code) = approved(f).await;
    query(&callback(f, &state, &code).await, "connect_id").unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_read_by_owner_does_not_consume(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    let connect_id = connected(&f).await;
    for _ in 0..2 {
        let view = f.service.pending(&f.scope, &connect_id).await.unwrap();
        assert_eq!(view.platform, PlatformId::new(OAUTH).unwrap());
        assert_eq!(view.connect, ConnectKind::OAuth);
        assert!(view.expires_at > Utc::now());
        let ids: Vec<&str> = view
            .discovered
            .iter()
            .map(|d| d.remote_id.as_str())
            .collect();
        assert_eq!(ids, ["mock-account-oauth-1", "mock-account-oauth-2"]);
    }
    let row = only_row(&pool).await;
    assert!(row.connect_id_hash.is_some() && !row.expired);

    // A delegate is refused before anything is read.
    let who = callers(&pool, f.owner, &[]).await;
    let read = f.service.pending(&who.publish, &connect_id).await;
    assert!(
        matches!(read, Err(AccountError::Scope(ScopeError::OwnerOnly))),
        "{read:?}"
    );
    assert!(f.service.pending(&f.scope, &connect_id).await.is_ok());

    // Unknown and expired IDs.
    let unknown = f.service.pending(&f.scope, "unknown").await;
    assert!(
        matches!(unknown, Err(AccountError::NotFound)),
        "{unknown:?}"
    );
    sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let expired = f.service.pending(&f.scope, &connect_id).await;
    assert!(
        matches!(expired, Err(AccountError::NotFound)),
        "{expired:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_read_by_other_user_discards_and_404s(pool: PgPool) {
    let f = fixture(&pool).await;
    let connect_id = connected(&f).await;
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);

    let read = f.service.pending(&stranger, &connect_id).await;
    assert!(matches!(read, Err(AccountError::NotFound)), "{read:?}");
    let read = f.service.pending(&f.scope, &connect_id).await;
    assert!(matches!(read, Err(AccountError::NotFound)), "{read:?}");
    let row = only_row(&pool).await;
    assert!(row.connect_id_hash.is_none());
    assert!(row.expired);
    assert!(row.grant_tokens.is_some());
    assert_eq!(row.remote_user_id.as_deref(), Some("mock-user"));
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_read_works_for_credential_connections(pool: PgPool) {
    platforms(&pool, &["mock"]).await;
    let (agent, _) = mock_agent("mock");
    let service = account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        TestOptions::default(),
    );
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let check = service
        .start_credentials(&scope, &platform("mock"), values(SECRET))
        .await
        .unwrap();
    let (connect_id, expires_at) = check.pending.unwrap();
    let connect_id = connect_id.as_str().to_owned();

    let view = service.pending(&scope, &connect_id).await.unwrap();
    assert_eq!(view.platform, platform("mock"));
    assert_eq!(view.connect, ConnectKind::Credentials);
    // The database keeps microseconds.
    assert_eq!(
        view.expires_at.timestamp_micros(),
        expires_at.timestamp_micros()
    );
    assert_eq!(view.discovered, check.discovered);
    assert!(service.pending(&scope, &connect_id).await.is_ok());

    let stranger = OwnerScope::own(seed_user(&pool, "active").await);
    let read = service.pending(&stranger, &connect_id).await;
    assert!(matches!(read, Err(AccountError::NotFound)), "{read:?}");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let read = service.pending(&scope, &connect_id).await;
    assert!(matches!(read, Err(AccountError::NotFound)), "{read:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_fallback_is_the_web_error_page(pool: PgPool) {
    let f = fixture(&pool).await;
    assert_error(
        &f.service.oauth_callback_fallback(),
        DONE,
        "exchange_failed",
    );
}
