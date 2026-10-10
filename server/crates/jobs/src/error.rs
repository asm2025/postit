use postit_data::DataError;

/// What a handler returns on failure. Messages must not contain user content or secrets. For
/// a job type that opted in (`ConsoleSpec::show_error_message`), the message is redacted
/// (URL credentials, token query parameters, bearer tokens, secret-looking `key=value` pairs)
/// before it is stored, and shown in the admin console; for any other type it is never stored.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// The handler failed but the job should be retried.
    #[error("retryable: {0}")]
    Retry(String),
    /// The handler failed and the job must not be retried.
    #[error("fatal: {0}")]
    Fatal(String),
}

/// Why enqueuing, registering, or scheduling a job failed.
#[derive(Debug, thiserror::Error)]
pub enum JobsError {
    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] DataError),

    /// The job payload could not be serialised.
    #[error("job payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),

    /// Two registrations share a job type name.
    #[error("job type `{0}` is registered twice")]
    DuplicateJobType(&'static str),

    /// A recurring job's cron expression is invalid.
    #[error("recurring job `{name}` has an invalid cron schedule: {reason}")]
    InvalidSchedule {
        /// Job type name of the recurring job.
        name: &'static str,
        /// Why the expression was rejected.
        reason: String,
    },

    /// An error from the job storage backend, rendered to text so no backend type leaks.
    #[error("job backend error: {0}")]
    Backend(String),
}

impl From<sqlx::Error> for JobsError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::from(err))
    }
}
