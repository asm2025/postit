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

/// One block of an email body. Its schema in the API contract is written by hand in the
/// private `schema` module: a `oneOf` over named variants with a `type` discriminator, which
/// client generators need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[cfg(feature = "openapi")]
mod schema {
    //! The [`EmailBlock`] schema. Each variant is a named object whose `type` is its tag, and
    //! the block is a `oneOf` over them with a discriminator mapping. The variant structs only
    //! describe the wire format; a test checks them against `EmailBlock`'s serialization.
    #![allow(dead_code)]

    use url::Url;
    use utoipa::openapi::RefOr;
    use utoipa::openapi::schema::{Discriminator, OneOfBuilder, Ref, Schema};
    use utoipa::{PartialSchema, ToSchema};

    use super::{EmailBlock, HeadingLevel};
    use crate::MediaAssetId;

    /// A heading.
    #[derive(ToSchema)]
    pub(super) struct EmailHeadingBlock {
        #[schema(rename = "type", inline)]
        tag: HeadingTag,
        /// Heading text.
        text: String,
        /// Heading size.
        level: HeadingLevel,
    }

    /// A paragraph of text.
    #[derive(ToSchema)]
    pub(super) struct EmailTextBlock {
        #[schema(rename = "type", inline)]
        tag: TextTag,
        /// Paragraph text.
        text: String,
    }

    /// An image from the media library.
    #[derive(ToSchema)]
    pub(super) struct EmailImageBlock {
        #[schema(rename = "type", inline)]
        tag: ImageTag,
        /// The stored asset.
        asset: MediaAssetId,
        /// Text for readers who cannot see the image.
        alt: String,
        /// Where the image links to, if anywhere.
        link: Option<Url>,
    }

    /// A call-to-action button.
    #[derive(ToSchema)]
    pub(super) struct EmailButtonBlock {
        #[schema(rename = "type", inline)]
        tag: ButtonTag,
        /// Button text.
        label: String,
        /// Where the button goes.
        url: Url,
    }

    /// A horizontal rule.
    #[derive(ToSchema)]
    pub(super) struct EmailDividerBlock {
        #[schema(rename = "type", inline)]
        tag: DividerTag,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum HeadingTag {
        Heading,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum TextTag {
        Text,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum ImageTag {
        Image,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum ButtonTag {
        Button,
    }

    #[derive(ToSchema)]
    #[schema(rename_all = "snake_case")]
    enum DividerTag {
        Divider,
    }

    /// Each tag and the name of its variant's schema.
    fn variants() -> [(&'static str, std::borrow::Cow<'static, str>); 5] {
        [
            ("heading", EmailHeadingBlock::name()),
            ("text", EmailTextBlock::name()),
            ("image", EmailImageBlock::name()),
            ("button", EmailButtonBlock::name()),
            ("divider", EmailDividerBlock::name()),
        ]
    }

    impl PartialSchema for EmailBlock {
        fn schema() -> RefOr<Schema> {
            let mut discriminator = Discriminator::new("type");
            let mut one_of = OneOfBuilder::new().description(Some("One block of an email body."));
            for (tag, name) in variants() {
                discriminator
                    .mapping
                    .insert(tag.to_owned(), format!("#/components/schemas/{name}"));
                one_of = one_of.item(Ref::from_schema_name(name));
            }
            Schema::OneOf(one_of.discriminator(Some(discriminator)).build()).into()
        }
    }

    impl ToSchema for EmailBlock {
        fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
            schemas.push((
                EmailHeadingBlock::name().into(),
                EmailHeadingBlock::schema(),
            ));
            <EmailHeadingBlock as ToSchema>::schemas(schemas);
            schemas.push((EmailTextBlock::name().into(), EmailTextBlock::schema()));
            <EmailTextBlock as ToSchema>::schemas(schemas);
            schemas.push((EmailImageBlock::name().into(), EmailImageBlock::schema()));
            <EmailImageBlock as ToSchema>::schemas(schemas);
            schemas.push((EmailButtonBlock::name().into(), EmailButtonBlock::schema()));
            <EmailButtonBlock as ToSchema>::schemas(schemas);
            schemas.push((
                EmailDividerBlock::name().into(),
                EmailDividerBlock::schema(),
            ));
            <EmailDividerBlock as ToSchema>::schemas(schemas);
        }
    }
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
}

impl EmailLanguage {
    /// The language tag, as on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
        }
    }

    /// Every language postit renders campaigns in.
    pub const ALL: [Self; 1] = [Self::EnUs];

    /// The campaign language for a BCP 47 tag, by its primary language subtag and ignoring case
    /// (`en`, `EN-gb`); `None` when postit renders no campaign in that language.
    #[must_use]
    pub fn for_tag(tag: &str) -> Option<Self> {
        let language = tag.trim().split(['-', '_']).next()?;
        Self::ALL.into_iter().find(|l| {
            l.as_str()
                .split('-')
                .next()
                .is_some_and(|own| own.eq_ignore_ascii_case(language))
        })
    }

    /// Whether text in the language runs right to left. The renderer sets the body's direction
    /// and alignment from it, so a right-to-left language needs only its footer words.
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        match self {
            Self::EnUs => false,
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
            language: EmailLanguage::EnUs,
            blocks: Vec::new(),
        };
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            value,
            json!({"subject": "Hi", "language": "en-US", "blocks": []})
        );
        assert_eq!(serde_json::from_value::<EmailDocument>(value).unwrap(), doc);
        assert_eq!(
            serde_json::to_value(EmailLanguage::EnUs).unwrap(),
            json!("en-US")
        );
        assert_eq!(EmailLanguage::EnUs.as_str(), "en-US");
        assert!(!EmailLanguage::EnUs.is_rtl());
        assert_eq!(EmailLanguage::for_tag("EN-gb"), Some(EmailLanguage::EnUs));
        assert_eq!(EmailLanguage::for_tag("en"), Some(EmailLanguage::EnUs));
        assert_eq!(EmailLanguage::for_tag("fr"), None);
        assert_eq!(EmailLanguage::for_tag(""), None);
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

    /// The block schema is a `oneOf` over named variants with a `type` discriminator (client
    /// generators need one), and each variant's properties are exactly the keys the block
    /// serializes, so the hand-written schema cannot drift from the wire format.
    #[cfg(feature = "openapi")]
    #[test]
    fn email_block_schema_is_discriminated_and_matches_the_wire() {
        use utoipa::{PartialSchema as _, ToSchema as _};

        let schema = serde_json::to_value(EmailBlock::schema()).unwrap();
        let mut components = Vec::new();
        EmailBlock::schemas(&mut components);
        let named: std::collections::BTreeMap<String, serde_json::Value> = components
            .into_iter()
            .map(|(name, schema)| (name, serde_json::to_value(schema).unwrap()))
            .collect();
        assert_eq!(schema["discriminator"]["propertyName"], "type");
        let mapping = schema["discriminator"]["mapping"].as_object().unwrap();
        let samples = [
            EmailBlock::Heading {
                text: "t".to_owned(),
                level: HeadingLevel::H1,
            },
            EmailBlock::Text {
                text: "t".to_owned(),
            },
            EmailBlock::Image {
                asset: MediaAssetId::from_uuid(Uuid::from_u128(1)),
                alt: "a".to_owned(),
                link: Some(Url::parse("https://x.example/").unwrap()),
            },
            EmailBlock::Button {
                label: "l".to_owned(),
                url: Url::parse("https://x.example/").unwrap(),
            },
            EmailBlock::Divider,
        ];
        // A new variant stops this match compiling; its arm and `variants` then ask for a
        // sample above.
        let variants = 5;
        let slot = |block: &EmailBlock| match block {
            EmailBlock::Heading { .. } => 0,
            EmailBlock::Text { .. } => 1,
            EmailBlock::Image { .. } => 2,
            EmailBlock::Button { .. } => 3,
            EmailBlock::Divider => 4,
        };
        let mut covered: Vec<usize> = samples.iter().map(slot).collect();
        covered.sort_unstable();
        assert_eq!(
            covered,
            (0..variants).collect::<Vec<_>>(),
            "one sample per variant"
        );
        assert_eq!(mapping.len(), samples.len());
        assert_eq!(schema["oneOf"].as_array().unwrap().len(), samples.len());
        for sample in samples {
            let wire = serde_json::to_value(&sample).unwrap();
            let tag = wire["type"].as_str().unwrap();
            let reference = mapping[tag].as_str().unwrap();
            let name = reference.trim_start_matches("#/components/schemas/");
            let variant = &named[name];
            let mut properties: Vec<&String> =
                variant["properties"].as_object().unwrap().keys().collect();
            let mut keys: Vec<&String> = wire.as_object().unwrap().keys().collect();
            properties.sort();
            keys.sort();
            assert_eq!(properties, keys, "{name}");
            assert_eq!(
                variant["properties"]["type"]["enum"],
                json!([tag]),
                "{name}"
            );
        }
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
