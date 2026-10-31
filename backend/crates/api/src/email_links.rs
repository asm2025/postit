//! Signed email links: one-click unsubscribe, click tracking, and the open pixel.
//!
//! A token is URL-safe unpadded base64 of `payload || tag`, where the tag is
//! `HMAC-SHA256(key, label || 0x00 || payload)` cut to 16 bytes. The label keeps a token of
//! one kind from verifying as another. Tokens carry no clock and never expire: an old
//! message must keep unsubscribing.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use postit_core::{
    DirectRecipient, DirectSendId, EmailLanguage, LinkSigner, LinkSignerSource, UserId,
};
use sha2::Sha256;
use url::Url;
use uuid::Uuid;

const TAG_LEN: usize = 16;
const UNSUBSCRIBE: &[u8] = b"postit-unsub-v1";
const CLICK: &[u8] = b"postit-click-v1";
const OPEN: &[u8] = b"postit-open-v1";

/// What an unsubscribe token carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsubscribe {
    /// The owner whose list the recipient leaves.
    pub owner: UserId,
    /// `email_hash` of the recipient's address.
    pub hash: [u8; 32],
    /// The language of the unsubscribe page.
    pub language: EmailLanguage,
    /// The send the link came from.
    pub send: DirectSendId,
}

/// What a click token carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    /// The send the link came from.
    pub send: DirectSendId,
    /// Where to redirect.
    pub destination: Url,
}

/// What an open token carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    /// The send the pixel came from.
    pub send: DirectSendId,
}

/// The link base is not an `http` or `https` URL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the email link base must be an http or https URL")]
pub struct NotWebBase;

/// Signs and verifies email links under `email.signing_key`.
#[derive(Clone)]
pub struct EmailLinks {
    key: Arc<[u8]>,
    /// An `http(s)` URL whose path ends in `/`, with no query or fragment.
    base: Url,
}

impl EmailLinks {
    /// Links under `base` (`email.tracking_base_url` or `server.public_url`). Its query and
    /// fragment are dropped.
    ///
    /// # Errors
    ///
    /// [`NotWebBase`] unless `base` is `http` or `https`: links must open in a browser, and
    /// only such a base can take a path.
    pub fn new(key: &[u8], mut base: Url) -> Result<Self, NotWebBase> {
        if !is_web(&base) {
            return Err(NotWebBase);
        }
        // Links are the base path plus a route, so the path must end in `/`.
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self {
            key: Arc::from(key),
            base,
        })
    }

    fn mac(&self, label: &[u8], payload: &[u8]) -> Hmac<Sha256> {
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(&self.key) else {
            unreachable!("HMAC-SHA256 accepts keys of any length")
        };
        mac.update(label);
        mac.update(&[0]);
        mac.update(payload);
        mac
    }

    fn seal(&self, label: &[u8], payload: &[u8]) -> String {
        let tag = self.mac(label, payload).finalize().into_bytes();
        let mut raw = payload.to_vec();
        raw.extend_from_slice(&tag[..TAG_LEN]);
        URL_SAFE_NO_PAD.encode(raw)
    }

    /// The payload of `token` when its tag is valid for `label`.
    fn open(&self, label: &[u8], token: &str) -> Option<Vec<u8>> {
        let raw = URL_SAFE_NO_PAD.decode(token).ok()?;
        let split = raw.len().checked_sub(TAG_LEN)?;
        let (payload, tag) = raw.split_at(split);
        // Constant-time comparison of the truncated tag.
        self.mac(label, payload).verify_truncated_left(tag).ok()?;
        Some(payload.to_vec())
    }

    /// `{base}public/{route}/{token}`. Tokens are URL-safe base64, so nothing is escaped.
    fn url(&self, route: &str, token: &str) -> Url {
        let mut url = self.base.clone();
        url.set_path(&format!("{}public/{route}/{token}", self.base.path()));
        url
    }

    /// The decoded unsubscribe token, or `None` when forged, damaged, or of another kind.
    #[must_use]
    pub fn verify_unsubscribe(&self, token: &str) -> Option<Unsubscribe> {
        let p = self.open(UNSUBSCRIBE, token)?;
        if p.len() != 16 + 32 + 1 + 16 {
            return None;
        }
        let owner = UserId::from_uuid(Uuid::from_slice(&p[..16]).ok()?);
        let hash: [u8; 32] = p[16..48].try_into().ok()?;
        let language = match p[48] {
            0 => EmailLanguage::EnUs,
            1 => EmailLanguage::Ar,
            _ => return None,
        };
        let send = DirectSendId::from_uuid(Uuid::from_slice(&p[49..]).ok()?);
        Some(Unsubscribe {
            owner,
            hash,
            language,
            send,
        })
    }

    /// The decoded click token, or `None` when forged, damaged, or of another kind.
    #[must_use]
    pub fn verify_click(&self, token: &str) -> Option<Click> {
        let p = self.open(CLICK, token)?;
        if p.len() <= 16 {
            return None;
        }
        let send = DirectSendId::from_uuid(Uuid::from_slice(&p[..16]).ok()?);
        let destination = Url::parse(std::str::from_utf8(&p[16..]).ok()?).ok()?;
        if !is_web(&destination) {
            return None;
        }
        Some(Click { send, destination })
    }

    /// The decoded open token, or `None` when forged, damaged, or of another kind.
    #[must_use]
    pub fn verify_open(&self, token: &str) -> Option<Open> {
        let p = self.open(OPEN, token)?;
        let send = DirectSendId::from_uuid(Uuid::from_slice(&p).ok()?);
        Some(Open { send })
    }
}

fn is_web(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// A signer bound to one owner.
struct OwnerLinks {
    links: EmailLinks,
    owner: UserId,
}

impl LinkSigner for OwnerLinks {
    fn unsubscribe_url(&self, recipient: &DirectRecipient, language: EmailLanguage) -> Url {
        let mut payload = Vec::with_capacity(65);
        payload.extend_from_slice(self.owner.as_uuid().as_bytes());
        payload.extend_from_slice(&postit_data::email_hash::email_hash(
            &self.links.key,
            &recipient.address,
        ));
        payload.push(match language {
            EmailLanguage::EnUs => 0,
            EmailLanguage::Ar => 1,
        });
        payload.extend_from_slice(recipient.send_id.as_uuid().as_bytes());
        self.links.url("u", &self.links.seal(UNSUBSCRIBE, &payload))
    }

    fn click_url(&self, send_id: DirectSendId, destination: &Url) -> Url {
        if !is_web(destination) {
            // Never signed: the verifier would refuse it, so the link is left as it is.
            return destination.clone();
        }
        let mut payload = send_id.as_uuid().as_bytes().to_vec();
        payload.extend_from_slice(destination.as_str().as_bytes());
        self.links.url("t/c", &self.links.seal(CLICK, &payload))
    }

    fn open_pixel_url(&self, send_id: DirectSendId) -> Url {
        self.links
            .url("t/o", &self.links.seal(OPEN, send_id.as_uuid().as_bytes()))
    }
}

impl LinkSignerSource for EmailLinks {
    fn for_owner(&self, owner: UserId) -> Arc<dyn LinkSigner> {
        Arc::new(OwnerLinks {
            links: self.clone(),
            owner,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

    fn links() -> EmailLinks {
        EmailLinks::new(KEY, Url::parse("https://postit.example/").unwrap()).unwrap()
    }

    fn owner() -> UserId {
        UserId::from_uuid(Uuid::from_u128(7))
    }

    fn send() -> DirectSendId {
        DirectSendId::from_uuid(Uuid::from_u128(9))
    }

    fn recipient() -> DirectRecipient {
        DirectRecipient {
            send_id: send(),
            address: "Bob@Example.com".into(),
            merge_fields: BTreeMap::new(),
            locale: "en-US".into(),
        }
    }

    fn token(url: &Url) -> String {
        url.path_segments().unwrap().next_back().unwrap().to_owned()
    }

    fn unsub_url() -> Url {
        links()
            .for_owner(owner())
            .unsubscribe_url(&recipient(), EmailLanguage::Ar)
    }

    fn click_url() -> Url {
        links()
            .for_owner(owner())
            .click_url(send(), &Url::parse("https://example.org/a?b=c").unwrap())
    }

    fn open_url() -> Url {
        links().for_owner(owner()).open_pixel_url(send())
    }

    #[test]
    fn each_kind_round_trips() {
        let l = links();
        let u = unsub_url();
        assert!(u.path().starts_with("/public/u/"));
        let got = l.verify_unsubscribe(&token(&u)).unwrap();
        assert_eq!(got.owner, owner());
        assert_eq!(got.language, EmailLanguage::Ar);
        assert_eq!(got.send, send());
        assert_eq!(
            got.hash,
            postit_data::email_hash::email_hash(KEY, "bob@example.com")
        );

        let c = click_url();
        assert!(c.path().starts_with("/public/t/c/"));
        let got = l.verify_click(&token(&c)).unwrap();
        assert_eq!(got.send, send());
        assert_eq!(got.destination.as_str(), "https://example.org/a?b=c");

        let o = open_url();
        assert!(o.path().starts_with("/public/t/o/"));
        assert_eq!(l.verify_open(&token(&o)).unwrap().send, send());
    }

    #[test]
    fn tampered_token_refused() {
        let l = links();
        for (t, kind) in [
            (token(&unsub_url()), 0),
            (token(&click_url()), 1),
            (token(&open_url()), 2),
        ] {
            let mut raw = URL_SAFE_NO_PAD.decode(&t).unwrap();
            raw[0] ^= 1;
            let bad = URL_SAFE_NO_PAD.encode(raw);
            assert!(match kind {
                0 => l.verify_unsubscribe(&bad).is_none(),
                1 => l.verify_click(&bad).is_none(),
                _ => l.verify_open(&bad).is_none(),
            });
        }
        assert!(l.verify_open("").is_none());
        assert!(l.verify_open("!!!").is_none());
        assert!(l.verify_open("AAAA").is_none());
        let other = EmailLinks::new(b"another key another key another!", links().base).unwrap();
        assert!(other.verify_open(&token(&open_url())).is_none());
    }

    #[test]
    fn base_with_path_keeps_its_segments() {
        for base in ["https://h.example/app", "https://h.example/app/"] {
            let l = EmailLinks::new(KEY, Url::parse(base).unwrap()).unwrap();
            let u = l.for_owner(owner()).open_pixel_url(send());
            assert!(u.path().starts_with("/app/public/t/o/"), "{u}");
            assert!(l.verify_open(&token(&u)).is_some());
        }
    }

    #[test]
    fn base_must_be_web_and_loses_query_and_fragment() {
        for base in ["mailto:links@example.org", "ftp://h.example/", "data:text/plain,x"] {
            assert!(
                EmailLinks::new(KEY, Url::parse(base).unwrap()).is_err(),
                "{base}"
            );
        }
        let l = EmailLinks::new(KEY, Url::parse("https://h.example/app?x=1#top").unwrap()).unwrap();
        let u = l.for_owner(owner()).open_pixel_url(send());
        assert!(u.as_str().starts_with("https://h.example/app/public/t/o/"), "{u}");
        assert_eq!((u.query(), u.fragment()), (None, None));
        assert!(l.verify_open(&token(&u)).is_some());
    }

    #[test]
    fn labels_are_domain_separated() {
        assert_eq!(UNSUBSCRIBE, b"postit-unsub-v1");
        assert_eq!(CLICK, b"postit-click-v1");
        assert_eq!(OPEN, b"postit-open-v1");
    }

    #[test]
    fn non_web_destination_is_not_signed_or_verified() {
        let l = links();
        let bad = Url::parse("javascript:alert(1)").unwrap();
        assert_eq!(l.for_owner(owner()).click_url(send(), &bad), bad);
        let mut payload = send().as_uuid().as_bytes().to_vec();
        payload.extend_from_slice(b"ftp://example.org/x");
        assert!(l.verify_click(&l.seal(CLICK, &payload)).is_none());
    }

    #[test]
    fn extended_or_truncated_payloads_are_refused() {
        let l = links();
        // Click with no destination, and with a garbage one.
        let id = send().as_uuid().as_bytes().to_vec();
        assert!(l.verify_click(&l.seal(CLICK, &id)).is_none());
        let mut junk = id.clone();
        junk.extend_from_slice(&[0xff, 0xfe]);
        assert!(l.verify_click(&l.seal(CLICK, &junk)).is_none());
        // Open with one byte too many or too few.
        let mut long = id.clone();
        long.push(0);
        assert!(l.verify_open(&l.seal(OPEN, &long)).is_none());
        assert!(l.verify_open(&l.seal(OPEN, &id[..15])).is_none());
        // Unsubscribe likewise.
        let raw = URL_SAFE_NO_PAD.decode(token(&unsub_url())).unwrap();
        let payload = &raw[..raw.len() - TAG_LEN];
        let mut extended = payload.to_vec();
        extended.push(0);
        assert!(
            l.verify_unsubscribe(&l.seal(UNSUBSCRIBE, &extended))
                .is_none()
        );
        assert!(
            l.verify_unsubscribe(&l.seal(UNSUBSCRIBE, &payload[..payload.len() - 1]))
                .is_none()
        );
        // A token cut short loses its tag.
        let t = token(&click_url());
        assert!(l.verify_click(&t[..t.len() - 2]).is_none());
    }

    #[test]
    fn unsub_token_refused_as_click() {
        let l = links();
        let u = token(&unsub_url());
        assert!(l.verify_unsubscribe(&u).is_some());
        assert!(l.verify_click(&u).is_none());
        assert!(l.verify_open(&u).is_none());
    }

    #[test]
    fn click_token_refused_as_unsub_or_open() {
        let l = links();
        let c = token(&click_url());
        assert!(l.verify_click(&c).is_some());
        assert!(l.verify_unsubscribe(&c).is_none());
        assert!(l.verify_open(&c).is_none());
    }

    #[test]
    fn open_token_refused_as_unsub_or_click() {
        let l = links();
        let o = token(&open_url());
        assert!(l.verify_open(&o).is_some());
        assert!(l.verify_unsubscribe(&o).is_none());
        assert!(l.verify_click(&o).is_none());
    }

    #[test]
    fn token_is_not_the_address_hash() {
        let hash = postit_data::email_hash::email_hash(KEY, "bob@example.com");
        let t = token(&unsub_url());
        assert!(!t.is_empty());
        assert_ne!(t, URL_SAFE_NO_PAD.encode(hash));
        assert!(!t.contains(&hex::encode(hash)));
        let raw = URL_SAFE_NO_PAD.decode(&t).unwrap();
        // The address itself is never embedded.
        assert!(!String::from_utf8_lossy(&raw).contains("example.com"));
    }

    #[test]
    fn token_never_expires() {
        // The payload has no timestamp: the same inputs always give the same token.
        assert_eq!(token(&unsub_url()), token(&unsub_url()));
        assert_eq!(
            URL_SAFE_NO_PAD.decode(token(&open_url())).unwrap().len(),
            16 + TAG_LEN
        );
    }

    #[test]
    fn tag_compared_in_constant_time() {
        // A tag differing in its last byte is refused by `verify_truncated_left`.
        let l = links();
        let payload = send().as_uuid().as_bytes().to_vec();
        let full = l.mac(OPEN, &payload).finalize().into_bytes();
        let mut raw = payload.clone();
        raw.extend_from_slice(&full[..TAG_LEN]);
        assert!(l.verify_open(&URL_SAFE_NO_PAD.encode(&raw)).is_some());
        let mut bad = payload;
        bad.extend_from_slice(&full[..TAG_LEN - 1]);
        bad.push(full[TAG_LEN - 1] ^ 1);
        assert!(l.verify_open(&URL_SAFE_NO_PAD.encode(&bad)).is_none());
    }
}
