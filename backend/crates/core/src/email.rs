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

/// An email: subject, optional preview text, and body blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EmailDocument {
    /// Subject line.
    pub subject: String,
    /// Preview text shown next to the subject in most inboxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preheader: Option<String>,
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
