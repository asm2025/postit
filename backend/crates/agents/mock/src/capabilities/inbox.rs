//! `InboxReader`: the comments on the account's posts, oldest first.
//!
//! The comments come from the `comments` publish option (seeded when the post is created) and
//! from `MockScript::push_comment`. The account's own replies are left out, as a platform's
//! notifications leave them out. Comments on a post that is not live yet (processing, or
//! natively scheduled for later) are not shown; the comments seeded on such a post are added
//! when it goes live, so a cursor taken before that does not skip them. The cursor is the ID of
//! the last comment returned.

use async_trait::async_trait;
use postit_core::{AccountRef, AgentError, InboxItem, InboxPage, InboxReader};

use super::handle;
use crate::client::MockClient;
use crate::platform::{Call, MockComment};

fn item(comment: &MockComment) -> InboxItem {
    InboxItem {
        remote_comment_id: comment.remote_id.clone(),
        remote_post_id: comment.post_remote_id.clone(),
        parent_remote_id: comment.parent_remote_id.clone(),
        author_handle: comment.author.clone(),
        author_display_name: None,
        text: comment.text.clone(),
        created_at: comment.created_at,
    }
}

#[async_trait]
impl InboxReader for MockClient {
    /// Comments after `cursor`. A cursor the platform does not know (its state is lost on
    /// restart) reads from the start, so a poller never stalls on a stale position. When
    /// nothing is new, the given cursor comes back unchanged.
    async fn fetch_since(
        &self,
        account: &AccountRef,
        cursor: Option<&str>,
    ) -> Result<InboxPage, AgentError> {
        self.platform().call(Call::Read).await;
        let own = handle(&account.remote_id);
        let now = self.now();
        let mut state = self.platform().lock();
        state.release_seeds(now);
        let start = cursor
            .and_then(|c| state.comments.iter().position(|x| x.remote_id == c))
            .map_or(0, |i| i + 1);
        let items: Vec<InboxItem> = state.comments[start..]
            .iter()
            .filter(|c| c.author != own)
            .filter(|c| {
                state
                    .post(&c.post_remote_id)
                    .is_some_and(|p| p.account == account.remote_id && p.live_at(now))
            })
            .map(item)
            .collect();
        let cursor = items
            .last()
            .map(|i| i.remote_comment_id.clone())
            .or_else(|| cursor.map(str::to_owned));
        Ok(InboxPage { items, cursor })
    }
}
