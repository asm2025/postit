//! The VAPID key and push endpoint routes: self only, push off answers `push_disabled`, and
//! endpoints are validated, replaced on re-registration, and private to their user.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::UserId;
use postit_data::push_endpoints::PushEndpointRepo;
use postit_notify::ece::TestSubscriber;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &str = "/api/v1/push/vapid-public-key";
const ENDPOINTS: &str = "/api/v1/me/push-endpoints";
const ACT_AS: &str = "x-postit-act-as";

async fn active_user(app: &TestApp, sub: &str) -> (String, Uuid) {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| unreachable!("no id: {:?}", me.body));
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    (token, id)
}

async fn act_as(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    owner: Uuid,
    body: Option<Value>,
) -> TestResponse {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(ACT_AS, owner.to_string());
    let body = match body {
        Some(json) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    app.send(b.body(body).unwrap_or_else(|e| unreachable!("{e}")))
        .await
}

fn device(endpoint: &str) -> Value {
    let sub = TestSubscriber::generate();
    json!({
        "endpoint": endpoint,
        "p256dh": sub.public_b64url(),
        "auth": sub.auth_b64url(),
        "platform": "web",
        "device_label": "Laptop",
    })
}

async fn register(app: &TestApp, token: &str, body: Value) -> TestResponse {
    app.call(Method::POST, ENDPOINTS, Some(token), Some(body))
        .await
}

async fn rows(app: &TestApp, user: Uuid) -> usize {
    let mut conn = app
        .pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    PushEndpointRepo::list_for_user(&mut conn, UserId::from_uuid(user))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .len()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn vapid_key_404_when_disabled(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (token, _) = active_user(&app, "alice").await;
    let res = app.call(Method::GET, KEY, Some(&token), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.body["code"], "push_disabled");
    let res = register(&app, &token, device("https://push.example.test/a")).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.body["code"], "push_disabled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn vapid_key_is_served_when_enabled(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (token, _) = active_user(&app, "alice").await;
    let res = app.call(Method::GET, KEY, Some(&token), None).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let key = res.body["key"].as_str().unwrap_or_default();
    assert_eq!(key.len(), 87, "{key}");
    assert!(key.starts_with('B'), "uncompressed point: {key}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn register_validates_keys(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (token, user) = active_user(&app, "alice").await;
    let good = device("https://push.example.test/send/abc");
    let res = register(&app, &token, good.clone()).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert!(
        res.body["id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .is_some()
    );

    let short_key = {
        let mut b = good.clone();
        b["p256dh"] = json!("BAAA");
        b
    };
    // 65 bytes starting with 0x04 that are not a point on the curve.
    let off_curve = {
        let mut b = good.clone();
        b["p256dh"] = json!(format!("BA{}", "A".repeat(85)));
        b
    };
    let bad_auth = {
        let mut b = good.clone();
        b["auth"] = json!("AAAA");
        b
    };
    let not_base64 = {
        let mut b = good.clone();
        b["auth"] = json!("not base64!");
        b
    };
    let mut cases = vec![
        (short_key, "p256dh"),
        (off_curve, "p256dh"),
        (bad_auth, "auth"),
        (not_base64, "auth"),
    ];
    for endpoint in [
        "ftp://push.example.test/a",
        "not a url",
        "https://user:pw@push.example.test/a",
        "http://push.example.test/a",
        // A loopback literal the outbound address policy refuses.
        "https://127.0.0.1/a",
    ] {
        let mut b = good.clone();
        b["endpoint"] = json!(endpoint);
        cases.push((b, "endpoint"));
    }
    for (body, field) in cases {
        let res = register(&app, &token, body.clone()).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(res.body["code"], "push_endpoint_invalid", "{body}");
        assert_eq!(res.body["field"], field, "{body}");
    }

    let mut long_label = good.clone();
    long_label["device_label"] = json!("x".repeat(101));
    let res = register(&app, &token, long_label).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");

    let mut bad_platform = good;
    bad_platform["platform"] = json!("windows");
    let res = register(&app, &token, bad_platform).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(rows(&app, user).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn http_endpoint_needs_allow_private(pool: PgPool) {
    let strict = TestApp::start_with_push(pool, false).await;
    let (token, _) = active_user(&strict, "alice").await;
    let res = register(&strict, &token, device("http://192.168.1.20:8080/up/abc")).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "push_endpoint_invalid");
    assert_eq!(res.body["field"], "endpoint");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn http_endpoint_is_accepted_with_allow_private(pool: PgPool) {
    let lan = TestApp::start_with_push(pool, true).await;
    let (token, _) = active_user(&lan, "bob").await;
    let res = register(&lan, &token, device("http://192.168.1.20:8080/up/abc")).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn re_register_replaces(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (token, user) = active_user(&app, "alice").await;
    let first = register(&app, &token, device("https://push.example.test/same")).await;
    assert_eq!(first.status, StatusCode::CREATED);
    let mut again = device("https://push.example.test/same");
    again["device_label"] = json!("Work laptop");
    again["platform"] = json!("android");
    let second = register(&app, &token, again).await;
    assert_eq!(second.status, StatusCode::CREATED);
    assert_eq!(first.body["id"], second.body["id"]);
    assert_eq!(rows(&app, user).await, 1);
    let mut conn = app
        .pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stored = PushEndpointRepo::list_for_user(&mut conn, UserId::from_uuid(user))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(stored[0].device_label.as_deref(), Some("Work laptop"));

    // Another user registering the same endpoint gets a row of their own.
    let (other, other_id) = active_user(&app, "bob").await;
    let res = register(&app, &other, device("https://push.example.test/same")).await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_ne!(res.body["id"], first.body["id"]);
    assert_eq!(rows(&app, other_id).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_other_users_endpoint_is_404(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, _) = active_user(&app, "bob").await;
    let res = register(&app, &alice, device("https://push.example.test/alice")).await;
    let id = res.body["id"].as_str().unwrap_or_default().to_owned();
    let path = format!("{ENDPOINTS}/{id}");

    let res = app.call(Method::DELETE, &path, Some(&bob), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.body["code"], "not_found");
    assert_eq!(rows(&app, alice_id).await, 1);

    let res = app.call(Method::DELETE, &path, Some(&alice), None).await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert!(res.bytes.is_empty());
    assert_eq!(rows(&app, alice_id).await, 0);
    let res = app.call(Method::DELETE, &path, Some(&alice), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_is_owner_only(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let (delegate, delegate_id) = active_user(&app, "delegate").await;
    let res = app
        .call(
            Method::POST,
            "/api/v1/delegations",
            Some(&owner),
            Some(json!({ "delegate_id": delegate_id, "level": "publish" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let res = register(&app, &owner, device("https://push.example.test/owner")).await;
    let id = res.body["id"].as_str().unwrap_or_default().to_owned();

    for (method, path, body) in [
        (Method::GET, KEY.to_owned(), None),
        (
            Method::POST,
            ENDPOINTS.to_owned(),
            Some(device("https://push.example.test/delegate")),
        ),
        (Method::DELETE, format!("{ENDPOINTS}/{id}"), None),
    ] {
        let res = act_as(&app, method, &path, &delegate, owner_id, body).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(res.body["code"], "owner_only", "{path}");
    }
    assert_eq!(rows(&app, owner_id).await, 1);
    assert_eq!(rows(&app, delegate_id).await, 0);
}

/// Four callers: the owner, a delegate at each level, a delegate limited to no accounts, and
/// an unrelated user. Devices are per user: each registers and removes only their own, and
/// nobody reaches the owner's.
#[sqlx::test(migrations = "../data/migrations")]
async fn devices_are_private_to_each_caller(pool: PgPool) {
    let app = TestApp::start_with_push(pool, false).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let mut others = Vec::new();
    for level in ["view", "edit", "publish"] {
        let (token, id) = active_user(&app, level).await;
        let res = app
            .call(
                Method::POST,
                "/api/v1/delegations",
                Some(&owner),
                Some(json!({ "delegate_id": id, "level": level })),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        others.push((token, id));
    }
    let (limited, limited_id) = active_user(&app, "limited").await;
    let res = app
        .call(
            Method::POST,
            "/api/v1/delegations",
            Some(&owner),
            Some(json!({ "delegate_id": limited_id, "level": "publish", "account_ids": [] })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    others.push((limited, limited_id));
    others.push(active_user(&app, "stranger").await);

    let res = register(&app, &owner, device("https://push.example.test/owner")).await;
    let owner_device = format!(
        "{ENDPOINTS}/{}",
        res.body["id"].as_str().unwrap_or_default()
    );
    for (i, (token, id)) in others.iter().enumerate() {
        let res = app
            .call(Method::DELETE, &owner_device, Some(token), None)
            .await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "caller {i}");
        let res = register(
            &app,
            token,
            device(&format!("https://push.example.test/own-{i}")),
        )
        .await;
        assert_eq!(res.status, StatusCode::CREATED, "caller {i}");
        assert_eq!(rows(&app, *id).await, 1, "caller {i}");
        let mine = format!(
            "{ENDPOINTS}/{}",
            res.body["id"].as_str().unwrap_or_default()
        );
        let res = app.call(Method::DELETE, &mine, Some(token), None).await;
        assert_eq!(res.status, StatusCode::NO_CONTENT, "caller {i}");
    }
    assert_eq!(rows(&app, owner_id).await, 1);
}
