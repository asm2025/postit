//! The delegation, workspace, and lookup responses and logs carry no bearer token and no
//! email. Its own test binary: tracing's callsite-interest cache is process-wide, so a
//! log-capturing test cannot share a binary with others.

use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn active_user(app: &TestApp, sub: &str) -> (String, Uuid) {
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
    (token, id)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegation_responses_leak_no_secrets(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let (logs, guard) = LogCapture::install();

    let mut bodies: Vec<(String, String)> = Vec::new();
    let mut record = |label: &str, res: &postit_api::testkit::TestResponse| {
        bodies.push((
            label.to_owned(),
            String::from_utf8_lossy(&res.bytes).into_owned(),
        ));
    };
    let created = app
        .call(
            Method::POST,
            "/api/v1/delegations",
            Some(&alice),
            Some(json!({ "delegate_id": bob_id, "level": "edit" })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    record("grant", &created);
    let id = created.body["id"].as_str().unwrap_or_default().to_owned();
    let calls = [
        (Method::GET, "/api/v1/delegations".to_owned(), &alice, None),
        (
            Method::PATCH,
            format!("/api/v1/delegations/{id}"),
            &alice,
            Some(json!({ "level": "view", "expires_at": null })),
        ),
        (
            Method::GET,
            "/api/v1/delegations/received".to_owned(),
            &bob,
            None,
        ),
        (Method::GET, "/api/v1/me/workspaces".to_owned(), &bob, None),
        (
            Method::GET,
            "/api/v1/users/lookup?email=bob%40postit.test".to_owned(),
            &alice,
            None,
        ),
        (
            Method::GET,
            "/api/v1/users/lookup?email=nobody%40postit.test".to_owned(),
            &alice,
            None,
        ),
        (
            Method::DELETE,
            format!("/api/v1/delegations/received/{id}"),
            &bob,
            None,
        ),
        (
            Method::GET,
            "/api/v1/delegations?include_ended=true".to_owned(),
            &alice,
            None,
        ),
    ];
    for (method, path, token, body) in calls {
        let label = format!("{method} {path}");
        let res = app.call(method, &path, Some(token), body).await;
        assert!(res.status.is_success(), "{label}: {:?}", res.body);
        record(&label, &res);
    }
    drop(guard);

    let log_text = logs.text();
    let mut outputs: Vec<(&str, &str)> = bodies
        .iter()
        .map(|(l, b)| (l.as_str(), b.as_str()))
        .collect();
    outputs.push(("logs", log_text.as_str()));
    assert_no_leak(
        &[
            alice.as_str(),
            bob.as_str(),
            "alice@postit.test",
            "bob@postit.test",
            "nobody@postit.test",
            "nobody%40postit.test",
        ],
        &outputs,
    );
}
