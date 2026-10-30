//! Notifications: who a notice reaches, and the rows that record it.
//!
//! [`Notifier::record`] runs in the caller's transaction, so a notice exists exactly when the
//! change it reports does. This crate depends on `core`, `data`, `jobs`, and `mail` only;
//! `identity`, `oauth`, and `publishing` call it, never the reverse.

mod digest;
mod email;
mod error;
mod record;

pub use digest::{CommentDigestLoader, DIGEST_HOUR, NotificationDigest, enqueue_due_digests};
use std::sync::Arc;
use std::time::Duration;

use postit_config::JobSchedules;
use postit_core::Clock;
use postit_jobs::{ConsoleCatalog, ConsoleSpec, JobContext, JobError, JobRegistry, RetryPolicy};
use postit_mail::MailOutbox;
use sqlx::PgPool;

pub use email::{NotificationAlertLoader, register_loaders};
pub use error::NotifyError;
#[cfg(feature = "testkit")]
pub use record::notifications_for;
pub use record::{ALERT_DELAY, Notifier, NotifierDeps, NotifyEvent, Recipients};

/// Describes the notification job types to the admin console.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    catalog.add::<NotificationDigest>(ConsoleSpec::new().recurring(&schedules.notification_digest));
}

/// Registers the `notification_digest` recurring job. The mail loaders are registered
/// separately by [`register_loaders`].
///
/// # Errors
///
/// [`NotifyError::Jobs`] if the job type is already registered or the schedule is invalid.
pub fn register(
    registry: &mut JobRegistry,
    pool: PgPool,
    outbox: MailOutbox,
    clock: Arc<dyn Clock>,
    schedules: &JobSchedules,
) -> Result<(), NotifyError> {
    registry.register_recurring(
        &schedules.notification_digest,
        RetryPolicy::Backoff {
            max_attempts: Some(3),
            initial: Duration::from_secs(60),
            max: Duration::from_secs(600),
        },
        move |_: NotificationDigest, _: JobContext| {
            let (pool, outbox, clock) = (pool.clone(), outbox.clone(), Arc::clone(&clock));
            async move {
                enqueue_due_digests(&pool, &outbox, clock.now())
                    .await
                    .map(|_| ())
                    .map_err(|e| JobError::Retry(e.to_string()))
            }
        },
    )?;
    Ok(())
}
