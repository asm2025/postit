use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::Serialize;
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

use crate::{SealContext, Sealed, Vault, VaultError};

/// Version of the storage format, the first byte of every sealed value.
pub const FORMAT_VERSION: u8 = 1;

/// Domain tag leading the associated data.
const DOMAIN: &[u8] = b"postit.vault.v1";

const DEK_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

pub(crate) const VERSION_AT: usize = 1;
pub(crate) const WRAP_NONCE_AT: usize = VERSION_AT + 4;
pub(crate) const WRAPPED_DEK_AT: usize = WRAP_NONCE_AT + NONCE_LEN;
pub(crate) const DATA_NONCE_AT: usize = WRAPPED_DEK_AT + DEK_LEN + TAG_LEN;
pub(crate) const CIPHERTEXT_AT: usize = DATA_NONCE_AT + NONCE_LEN;
/// Header plus the tag of an empty ciphertext.
pub(crate) const MIN_LEN: usize = CIPHERTEXT_AT + TAG_LEN;

const _: () = assert!(CIPHERTEXT_AT == 101, "the header is 101 bytes");

type Dek = Zeroizing<[u8; DEK_LEN]>;

impl Vault {
    /// Seals `value` for the row, column, and table in `ctx`, under the newest key version.
    ///
    /// # Errors
    ///
    /// [`VaultError::Serialization`] when `value` cannot be encoded as JSON.
    pub fn seal<T: Serialize>(
        &self,
        value: &T,
        ctx: &SealContext,
    ) -> Result<Sealed<T>, VaultError> {
        let mut dek: Dek = Zeroizing::new([0u8; DEK_LEN]);
        OsRng.fill_bytes(dek.as_mut());
        let mut wrap_nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut wrap_nonce);
        let mut data_nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut data_nonce);
        self.seal_parts(value, ctx, &dek, &wrap_nonce, &data_nonce)
    }

    /// Opens a value sealed for the row, column, and table in `ctx`.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownKeyVersion`] when the key version is not configured,
    /// [`VaultError::AuthenticationFailed`] for a wrong key or context or tampered bytes, and
    /// [`VaultError::Serialization`] when the plaintext is not a JSON `T`.
    pub fn open<T: DeserializeOwned>(
        &self,
        sealed: &Sealed<T>,
        ctx: &SealContext,
    ) -> Result<T, VaultError> {
        let aad = associated_data(ctx)?;
        let dek = self.unwrap_dek(sealed, &aad)?;
        let plaintext = Zeroizing::new(decrypt(
            dek.as_ref(),
            sealed.data_nonce(),
            sealed.ciphertext(),
            &aad,
        )?);
        serde_json::from_slice(&plaintext).map_err(|_| VaultError::Serialization)
    }

    /// Re-wraps the data key of `sealed` under the newest key version, leaving the data nonce
    /// and ciphertext as they are. Returns `None` when it is already on the newest version.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownKeyVersion`] when the key version is not configured and
    /// [`VaultError::AuthenticationFailed`] for a wrong key or context or tampered bytes.
    pub fn rewrap<T>(
        &self,
        sealed: &Sealed<T>,
        ctx: &SealContext,
    ) -> Result<Option<Sealed<T>>, VaultError> {
        let (newest, kek) = self.newest_kek()?;
        if sealed.key_version() == newest {
            return Ok(None);
        }
        let aad = associated_data(ctx)?;
        let dek = self.unwrap_dek(sealed, &aad)?;
        let mut wrap_nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut wrap_nonce);
        let wrapped = encrypt(kek.as_ref(), &wrap_nonce, dek.as_ref(), &aad)?;

        let payload = sealed.payload();
        let mut bytes = Vec::with_capacity(DATA_NONCE_AT + payload.len());
        push_header(&mut bytes, newest, &wrap_nonce, &wrapped);
        bytes.extend_from_slice(payload);
        Ok(Some(Sealed::from_valid(bytes)))
    }

    /// Seals with a caller-supplied data key and nonces. Only [`Vault::seal`] and the format
    /// test vector call it.
    fn seal_parts<T: Serialize>(
        &self,
        value: &T,
        ctx: &SealContext,
        dek: &Dek,
        wrap_nonce: &[u8; NONCE_LEN],
        data_nonce: &[u8; NONCE_LEN],
    ) -> Result<Sealed<T>, VaultError> {
        let plaintext =
            Zeroizing::new(serde_json::to_vec(value).map_err(|_| VaultError::Serialization)?);
        let aad = associated_data(ctx)?;
        let (version, kek) = self.newest_kek()?;
        let wrapped = encrypt(kek.as_ref(), wrap_nonce, dek.as_ref(), &aad)?;
        let ciphertext = encrypt(dek.as_ref(), data_nonce, &plaintext, &aad)?;

        let mut bytes = Vec::with_capacity(CIPHERTEXT_AT + ciphertext.len());
        push_header(&mut bytes, version, wrap_nonce, &wrapped);
        bytes.extend_from_slice(data_nonce);
        bytes.extend_from_slice(&ciphertext);
        Ok(Sealed::from_valid(bytes))
    }

    /// Test only: [`Vault::seal`] with a fixed data key and nonces.
    #[cfg(test)]
    pub(crate) fn seal_with<T: Serialize>(
        &self,
        value: &T,
        ctx: &SealContext,
        dek: &[u8; DEK_LEN],
        wrap_nonce: &[u8; NONCE_LEN],
        data_nonce: &[u8; NONCE_LEN],
    ) -> Result<Sealed<T>, VaultError> {
        self.seal_parts(value, ctx, &Zeroizing::new(*dek), wrap_nonce, data_nonce)
    }

    /// Decrypts the data key of `sealed` under its recorded key version.
    fn unwrap_dek<T>(&self, sealed: &Sealed<T>, aad: &[u8]) -> Result<Dek, VaultError> {
        let kek = self.kek(sealed.key_version())?;
        let dek = Zeroizing::new(decrypt(
            kek.as_ref(),
            sealed.wrap_nonce(),
            sealed.wrapped_dek(),
            aad,
        )?);
        if dek.len() != DEK_LEN {
            return Err(VaultError::Malformed);
        }
        let mut out: Dek = Zeroizing::new([0u8; DEK_LEN]);
        out.copy_from_slice(&dek);
        Ok(out)
    }
}

/// XChaCha20-Poly1305 encryption of `msg` under `key`, authenticating `aad`.
fn encrypt(
    key: &[u8],
    nonce: &[u8; NONCE_LEN],
    msg: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, VaultError> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(XNonce::from_slice(nonce), Payload { msg, aad })
        // Only fails for plaintexts beyond the cipher's length limit.
        .map_err(|_| VaultError::Serialization)
}

/// XChaCha20-Poly1305 decryption; any failure is an authentication failure. `nonce` is a
/// 24-byte slice of an envelope whose length was checked by [`Sealed::from_bytes`].
fn decrypt(key: &[u8], nonce: &[u8], msg: &[u8], aad: &[u8]) -> Result<Vec<u8>, VaultError> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), Payload { msg, aad })
        .map_err(|_| VaultError::AuthenticationFailed)
}

/// Appends the format byte, key version, wrap nonce, and wrapped data key.
fn push_header(bytes: &mut Vec<u8>, version: u32, wrap_nonce: &[u8], wrapped: &[u8]) {
    bytes.push(FORMAT_VERSION);
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes.extend_from_slice(wrap_nonce);
    bytes.extend_from_slice(wrapped);
}

/// The length-prefixed (`u32` big-endian) concatenation of the domain tag, table, column, and
/// the 16 raw row ID bytes.
fn associated_data(ctx: &SealContext) -> Result<Vec<u8>, VaultError> {
    let parts: [&[u8]; 4] = [
        DOMAIN,
        ctx.table.as_bytes(),
        ctx.column.as_bytes(),
        ctx.row.as_bytes(),
    ];
    let mut aad = Vec::with_capacity(parts.iter().map(|p| 4 + p.len()).sum());
    for part in parts {
        let len = u32::try_from(part.len()).map_err(|_| VaultError::Serialization)?;
        aad.extend_from_slice(&len.to_be_bytes());
        aad.extend_from_slice(part);
    }
    Ok(aad)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    use crate::{SealContext, Sealed, Vault, VaultError};

    const KEY_1: &str = "test-only-vault-key-material-version-1-abcdefgh";
    const KEY_2: &str = "test-only-vault-key-material-version-2-ijklmnop";

    fn material(version: u32) -> Vec<u8> {
        match version {
            1 => KEY_1.as_bytes().to_vec(),
            2 => KEY_2.as_bytes().to_vec(),
            other => format!("test-only-vault-key-material-version-{other}-qrstuvwx").into_bytes(),
        }
    }

    fn vault(versions: &[u32]) -> Vault {
        Vault::from_keys(versions.iter().map(|v| (*v, material(*v)))).unwrap()
    }

    fn ctx() -> SealContext {
        SealContext {
            table: "accounts",
            column: "access_token",
            row: Uuid::from_u128(0x0192_3e4f_5a6b_7c8d_9eaf_b0c1_d2e3_f405),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Credentials {
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
    }

    #[test]
    fn round_trip_string_and_struct() {
        let vault = vault(&[1]);
        let sealed = vault.seal(&"hunter2".to_owned(), &ctx()).unwrap();
        let opened: String = vault.open(&sealed, &ctx()).unwrap();
        assert_eq!(opened, "hunter2");

        let creds = Credentials {
            client_id: "id".into(),
            client_secret: "s3cret".into(),
            scopes: vec!["a".into(), "b".into()],
        };
        let sealed = vault.seal(&creds, &ctx()).unwrap();
        assert_eq!(vault.open(&sealed, &ctx()).unwrap(), creds);
    }

    #[test]
    fn two_seals_of_one_value_differ() {
        let vault = vault(&[1]);
        let a = vault.seal(&"same".to_owned(), &ctx()).unwrap();
        let b = vault.seal(&"same".to_owned(), &ctx()).unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn empty_and_one_mebibyte_plaintexts_round_trip() {
        let vault = vault(&[1]);
        let empty = vault.seal(&String::new(), &ctx()).unwrap();
        assert_eq!(vault.open::<String>(&empty, &ctx()).unwrap(), "");

        let big: Vec<u8> = (0..=250u8).cycle().take(1024 * 1024).collect();
        let sealed = vault.seal(&big, &ctx()).unwrap();
        assert_eq!(vault.open::<Vec<u8>>(&sealed, &ctx()).unwrap(), big);
    }

    #[test]
    fn other_row_column_or_table_fails() {
        let vault = vault(&[1]);
        let sealed = vault.seal(&"token".to_owned(), &ctx()).unwrap();
        let variations = [
            SealContext {
                row: Uuid::from_u128(1),
                ..ctx()
            },
            SealContext {
                column: "refresh_token",
                ..ctx()
            },
            SealContext {
                table: "platforms",
                ..ctx()
            },
            // Moving bytes between the table and column must not produce the same context.
            SealContext {
                table: "accountsaccess_",
                column: "token",
                ..ctx()
            },
        ];
        for other in &variations {
            assert_eq!(
                vault.open::<String>(&sealed, other).unwrap_err(),
                VaultError::AuthenticationFailed,
                "{other:?}"
            );
        }
    }

    #[test]
    fn every_single_byte_flip_fails() {
        let vault = vault(&[1]);
        let sealed = vault.seal(&"x".to_owned(), &ctx()).unwrap();
        let bytes = sealed.as_bytes().to_vec();
        for i in 0..bytes.len() {
            for mask in [0x01u8, 0x80] {
                let mut flipped = bytes.clone();
                flipped[i] ^= mask;
                let result = Sealed::<String>::from_bytes(flipped)
                    .and_then(|s| vault.open::<String>(&s, &ctx()));
                match result {
                    Err(
                        VaultError::Malformed
                        | VaultError::AuthenticationFailed
                        | VaultError::UnknownKeyVersion { .. },
                    ) => {}
                    other => panic!("byte {i} mask {mask:#04x}: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn newest_seals_and_old_opens() {
        let old = vault(&[1]);
        let sealed = old.seal(&"value".to_owned(), &ctx()).unwrap();
        assert_eq!(sealed.key_version(), 1);

        let both = vault(&[2, 1]);
        assert_eq!(both.newest_version(), 2);
        assert_eq!(both.versions(), vec![1, 2]);
        assert_eq!(both.open::<String>(&sealed, &ctx()).unwrap(), "value");

        let fresh = both.seal(&"value".to_owned(), &ctx()).unwrap();
        assert_eq!(fresh.key_version(), 2);
        assert_eq!(both.open::<String>(&fresh, &ctx()).unwrap(), "value");
    }

    #[test]
    fn rewrap_moves_to_newest_and_keeps_plaintext() {
        let sealed = vault(&[1]).seal(&"value".to_owned(), &ctx()).unwrap();
        let both = vault(&[1, 2]);
        let rewrapped = both.rewrap(&sealed, &ctx()).unwrap().unwrap();
        assert_eq!(rewrapped.key_version(), 2);
        assert_eq!(both.open::<String>(&rewrapped, &ctx()).unwrap(), "value");
        // Only the DEK is re-wrapped: data nonce and ciphertext are unchanged.
        assert_eq!(rewrapped.as_bytes()[77..], sealed.as_bytes()[77..]);
        assert_ne!(rewrapped.as_bytes()[..77], sealed.as_bytes()[..77]);
        // The newest-only vault can open it; the old one cannot.
        assert_eq!(
            vault(&[2]).open::<String>(&rewrapped, &ctx()).unwrap(),
            "value"
        );
        assert_eq!(
            vault(&[1]).open::<String>(&rewrapped, &ctx()).unwrap_err(),
            VaultError::UnknownKeyVersion { version: 2 }
        );
        // Already on the newest version: nothing to do.
        assert!(both.rewrap(&rewrapped, &ctx()).unwrap().is_none());
    }

    #[test]
    fn rewrap_checks_the_context() {
        let sealed = vault(&[1]).seal(&"value".to_owned(), &ctx()).unwrap();
        let other = SealContext {
            row: Uuid::from_u128(7),
            ..ctx()
        };
        assert_eq!(
            vault(&[1, 2]).rewrap(&sealed, &other).unwrap_err(),
            VaultError::AuthenticationFailed
        );
    }

    #[test]
    fn unknown_version_fails() {
        let sealed = vault(&[2]).seal(&"value".to_owned(), &ctx()).unwrap();
        assert_eq!(
            vault(&[1]).open::<String>(&sealed, &ctx()).unwrap_err(),
            VaultError::UnknownKeyVersion { version: 2 }
        );
    }

    #[test]
    fn wrong_key_material_fails() {
        let sealed = vault(&[1]).seal(&"value".to_owned(), &ctx()).unwrap();
        let other = Vault::from_keys([(1, material(9))]).unwrap();
        assert_eq!(
            other.open::<String>(&sealed, &ctx()).unwrap_err(),
            VaultError::AuthenticationFailed
        );
    }

    #[test]
    fn wrong_type_is_a_serialization_error() {
        let vault = vault(&[1]);
        let sealed = vault.seal(&"not a number".to_owned(), &ctx()).unwrap();
        let retyped = Sealed::<u64>::from_bytes(sealed.as_bytes().to_vec()).unwrap();
        assert_eq!(
            vault.open(&retyped, &ctx()).unwrap_err(),
            VaultError::Serialization
        );
    }

    /// Pins the storage format. Changing it breaks every value already stored.
    const FORMAT_VECTOR: &str = concat!(
        "01",                                               // format version
        "00000001",                                         // KEK version 1
        "222222222222222222222222222222222222222222222222", // wrap nonce
        "c5ce07965a1046761b03a8f4581ccbbb62bdaa4f68387140", // wrapped DEK (32 bytes,
        "991a51c4689e55a99ab4ec077d2f45c5e5fa353b9fa2bb03", // ... then its 16-byte tag)
        "333333333333333333333333333333333333333333333333", // data nonce
        "98d212ea0c04711562cf89ddf7396f87fc288b340aa6bb28", // ciphertext and tag
    );

    #[test]
    fn format_test_vector() {
        let vault = vault(&[1]);
        let dek = [0x11u8; 32];
        let wrap_nonce = [0x22u8; 24];
        let data_nonce = [0x33u8; 24];
        let sealed = vault
            .seal_with(&"vector".to_owned(), &ctx(), &dek, &wrap_nonce, &data_nonce)
            .unwrap();
        let bytes = sealed.as_bytes();

        // Layout, checked field by field.
        let json = br#""vector""#;
        assert_eq!(bytes.len(), 101 + json.len() + 16);
        assert_eq!(bytes[0], crate::FORMAT_VERSION);
        assert_eq!(bytes[1..5], 1u32.to_be_bytes());
        assert_eq!(bytes[5..29], wrap_nonce);
        assert_eq!(bytes[77..101], data_nonce);

        assert_eq!(hex(bytes), FORMAT_VECTOR);
        assert_eq!(vault.open::<String>(&sealed, &ctx()).unwrap(), "vector");
    }

    #[test]
    fn key_rules() {
        assert_eq!(
            Vault::from_keys(Vec::<(u32, Vec<u8>)>::new()).unwrap_err(),
            VaultError::NoKeys
        );
        assert_eq!(
            Vault::from_keys([(1, vec![7u8; 31])]).unwrap_err(),
            VaultError::KeyTooShort { version: 1 }
        );
        assert!(Vault::from_keys([(1, vec![7u8; 32])]).is_ok());
        assert_eq!(
            Vault::from_keys([(0, material(1))]).unwrap_err(),
            VaultError::InvalidVersion
        );
        assert_eq!(
            Vault::from_keys([(3, material(1)), (3, material(2))]).unwrap_err(),
            VaultError::DuplicateVersion { version: 3 }
        );
    }

    #[test]
    fn from_settings_reads_the_configured_keys() {
        let settings: postit_config::VaultSettings = serde_json::from_value(serde_json::json!({
            "keys": { "1": KEY_1, "2": KEY_2 }
        }))
        .unwrap();
        let from_settings = Vault::from_settings(&settings).unwrap();
        assert_eq!(from_settings.versions(), vec![1, 2]);
        let sealed = vault(&[1, 2]).seal(&"value".to_owned(), &ctx()).unwrap();
        assert_eq!(
            from_settings.open::<String>(&sealed, &ctx()).unwrap(),
            "value"
        );

        let empty = postit_config::VaultSettings::default();
        assert_eq!(
            Vault::from_settings(&empty).unwrap_err(),
            VaultError::NoKeys
        );

        let bad: postit_config::VaultSettings = serde_json::from_value(serde_json::json!({
            "keys": { "01": KEY_1 }
        }))
        .unwrap();
        assert_eq!(
            Vault::from_settings(&bad).unwrap_err(),
            VaultError::InvalidVersion
        );
    }

    #[test]
    fn nothing_secret_in_debug_or_errors() {
        let vault = vault(&[1, 2]);
        let plaintext = "plaintext-marker-value";
        let sealed = vault.seal(&plaintext, &ctx()).unwrap();

        let secrets = [
            KEY_1.to_owned(),
            KEY_2.to_owned(),
            hex(KEY_1.as_bytes()),
            hex(KEY_2.as_bytes()),
            plaintext.to_owned(),
            hex(plaintext.as_bytes()),
            hex(&sealed.as_bytes()[101..]),
            hex(&sealed.as_bytes()[29..77]),
            format!("{:?}", sealed.as_bytes()),
            format!("{:?}", &sealed.as_bytes()[101..]),
        ];

        let vault_debug = format!("{vault:?}");
        assert_eq!(vault_debug, "Vault { versions: [1, 2] }");
        let sealed_debug = format!("{sealed:?}");
        assert_eq!(
            sealed_debug,
            format!("Sealed(kek v2, {} bytes)", sealed.as_bytes().len())
        );

        let errors = [
            VaultError::NoKeys,
            VaultError::KeyTooShort { version: 1 },
            VaultError::InvalidVersion,
            VaultError::DuplicateVersion { version: 1 },
            VaultError::UnknownKeyVersion { version: 1 },
            VaultError::Malformed,
            VaultError::AuthenticationFailed,
            VaultError::Serialization,
        ];
        let mut outputs = vec![vault_debug, sealed_debug];
        for error in &errors {
            outputs.push(format!("{error}"));
            outputs.push(format!("{error:?}"));
        }
        for output in &outputs {
            for secret in &secrets {
                assert!(!output.contains(secret.as_str()), "{output} leaks");
            }
        }
    }
}
