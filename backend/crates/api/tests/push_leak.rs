//! A refused push registration names the field at fault, never the endpoint or a key, in the
//! problem body or the logs. Its own binary because it captures logs.

use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_notify::ece::TestSubscriber;
use serde_json::json;
use sqlx::PgPool;

const ENDPOINT: &str = "https://push.example.test/send/very-secret-endpoint-token-77aa";

#[sqlx::test(migrations = "../data/migrations")]
async fn push_routes_never_echo_the_endpoint_or_keys(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let app = TestApp::start_with_push(pool, false).await;
    let token = app.token("alice");
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"].as_str().unwrap_or_default().to_owned();
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);

    let device = TestSubscriber::generate();
    let (p256dh, auth) = (device.public_b64url(), device.auth_b64url());
    let bad_point = format!("BA{}", "A".repeat(85));
    let http_endpoint = ENDPOINT.replace("https:", "http:");
    let mut bodies = String::new();
    for body in [
        json!({ "endpoint": ENDPOINT, "p256dh": bad_point, "auth": auth, "platform": "web", "device_label": "x" }),
        json!({ "endpoint": ENDPOINT, "p256dh": p256dh, "auth": "c2hvcnQ", "platform": "web", "device_label": "x" }),
        json!({ "endpoint": http_endpoint, "p256dh": p256dh, "auth": auth, "platform": "web", "device_label": "x" }),
        json!({ "endpoint": ENDPOINT, "p256dh": p256dh, "auth": auth, "platform": "web", "device_label": "x".repeat(200) }),
        json!({ "endpoint": ENDPOINT, "p256dh": p256dh, "auth": auth, "platform": "nope", "device_label": "x" }),
    ] {
        let res = app
            .call(
                Method::POST,
                "/api/v1/me/push-endpoints",
                Some(&token),
                Some(body),
            )
            .await;
        assert_eq!(
            res.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{:?}",
            res.body
        );
        bodies.push_str(&String::from_utf8_lossy(&res.bytes));
    }
    let ok = app
        .call(
            Method::POST,
            "/api/v1/me/push-endpoints",
            Some(&token),
            Some(json!({ "endpoint": ENDPOINT, "p256dh": p256dh, "auth": auth, "platform": "web", "device_label": "x" })),
        )
        .await;
    assert_eq!(ok.status, StatusCode::CREATED);
    bodies.push_str(&String::from_utf8_lossy(&ok.bytes));

    let logs = logs.text();
    assert_no_leak(
        &[
            ENDPOINT,
            "very-secret-endpoint-token-77aa",
            &p256dh,
            &auth,
            &bad_point,
            postit_api::testkit::TEST_VAPID_PRIVATE_KEY,
        ],
        &[("problem bodies", &bodies), ("logs", &logs)],
    );
}
