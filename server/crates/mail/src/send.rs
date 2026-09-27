use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserStatus, UsersRepo};
use postit_jobs::{JobContext, JobError, JobRegistry, JobsError, RetryPolicy};
use sqlx::{PgConnection, PgPool};
use url::Url;

use crate::error::MailError;
use crate::loaders::{LoadOutcome, MailLoaders};
use crate::mailer::{Mailer, RenderedMessage};
use crate::outbox::{MailKind, MailOutbox, SendEmail};
use crate::templates::render;

pub struct SendEmailDeps {
    pub pool: PgPool,
    pub ids: Arc<dyn IdGenerator>,
    pub mailer: Arc<dyn Mailer>,
    pub loaders: MailLoaders,
    pub outbox: MailOutbox,
    pub app_url: Url,
    pub max_attempts: u32,
}

#[derive(Clone)]
pub struct SendEmailHandler {
    deps: Arc<SendEmailDeps>,
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
    #[must_use]
    pub fn new(deps: SendEmailDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// # Errors
    ///
    /// [`JobError::Retry`] for database or transient SMTP failures with attempts left;
    /// [`JobError::Fatal`] for a permanent SMTP failure, the last attempt, or a mail kind
    /// with no registered loader.
    pub async fn handle(&self, job: SendEmail, ctx: JobContext) -> Result<(), JobError> {
        let d = &self.deps;
        let recipient_id = UserId::from(job.recipient);
        let now = Utc::now();
        let mut tx = d.pool.begin().await.map_err(retry)?;

        // The row lock serializes concurrent sends to one recipient; coalescing depends on it.
        let Some(recipient) = UsersRepo::lock_by_id(&mut tx, recipient_id)
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

        self.deliver(
            tx,
            loader.as_ref(),
            &recipient,
            recipient_id,
            job.kind,
            &content,
            now,
            &ctx,
        )
        .await
    }

    /// Renders and sends `content`, then either marks it sent and audits `EmailSent` (on
    /// success), retries (transient failure with attempts left), or audits `EmailFailed`
    /// and gives up (permanent failure or last attempt).
    #[allow(clippy::too_many_arguments)]
    async fn deliver(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        loader: &(dyn crate::loaders::MailContextLoader + '_),
        recipient: &postit_data::users::UserRecord,
        recipient_id: UserId,
        kind: MailKind,
        content: &crate::templates::MailContent,
        now: chrono::DateTime<Utc>,
        ctx: &JobContext,
    ) -> Result<(), JobError> {
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
                let give_up = matches!(err, MailError::Permanent(_)) || ctx.is_last_attempt();
                if !give_up {
                    return Err(JobError::Retry(err.to_string()));
                }
                let mut conn = d.pool.acquire().await.map_err(retry)?;
                self.audit(
                    &mut conn,
                    AuditEventKind::EmailFailed,
                    recipient_id,
                    kind,
                    None,
                )
                .await
                .map_err(retry)?;
                Err(JobError::Fatal(err.to_string()))
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
