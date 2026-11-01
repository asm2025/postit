//! The error the contact service reports.

use postit_data::{Capability, DataError, ScopeError};
use postit_jobs::JobsError;

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
    /// The status cannot be set by hand: the target is `bounced` or `complained`, or the
    /// contact's address bounced or complained.
    #[error("the status cannot be set")]
    StatusNotSettable,
    /// The suppression entry was not a bounce, which is the only kind that can be removed.
    #[error("the suppression cannot be removed")]
    NotRemovable,
    /// The list name is empty or holds a NUL character.
    #[error("the list name is invalid")]
    InvalidName,
    /// A text value holds a NUL character, which the database cannot store. `field` names the
    /// input (`first_name`, `last_name`, `fields`, `tags`, `locale`, `consent_source`, the
    /// `tag` filter, or an import's `columns`); the value itself is never carried.
    #[error("the {field} value holds a NUL character")]
    InvalidText {
        /// The input the value came in.
        field: &'static str,
    },
    /// The contact, list, or suppression does not exist for this owner.
    #[error("not found")]
    NotFound,
    /// The caller's access is below what the action needs.
    #[error("access to this action is not allowed")]
    Forbidden(Capability),
    /// An import was started without attesting that the owner has consent for the addresses.
    #[error("consent for the imported addresses must be attested")]
    AttestationRequired,
    /// The CSV file cannot be read: a mapped column is missing, or the file is not UTF-8.
    #[error("the CSV file is invalid")]
    CsvInvalid,
    /// The CSV file is over `contacts.csv_max_bytes` or `contacts.csv_max_rows`.
    #[error("the CSV file is too large")]
    CsvTooLarge,
    /// The upload sat idle until its body timed out (an [`std::io::ErrorKind::TimedOut`] read).
    #[error("the upload timed out")]
    BodyTimedOut,
    /// A database failure.
    #[error(transparent)]
    Data(DataError),
    /// The import job could not be queued.
    #[error("the import job could not be queued")]
    Queue(#[source] JobsError),
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
            DataError::Conflict(ref code) if code == "status_not_settable" => {
                Self::StatusNotSettable
            }
            other => Self::Data(other),
        }
    }
}

impl From<JobsError> for ContactError {
    fn from(err: JobsError) -> Self {
        match err {
            JobsError::Data(err) => Self::from(err),
            other => Self::Queue(other),
        }
    }
}

impl From<sqlx::Error> for ContactError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::Sql(err))
    }
}
