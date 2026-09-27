use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::users::UserRecord;
use postit_mail::{LoadOutcome, MailContent, MailContextLoader, MailError, MailParams};
use sqlx::PgConnection;

/// `user_approved`: always sent; no sent marker (a one-off account email, at-least-once).
pub struct ApprovedLoader;

#[async_trait]
impl MailContextLoader for ApprovedLoader {
    async fn load(
        &self,
        _conn: &mut PgConnection,
        recipient: &UserRecord,
        _params: &MailParams,
        _now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        Ok(LoadOutcome::Send(MailContent::UserApproved {
            display_name: recipient.display_name.clone(),
        }))
    }

    async fn mark_sent(
        &self,
        _conn: &mut PgConnection,
        _recipient: UserId,
        _at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        Ok(())
    }
}
