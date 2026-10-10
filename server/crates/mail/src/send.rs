use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserStatus, UsersRepo};
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, JobContext, JobError, JobRegistry, JobsError, RetryPolicy,
};
use sqlx::{PgConnection, PgPool};
use url::Url;

use crate::error::MailError;
use crate::loaders::{LoadOutcome, MailLoaders};
use crate::mailer::{Mailer, RenderedMessage};
use crate::outbox::{MailKind, MailOutbox, SendEmail};
use crate::templates::render;

/// Everything the `send_email` handler needs.
pub struct SendEmailDeps {
    /// Database pool for the handler's transaction.
    pub pool: PgPool,
    /// Source of audit event IDs.
    pub ids: Arc<dyn IdGenerator>,
    /// Transport that delivers the rendered message.
    pub mailer: Arc<dyn Mailer>,
    /// Context loaders, one per mail kind.
    pub loaders: MailLoaders,
    /// Outbox used to re-enqueue deferred mail.
    pub outbox: MailOutbox,
    /// Base URL of the web app, used for links in emails.
    pub app_url: Url,
    /// Attempts before a failure becomes final.
    pub max_attempts: u32,
}

/// Handler for the `send_email` job.
#[derive(Clone)]
pub struct SendEmailHandler {
    deps: Arc<SendEmailDeps>,
}

struct DeliverArgs<'a> {
    tx: sqlx::Transaction<'a, sqlx::Postgres>,
    loader: &'a (dyn crate::loaders::MailContextLoader + 'a),
    recipient: &'a postit_data::users::UserRecord,
    kind: MailKind,
    content: &'a crate::templates::MailContent,
    now: chrono::DateTime<Utc>,
    is_last_attempt: bool,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

fn drop_reason(recipient: &postit_data::users::UserRecord) -> Option<&'static str> {
    if recipient.status != UserStatus::Active {
        Some("inactive")
    } else if !recipient.email_verified || recipient.email.is_none() {
        Some("no_verified_email")
    } else {
        None
    }
}

impl SendEmailHandler {
    /// A handler over `deps`.
    #[must_use]
    pub fn new(deps: SendEmailDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// # Errors
    ///
    /// [`JobError::Retry`] for database or transient SMTP failures, including on the last
    /// attempt (the dispatcher turns that into an abort once the budget is exhausted);
    /// [`JobError::Fatal`] for a permanent SMTP failure or a mail kind with no registered
    /// loader.
    pub async fn handle(&self, job: SendEmail, ctx: JobContext) -> Result<(), JobError> {
        let d = &self.deps;
        let recipient_id = UserId::from(job.recipient);
        let mut tx = d.pool.begin().await.map_err(retry)?;
        // The database clock, not this process's: apalis schedules run_at by the database's
        // now(), so comparing and deferring against it cannot churn on app/DB clock skew.
        let now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx)
            .await
            .map_err(retry)?;

        // FOR NO KEY UPDATE serializes concurrent sends to one recipient (coalescing depends
        // on it) without blocking audit writes, which take FOR KEY SHARE on this row.
        let Some(recipient) = UsersRepo::lock_for_send(&mut tx, recipient_id)
            .await
            .map_err(retry)?
        else {
            // No audit row: writing the raw ID of a deleted (pseudonymized) user would
            // re-introduce it into audit_events.
            tracing::info!(
                kind = job.kind.as_str(),
                "mail recipient no longer exists; dropped"
            );
            return Ok(());
        };

        if let Some(reason) = drop_reason(&recipient) {
            self.audit(
                &mut tx,
                AuditEventKind::EmailDropped,
                recipient_id,
                job.kind,
                Some(reason),
            )
            .await
            .map_err(retry)?;
            tx.commit().await.map_err(retry)?;
            return Ok(());
        }

        let loader = d
            .loaders
            .get(job.kind)
            .ok_or_else(|| JobError::Fatal(MailError::NoLoader(job.kind).to_string()))?;
        let content = match loader
            .load(&mut tx, &recipient, &job.params, now)
            .await
            .map_err(retry)?
        {
            LoadOutcome::Skip => {
                tx.commit().await.map_err(retry)?;
                return Ok(());
            }
            LoadOutcome::Defer(at) => {
                let at = at.max(now + chrono::Duration::seconds(1));
                d.outbox
                    .send(
                        &mut tx,
                        job.kind,
                        recipient_id,
                        job.params.clone(),
                        Some(at),
                    )
                    .await
                    .map_err(retry)?;
                tx.commit().await.map_err(retry)?;
                return Ok(());
            }
            LoadOutcome::Send(content) => content,
        };

        self.deliver(DeliverArgs {
            tx,
            loader: loader.as_ref(),
            recipient: &recipient,
            kind: job.kind,
            content: &content,
            now,
            is_last_attempt: ctx.is_last_attempt(),
        })
        .await
    }

    /// Renders and sends `content`, then either marks it sent and audits `EmailSent` (on
    /// success), retries (transient failure with attempts left), or audits `EmailFailed`
    /// and gives up (permanent failure or last attempt).
    async fn deliver(&self, args: DeliverArgs<'_>) -> Result<(), JobError> {
        let DeliverArgs {
            mut tx,
            loader,
            recipient,
            kind,
            content,
            now,
            is_last_attempt,
        } = args;
        let recipient_id = recipient.id;
        let d = &self.deps;
        let rendered = render(content, &d.app_url).map_err(|e| JobError::Fatal(e.to_string()))?;
        let message = RenderedMessage {
            to: recipient.email.clone().unwrap_or_default(),
            subject: rendered.subject,
            text: rendered.text,
            html: rendered.html,
        };

        match d.mailer.send(message).await {
            Ok(()) => {
                loader
                    .mark_sent(&mut tx, recipient_id, now)
                    .await
                    .map_err(retry)?;
                self.audit(&mut tx, AuditEventKind::EmailSent, recipient_id, kind, None)
                    .await
                    .map_err(retry)?;
                tx.commit().await.map_err(retry)?;
                Ok(())
            }
            Err(err) => {
                drop(tx);
                let give_up = matches!(err, MailError::Permanent(_)) || is_last_attempt;
                if !give_up {
                    return Err(JobError::Retry(err.to_string()));
                }
                // Re-lock the recipient in a fresh transaction before auditing. The send
                // above held the row lock, but `drop(tx)` released it, so between that drop
                // and the re-lock below the user may have been deleted (and its audit rows
                // pseudonymized). Writing the raw ID then would re-introduce it into
                // audit_events.
                let mut tx = d.pool.begin().await.map_err(retry)?;
                if UsersRepo::lock_for_send(&mut tx, recipient_id)
                    .await
                    .map_err(retry)?
                    .is_some()
                {
                    self.audit(
                        &mut tx,
                        AuditEventKind::EmailFailed,
                        recipient_id,
                        kind,
                        None,
                    )
                    .await
                    .map_err(retry)?;
                    tx.commit().await.map_err(retry)?;
                } else {
                    tracing::info!(
                        kind = kind.as_str(),
                        "mail recipient no longer exists; failure not audited"
                    );
                }
                // A permanent failure is fatal. Running out of attempts is not the handler's
                // verdict: returning `Retry` lets `dispatch` see the exhausted budget and record
                // the job `dead` (retries_exhausted) instead of `killed`.
                Err(match err {
                    MailError::Permanent(_) => JobError::Fatal(err.to_string()),
                    _ => JobError::Retry(err.to_string()),
                })
            }
        }
    }

    async fn audit(
        &self,
        conn: &mut PgConnection,
        kind: AuditEventKind,
        recipient: UserId,
        mail_kind: MailKind,
        reason: Option<&str>,
    ) -> Result<(), MailError> {
        let mut event = AuditEvent::new(kind)
            .subject(recipient)
            .detail("mail_kind", mail_kind.as_str())?;
        if let Some(reason) = reason {
            event = event.detail("reason", reason)?;
        }
        AuditLog::record(conn, AuditEventId::from(self.deps.ids.generate()), event).await?;
        Ok(())
    }

    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy::Backoff {
            max_attempts: Some(self.deps.max_attempts),
            initial: Duration::from_secs(30),
            max: Duration::from_mins(30),
        }
    }
}

/// Describes `send_email` to the admin console: the recipient and the mail kind, nothing else.
pub fn console(catalog: &mut ConsoleCatalog) {
    catalog.add::<SendEmail>(
        ConsoleSpec::new()
            .id_field("recipient", "recipient_user_id")
            .enum_field("kind", "mail_kind", MailKind::NAMES),
    );
}

/// Registers `send_email`. `postit-server` calls this in P6 after every crate has added
/// its loaders to the [`MailLoaders`] inside `handler`.
///
/// # Errors
///
/// Returns [`JobsError::DuplicateJobType`] if `send_email` is already registered.
pub fn register(registry: &mut JobRegistry, handler: SendEmailHandler) -> Result<(), JobsError> {
    let policy = handler.retry_policy();
    registry.register(policy, move |job: SendEmail, ctx: JobContext| {
        let handler = handler.clone();
        async move { handler.handle(job, ctx).await }
    })
}
