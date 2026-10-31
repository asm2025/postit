//! Rendering an [`EmailDocument`] to the HTML and plain-text parts of each recipient's mail.

mod footer;
mod merge;
mod mjml;
mod text;

use std::collections::{BTreeMap, HashMap};

use postit_core::{
    DirectRecipient, DirectSendId, EmailBlock, EmailDocument, EmailLanguage, HeadingLevel,
    LinkSigner, MediaAssetId, TEST_SEND_ID,
};
use url::Url;
use uuid::Uuid;

use merge::escape_into;
pub use merge::{Escape, MergeError, MergeTemplate};

/// Who a campaign comes from, as the account's credentials hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    /// The display name in `From`.
    pub from_name: String,
    /// The address in `From`.
    pub from_address: String,
    /// Where replies go, when not to `from_address`.
    pub reply_to: Option<String>,
    /// The sender's postal address, printed in every footer.
    pub postal_address: String,
}

/// One recipient's mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMail {
    /// The subject line, with no CR or LF.
    pub subject: String,
    /// The HTML part.
    pub html: String,
    /// The plain-text part.
    pub text: String,
    /// The recipient's unsubscribe link, for `List-Unsubscribe`.
    pub unsubscribe: Url,
}

/// Why a document cannot be rendered. It names where, never the text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// A merge tag that does not parse, at `path` (such as `email.blocks[2].text`).
    #[error("invalid merge tag at {path}")]
    MergeTag {
        /// Where the tag is.
        path: String,
        /// What is wrong with it.
        #[source]
        error: MergeError,
    },
    /// A merge tag inside a URL, raw or percent-encoded.
    #[error("merge tag in the URL at {path}")]
    MergeTagInUrl {
        /// Where the URL is.
        path: String,
    },
    /// A link whose scheme is not `http`, `https`, `mailto`, or `tel`.
    #[error("link scheme not allowed at {path}")]
    UrlScheme {
        /// Where the URL is.
        path: String,
    },
    /// An image block whose asset has no URL.
    #[error("no URL for the image at {path}")]
    ImageMissing {
        /// Where the image is.
        path: String,
    },
    /// The sender has no postal address for the footer.
    #[error("the sender has no postal address")]
    PostalAddressMissing,
    /// `mrml` refused the generated source.
    #[error("the email could not be compiled")]
    Compile,
}

/// Where one recipient's value goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    /// A merge field's value, or its fallback.
    Merge {
        name: String,
        fallback: Option<String>,
    },
    /// A link: through `click_url` when it is `http` or `https` and the send is tracked.
    Link(Url),
    /// The recipient's unsubscribe link.
    Unsubscribe,
    /// The open pixel, when tracking opens. HTML only.
    Pixel,
}

/// A document checked and parsed, block by block.
pub(crate) struct Parsed {
    pub language: EmailLanguage,
    pub subject: MergeTemplate,
    pub preheader: Option<MergeTemplate>,
    pub blocks: Vec<Block>,
    /// The sender's name, or its address when it has none.
    pub sender_name: String,
    /// The postal address, one trimmed line each, never empty.
    pub postal_lines: Vec<String>,
}

/// One block, with its merge tags parsed and its image's URL found.
pub(crate) enum Block {
    Heading {
        text: MergeTemplate,
        level: HeadingLevel,
    },
    Text(MergeTemplate),
    Image {
        src: Url,
        alt: String,
        link: Option<Url>,
    },
    Button {
        label: MergeTemplate,
        url: Url,
    },
    Divider,
}

fn template(source: &str, path: impl FnOnce() -> String) -> Result<MergeTemplate, RenderError> {
    MergeTemplate::parse(source).map_err(|error| RenderError::MergeTag {
        path: path(),
        error,
    })
}

/// The link schemes a block may use. `javascript:`, `data:`, and the rest are refused.
const LINK_SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];

/// Refuses a merge tag in a URL, and a scheme outside [`LINK_SCHEMES`]. `Url` percent-encodes
/// braces in a path but leaves them raw in a query, so the URL is percent-decoded before it is
/// searched, which also catches mixed forms such as `{%7B`.
fn checked_url(url: &Url, path: impl FnOnce() -> String) -> Result<Url, RenderError> {
    let decoded = urlencoding::decode_binary(url.as_str().as_bytes());
    if decoded.windows(2).any(|pair| pair == b"{{") {
        return Err(RenderError::MergeTagInUrl { path: path() });
    }
    // `Url` lowercases the scheme and drops leading whitespace when it parses.
    if !LINK_SCHEMES.contains(&url.scheme()) {
        return Err(RenderError::UrlScheme { path: path() });
    }
    Ok(url.clone())
}

impl Parsed {
    fn new(
        doc: &EmailDocument,
        images: &HashMap<MediaAssetId, Url>,
        sender: &Sender,
    ) -> Result<Self, RenderError> {
        let postal_lines: Vec<String> = sender
            .postal_address
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        if postal_lines.is_empty() {
            return Err(RenderError::PostalAddressMissing);
        }
        let subject = template(&doc.subject, || "email.subject".to_owned())?;
        let preheader = doc
            .preheader
            .as_deref()
            .map(|p| template(p, || "email.preheader".to_owned()))
            .transpose()?;
        let mut blocks = Vec::with_capacity(doc.blocks.len());
        for (i, block) in doc.blocks.iter().enumerate() {
            let at = |field: &'static str| move || format!("email.blocks[{i}].{field}");
            blocks.push(match block {
                EmailBlock::Heading { text, level } => Block::Heading {
                    text: template(text, at("text"))?,
                    level: *level,
                },
                EmailBlock::Text { text } => Block::Text(template(text, at("text"))?),
                EmailBlock::Image { asset, alt, link } => Block::Image {
                    src: images
                        .get(asset)
                        .cloned()
                        .ok_or_else(|| RenderError::ImageMissing {
                            path: format!("email.blocks[{i}]"),
                        })?,
                    alt: alt.clone(),
                    link: link
                        .as_ref()
                        .map(|l| checked_url(l, at("link")))
                        .transpose()?,
                },
                EmailBlock::Button { label, url } => Block::Button {
                    label: template(label, at("label"))?,
                    url: checked_url(url, at("url"))?,
                },
                EmailBlock::Divider => Block::Divider,
            });
        }
        Ok(Self {
            language: doc.language,
            subject,
            preheader,
            blocks,
            sender_name: match sender.from_name.trim() {
                "" => sender.from_address.trim().to_owned(),
                name => name.to_owned(),
            },
            postal_lines,
        })
    }
}

/// A document compiled once, ready to fill for each recipient.
#[derive(Debug, Clone)]
pub struct Skeleton {
    language: EmailLanguage,
    subject: MergeTemplate,
    html: mjml::Compiled,
    text: Vec<text::Piece>,
}

impl Skeleton {
    /// Compiles `doc`. `images` holds the URL of each image block's asset. `mrml` runs here,
    /// once, never per recipient.
    ///
    /// # Errors
    /// [`RenderError`] for a bad merge tag, a merge tag in a URL, a link scheme other than
    /// `http`, `https`, `mailto`, or `tel`, an image with no URL, a
    /// sender with no postal address, or source `mrml` refuses.
    pub fn compile(
        doc: &EmailDocument,
        images: &HashMap<MediaAssetId, Url>,
        sender: &Sender,
    ) -> Result<Self, RenderError> {
        let parsed = Parsed::new(doc, images, sender)?;
        Ok(Self {
            language: parsed.language,
            html: mjml::compile(&parsed)?,
            text: text::pieces(&parsed),
            subject: parsed.subject,
        })
    }

    /// The mail for `recipient`. A test send ([`TEST_SEND_ID`]) gets no tracked links and no
    /// pixel; its unsubscribe link is the signer's test token. The recipient's locale is not
    /// read: the campaign's language decides the direction and the footer.
    #[must_use]
    pub fn for_recipient(
        &self,
        recipient: &DirectRecipient,
        links: &dyn LinkSigner,
        open_tracking: bool,
    ) -> RenderedMail {
        let unsubscribe = links.unsubscribe_url(recipient, self.language);
        let tracked = recipient.send_id != TEST_SEND_ID;
        let fill = Fill {
            recipient,
            links,
            unsubscribe: &unsubscribe,
            tracked,
            pixel: tracked && open_tracking,
        };
        RenderedMail {
            subject: self.subject.render(&recipient.merge_fields, Escape::Header),
            html: self.html.fill(&fill),
            text: text::fill(&self.text, &fill),
            unsubscribe,
        }
    }

    /// The largest HTML part any recipient can get, which is what `email_too_large` measures:
    /// every link at its signed length under `links`, and merge values empty. The open pixel
    /// is counted whether or not the campaign tracks opens, a deliberate overestimate that
    /// keeps the measure one number per document.
    #[must_use]
    pub fn worst_case_html_bytes(&self, links: &dyn LinkSigner) -> usize {
        // Every real send ID, owner, and address hash has one length in a token.
        let sample = DirectRecipient {
            send_id: DirectSendId::from_uuid(Uuid::max()),
            address: String::new(),
            merge_fields: BTreeMap::new(),
            locale: String::new(),
        };
        self.for_recipient(&sample, links, true).html.len()
    }
}

/// What one recipient's slots are filled with.
pub(crate) struct Fill<'a> {
    recipient: &'a DirectRecipient,
    links: &'a dyn LinkSigner,
    unsubscribe: &'a Url,
    tracked: bool,
    pixel: bool,
}

impl Fill<'_> {
    /// Writes `slot`'s value to `out`. The value goes straight to the output and is never
    /// read again, so a value that looks like a merge tag or a sentinel stays text.
    pub(crate) fn write(&self, slot: &Slot, escape: Escape, out: &mut String) {
        match slot {
            Slot::Merge { name, fallback } => escape_into(
                out,
                merge::value(&self.recipient.merge_fields, name, fallback.as_deref()),
                escape,
            ),
            Slot::Link(url) => {
                // `click_url` hands back other schemes unsigned, so it is never what chooses.
                if self.tracked && matches!(url.scheme(), "http" | "https") {
                    let signed = self.links.click_url(self.recipient.send_id, url);
                    escape_into(out, signed.as_str(), escape);
                } else {
                    escape_into(out, url.as_str(), escape);
                }
            }
            Slot::Unsubscribe => escape_into(out, self.unsubscribe.as_str(), escape),
            Slot::Pixel => {
                if self.pixel {
                    let src = self.links.open_pixel_url(self.recipient.send_id);
                    out.push_str("<img src=\"");
                    escape_into(out, src.as_str(), Escape::Html);
                    out.push_str(
                        "\" width=\"1\" height=\"1\" alt=\"\" \
                         style=\"display:block;border:0;width:1px;height:1px;\" />",
                    );
                }
            }
        }
    }
}
