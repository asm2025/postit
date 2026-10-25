//! The error the media service reports.

use postit_data::{DataError, ScopeError};

/// Why a media operation failed.
#[derive(Debug, thiserror::Error)]
pub enum MediaServiceError {
    /// The asset or upload session does not exist for this owner.
    #[error("media not found")]
    NotFound,
    /// The caller's access does not allow the action.
    #[error(transparent)]
    Scope(ScopeError),
    /// The file is not an accepted image or video.
    #[error("unsupported media type")]
    UnsupportedMediaType,
    /// The file is larger than its kind allows.
    #[error("file is larger than {max} bytes")]
    PayloadTooLarge {
        /// The limit that applied.
        max: u64,
    },
    /// Storing the file would pass the owner's quota.
    #[error("storage quota of {quota} bytes exceeded ({used} used)")]
    StorageQuotaExceeded {
        /// The owner's quota in bytes.
        quota: u64,
        /// The bytes already stored or reserved.
        used: u64,
    },
    /// A chunk does not start where the stored bytes end, or the upload is not complete.
    #[error("upload offset is {offset}")]
    UploadOffsetMismatch {
        /// Where the stored bytes end.
        offset: u64,
    },
    /// A post or a job that has not ended still uses the asset.
    #[error("media is in use")]
    InUse,
    /// A request value is not acceptable.
    #[error("invalid value for {field}")]
    Invalid {
        /// The offending field.
        field: &'static str,
    },
    /// A database failure.
    #[error(transparent)]
    Data(DataError),
    /// A file-system failure.
    #[error("media I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl From<DataError> for MediaServiceError {
    fn from(err: DataError) -> Self {
        match err {
            DataError::NotFound => Self::NotFound,
            DataError::Scope(scope) => Self::Scope(scope),
            DataError::StorageQuotaExceeded { quota, used } => Self::StorageQuotaExceeded {
                quota: u64::try_from(quota).unwrap_or(0),
                used: u64::try_from(used).unwrap_or(0),
            },
            DataError::Conflict(reason) if reason == "media_in_use" => Self::InUse,
            other => Self::Data(other),
        }
    }
}

impl From<sqlx::Error> for MediaServiceError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::Sql(err))
    }
}
