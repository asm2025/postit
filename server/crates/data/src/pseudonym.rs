use hmac::{Hmac, Mac};
use postit_core::UserId;
use sha2::Sha256;
use uuid::Uuid;

/// `HMAC-SHA256(key, user_id bytes)`, truncated to 16 bytes, stamped as a `UUIDv8` with the
/// RFC 4122 variant. postit never generates v8 UUIDs otherwise, so `GET /admin/audit` can
/// recognise a pseudonym by its version alone. Same key + same user → same pseudonym, so a
/// deleted user's events stay linked to each other.
#[must_use]
pub fn pseudonym_for(key: &[u8], user: UserId) -> Uuid {
    // HMAC accepts keys of any length; `new_from_slice` only fails for fixed-size MACs.
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(key) else {
        unreachable!("HMAC-SHA256 accepts keys of any length")
    };
    mac.update(user.as_uuid().as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80; // version 8
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant (10xx)
    Uuid::from_bytes(bytes)
}
