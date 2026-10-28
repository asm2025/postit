//! The OAuth routes: starting a connection, the provider's callback, reading the pending
//! result, confirming it, and platform events, over HTTP against the mock OAuth provider.
#![allow(clippy::unwrap_used)]

mod oauth_support;

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use oauth_support::{
    ACT_AS, CREDENTIALS, DONE, Fixture, OAUTH, active_user, authorize, authorize_url, call_as,
    callback, callback_path, deauthorize, event, fixture, fixture_with, location, pending_result,
    query, sign, start_as,
};
use postit_core::{AccountId, UserId};
use postit_data::testkit::grant;
use postit_data::{AccountLimit, Level};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn header(res: &postit_api::testkit::TestResponse, name: header::HeaderName) -> &str {
    res.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

fn assert_code(
    res: &postit_api::testkit::TestResponse,
    status: StatusCode,
    code: &str,
    what: &str,
) {
    assert_eq!(res.status, status, "{what}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{what}");
}

async fn pending_read(
    f: &Fixture,
    connect_id: &str,
    token: &str,
    owner: Option<UserId>,
) -> postit_api::testkit::TestResponse {
    call_as(
        &f.app,
        Method::GET,
        &format!("/api/v1/accounts/connect/{connect_id}"),
        token,
        owner,
        None,
    )
    .await
}

async fn confirm(
    f: &Fixture,
    connect_id: &str,
    token: &str,
    owner: Option<UserId>,
) -> postit_api::testkit::TestResponse {
    call_as(
        &f.app,
        Method::POST,
        &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
        token,
        owner,
        Some(json!({})),
    )
    .await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn full_flow_over_http(pool: PgPool) {
    let f = fixture(pool).await;
    f.script.set_oauth_accounts(2);

    let started = start_as(&f.app, &f.owner, None, DONE).await;
    let url = authorize_url(&started);
    assert!(started.body["expires_at"].is_string(), "{:?}", started.body);
    assert_eq!(query(&url, "client_id").as_deref(), Some("postit-app"));
    let code = f.provider.approve(&url);
    let state = query(&url, "state").unwrap();

    let res = callback(&f.app, &[("code", &code), ("state", &state)]).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(header(&res, header::CACHE_CONTROL), "no-store");
    assert_eq!(header(&res, header::REFERRER_POLICY), "no-referrer");
    let redirect = location(&res);
    let mut base = redirect.clone();
    base.set_query(None);
    assert_eq!(base.as_str(), DONE);
    assert_eq!(query(&redirect, "result").as_deref(), Some("ok"));
    let connect_id = query(&redirect, "connect_id").unwrap();

    let pending = pending_read(&f, &connect_id, &f.owner, None).await;
    assert_eq!(pending.status, StatusCode::OK, "{:?}", pending.body);
    assert_eq!(pending.body["platform"], OAUTH);
    assert_eq!(pending.body["connect"], "oauth");
    assert!(pending.body["expires_at"].is_string());
    let ids: Vec<&str> = pending.body["discovered"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["remote_id"].as_str())
        .collect();
    assert_eq!(ids, ["mock-account-oauth-1", "mock-account-oauth-2"]);
    // Reading does not consume.
    let again = pending_read(&f, &connect_id, &f.owner, None).await;
    assert_eq!(again.status, StatusCode::OK);

    let confirmed = confirm(&f, &connect_id, &f.owner, None).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
    assert_eq!(
        confirmed.body["connected"].as_array().map_or(0, Vec::len),
        2
    );
    assert_eq!(
        confirmed.body["connected"][0]["account"]["connect"],
        "oauth"
    );

    let listed = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.owner), None)
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["total"], 2);
    assert!(
        listed.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["platform"] == OAUTH && a["status"] == "active")
    );

    // The result is gone once confirmed.
    let gone = pending_read(&f, &connect_id, &f.owner, None).await;
    assert_code(
        &gone,
        StatusCode::NOT_FOUND,
        "not_found",
        "read after confirm",
    );
}

/// Owner, a delegate at each level, a delegate limited to another account, and a stranger.
struct World {
    f: Fixture,
    delegates: Vec<(&'static str, String)>,
    stranger: (String, UserId),
}

async fn world(pool: PgPool) -> World {
    let f = fixture(pool.clone()).await;
    f.script.set_oauth_accounts(2);
    let connect_id = pending_result(&f, &f.owner).await;
    let confirmed = confirm(&f, &connect_id, &f.owner, None).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
    let b = confirmed.body["connected"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["account"]["remote_account_id"] == "mock-account-oauth-2")
        .and_then(|c| c["account"]["id"].as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap();

    let mut delegates = Vec::new();
    for (sub, level, limit) in [
        ("view", Level::View, AccountLimit::All),
        ("edit", Level::Edit, AccountLimit::All),
        ("publish", Level::Publish, AccountLimit::All),
        (
            "limited",
            Level::Publish,
            AccountLimit::only([AccountId::from(b)]),
        ),
    ] {
        let (token, id) = active_user(&f.app, sub).await;
        grant(&pool, f.owner_id, id, level, limit).await;
        delegates.push((sub, token));
    }
    let stranger = active_user(&f.app, "stranger").await;
    World {
        f,
        delegates,
        stranger,
    }
}

async fn states_of(pool: &PgPool, user: UserId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_states WHERE user_id = $1")
        .bind(user.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn isolation_owner_may_start_read_and_confirm(pool: PgPool) {
    let w = world(pool).await;
    let f = &w.f;
    let started = start_as(&f.app, &f.owner, None, DONE).await;
    assert_eq!(started.status, StatusCode::OK, "{:?}", started.body);
    let connect_id = pending_result(f, &f.owner).await;
    let read = pending_read(f, &connect_id, &f.owner, None).await;
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    let confirmed = confirm(f, &connect_id, &f.owner, None).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn isolation_every_delegate_is_owner_only(pool: PgPool) {
    let w = world(pool).await;
    let f = &w.f;
    for (who, token) in &w.delegates {
        let owner = Some(f.owner_id);
        let before = states_of(&f.pool, f.owner_id).await;
        let started = start_as(&f.app, token, owner, DONE).await;
        assert_code(
            &started,
            StatusCode::FORBIDDEN,
            "owner_only",
            &format!("{who} start"),
        );
        assert_eq!(
            states_of(&f.pool, f.owner_id).await,
            before,
            "{who} started nothing"
        );

        let connect_id = pending_result(f, &f.owner).await;
        let read = pending_read(f, &connect_id, token, owner).await;
        assert_code(
            &read,
            StatusCode::FORBIDDEN,
            "owner_only",
            &format!("{who} read"),
        );
        let confirmed = confirm(f, &connect_id, token, owner).await;
        assert_code(
            &confirmed,
            StatusCode::FORBIDDEN,
            "owner_only",
            &format!("{who} confirm"),
        );
        // The refusals left the owner's result alone.
        let mine = pending_read(f, &connect_id, &f.owner, None).await;
        assert_eq!(mine.status, StatusCode::OK, "{who}: {:?}", mine.body);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn isolation_unrelated_user_gets_404_and_starts_in_their_own_workspace(pool: PgPool) {
    let w = world(pool).await;
    let f = &w.f;
    let (stranger, stranger_id) = (&w.stranger.0, w.stranger.1);

    let connect_id = pending_result(f, &f.owner).await;
    let read = pending_read(f, &connect_id, stranger, None).await;
    assert_code(&read, StatusCode::NOT_FOUND, "not_found", "stranger read");
    // A foreign read ends the result.
    let mine = pending_read(f, &connect_id, &f.owner, None).await;
    assert_code(
        &mine,
        StatusCode::NOT_FOUND,
        "not_found",
        "owner after stranger",
    );

    let connect_id = pending_result(f, &f.owner).await;
    let confirmed = confirm(f, &connect_id, stranger, None).await;
    assert_code(
        &confirmed,
        StatusCode::NOT_FOUND,
        "not_found",
        "stranger confirm",
    );

    // Acting as the owner without a grant is not a way in either.
    let acting = start_as(&f.app, stranger, Some(f.owner_id), DONE).await;
    assert_code(
        &acting,
        StatusCode::FORBIDDEN,
        "delegation_invalid",
        "stranger act-as",
    );

    let owner_states = states_of(&f.pool, f.owner_id).await;
    let started = start_as(&f.app, stranger, None, DONE).await;
    assert_eq!(started.status, StatusCode::OK, "{:?}", started.body);
    assert_eq!(states_of(&f.pool, stranger_id).await, 1);
    assert_eq!(states_of(&f.pool, f.owner_id).await, owner_states);

    // The stranger's own connection lands in their workspace only: of the three accounts the
    // authorization reaches, the owner already has two, so only the third is theirs.
    f.script.set_oauth_accounts(3);
    let theirs = pending_result(f, stranger).await;
    let confirmed = confirm(f, &theirs, stranger, None).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
    assert_eq!(
        confirmed.body["skipped_elsewhere"], 2,
        "{:?}",
        confirmed.body
    );
    let stranger_list = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(stranger), None)
        .await;
    assert_eq!(stranger_list.body["total"], 1, "{:?}", stranger_list.body);
    assert_eq!(
        stranger_list.body["data"][0]["remote_account_id"],
        "mock-account-oauth-3"
    );
    let owner_list = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.owner), None)
        .await;
    assert_eq!(owner_list.body["total"], 2, "{:?}", owner_list.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn return_to_invalid_is_422(pool: PgPool) {
    let f = fixture(pool).await;
    for return_to in [
        "https://evil.test/oauth-done",
        "https://postit.local:44315/oauth-done/",
        "https://postit.local:44315/oauth-done?x=1",
        "",
    ] {
        let res = start_as(&f.app, &f.owner, None, return_to).await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "return_to_invalid",
            return_to,
        );
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_states")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_on_a_credentials_platform_is_capability_unsupported(pool: PgPool) {
    let f = fixture(pool).await;
    let res = call_as(
        &f.app,
        Method::POST,
        &format!("/api/v1/accounts/connect/{CREDENTIALS}"),
        &f.owner,
        None,
        Some(json!({ "return_to": DONE })),
    )
    .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "capability_unsupported",
        "credentials",
    );
    let res = call_as(
        &f.app,
        Method::POST,
        "/api/v1/accounts/connect/nope",
        &f.owner,
        None,
        Some(json!({ "return_to": DONE })),
    )
    .await;
    assert_code(&res, StatusCode::NOT_FOUND, "not_found", "unknown platform");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_needs_no_bearer_and_ignores_act_as(pool: PgPool) {
    let f = fixture(pool).await;
    let url = authorize_url(&start_as(&f.app, &f.owner, None, DONE).await);
    let code = f.provider.approve(&url);
    let state = query(&url, "state").unwrap();

    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(callback_path(&[("code", &code), ("state", &state)]))
        .header(ACT_AS, Uuid::now_v7().to_string())
        .body(axum::body::Body::empty())
        .unwrap();
    let res = f.app.send(req).await;
    let redirect = location(&res);
    assert_eq!(
        query(&redirect, "result").as_deref(),
        Some("ok"),
        "{redirect}"
    );
    let connect_id = query(&redirect, "connect_id").unwrap();
    // The result belongs to the user who started it.
    let read = pending_read(&f, &connect_id, &f.owner, None).await;
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);

    // A replay, an unknown state, and an extra provider parameter all still redirect.
    let replay = callback(&f.app, &[("code", &code), ("state", &state)]).await;
    let target = location(&replay);
    assert_eq!(query(&target, "code").as_deref(), Some("state_invalid"));
    let unknown = callback(&f.app, &[("state", "nope"), ("iss", "x")]).await;
    assert_eq!(
        query(&location(&unknown), "code").as_deref(),
        Some("state_invalid")
    );
    let empty = f
        .app
        .call(
            Method::GET,
            &format!("/api/v1/platforms/{OAUTH}/oauth/callback"),
            None,
            None,
        )
        .await;
    assert_eq!(
        query(&location(&empty), "code").as_deref(),
        Some("state_invalid")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_with_a_denial_redirects_with_access_denied(pool: PgPool) {
    let f = fixture(pool).await;
    let url = authorize_url(&start_as(&f.app, &f.owner, None, DONE).await);
    let state = query(&url, "state").unwrap();
    let res = callback(
        &f.app,
        &[
            ("error", "access_denied"),
            ("error_description", "The user said no"),
            ("state", &state),
        ],
    )
    .await;
    let target = location(&res);
    assert_eq!(query(&target, "result").as_deref(), Some("error"));
    assert_eq!(query(&target, "code").as_deref(), Some("access_denied"));
    assert!(!target.as_str().contains("said"), "{target}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn callback_outlives_the_default_request_timeout(pool: PgPool) {
    let f = fixture_with(
        pool,
        |s| s.request_timeout = Duration::from_secs(1),
        Duration::from_secs(2),
    )
    .await;
    // The default timeout applies elsewhere.
    let slow = f
        .app
        .call(Method::GET, "/api/v1/_test/sleep?ms=1500", None, None)
        .await;
    assert_eq!(slow.status, StatusCode::REQUEST_TIMEOUT);

    f.provider.set_delay(Some(Duration::from_millis(1500)));
    let res = authorize(&f, &f.owner).await;
    let redirect = location(&res);
    assert_eq!(
        query(&redirect, "result").as_deref(),
        Some("ok"),
        "{redirect}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_applies_deauthorization(pool: PgPool) {
    let f = fixture(pool).await;
    let connect_id = pending_result(&f, &f.owner).await;
    let confirmed = confirm(&f, &connect_id, &f.owner, None).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);

    let (body, signature) = deauthorize("mock-user");
    let res = event(&f.app, OAUTH, "deauthorize", body, Some(&signature)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert!(res.bytes.is_empty());

    let listed = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.owner), None)
        .await;
    assert_eq!(listed.body["total"], 1);
    assert_eq!(listed.body["data"][0]["status"], "revoked");
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind = 'account_revoked_by_platform'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_rejects_bad_signature_with_401_and_empty_detail(pool: PgPool) {
    let f = fixture(pool).await;
    let connect_id = pending_result(&f, &f.owner).await;
    confirm(&f, &connect_id, &f.owner, None).await;

    let (body, _) = deauthorize("mock-user");
    let wrong = sign(&body, "not-the-secret");
    for signature in [Some(wrong.as_str()), None] {
        let res = event(&f.app, OAUTH, "deauthorize", body.clone(), signature).await;
        assert_code(
            &res,
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "bad signature",
        );
        assert!(res.body["detail"].is_null(), "{:?}", res.body);
    }
    let listed = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.owner), None)
        .await;
    assert_eq!(listed.body["data"][0]["status"], "active");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_body_over_limit_is_413(pool: PgPool) {
    let f = fixture(pool).await;
    let body = vec![b' '; 64 * 1024 + 1];
    let signature = sign(&body, oauth_support::CLIENT_SECRET);
    let res = event(&f.app, OAUTH, "deauthorize", body, Some(&signature)).await;
    assert_code(
        &res,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
        "over limit",
    );

    // Exactly the limit is read and handed to the agent (which finds no user in it).
    let body = vec![b' '; 64 * 1024];
    let signature = sign(&body, oauth_support::CLIENT_SECRET);
    let res = event(&f.app, OAUTH, "deauthorize", body, Some(&signature)).await;
    assert_ne!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{:?}", res.body);
}

/// A body the platform signed but the agent cannot read is the sender's fault: a 4xx, never a
/// 503 that invites the platform to send it again.
#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_malformed_signed_body_is_a_client_error(pool: PgPool) {
    let f = fixture(pool).await;
    for body in [b"not json".to_vec(), b"{\"other\": 1}".to_vec()] {
        let signature = sign(&body, oauth_support::CLIENT_SECRET);
        let res = event(&f.app, OAUTH, "deauthorize", body, Some(&signature)).await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "malformed",
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_unknown_platform_or_kind_is_404(pool: PgPool) {
    let f = fixture(pool).await;
    let (body, signature) = deauthorize("mock-user");
    for (platform, kind) in [
        ("nope", "deauthorize"),
        ("Not_A_Platform", "deauthorize"),
        (OAUTH, "something_else"),
        (CREDENTIALS, "deauthorize"),
    ] {
        let res = event(&f.app, platform, kind, body.clone(), Some(&signature)).await;
        assert_code(
            &res,
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("{platform}/{kind}"),
        );
    }
}

/// A registered OAuth platform the deployment has not configured looks like an unknown one.
#[sqlx::test(migrations = "../data/migrations")]
async fn events_route_unconfigured_oauth_platform_is_404(pool: PgPool) {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    let provider = postit_agent_kit::testkit::MockOAuthProvider::start().await;
    let bare = oauth_support::platform("mock_oauth_bare");
    let agent =
        postit_agent_mock::MockAgent::oauth(&provider.base_url()).with_platform(bare.clone());
    let mut configs = BTreeMap::new();
    configs.insert(bare.clone(), None);
    let app = postit_api::testkit::TestApp::start_with_oauth(
        pool.clone(),
        vec![Arc::new(agent) as Arc<dyn postit_core::Agent>],
        |_| {},
        postit_oauth::testkit::TestOptions {
            configs,
            ..postit_oauth::testkit::TestOptions::default()
        },
    )
    .await;
    postit_data::platforms::PlatformRepo::upsert_registered(
        &pool,
        &[(bare.clone(), "Bare OAuth".to_owned())],
    )
    .await
    .unwrap();

    let (body, signature) = deauthorize("mock-user");
    let res = event(&app, bare.as_str(), "deauthorize", body, Some(&signature)).await;
    assert_code(&res, StatusCode::NOT_FOUND, "not_found", "unconfigured");
}
