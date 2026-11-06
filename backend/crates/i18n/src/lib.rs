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
            (Some("en-GB"), Locale::EnUs),
            (Some("fr-CA, en;q=0.5"), Locale::EnUs),
            (Some("en;q=0"), Locale::EnUs),
            (Some("en;q=bad, fr"), Locale::EnUs),
            (Some(""), Locale::EnUs),
            (Some("%%%"), Locale::EnUs),
            (None, Locale::EnUs),
        ] {
            assert_eq!(negotiate(header), want, "{header:?}");
        }
    }
    #[test]
    fn catalogs_have_identical_ids() {
        for locale in Locale::ALL {
            assert_eq!(
                message_ids(locale),
                message_ids(Locale::EnUs),
                "{}",
                locale.tag()
            );
        }
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
    fn missing_id_falls_back() {
        assert_eq!(t(Locale::EnUs, "no-such-id", &Args::new()), "no-such-id");
    }
    #[test]
    fn tags_round_trip() {
        for l in Locale::ALL {
            assert_eq!(Locale::from_tag(l.tag()), Some(l));
        }
        assert_eq!(Locale::from_tag("en"), None);
    }
    #[test]
    fn dir_follows_the_direction() {
        for l in Locale::ALL {
            assert_eq!(l.dir(), if l.is_rtl() { "rtl" } else { "ltr" });
        }
        assert_eq!(Locale::EnUs.dir(), "ltr");
    }
}
