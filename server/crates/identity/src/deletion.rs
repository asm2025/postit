use std::sync::Arc;
use std::time::Duration;

use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserStatus, UsersRepo};
use postit_jobs::{Job, JobContext, JobError, JobQueue, Queue, RetryPolicy};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::IdentityError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteUser {
    pub user_id: Uuid,
}

impl Job for DeleteUser {
    const JOB_TYPE: &'static str = "delete_user";
    const QUEUE: Queue = Queue::Default;
}

/// Never gives up: a stuck deletion stays visible and retryable in the admin console.
#[must_use]
pub fn delete_user_retry_policy() -> RetryPolicy {
    RetryPolicy::Backoff {
        max_attempts: None,
        initial: Duration::from_secs(30),
        max: Duration::from_secs(3600),
    }
}

/// Sets `target` to the terminal `deleting` status, records `user_deleted`, and enqueues
/// `delete_user`, all in `conn`'s transaction. The caller has locked and checked `target`.
pub(crate) async fn start_deletion(
    conn: &mut PgConnection,
    ids: &dyn IdGenerator,
    jobs: &JobQueue,
    target: UserId,
    actor: Option<UserId>,
) -> Result<(), IdentityError> {
    UsersRepo::set_status(conn, target, UserStatus::Deleting, None).await?;
    let mut event = AuditEvent::new(AuditEventKind::UserDeleted).subject(target);
    if let Some(actor) = actor {
        event = event.actor(actor);
    }
    AuditLog::record(conn, AuditEventId::from(ids.generate()), event).await?;
    jobs.enqueue_in(
        conn,
        &DeleteUser {
            user_id: target.as_uuid(),
        },
        None,
    )
    .await?;
    Ok(())
}

#[derive(Clone)]
pub struct DeleteUserHandler {
    pool: PgPool,
    pseudonym_key: Arc<SecretString>,
    fail_after: Option<u8>,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

impl DeleteUserHandler {
    #[must_use]
    pub fn new(pool: PgPool, pseudonym_key: SecretString) -> Self {
        Self {
            pool,
            pseudonym_key: Arc::new(pseudonym_key),
            fail_after: None,
        }
    }

    /// Test hook: fail with a retryable error right after step `step` completes.
    #[cfg(feature = "testkit")]
    #[must_use]
    pub fn failing_after(mut self, step: u8) -> Self {
        self.fail_after = Some(step);
        self
    }

    fn checkpoint(&self, step: u8) -> Result<(), JobError> {
        if self.fail_after == Some(step) {
            return Err(JobError::Retry(format!(
                "injected failure after step {step}"
            )));
        }
        Ok(())
    }

    /// Each step commits on its own and is safe to repeat, so a crash anywhere resumes.
    /// Plan 03 inserts its steps (cancel publish jobs, remote deletes, token revocation,
    /// owned rows and media, delegations) between steps 1 and 2.
    ///
    /// # Errors
    ///
    /// [`JobError::Retry`] on a database failure; [`JobError::Fatal`] if the user exists
    /// but is not `deleting` (a job should never exist for such a user).
    pub async fn handle(&self, job: DeleteUser, _ctx: JobContext) -> Result<(), JobError> {
        let user = UserId::from(job.user_id);

        // 1. Load.
        let mut conn = self.pool.acquire().await.map_err(retry)?;
        let Some(record) = UsersRepo::find_by_id(&mut conn, user)
            .await
            .map_err(retry)?
        else {
            return Ok(()); // already fully deleted
        };
        if record.status != UserStatus::Deleting {
            return Err(JobError::Fatal(
                "delete_user job for a user that is not deleting".into(),
            ));
        }
        drop(conn);
        self.checkpoint(1)?;

        // 2. Pseudonymize every audit reference written so far.
        let mut tx = self.pool.begin().await.map_err(retry)?;
        AuditLog::pseudonymize_user(&mut tx, user, self.pseudonym_key.expose_secret().as_bytes())
            .await
            .map_err(retry)?;
        tx.commit().await.map_err(retry)?;
        self.checkpoint(2)?;

        // 3. Lock the row, re-scrub anything a concurrent job (e.g. `send_email`, which
        // locks the still-`deleting` row and can write an `EmailDropped`/`EmailFailed`
        // audit with the raw user ID) wrote in the gap since step 2 committed, then delete
        // the row — all in one transaction, so nothing can slip in between the final scrub
        // and the delete. Idempotent: a retry that finds no row here (already deleted)
        // returns `Ok` with nothing to do.
        let mut tx = self.pool.begin().await.map_err(retry)?;
        if UsersRepo::lock_by_id(&mut tx, user)
            .await
            .map_err(retry)?
            .is_none()
        {
            return Ok(());
        }
        AuditLog::pseudonymize_user(&mut tx, user, self.pseudonym_key.expose_secret().as_bytes())
            .await
            .map_err(retry)?;
        UsersRepo::delete(&mut tx, user).await.map_err(retry)?;
        tx.commit().await.map_err(retry)?;
        self.checkpoint(3)?;

        Ok(())
    }
}
