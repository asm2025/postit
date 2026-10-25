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
    /// When it was created, by the publishing client's clock.
    pub created_at: DateTime<Utc>,
    /// What publishing it returned.
    pub receipt: PublishReceipt,
}

impl MockPost {
    /// Whether the post is live at `now`: a natively scheduled one once its time has come,
    /// any other by its `live` flag. Status, metrics, and the inbox all decide with this.
    pub(crate) fn live_at(&self, now: DateTime<Utc>) -> bool {
        self.publish_at.map_or(self.live, |at| now >= at)
    }
}

/// A comment on a mock post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockComment {
    /// `mock-comment-{n}`, counting from 1 per agent.
    pub remote_id: String,
    /// The post it is on.
    pub post_remote_id: String,
    /// The comment it answers, for a nested reply.
    pub parent_remote_id: Option<String>,
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
    comment_keys: HashMap<DeliveryId, String>,
    pub(crate) comments: Vec<MockComment>,
    pub(crate) sends: Vec<MockSend>,
    /// Seeded comments held back, by post, until the post goes live.
    unseeded: HashMap<String, u32>,
    next_post: u64,
    next_comment: u64,
}

/// A new reply's details.
pub(crate) struct NewReply<'a> {
    pub post: &'a str,
    pub parent: Option<&'a str>,
    pub author: String,
    pub text: &'a str,
    pub now: DateTime<Utc>,
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

    /// Every post, in creation order.
    pub(crate) fn all_posts(&self) -> &[MockPost] {
        &self.posts
    }

    pub(crate) fn post(&self, remote_id: &str) -> Option<&MockPost> {
        self.posts.iter().find(|p| p.remote_id == remote_id)
    }

    pub(crate) fn post_mut(&mut self, remote_id: &str) -> Option<&mut MockPost> {
        self.posts.iter_mut().find(|p| p.remote_id == remote_id)
    }

    /// Stores a post under `key`, seeds its comments, and returns its outcome. A post that is
    /// not live yet gets its seeded comments when it goes live (see [`State::release_seeds`]).
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
            created_at: new.now,
            receipt,
        };
        let answer = outcome(&post);
        let live = post.live_at(new.now);
        self.posts.push(post);
        self.receipts.insert(key, remote_id.clone());
        if live {
            self.seed(&remote_id, new.comments, new.now);
        } else if new.comments > 0 {
            self.unseeded.insert(remote_id, new.comments);
        }
        answer
    }

    fn seed(&mut self, post: &str, count: u32, at: DateTime<Utc>) {
        for i in 1..=count {
            self.add_comment(post, None, format!("@fan{i}"), format!("Comment {i}"), at);
        }
    }

    /// Seeds the held-back comments of every post live at `now`, in post creation order. They
    /// are appended then, not when the post was created, so an inbox cursor taken while the
    /// post was not live yet does not already sit past them.
    pub(crate) fn release_seeds(&mut self, now: DateTime<Utc>) {
        if self.unseeded.is_empty() {
            return;
        }
        let due: Vec<(String, u32, DateTime<Utc>)> = self
            .posts
            .iter()
            .filter(|p| p.live_at(now))
            .filter_map(|p| {
                let count = *self.unseeded.get(&p.remote_id)?;
                Some((p.remote_id.clone(), count, p.publish_at.unwrap_or(now)))
            })
            .collect();
        for (post, count, at) in due {
            self.unseeded.remove(&post);
            self.seed(&post, count, at);
        }
    }

    /// Removes a post, the key that created it, and its comments.
    pub(crate) fn delete_post(&mut self, remote_id: &str) {
        self.posts.retain(|p| p.remote_id != remote_id);
        self.receipts.retain(|_, id| id != remote_id);
        self.unseeded.remove(remote_id);
        let gone: Vec<String> = self
            .comments
            .iter()
            .filter(|c| c.post_remote_id == remote_id)
            .map(|c| c.remote_id.clone())
            .collect();
        self.comment_keys.retain(|_, id| !gone.contains(id));
        self.comments.retain(|c| c.post_remote_id != remote_id);
    }

    pub(crate) fn comment(&self, remote_id: &str) -> Option<&MockComment> {
        self.comments.iter().find(|c| c.remote_id == remote_id)
    }

    /// Appends a comment `mock-comment-{n}` and returns its ID.
    pub(crate) fn add_comment(
        &mut self,
        post: &str,
        parent: Option<&str>,
        author: String,
        text: String,
        created_at: DateTime<Utc>,
    ) -> String {
        self.next_comment += 1;
        let remote_id = format!("mock-comment-{}", self.next_comment);
        self.comments.push(MockComment {
            remote_id: remote_id.clone(),
            post_remote_id: post.to_owned(),
            parent_remote_id: parent.map(str::to_owned),
            author,
            text,
            created_at,
        });
        remote_id
    }

    /// A comment from a follower on `post`, as if one just arrived: author `@fan{i}` with `i`
    /// counting the post's comments, dated no earlier than the latest thing on the post (the
    /// mock platform has no clock of its own). `None` when there is no such post.
    pub(crate) fn push_comment(&mut self, post: &str, text: &str) -> Option<String> {
        let created = self.post(post)?.created_at;
        let on_post = || self.comments.iter().filter(|c| c.post_remote_id == post);
        let i = on_post().count() + 1;
        let at = on_post().map(|c| c.created_at).fold(created, DateTime::max);
        Some(self.add_comment(post, None, format!("@fan{i}"), text.to_owned(), at))
    }

    /// The outcome already recorded for a comment's `key`.
    pub(crate) fn existing_comment(&self, key: DeliveryId) -> Option<PublishOutcome> {
        self.comment_receipt_for(key).map(PublishOutcome::Published)
    }

    /// The receipt recorded for a comment's `key`.
    pub(crate) fn comment_receipt_for(&self, key: DeliveryId) -> Option<PublishReceipt> {
        let remote_id = self.comment_keys.get(&key)?;
        self.comment(remote_id).map(comment_receipt)
    }

    /// Stores a reply under `key` and returns its outcome. Comments have no processing step,
    /// so a stored reply is always `Published`.
    pub(crate) fn create_reply(&mut self, key: DeliveryId, new: &NewReply<'_>) -> PublishOutcome {
        let id = self.add_comment(
            new.post,
            new.parent,
            new.author.clone(),
            new.text.to_owned(),
            new.now,
        );
        let receipt = reply_receipt(new.post, &id);
        self.comment_keys.insert(key, id);
        PublishOutcome::Published(receipt)
    }
}

/// What posting `comment` returned.
fn comment_receipt(comment: &MockComment) -> PublishReceipt {
    reply_receipt(&comment.post_remote_id, &comment.remote_id)
}

/// A reply's receipt, linking `https://mock.test/posts/{post}/comments/{comment}`.
fn reply_receipt(post: &str, comment: &str) -> PublishReceipt {
    PublishReceipt {
        remote_id: Some(comment.to_owned()),
        remote_url: Url::parse(&format!(
            "https://mock.test/posts/{post}/comments/{comment}"
        ))
        .ok(),
        container_ref: None,
        remote_publish_at: None,
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
pub(crate) struct MockPlatform {
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

    /// Adds a follower's comment to `remote_post_id`; see `State::push_comment`.
    pub(crate) fn push_comment(&self, remote_post_id: &str, text: &str) -> Option<String> {
        self.lock().push_comment(remote_post_id, text)
    }

    /// Every accepted direct send, in order.
    #[must_use]
    pub fn sends(&self) -> Vec<MockSend> {
        self.lock().sends.clone()
    }
}
