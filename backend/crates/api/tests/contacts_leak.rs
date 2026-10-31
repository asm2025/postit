//! No contact address reaches a log line, at any level, on any contacts route. Its own test
//! binary: tracing's callsite-interest cache is process-wide, so a log-capturing test cannot
//! share a binary with others.
#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TEST_EMAIL_SIGNING_KEY, TestApp};
use postit_core::UserId;
use postit_data::OwnerScope;
use postit_data::contacts::{SuppressionReason, SuppressionRepo};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const BOUNDARY: &str = "leak-boundary";

async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = Uuid::parse_str(me.body["id"].as_str().unwrap()).unwrap();
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

fn import_request(token: &str, csv: &str) -> Request<Body> {
    let options = json!({
        "attest_consent": true,
        "columns": { "email": "email", "consent_source": "consent", "first_name": "name" }
    });
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"options\"\r\n\
         Content-Type: application/json\r\n\r\n{options}\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"c.csv\"\r\n\
         Content-Type: text/csv\r\n\r\n{csv}\r\n--{BOUNDARY}--\r\n"
    );
    Request::post("/api/v1/contacts/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn contacts_leak(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let (logs, guard) = LogCapture::install();

    let created = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({ "email": "secret.ada@example.com", "first_name": "Adalind", "consent_source": "signup form" })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let id = created.body["id"].as_str().unwrap().to_owned();
    let refused = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({ "email": "broken.address@", "consent_source": "x" })),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut conn = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut conn,
        &OwnerScope::own(owner_id),
        TEST_EMAIL_SIGNING_KEY,
        "secret.ada@example.com",
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    drop(conn);

    for (method, path, body) in [
        (Method::GET, "/api/v1/contacts".to_owned(), None),
        (Method::GET, format!("/api/v1/contacts/{id}"), None),
        (
            Method::PATCH,
            format!("/api/v1/contacts/{id}"),
            Some(json!({ "status": "subscribed" })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/contacts/{id}"),
            Some(json!({ "email": "other.secret@example.com" })),
        ),
        (Method::GET, "/api/v1/suppressions".to_owned(), None),
    ] {
        app.call(method, &path, Some(&owner), body).await;
    }
    let imported = app
        .send(import_request(
            &owner,
            "email,consent,name\nimported.secret@example.com,event,Brunhild\nbad.row.secret@,event,X\n",
        ))
        .await;
    assert_eq!(imported.status, StatusCode::ACCEPTED, "{:?}", imported.body);
    let import_id = imported.body["id"].as_str().unwrap().to_owned();
    app.call(
        Method::GET,
        &format!("/api/v1/contacts/imports/{import_id}"),
        Some(&owner),
        None,
    )
    .await;
    let exported = app
        .send(
            Request::get("/api/v1/contacts/export")
                .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(exported.status, StatusCode::OK);
    app.call(
        Method::DELETE,
        &format!("/api/v1/contacts/{id}"),
        Some(&owner),
        None,
    )
    .await;
    drop(guard);

    let text = logs.text();
    assert!(!text.is_empty(), "the capture saw the requests");
    assert_no_leak(
        &[
            "secret.ada@example.com",
            "secret.ada",
            "Adalind",
            "broken.address",
            "other.secret",
            "imported.secret",
            "bad.row.secret",
            "Brunhild",
            owner.as_str(),
        ],
        &[("logs", text.as_str())],
    );
}
