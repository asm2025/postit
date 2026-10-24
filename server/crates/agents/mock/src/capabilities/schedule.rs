//! `NativeScheduler`: moving and removing posts the mock platform holds for later.
//!
//! A publish with `native_publish_at` stores the post with that time (see the client), and
//! `status` reports it `Published` once the client's clock passes it. Only a post still waiting
//! can be moved or removed; a new time must fall inside the descriptor's schedule window.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::{AccountRef, AgentError, NativeScheduler, PublishReceipt};

use super::not_found;
use crate::client::MockClient;
use crate::platform::{Call, MockPost};

impl MockClient {
    fn not_scheduled(&self) -> AgentError {
        self.violation(
            "not_scheduled",
            "This post is no longer waiting for its scheduled time",
        )
    }

    /// Whether `post` belongs to `account` and still waits for its scheduled time.
    fn pending(&self, post: &MockPost, now: DateTime<Utc>) -> Result<(), AgentError> {
        match post.publish_at {
            Some(at) if now < at => Ok(()),
            _ => Err(self.not_scheduled()),
        }
    }
}

fn owned<'a>(post: Option<&'a mut MockPost>, account: &AccountRef) -> Option<&'a mut MockPost> {
    post.filter(|p| p.account == account.remote_id)
}

#[async_trait]
impl NativeScheduler for MockClient {
    async fn reschedule(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
        at: DateTime<Utc>,
    ) -> Result<PublishReceipt, AgentError> {
        self.check_schedule(Some(at))?;
        self.platform().call(Call::Mutating).await;
        let now = self.now();
        let mut state = self.platform().lock();
        let post = receipt
            .remote_id
            .as_deref()
            .and_then(|id| state.post_mut(id));
        let post = owned(post, account).ok_or_else(not_found)?;
        self.pending(post, now)?;
        post.publish_at = Some(at);
        post.receipt.remote_publish_at = Some(at);
        Ok(post.receipt.clone())
    }

    async fn delete(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<(), AgentError> {
        self.platform().call(Call::Mutating).await;
        let now = self.now();
        let mut state = self.platform().lock();
        let Some(id) = receipt.remote_id.as_deref() else {
            return Err(not_found());
        };
        let post = owned(state.post_mut(id), account).ok_or_else(not_found)?;
        self.pending(post, now)?;
        state.delete_post(id);
        Ok(())
    }
}
