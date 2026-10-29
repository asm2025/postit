use postit_data::DataError;
use postit_jobs::JobsError;
use postit_mail::MailError;

/// Failures recording or delivering a notification.
#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// Enqueuing a job failed.
    #[error(transparent)]
    Jobs(#[from] JobsError),
    /// Enqueuing an email failed.
    #[error(transparent)]
    Mail(#[from] MailError),
}

impl From<sqlx::Error> for NotifyError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::from(err))
    }
}
