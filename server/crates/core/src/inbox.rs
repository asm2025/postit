//! Comments read from a platform.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One comment on one of the account's posts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InboxItem {
    /// The platform's ID for the comment.
    pub remote_comment_id: String,
    /// The platform's ID for the post it is on.
    pub remote_post_id: String,
    /// The comment this one answers, for a nested reply.
    pub parent_remote_id: Option<String>,
    /// Who wrote it, as `@handle` or the platform's equivalent.
    pub author_handle: String,
    /// The author's display name, when the platform has one.
    pub author_display_name: Option<String>,
    /// The comment text.
    pub text: String,
    /// When it was written.
    pub created_at: DateTime<Utc>,
}

/// A page of comments with the position to continue from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InboxPage {
    /// The comments, oldest first.
    pub items: Vec<InboxItem>,
    /// Opaque position for the next poll; `None` when the platform gave none.
    pub cursor: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn inbox_item_round_trips() {
        let item = InboxItem {
            remote_comment_id: "c1".to_owned(),
            remote_post_id: "p1".to_owned(),
            parent_remote_id: None,
            author_handle: "@a".to_owned(),
            author_display_name: Some("A".to_owned()),
            text: "hi".to_owned(),
            created_at: Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap(),
        };
        let page = InboxPage {
            items: vec![item],
            cursor: Some("next".to_owned()),
        };
        let json = serde_json::to_string(&page).unwrap();
        assert_eq!(serde_json::from_str::<InboxPage>(&json).unwrap(), page);
    }
}
