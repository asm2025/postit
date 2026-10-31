//! The structured email document: blocks the mail agent renders to HTML.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::MediaAssetId;

/// How large a heading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum HeadingLevel {
    /// Main heading.
    H1,
    /// Section heading.
    H2,
}

/// One block of an email body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmailBlock {
    /// A heading.
    Heading {
        /// Heading text.
        text: String,
        /// Heading size.
        level: HeadingLevel,
    },
    /// A paragraph of text.
    Text {
        /// Paragraph text.
        text: String,
    },
    /// An image from the media library.
    Image {
        /// The stored asset.
        asset: MediaAssetId,
        /// Text for readers who cannot see the image.
        alt: String,
        /// Where the image links to, if anywhere.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<Url>,
    },
    /// A call-to-action button.
    Button {
        /// Button text.
        label: String,
        /// Where the button goes.
        url: Url,
    },
    /// A horizontal rule.
    Divider,
}

/// What happened to a sent email, as a provider or a recipient reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailEventKind {
    /// The recipient's server accepted the message.
    Delivered,
    /// The address bounced for good.
    Bounced,
    /// The recipient reported the message as spam.
    Complained,
    /// The recipient followed a link.
    Clicked,
    /// The recipient opened the message.
    Opened,
    /// The recipient unsubscribed.
    Unsubscribed,
}

impl EmailEventKind {
    /// The value stored in `email_events.kind`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Bounced => "bounced",
            Self::Complained => "complained",
            Self::Clicked => "clicked",
            Self::Opened => "opened",
            Self::Unsubscribed => "unsubscribed",
        }
    }
}

/// The language an email campaign is written in. It sets the body's direction, the footer's
/// language, and the language of the unsubscribe page, whatever each contact's own locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum EmailLanguage {
    /// English (United States), left to right.
    #[serde(rename = "en-US")]
    EnUs,
    /// Arabic, right to left.
    #[serde(rename = "ar")]
    Ar,
}

impl EmailLanguage {
    /// The language tag, as on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::Ar => "ar",
        }
    }
}

/// An email: subject, optional preview text, language, and body blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EmailDocument {
    /// Subject line.
    pub subject: String,
    /// Preview text shown next to the subject in most inboxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preheader: Option<String>,
    /// The campaign's language. Required: a contact's locale is often empty, so it cannot
    /// decide the direction or the footer.
    pub language: EmailLanguage,
    /// Body blocks, top to bottom.
    pub blocks: Vec<EmailBlock>,
}

impl EmailDocument {
    /// The media assets the document embeds, in block order.
    #[must_use]
    pub fn media_ids(&self) -> Vec<MediaAssetId> {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                EmailBlock::Image { asset, .. } => Some(*asset),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn email_block_wire_format() {
        let block = EmailBlock::Button {
            label: "Shop".to_owned(),
            url: Url::parse("https://shop.example/").unwrap(),
        };
        let value = serde_json::to_value(&block).unwrap();
        assert_eq!(
            value,
            json!({"type":"button","label":"Shop","url":"https://shop.example/"})
        );
        assert_eq!(serde_json::from_value::<EmailBlock>(value).unwrap(), block);
        assert_eq!(
            serde_json::to_value(EmailBlock::Divider).unwrap(),
            json!({"type":"divider"})
        );
    }

    #[test]
    fn email_document_requires_language() {
        let doc = EmailDocument {
            subject: "Hi".to_owned(),
            preheader: None,
            language: EmailLanguage::Ar,
            blocks: Vec::new(),
        };
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            value,
            json!({"subject": "Hi", "language": "ar", "blocks": []})
        );
        assert_eq!(serde_json::from_value::<EmailDocument>(value).unwrap(), doc);
        assert_eq!(
            serde_json::to_value(EmailLanguage::EnUs).unwrap(),
            json!("en-US")
        );
        assert_eq!(EmailLanguage::EnUs.as_str(), "en-US");
        assert_eq!(EmailLanguage::Ar.as_str(), "ar");
        // No language, or one postit does not render, is refused.
        assert!(
            serde_json::from_value::<EmailDocument>(json!({"subject": "Hi", "blocks": []}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<EmailDocument>(
                json!({"subject": "Hi", "language": "fr", "blocks": []})
            )
            .is_err()
        );
    }

    #[test]
    fn media_ids_lists_images_in_order() {
        let a = MediaAssetId::from_uuid(Uuid::from_u128(1));
        let b = MediaAssetId::from_uuid(Uuid::from_u128(2));
        let image = |asset| EmailBlock::Image {
            asset,
            alt: String::new(),
            link: None,
        };
        let doc = EmailDocument {
            subject: "Hi".to_owned(),
            preheader: None,
            language: EmailLanguage::EnUs,
            blocks: vec![
                image(b),
                EmailBlock::Divider,
                EmailBlock::Text {
                    text: "x".to_owned(),
                },
                image(a),
            ],
        };
        assert_eq!(doc.media_ids(), vec![b, a]);
    }
}
