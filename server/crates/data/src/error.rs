/// Errors from `postit-data`. Never carries a secret or a raw credential — `sqlx::Error`'s
/// `Display` can include a query string, but this crate never binds a secret value into a
/// query, so that's safe to surface.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("database error: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("row not found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),
}
