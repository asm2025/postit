use postit_data::audit::AuditContext;

use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_identity::auth::AllowAll;
use secrecy::SecretString;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn bearer_tokens_never_reach_logs_or_responses(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();

    // userinfo = always: the cache miss below sends the bearer to the IdP, the one outbound
    // request that carries it.
    let app = TestApp::start_with_userinfo(pool).await;
    let token = app.token(ADMIN_SUB);
    app.issuer
        .mount_userinfo(
            &token,
            serde_json::json!({
                "sub": ADMIN_SUB,
                "email": "admin@postit.test",
                "email_verified": true,
                "name": ADMIN_SUB,
            }),
        )
        .await;
    let ok = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let bad_token = format!("{token}tampered");
    let bad = app
        .call(Method::GET, "/api/v1/me", Some(&bad_token), None)
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);

    // Query strings never reach the logs: an OAuth code on the Swagger redirect page and a
    // search term on an admin route.
    app.call(
        Method::GET,
        "/docs/oauth2-redirect.html?code=SECRETCODE123&state=s",
        None,
        None,
    )
    .await;
    app.call(
        Method::GET,
        "/api/v1/users?search=secret-term-456",
        Some(&token),
        None,
    )
    .await;

    // Error values carry the failure kind, never the token.
    let err = app
        .state
        .auth
        .authenticate(
            &SecretString::from(bad_token.clone()),
            &AllowAll,
            AuditContext::default(),
        )
        .await
        .err()
        .map(|e| format!("{e} {e:?}"))
        .unwrap_or_default();

    let logs = logs.text();
    // The capture really saw the request path: the span and the extractor's rejection line.
    assert!(
        logs.contains("route="),
        "trace spans were not captured: {logs}"
    );
    assert!(
        logs.contains("bearer token rejected"),
        "the rejection was not captured: {logs}"
    );

    let signature = token.rsplit('.').next().unwrap_or_default();
    assert!(!signature.is_empty());
    assert_no_leak(
        &[signature],
        &[
            ("logs", &logs),
            ("ok body", &ok.body.to_string()),
            ("bad body", &bad.body.to_string()),
            ("AuthError", &err),
        ],
    );
    // Query strings never reach the logs.
    assert_no_leak(&["SECRETCODE123", "secret-term-456"], &[("logs", &logs)]);
}
