//! The `outcome` publish option, idempotency, reconcile, native schedules, cancellation, and dry
//! runs on the mock agent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use chrono::{TimeDelta, TimeZone, Utc};
use common::{Progress, TOKEN, account, client, context, key, variant};
use postit_agent_mock::MockAgent;
use postit_core::{
    AgentClient, AgentError, DeliveryId, PublishMode, PublishOutcome, PublishReceipt,
    PublishRequest, ReconcileOutcome, ReconcileRequest, RetryClass, Variant,
};
use serde_json::json;

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

/// The message key of a single agent-rule violation.
fn rule_key(err: &AgentError) -> String {
    let AgentError::Validation(violations) = err else {
        panic!("Validation expected, got {err:?}");
    };
    assert_eq!(violations.len(), 1, "{violations:?}");
    violations[0].params["message"]["key"]
        .as_str()
        .unwrap()
        .to_owned()
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
async fn status_of_another_accounts_post_is_not_found() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let outcome = live(mock.as_ref(), "processing").await.unwrap();
    let PublishOutcome::Processing(receipt) = outcome else {
        panic!("Processing expected, got {outcome:?}");
    };
    let mut other = account("mock");
    other.remote_id = "mock-account-other".to_owned();
    let err = mock.status(&other, &receipt).await.unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(404),
                ..
            }
        ),
        "{err:?}"
    );
    // The wrong account's question does not move the post along either.
    assert!(!agent.script().posts()[0].live);
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
async fn quota_resets_at_the_next_utc_midnight() {
    let agent = MockAgent::broadcast();
    let mut ctx = context(TOKEN);
    ctx.clock = std::sync::Arc::new(postit_core::TestClock::new(
        Utc.with_ymd_and_hms(2026, 3, 5, 13, 45, 0)
            .single()
            .unwrap(),
    ));
    let err = live(client(&agent, &ctx).as_ref(), "quota_exhausted")
        .await
        .unwrap_err();
    let AgentError::Quota { resets_at, .. } = err else {
        panic!("Quota expected, got {err:?}");
    };
    assert_eq!(
        resets_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 6, 0, 0, 0).single().unwrap())
    );
}

#[tokio::test]
async fn native_time_outside_the_window_is_refused() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let now = postit_core::Clock::now(ctx.clock.as_ref());
    for lead in [TimeDelta::minutes(5), TimeDelta::days(31)] {
        for mode in [PublishMode::Live, PublishMode::DryRun] {
            let err = mock
                .publish(
                    PublishRequest {
                        mode,
                        native_publish_at: Some(now + lead),
                        ..request(&post, key())
                    },
                    &Progress(false),
                )
                .await
                .unwrap_err();
            assert_eq!(
                rule_key(&err),
                "mock.schedule_outside_window",
                "{lead} {mode:?}"
            );
        }
    }
    assert!(agent.script().posts().is_empty());
}

#[tokio::test]
async fn invalid_options_are_refused_by_validate_and_publish() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    for (name, value, want) in [
        ("outcome", json!("explode"), "mock.options.invalid_outcome"),
        ("outcome", json!(7), "mock.options.invalid_outcome"),
        ("comments", json!(21), "mock.options.invalid_comments"),
        ("comments", json!(-1), "mock.options.invalid_comments"),
        ("comments", json!("many"), "mock.options.invalid_comments"),
    ] {
        let mut bad = variant(None);
        bad.options.insert(name.to_owned(), value.clone());
        let violations = mock.validate(&bad, &[]).await.unwrap();
        assert_eq!(violations.len(), 1, "{name}={value}");
        assert_eq!(
            violations[0].params["message"]["key"], want,
            "{name}={value}"
        );
        assert_eq!(
            violations[0].path.as_deref(),
            Some(format!("options.{name}").as_str())
        );

        let err = mock
            .publish(request(&bad, key()), &Progress(false))
            .await
            .unwrap_err();
        assert_eq!(rule_key(&err), want, "{name}={value}");
    }
    assert!(agent.script().posts().is_empty());
    assert_eq!(agent.script().mutating_calls(), 0);
}

#[tokio::test]
async fn comments_option_seeds_that_many_comments() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let mut seeded = variant(None);
    seeded.options.insert("comments".to_owned(), json!(3));
    let outcome = mock
        .publish(request(&seeded, key()), &Progress(false))
        .await
        .unwrap();
    let receipt = receipt(outcome);
    let comments = agent.script().comments();
    assert_eq!(comments.len(), 3);
    for (i, comment) in comments.iter().enumerate() {
        assert_eq!(comment.post_remote_id, receipt.remote_id.clone().unwrap());
        assert_eq!(comment.author, format!("@fan{}", i + 1));
        assert_eq!(comment.text, format!("Comment {}", i + 1));
    }
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

#[tokio::test]
async fn known_key_wins_over_refusal() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let k = key();
    let first = mock
        .publish(request(&variant(None), k), &Progress(false))
        .await
        .unwrap();
    // The retry of an accepted publish gets its receipt even when the platform would now
    // refuse a new post, and even when its native time has fallen outside the window.
    for refused in ["permanent_failure", "rate_limited", "needs_reconnect"] {
        let again = mock
            .publish(request(&variant(Some(refused)), k), &Progress(false))
            .await
            .unwrap();
        assert_eq!(again, first, "{refused}");
    }
    let plain = variant(None);
    let too_soon = PublishRequest {
        native_publish_at: Some(postit_core::Clock::now(ctx.clock.as_ref())),
        ..request(&plain, k)
    };
    assert_eq!(
        mock.publish(too_soon, &Progress(false)).await.unwrap(),
        first
    );
    assert_eq!(agent.script().posts().len(), 1);
}
