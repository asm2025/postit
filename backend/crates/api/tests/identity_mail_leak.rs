//! The identity mail webhook logs the event type and status, never the address, code, link,
//! or signing key. Its own binary because it captures logs.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::{Duration, TimeZone, Utc};
use hmac::{Hmac, Mac};
use postit_api::internal::{IdentityMailState, internal_router};
use postit_config::RedactedSecret;
use postit_core::TestClock;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_mail::{MailError, Mailer, MemoryMailer, RenderedMessage};
use sha2::Sha256;
use tower::ServiceExt;

const KEY: &str = "leak-check-signing-key";
const FIXTURE: &str = include_str!("../../mail/tests/fixtures/zitadel/verify_email_code.json");

struct RejectingMailer;

#[async_trait::async_trait]
impl Mailer for RejectingMailer {
    async fn send(&self, _message: RenderedMessage) -> Result<(), MailError> {
        Err(MailError::Permanent("rejected".into()))
    }
}

async fn deliver(mailer: Arc<dyn Mailer>) -> StatusCode {
    let at = Utc.with_ymd_and_hms(2026, 10, 24, 12, 0, 0).unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(KEY.as_bytes()).unwrap();
    mac.update(format!("{}.", at.timestamp()).as_bytes());
    mac.update(FIXTURE.as_bytes());
    let signature = format!(
        "t={},v1={}",
        at.timestamp(),
        hex::encode(mac.finalize().into_bytes())
    );
    let router = internal_router(IdentityMailState {
        mailer,
        signing_key: RedactedSecret::from(KEY.to_string()),
        clock: Arc::new(TestClock::new(at)),
        tolerance: Duration::seconds(300),
        max_body_bytes: 64 * 1024,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/internal/identity-mail")
        .header(header::CONTENT_TYPE, "application/json")
        .header("zitadel-signature", signature)
        .body(Body::from(FIXTURE))
        .unwrap();
    router.oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn address_code_link_and_key_never_reach_logs() {
    let (logs, _guard) = LogCapture::install();
    assert_eq!(
        deliver(Arc::new(RejectingMailer)).await,
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        deliver(Arc::new(MemoryMailer::default())).await,
        StatusCode::NO_CONTENT
    );
    let logs = logs.text();
    assert!(logs.contains("user.human.email.code.added"), "{logs}");
    assert_no_leak(
        &["123456", "ada@example.test", "ABCDEF", KEY],
        &[("logs", &logs)],
    );
}
