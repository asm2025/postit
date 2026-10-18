//! The supported locales and `Accept-Language` negotiation.

use fluent_langneg::accepted_languages;

/// A locale the server can render text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    /// American English, the default and the fallback.
    #[default]
    EnUs,
    /// Arabic (right to left).
    Ar,
}

impl Locale {
    /// Every supported locale.
    pub const ALL: [Locale; 2] = [Locale::EnUs, Locale::Ar];

    /// The BCP 47 tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Locale::EnUs => "en-US",
            Locale::Ar => "ar",
        }
    }

    /// Parses an exact tag (`en-US` or `ar`); anything else is `None`.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Locale> {
        Locale::ALL.into_iter().find(|l| l.tag() == tag)
    }

    /// Whether the script runs right to left.
    #[must_use]
    pub fn is_rtl(self) -> bool {
        matches!(self, Locale::Ar)
    }

    /// The HTML `dir` attribute value.
    #[must_use]
    pub fn dir(self) -> &'static str {
        if self.is_rtl() { "rtl" } else { "ltr" }
    }
}

/// Picks the first `Accept-Language` entry whose language is `ar` or `en`; otherwise
/// [`Locale::EnUs`].
#[must_use]
pub fn negotiate(accept_language: Option<&str>) -> Locale {
    let Some(header) = accept_language else {
        return Locale::EnUs;
    };
    for lang in accepted_languages::parse(header) {
        match lang.language.as_str() {
            "ar" => return Locale::Ar,
            "en" => return Locale::EnUs,
            _ => {}
        }
    }
    Locale::EnUs
}
