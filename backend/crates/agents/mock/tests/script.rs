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
