//! The Web Push message body: RFC 8291 key derivation over RFC 8188 `aes128gcm`, one record.
//!
//! Built on `RustCrypto` (`p256` ECDH, `hkdf`, `aes-gcm`) rather than a Web Push crate: the
//! candidates either pull in OpenSSL (`web-push` through `ece`) or build the VAPID audience
//! without the endpoint's port (`web-push-native`), which RFC 8292 requires.

use aes_gcm::aead::{Aead as _, KeyInit as _};
use aes_gcm::{Aes128Gcm, Nonce};
use hkdf::Hkdf;
use p256::elliptic_curve::Generate as _;
use p256::elliptic_curve::sec1::ToSec1Point as _;
use p256::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore as _};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::NotifyError;

/// The record size written in the header (RFC 8291 section 4 uses 4096).
const RECORD_SIZE: u32 = 4096;
/// The largest plaintext one message holds. RFC 8291 section 4 caps the whole `aes128gcm` body
/// at 4096 bytes (the record size), and that body carries the 86-byte header (16-byte salt,
/// 4-byte record size, 1-byte key ID length, 65-byte sender key) besides the single record, which
/// adds the 16-byte AEAD tag and the 1-byte padding delimiter: 4096 - 86 - 16 - 1 = 3993.
pub const MAX_PLAINTEXT: usize = 3993;
/// An uncompressed P-256 point: `0x04 || x || y`.
pub const PUBLIC_KEY_LEN: usize = 65;
/// The subscription's authentication secret.
pub const AUTH_LEN: usize = 16;

fn invalid(what: &str) -> NotifyError {
    NotifyError::Push(format!("web push encryption failed: {what}"))
}

/// The subscription's public key, which must be an uncompressed P-256 point.
///
/// # Errors
///
/// [`NotifyError::Push`] when `bytes` is not one.
pub fn parse_public_key(bytes: &[u8]) -> Result<PublicKey, NotifyError> {
    if bytes.len() != PUBLIC_KEY_LEN || bytes[0] != 0x04 {
        return Err(invalid(
            "the subscription key is not an uncompressed P-256 point",
        ));
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| invalid("the subscription key is not on P-256"))
}

fn uncompressed(key: &PublicKey) -> Vec<u8> {
    key.as_affine().to_sec1_point(false).as_bytes().to_vec()
}

/// Encrypts `plaintext` for the subscription (`ua_public`, `auth`) with a fresh sender key
/// and salt.
///
/// # Errors
///
/// [`NotifyError::Push`] when a key is malformed, the plaintext is longer than
/// [`MAX_PLAINTEXT`], or the system random source fails.
pub fn encrypt(plaintext: &[u8], ua_public: &[u8], auth: &[u8]) -> Result<Vec<u8>, NotifyError> {
    let sender = SecretKey::try_generate().map_err(|_| invalid("no random source"))?;
    let mut salt = [0u8; 16];
    getrandom(&mut salt)?;
    seal(plaintext, ua_public, auth, &sender, salt)
}

fn getrandom(buf: &mut [u8]) -> Result<(), NotifyError> {
    OsRng
        .try_fill_bytes(buf)
        .map_err(|_| invalid("no random source"))
}

fn seal(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    sender: &SecretKey,
    salt: [u8; 16],
) -> Result<Vec<u8>, NotifyError> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(invalid("the message is too long"));
    }
    if auth.len() != AUTH_LEN {
        return Err(invalid("the authentication secret is not 16 bytes"));
    }
    let ua = parse_public_key(ua_public)?;
    let as_public = uncompressed(&sender.public_key());
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), ua.as_affine());
    let (cek, nonce) = derive(
        shared.raw_secret_bytes().as_slice(),
        auth,
        &uncompressed(&ua),
        &as_public,
        &salt,
    )?;

    let mut record = Vec::with_capacity(plaintext.len() + 1);
    record.extend_from_slice(plaintext);
    record.push(0x02); // the last (and only) record's delimiter
    let cipher =
        Aes128Gcm::new_from_slice(cek.as_slice()).map_err(|_| invalid("bad content key"))?;
    let sealed = cipher
        .encrypt(&Nonce::from(nonce), record.as_slice())
        .map_err(|_| invalid("aes-gcm failed"))?;

    let mut body = Vec::with_capacity(16 + 4 + 1 + as_public.len() + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(as_public.len()).map_err(|_| invalid("key id too long"))?);
    body.extend_from_slice(&as_public);
    body.extend_from_slice(&sealed);
    Ok(body)
}

/// RFC 8291 section 3.4: the input keying material from the ECDH secret and `auth`, then
/// RFC 8188's content encryption key and nonce from it and `salt`.
fn derive(
    ecdh_secret: &[u8],
    auth: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    salt: &[u8],
) -> Result<(Zeroizing<[u8; 16]>, [u8; 12]), NotifyError> {
    let mut key_info = Vec::with_capacity(14 + ua_public.len() + as_public.len());
    key_info.extend_from_slice(b"WebPush: info\0");
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    // The keying material and the content key are wiped when they go out of scope.
    let mut ikm = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(auth), ecdh_secret)
        .expand(&key_info, ikm.as_mut_slice())
        .map_err(|_| invalid("hkdf"))?;

    let prk = Hkdf::<Sha256>::new(Some(salt), ikm.as_slice());
    let mut cek = Zeroizing::new([0u8; 16]);
    prk.expand(b"Content-Encoding: aes128gcm\0", cek.as_mut_slice())
        .map_err(|_| invalid("hkdf"))?;
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| invalid("hkdf"))?;
    Ok((cek, nonce))
}

/// Encrypts with a fixed sender key and salt, for the RFC 8291 test vector. Never use it to
/// send: reusing a salt with a key breaks the encryption.
///
/// # Errors
///
/// As [`encrypt`], and when `as_private` is not a P-256 scalar.
#[cfg(feature = "testkit")]
pub fn encrypt_with(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    as_private: &[u8],
    salt: [u8; 16],
) -> Result<Vec<u8>, NotifyError> {
    let sender = SecretKey::from_slice(as_private).map_err(|_| invalid("bad sender key"))?;
    seal(plaintext, ua_public, auth, &sender, salt)
}

/// Decrypts a one-record body as the subscriber holding `ua_private` and `auth` would.
///
/// # Errors
///
/// [`NotifyError::Push`] when the body is malformed or does not open.
#[cfg(feature = "testkit")]
pub fn decrypt(body: &[u8], ua_private: &[u8], auth: &[u8]) -> Result<Vec<u8>, NotifyError> {
    let ua = SecretKey::from_slice(ua_private).map_err(|_| invalid("bad subscriber key"))?;
    if body.len() < 21 {
        return Err(invalid("short body"));
    }
    let (salt, rest) = body.split_at(16);
    let id_len = usize::from(rest[4]);
    let rest = &rest[5..];
    if rest.len() < id_len {
        return Err(invalid("short key id"));
    }
    let (as_public, sealed) = rest.split_at(id_len);
    let sender = parse_public_key(as_public)?;
    let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), sender.as_affine());
    let (cek, nonce) = derive(
        shared.raw_secret_bytes().as_slice(),
        auth,
        &uncompressed(&ua.public_key()),
        as_public,
        salt,
    )?;
    let cipher =
        Aes128Gcm::new_from_slice(cek.as_slice()).map_err(|_| invalid("bad content key"))?;
    let mut record = cipher
        .decrypt(&Nonce::from(nonce), sealed)
        .map_err(|_| invalid("does not open"))?;
    match record.pop() {
        Some(0x02) => Ok(record),
        _ => Err(invalid("missing delimiter")),
    }
}

/// A browser subscription for tests: a P-256 key pair and an authentication secret.
#[cfg(feature = "testkit")]
pub struct TestSubscriber {
    private: SecretKey,
    auth: [u8; AUTH_LEN],
}

#[cfg(feature = "testkit")]
impl TestSubscriber {
    /// A fresh subscription.
    ///
    /// # Panics
    ///
    /// When the system random source fails; this is a test helper.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn generate() -> Self {
        let private = SecretKey::try_generate().expect("random key");
        let mut auth = [0u8; AUTH_LEN];
        getrandom(&mut auth).expect("random auth");
        Self { private, auth }
    }

    /// The `p256dh` value a browser sends: the public key, base64url without padding.
    #[must_use]
    pub fn public_b64url(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(uncompressed(&self.private.public_key()))
    }

    /// The `auth` value a browser sends, base64url without padding.
    #[must_use]
    pub fn auth_b64url(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.auth)
    }

    /// Opens a pushed body.
    ///
    /// # Errors
    ///
    /// As [`decrypt`].
    pub fn decrypt(&self, body: &[u8]) -> Result<Vec<u8>, NotifyError> {
        decrypt(body, &self.private.to_bytes(), &self.auth)
    }
}
