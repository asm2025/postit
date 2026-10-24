//! The fake platform: what the mock "server" stores. One [`MockPlatform`] per agent, shared by
//! every client the agent creates, held in memory and lost when the process exits.
//!
//! A real agent has no such module (its platform is remote); the mock keeps its state here so
//! the client modules read like protocol code: make a call, look at the answer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use postit_core::{DeliveryId, DirectSendId, PublishOutcome, PublishReceipt};
use url::Url;

/// A post on the mock platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockPost {
    /// `mock-post-{n}`, counting from 1 per agent.
    pub remote_id: String,
    /// The remote ID of the account it was posted to.
    pub account: String,
    /// The post's text.
    pub text: String,
    /// Whether it is live (`false` while processing or natively scheduled).
    pub live: bool,
    /// When a natively scheduled post goes live.
    pub publish_at: Option<DateTime<Utc>>,
    /// What publishing it returned.
    pub receipt: PublishReceipt,
}

/// A comment on a mock post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockComment {
    /// `mock-comment-{n}`, counting from 1 per agent.
    pub remote_id: String,
    /// The post it answers.
    pub post_remote_id: String,
    /// The author's handle.
    pub author: String,
    /// The comment's text.
    pub text: String,
    /// When it was made, by the client's clock.
    pub created_at: DateTime<Utc>,
}

/// One direct message the mock platform accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockSend {
    /// The send it answers.
    pub send_id: DirectSendId,
    /// The platform's ID for the message.
    pub remote_id: String,
}

/// Everything the mock platform holds.
#[derive(Debug, Default)]
pub(crate) struct State {
    posts: Vec<MockPost>,
    receipts: HashMap<DeliveryId, String>,
    pub(crate) comments: Vec<MockComment>,
    pub(crate) sends: Vec<MockSend>,
    next_post: u64,
    next_comment: u64,
}

/// A new post's details.
pub(crate) struct NewPost<'a> {
    pub account: &'a str,
    pub text: &'a str,
    pub live: bool,
    pub publish_at: Option<DateTime<Utc>>,
    pub comments: u32,
    pub now: DateTime<Utc>,
}

impl State {
    /// The outcome already recorded for `key`, so a repeated publish returns it.
    pub(crate) fn existing(&self, key: DeliveryId) -> Option<PublishOutcome> {
        let remote_id = self.receipts.get(&key)?;
        self.post(remote_id).map(outcome)
    }

    /// The receipt recorded for `key`.
    pub(crate) fn receipt_for(&self, key: DeliveryId) -> Option<PublishReceipt> {
        let remote_id = self.receipts.get(&key)?;
        self.post(remote_id).map(|p| p.receipt.clone())
    }

    pub(crate) fn post(&self, remote_id: &str) -> Option<&MockPost> {
        self.posts.iter().find(|p| p.remote_id == remote_id)
    }

    pub(crate) fn post_mut(&mut self, remote_id: &str) -> Option<&mut MockPost> {
        self.posts.iter_mut().find(|p| p.remote_id == remote_id)
    }

    /// Stores a post under `key`, seeds its comments, and returns its outcome.
    pub(crate) fn create_post(&mut self, key: DeliveryId, new: &NewPost<'_>) -> PublishOutcome {
        self.next_post += 1;
        let remote_id = format!("mock-post-{}", self.next_post);
        let receipt = PublishReceipt {
            remote_url: post_url(&remote_id),
            remote_id: Some(remote_id.clone()),
            container_ref: None,
            remote_publish_at: new.publish_at,
        };
        let post = MockPost {
            remote_id: remote_id.clone(),
            account: new.account.to_owned(),
            text: new.text.to_owned(),
            live: new.live,
            publish_at: new.publish_at,
            receipt,
        };
        let answer = outcome(&post);
        self.posts.push(post);
        self.receipts.insert(key, remote_id.clone());
        for i in 1..=new.comments {
            self.next_comment += 1;
            self.comments.push(MockComment {
                remote_id: format!("mock-comment-{}", self.next_comment),
                post_remote_id: remote_id.clone(),
                author: format!("@fan{i}"),
                text: format!("Comment {i}"),
                created_at: new.now,
            });
        }
        answer
    }
}

/// `Published` for a live post, `Processing` otherwise.
pub(crate) fn outcome(post: &MockPost) -> PublishOutcome {
    if post.live {
        PublishOutcome::Published(post.receipt.clone())
    } else {
        PublishOutcome::Processing(post.receipt.clone())
    }
}

/// `https://mock.test/posts/{remote_id}`.
pub(crate) fn post_url(remote_id: &str) -> Option<Url> {
    Url::parse(&format!("https://mock.test/posts/{remote_id}")).ok()
}

/// Whether a call could change the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Call {
    Read,
    Mutating,
}

/// The shared platform: its state and the count of calls it received.
#[derive(Debug, Clone, Default)]
pub struct MockPlatform {
    state: Arc<Mutex<State>>,
    remote: Arc<AtomicUsize>,
    mutating: Arc<AtomicUsize>,
}

impl MockPlatform {
    /// Locks the state. Callers never hold the guard across an `.await`, so a check and the
    /// store it guards happen as one step even when two clients publish at once.
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a request reaching the platform, then yields, as a network round trip would,
    /// so concurrent calls interleave in tests.
    pub(crate) async fn call(&self, call: Call) {
        self.remote.fetch_add(1, Ordering::SeqCst);
        if call == Call::Mutating {
            self.mutating.fetch_add(1, Ordering::SeqCst);
        }
        tokio::task::yield_now().await;
    }

    /// Requests received so far.
    #[must_use]
    pub fn remote_calls(&self) -> usize {
        self.remote.load(Ordering::SeqCst)
    }

    /// Received requests that could change the platform.
    #[must_use]
    pub fn mutating_calls(&self) -> usize {
        self.mutating.load(Ordering::SeqCst)
    }

    /// Every post, in creation order.
    #[must_use]
    pub fn posts(&self) -> Vec<MockPost> {
        self.lock().posts.clone()
    }

    /// Every comment, in creation order.
    #[must_use]
    pub fn comments(&self) -> Vec<MockComment> {
        self.lock().comments.clone()
    }

    /// Every accepted direct send, in order.
    #[must_use]
    pub fn sends(&self) -> Vec<MockSend> {
        self.lock().sends.clone()
    }
}
