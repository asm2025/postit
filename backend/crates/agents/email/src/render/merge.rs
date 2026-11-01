//! Merge tags: `{{first_name}}`, and `{{first_name|there}}` for a fallback.

use std::collections::BTreeMap;

/// How text is made safe for where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// HTML text or attribute value: `&`, `<`, `>`, `"`, and `'` become entities.
    Html,
    /// A mail header: CR and LF are removed.
    Header,
    /// Plain text, as it is.
    None,
}

/// A merge tag that does not parse. It carries a byte offset, never the text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    /// A `{{` with no `}}` after it.
    #[error("merge tag opened at byte {at} is not closed")]
    Unclosed {
        /// Byte offset of the `{{`.
        at: usize,
    },
    /// A tag with no field name, or one holding a brace.
    #[error("merge tag at byte {at} has no valid field name")]
    BadName {
        /// Byte offset of the `{{`.
        at: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Part {
    Literal(String),
    Field {
        name: String,
        fallback: Option<String>,
    },
}

/// Text with merge tags, parsed once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeTemplate {
    parts: Vec<Part>,
}

impl MergeTemplate {
    /// Parses `source`.
    ///
    /// # Errors
    /// [`MergeError`] when a tag is not closed or has no field name.
    pub fn parse(source: &str) -> Result<Self, MergeError> {
        let mut parts = Vec::new();
        let mut rest = source;
        let mut offset = 0;
        while let Some(open) = rest.find("{{") {
            if open > 0 {
                parts.push(Part::Literal(rest[..open].to_owned()));
            }
            let at = offset + open;
            let inner_start = open + 2;
            let Some(len) = rest[inner_start..].find("}}") else {
                return Err(MergeError::Unclosed { at });
            };
            let inner = &rest[inner_start..inner_start + len];
            let (name, fallback) = match inner.split_once('|') {
                Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
                None => (inner.trim(), None),
            };
            let braces = |s: &str| s.contains(['{', '}']);
            if name.is_empty() || braces(name) || fallback.is_some_and(braces) {
                return Err(MergeError::BadName { at });
            }
            parts.push(Part::Field {
                name: name.to_owned(),
                fallback: fallback.filter(|f| !f.is_empty()).map(str::to_owned),
            });
            let consumed = inner_start + len + 2;
            rest = &rest[consumed..];
            offset += consumed;
        }
        if !rest.is_empty() {
            parts.push(Part::Literal(rest.to_owned()));
        }
        Ok(Self { parts })
    }

    /// The text for one recipient. A missing or empty field renders its fallback, or nothing.
    /// Literal text and values are escaped alike, and a value is written once, never parsed.
    #[must_use]
    pub fn render(&self, fields: &BTreeMap<String, String>, escape: Escape) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => escape_into(&mut out, text, escape),
                Part::Field { name, fallback } => {
                    escape_into(&mut out, value(fields, name, fallback.as_deref()), escape);
                }
            }
        }
        out
    }

    pub(crate) fn parts(&self) -> &[Part] {
        &self.parts
    }
}

/// The value a field renders: the recipient's, else the fallback, else nothing.
pub(crate) fn value<'a>(
    fields: &'a BTreeMap<String, String>,
    name: &str,
    fallback: Option<&'a str>,
) -> &'a str {
    match fields.get(name) {
        Some(v) if !v.is_empty() => v,
        _ => fallback.unwrap_or_default(),
    }
}

/// Appends `text` to `out`, made safe by `escape`.
pub(crate) fn escape_into(out: &mut String, text: &str, escape: Escape) {
    match escape {
        Escape::None => out.push_str(text),
        Escape::Header => out.extend(text.chars().filter(|c| !matches!(c, '\r' | '\n'))),
        Escape::Html => {
            for c in text.chars() {
                match c {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    '\'' => out.push_str("&#39;"),
                    _ => out.push(c),
                }
            }
        }
    }
}

/// `text` made safe by `escape`.
pub(crate) fn escaped(text: &str, escape: Escape) -> String {
    let mut out = String::with_capacity(text.len());
    escape_into(&mut out, text, escape);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn merge_with_fallback() {
        let t = MergeTemplate::parse("Hi {{first_name|there}}!").unwrap();
        assert_eq!(t.render(&fields(&[]), Escape::None), "Hi there!");
        assert_eq!(
            t.render(&fields(&[("first_name", "Sam")]), Escape::None),
            "Hi Sam!"
        );
        // An empty value takes the fallback too.
        assert_eq!(
            t.render(&fields(&[("first_name", "")]), Escape::None),
            "Hi there!"
        );
        // Spaces around the name and the fallback are trimmed.
        let t = MergeTemplate::parse("Hi {{ first_name | friend }}").unwrap();
        assert_eq!(t.render(&fields(&[]), Escape::None), "Hi friend");
    }

    #[test]
    fn missing_field_without_fallback_is_empty() {
        let t = MergeTemplate::parse("Hi {{first_name}}, {{plan}} plan").unwrap();
        assert_eq!(
            t.render(&fields(&[("plan", "pro")]), Escape::None),
            "Hi , pro plan"
        );
    }

    #[test]
    fn subject_strips_crlf() {
        let t = MergeTemplate::parse("Line\r\none {{first_name}}").unwrap();
        let got = t.render(
            &fields(&[("first_name", "Sam\r\nBcc: x@y")]),
            Escape::Header,
        );
        assert_eq!(got, "Lineone SamBcc: x@y");
        assert!(!got.contains(['\r', '\n']));
    }

    #[test]
    fn values_are_html_escaped() {
        let t = MergeTemplate::parse("<{{a}}> & \"{{b|it's}}\"").unwrap();
        assert_eq!(
            t.render(&fields(&[("a", "<b>x</b>")]), Escape::Html),
            "&lt;&lt;b&gt;x&lt;/b&gt;&gt; &amp; &quot;it&#39;s&quot;"
        );
    }

    #[test]
    fn bad_tags_are_refused() {
        assert_eq!(
            MergeTemplate::parse("ok {{first_name").unwrap_err(),
            MergeError::Unclosed { at: 3 }
        );
        assert_eq!(
            MergeTemplate::parse("{{}}").unwrap_err(),
            MergeError::BadName { at: 0 }
        );
        assert_eq!(
            MergeTemplate::parse("{{ |x}}").unwrap_err(),
            MergeError::BadName { at: 0 }
        );
        assert_eq!(
            MergeTemplate::parse("a {{b{{c}}").unwrap_err(),
            MergeError::BadName { at: 2 }
        );
        // Single braces and a stray closing pair are text.
        let t = MergeTemplate::parse("{a} }} {").unwrap();
        assert_eq!(t.render(&fields(&[]), Escape::None), "{a} }} {");
    }

    #[test]
    fn values_are_not_parsed_again() {
        let t = MergeTemplate::parse("{{first_name}}").unwrap();
        let f = fields(&[("first_name", "{{last_name}}"), ("last_name", "Doe")]);
        assert_eq!(t.render(&f, Escape::None), "{{last_name}}");
    }
}
