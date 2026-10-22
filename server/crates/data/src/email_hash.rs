//! The one place an email address becomes a hash.
//!
//! Contacts and suppressions both store `email_hash`, so a suppression matches a contact, and
//! survives the contact being deleted, without keeping the address.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// The address as it is compared and hashed: trimmed, with ASCII letters lowercased.
///
/// Only ASCII is folded, so the result does not depend on a locale or on a Unicode version.
#[must_use]
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// `HMAC-SHA256(key, normalize_email(raw))`.
#[must_use]
pub fn email_hash(key: &[u8], raw: &str) -> [u8; 32] {
    // HMAC accepts keys of any length; `new_from_slice` only fails for fixed-size MACs.
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(key) else {
        unreachable!("HMAC-SHA256 accepts keys of any length")
    };
    mac.update(normalize_email(raw).as_bytes());
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::{email_hash, normalize_email};

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_email(" Bob@Example.COM "), "bob@example.com");
        assert_eq!(normalize_email("\tbob@example.com\n"), "bob@example.com");
    }

    #[test]
    fn normalize_leaves_non_ascii_alone() {
        assert_eq!(normalize_email("É@Example.com"), "É@example.com");
    }

    #[test]
    fn hash_ignores_case_and_whitespace_but_not_the_key() {
        let key = b"suppression-key";
        assert_eq!(
            email_hash(key, " Bob@Example.COM "),
            email_hash(key, "bob@example.com")
        );
        assert_ne!(
            email_hash(key, "bob@example.com"),
            email_hash(b"another-key", "bob@example.com")
        );
        assert_ne!(
            email_hash(key, "bob@example.com"),
            email_hash(key, "rob@example.com")
        );
    }
}
