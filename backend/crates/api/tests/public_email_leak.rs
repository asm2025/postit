//! Its own test binary: tracing's callsite-interest cache is process-wide, so a log-capturing
//! test cannot share a binary with others.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use postit_api::testkit::TestApp;
use postit_core::{DirectRecipient, DirectSendId, EmailLanguage, LinkSignerSource as _};
use postit_data::testkit::seed_user;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const ADDRESS: &str = "leaky.reader@example.com";
const DESTINATION: &str = "https://shop.example/private-offer-7f3a";

fn token(url: &Url) -> String {
    url.path_segments().unwrap().next_back().unwrap().to_owned()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_email_routes_log_no_token(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let app = TestApp::start(pool.clone()).await;
    let owner = seed_user(&pool, "active").await;
    let signer = app.state.email_links.for_owner(owner);
    let send = DirectSendId::from_uuid(Uuid::now_v7());
    let recipient = DirectRecipient {
        send_id: send,
        address: ADDRESS.to_string(),
        merge_fields: BTreeMap::new(),
        locale: String::new(),
    };
    let unsub = token(&signer.unsubscribe_url(&recipient, EmailLanguage::EnUs));
    let click = token(&signer.click_url(send, &Url::parse(DESTINATION).unwrap()));
    let open = token(&signer.open_pixel_url(send));
    let forged = "FORGEDEMAILTOKEN0123456789abcdefXYZ";

    let mut bodies = Vec::new();
    for (method, path, want) in [
        ("GET", format!("/public/u/{unsub}"), StatusCode::OK),
        ("POST", format!("/public/u/{unsub}"), StatusCode::OK),
        ("GET", format!("/public/t/c/{click}"), StatusCode::SEE_OTHER),
        ("GET", format!("/public/t/o/{open}"), StatusCode::OK),
        ("GET", format!("/public/u/{forged}"), StatusCode::NOT_FOUND),
        ("POST", format!("/public/u/{forged}"), StatusCode::NOT_FOUND),
        (
            "GET",
            format!("/public/t/c/{forged}"),
            StatusCode::NOT_FOUND,
        ),
        (
            "GET",
            format!("/public/t/o/{forged}"),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let res = app
            .send(
                Request::builder()
                    .method(method)
                    .uri(&path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status, want, "{method} {path}");
        bodies.push(String::from_utf8_lossy(&res.bytes).into_owned());
    }

    let logs = logs.text();
    assert!(
        logs.contains("route="),
        "trace spans were not captured: {logs}"
    );
    let mut sources = vec![("logs", logs.as_str())];
    for body in &bodies {
        sources.push(("response body", body.as_str()));
    }
    assert_no_leak(
        &[
            unsub.as_str(),
            click.as_str(),
            open.as_str(),
            forged,
            ADDRESS,
        ],
        &sources,
    );
    // The destination appears in the redirect only, never in a log line.
    assert_no_leak(&[DESTINATION], &[("logs", logs.as_str())]);
}
