use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;

const PATH: &str = "/api/v1/me/preferences";

#[sqlx::test(migrations = "../data/migrations")]
async fn get_returns_default_locale(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app
        .call(Method::GET, PATH, Some(&app.token(ADMIN_SUB)), None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn put_sets_locale_and_get_reflects_it(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let put = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"locale": "ar"})),
        )
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(put.body["locale"], "ar");
    let get = app.call(Method::GET, PATH, Some(&token), None).await;
    assert_eq!(get.body["locale"], "ar");
    let back = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"locale": "en-US"})),
        )
        .await;
    assert_eq!(back.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn put_rejects_unsupported_locale(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    for tag in ["fr", "en"] {
        let res = app
            .call(
                Method::PUT,
                PATH,
                Some(&token),
                Some(json!({"locale": tag})),
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{tag}");
        assert_eq!(res.body["code"], "validation_failed", "{tag}");
    }
    let get = app.call(Method::GET, PATH, Some(&token), None).await;
    assert_eq!(get.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_user_can_read_and_change(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let bob = app.token("bob");
    let me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(me.body["status"], "pending");
    let get = app.call(Method::GET, PATH, Some(&bob), None).await;
    assert_eq!(get.status, StatusCode::OK);
    let put = app
        .call(Method::PUT, PATH, Some(&bob), Some(json!({"locale": "ar"})))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(put.body["locale"], "ar");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_user_is_rejected(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = app.token("bob");
    let me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    let id = me.body["id"].as_str().unwrap_or_default().to_owned();
    let path = format!("/api/v1/users/{id}");
    for status in ["active", "disabled"] {
        let res = app
            .call(
                Method::PATCH,
                &path,
                Some(&admin),
                Some(json!({ "status": status })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{status}");
    }
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(json!({"locale": "ar"}))),
    ] {
        let res = app.call(method, PATH, Some(&bob), body).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN);
        assert_eq!(res.body["code"], "account_disabled");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn users_only_see_their_own(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (a, b) = (app.token(ADMIN_SUB), app.token("bob"));
    let put = app
        .call(Method::PUT, PATH, Some(&a), Some(json!({"locale": "ar"})))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    let theirs = app.call(Method::GET, PATH, Some(&b), None).await;
    assert_eq!(theirs.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_header_does_not_reach_another_user(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = app.token("bob");
    let bob_id = app
        .call(Method::GET, "/api/v1/me", Some(&bob), None)
        .await
        .body["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    // Preferences are the caller's own: the header is refused, exactly as for `GET /me`.
    for (method, body) in [
        (Method::GET, Body::empty()),
        (Method::PUT, Body::from(json!({"locale": "ar"}).to_string())),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(PATH)
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-postit-act-as", &bob_id)
            .body(body)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let res = app.send(req).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN);
        assert_eq!(res.body["code"], "owner_only");
    }
    let theirs = app.call(Method::GET, PATH, Some(&bob), None).await;
    assert_eq!(theirs.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_is_401(pool: PgPool) {
    let app = TestApp::start(pool).await;
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(json!({"locale": "ar"}))),
    ] {
        let res = app.call(method, PATH, None, body).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED);
        assert_eq!(res.body["code"], "unauthenticated");
    }
}
