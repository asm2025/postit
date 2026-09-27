use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use postit_config::{AuditSettings, JobSchedules};
use postit_core::IdGenerator;
use postit_jobs::{JobContext, JobError, JobQueue, JobRegistry, RetryPolicy};
use postit_mail::{MailKind, MailLoaders};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::deletion::{DeleteUser, DeleteUserHandler, delete_user_retry_policy};
use crate::error::IdentityError;
use crate::mail::{ApprovedLoader, PendingApprovalLoader};
use crate::maintenance::{AuditRetention, PurgePendingUsers, audit_retention, purge_pending_users};

pub struct IdentityJobs {
    pub pool: PgPool,
    pub ids: Arc<dyn IdGenerator>,
    pub jobs: JobQueue,
    pub pending_ttl: Duration,
    pub approval_email_interval: Duration,
    pub audit: AuditSettings,
    pub schedules: JobSchedules,
}

fn maintenance_retry() -> RetryPolicy {
    RetryPolicy::Backoff {
        max_attempts: Some(3),
        initial: Duration::from_secs(60),
        max: Duration::from_secs(600),
    }
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

/// Registers `delete_user`, `purge_pending_users`, `audit_retention`, and the two mail
/// context loaders. `postit-server` calls this in P6, before `postit_mail::register`.
///
/// # Errors
///
/// Returns [`IdentityError`] if a job type or mail kind is already registered, or a
/// schedule is invalid.
pub fn register(
    registry: &mut JobRegistry,
    loaders: &mut MailLoaders,
    deps: IdentityJobs,
) -> Result<(), IdentityError> {
    loaders.register(
        MailKind::UserPendingApproval,
        Arc::new(PendingApprovalLoader::new(deps.approval_email_interval)),
    )?;
    loaders.register(MailKind::UserApproved, Arc::new(ApprovedLoader))?;

    let delete = DeleteUserHandler::new(
        deps.pool.clone(),
        SecretString::from(deps.audit.pseudonym_key.expose().to_owned()),
    );
    registry.register(
        delete_user_retry_policy(),
        move |job: DeleteUser, ctx: JobContext| {
            let delete = delete.clone();
            async move { delete.handle(job, ctx).await }
        },
    )?;

    let (pool, ids, jobs, ttl) = (
        deps.pool.clone(),
        Arc::clone(&deps.ids),
        deps.jobs.clone(),
        deps.pending_ttl,
    );
    registry.register_recurring(
        &deps.schedules.purge_pending_users,
        maintenance_retry(),
        move |_: PurgePendingUsers, _: JobContext| {
            let (pool, ids, jobs) = (pool.clone(), Arc::clone(&ids), jobs.clone());
            async move {
                purge_pending_users(&pool, ids.as_ref(), &jobs, ttl, Utc::now())
                    .await
                    .map(|_| ())
                    .map_err(retry)
            }
        },
    )?;

    let (pool, audit) = (deps.pool, deps.audit);
    registry.register_recurring(
        &deps.schedules.audit_retention,
        maintenance_retry(),
        move |_: AuditRetention, _: JobContext| {
            let (pool, audit) = (pool.clone(), audit.clone());
            async move { audit_retention(&pool, &audit).await.map_err(retry) }
        },
    )?;

    Ok(())
}
