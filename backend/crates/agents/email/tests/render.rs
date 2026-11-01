//! Rendering an email document for each recipient.

#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap};

use postit_agent_email::{RenderError, Sender, Skeleton};
use postit_agent_kit::testkit::FakeLinkSigner;
use postit_core::{
    DirectRecipient, DirectSendId, EmailBlock, EmailDocument, EmailLanguage, HeadingLevel,
    LinkSigner, MediaAssetId, TEST_SEND_ID,
};
use url::Url;
use uuid::Uuid;

fn sender() -> Sender {
    Sender {
        from_name: "Corner Bakery".to_owned(),
        from_address: "news@bakery.example".to_owned(),
        reply_to: None,
        postal_address: "12 Main St\nSpringfield, IL 62701".to_owned(),
    }
}

fn image_id() -> MediaAssetId {
    MediaAssetId::from_uuid(Uuid::from_u128(5))
}

fn images() -> HashMap<MediaAssetId, Url> {
    HashMap::from([(
        image_id(),
        Url::parse("https://media.example/m/5?sig=abc&exp=1").unwrap(),
    )])
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn doc(language: EmailLanguage, blocks: Vec<EmailBlock>) -> EmailDocument {
    EmailDocument {
        subject: "Sale for {{first_name|you}}".to_owned(),
        preheader: Some("Fresh bread, {{first_name}}".to_owned()),
        language,
        blocks,
    }
}

fn text(s: &str) -> EmailBlock {
    EmailBlock::Text { text: s.to_owned() }
}

fn button(label: &str, to: &str) -> EmailBlock {
    EmailBlock::Button {
        label: label.to_owned(),
        url: url(to),
    }
}

fn image(link: Option<&str>) -> EmailBlock {
    EmailBlock::Image {
        asset: image_id(),
        alt: "Bread".to_owned(),
        link: link.map(url),
    }
}

/// Every block type, as the snapshots show them.
fn full(language: EmailLanguage) -> EmailDocument {
    let (heading, body, label, hours, open) = match language {
        EmailLanguage::EnUs => (
            "Big news, {{first_name|friend}}",
            "Fresh bread every morning.\nCome early.",
            "Order now",
            "Hours",
            "7 to 3, every day.",
        ),
        EmailLanguage::Ar => (
            "أخبار سارة يا {{first_name|صديقي}}",
            "خبز طازج كل صباح.\nتعال مبكرًا.",
            "اطلب الآن",
            "ساعات العمل",
            "من 7 إلى 3، كل يوم.",
        ),
    };
    let mut d = doc(
        language,
        vec![
            EmailBlock::Heading {
                text: heading.to_owned(),
                level: HeadingLevel::H1,
            },
            text(body),
            image(Some("https://bakery.example/menu")),
            button(label, "https://bakery.example/order?item=1&size=2"),
            EmailBlock::Divider,
            EmailBlock::Heading {
                text: hours.to_owned(),
                level: HeadingLevel::H2,
            },
            text(open),
        ],
    );
    if language == EmailLanguage::Ar {
        "عرض خاص لك يا {{first_name|صديقي}}".clone_into(&mut d.subject);
        d.preheader = Some("خبز طازج يا {{first_name}}".to_owned());
    }
    d
}

fn send_id() -> DirectSendId {
    DirectSendId::from_uuid(Uuid::from_u128(9))
}

fn recipient(fields: &[(&str, &str)]) -> DirectRecipient {
    DirectRecipient {
        send_id: send_id(),
        address: "sam@example.org".to_owned(),
        merge_fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        locale: "en-US".to_owned(),
    }
}

fn compile(doc: &EmailDocument) -> Skeleton {
    Skeleton::compile(doc, &images(), &sender()).unwrap()
}

#[test]
fn subject_and_preheader_take_merge_values() {
    let skel = compile(&doc(EmailLanguage::EnUs, vec![]));
    let mail = skel.for_recipient(&recipient(&[("first_name", "Sam")]), &FakeLinkSigner, false);
    assert_eq!(mail.subject, "Sale for Sam");
    assert!(mail.html.contains("Fresh bread, Sam"), "{}", mail.html);
    let mail = skel.for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    assert_eq!(mail.subject, "Sale for you");
}

#[test]
fn merge_tag_errors_name_the_path() {
    let mut d = doc(
        EmailLanguage::EnUs,
        vec![text("ok"), text("Hi {{first_name")],
    );
    let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
    assert!(
        matches!(&err, RenderError::MergeTag { path, .. } if path == "email.blocks[1].text"),
        "{err:?}"
    );
    d.blocks.clear();
    d.subject = "{{}}".to_owned();
    let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
    assert!(
        matches!(&err, RenderError::MergeTag { path, .. } if path == "email.subject"),
        "{err:?}"
    );
}

#[test]
fn merge_tag_in_url_rejected() {
    // `Url` leaves braces raw in a query.
    let raw = url("https://shop.example/?n={{first_name}}");
    assert!(raw.as_str().contains("{{"));
    // ...and percent-encodes them in a path.
    let encoded = url("https://shop.example/{{first_name}}/x");
    assert!(encoded.as_str().contains("%7B%7B"));
    let lower = url("https://shop.example/%7b%7bfirst_name%7d%7d");
    // One brace raw and one encoded, in a query.
    let mixed = url("https://shop.example/?n={%7Bfirst_name}}");
    assert!(mixed.as_str().contains("{%7B"));
    for (i, bad) in [raw, encoded, lower, mixed].into_iter().enumerate() {
        let d = doc(
            EmailLanguage::EnUs,
            vec![EmailBlock::Button {
                label: "Go".to_owned(),
                url: bad.clone(),
            }],
        );
        let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
        assert_eq!(
            err,
            RenderError::MergeTagInUrl {
                path: "email.blocks[0].url".to_owned()
            },
            "case {i}"
        );
        let d = doc(
            EmailLanguage::EnUs,
            vec![
                text("x"),
                EmailBlock::Image {
                    asset: image_id(),
                    alt: String::new(),
                    link: Some(bad),
                },
            ],
        );
        let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
        assert_eq!(
            err,
            RenderError::MergeTagInUrl {
                path: "email.blocks[1].link".to_owned()
            },
            "case {i}"
        );
    }
}

#[test]
fn link_schemes_refused() {
    let bad = [
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        " JaVaScRiPt:alert(1)",
    ];
    for (i, bad) in bad.into_iter().enumerate() {
        let bad = url(bad);
        let d = doc(
            EmailLanguage::EnUs,
            vec![EmailBlock::Button {
                label: "Go".to_owned(),
                url: bad.clone(),
            }],
        );
        let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
        assert_eq!(
            err,
            RenderError::UrlScheme {
                path: "email.blocks[0].url".to_owned()
            },
            "case {i}"
        );
        // The error names the place, never the URL.
        assert!(!err.to_string().contains("alert"), "case {i}");
        let d = doc(
            EmailLanguage::EnUs,
            vec![
                text("x"),
                EmailBlock::Image {
                    asset: image_id(),
                    alt: String::new(),
                    link: Some(bad),
                },
            ],
        );
        let err = Skeleton::compile(&d, &images(), &sender()).unwrap_err();
        assert_eq!(
            err,
            RenderError::UrlScheme {
                path: "email.blocks[1].link".to_owned()
            },
            "case {i}"
        );
    }
}

/// `mj-title` and `mj-preview` take no markup, so a line break there must not become one.
#[test]
fn subject_strips_crlf() {
    let mut d = doc(EmailLanguage::EnUs, vec![text("x")]);
    d.subject = "Big\r\nsale {{first_name}}\nnow".to_owned();
    d.preheader = Some("Fresh\r\nbread\rtoday".to_owned());
    let mail =
        compile(&d).for_recipient(&recipient(&[("first_name", "Sam")]), &FakeLinkSigner, false);
    assert_eq!(mail.subject, "Bigsale Samnow");
    assert!(
        mail.html.contains("<title>Big sale Sam now</title>"),
        "{}",
        mail.html
    );
    assert!(
        mail.html.contains(">Fresh bread today</div>"),
        "{}",
        mail.html
    );
}

#[test]
fn image_without_url_is_refused() {
    let d = doc(EmailLanguage::EnUs, vec![text("x"), image(None)]);
    let err = Skeleton::compile(&d, &HashMap::new(), &sender()).unwrap_err();
    assert_eq!(
        err,
        RenderError::ImageMissing {
            path: "email.blocks[1]".to_owned()
        }
    );
}

#[test]
fn document_text_is_escaped() {
    let d = doc(
        EmailLanguage::EnUs,
        vec![
            EmailBlock::Heading {
                text: "<i>1 < 2</i>".to_owned(),
                level: HeadingLevel::H2,
            },
            text(r#"a & b <mj-include path="/etc/passwd" /> <b>bold</b>"#),
            button("</mj-button><mj-raw>x</mj-raw>", "https://shop.example/"),
            EmailBlock::Image {
                asset: image_id(),
                alt: r#"x" onerror="y"#.to_owned(),
                link: None,
            },
        ],
    );
    let mail = compile(&d).for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    assert!(
        mail.html.contains("&lt;i&gt;1 &lt; 2&lt;/i&gt;"),
        "{}",
        mail.html
    );
    assert!(
        mail.html
            .contains("a &amp; b &lt;mj-include path=&quot;/etc/passwd&quot; /&gt;"),
        "{}",
        mail.html
    );
    assert!(mail.html.contains("&lt;b&gt;bold&lt;/b&gt;"));
    assert!(mail.html.contains("&lt;/mj-button&gt;&lt;mj-raw&gt;x"));
    assert!(!mail.html.contains("<b>bold"));
    assert!(!mail.html.contains("<mj-"));
    assert!(!mail.html.contains("<i>"));
    assert!(
        mail.html.contains(r#"alt="x&quot; onerror=&quot;y""#),
        "{}",
        mail.html
    );
    assert!(!mail.html.contains("onerror=\"y"));
    // The plain-text part shows the text as written.
    assert!(
        mail.text
            .contains(r#"a & b <mj-include path="/etc/passwd" />"#)
    );
}

/// Links of a fixed, long length, as a real signer's are.
struct LongSigner;

fn long(prefix: &str) -> Url {
    url(&format!("https://t.example/{prefix}/{}", "a".repeat(400)))
}

impl LinkSigner for LongSigner {
    fn unsubscribe_url(&self, _: &DirectRecipient, _: EmailLanguage) -> Url {
        long("u")
    }
    fn click_url(&self, _: DirectSendId, destination: &Url) -> Url {
        if matches!(destination.scheme(), "http" | "https") {
            long("c")
        } else {
            destination.clone()
        }
    }
    fn open_pixel_url(&self, _: DirectSendId) -> Url {
        long("o")
    }
}

#[test]
fn worst_case_bytes_count_signed_links() {
    let skel = compile(&full(EmailLanguage::EnUs));
    let worst = skel.worst_case_html_bytes(&LongSigner);
    // A recipient with no merge values and tracking on gets exactly the worst case.
    let mail = skel.for_recipient(&recipient(&[]), &LongSigner, true);
    assert_eq!(worst, mail.html.len());
    // Two tracked links, the unsubscribe link, and the pixel each count at their signed
    // length: the difference between two signers is exactly the difference of their links.
    // Every send ID has one length, so the fixture's stands in for the sample's.
    let fake = FakeLinkSigner;
    let r = recipient(&[]);
    let pairs = [
        (
            long("c"),
            fake.click_url(send_id(), &url("https://bakery.example/menu")),
        ),
        (
            long("c"),
            fake.click_url(
                send_id(),
                &url("https://bakery.example/order?item=1&size=2"),
            ),
        ),
        (long("u"), fake.unsubscribe_url(&r, EmailLanguage::EnUs)),
        (long("o"), fake.open_pixel_url(send_id())),
    ];
    let extra: usize = pairs
        .iter()
        .map(|(l, s)| l.as_str().len() - s.as_str().len())
        .sum();
    let short = skel.worst_case_html_bytes(&fake);
    assert_eq!(worst, short + extra);
    // Merge values count as empty: a long name makes a mail larger than the worst case.
    let named = skel.for_recipient(
        &recipient(&[("first_name", &"n".repeat(50))]),
        &LongSigner,
        true,
    );
    assert!(named.html.len() > worst);
}

#[test]
fn footer_always_present() {
    let skel = compile(&doc(EmailLanguage::EnUs, vec![]));
    let mail = skel.for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    let unsub = FakeLinkSigner.unsubscribe_url(&recipient(&[]), EmailLanguage::EnUs);
    assert_eq!(mail.unsubscribe, unsub);
    assert!(
        mail.html.contains(&format!("href=\"{unsub}\"")),
        "{}",
        mail.html
    );
    assert!(mail.html.contains("12 Main St<br />Springfield, IL 62701"));
    assert!(mail.html.contains("Unsubscribe"));
    assert!(mail.text.contains("12 Main St\nSpringfield, IL 62701"));
    assert!(mail.text.contains(&format!("Unsubscribe: {unsub}")));

    let mut no_address = sender();
    no_address.postal_address = "  \n ".to_owned();
    let err =
        Skeleton::compile(&doc(EmailLanguage::EnUs, vec![]), &images(), &no_address).unwrap_err();
    assert_eq!(err, RenderError::PostalAddressMissing);
}

#[test]
fn footer_arabic_rtl_for_ar_document() {
    let skel = compile(&full(EmailLanguage::Ar));
    let mail = skel.for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    assert!(mail.html.contains(r#"lang="ar""#), "{}", mail.html);
    assert!(mail.html.contains(r#"dir="rtl""#));
    assert!(mail.html.contains("إلغاء الاشتراك"));
    assert!(!mail.html.contains(">Unsubscribe<"));
    assert!(mail.unsubscribe.as_str().ends_with("lang=ar"));
    assert!(mail.text.contains("إلغاء الاشتراك: "));
}

#[test]
fn recipient_locale_does_not_change_direction() {
    let mut r = recipient(&[]);
    r.locale = "ar".to_owned();
    let mail = compile(&full(EmailLanguage::EnUs)).for_recipient(&r, &FakeLinkSigner, false);
    assert!(mail.html.contains(r#"lang="en-US""#));
    assert!(mail.html.contains(r#"dir="ltr""#));
    assert!(!mail.html.contains(r#"dir="rtl""#));
    assert!(mail.html.contains(">Unsubscribe<"));
    assert!(mail.unsubscribe.as_str().ends_with("lang=en-US"));

    r.locale = "en-US".to_owned();
    let mail = compile(&full(EmailLanguage::Ar)).for_recipient(&r, &FakeLinkSigner, false);
    assert!(mail.html.contains(r#"dir="rtl""#));
}

#[test]
fn plain_text_built_from_blocks() {
    let r = recipient(&[("first_name", "Sam")]);
    let mail = compile(&full(EmailLanguage::EnUs)).for_recipient(&r, &FakeLinkSigner, false);
    let menu = FakeLinkSigner.click_url(send_id(), &url("https://bakery.example/menu"));
    let order = FakeLinkSigner.click_url(
        send_id(),
        &url("https://bakery.example/order?item=1&size=2"),
    );
    let unsub = FakeLinkSigner.unsubscribe_url(&r, EmailLanguage::EnUs);
    let expected = format!(
        "Big news, Sam\n\n\
         Fresh bread every morning.\nCome early.\n\n\
         [Bread] {menu}\n\n\
         Order now: {order}\n\n\
         ----------\n\n\
         Hours\n\n\
         7 to 3, every day.\n\n\
         ----------\n\
         You received this email from Corner Bakery.\n\
         12 Main St\nSpringfield, IL 62701\n\
         Unsubscribe: {unsub}\n"
    );
    assert_eq!(mail.text, expected);
}

#[test]
fn buttons_rewritten_through_click_url() {
    let mail =
        compile(&full(EmailLanguage::EnUs)).for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    let order = FakeLinkSigner.click_url(
        send_id(),
        &url("https://bakery.example/order?item=1&size=2"),
    );
    let menu = FakeLinkSigner.click_url(send_id(), &url("https://bakery.example/menu"));
    assert!(
        mail.html.contains(&format!("href=\"{order}\"")),
        "{}",
        mail.html
    );
    assert!(mail.html.contains(&format!("href=\"{menu}\"")));
    assert!(!mail.html.contains("href=\"https://bakery.example/"));
    // The image itself loads from its media URL, never through tracking.
    assert!(
        mail.html
            .contains("src=\"https://media.example/m/5?sig=abc&amp;exp=1\"")
    );
}

#[test]
fn mailto_not_rewritten() {
    let d = doc(
        EmailLanguage::EnUs,
        vec![
            button("Write to us", "mailto:hello@bakery.example"),
            EmailBlock::Image {
                asset: image_id(),
                alt: "Call".to_owned(),
                link: Some(url("tel:+15550100")),
            },
        ],
    );
    let mail = compile(&d).for_recipient(&recipient(&[]), &FakeLinkSigner, true);
    assert!(
        mail.html.contains("href=\"mailto:hello@bakery.example\""),
        "{}",
        mail.html
    );
    assert!(mail.html.contains("href=\"tel:+15550100\""));
    assert!(!mail.html.contains("links.test/click"));
    assert!(
        mail.text
            .contains("Write to us: mailto:hello@bakery.example")
    );
}

#[test]
fn no_pixel_unless_enabled() {
    let skel = compile(&full(EmailLanguage::EnUs));
    let pixel = FakeLinkSigner.open_pixel_url(send_id());
    let off = skel.for_recipient(&recipient(&[]), &FakeLinkSigner, false);
    assert!(!off.html.contains("links.test/open"));
    let on = skel.for_recipient(&recipient(&[]), &FakeLinkSigner, true);
    assert!(on.html.contains(&format!("src=\"{pixel}\"")), "{}", on.html);
    assert!(!on.text.contains("links.test/open"));
}

#[test]
fn test_send_skips_rewriting() {
    let mut r = recipient(&[("first_name", "Sam")]);
    r.send_id = TEST_SEND_ID;
    let mail = compile(&full(EmailLanguage::EnUs)).for_recipient(&r, &FakeLinkSigner, true);
    assert!(!mail.html.contains("links.test/click"));
    assert!(!mail.html.contains("links.test/open"));
    assert!(!mail.text.contains("links.test/click"));
    assert!(
        mail.html
            .contains("href=\"https://bakery.example/order?item=1&amp;size=2\""),
        "{}",
        mail.html
    );
    // The footer still links to the test token's page.
    let unsub = FakeLinkSigner.unsubscribe_url(&r, EmailLanguage::EnUs);
    assert_eq!(mail.unsubscribe, unsub);
    assert!(mail.html.contains(&format!("href=\"{unsub}\"")));
}

/// Review Focus 2: a contact's own data that looks like markup or a merge tag.
#[test]
fn contact_data_is_inert() {
    let d = doc(
        EmailLanguage::EnUs,
        vec![
            text("Hi {{first_name}} {{last_name|}}"),
            button("For {{first_name}}", "https://shop.example/"),
        ],
    );
    let skel = compile(&d);
    let mail = skel.for_recipient(
        &recipient(&[("first_name", "<b>x</b>")]),
        &FakeLinkSigner,
        false,
    );
    assert!(
        mail.html.contains("Hi &lt;b&gt;x&lt;/b&gt;"),
        "{}",
        mail.html
    );
    assert!(mail.html.contains("For &lt;b&gt;x&lt;/b&gt;"));
    assert!(!mail.html.contains("<b>x</b>"));
    assert_eq!(mail.subject, "Sale for <b>x</b>");
    assert!(mail.text.contains("Hi <b>x</b>"));

    let mail = skel.for_recipient(
        &recipient(&[("first_name", "{{last_name}}"), ("last_name", "Doe")]),
        &FakeLinkSigner,
        false,
    );
    assert!(mail.html.contains("Hi {{last_name}} Doe"), "{}", mail.html);
    assert!(mail.html.contains("For {{last_name}}"));
    assert_eq!(mail.subject, "Sale for {{last_name}}");
    assert!(mail.text.contains("Hi {{last_name}} Doe"));
}

/// One HTML document serves both widths: a 600px container on desktop, and full-width
/// columns under the 480px breakpoint on mobile. The snapshots pin both, per direction.
fn assert_responsive(html: &str) {
    // Desktop: the body is capped at 600px.
    assert!(html.contains("max-width:600px"), "{html}");
    // Mobile: columns are full width until the breakpoint.
    assert!(html.contains("@media only screen and (min-width:480px)"));
    assert!(html.contains("mj-column-per-100"));
}

#[test]
fn snapshot_desktop_and_mobile_ltr() {
    let r = recipient(&[("first_name", "Sam")]);
    let mail = compile(&full(EmailLanguage::EnUs)).for_recipient(&r, &FakeLinkSigner, true);
    assert_responsive(&mail.html);
    insta::assert_snapshot!("en_us_html", mail.html);
    insta::assert_snapshot!("en_us_text", mail.text);
}

#[test]
fn snapshot_desktop_and_mobile_rtl() {
    let r = recipient(&[("first_name", "سام")]);
    let mail = compile(&full(EmailLanguage::Ar)).for_recipient(&r, &FakeLinkSigner, true);
    assert_responsive(&mail.html);
    insta::assert_snapshot!("ar_html", mail.html);
    insta::assert_snapshot!("ar_text", mail.text);
}
