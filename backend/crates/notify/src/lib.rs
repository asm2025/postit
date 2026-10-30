//! Notifications: who a notice reaches, and the rows that record it.
//!
//! [`Notifier::record`] runs in the caller's transaction, so a notice exists exactly when the
//! change it reports does. This crate depends on `config`, `core`, `data`, `http`, `i18n`,
//! `jobs`, `mail`, and `vault` only; `identity`, `oauth`, and `publishing` call it, never the
//! reverse. Web Push ([`PushSender`], [`PushEndpoints`]) sends through `postit-http`'s public
//! client, so the outbound address policy applies to every endpoint.

mod digest;
pub mod ece;
mod email;
mod error;
mod push;
mod record;
pub mod vapid;

use std::sync::Arc;
use std::time::Duration;

use postit_config::JobSchedules;
use postit_core::Clock;
use postit_jobs::{ConsoleCatalog, ConsoleSpec, JobContext, JobError, JobRegistry, RetryPolicy};
use postit_mail::MailOutbox;
use sqlx::PgPool;

pub use digest::{CommentDigestLoader, DIGEST_HOUR, NotificationDigest, enqueue_due_digests};
pub use email::{NotificationAlertLoader, register_loaders};
pub use error::NotifyError;
pub use push::{
    MAX_DEVICE_LABEL, MAX_ENDPOINT_LEN, NewDevice, PUSH_TTL_SECS, PushEndpointError, PushEndpoints,
    PushEndpointsDeps, PushSend, PushSender, PushSenderDeps, push_retry_policy, push_tag,
    push_topic,
};
#[cfg(feature = "testkit")]
pub use record::notifications_for;
pub use record::{ALERT_DELAY, Notifier, NotifierDeps, NotifyEvent, Recipients};

/// Describes the notification job types to the admin console: the digest's schedule, and the
/// notification and device IDs of `push_send`.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    catalog.add::<NotificationDigest>(ConsoleSpec::new().recurring(&schedules.notification_digest));
    catalog.add::<PushSend>(
        ConsoleSpec::new()
            .id_field("notification", "notification_id")
            .id_field("endpoint", "endpoint_id"),
    );
}

/// Registers the `notification_digest` recurring job and `push_send`. The mail loaders are
/// registered separately by [`register_loaders`].
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
    push: PushSender,
) -> Result<(), NotifyError> {
    registry.register(
        push_retry_policy(),
        move |job: PushSend, ctx: JobContext| {
            let push = push.clone();
            async move { push.handle(job, ctx).await }
        },
    )?;
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
