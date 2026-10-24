//! `MetricsReader`: deterministic engagement numbers computed from each post.
//!
//! The formula, for a live post whose text has `n` characters (Unicode scalar values):
//!
//! - `views` = [`BASE_VIEWS`] + [`VIEWS_PER_CHAR`] × `n`
//! - `likes` = `views` / 10, `shares` = `likes` / 4, `saves` = `likes` / 5 (integer division)
//! - `comments` = the comments stored on the post
//! - `clicks` is not reported (`None`): mock posts carry no tracked link.
//!
//! A post that is not live yet (processing or natively scheduled) has seen nothing: every
//! reported number is zero. Account numbers sum the account's posts: `reach` is the sum of
//! their views and `followers` the sum of their likes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::{
    AccountMetrics, AccountRef, AgentError, MetricSnapshot, MetricsReader, PublishReceipt,
};

use super::not_found;
use crate::client::MockClient;
use crate::platform::{Call, MockPost, State};

/// Views every live post starts with.
pub const BASE_VIEWS: u64 = 100;
/// Views each character of a live post's text adds.
pub const VIEWS_PER_CHAR: u64 = 10;

fn snapshot(state: &State, post: &MockPost, collected_at: DateTime<Utc>) -> MetricSnapshot {
    let comments = state
        .comments
        .iter()
        .filter(|c| c.post_remote_id == post.remote_id)
        .count();
    let chars = u64::try_from(post.text.chars().count()).unwrap_or(u64::MAX);
    let views = if post.live {
        BASE_VIEWS.saturating_add(VIEWS_PER_CHAR.saturating_mul(chars))
    } else {
        0
    };
    let likes = views / 10;
    MetricSnapshot {
        views: Some(views),
        likes: Some(likes),
        comments: Some(if post.live {
            u64::try_from(comments).unwrap_or(u64::MAX)
        } else {
            0
        }),
        shares: Some(likes / 4),
        saves: Some(likes / 5),
        clicks: None,
        collected_at,
    }
}

#[async_trait]
impl MetricsReader for MockClient {
    async fn post_metrics(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<MetricSnapshot, AgentError> {
        self.platform().call(Call::Read).await;
        let now = self.now();
        let state = self.platform().lock();
        receipt
            .remote_id
            .as_deref()
            .and_then(|id| state.post(id))
            .filter(|p| p.account == account.remote_id)
            .map(|post| snapshot(&state, post, now))
            .ok_or_else(not_found)
    }

    async fn account_metrics(&self, account: &AccountRef) -> Result<AccountMetrics, AgentError> {
        self.platform().call(Call::Read).await;
        let now = self.now();
        let state = self.platform().lock();
        let (reach, followers) = state
            .all_posts()
            .iter()
            .filter(|p| p.account == account.remote_id)
            .map(|p| snapshot(&state, p, now))
            .fold((0_u64, 0_u64), |(reach, followers), s| {
                (
                    reach.saturating_add(s.views.unwrap_or(0)),
                    followers.saturating_add(s.likes.unwrap_or(0)),
                )
            });
        Ok(AccountMetrics {
            followers: Some(followers),
            reach: Some(reach),
        })
    }
}
