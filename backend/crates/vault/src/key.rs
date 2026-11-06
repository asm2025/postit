use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::sync::Arc;

use hkdf::Hkdf;
use postit_config::VaultSettings;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::VaultError;

/// Shortest key material accepted, in bytes.
const MIN_KEY_BYTES: usize = 32;

/// A derived 32-byte key-encryption key, wiped on drop.
pub(crate) type Kek = Zeroizing<[u8; 32]>;

/// Seals and opens secret values with envelope encryption.
///
/// Holds one key-encryption key per configured version; the highest version seals new
/// values, and every held version opens. Cloning is cheap (the key ring is shared).
#[derive(Clone)]
pub struct Vault {
    ring: Arc<KeyRing>,
}

/// Key-encryption keys by version. Never empty.
struct KeyRing {
    keks: BTreeMap<u32, Kek>,
    newest: u32,
}

impl Vault {
    /// Builds a vault from the `[vault]` settings.
    ///
    /// # Errors
    ///
    /// [`VaultError::InvalidVersion`] when a key name is not a canonical positive integer,
    /// and every error of [`Vault::from_keys`].
    pub fn from_settings(settings: &VaultSettings) -> Result<Self, VaultError> {
        let versions = settings
            .versions()
            .map_err(|_| VaultError::InvalidVersion)?;
        Self::from_keys(
            versions
                .into_iter()
                .map(|(version, key)| (version, key.expose().as_bytes().to_vec())),
        )
    }

    /// Builds a vault from `(version, key material)` pairs.
    ///
    /// # Errors
    ///
    /// [`VaultError::NoKeys`] for an empty list, [`VaultError::InvalidVersion`] for version
    /// `0`, [`VaultError::KeyTooShort`] for material under 32 bytes, and
    /// [`VaultError::DuplicateVersion`] for a repeated version.
    pub fn from_keys(keys: impl IntoIterator<Item = (u32, Vec<u8>)>) -> Result<Self, VaultError> {
        let mut derived = BTreeMap::new();
        for (version, material) in keys {
            let material = Zeroizing::new(material);
            if version == 0 {
                return Err(VaultError::InvalidVersion);
            }
            if material.len() < MIN_KEY_BYTES {
                return Err(VaultError::KeyTooShort { version });
            }
            match derived.entry(version) {
                Entry::Occupied(_) => return Err(VaultError::DuplicateVersion { version }),
                Entry::Vacant(slot) => {
                    slot.insert(derive_kek(version, &material)?);
                }
            }
        }
        let newest = *derived.keys().next_back().ok_or(VaultError::NoKeys)?;
        Ok(Self {
            ring: Arc::new(KeyRing {
                keks: derived,
                newest,
            }),
        })
    }

    /// The version that seals new values (the highest configured).
    #[must_use]
    pub fn newest_version(&self) -> u32 {
        self.ring.newest
    }

    /// Every configured version, lowest first.
    #[must_use]
    pub fn versions(&self) -> Vec<u32> {
        self.ring.keks.keys().copied().collect()
    }

    /// A 32-byte key for `purpose`, derived with HKDF-SHA-256 from the newest version's
    /// key-encryption key, with info `postit:derive:<purpose>`.
    ///
    /// Rotating the vault's newest key changes every derived key, so use one only for a value
    /// that may change on rotation (a keyed hash that marks repeats, not one that must match
    /// rows hashed earlier).
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownKeyVersion`] when the ring lost its newest version and
    /// [`VaultError::KeyDerivation`] when HKDF refuses the output length. Neither can happen
    /// with a vault built by [`Vault::from_keys`]; they are errors, never a zero key.
    pub fn derive_key(&self, purpose: &str) -> Result<Zeroizing<[u8; 32]>, VaultError> {
        let (_, kek) = self.newest_kek()?;
        let mut out = Zeroizing::new([0u8; 32]);
        let info = format!("postit:derive:{purpose}");
        Hkdf::<Sha256>::new(None, kek.as_ref())
            .expand(info.as_bytes(), out.as_mut())
            .map_err(|_| VaultError::KeyDerivation)?;
        Ok(out)
    }

    /// The key-encryption key of `version`.
    pub(crate) fn kek(&self, version: u32) -> Result<&Kek, VaultError> {
        self.ring
            .keks
            .get(&version)
            .ok_or(VaultError::UnknownKeyVersion { version })
    }

    /// The newest version and its key-encryption key.
    pub(crate) fn newest_kek(&self) -> Result<(u32, &Kek), VaultError> {
        let version = self.ring.newest;
        Ok((version, self.kek(version)?))
    }
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("versions", &self.versions())
            .finish()
    }
}

/// HKDF-SHA256, no salt, info `postit vault kek v{version}`, 32-byte output.
fn derive_kek(version: u32, material: &[u8]) -> Result<Kek, VaultError> {
    let mut kek = Zeroizing::new([0u8; 32]);
    let info = format!("postit vault kek v{version}");
    Hkdf::<Sha256>::new(None, material)
        .expand(info.as_bytes(), kek.as_mut())
        // Unreachable: 32 bytes is far below HKDF-SHA256's 8160-byte output limit.
        .map_err(|_| VaultError::KeyTooShort { version })?;
    Ok(kek)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::Vault;

    fn vault(material: &[u8]) -> Vault {
        Vault::from_keys([(1, material.to_vec())]).unwrap()
    }

    #[test]
    fn derive_key_is_stable_and_separated() {
        let a = vault(b"0123456789abcdef0123456789abcdef-one");
        let same = vault(b"0123456789abcdef0123456789abcdef-one");
        let other = vault(b"0123456789abcdef0123456789abcdef-two");
        assert_eq!(
            *a.derive_key("ai_prompt_hash").unwrap(),
            *same.derive_key("ai_prompt_hash").unwrap()
        );
        assert_ne!(
            *a.derive_key("ai_prompt_hash").unwrap(),
            *a.derive_key("something_else").unwrap()
        );
        assert_ne!(
            *a.derive_key("ai_prompt_hash").unwrap(),
            *other.derive_key("ai_prompt_hash").unwrap()
        );
        assert_ne!(*a.derive_key("ai_prompt_hash").unwrap(), [0u8; 32]);
    }

    #[test]
    fn derive_key_follows_the_newest_version() {
        let one = vault(b"0123456789abcdef0123456789abcdef-one");
        let two = Vault::from_keys([
            (1, b"0123456789abcdef0123456789abcdef-one".to_vec()),
            (2, b"0123456789abcdef0123456789abcdef-two".to_vec()),
        ])
        .unwrap();
        assert_ne!(*one.derive_key("p").unwrap(), *two.derive_key("p").unwrap());
    }
}
