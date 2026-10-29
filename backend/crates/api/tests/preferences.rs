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

fn switch<'a>(body: &'a serde_json::Value, kind: &str) -> &'a serde_json::Value {
    body["switches"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["kind"] == kind))
        .unwrap_or_else(|| unreachable!("no switch for {kind}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_returns_every_kind_with_defaults(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app
        .call(Method::GET, PATH, Some(&app.token(ADMIN_SUB)), None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(res.body["timezone"].is_string());
    assert_eq!(res.body["email_notifications"], true);
    assert_eq!(res.body["comment_digest"], false);
    let kinds: Vec<_> = postit_core::NotificationKind::ALL
        .iter()
        .map(postit_core::NotificationKind::as_str)
        .collect();
    let got: Vec<_> = res.body["switches"]
        .as_array()
        .unwrap_or_else(|| unreachable!("switches"))
        .iter()
        .map(|s| s["kind"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(got, kinds);
    let failed = switch(&res.body, "delivery_failed");
    assert_eq!(failed["push"], true);
    assert_eq!(failed["email"], true);
    let published = switch(&res.body, "delivery_published");
    assert!(published["email"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn put_creates_missing_row(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let token = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"].as_str().unwrap_or_default().to_owned();
    sqlx::query("DELETE FROM user_preferences WHERE user_id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let put = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"comment_digest": true})),
        )
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(put.body["comment_digest"], true);
    assert_eq!(put.body["locale"], "en-US");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn put_applies_only_given_fields(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let put = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({
                "locale": "ar",
                "timezone": "Asia/Riyadh",
                "switches": [{"kind": "delivery_failed", "email": false}],
            })),
        )
        .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.body);
    let again = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"email_notifications": false, "switches": [{"kind": "new_comments", "push": false}]})),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body["locale"], "ar");
    assert_eq!(again.body["timezone"], "Asia/Riyadh");
    assert_eq!(again.body["email_notifications"], false);
    assert_eq!(switch(&again.body, "delivery_failed")["email"], false);
    assert_eq!(switch(&again.body, "delivery_failed")["push"], true);
    assert_eq!(switch(&again.body, "new_comments")["push"], false);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn null_fields_are_unchanged(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    app.call(
        Method::PUT,
        PATH,
        Some(&token),
        Some(json!({"locale": "ar", "timezone": "Asia/Kolkata", "comment_digest": true})),
    )
    .await;
    let put = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({
                "locale": null, "timezone": null, "email_notifications": null,
                "comment_digest": null, "switches": null,
            })),
        )
        .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.body);
    assert_eq!(put.body["locale"], "ar");
    assert_eq!(put.body["timezone"], "Asia/Kolkata");
    assert_eq!(put.body["comment_digest"], true);
    assert_eq!(put.body["email_notifications"], true);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_timezone_is_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let before = app.call(Method::GET, PATH, Some(&token), None).await;
    let res = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"timezone": "Mars/Olympus-secret"})),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");
    let text = res.body.to_string();
    assert!(text.contains("timezone"), "{text}");
    assert!(!text.contains("Olympus-secret"), "{text}");
    let after = app.call(Method::GET, PATH, Some(&token), None).await;
    assert_eq!(after.body["timezone"], before.body["timezone"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn email_switch_on_info_kind_is_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let res = app
        .call(
            Method::PUT,
            PATH,
            Some(&token),
            Some(json!({"switches": [{"kind": "delivery_published", "email": true}]})),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");
    assert!(res.body.to_string().contains("switches"));
}

#[test]
fn locale_is_an_enum_in_openapi() {
    let doc = serde_json::to_value(postit_api::openapi::openapi())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let schemas = &doc["components"]["schemas"];
    assert_eq!(schemas["LocaleDto"]["enum"], json!(["en-US", "ar"]));
    let update = &schemas["UpdatePreferences"]["properties"]["locale"];
    assert!(update.to_string().contains("LocaleDto"), "{update}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_is_owner_only(pool: PgPool) {
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
    for (method, body) in [
        (Method::GET, Body::empty()),
        (
            Method::PUT,
            Body::from(json!({"comment_digest": true}).to_string()),
        ),
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
}
