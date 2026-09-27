use postit_data::DataError;

/// What a handler returns on failure. Messages must not contain user content or secrets:
/// they are stored with the job and (from P8) shown, redacted, in the admin console.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("retryable: {0}")]
    Retry(String),
    #[error("fatal: {0}")]
    Fatal(String),
}

#[derive(Debug, thiserror::Error)]
pub enum JobsError {
    #[error(transparent)]
    Data(#[from] DataError),

    #[error("job payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("job type `{0}` is registered twice")]
    DuplicateJobType(&'static str),

    #[error("recurring job `{name}` has an invalid cron schedule: {reason}")]
    InvalidSchedule { name: &'static str, reason: String },

    /// An error from the job storage backend, rendered to text so no backend type leaks.
    #[error("job backend error: {0}")]
    Backend(String),
}

impl From<sqlx::Error> for JobsError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::from(err))
    }
}
