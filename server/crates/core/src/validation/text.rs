//! Measuring text the way each platform counts it.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// How a platform counts the length of a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TextMeasure {
    /// Extended grapheme clusters: what a person sees as one character.
    Graphemes,
    /// X's weighted count: URLs cost 23, emoji and CJK cost 2, most other text costs 1.
    XWeighted,
    /// UTF-16 code units (what JavaScript's `length` returns).
    Utf16Units,
    /// UTF-8 bytes.
    Bytes,
}

/// Length of a URL after X's t.co shortening.
const X_URL_WEIGHT: u32 = 23;

/// Measures `text` under `measure`. The result saturates at `u32::MAX`.
///
/// `XWeighted` follows X's twitter-text v3 configuration: the text is NFC-normalized, then
/// every whitespace-delimited token that starts with `http://` or `https://` (any case) counts 23
/// plus its trailing sentence punctuation, an
/// emoji sequence counts 2, and every other `char` counts 1 inside the ranges U+0000–U+10FF,
/// U+2000–U+200D, U+2010–U+201F and U+2032–U+2037, and 2 elsewhere. Bare domains without a
/// scheme are counted as text, which can overcount a link X would shorten.
#[must_use]
pub fn measure(text: &str, measure: TextMeasure) -> u32 {
    let count = match measure {
        TextMeasure::Graphemes => text.graphemes(true).count(),
        TextMeasure::Utf16Units => text.encode_utf16().count(),
        TextMeasure::Bytes => text.len(),
        TextMeasure::XWeighted => return x_weighted(text),
    };
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn x_weighted(text: &str) -> u32 {
    let text: String = text.nfc().collect();
    let mut total: u32 = 0;
    let mut rest = text.as_str();
    while let Some(first) = rest.chars().next() {
        let whitespace = first.is_whitespace();
        let end = rest
            .find(|c: char| c.is_whitespace() != whitespace)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        let weight = if !whitespace && has_url_scheme(run) {
            let trimmed = run.trim_end_matches(is_url_trailing);
            let tail: u32 = run[trimmed.len()..]
                .graphemes(true)
                .map(grapheme_weight)
                .sum();
            X_URL_WEIGHT.saturating_add(tail)
        } else {
            run.graphemes(true).map(grapheme_weight).sum()
        };
        total = total.saturating_add(weight);
        rest = tail;
    }
    total
}

fn has_url_scheme(run: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        run.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// Punctuation that ends a sentence or a bracket rather than a link.
fn is_url_trailing(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'
    )
}

fn grapheme_weight(grapheme: &str) -> u32 {
    if is_emoji_sequence(grapheme) {
        2
    } else {
        grapheme.chars().map(char_weight).sum()
    }
}

fn is_emoji_sequence(grapheme: &str) -> bool {
    grapheme.chars().nth(1).is_some()
        && grapheme.chars().any(|c| {
            matches!(
                c,
                '\u{200D}' | '\u{FE0F}' | '\u{20E3}' | '\u{1F1E6}'..='\u{1F1FF}' | '\u{1F3FB}'..='\u{1F3FF}'
            )
        })
}

fn char_weight(c: char) -> u32 {
    if matches!(
        c,
        '\u{0000}'..='\u{10FF}'
            | '\u{2000}'..='\u{200D}'
            | '\u{2010}'..='\u{201F}'
            | '\u{2032}'..='\u{2037}'
    ) {
        1
    } else {
        2
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use proptest::prelude::*;

    use super::TextMeasure::{Bytes, Graphemes, Utf16Units, XWeighted};
    use super::*;

    #[test]
    fn measures_table() {
        let table: &[(&str, [u32; 4])] = &[
            ("Hello", [5, 5, 5, 5]),
            ("مرحبا", [5, 5, 5, 10]),
            ("你好", [2, 4, 2, 6]),
            ("👨\u{200D}👩\u{200D}👧", [1, 2, 8, 18]),
            ("e\u{301}", [1, 1, 2, 3]),
            ("see https://example.com/a/very/long/path", [40, 27, 40, 40]),
            ("🇸🇦", [1, 2, 4, 8]),
            ("1\u{FE0F}\u{20E3}", [1, 2, 3, 7]),
        ];
        for (input, expected) in table {
            for (m, want) in [Graphemes, XWeighted, Utf16Units, Bytes]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(measure(input, m), *want, "{input:?} as {m:?}");
            }
        }
    }

    #[test]
    fn x_weighted_280_ascii_fits() {
        assert_eq!(measure(&"a".repeat(280), XWeighted), 280);
    }

    #[test]
    fn arabic_with_harakat_counts_code_points() {
        let text = "مَرْحَبًا";
        assert_eq!(measure(text, Graphemes), 5);
        assert_eq!(measure(text, XWeighted), 9);
    }

    #[test]
    fn url_inside_text_costs_23_wherever_it_sits() {
        assert_eq!(measure("https://a.example/x", XWeighted), 23);
        assert_eq!(
            measure("a https://a.example/x b", XWeighted),
            1 + 1 + 23 + 1 + 1
        );
        assert_eq!(measure("http://a.example\nnext", XWeighted), 23 + 1 + 4);
    }

    #[test]
    fn url_scheme_is_case_insensitive_and_trailing_punctuation_is_text() {
        assert_eq!(measure("HTTPS://a.example/x", XWeighted), 23);
        assert_eq!(measure("https://a.example.", XWeighted), 23 + 1);
        assert_eq!(measure("https://a.example/x?!", XWeighted), 23 + 2);
        assert_eq!(measure("https://a.example/x)", XWeighted), 23 + 1);
    }

    #[test]
    fn bare_domain_counts_as_text() {
        assert_eq!(measure("example.com", XWeighted), 11);
    }

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_value(XWeighted).unwrap(),
            serde_json::json!("x_weighted")
        );
        assert_eq!(
            serde_json::to_value(Utf16Units).unwrap(),
            serde_json::json!("utf16_units")
        );
    }

    proptest! {
        #[test]
        fn counts_are_ordered(s in ".*") {
            let g = measure(&s, Graphemes);
            let u = measure(&s, Utf16Units);
            let b = measure(&s, Bytes);
            prop_assert!(g <= u);
            prop_assert!(u <= b);
        }
    }
}
