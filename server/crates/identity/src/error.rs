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

    #[error("the last active admin cannot be disabled or demoted")]
    LastAdmin,
}
