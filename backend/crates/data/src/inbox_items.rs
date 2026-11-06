//! A read-only look at inbox items, for the drafter's reply mode.
//!
//! The inbox itself (polling, status, replies) arrives with plan 03 phase E3. This module only
//! reads the fields a reply draft needs, through the caller's scope.

use postit_core::{AccountId, DeliveryId, InboxItemId};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::{Capability, OwnerScope};

/// The fields of an inbox item a reply draft needs. Not `InboxItem`, which `postit-core`
/// already defines for agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemRow {
    /// The item.
    pub id: InboxItemId,
    /// The account the comment arrived on.
    pub account_id: AccountId,
    /// The delivery the comment answers, when it is known.
    pub delivery_id: Option<DeliveryId>,
    /// The platform's ID of the comment.
    pub remote_comment_id: String,
    /// The platform's ID of the post it is on.
    pub remote_post_id: Option<String>,
    /// The commenter's display name.
    pub author_name: Option<String>,
    /// The commenter's handle.
    pub author_handle: Option<String>,
    /// The comment.
    pub text: String,
}

struct Row {
    id: Uuid,
    account_id: Uuid,
    delivery_id: Option<Uuid>,
    remote_comment_id: String,
    remote_post_id: Option<String>,
    author_name: Option<String>,
    author_handle: Option<String>,
    text: String,
}

impl From<Row> for InboxItemRow {
    fn from(row: Row) -> Self {
        Self {
            id: InboxItemId::from_uuid(row.id),
            account_id: AccountId::from_uuid(row.account_id),
            delivery_id: row.delivery_id.map(DeliveryId::from_uuid),
            remote_comment_id: row.remote_comment_id,
            remote_post_id: row.remote_post_id,
            author_name: row.author_name,
            author_handle: row.author_handle,
            text: row.text,
        }
    }
}

/// Inbox items.
pub struct InboxItemRepo;

impl InboxItemRepo {
    /// One item, or `None` when the owner has none with this ID or it is on an account outside
    /// the caller's limit.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] below the view level, and [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: InboxItemId,
    ) -> Result<Option<InboxItemRow>, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        let row = sqlx::query_as!(
            Row,
            "SELECT id, account_id, delivery_id, remote_comment_id, remote_post_id,
                    author_name, author_handle, text
             FROM inbox_items
             WHERE owner_id = $1 AND id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }
}
