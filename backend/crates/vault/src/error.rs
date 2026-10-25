use thiserror::Error;

/// Why a vault could not be built or a value could not be sealed or opened.
///
/// Messages carry key versions only, never key material, plaintext, or sealed bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VaultError {
    /// No key is configured.
    #[error("the vault has no keys; configure at least one vault.keys.<version>")]
    NoKeys,
    /// A key's material is shorter than 32 bytes.
    #[error("vault key version {version} is shorter than 32 bytes")]
    KeyTooShort {
        /// The offending key version.
        version: u32,
    },
    /// A key version is `0` or not a canonical positive integer.
    #[error("a vault key version is not a positive integer")]
    InvalidVersion,
    /// Two keys share a version.
    #[error("vault key version {version} is configured more than once")]
    DuplicateVersion {
        /// The repeated key version.
        version: u32,
    },
    /// The value was sealed under a key version this vault does not hold.
    #[error("the value is sealed under vault key version {version}, which is not configured")]
    UnknownKeyVersion {
        /// The key version recorded in the envelope.
        version: u32,
    },
    /// The bytes are not a sealed value in a known format.
    #[error("the sealed value is malformed")]
    Malformed,
    /// Decryption failed: wrong key, wrong row, column, or table, or tampered bytes.
    #[error("the sealed value failed authentication")]
    AuthenticationFailed,
    /// The value could not be encoded to, or decoded from, its JSON plaintext.
    #[error("the sealed value could not be encoded or decoded")]
    Serialization,
}
