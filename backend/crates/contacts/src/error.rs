//! The error the contact service reports.

use postit_data::{Capability, DataError, ScopeError};

/// Why a contact operation failed. No variant carries an address or a name.
#[derive(Debug, thiserror::Error)]
pub enum ContactError {
    /// A new contact would pass the owner's contacts cap.
    #[error("the contacts limit is reached")]
    Limit,
    /// The text is not a usable email address.
    #[error("invalid email address")]
    InvalidEmail,
    /// A contact needs a note of where its consent came from.
    #[error("a consent source is required")]
    ConsentRequired,
    /// The address is on the suppression list, so the contact cannot be subscribed.
    #[error("the address is suppressed")]
    Suppressed,
    /// The suppression entry was not a bounce, which is the only kind that can be removed.
    #[error("the suppression cannot be removed")]
    NotRemovable,
    /// The list name is empty.
    #[error("the list name is empty")]
    InvalidName,
    /// The contact, list, or suppression does not exist for this owner.
    #[error("not found")]
    NotFound,
    /// The caller's access is below what the action needs.
    #[error("access to this action is not allowed")]
    Forbidden(Capability),
    /// A database failure.
    #[error(transparent)]
    Data(DataError),
}

impl From<DataError> for ContactError {
    fn from(err: DataError) -> Self {
        match err {
            DataError::NotFound => Self::NotFound,
            DataError::Scope(ScopeError::OwnerOnly) => Self::Forbidden(Capability::OwnerOnly),
            DataError::Scope(ScopeError::InsufficientAccess) => Self::Forbidden(Capability::Edit),
            DataError::Conflict(ref code) if code == "suppressed" => Self::Suppressed,
            DataError::Conflict(ref code) if code == "suppression_not_removable" => {
                Self::NotRemovable
            }
            other => Self::Data(other),
        }
    }
}

impl From<sqlx::Error> for ContactError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::Sql(err))
    }
}
