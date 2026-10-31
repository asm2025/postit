//! The HTML part: MJML source built from the blocks, compiled once by `mrml` with sentinel
//! placeholders where each recipient's values go.
//!
//! Document text is XML-escaped before it enters the source, so no block can add a tag or an
//! `mj-include`. Sentinels start with a random prefix chosen per compile, and a compile whose
//! escaped text happens to hold the prefix is retried with another, so document text never
//! passes for a sentinel.

use std::fmt::Write as _;
use std::ops::Range;

use mrml::prelude::parser::ParserOptions;
use mrml::prelude::parser::noop_loader::NoopIncludeLoader;
use mrml::prelude::render::RenderOptions;
use postit_core::HeadingLevel;

use super::footer::{footer, is_rtl};
use super::merge::{Escape, MergeTemplate, Part, escaped};
use super::{Block, Fill, Parsed, RenderError, Slot};

/// The body's width on a desktop screen. Below `mrml`'s 480px breakpoint, columns take the
/// full width.
const BODY_WIDTH: &str = "600px";
/// Prefix attempts before giving up; a collision on the first is already about 2^-64.
const ATTEMPTS: usize = 4;

/// Compiled HTML with its sentinels found, so each recipient's fill is one pass.
#[derive(Debug, Clone)]
pub(crate) struct Compiled {
    html: String,
    segments: Vec<Segment>,
    slots: Vec<Slot>,
}

#[derive(Debug, Clone)]
enum Segment {
    Text(Range<usize>),
    Slot(usize),
}

impl Compiled {
    /// The HTML part for one recipient: the compiled text left to right, each slot's value
    /// written straight to the output.
    pub(crate) fn fill(&self, fill: &Fill<'_>) -> String {
        let mut out = String::with_capacity(self.html.len() + 1024);
        for segment in &self.segments {
            match segment {
                Segment::Text(range) => out.push_str(&self.html[range.clone()]),
                Segment::Slot(i) => fill.write(&self.slots[*i], Escape::Html, &mut out),
            }
        }
        out
    }
}

/// Compiles `doc` to HTML.
pub(crate) fn compile(doc: &Parsed) -> Result<Compiled, RenderError> {
    compile_with(doc, random_prefix)
}

/// A sentinel prefix nobody can guess. `z` and `k` appear in no entity `escaped` writes.
fn random_prefix() -> String {
    let mut bytes = [0_u8; 8];
    rand::fill(&mut bytes);
    format!("zk{}_", hex::encode(bytes))
}

fn compile_with(doc: &Parsed, mut prefix: impl FnMut() -> String) -> Result<Compiled, RenderError> {
    for _ in 0..ATTEMPTS {
        let mut source = Source::new(prefix());
        source.document(doc);
        if source.buf.matches(&source.prefix).count() != source.slots.len() {
            // The document's own text holds the prefix: try another.
            continue;
        }
        let html = render(&source.buf)?;
        let segments = split(&html, &source.prefix, source.slots.len())?;
        return Ok(Compiled {
            html,
            segments,
            slots: source.slots,
        });
    }
    Err(RenderError::Compile)
}

fn render(source: &str) -> Result<String, RenderError> {
    let options = ParserOptions {
        include_loader: Box::new(NoopIncludeLoader),
    };
    // The errors name document text, so they are dropped.
    let parsed = mrml::parse_with_options(source, &options).map_err(|_| RenderError::Compile)?;
    let render = RenderOptions {
        disable_comments: true,
        ..RenderOptions::default()
    };
    parsed
        .element
        .render(&render)
        .map_err(|_| RenderError::Compile)
}

/// Finds every sentinel in `html`, left to right.
fn split(html: &str, prefix: &str, slots: usize) -> Result<Vec<Segment>, RenderError> {
    let mut segments = Vec::new();
    let mut at = 0;
    while let Some(found) = html[at..].find(prefix) {
        let start = at + found;
        if start > at {
            segments.push(Segment::Text(at..start));
        }
        let digits_at = start + prefix.len();
        let digits = html[digits_at..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let end = digits_at + digits;
        let index: usize = html[digits_at..end]
            .parse()
            .map_err(|_| RenderError::Compile)?;
        if index >= slots || html.as_bytes().get(end) != Some(&b'_') {
            return Err(RenderError::Compile);
        }
        segments.push(Segment::Slot(index));
        at = end + 1;
    }
    if at < html.len() {
        segments.push(Segment::Text(at..html.len()));
    }
    Ok(segments)
}

/// MJML source being written, with the slots its sentinels name.
struct Source {
    buf: String,
    prefix: String,
    slots: Vec<Slot>,
}

impl Source {
    fn new(prefix: String) -> Self {
        Self {
            buf: String::with_capacity(8 * 1024),
            prefix,
            slots: Vec::new(),
        }
    }

    fn raw(&mut self, markup: &str) {
        self.buf.push_str(markup);
    }

    /// Escaped text, each line break a `<br />`.
    fn text(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.buf.push_str("<br />");
            }
            self.buf.push_str(&escaped(
                line.strip_suffix('\r').unwrap_or(line),
                Escape::Html,
            ));
        }
    }

    /// An attribute value, escaped.
    fn attr(&mut self, value: &str) {
        self.buf.push_str(&escaped(value, Escape::Html));
    }

    fn slot(&mut self, slot: Slot) {
        let index = self.slots.len();
        self.slots.push(slot);
        let _ = write!(self.buf, "{}{index}_", self.prefix);
    }

    fn template(&mut self, template: &MergeTemplate) {
        for part in template.parts() {
            match part {
                Part::Literal(text) => self.text(text),
                Part::Field { name, fallback } => self.slot(Slot::Merge {
                    name: name.clone(),
                    fallback: fallback.clone(),
                }),
            }
        }
    }

    fn document(&mut self, doc: &Parsed) {
        let rtl = is_rtl(doc.language);
        let (dir, align, font) = if rtl {
            ("rtl", "right", "Tahoma, Arial, sans-serif")
        } else {
            ("ltr", "left", "Arial, Helvetica, sans-serif")
        };
        self.raw("<mjml lang=\"");
        self.raw(doc.language.as_str());
        self.raw("\" dir=\"");
        self.raw(dir);
        self.raw("\"><mj-head><mj-title>");
        self.template(&doc.subject);
        self.raw("</mj-title>");
        if let Some(preheader) = &doc.preheader {
            self.raw("<mj-preview>");
            self.template(preheader);
            self.raw("</mj-preview>");
        }
        self.raw("<mj-attributes><mj-all font-family=\"");
        self.raw(font);
        self.raw("\" /><mj-text font-size=\"16px\" line-height=\"1.5\" color=\"#222222\" align=\"");
        self.raw(align);
        self.raw(
            "\" /><mj-button background-color=\"#1a73e8\" color=\"#ffffff\" font-size=\"16px\" \
             border-radius=\"4px\" />",
        );
        if rtl {
            self.raw("<mj-section direction=\"rtl\" /><mj-column direction=\"rtl\" />");
        }
        self.raw("</mj-attributes></mj-head>");
        self.raw("<mj-body width=\"");
        self.raw(BODY_WIDTH);
        self.raw(
            "\" background-color=\"#f4f4f4\"><mj-section background-color=\"#ffffff\" \
             padding=\"16px 0\"><mj-column>",
        );
        for block in &doc.blocks {
            self.block(block);
        }
        self.raw("</mj-column></mj-section>");
        self.footer(doc);
        // The open pixel, when on, goes last, outside every table.
        self.raw("<mj-raw>");
        self.slot(Slot::Pixel);
        self.raw("</mj-raw></mj-body></mjml>");
    }

    fn block(&mut self, block: &Block) {
        match block {
            Block::Heading { text, level } => {
                let (tag, size) = match level {
                    HeadingLevel::H1 => ("h1", "28px"),
                    HeadingLevel::H2 => ("h2", "22px"),
                };
                let _ = write!(
                    self.buf,
                    "<mj-text><{tag} style=\"margin:0;font-size:{size};line-height:1.3;\
                     font-weight:bold;\">"
                );
                self.template(text);
                let _ = write!(self.buf, "</{tag}></mj-text>");
            }
            Block::Text(text) => {
                self.raw("<mj-text>");
                self.template(text);
                self.raw("</mj-text>");
            }
            Block::Image { src, alt, link } => {
                self.raw("<mj-image src=\"");
                self.attr(src.as_str());
                self.raw("\" alt=\"");
                self.attr(alt);
                self.raw("\"");
                if let Some(link) = link {
                    self.raw(" href=\"");
                    self.slot(Slot::Link(link.clone()));
                    self.raw("\"");
                }
                self.raw(" />");
            }
            Block::Button { label, url } => {
                self.raw("<mj-button href=\"");
                self.slot(Slot::Link(url.clone()));
                self.raw("\">");
                self.template(label);
                self.raw("</mj-button>");
            }
            Block::Divider => {
                self.raw("<mj-divider border-width=\"1px\" border-color=\"#dddddd\" />");
            }
        }
    }

    /// The footer the renderer adds to every mail: who sent it, their postal address, and the
    /// unsubscribe link.
    fn footer(&mut self, doc: &Parsed) {
        let words = footer(doc.language);
        self.raw(
            "<mj-section padding=\"16px 0\"><mj-column><mj-text font-size=\"12px\" \
             line-height=\"1.5\" color=\"#666666\">",
        );
        self.text(words.before_sender);
        self.text(&doc.sender_name);
        self.text(words.after_sender);
        for line in &doc.postal_lines {
            self.raw("<br />");
            self.text(line);
        }
        self.raw("<br /><a href=\"");
        self.slot(Slot::Unsubscribe);
        self.raw("\" style=\"color:#666666;text-decoration:underline;\">");
        self.text(words.unsubscribe);
        self.raw("</a></mj-text></mj-column></mj-section>");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use postit_agent_kit::testkit::FakeLinkSigner;
    use postit_core::{DirectRecipient, DirectSendId, EmailLanguage};

    use super::*;

    fn parsed(text: &str) -> Parsed {
        Parsed {
            language: EmailLanguage::EnUs,
            subject: MergeTemplate::parse("Hi").unwrap(),
            preheader: None,
            blocks: vec![
                Block::Text(MergeTemplate::parse(text).unwrap()),
                Block::Text(MergeTemplate::parse("{{first_name}}").unwrap()),
            ],
            sender_name: "Shop".to_owned(),
            postal_lines: vec!["1 Main St".to_owned()],
        }
    }

    fn fill(compiled: &Compiled, first_name: &str) -> String {
        let recipient = DirectRecipient {
            send_id: DirectSendId::from_uuid(uuid::Uuid::from_u128(1)),
            address: "a@b.example".to_owned(),
            merge_fields: BTreeMap::from([("first_name".to_owned(), first_name.to_owned())]),
            locale: String::new(),
        };
        let unsubscribe = url::Url::parse("https://u.example/").unwrap();
        compiled.fill(&Fill {
            recipient: &recipient,
            links: &FakeLinkSigner,
            unsubscribe: &unsubscribe,
            tracked: true,
            pixel: false,
        })
    }

    #[test]
    fn document_text_holding_the_prefix_is_retried() {
        // The document's text holds the first prefix tried, sentinel and all.
        let doc = parsed("forged zkaaaa_0_ text");
        let mut tries = vec!["zkbbbb_".to_owned(), "zkaaaa_".to_owned()];
        let compiled = compile_with(&doc, || tries.pop().unwrap()).unwrap();
        let html = fill(&compiled, "Sam");
        assert!(html.contains("forged zkaaaa_0_ text"), "{html}");
        assert!(html.contains(">Sam<"));
        assert!(!html.contains("zkbbbb_"));
    }

    #[test]
    fn a_prefix_that_keeps_colliding_fails_closed() {
        let doc = parsed("zkaaaa_");
        assert!(matches!(
            compile_with(&doc, || "zkaaaa_".to_owned()),
            Err(RenderError::Compile)
        ));
    }

    #[test]
    fn values_holding_a_sentinel_stay_text() {
        let compiled = compile_with(&parsed("x"), || "zkcccc_".to_owned()).unwrap();
        // A value that spells a sentinel is written once and never read again.
        let html = fill(&compiled, "zkcccc_0_");
        assert!(html.contains(">zkcccc_0_<"), "{html}");
    }

    #[test]
    fn random_prefixes_differ() {
        let a = random_prefix();
        assert_eq!(a.len(), 2 + 16 + 1);
        assert_ne!(a, random_prefix());
    }
}
