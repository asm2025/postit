//! The optional capabilities, one module each, all implemented on [`MockClient`] and exposed
//! through the `AgentClient` accessor of the same name only when the descriptor declares them
//! (`mock` and `mock_oauth`: everything but direct sends; `mock_direct`: direct sends only).
//!
//! - [`comment`]: `Commenter`, replies to stored posts with the same `outcome` scripting and
//!   idempotency as publishing.
//! - [`inbox`]: `InboxReader`, the comments on the account's posts in creation order.
//! - [`metrics`]: `MetricsReader`, deterministic numbers computed from each post.
//! - [`schedule`]: `NativeScheduler`, moving and removing natively scheduled posts.
//! - [`direct`]: `DirectSender`, results chosen by each recipient's address.
//!
//! [`MockClient`]: crate::MockClient

pub mod comment;
pub mod direct;
pub mod inbox;
pub mod metrics;
pub mod schedule;

use postit_core::AgentError;

use crate::credentials::ACCOUNT_PREFIX;

/// What the mock platform answers for a post or comment it does not have.
pub(crate) fn not_found() -> AgentError {
    AgentError::Remote {
        status: Some(404),
        code: Some("not_found".to_owned()),
        message: "the mock platform has no such post".to_owned(),
    }
}

/// The account's handle, the author of its replies: `@shop` for `mock-account-shop`.
pub(crate) fn handle(account_remote_id: &str) -> String {
    let name = account_remote_id
        .strip_prefix(ACCOUNT_PREFIX)
        .unwrap_or(account_remote_id);
    format!("@{name}")
}
