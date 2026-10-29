//! The coalesced alert email: every critical notification of a user not yet emailed, in one
//! message.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::notifications::NotificationRepo;
use postit_data::users::UserRecord;
use postit_mail::{
    LoadOutcome, MailContent, MailContextLoader, MailError, MailKind, MailLoaders, MailParams,
};
use sqlx::PgConnection;

/// `notification_alert`: takes every critical row of the recipient not yet emailed and lists
/// them in one email; skips when an earlier job already took them.
///
/// The rows are marked emailed in [`MailContextLoader::load`], because `mark_sent` is not told
/// which rows the email included. `load` runs in the handler's transaction, which rolls back
/// when the send fails, so the rows are marked exactly when the email goes out.
pub struct NotificationAlertLoader;

#[async_trait]
impl MailContextLoader for NotificationAlertLoader {
    async fn load(
        &self,
        conn: &mut PgConnection,
        recipient: &UserRecord,
        _params: &MailParams,
        now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        let taken = NotificationRepo::take_unemailed_critical(conn, recipient.id, now).await?;
        if taken.is_empty() {
            return Ok(LoadOutcome::Skip);
        }
        Ok(LoadOutcome::Send(MailContent::NotificationAlert {
            notices: taken.into_iter().map(|row| row.notice).collect(),
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

/// Registers this crate's mail context loaders. `postit-server` calls this before
/// `postit_mail::register`.
///
/// # Errors
///
/// [`MailError::DuplicateLoader`] if a kind already has a loader.
pub fn register_loaders(loaders: &mut MailLoaders) -> Result<(), MailError> {
    loaders.register(
        MailKind::NotificationAlert,
        Arc::new(NotificationAlertLoader),
    )
}
