//! The account service's error. No variant carries a credential value.

use std::time::Duration;

use postit_core::{AgentError, Violation};
use postit_data::{DataError, ScopeError};
use postit_vault::VaultError;

/// Why an account operation failed.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// No agent serves this platform ID in this build, or the platform has no row.
    #[error("platform not found")]
    PlatformNotFound,
    /// An admin switched the platform off.
    #[error("platform disabled")]
    PlatformDisabled,
    /// The platform needs deployment configuration the deployment lacks, or its API failed.
    #[error("platform unavailable")]
    PlatformUnavailable,
    /// The platform connects another way (OAuth), not with pasted credentials.
    #[error("connect method not supported")]
    ConnectMethodUnsupported,
    /// The pasted values do not fit the credentials schema. Names fields and codes only.
    #[error("validation failed with {} violation(s)", .0.len())]
    Validation(Vec<Violation>),
    /// The platform rejected the credentials, or they lack permission.
    #[error("credentials invalid")]
    CredentialsInvalid {
        /// The credential field at fault, when known.
        field: Option<String>,
        /// Scopes the credentials lack, when the platform says.
        missing_scopes: Vec<String>,
    },
    /// The platform asked the caller to slow down.
    #[error("rate limited")]
    RateLimited {
        /// How long to wait, when the platform said.
        retry_after: Option<Duration>,
    },
    /// No such row, or it is outside the caller's account limit.
    #[error("not found")]
    NotFound,
    /// The account is disconnected.
    #[error("the account is disconnected")]
    AccountDisconnected,
    /// A delivery of the account is being published right now.
    #[error("a post is being published to this account")]
    JobActive,
    /// The owner already has an email sending method.
    #[error("the owner already has an email sending method")]
    SendingMethodExists,
    /// `return_to` is not one of the deployment's allowed return URLs.
    #[error("return_to is not allowed")]
    ReturnToInvalid,
    /// The agent could not verify a platform event (a missing or wrong signature).
    #[error("the platform event could not be verified")]
    EventRejected,
    /// The agent verified a platform event but could not read it: the sender's fault, so a
    /// retry of the same event cannot succeed.
    #[error("the platform event could not be read")]
    EventMalformed,
    /// The CSV is malformed. Names a column or line, never a value.
    #[error("invalid CSV: {0}")]
    CsvInvalid(String),
    /// The caller's access does not allow the action.
    #[error(transparent)]
    Scope(#[from] ScopeError),
    /// A repository failed.
    #[error(transparent)]
    Data(DataError),
    /// The vault could not seal or open a credential.
    #[error(transparent)]
    Vault(#[from] VaultError),
}

impl From<DataError> for AccountError {
    fn from(error: DataError) -> Self {
        match error {
            // The account, credential, and pending-connection repositories never raise
            // `AccountNotInScope` (that is the whole-object rule of posts, jobs, and
            // recurrences); if one ever did, hiding the row is the safe answer here.
            DataError::NotFound | DataError::AccountNotInScope => Self::NotFound,
            DataError::Scope(scope) => Self::Scope(scope),
            DataError::SendingMethodExists => Self::SendingMethodExists,
            other => Self::Data(other),
        }
    }
}

impl From<postit_notify::NotifyError> for AccountError {
    fn from(error: postit_notify::NotifyError) -> Self {
        match error {
            postit_notify::NotifyError::Data(data) => data.into(),
            // Recording a notice only writes a row; it enqueues nothing.
            other => {
                tracing::error!(%other, "recording a notification failed");
                Self::Data(DataError::Conflict("notification".to_owned()))
            }
        }
    }
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        Self::Data(DataError::Sql(error))
    }
}

impl AccountError {
    /// Maps an agent's error while connecting or checking. Only the variant name is logged.
    #[must_use]
    pub fn from_agent(error: AgentError) -> Self {
        match error {
            AgentError::InvalidCredentials { field } => Self::CredentialsInvalid {
                field,
                missing_scopes: Vec::new(),
            },
            AgentError::Authentication { .. } => Self::CredentialsInvalid {
                field: None,
                missing_scopes: Vec::new(),
            },
            AgentError::Authorization { missing_scopes } => Self::CredentialsInvalid {
                field: None,
                missing_scopes,
            },
            AgentError::RateLimited { retry_after } => Self::RateLimited { retry_after },
            other => {
                tracing::warn!(error = agent_error_name(&other), "platform call failed");
                Self::PlatformUnavailable
            }
        }
    }
}

pub(crate) fn agent_error_name(error: &AgentError) -> &'static str {
    error.code()
}
