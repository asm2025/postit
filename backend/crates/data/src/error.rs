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

    /// The post changed since the caller read it; reload and retry.
    #[error("the row changed since it was read")]
    VersionConflict,

    /// A publish job is active for the post, so it cannot be edited.
    #[error("the post is locked by an active publish job")]
    PostLocked,

    /// The post already has a live publish job.
    #[error("the post already has a live publish job")]
    JobActive,

    /// Another owner has this remote account connected.
    #[error("the remote account is connected to another owner")]
    AccountConnectedElsewhere,

    /// The owner already has an email sending method.
    #[error("the owner already has an email sending method")]
    SendingMethodExists,

    /// The row belongs to an account outside the caller's delegation limit.
    #[error("the account is outside the caller's delegation limit")]
    AccountNotInScope,

    /// The caller's access does not allow the action.
    #[error(transparent)]
    Scope(#[from] crate::ScopeError),

    /// The configured vault's newest key version is lower than one already recorded, so the
    /// deployment lost a key.
    #[error("the vault's newest key version {configured} is behind the stored version {stored}")]
    VaultKeyBehind {
        /// Newest version in the configured vault.
        configured: u32,
        /// Newest version ever recorded in `vault_state`.
        stored: u32,
    },

    /// Stored sealed values use a key version the configured vault does not hold.
    #[error("sealed values use vault key version {version}, which is not configured")]
    VaultKeyMissing {
        /// The missing key version.
        version: u32,
    },

    /// Another re-seal run holds the rotation lock.
    #[error("a vault re-seal run is already in progress")]
    RotationInProgress,

    /// Sealing or opening a value failed.
    #[error("vault error: {0}")]
    Vault(#[from] postit_vault::VaultError),
}
