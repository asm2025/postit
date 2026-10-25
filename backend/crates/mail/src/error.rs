use postit_data::DataError;
use postit_jobs::JobsError;

/// Failures sending or preparing an email.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// Connection failure or SMTP 4xx: retry.
    #[error("transient mail failure: {0}")]
    Transient(String),
    /// SMTP 5xx or an unusable recipient: do not retry.
    #[error("permanent mail failure: {0}")]
    Permanent(String),
    /// The email template could not be rendered.
    #[error("template rendering failed: {0}")]
    Template(String),
    /// The mail settings are unusable, for example an invalid sender address.
    #[error("mail configuration: {0}")]
    Config(String),
    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// Enqueuing or running the job failed.
    #[error(transparent)]
    Jobs(#[from] JobsError),
    /// Writing the audit event failed.
    #[error(transparent)]
    Audit(#[from] postit_data::audit::AuditError),
    /// No context loader is registered for the mail kind.
    #[error("no context loader is registered for mail kind `{}`", .0.as_str())]
    NoLoader(crate::outbox::MailKind),
    /// A context loader for the mail kind is already registered.
    #[error("a context loader for mail kind `{}` is already registered", .0.as_str())]
    DuplicateLoader(crate::outbox::MailKind),
}
