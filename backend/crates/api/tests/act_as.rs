//! `X-Postit-Act-As`: `Scope` resolves it against a live grant, every other authenticated
//! extractor refuses it with `owner_only`, and every route without an auth extractor refuses
//! it in a route layer.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::UserId;
use postit_data::delegations::{DelegationRepo, EndedReason};
use postit_data::testkit::{grant, seed_user};
use postit_data::{AccountLimit, Level};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const ACT_AS: &str = "x-postit-act-as";

/// Signs `sub` in, approves them through the admin API, and returns their token and ID.
async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| unreachable!("no id: {:?}", me.body));
    let approved = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    (token, UserId::from(id))
}

fn request(method: &Method, path: &str, token: Option<&str>, act_as: &[&str]) -> Request<Body> {
    let mut b = Request::builder().method(method.clone()).uri(path);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for value in act_as {
        b = b.header(ACT_AS, *value);
    }
    let body = if [Method::PUT, Method::POST, Method::PATCH].contains(method) {
        b = b.header(header::CONTENT_TYPE, "application/json");
        Body::from(json!({ "locale": "ar" }).to_string())
    } else {
        Body::empty()
    };
    b.body(body).unwrap_or_else(|e| unreachable!("{e}"))
}

fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .unwrap_or_else(|e| unreachable!("png: {e}"));
    out.into_inner()
}

async fn upload(app: &TestApp, token: &str) -> String {
    let bytes = tiny_png();
    let req = Request::post("/api/v1/media?filename=a.png")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let res = app.send(req).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap_or_default().to_owned()
}

fn without_request_id(res: &TestResponse) -> serde_json::Value {
    let mut body = res.body.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("request_id");
        object.remove("instance");
    }
    body
}

fn assert_code(res: &TestResponse, status: StatusCode, code: &str, what: &str) {
    assert_eq!(res.status, status, "{what}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{what}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn own_scope_without_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app
        .call(
            Method::GET,
            "/api/v1/accounts",
            Some(&app.token(ADMIN_SUB)),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn valid_header_acts_in_owner_workspace(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let media = upload(&app, &alice).await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;

    let header = alice_id.to_string();
    let acting = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&header],
        ))
        .await;
    assert_eq!(acting.status, StatusCode::OK, "{:?}", acting.body);
    assert_eq!(acting.body["data"][0]["id"], media.as_str());

    let own = app
        .call(Method::GET, "/api/v1/media", Some(&bob), None)
        .await;
    assert_eq!(own.status, StatusCode::OK);
    assert_eq!(own.body["data"].as_array().map_or(0, Vec::len), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn uppercase_uuid_header_is_accepted(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;
    let header = alice_id.to_string().to_uppercase();
    let res = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&header],
        ))
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_headers_are_delegation_invalid_with_identical_bodies(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let (_, carol_id) = active_user(&app, "carol").await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;
    let revoked = grant(&pool, carol_id, bob_id, Level::View, AccountLimit::All).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    DelegationRepo::end(
        &mut conn,
        revoked,
        EndedReason::Revoked,
        Some(carol_id),
        chrono::Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let pending = seed_user(&pool, "pending").await;

    let alice = alice_id.to_string();
    let spaced = format!(" {alice}");
    let braced = format!("{{{alice}}}");
    let urn = format!("urn:uuid:{alice}");
    let simple = alice.replace('-', "");
    let own = bob_id.to_string();
    let unknown = Uuid::now_v7().to_string();
    let pending = pending.to_string();
    let carol = carol_id.to_string();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("not a uuid", vec!["not-a-uuid"]),
        ("empty", vec![""]),
        ("leading space", vec![&spaced]),
        ("braced", vec![&braced]),
        ("urn", vec![&urn]),
        ("unhyphenated", vec![&simple]),
        ("two values", vec![&alice, &alice]),
        ("own id", vec![&own]),
        ("unknown id", vec![&unknown]),
        ("pending user", vec![&pending]),
        ("revoked grant", vec![&carol]),
    ];
    let mut first: Option<serde_json::Value> = None;
    for (what, values) in cases {
        let res = app
            .send(request(&Method::GET, "/api/v1/media", Some(&bob), &values))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "delegation_invalid", what);
        assert!(res.body["detail"].is_null(), "{what}: detail leaked");
        let body = without_request_id(&res);
        match &first {
            None => first = Some(body),
            Some(expected) => assert_eq!(&body, expected, "{what}"),
        }
    }
    // The valid grant still resolves, so the refusals above are about the header alone.
    let ok = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&alice],
        ))
        .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn self_routes_refuse_header_as_owner_only(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    grant(&pool, alice_id, bob_id, Level::Publish, AccountLimit::All).await;
    let header = alice_id.to_string();
    for (method, path) in [
        (Method::GET, "/api/v1/me"),
        (Method::GET, "/api/v1/me/preferences"),
        (Method::PUT, "/api/v1/me/preferences"),
    ] {
        let what = format!("{method} {path}");
        let res = app
            .send(request(&method, path, Some(&bob), &[&header]))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
    }
    let prefs = app
        .call(Method::GET, "/api/v1/me/preferences", Some(&bob), None)
        .await;
    assert_eq!(
        prefs.body["locale"], "en-US",
        "the refused PUT changed nothing"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_routes_refuse_header_as_owner_only(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let admin_id = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await
        .body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .map_or_else(|| unreachable!("admin id"), UserId::from);
    let (_, alice_id) = active_user(&app, "alice").await;
    grant(&pool, alice_id, admin_id, Level::Publish, AccountLimit::All).await;
    let header = alice_id.to_string();
    for path in ["/api/v1/admin/jobs", "/api/v1/users", "/api/v1/admin/audit"] {
        let res = app
            .send(request(&Method::GET, path, Some(&admin), &[&header]))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", path);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_token_with_header_is_unauthenticated(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    for path in ["/api/v1/media", "/api/v1/me", "/api/v1/admin/jobs"] {
        let res = app
            .send(request(&Method::GET, path, Some("not-a-jwt"), &[&header]))
            .await;
        assert_code(&res, StatusCode::UNAUTHORIZED, "unauthenticated", path);
        let anon = app
            .send(request(&Method::GET, path, None, &[&header]))
            .await;
        assert_code(&anon, StatusCode::UNAUTHORIZED, "unauthenticated", path);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_routes_refuse_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    let media = format!("/public/media/{}", Uuid::now_v7());
    for (method, path) in [
        (Method::GET, "/health"),
        (Method::GET, "/ready"),
        (Method::GET, "/api/v1/auth/config"),
        (Method::GET, "/api/openapi.json"),
        (Method::GET, "/docs/"),
        (Method::GET, media.as_str()),
        (Method::GET, "/api/v1/_test/sleep?ms=1"),
        (Method::POST, "/api/v1/_test/echo"),
        (Method::GET, "/api/v1/_test/marked"),
        (Method::GET, "/api/v1/_test/raised/sleep?ms=1"),
        (Method::POST, "/api/v1/_test/raised/echo"),
    ] {
        let what = format!("{method} {path}");
        let res = app.send(request(&method, path, None, &[&header])).await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
    }
    // Without the header they still answer.
    let health = app.call(Method::GET, "/health", None, None).await;
    assert_eq!(health.status, StatusCode::OK);
}

/// Every operation in the contract with no `security` requirement has no auth extractor, so
/// only the route layer can refuse the header. A new unauthenticated route that misses the
/// layer fails here.
#[sqlx::test(migrations = "../data/migrations")]
async fn every_unsecured_route_refuses_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    let spec: serde_json::Value = serde_json::from_str(&postit_api::openapi::openapi_json_pretty())
        .unwrap_or_else(|e| unreachable!("spec: {e}"));
    let paths = spec["paths"]
        .as_object()
        .unwrap_or_else(|| unreachable!("no paths"));
    let placeholder = Uuid::now_v7().to_string();
    let mut checked = 0;
    for (path, item) in paths {
        let concrete = fill_params(path, &placeholder);
        for method in ["get", "post", "put", "patch", "delete", "head"] {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let secured = operation
                .get("security")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|s| !s.is_empty());
            if secured {
                continue;
            }
            let m = Method::from_bytes(method.to_uppercase().as_bytes())
                .unwrap_or_else(|e| unreachable!("{e}"));
            let res = app.send(request(&m, &concrete, None, &[&header])).await;
            assert_eq!(res.status, StatusCode::FORBIDDEN, "{method} {path}");
            if method != "head" {
                assert_eq!(res.body["code"], "owner_only", "{method} {path}");
            }
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} unsecured operations found");
}

fn fill_params(path: &str, placeholder: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        out.push_str(placeholder);
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}
