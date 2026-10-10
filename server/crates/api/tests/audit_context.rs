//! Audit events written for an API request carry the caller's address (trusted-proxy
//! resolved) and the request ID.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const PROXY: [u8; 4] = [10, 0, 0, 2];

async fn app(pool: PgPool) -> TestApp {
    TestApp::start_with(pool, |s| {
        s.trusted_proxies = postit_api::client_ip::parse_trusted_proxies(&["10.0.0.0/8".into()])
            .unwrap_or_default();
    })
    .await
}

/// One request through the proxy `peer`, claiming to come from `client` with request ID `rid`.
async fn send(
    app: &TestApp,
    (peer, client): ([u8; 4], &str),
    rid: Uuid,
    method: Method,
    path: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> StatusCode {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("x-forwarded-for", client)
        .header("x-request-id", rid.to_string());
    let mut req = match body {
        Some(json) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap_or_else(|e| unreachable!("request: {e}"));
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((peer, 1))));
    app.send(req).await.status
}

async fn rows(pool: &PgPool, kind: &str) -> Vec<(Option<String>, Option<Uuid>)> {
    sqlx::query_as("SELECT ip, request_id FROM audit_events WHERE kind = $1 ORDER BY at, id")
        .bind(kind)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| unreachable!("audit: {e}"))
}

async fn only(pool: &PgPool, kind: &str) -> (Option<String>, Option<Uuid>) {
    let mut found = rows(pool, kind).await;
    assert_eq!(found.len(), 1, "{kind}: {found:?}");
    found.remove(0)
}

fn expect(ip: &str, rid: Uuid) -> (Option<String>, Option<Uuid>) {
    (Some(ip.to_string()), Some(rid))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn first_sign_in_records_the_client_ip_and_request_id(pool: PgPool) {
    let app = app(pool.clone()).await;
    let rid = Uuid::now_v7();
    let status = send(
        &app,
        (PROXY, "192.0.2.50"),
        rid,
        Method::GET,
        "/api/v1/me",
        &app.token(ADMIN_SUB),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        only(&pool, "user_provisioned").await,
        expect("192.0.2.50", rid)
    );
    assert_eq!(
        only(&pool, "bootstrap_admin_granted").await,
        expect("192.0.2.50", rid)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn user_admin_actions_record_the_client_ip_and_request_id(pool: PgPool) {
    let app = app(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let ip = "192.0.2.60";
    let call = |method: Method, path: String, body: Option<serde_json::Value>| {
        let (app, admin) = (&app, admin.clone());
        async move {
            let rid = Uuid::now_v7();
            let status = send(app, (PROXY, ip), rid, method, &path, &admin, body).await;
            (status, rid)
        }
    };
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let bob = app
        .call(Method::GET, "/api/v1/me", Some(&app.token("bob")), None)
        .await
        .body["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let user = format!("/api/v1/users/{bob}");

    let (status, rid) = call(
        Method::PATCH,
        user.clone(),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only(&pool, "user_approved").await, expect(ip, rid));

    let (status, rid) = call(
        Method::PATCH,
        user.clone(),
        Some(json!({"status": "disabled"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only(&pool, "user_disabled").await, expect(ip, rid));

    let (status, rid) = call(
        Method::PATCH,
        user.clone(),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only(&pool, "user_enabled").await, expect(ip, rid));

    let (status, rid) = call(Method::PATCH, user.clone(), Some(json!({"role": "admin"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only(&pool, "role_changed").await, expect(ip, rid));

    let (status, rid) = call(Method::DELETE, user, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(only(&pool, "user_deleted").await, expect(ip, rid));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn self_deletion_records_the_client_ip_and_request_id(pool: PgPool) {
    let app = app(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let bob = app.token("bob");
    app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    let listed = app
        .call(
            Method::GET,
            "/api/v1/users?status=pending",
            Some(&admin),
            None,
        )
        .await;
    let id = listed.body["data"][0]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    app.call(
        Method::PATCH,
        &format!("/api/v1/users/{id}"),
        Some(&admin),
        Some(json!({"status": "active"})),
    )
    .await;

    let rid = Uuid::now_v7();
    let status = send(
        &app,
        (PROXY, "192.0.2.70"),
        rid,
        Method::DELETE,
        "/api/v1/me",
        &bob,
        Some(json!({"display_name": "bob"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(only(&pool, "user_deleted").await, expect("192.0.2.70", rid));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn forwarded_for_from_an_untrusted_peer_is_ignored(pool: PgPool) {
    let app = app(pool.clone()).await;
    let rid = Uuid::now_v7();
    let status = send(
        &app,
        ([203, 0, 113, 7], "192.0.2.99"),
        rid,
        Method::GET,
        "/api/v1/me",
        &app.token(ADMIN_SUB),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        only(&pool, "user_provisioned").await,
        expect("203.0.113.7", rid)
    );
}
