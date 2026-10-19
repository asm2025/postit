//! Envelope-sealed secret values for postit.
//!
//! Every secret postit stores (account tokens, API keys, client secrets) is sealed by a
//! [`Vault`] before it reaches the database and opened only where the value is needed.
//!
//! # Envelope
//!
//! [`Vault::seal`] encodes the value as JSON, generates a random 32-byte data key (DEK), and
//! encrypts the JSON under the DEK with XChaCha20-Poly1305 and a random 24-byte nonce. The DEK
//! is then wrapped (also XChaCha20-Poly1305, with its own random nonce) under the
//! key-encryption key (KEK) of the newest configured version. Rotation ([`Vault::rewrap`]) only
//! re-wraps the DEK; the payload ciphertext is left as it is.
//!
//! Each KEK is derived from the configured key material (at least 32 bytes) with HKDF-SHA256,
//! no salt, and the info string `postit vault kek v{version}`.
//!
//! # Associated data
//!
//! Both encryptions authenticate the same associated data, built from a [`SealContext`]: the
//! concatenation of `postit.vault.v1`, the table name, the column name, and the 16 raw bytes of
//! the row ID, each preceded by its length as a big-endian `u32`. The associated data is never
//! stored, so a value copied to another row, column, or table fails to open.
//!
//! # Storage format
//!
//! A sealed value is one `bytea`, laid out as:
//!
//! ```text
//! offset  size  field
//!      0     1  format version (FORMAT_VERSION = 1)
//!      1     4  KEK version (u32, big-endian, never 0)
//!      5    24  wrap nonce
//!     29    48  wrapped DEK (32 bytes + 16-byte tag)
//!     77    24  data nonce
//!    101   n+16 ciphertext of the JSON plaintext + 16-byte tag
//! ```
//!
//! The header is 101 bytes, so the shortest valid envelope is 117 bytes.
//!
//! # Leaks
//!
//! No key material, plaintext, or sealed bytes appear in any `Debug` or `Display` output or in
//! any [`VaultError`]. [`Sealed`] has no `Display` and no `Serialize`.

mod envelope;
mod error;
mod key;
mod sealed;

pub use envelope::FORMAT_VERSION;
pub use error::VaultError;
pub use key::Vault;
pub use sealed::{SealContext, Sealed};
