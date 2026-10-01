use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_config::RateBucket;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn auth_config_is_public(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app
        .call(Method::GET, "/api/v1/auth/config", None, None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["client_id"], "postit-app");
    assert!(
        res.body["issuer"]
            .as_str()
            .is_some_and(|i| i.starts_with("http"))
    );
    assert!(res.body["scopes"].as_array().is_some_and(|s| !s.is_empty()));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn me_requires_a_token(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let challenge = |res: &postit_api::testkit::TestResponse| {
        res.headers
            .get(axum::http::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let res = app.call(Method::GET, "/api/v1/me", None, None).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(res.body["code"], "unauthenticated");
    assert_eq!(
        challenge(&res).as_deref(),
        Some("Bearer"),
        "no token: no error code"
    );
    let bad = app
        .call(Method::GET, "/api/v1/me", Some("garbage"), None)
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        challenge(&bad).as_deref(),
        Some("Bearer error=\"invalid_token\"")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bootstrap_admin_and_pending_member_see_their_own_profile(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app
        .call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None)
        .await;
    assert_eq!(admin.status, StatusCode::OK);
    assert_eq!(admin.body["role"], "admin");
    assert_eq!(admin.body["status"], "active");
    assert_eq!(admin.body["email"], "admin@postit.test");

    let member = app
        .call(Method::GET, "/api/v1/me", Some(&app.token("bob")), None)
        .await;
    assert_eq!(member.status, StatusCode::OK);
    assert_eq!(member.body["status"], "pending");
    assert!(member.body.get("account_url").is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_pending_user_cannot_delete_themselves(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token("bob");
    let res = app
        .call(
            Method::DELETE,
            "/api/v1/me",
            Some(&token),
            Some(serde_json::json!({ "display_name": "bob" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "account_pending");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_me_checks_the_display_name_and_the_last_admin(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let wrong = app
        .call(
            Method::DELETE,
            "/api/v1/me",
            Some(&admin),
            Some(serde_json::json!({ "display_name": "nope" })),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(wrong.body["code"], "validation_failed");

    let last = app
        .call(
            Method::DELETE,
            "/api/v1/me",
            Some(&admin),
            Some(serde_json::json!({ "display_name": ADMIN_SUB })),
        )
        .await;
    assert_eq!(last.status, StatusCode::CONFLICT);
    assert_eq!(last.body["code"], "last_admin");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_tokens_are_charged_to_the_ip_and_create_no_user(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
    })
    .await;
    for _ in 0..2 {
        assert_eq!(
            app.call(Method::GET, "/api/v1/me", Some("bad.token.here"), None)
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    let limited = app
        .call(Method::GET, "/api/v1/me", Some("bad.token.here"), None)
        .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(users, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_users_behind_one_ip_have_separate_buckets(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.authenticated = RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let (a, b) = (app.token(ADMIN_SUB), app.token("bob"));
    for _ in 0..2 {
        assert_eq!(
            app.call(Method::GET, "/api/v1/me", Some(&a), None)
                .await
                .status,
            StatusCode::OK
        );
        assert_eq!(
            app.call(Method::GET, "/api/v1/me", Some(&b), None)
                .await
                .status,
            StatusCode::OK
        );
    }
    assert_eq!(
        app.call(Method::GET, "/api/v1/me", Some(&a), None)
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_provisioning_bucket_trips_before_a_row_is_created(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), |s| {
        s.rate_limit.provisioning = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    assert_eq!(
        app.call(Method::GET, "/api/v1/me", Some(&app.token("u1")), None)
            .await
            .status,
        StatusCode::OK
    );
    let limited = app
        .call(Method::GET, "/api/v1/me", Some(&app.token("u2")), None)
        .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(users, 1);
    // An existing user is never charged to the provisioning bucket.
    assert_eq!(
        app.call(Method::GET, "/api/v1/me", Some(&app.token("u1")), None)
            .await
            .status,
        StatusCode::OK
    );
}
