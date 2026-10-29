//! Registration of the identity jobs.

use std::sync::Arc;
use std::time::Duration;

use postit_config::{AuditSettings, JobSchedules};
use postit_core::{Clock, IdGenerator};
use postit_jobs::{
    Actions, ConsoleCatalog, ConsoleSpec, DisabledReason, JobContext, JobError, JobQueue,
    JobRegistry, RetryPolicy,
};
use postit_mail::{MailKind, MailLoaders};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::delegation::{DelegationExpiry, expire_delegations};
use crate::deletion::{DeleteUser, DeleteUserHandler, UserDeletionStep, delete_user_retry_policy};
use crate::error::IdentityError;
use crate::mail::{ApprovedLoader, PendingApprovalLoader};
use crate::maintenance::{AuditRetention, PurgePendingUsers, audit_retention, purge_pending_users};

/// Everything needed to register the identity jobs.
pub struct IdentityJobs {
    /// Database pool.
    pub pool: PgPool,
    /// Source of ids.
    pub ids: Arc<dyn IdGenerator>,
    /// Source of time.
    pub clock: Arc<dyn Clock>,
    /// Queue used to enqueue follow-up jobs.
    pub jobs: JobQueue,
    /// How long a user may stay `pending`.
    pub pending_ttl: Duration,
    /// Minimum interval between approval emails to one admin.
    pub approval_email_interval: Duration,
    /// Audit retention windows and the pseudonym key.
    pub audit: AuditSettings,
    /// Cron schedules of the recurring jobs.
    pub schedules: JobSchedules,
    /// Steps other crates add to `delete_user`, run in this order.
    pub deletion_steps: Vec<Arc<dyn UserDeletionStep>>,
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

/// Describes the identity job types to the admin console: `delete_user` shows its `user_id`,
/// the two maintenance jobs report their schedules.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    // Cancel and delete could strand a user in `deleting`: only this job erases them.
    catalog.add::<DeleteUser>(ConsoleSpec::new().id_field("user_id", "user_id").restrict(
        Actions {
            retry: true,
            cancel: false,
            delete: false,
        },
        DisabledReason::MustFinish,
    ));
    catalog.add::<PurgePendingUsers>(ConsoleSpec::new().recurring(&schedules.purge_pending_users));
    catalog.add::<AuditRetention>(ConsoleSpec::new().recurring(&schedules.audit_retention));
    catalog.add::<DelegationExpiry>(ConsoleSpec::new().recurring(&schedules.delegation_expiry));
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
    )
    .with_steps(deps.deletion_steps);
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
    let purge_clock = Arc::clone(&deps.clock);
    registry.register_recurring(
        &deps.schedules.purge_pending_users,
        maintenance_retry(),
        move |_: PurgePendingUsers, _: JobContext| {
            let (pool, ids, jobs) = (pool.clone(), Arc::clone(&ids), jobs.clone());
            let clock = Arc::clone(&purge_clock);
            async move {
                purge_pending_users(&pool, ids.as_ref(), &jobs, ttl, clock.now())
                    .await
                    .map(|_| ())
                    .map_err(retry)
            }
        },
    )?;

    let (pool, ids, clock) = (
        deps.pool.clone(),
        Arc::clone(&deps.ids),
        Arc::clone(&deps.clock),
    );
    registry.register_recurring(
        &deps.schedules.delegation_expiry,
        maintenance_retry(),
        move |_: DelegationExpiry, _: JobContext| {
            let (pool, ids, clock) = (pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
            async move {
                expire_delegations(&pool, ids.as_ref(), clock.now())
                    .await
                    .map(|_| ())
                    .map_err(retry)
            }
        },
    )?;

    let (pool, audit, clock) = (deps.pool, deps.audit, deps.clock);
    registry.register_recurring(
        &deps.schedules.audit_retention,
        maintenance_retry(),
        move |_: AuditRetention, _: JobContext| {
            let (pool, audit, clock) = (pool.clone(), audit.clone(), Arc::clone(&clock));
            async move {
                audit_retention(&pool, &audit, clock.now())
                    .await
                    .map_err(retry)
            }
        },
    )?;

    Ok(())
}
