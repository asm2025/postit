//! The `POST /internal/identity-mail` webhook: signature, limits, recipient checks, and the
//! mapping of mail failures to statuses.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::{DateTime, Duration, TimeZone, Utc};
use hmac::{Hmac, Mac};
use postit_api::internal::{IdentityMailState, internal_router};
use postit_config::RedactedSecret;
use postit_core::TestClock;
use postit_mail::{MailError, Mailer, MemoryMailer, RenderedMessage};
use sha2::Sha256;
use tower::ServiceExt;

const KEY: &str = "identity-signing-key";
const MAX_BODY: usize = 64 * 1024;
const CODE_FIXTURE: &str = include_str!("../../mail/tests/fixtures/zitadel/verify_email_code.json");

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 24, 12, 0, 0).unwrap()
}

fn sign(key: &str, at: DateTime<Utc>, body: &[u8]) -> String {
    let t = at.timestamp();
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
    mac.update(format!("{t}.").as_bytes());
    mac.update(body);
    format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
}

/// Fails every send with the given kind.
struct FailingMailer {
    permanent: bool,
}

#[async_trait::async_trait]
impl Mailer for FailingMailer {
    async fn send(&self, _message: RenderedMessage) -> Result<(), MailError> {
        Err(if self.permanent {
            MailError::Permanent("rejected".into())
        } else {
            MailError::Transient("unavailable".into())
        })
    }
}

fn router_with(mailer: Arc<dyn Mailer>) -> Router {
    internal_router(IdentityMailState {
        mailer,
        signing_key: RedactedSecret::from(KEY.to_string()),
        clock: Arc::new(TestClock::new(now())),
        tolerance: Duration::seconds(300),
        max_body_bytes: MAX_BODY,
    })
}

fn post(body: &str, signature: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/internal/identity-mail")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(signature) = signature {
        builder = builder.header("zitadel-signature", signature);
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}

async fn status(router: Router, req: Request<Body>) -> StatusCode {
    router.oneshot(req).await.unwrap().status()
}

fn signed(body: &str) -> Request<Body> {
    post(body, Some(&sign(KEY, now(), body.as_bytes())))
}

fn with_recipient(recipient: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(CODE_FIXTURE).unwrap();
    value["contextInfo"]["recipientEmailAddress"] = recipient.into();
    value.to_string()
}

#[tokio::test]
async fn signed_code_payload_sends_and_answers_204() {
    let mailer = MemoryMailer::default();
    let res = router_with(Arc::new(mailer.clone()))
        .oneshot(signed(CODE_FIXTURE))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "ada@example.test");
    assert!(
        sent[0].subject.contains("Verify email"),
        "{}",
        sent[0].subject
    );
    assert!(sent[0].html.contains("123456"));
    assert!(sent[0].text.contains("123456"));
}

/// An Arabic display name sent as UTF-8 reaches the rendered mail unchanged, never as `?`.
#[tokio::test]
async fn arabic_display_name_round_trips_unchanged() {
    const NAME: &str = "آدا لوفلايس";
    let mut value: serde_json::Value = serde_json::from_str(CODE_FIXTURE).unwrap();
    value["templateData"]["subject"] = "أكّد بريدك الإلكتروني في postit".into();
    value["templateData"]["title"] = "أكّد بريدك الإلكتروني".into();
    value["templateData"]["greeting"] = format!("مرحبًا {NAME}،").into();
    value["templateData"]["text"] = "يرجى تأكيد بريدك الإلكتروني.".into();
    value["args"]["displayName"] = NAME.into();
    let body = value.to_string();
    assert!(body.contains(NAME), "the JSON body must carry raw UTF-8");

    let mailer = MemoryMailer::default();
    let res = router_with(Arc::new(mailer.clone()))
        .oneshot(signed(&body))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    let greeting = format!("مرحبًا {NAME}،");
    assert!(sent[0].html.contains(&greeting), "{}", sent[0].html);
    assert!(sent[0].text.contains(&greeting), "{}", sent[0].text);
    assert_eq!(sent[0].subject, "أكّد بريدك الإلكتروني في postit");
    assert!(!sent[0].html.contains("??"), "{}", sent[0].html);
    assert!(!sent[0].text.contains("??"), "{}", sent[0].text);
}

#[tokio::test]
async fn missing_wrong_and_stale_signatures_answer_401_and_send_nothing() {
    let mailer = MemoryMailer::default();
    let stale = sign(KEY, now() - Duration::seconds(301), CODE_FIXTURE.as_bytes());
    let wrong = sign("other-key", now(), CODE_FIXTURE.as_bytes());
    for signature in [None, Some(wrong.as_str()), Some(stale.as_str())] {
        let code = status(
            router_with(Arc::new(mailer.clone())),
            post(CODE_FIXTURE, signature),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{signature:?}");
    }
    assert!(mailer.sent().is_empty());
}

#[tokio::test]
async fn oversized_body_answers_413() {
    let body = "x".repeat(MAX_BODY + 1);
    let code = status(
        router_with(Arc::new(MemoryMailer::default())),
        signed(&body),
    )
    .await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn unreadable_body_answers_400_not_413() {
    let stream = futures::stream::iter([Err::<Vec<u8>, std::io::Error>(std::io::Error::other(
        "connection reset",
    ))]);
    let req = Request::builder()
        .method("POST")
        .uri("/internal/identity-mail")
        .body(Body::from_stream(stream))
        .unwrap();
    let code = status(router_with(Arc::new(MemoryMailer::default())), req).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn malformed_json_answers_400() {
    let code = status(
        router_with(Arc::new(MemoryMailer::default())),
        signed("{not json"),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_recipient_answers_400() {
    let mut value: serde_json::Value = serde_json::from_str(CODE_FIXTURE).unwrap();
    value["contextInfo"]
        .as_object_mut()
        .unwrap()
        .remove("recipientEmailAddress");
    let mailer = MemoryMailer::default();
    let code = status(
        router_with(Arc::new(mailer.clone())),
        signed(&value.to_string()),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(mailer.sent().is_empty());
}

#[tokio::test]
async fn crlf_or_display_name_recipient_answers_400() {
    for recipient in ["a@b.c\r\nBcc: x@y.z", "Ada <a@b.c>"] {
        let mailer = MemoryMailer::default();
        let code = status(
            router_with(Arc::new(mailer.clone())),
            signed(&with_recipient(recipient)),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{recipient:?}");
        assert!(mailer.sent().is_empty());
    }
}

#[tokio::test]
async fn transient_smtp_failure_answers_503() {
    let mailer = Arc::new(FailingMailer { permanent: false });
    assert_eq!(
        status(router_with(mailer), signed(CODE_FIXTURE)).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn permanent_smtp_failure_answers_502() {
    let mailer = Arc::new(FailingMailer { permanent: true });
    assert_eq!(
        status(router_with(mailer), signed(CODE_FIXTURE)).await,
        StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn get_is_405_and_other_paths_404() {
    let router = || router_with(Arc::new(MemoryMailer::default()));
    let get = Request::builder()
        .method("GET")
        .uri("/internal/identity-mail")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(router(), get).await, StatusCode::METHOD_NOT_ALLOWED);
    let other = Request::builder()
        .method("POST")
        .uri("/internal/other")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(router(), other).await, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_api_router_does_not_serve_it(pool: sqlx::PgPool) {
    let app = postit_api::testkit::TestApp::start(pool).await;
    let res = app.send(signed(CODE_FIXTURE)).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}
