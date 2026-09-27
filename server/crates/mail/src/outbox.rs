use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_jobs::{Job, JobId, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::MailError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailKind {
    /// To an admin: users waiting for approval (coalesced).
    UserPendingApproval,
    /// To a user: their account was approved.
    UserApproved,
}

impl MailKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserPendingApproval => "user_pending_approval",
            Self::UserApproved => "user_approved",
        }
    }
}

/// Typed ID parameters for a mail kind. Neither P5 kind needs any; plan 03's
/// notification kinds add variants carrying notification IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailParams {
    #[default]
    None,
}

/// The `send_email` payload: IDs only. Rendering reads current data when the job runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmail {
    pub kind: MailKind,
    pub recipient: Uuid,
    pub params: MailParams,
}

impl Job for SendEmail {
    const JOB_TYPE: &'static str = "send_email";
    const QUEUE: Queue = Queue::Mail;
}

/// The only way to send mail. Callers never send inline.
#[derive(Clone)]
pub struct MailOutbox {
    queue: JobQueue,
}

impl MailOutbox {
    #[must_use]
    pub fn new(queue: JobQueue) -> Self {
        Self { queue }
    }

    /// Enqueues `send_email` in `conn` (the caller's transaction, when it is one).
    ///
    /// # Errors
    ///
    /// Returns [`MailError::Jobs`] if the enqueue fails.
    pub async fn send(
        &self,
        conn: &mut PgConnection,
        kind: MailKind,
        recipient: UserId,
        params: MailParams,
        run_at: Option<DateTime<Utc>>,
    ) -> Result<JobId, MailError> {
        let job = SendEmail {
            kind,
            recipient: recipient.as_uuid(),
            params,
        };
        Ok(self.queue.enqueue_in(conn, &job, run_at).await?)
    }
}
