//! Applying email events: what a provider or a recipient reports about a sent message.

use postit_core::{DirectSendId, EmailEventKind, UserId};
use postit_data::OwnerScope;
use postit_data::contacts::{SuppressionReason, SuppressionRepo};
use postit_data::publishing::direct::DirectRepo;
use postit_jobs::JobError;
use sqlx::Connection as _;

use crate::{ContactError, ContactService, EmailEventApply};

fn job_error(err: &ContactError) -> JobError {
    let what = match err {
        ContactError::Data(_) => "a database error",
        _ => "an unexpected error",
    };
    JobError::Retry(format!("applying an email event stopped on {what}"))
}

impl ContactService {
    /// The `email_event_apply` job: records the event on its send and, for a bounce or a
    /// complaint, suppresses the send's address. The event and the suppression commit together.
    /// A send that is gone (purged) or not the owner's changes nothing. A once-only kind
    /// already recorded changes nothing either, but the suppression is still applied, so a
    /// retry after a partial failure finishes the work.
    ///
    /// # Errors
    ///
    /// [`JobError::Retry`] on a database failure.
    pub async fn apply_event(&self, job: EmailEventApply) -> Result<(), JobError> {
        self.apply(job).await.map_err(|err| job_error(&err))
    }

    async fn apply(&self, job: EmailEventApply) -> Result<(), ContactError> {
        let owner = UserId::from_uuid(job.owner);
        let send = DirectSendId::from_uuid(job.send);
        let scope = OwnerScope::own(owner);
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;
        let found =
            DirectRepo::record_event(&mut tx, owner, send, job.kind, None, self.clock.now())
                .await?;
        if !found {
            return Ok(());
        }
        let reason = match job.kind {
            EmailEventKind::Bounced => Some(SuppressionReason::Bounced),
            EmailEventKind::Complained => Some(SuppressionReason::Complained),
            _ => None,
        };
        if let Some(reason) = reason
            && let Some(hash) = DirectRepo::send_hash(&mut tx, owner, send).await?
        {
            SuppressionRepo::add_hash(&mut tx, &scope, hash, reason).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
