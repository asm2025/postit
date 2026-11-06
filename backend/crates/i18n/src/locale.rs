//! The supported locales and `Accept-Language` negotiation.

use unic_langid::LanguageIdentifier;

/// A locale the server can render text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    /// American English, the default and the fallback.
    #[default]
    EnUs,
}

impl Locale {
    /// Every supported locale.
    pub const ALL: [Locale; 1] = [Locale::EnUs];

    /// The BCP 47 tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Locale::EnUs => "en-US",
        }
    }

    /// The language subtag `Accept-Language` negotiation matches.
    #[must_use]
    pub fn language(self) -> &'static str {
        match self {
            Locale::EnUs => "en",
        }
    }

    /// Parses an exact tag (`en-US`); anything else is `None`.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Locale> {
        Locale::ALL.into_iter().find(|l| l.tag() == tag)
    }

    /// Whether the script runs right to left. Every locale states its direction, and every
    /// server-rendered page and email takes `dir` from it, so a right-to-left locale needs no
    /// other change.
    #[must_use]
    pub fn is_rtl(self) -> bool {
        match self {
            Locale::EnUs => false,
        }
    }

    /// The HTML `dir` attribute value.
    #[must_use]
    pub fn dir(self) -> &'static str {
        if self.is_rtl() { "rtl" } else { "ltr" }
    }
}

/// Picks the most preferred `Accept-Language` entry whose language is a supported locale's, by
/// `q` weight and then by order; otherwise [`Locale::EnUs`]. Entries with `q=0` are refused, not
/// chosen.
#[must_use]
pub fn negotiate(accept_language: Option<&str>) -> Locale {
    let Some(header) = accept_language else {
        return Locale::EnUs;
    };
    let mut ranked: Vec<(f32, Locale)> = header
        .split(',')
        .filter_map(|entry| {
            let mut parts = entry.split(';');
            let tag: LanguageIdentifier = parts.next()?.trim().parse().ok()?;
            let locale = Locale::ALL
                .into_iter()
                .find(|l| l.language() == tag.language.as_str())?;
            let q = parts
                .find_map(|p| p.trim().strip_prefix("q="))
                .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok())?;
            (q > 0.0).then_some((q, locale))
        })
        .collect();
    // Stable, so equal weights keep the header's order.
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    ranked.first().map_or(Locale::EnUs, |&(_, locale)| locale)
}
