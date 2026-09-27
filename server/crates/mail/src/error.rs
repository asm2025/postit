use postit_data::DataError;
use postit_jobs::JobsError;

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// Connection failure or SMTP 4xx: retry.
    #[error("transient mail failure: {0}")]
    Transient(String),
    /// SMTP 5xx or an unusable recipient: do not retry.
    #[error("permanent mail failure: {0}")]
    Permanent(String),
    #[error("template rendering failed: {0}")]
    Template(String),
    #[error("mail configuration: {0}")]
    Config(String),
    #[error(transparent)]
    Data(#[from] DataError),
    #[error(transparent)]
    Jobs(#[from] JobsError),
    #[error(transparent)]
    Audit(#[from] postit_data::audit::AuditError),
    #[error("no context loader is registered for mail kind `{}`", .0.as_str())]
    NoLoader(crate::outbox::MailKind),
    #[error("a context loader for mail kind `{}` is already registered", .0.as_str())]
    DuplicateLoader(crate::outbox::MailKind),
}
