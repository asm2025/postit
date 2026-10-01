#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("bad signature")]
    BadSignature,
    #[error("wrong issuer")]
    WrongIssuer,
    #[error("wrong audience")]
    WrongAudience,
    #[error("token expired")]
    Expired,
    #[error("token not yet valid")]
    NotYetValid,
    #[error("algorithm not accepted")]
    UnacceptedAlgorithm,
    #[error("unknown key id")]
    UnknownKid,
    #[error("malformed token")]
    Malformed,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error(transparent)]
    Verify(#[from] VerifyError),

    #[error(transparent)]
    Data(#[from] postit_data::DataError),

    #[error("http error: {0}")]
    Http(#[from] postit_http::HttpError),

    #[error(transparent)]
    Audit(#[from] postit_data::audit::AuditError),

    #[error("status transition not allowed: {0} -> {1}")]
    InvalidTransition(&'static str, &'static str),

    #[error("the last active admin cannot be disabled, demoted, or deleted")]
    LastAdmin,

    #[error("the user is being deleted")]
    UserDeleting,

    #[error("an admin cannot delete their own account through user administration")]
    CannotDeleteSelf,

    #[error(transparent)]
    Mail(#[from] postit_mail::MailError),

    #[error(transparent)]
    Jobs(#[from] postit_jobs::JobsError),

    #[error(
        "production has no active admin and no bootstrap rule: set auth.bootstrap.admin_email (POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL) or auth.bootstrap.admin_subject"
    )]
    BootstrapRequired,
}
