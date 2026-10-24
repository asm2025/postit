//! The `outcome` publish option, idempotency, reconcile, native schedules, cancellation, and dry
//! runs on the mock agent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use chrono::TimeDelta;
use common::{Progress, TOKEN, account, client, context, key, variant};
use postit_agent_mock::MockAgent;
use postit_core::{
    AgentClient, AgentError, DeliveryId, PublishMode, PublishOutcome, PublishReceipt,
    PublishRequest, ReconcileOutcome, ReconcileRequest, RetryClass, Variant,
};

fn request(variant: &Variant, key: DeliveryId) -> PublishRequest<'_> {
    PublishRequest {
        variant,
        mode: PublishMode::Live,
        native_publish_at: None,
        container_ref: None,
        idempotency_key: key,
    }
}

async fn live(client: &dyn AgentClient, outcome: &str) -> Result<PublishOutcome, AgentError> {
    let variant = variant(Some(outcome));
    let result = client
        .publish(request(&variant, key()), &Progress(false))
        .await;
    let shown = format!("{result:?}");
    assert!(!shown.contains(TOKEN), "{shown}");
    if let Err(err) = &result {
        assert!(!err.to_string().contains(TOKEN));
    }
    result
}

fn receipt(outcome: PublishOutcome) -> PublishReceipt {
    match outcome {
        PublishOutcome::Published(r) | PublishOutcome::Processing(r) => r,
        PublishOutcome::Simulated { planned } => {
            panic!("a receipt expected, got a dry run: {planned:?}")
        }
    }
}

async fn reconcile(
    client: &dyn AgentClient,
    variant: &Variant,
    key: DeliveryId,
) -> ReconcileOutcome {
    client
        .reconcile(ReconcileRequest {
            variant,
            container_ref: None,
            attempted_at: chrono::Utc::now(),
            idempotency_key: key,
        })
        .await
        .unwrap()
}

fn setup() -> (MockAgent, postit_agent_kit::testkit::TestContext) {
    (MockAgent::broadcast(), context(TOKEN))
}

#[tokio::test]
async fn published_by_default_and_by_option() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let plain = variant(None);
    let first = receipt(
        mock.publish(request(&plain, key()), &Progress(false))
            .await
            .unwrap(),
    );
    assert_eq!(first.remote_id.as_deref(), Some("mock-post-1"));
    assert_eq!(
        first.remote_url.as_ref().map(url::Url::as_str),
        Some("https://mock.test/posts/mock-post-1")
    );
    let second = live(mock.as_ref(), "published").await.unwrap();
    assert!(matches!(second, PublishOutcome::Published(_)));
    assert_eq!(agent.script().posts().len(), 2);
}

#[tokio::test]
async fn processing_then_status_published() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let outcome = live(mock.as_ref(), "processing").await.unwrap();
    let PublishOutcome::Processing(receipt) = outcome else {
        panic!("Processing expected, got {outcome:?}");
    };
    let status = mock.status(&account("mock"), &receipt).await.unwrap();
    assert_eq!(status, PublishOutcome::Published(receipt));
}

#[tokio::test]
async fn retryable_failure_is_retryable() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "retryable_failure")
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(503),
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.retry_class(), RetryClass::Retryable);
    assert!(agent.script().posts().is_empty());
}

#[tokio::test]
async fn permanent_failure_is_permanent() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "permanent_failure")
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(400),
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.retry_class(), RetryClass::Permanent);
}

#[tokio::test]
async fn outcome_unknown_landed_stores_the_post() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "outcome_unknown_landed")
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::OutcomeUnknown { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::Ambiguous);
    assert_eq!(agent.script().posts().len(), 1);
}

#[tokio::test]
async fn outcome_unknown_lost_stores_nothing() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "outcome_unknown_lost")
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Ambiguous);
    assert!(agent.script().posts().is_empty());
}

#[tokio::test]
async fn needs_reconnect_is_authentication() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "needs_reconnect")
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);
}

#[tokio::test]
async fn rate_limited_waits_thirty_seconds() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "rate_limited")
        .await
        .unwrap_err();
    assert_eq!(
        err.retry_class(),
        RetryClass::RetryAfter(Duration::from_secs(30))
    );
}

#[tokio::test]
async fn quota_exhausted_names_the_bucket() {
    let (agent, ctx) = setup();
    let err = live(client(&agent, &ctx).as_ref(), "quota_exhausted")
        .await
        .unwrap_err();
    let AgentError::Quota { bucket, .. } = &err else {
        panic!("Quota expected, got {err:?}");
    };
    assert_eq!(bucket, "posts");
    assert_eq!(err.retry_class(), RetryClass::QuotaExceeded);
}

#[tokio::test]
async fn same_key_same_receipt() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let k = key();
    let first = mock
        .publish(request(&post, k), &Progress(false))
        .await
        .unwrap();
    let again = mock
        .publish(request(&post, k), &Progress(false))
        .await
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(agent.script().posts().len(), 1);
}

#[tokio::test]
async fn concurrent_same_key_one_post() {
    let (agent, ctx) = setup();
    let one = client(&agent, &ctx);
    let two = client(&agent, &ctx);
    let post = variant(None);
    let k = key();
    let (a, b) = tokio::join!(
        one.publish(request(&post, k), &Progress(false)),
        two.publish(request(&post, k), &Progress(false)),
    );
    let (a, b) = (receipt(a.unwrap()), receipt(b.unwrap()));
    assert_eq!(a, b);
    assert_eq!(agent.script().posts().len(), 1);
}

#[tokio::test]
async fn reconcile_landed_found() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(Some("outcome_unknown_landed"));
    let k = key();
    assert!(
        mock.publish(request(&post, k), &Progress(false))
            .await
            .is_err()
    );
    let ReconcileOutcome::Found(found) = reconcile(mock.as_ref(), &post, k).await else {
        panic!("Found expected");
    };
    assert_eq!(found.remote_id.as_deref(), Some("mock-post-1"));
    let again = mock
        .publish(request(&post, k), &Progress(false))
        .await
        .unwrap();
    assert_eq!(receipt(again), found);
}

#[tokio::test]
async fn reconcile_lost_absent() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(Some("outcome_unknown_lost"));
    let k = key();
    assert!(
        mock.publish(request(&post, k), &Progress(false))
            .await
            .is_err()
    );
    assert_eq!(
        reconcile(mock.as_ref(), &post, k).await,
        ReconcileOutcome::Absent
    );
}

#[tokio::test]
async fn native_schedule_stores_publish_at() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let at = postit_core::Clock::now(ctx.clock.as_ref()) + TimeDelta::hours(2);
    let outcome = mock
        .publish(
            PublishRequest {
                native_publish_at: Some(at),
                ..request(&post, key())
            },
            &Progress(false),
        )
        .await
        .unwrap();
    let PublishOutcome::Processing(receipt) = outcome else {
        panic!("a scheduled post is Processing until it goes live, got {outcome:?}");
    };
    assert_eq!(receipt.remote_publish_at, Some(at));
    let posts = agent.script().posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].publish_at, Some(at));
    assert_eq!(
        mock.status(&account("mock"), &receipt).await.unwrap(),
        PublishOutcome::Processing(receipt)
    );
}

#[tokio::test]
async fn cancel_before_store() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let calls = agent.script().mutating_calls();
    let result = mock.publish(request(&post, key()), &Progress(true)).await;
    assert_eq!(result, Err(AgentError::Cancelled));
    assert!(agent.script().posts().is_empty());
    assert_eq!(agent.script().mutating_calls(), calls);
}

#[tokio::test]
async fn dry_run_stores_nothing() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(Some("permanent_failure"));
    let calls = agent.script().remote_calls();
    let outcome = mock
        .publish(
            PublishRequest {
                mode: PublishMode::DryRun,
                ..request(&post, key())
            },
            &Progress(false),
        )
        .await
        .unwrap();
    let PublishOutcome::Simulated { planned } = outcome else {
        panic!("Simulated expected, got {outcome:?}");
    };
    assert_eq!(planned.len(), 1, "{planned:?}");
    assert!(agent.script().posts().is_empty());
    assert_eq!(agent.script().remote_calls(), calls);
}

#[tokio::test]
async fn live_publish_counts_calls() {
    let (agent, ctx) = setup();
    let script = agent.script();
    live(client(&agent, &ctx).as_ref(), "published")
        .await
        .unwrap();
    assert_eq!(script.remote_calls(), 1);
    assert_eq!(script.mutating_calls(), 1);
}

#[test]
fn descriptors_validate() {
    use postit_core::{Agent, AgentKind, Capability};
    let broadcast = MockAgent::broadcast();
    let d = broadcast.descriptor();
    d.validate().unwrap();
    assert_eq!(d.platform.as_str(), "mock");
    assert_eq!(d.kind, AgentKind::Broadcast);
    assert!(!d.capabilities.contains(&Capability::DirectSender));
    assert_eq!(d.capabilities.len(), 4);
    assert_eq!(d.quota_buckets[0].id, "posts");
    let direct = MockAgent::direct();
    direct.descriptor().validate().unwrap();
    assert_eq!(direct.descriptor().platform.as_str(), "mock_direct");
    assert_eq!(direct.descriptor().kind, AgentKind::Direct);
    let other =
        MockAgent::broadcast().with_platform(postit_core::PlatformId::new("mock_two").unwrap());
    assert_eq!(other.descriptor().platform.as_str(), "mock_two");
}
