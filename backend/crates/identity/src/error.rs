/// Why a bearer token was rejected.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The signature does not verify.
    #[error("bad signature")]
    BadSignature,
    /// The `iss` claim is not the configured issuer.
    #[error("wrong issuer")]
    WrongIssuer,
    /// The `aud` claim names none of the accepted audiences.
    #[error("wrong audience")]
    WrongAudience,
    /// The token is past `exp`.
    #[error("token expired")]
    Expired,
    /// The token is before its `nbf` time.
    #[error("token not yet valid")]
    NotYetValid,
    /// The header names an algorithm that is not accepted.
    #[error("algorithm not accepted")]
    UnacceptedAlgorithm,
    /// No published key has the token's `kid`.
    #[error("unknown key id")]
    UnknownKid,
    /// The token was issued to a client that is not allowed.
    #[error("client not allowed")]
    WrongClient,
    /// The token is not a well-formed JWT.
    #[error("malformed token")]
    Malformed,
}

/// Failures in identity resolution and administration.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// Token verification failed.
    #[error(transparent)]
    Verify(#[from] VerifyError),

    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] postit_data::DataError),

    /// An outbound HTTP request failed.
    #[error("http error: {0}")]
    Http(#[from] postit_http::HttpError),

    /// An audit event was malformed.
    #[error(transparent)]
    Audit(#[from] postit_data::audit::AuditError),

    /// The user's status cannot move between these two states.
    #[error("status transition not allowed: {0} -> {1}")]
    InvalidTransition(&'static str, &'static str),

    /// The change would leave no active admin.
    #[error("the last active admin cannot be disabled, demoted, or deleted")]
    LastAdmin,

    /// The user is being deleted.
    #[error("the user is being deleted")]
    UserDeleting,

    /// An admin tried to delete their own account.
    #[error("an admin cannot delete their own account through user administration")]
    CannotDeleteSelf,

    /// Enqueuing or sending mail failed.
    #[error(transparent)]
    Mail(#[from] postit_mail::MailError),

    /// Enqueuing a job failed.
    #[error(transparent)]
    Jobs(#[from] postit_jobs::JobsError),

    /// A notification could not be recorded.
    #[error(transparent)]
    Notify(#[from] postit_notify::NotifyError),

    /// A delegation refused its input (a grant that already exists, or an invalid field)
    /// where no caller input was given, such as in the expiry job.
    #[error(transparent)]
    Delegation(crate::delegation::DelegationError),

    /// Production has neither an active admin nor a bootstrap rule.
    #[error(
        "production has no active admin and no bootstrap rule: set auth.bootstrap.admin_email (POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL) or auth.bootstrap.admin_subject"
    )]
    BootstrapRequired,
}
