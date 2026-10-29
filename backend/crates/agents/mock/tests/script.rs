//! The script's publish controls: multi-step uploads with saved container references, and a
//! gate that holds live publishes before they land.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{TOKEN, client, context, key, variant};
use postit_agent_mock::MockAgent;
use postit_core::{AgentError, PublishMode, PublishOutcome, PublishProgress, PublishRequest};

/// Records every saved reference and cancels once `cancel_after` saves happened.
#[derive(Default)]
struct Recorder {
    saved: Mutex<Vec<String>>,
    cancel_after: Option<usize>,
    cancel: AtomicBool,
}

#[async_trait::async_trait]
impl PublishProgress for Recorder {
    async fn save_container_ref(&self, container_ref: &str) -> Result<(), AgentError> {
        let mut saved = self.saved.lock().unwrap();
        saved.push(container_ref.to_owned());
        if self.cancel_after == Some(saved.len()) {
            self.cancel.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

#[tokio::test]
async fn steps_save_a_container_ref_after_each_chunk() {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    script.set_publish_steps(3);
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let progress = Recorder::default();

    let outcome = mock
        .publish(
            PublishRequest {
                variant: &post,
                mode: PublishMode::Live,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: key(),
            },
            &progress,
        )
        .await
        .unwrap();

    assert!(matches!(outcome, PublishOutcome::Published(_)));
    assert_eq!(
        *progress.saved.lock().unwrap(),
        vec!["chunks:1", "chunks:2", "chunks:3"]
    );
    assert_eq!(script.chunk_uploads(), 3);
    assert_eq!(script.seen_container_refs(), vec![None]);
}

#[tokio::test]
async fn steps_stop_at_the_next_checkpoint_and_resume_from_the_ref() {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    script.set_publish_steps(3);
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let post = variant(None);
    let progress = Recorder {
        cancel_after: Some(1),
        ..Recorder::default()
    };
    let id = key();
    let request = |container_ref| PublishRequest {
        variant: &post,
        mode: PublishMode::Live,
        native_publish_at: None,
        container_ref,
        idempotency_key: id,
    };

    let stopped = mock.publish(request(None), &progress).await;
    assert_eq!(stopped, Err(AgentError::Cancelled));
    assert_eq!(script.chunk_uploads(), 1);
    assert!(script.posts().is_empty());

    let resumed = mock
        .publish(request(Some("chunks:1")), &Recorder::default())
        .await
        .unwrap();
    assert!(matches!(resumed, PublishOutcome::Published(_)));
    // Only the two chunks left were uploaded.
    assert_eq!(script.chunk_uploads(), 3);
    assert_eq!(script.posts().len(), 1);
    assert_eq!(
        script.seen_container_refs(),
        vec![None, Some("chunks:1".to_owned())]
    );
}

#[tokio::test]
async fn gate_holds_live_publishes_until_released() {
    let agent = Arc::new(MockAgent::broadcast());
    let script = agent.script();
    let gate = script.hold_publishes();
    let ctx = Arc::new(context(TOKEN));
    let held = {
        let agent = Arc::clone(&agent);
        let ctx = Arc::clone(&ctx);
        tokio::spawn(async move {
            let mock = client(&agent, &ctx);
            let post = variant(None);
            mock.publish(
                PublishRequest {
                    variant: &post,
                    mode: PublishMode::Live,
                    native_publish_at: None,
                    container_ref: None,
                    idempotency_key: key(),
                },
                &Recorder::default(),
            )
            .await
        })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(5)).await);
    assert!(script.posts().is_empty(), "held before it lands");

    // New calls pass once the gate stops holding; the held one still waits.
    gate.stop_holding();
    let mock = client(&agent, &ctx);
    let other = variant(None);
    mock.publish(
        PublishRequest {
            variant: &other,
            mode: PublishMode::Live,
            native_publish_at: None,
            container_ref: None,
            idempotency_key: key(),
        },
        &Recorder::default(),
    )
    .await
    .unwrap();
    assert_eq!(script.posts().len(), 1);
    assert!(!held.is_finished());

    gate.release();
    let outcome = held.await.unwrap().unwrap();
    assert!(matches!(outcome, PublishOutcome::Published(_)));
    assert_eq!(script.posts().len(), 2);
}

fn with_option(mut post: postit_core::Variant, key: &str, value: &str) -> postit_core::Variant {
    post.options
        .insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
    post
}

async fn reconcile_with(
    mock: &dyn postit_core::AgentClient,
    post: &postit_core::Variant,
    id: postit_core::DeliveryId,
) -> Result<postit_core::ReconcileOutcome, AgentError> {
    mock.reconcile(postit_core::ReconcileRequest {
        variant: post,
        container_ref: None,
        attempted_at: chrono::Utc::now(),
        idempotency_key: id,
    })
    .await
}

#[tokio::test]
async fn reconcile_option_scripts_the_answer() {
    use postit_core::ReconcileOutcome;

    let agent = MockAgent::broadcast();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let landed = variant(Some("outcome_unknown_landed"));
    let id = key();
    let lost = mock
        .publish(
            PublishRequest {
                variant: &landed,
                mode: PublishMode::Live,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: id,
            },
            &Recorder::default(),
        )
        .await;
    assert!(lost.is_err());

    // `found` (and no option) looks the post up: found when it landed, absent otherwise.
    for post in [
        landed.clone(),
        with_option(landed.clone(), "reconcile", "found"),
    ] {
        let found = reconcile_with(mock.as_ref(), &post, id).await.unwrap();
        assert!(matches!(found, ReconcileOutcome::Found(_)), "{found:?}");
        let other = reconcile_with(mock.as_ref(), &post, key()).await.unwrap();
        assert_eq!(other, ReconcileOutcome::Absent);
    }
    let absent = with_option(landed.clone(), "reconcile", "absent");
    assert_eq!(
        reconcile_with(mock.as_ref(), &absent, id).await.unwrap(),
        ReconcileOutcome::Absent
    );
    let unresolvable = with_option(landed.clone(), "reconcile", "unresolvable");
    assert_eq!(
        reconcile_with(mock.as_ref(), &unresolvable, id)
            .await
            .unwrap(),
        ReconcileOutcome::Unresolvable
    );
    let unsupported = with_option(landed.clone(), "reconcile", "unsupported");
    assert!(matches!(
        reconcile_with(mock.as_ref(), &unsupported, id).await,
        Err(AgentError::Unsupported { .. })
    ));
    let bogus = with_option(landed.clone(), "reconcile", "maybe");
    assert!(matches!(
        reconcile_with(mock.as_ref(), &bogus, id).await,
        Err(AgentError::Validation(_))
    ));

    // A scripted transport failure wins over the option until cleared.
    agent.script().fail_reconcile(true);
    let failed = reconcile_with(mock.as_ref(), &landed, id).await;
    assert!(
        matches!(failed, Err(AgentError::Transport { .. })),
        "{failed:?}"
    );
    agent.script().fail_reconcile(false);
    assert!(reconcile_with(mock.as_ref(), &landed, id).await.is_ok());
}

#[tokio::test]
async fn reconcile_comment_follows_the_same_option() {
    use postit_core::{ReconcileOutcome, ReconcileRequest, ReplyTo};

    let agent = MockAgent::broadcast();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let commenter = mock.commenter().unwrap();
    let mut answer = variant(None);
    answer.reply_to = Some(ReplyTo {
        remote_post_id: "mock-post-1".to_owned(),
        remote_comment_id: None,
        url: None,
    });
    let ask = |post: &postit_core::Variant| {
        let post = post.clone();
        async move {
            commenter
                .reconcile_comment(ReconcileRequest {
                    variant: &post,
                    container_ref: None,
                    attempted_at: chrono::Utc::now(),
                    idempotency_key: key(),
                })
                .await
        }
    };
    assert_eq!(ask(&answer).await.unwrap(), ReconcileOutcome::Absent);
    assert_eq!(
        ask(&with_option(answer.clone(), "reconcile", "unresolvable"))
            .await
            .unwrap(),
        ReconcileOutcome::Unresolvable
    );
    assert!(matches!(
        ask(&with_option(answer.clone(), "reconcile", "unsupported")).await,
        Err(AgentError::Unsupported { .. })
    ));
}

#[tokio::test]
async fn keep_processing_holds_status_until_cleared() {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let post = variant(Some("processing"));
    let outcome = mock
        .publish(
            PublishRequest {
                variant: &post,
                mode: PublishMode::Live,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: key(),
            },
            &Recorder::default(),
        )
        .await
        .unwrap();
    let PublishOutcome::Processing(receipt) = outcome else {
        panic!("processing expected, got {outcome:?}");
    };
    script.keep_processing(true);
    let still = mock
        .status(&common::account("mock"), &receipt)
        .await
        .unwrap();
    assert!(matches!(still, PublishOutcome::Processing(_)), "{still:?}");
    script.keep_processing(false);
    let live = mock
        .status(&common::account("mock"), &receipt)
        .await
        .unwrap();
    assert!(matches!(live, PublishOutcome::Published(_)), "{live:?}");
}

#[tokio::test]
async fn retry_after_rate_limits_status_and_reconcile() {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let post = variant(Some("processing"));
    let id = key();
    let outcome = mock
        .publish(
            PublishRequest {
                variant: &post,
                mode: PublishMode::Live,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: id,
            },
            &Recorder::default(),
        )
        .await
        .unwrap();
    let PublishOutcome::Processing(receipt) = outcome else {
        panic!("processing expected, got {outcome:?}");
    };
    let after = Duration::from_secs(90);
    script.set_retry_after(Some(after));
    let limited = AgentError::RateLimited {
        retry_after: Some(after),
    };
    assert_eq!(
        mock.status(&common::account("mock"), &receipt).await,
        Err(limited.clone())
    );
    assert_eq!(reconcile_with(mock.as_ref(), &post, id).await, Err(limited));
    script.set_retry_after(None);
    assert!(
        mock.status(&common::account("mock"), &receipt)
            .await
            .is_ok()
    );
}
