use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_identity::auth::AllowAll;
use secrecy::SecretString;
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for Captured {
    type Writer = Self;
    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bearer_tokens_never_reach_logs_or_responses(pool: PgPool) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

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
        .authenticate(&SecretString::from(bad_token.clone()), &AllowAll)
        .await
        .err()
        .map(|e| format!("{e} {e:?}"))
        .unwrap_or_default();

    let logs = String::from_utf8_lossy(
        &captured
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .to_string();
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
    for (place, text) in [
        ("logs", logs.as_str()),
        ("ok body", &ok.body.to_string()),
        ("bad body", &bad.body.to_string()),
        ("AuthError", err.as_str()),
    ] {
        assert!(
            !text.contains(signature),
            "token signature leaked into {place}"
        );
    }
    assert!(
        !logs.contains("SECRETCODE123"),
        "an OAuth code leaked into logs"
    );
    assert!(
        !logs.contains("secret-term-456"),
        "a query string leaked into logs"
    );
}
