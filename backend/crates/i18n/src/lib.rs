//! Localization for server-rendered text: locales, negotiation, and Fluent catalogs.

mod catalog;
mod locale;

pub use catalog::{Args, message_ids, t};
pub use locale::{Locale, negotiate};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_table() {
        for (header, want) in [
            (Some("ar"), Locale::Ar),
            (Some("ar-SA,en;q=0.8"), Locale::Ar),
            (Some("fr-CA, ar;q=0.5"), Locale::Ar),
            (Some("en;q=0.3, ar;q=0.9"), Locale::Ar),
            (Some("ar;q=0.5, en-US"), Locale::EnUs),
            (Some("ar;q=0, en;q=0.1"), Locale::EnUs),
            (Some("ar;q=0"), Locale::EnUs),
            (Some("en, ar"), Locale::EnUs),
            (Some("ar;q=bad, en;q=0.2"), Locale::EnUs),
            (Some("en-GB"), Locale::EnUs),
            (Some(""), Locale::EnUs),
            (Some("%%%"), Locale::EnUs),
            (None, Locale::EnUs),
        ] {
            assert_eq!(negotiate(header), want, "{header:?}");
        }
    }
    #[test]
    fn catalogs_have_identical_ids() {
        assert_eq!(message_ids(Locale::EnUs), message_ids(Locale::Ar));
    }
    #[test]
    fn arabic_numbers_use_arabic_indic_digits() {
        let s = t(Locale::Ar, "pending-subject", &Args::new().num("count", 12));
        assert!(s.contains("١٢") && !s.chars().any(|c| c.is_ascii_digit()));
    }
    #[test]
    fn english_plural() {
        assert_eq!(
            t(
                Locale::EnUs,
                "pending-subject",
                &Args::new().num("count", 1)
            ),
            "1 postit account is waiting for approval"
        );
    }
    #[test]
    fn arabic_plural_forms_differ() {
        let f = |n| t(Locale::Ar, "pending-subject", &Args::new().num("count", n));
        assert_ne!(f(1), f(2));
        assert_ne!(f(2), f(3));
        assert_ne!(f(3), f(11));
    }
    #[test]
    fn missing_id_falls_back() {
        assert_eq!(t(Locale::Ar, "no-such-id", &Args::new()), "no-such-id");
    }
    #[test]
    fn tags_round_trip() {
        for l in Locale::ALL {
            assert_eq!(Locale::from_tag(l.tag()), Some(l));
        }
        assert_eq!(Locale::from_tag("en"), None);
    }
}
