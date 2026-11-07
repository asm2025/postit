//! Publishing, retry, and resolve leak neither a credential nor the post text: not in a response,
//! not in a problem's detail, not in a log line. A mock platform fails the publishes in the
//! ways that carry platform text in a real agent. Its own test binary: tracing's callsite cache
//! is process-wide, so a log-capturing test cannot share a binary with others.
#![allow(clippy::unwrap_used)]

mod common;

use axum::http::Method;
use common::{Who, etag, world};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use serde_json::json;
use sqlx::PgPool;

const TEXT: &str = "POST-TEXT-MARKER-4471 fresh loaves";
/// The credential `common::connect` stores for every account.
const TOKEN: &str = "s3cret-token-value";

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_retry_and_resolve_leak_nothing(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let w = world(pool).await;
    let mut outputs: Vec<(String, String)> = Vec::new();

    // A post whose platform refuses it for good, published, read, retried.
    let (failing, tag) = w.post_to(TEXT, &[w.a]).await;
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/posts/{failing}/overrides/{}", w.a),
            Some(json!({"patch": {}, "options": {"outcome": "permanent_failure"}})),
            &[("if-match", &tag)],
        )
        .await;
    let published = w
        .call_with(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/posts/{failing}/publish"),
            Some(json!({"targets": [w.a.to_string()], "confirm": true})),
            &[("if-match", &etag(&put))],
        )
        .await;
    assert_eq!(published.status, 201, "{:?}", published.body);
    outputs.push((
        "publish".into(),
        String::from_utf8_lossy(&published.bytes).into(),
    ));
    w.app.drain_publishing().await;
    let job_id = published.body["id"].as_str().unwrap().to_owned();
    let job = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/publish-jobs/{job_id}"),
            None,
        )
        .await;
    outputs.push(("job".into(), String::from_utf8_lossy(&job.bytes).into()));
    let delivery = job.body["deliveries"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        job.body["deliveries"][0]["state"], "failed",
        "{:?}",
        job.body
    );
    let read = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/deliveries/{delivery}"),
            None,
        )
        .await;
    outputs.push((
        "delivery".into(),
        String::from_utf8_lossy(&read.bytes).into(),
    ));
    let retry = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/retry"),
            Some(json!({"confirm": true})),
        )
        .await;
    outputs.push(("retry".into(), String::from_utf8_lossy(&retry.bytes).into()));
    w.app.drain_publishing().await;
    let refused = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/resolve"),
            Some(json!({"outcome": "published"})),
        )
        .await;
    outputs.push((
        "refused resolve".into(),
        String::from_utf8_lossy(&refused.bytes).into(),
    ));

    // A post whose outcome nobody can verify, settled by hand.
    let (unknown, tag) = w.post_to(TEXT, &[w.b]).await;
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/posts/{unknown}/overrides/{}", w.b),
            Some(json!({"patch": {}, "options": {
                "outcome": "outcome_unknown_lost", "reconcile": "unsupported"
            }})),
            &[("if-match", &tag)],
        )
        .await;
    let second = w
        .call_with(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/posts/{unknown}/publish"),
            Some(json!({"targets": [w.b.to_string()], "confirm": true})),
            &[("if-match", &etag(&put))],
        )
        .await;
    w.app.drain_publishing().await;
    let job = w
        .call(
            Who::Owner,
            Method::GET,
            &format!(
                "/api/v1/publish-jobs/{}",
                second.body["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    let unresolved = job.body["deliveries"][0]["id"].as_str().unwrap().to_owned();
    let no_ack = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{unresolved}/retry"),
            Some(json!({"confirm": true})),
        )
        .await;
    outputs.push((
        "retry without acknowledgment".into(),
        String::from_utf8_lossy(&no_ack.bytes).into(),
    ));
    let resolved = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{unresolved}/resolve"),
            Some(json!({"outcome": "failed"})),
        )
        .await;
    assert_eq!(resolved.status, 200, "{:?}", resolved.body);
    outputs.push((
        "resolve".into(),
        String::from_utf8_lossy(&resolved.bytes).into(),
    ));
    let list = w
        .call(Who::Owner, Method::GET, "/api/v1/publish-jobs", None)
        .await;
    outputs.push(("jobs".into(), String::from_utf8_lossy(&list.bytes).into()));
    assert!(!logs.text().is_empty(), "the capture saw the run");
    outputs.push(("logs".into(), logs.text()));

    let refs: Vec<(&str, &str)> = outputs
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    assert_no_leak(&[TOKEN, "POST-TEXT-MARKER-4471"], &refs);
}
