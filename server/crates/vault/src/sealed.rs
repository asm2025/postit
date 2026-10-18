use std::fmt;
use std::marker::PhantomData;

use uuid::Uuid;

use crate::VaultError;
use crate::envelope::{
    CIPHERTEXT_AT, DATA_NONCE_AT, FORMAT_VERSION, MIN_LEN, VERSION_AT, WRAP_NONCE_AT,
    WRAPPED_DEK_AT,
};

/// Where a sealed value lives. It is authenticated as associated data and never stored, so a
/// value only opens in the table, column, and row it was sealed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealContext {
    /// Table name.
    pub table: &'static str,
    /// Column name.
    pub column: &'static str,
    /// ID of the row holding the value.
    pub row: Uuid,
}

/// An envelope-sealed `T`, as stored in a `bytea` column.
///
/// Its `Debug` shows the key version and length only. It has no `Display` and no
/// `Serialize`, so it cannot reach an API response by accident.
pub struct Sealed<T> {
    bytes: Vec<u8>,
    _value: PhantomData<fn() -> T>,
}

impl<T> Sealed<T> {
    /// Wraps stored bytes, checking the header.
    ///
    /// # Errors
    ///
    /// [`VaultError::Malformed`] when the bytes are too short, the format version is unknown,
    /// or the key version is `0`.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, VaultError> {
        if bytes.len() < MIN_LEN || bytes[0] != FORMAT_VERSION || read_version(&bytes) == 0 {
            return Err(VaultError::Malformed);
        }
        Ok(Self::from_valid(bytes))
    }

    /// Wraps bytes the envelope code just built.
    pub(crate) fn from_valid(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            _value: PhantomData,
        }
    }

    /// The stored bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The version of the key-encryption key that wraps this value's data key.
    #[must_use]
    pub fn key_version(&self) -> u32 {
        read_version(&self.bytes)
    }

    pub(crate) fn wrap_nonce(&self) -> &[u8] {
        &self.bytes[WRAP_NONCE_AT..WRAPPED_DEK_AT]
    }

    pub(crate) fn wrapped_dek(&self) -> &[u8] {
        &self.bytes[WRAPPED_DEK_AT..DATA_NONCE_AT]
    }

    pub(crate) fn data_nonce(&self) -> &[u8] {
        &self.bytes[DATA_NONCE_AT..CIPHERTEXT_AT]
    }

    pub(crate) fn ciphertext(&self) -> &[u8] {
        &self.bytes[CIPHERTEXT_AT..]
    }

    /// The data nonce and ciphertext, which rotation keeps as they are.
    pub(crate) fn payload(&self) -> &[u8] {
        &self.bytes[DATA_NONCE_AT..]
    }
}

/// The big-endian key version at offset 1; `0` when the bytes are too short.
fn read_version(bytes: &[u8]) -> u32 {
    bytes
        .get(VERSION_AT..WRAP_NONCE_AT)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_be_bytes)
}

impl<T> Clone for Sealed<T> {
    fn clone(&self) -> Self {
        Self::from_valid(self.bytes.clone())
    }
}

impl<T> PartialEq for Sealed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<T> Eq for Sealed<T> {}

impl<T> fmt::Debug for Sealed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Sealed(kek v{}, {} bytes)",
            self.key_version(),
            self.bytes.len()
        )
    }
}

#[cfg(feature = "sqlx")]
mod sqlx_impls {
    use sqlx::encode::IsNull;
    use sqlx::error::BoxDynError;
    use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};
    use sqlx::{Decode, Encode, Postgres, Type};

    use super::Sealed;

    impl<T> Type<Postgres> for Sealed<T> {
        fn type_info() -> PgTypeInfo {
            <Vec<u8> as Type<Postgres>>::type_info()
        }

        fn compatible(ty: &PgTypeInfo) -> bool {
            <Vec<u8> as Type<Postgres>>::compatible(ty)
        }
    }

    impl<T> Encode<'_, Postgres> for Sealed<T> {
        fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
            <&[u8] as Encode<Postgres>>::encode(self.as_bytes(), buf)
        }
    }

    impl<'r, T> Decode<'r, Postgres> for Sealed<T> {
        fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
            let bytes = <Vec<u8> as Decode<Postgres>>::decode(value)?;
            Ok(Sealed::from_bytes(bytes)?)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use uuid::Uuid;

    use crate::{FORMAT_VERSION, SealContext, Sealed, Vault, VaultError};

    fn sealed() -> Sealed<String> {
        let vault =
            Vault::from_keys([(1, b"test-only-vault-key-material-abcdefghijk".to_vec())]).unwrap();
        let ctx = SealContext {
            table: "t",
            column: "c",
            row: Uuid::from_u128(42),
        };
        vault.seal(&"value".to_owned(), &ctx).unwrap()
    }

    #[test]
    fn truncated_envelope_is_malformed() {
        let bytes = sealed().as_bytes().to_vec();
        // Anything shorter than the header plus one tag is rejected up front.
        for len in [0, 1, 5, 100, 101, 116] {
            assert_eq!(
                Sealed::<String>::from_bytes(bytes[..len.min(bytes.len())].to_vec()).unwrap_err(),
                VaultError::Malformed,
                "length {len}"
            );
        }
        // A cut inside the ciphertext still parses but fails to authenticate.
        let cut = Sealed::<String>::from_bytes(bytes[..bytes.len() - 1].to_vec()).unwrap();
        let vault =
            Vault::from_keys([(1, b"test-only-vault-key-material-abcdefghijk".to_vec())]).unwrap();
        let ctx = SealContext {
            table: "t",
            column: "c",
            row: Uuid::from_u128(42),
        };
        assert_eq!(
            vault.open(&cut, &ctx).unwrap_err(),
            VaultError::AuthenticationFailed
        );
    }

    #[test]
    fn from_bytes_checks_format_and_version() {
        let mut bytes = sealed().as_bytes().to_vec();
        assert!(Sealed::<String>::from_bytes(bytes.clone()).is_ok());

        bytes[0] = FORMAT_VERSION + 1;
        assert_eq!(
            Sealed::<String>::from_bytes(bytes.clone()).unwrap_err(),
            VaultError::Malformed
        );

        bytes[0] = FORMAT_VERSION;
        bytes[1..5].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            Sealed::<String>::from_bytes(bytes).unwrap_err(),
            VaultError::Malformed
        );
    }

    #[test]
    fn bytes_round_trip_and_debug_is_opaque() {
        let original = sealed();
        let copy = Sealed::<String>::from_bytes(original.as_bytes().to_vec()).unwrap();
        assert_eq!(copy, original);
        assert_eq!(copy.key_version(), 1);
        assert_eq!(
            format!("{copy:?}"),
            format!("Sealed(kek v1, {} bytes)", original.as_bytes().len())
        );
    }
}
