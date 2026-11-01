//! The contacts jobs: payloads, registration, and console specs. Payloads hold IDs only.

use std::sync::Arc;
use std::time::Duration;

use postit_core::EmailEventKind;
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, Job, JobContext, JobRegistry, JobsError, Queue, RetryPolicy,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ContactService;

/// Applies the staged rows of one contact import, then completes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactImportJob {
    /// The import.
    pub import: Uuid,
    /// The owner of the import.
    pub owner: Uuid,
}

impl Job for ContactImportJob {
    const JOB_TYPE: &'static str = "contact_import";
    const QUEUE: Queue = Queue::Contacts;
}

/// Records what happened to one sent email and suppresses the address on a bounce or a
/// complaint. Carries IDs and the kind only, never an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailEventApply {
    /// The owner of the send.
    pub owner: Uuid,
    /// The `direct_sends` row the event is about.
    pub send: Uuid,
    /// What happened.
    pub kind: EmailEventKind,
}

impl Job for EmailEventApply {
    const JOB_TYPE: &'static str = "email_event_apply";
    const QUEUE: Queue = Queue::Contacts;
}

/// How a failed event run is retried. Recording is idempotent for once-only kinds, so a retry
/// is safe.
pub const EVENT_RETRY: RetryPolicy = RetryPolicy::Backoff {
    max_attempts: Some(5),
    initial: Duration::from_secs(10),
    max: Duration::from_mins(5),
};

/// How a failed import run is retried. Each chunk commits on its own, so a retry resumes at
/// the first row not yet applied. An import whose retries run out stays `processing` until the
/// stale sweep fails it.
pub const IMPORT_RETRY: RetryPolicy = RetryPolicy::Backoff {
    max_attempts: Some(5),
    initial: Duration::from_secs(30),
    max: Duration::from_mins(10),
};

/// Registers `contact_import`, which runs [`ContactService::run_import`], and
/// `email_event_apply`, which runs [`ContactService::apply_event`].
///
/// # Errors
///
/// [`JobsError::DuplicateJobType`] when the type is already registered.
pub fn register(registry: &mut JobRegistry, service: Arc<ContactService>) -> Result<(), JobsError> {
    let events = Arc::clone(&service);
    registry.register(IMPORT_RETRY, move |job: ContactImportJob, _: JobContext| {
        let service = Arc::clone(&service);
        async move { service.run_import(job).await }
    })?;
    registry.register(EVENT_RETRY, move |job: EmailEventApply, _: JobContext| {
        let service = Arc::clone(&events);
        async move { service.apply_event(job).await }
    })
}

/// Describes the contacts jobs to the admin console. A retry is safe: an import applies each
/// staged row once, a completed import ignores a second run, and an event is recorded once.
pub fn console(catalog: &mut ConsoleCatalog) {
    catalog.add::<ContactImportJob>(
        ConsoleSpec::new()
            .id_field("import", "import_id")
            .id_field("owner", "owner_id"),
    );
    catalog.add::<EmailEventApply>(
        ConsoleSpec::new()
            .id_field("send", "send_id")
            .id_field("owner", "owner_id"),
    );
}
