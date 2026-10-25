//! `Commenter`: replies to posts (and to comments on them) stored on the mock platform.
//!
//! A comment follows the same steps as a publish: the `outcome` option and armed faults pick
//! the platform's reaction, a key the platform already answered returns that reply before any
//! refusal, and a dry run or a cancelled request calls nothing. A comment without a reply
//! target is refused in a dry run too. Comments have no processing step, so the `processing`
//! outcome stores a reply that is `Published` at once.

use async_trait::async_trait;
use postit_core::{
    AccountRef, AgentError, Commenter, PublishMode, PublishOutcome, PublishProgress,
    PublishRequest, ReconcileOutcome, ReconcileRequest, RemoteTarget, ReplyTo,
};
use url::Url;

use super::handle;
use crate::client::MockClient;
use crate::options::PostOptions;
use crate::platform::{Call, NewReply};

/// The host every mock post link is on.
const HOST: &str = "mock.test";

/// The post and, for a reply link, comment ID in `https://mock.test/posts/{post}` or
/// `https://mock.test/posts/{post}/comments/{comment}` (a trailing slash is allowed), when `url`
/// has one of those shapes.
fn link_ids(url: &Url) -> Option<(&str, Option<&str>)> {
    if url.scheme() != "https" || url.host_str() != Some(HOST) {
        return None;
    }
    let mut segments: Vec<&str> = url.path_segments()?.collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    match segments.as_slice() {
        ["posts", post] if !post.is_empty() => Some((post, None)),
        ["posts", post, "comments", comment] if !post.is_empty() && !comment.is_empty() => {
            Some((post, Some(comment)))
        }
        _ => None,
    }
}

impl MockClient {
    fn target_unknown(&self) -> AgentError {
        self.violation(
            "target_unknown",
            "This link is not a post on the mock platform",
        )
    }
}

#[async_trait]
impl Commenter for MockClient {
    async fn resolve_target(
        &self,
        _account: &AccountRef,
        url: &Url,
    ) -> Result<RemoteTarget, AgentError> {
        let Some((post, comment)) = link_ids(url) else {
            return Err(self.target_unknown());
        };
        self.platform().call(Call::Read).await;
        let state = self.platform().lock();
        let known = state.post(post).is_some()
            && comment.is_none_or(|c| state.comment(c).is_some_and(|c| c.post_remote_id == post));
        if !known {
            return Err(self.target_unknown());
        }
        Ok(RemoteTarget {
            reply_to: ReplyTo {
                remote_post_id: post.to_owned(),
                remote_comment_id: comment.map(str::to_owned),
                url: Some(url.clone()),
            },
        })
    }

    async fn comment(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        let options = PostOptions::parse(&self.platform_id, &request.variant.options)
            .map_err(AgentError::Validation)?;
        let reply_to = request.variant.reply_to.as_ref();
        let Some(reply_to) = reply_to else {
            return Err(self.violation("reply_to_missing", "A comment needs a post to answer"));
        };
        if request.mode == PublishMode::DryRun {
            let answered = reply_to
                .remote_comment_id
                .as_ref()
                .unwrap_or(&reply_to.remote_post_id);
            return Ok(PublishOutcome::Simulated {
                planned: vec![format!("reply to {answered}")],
            });
        }
        if progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        let token = self.token().await?;
        self.platform().call(Call::Mutating).await;
        let reaction = self.reaction(token, options);
        let key = request.idempotency_key;
        let variant = request.variant;
        let now = self.now();
        self.deliver(
            reaction,
            |state| state.existing_comment(key),
            |state, _live| {
                let post = &reply_to.remote_post_id;
                let parent = reply_to.remote_comment_id.as_deref();
                let known = state.post(post).is_some()
                    && parent.is_none_or(|parent| {
                        state
                            .comment(parent)
                            .is_some_and(|c| &c.post_remote_id == post)
                    });
                if !known {
                    return Err(self.target_unknown());
                }
                Ok(state.create_reply(
                    key,
                    &NewReply {
                        post,
                        parent,
                        author: handle(&variant.target.remote_id),
                        text: &variant.text,
                        now,
                    },
                ))
            },
        )
    }

    async fn reconcile_comment(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        self.platform().call(Call::Read).await;
        Ok(self
            .platform()
            .lock()
            .comment_receipt_for(request.idempotency_key)
            .map_or(ReconcileOutcome::Absent, ReconcileOutcome::Found))
    }
}
