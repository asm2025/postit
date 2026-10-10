/// Errors from `postit-data`. Never carries a secret or a raw credential — `sqlx::Error`'s
/// `Display` can include a query string, but this crate never binds a secret value into a
/// query, so that's safe to surface.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    /// A database query failed.
    #[error("database error: {0}")]
    Sql(#[from] sqlx::Error),

    /// Applying migrations failed.
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    /// No row matched.
    #[error("row not found")]
    NotFound,

    /// A uniqueness or state rule rejected the change; the message says which.
    #[error("conflict: {0}")]
    Conflict(String),
}
