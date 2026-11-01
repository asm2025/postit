//! The plain-text part, built straight from the blocks rather than from the HTML.

use super::footer::footer;
use super::merge::{Escape, MergeTemplate, Part};
use super::{Block, Fill, Parsed, Slot};

/// A piece of the plain-text part: fixed text, or a recipient's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Literal(String),
    Slot(Slot),
}

const RULE: &str = "----------";

#[derive(Default)]
struct Builder {
    pieces: Vec<Piece>,
}

impl Builder {
    fn literal(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Piece::Literal(last)) = self.pieces.last_mut() {
            last.push_str(text);
        } else {
            self.pieces.push(Piece::Literal(text.to_owned()));
        }
    }

    fn slot(&mut self, slot: Slot) {
        self.pieces.push(Piece::Slot(slot));
    }

    fn template(&mut self, template: &MergeTemplate) {
        for part in template.parts() {
            match part {
                Part::Literal(text) => self.literal(text),
                Part::Field { name, fallback } => self.slot(Slot::Merge {
                    name: name.clone(),
                    fallback: fallback.clone(),
                }),
            }
        }
    }
}

/// The pieces of the plain-text part: each block in order, a blank line between them, then
/// the footer.
pub(crate) fn pieces(doc: &Parsed) -> Vec<Piece> {
    let mut b = Builder::default();
    for block in &doc.blocks {
        match block {
            // An image with no words and no link has nothing to say in text.
            Block::Image {
                alt, link: None, ..
            } if alt.is_empty() => continue,
            Block::Heading { text, .. } | Block::Text(text) => b.template(text),
            Block::Image { alt, link, .. } => {
                if !alt.is_empty() {
                    b.literal(&format!("[{alt}]"));
                }
                if let Some(link) = link {
                    if !alt.is_empty() {
                        b.literal(" ");
                    }
                    b.slot(Slot::Link(link.clone()));
                }
            }
            Block::Button { label, url } => {
                b.template(label);
                b.literal(": ");
                b.slot(Slot::Link(url.clone()));
            }
            Block::Divider => b.literal(RULE),
        }
        b.literal("\n\n");
    }
    let words = footer(doc.language);
    b.literal(RULE);
    b.literal("\n");
    b.literal(words.before_sender);
    b.literal(&doc.sender_name);
    b.literal(words.after_sender);
    b.literal("\n");
    for line in &doc.postal_lines {
        b.literal(line);
        b.literal("\n");
    }
    b.literal(words.unsubscribe);
    b.literal(": ");
    b.slot(Slot::Unsubscribe);
    b.literal("\n");
    b.pieces
}

/// The plain-text part for one recipient.
pub(crate) fn fill(pieces: &[Piece], fill: &Fill<'_>) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::Slot(slot) => fill.write(slot, Escape::None, &mut out),
        }
    }
    out
}
