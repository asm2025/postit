use postit_i18n::Locale;
use postit_mail::{IdentityMessage, Rendered, identity_locale, render_identity};
use serde_json::{Value, json};

const CODE: &str = include_str!("fixtures/zitadel/verify_email_code.json");
const LINK: &str = include_str!("fixtures/zitadel/password_reset_link.json");
const URL: &str = "https://postit.local:44300/ui/login/mail/verification?userID=1&code=ABCDEF";

fn fixture(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|e| unreachable!("fixture: {e}"))
}

fn message(value: &Value) -> IdentityMessage {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| unreachable!("message: {e}"))
}

fn render(value: &Value) -> Rendered {
    render_identity(&message(value)).unwrap_or_else(|e| unreachable!("render: {e}"))
}

/// A link-only message: the code removed from the code fixture.
fn link_only() -> Value {
    let mut v = fixture(CODE);
    v["args"]["code"] = Value::Null;
    v
}

/// A code-only message: the URL removed from the code fixture.
fn code_only() -> Value {
    let mut v = fixture(CODE);
    v["templateData"]["url"] = Value::Null;
    v
}

fn arabic(mut v: Value) -> Value {
    v["templateData"]["subject"] = json!("تأكيد البريد الإلكتروني");
    v["templateData"]["title"] = json!("تأكيد البريد الإلكتروني");
    v["templateData"]["greeting"] = json!("مرحبًا آدا،");
    v["templateData"]["text"] = json!("تمت إضافة بريد إلكتروني جديد. استخدم الزر أدناه لتأكيده.");
    v["templateData"]["buttonText"] = json!("تأكيد البريد");
    v["templateData"]["preHeader"] = json!("تأكيد البريد الإلكتروني");
    v
}

fn unknown_type() -> Value {
    let mut v = link_only();
    v["contextInfo"]["eventType"] = json!("user.human.something.new");
    v
}

#[test]
fn parses_the_captured_code_payload() {
    let msg = message(&fixture(CODE));
    assert_eq!(msg.context_info.event_type, "user.human.email.code.added");
    assert_eq!(msg.context_info.recipient_email_address, "ada@example.test");
    let debug = format!("{msg:?}");
    for secret in ["123456", "ABCDEF", "ada@example.test", "postit.local"] {
        assert!(!debug.contains(secret), "{debug}");
    }
}

#[test]
fn parses_the_captured_link_payload() {
    let msg = message(&fixture(LINK));
    assert_eq!(
        msg.context_info.event_type,
        "user.human.password.code.added"
    );
    assert_eq!(msg.context_info.recipient_email_address, "ada@example.test");
    let debug = format!("{msg:?}");
    for secret in ["123456", "ABCDEF", "ada@example.test", "postit.local"] {
        assert!(!debug.contains(secret), "{debug}");
    }
}

#[test]
fn link_variant_has_button_and_fallback() {
    let out = render(&link_only());
    assert_eq!(out.html.matches("class=\"btn\"").count(), 1, "{}", out.html);
    let href = URL.replace('&', "&#38;"); // askama's numeric reference, decoded by browsers
    assert!(
        out.html.contains(&format!("href=\"{href}\"")),
        "{}",
        out.html
    );
    assert!(out.html.contains("If the button does not work"));
    assert!(!out.html.contains("code-box\" dir"), "{}", out.html);
    assert!(
        out.text.contains(&format!("Verify email: {URL}")),
        "{}",
        out.text
    );
}

#[test]
fn code_variant_has_code_box_only() {
    let out = render(&code_only());
    assert!(
        out.html.contains(
            "<p style=\"margin-block:20px 0\">Your code is:</p>\n<div class=\"code-box\" dir=\"ltr\">123456</div>"
        ),
        "{}",
        out.html
    );
    assert!(!out.html.contains("class=\"btn\""), "{}", out.html);
    assert!(!out.html.contains("If the button does not work"));
    assert!(out.text.contains("Your code is:\n123456\n"), "{}", out.text);
}

#[test]
fn arabic_code_label_precedes_the_code_box() {
    let out = render(&arabic(code_only()));
    assert!(
        out.html.contains(
            "<p style=\"margin-block:20px 0\">رمزك هو:</p>\n<div class=\"code-box\" dir=\"ltr\">123456</div>"
        ),
        "{}",
        out.html
    );
    assert!(out.text.contains("رمزك هو:\n123456\n"), "{}", out.text);
    assert!(!out.html.contains("Your code is:"), "{}", out.html);
}

#[test]
fn link_only_message_has_no_code_label() {
    let out = render(&link_only());
    assert!(!out.html.contains("Your code is:"), "{}", out.html);
    assert!(!out.text.contains("Your code is:"), "{}", out.text);
}

#[test]
fn both_variant_has_both() {
    let out = render(&fixture(CODE));
    assert!(out.html.contains("class=\"btn\""));
    assert!(
        out.html
            .contains("<div class=\"code-box\" dir=\"ltr\">123456</div>")
    );
    assert!(out.text.contains(URL));
    assert!(out.text.lines().any(|l| l == "123456"));
}

#[test]
fn unknown_event_type_renders_generically() {
    let out = render(&unknown_type());
    assert_eq!(out.subject, "Verify email");
    assert!(out.html.contains("<h2"), "{}", out.html);
    assert!(out.html.contains("Zitadel - Verify email"));
    assert!(out.html.contains("class=\"btn\""));
    assert!(out.html.contains("A new email has been added."));
}

#[test]
fn javascript_url_is_dropped() {
    let mut v = fixture(CODE);
    v["templateData"]["url"] = json!("javascript:alert(1)");
    let out = render(&v);
    assert!(!out.html.contains("href"), "{}", out.html);
    assert!(!out.html.contains("javascript"), "{}", out.html);
    assert!(!out.text.contains("javascript"), "{}", out.text);
    assert!(!out.html.contains("class=\"btn\""));
}

#[test]
fn markup_in_texts_is_escaped() {
    let mut v = fixture(CODE);
    v["templateData"]["greeting"] = json!("Hi <b>Ada</b>");
    v["templateData"]["text"] = json!("line <script>alert(1)</script>");
    let out = render(&v);
    assert!(
        out.html.contains("Hi &#60;b&#62;Ada&#60;/b&#62;"),
        "{}",
        out.html
    );
    assert!(out.html.contains("&#60;script&#62;"), "{}", out.html);
    assert!(!out.html.contains("<script"), "{}", out.html);
    assert!(!out.html.contains("<b>"), "{}", out.html);
}

#[test]
fn arabic_by_script() {
    let ar = message(&arabic(fixture(CODE)));
    assert_eq!(identity_locale(&ar), Locale::Ar);
    let out = render_identity(&ar).unwrap_or_else(|e| unreachable!("render: {e}"));
    assert!(out.html.contains("dir=\"rtl\""));
    assert!(out.html.contains("lang=\"ar\""));
    let en = message(&fixture(CODE));
    assert_eq!(identity_locale(&en), Locale::EnUs);
    assert!(render(&fixture(CODE)).html.contains("dir=\"ltr\""));
}

#[test]
fn bare_translation_key_footer_is_dropped() {
    let out = render(&fixture(CODE));
    assert!(!out.html.contains("VerifyEmail.Footer"), "{}", out.html);
    assert!(!out.html.contains("class=\"footer\""), "{}", out.html);
    assert!(!out.text.contains("VerifyEmail.Footer"));
}

#[test]
fn real_footer_text_is_kept() {
    let mut v = fixture(CODE);
    v["templateData"]["footerText"] = json!("Sent by postit.\nIgnore if unexpected.");
    let out = render(&v);
    assert!(out.html.contains("class=\"footer\""), "{}", out.html);
    assert!(out.html.contains("Ignore if unexpected."));
    assert!(out.text.contains("Sent by postit."));
}

#[test]
fn subject_strips_newlines() {
    let mut v = fixture(CODE);
    v["templateData"]["subject"] = json!("a\r\nBcc: x");
    assert_eq!(render(&v).subject, "aBcc: x");
}

#[test]
fn subject_falls_back_to_title_then_postit() {
    let mut v = fixture(CODE);
    v["templateData"]["subject"] = Value::Null;
    assert_eq!(render(&v).subject, "Zitadel - Verify email");
    v["templateData"]["title"] = Value::Null;
    assert_eq!(render(&v).subject, "postit");
}

#[test]
fn snapshots() {
    insta::assert_snapshot!("identity_link_en", render(&link_only()).html);
    insta::assert_snapshot!("identity_code_en", render(&code_only()).html);
    insta::assert_snapshot!("identity_both_en", render(&fixture(CODE)).html);
    insta::assert_snapshot!("identity_link_ar", render(&arabic(link_only())).html);
    insta::assert_snapshot!("identity_code_ar", render(&arabic(code_only())).html);
    insta::assert_snapshot!("identity_both_ar", render(&arabic(fixture(CODE))).html);
    insta::assert_snapshot!("identity_unknown_en", render(&unknown_type()).html);
}

#[test]
fn arabic_snapshots_carry_the_arabic_pre_header() {
    let out = render(&arabic(fixture(CODE)));
    assert!(
        out.html.contains(">تأكيد البريد الإلكتروني</div>"),
        "{}",
        out.html
    );
}

#[test]
fn plain_text_output_is_pinned() {
    assert_eq!(
        render(&fixture(CODE)).text,
        format!(
            "Zitadel - Verify email

Hello Ada Capture,

A new email has been added. Please use the button below to verify your email. (Code 123456) If you didn't add a new email, please ignore this email.

Verify email: {URL}

Your code is:
123456
"
        )
    );
}

#[test]
fn non_web_and_scheme_less_urls_get_no_button() {
    for bad in [
        "ftp://example.test/file",
        "data:text/html,<p>x</p>",
        "postit.local/verify?code=1",
        "//postit.local/verify",
    ] {
        let mut v = link_only();
        v["templateData"]["url"] = json!(bad);
        let out = render(&v);
        assert!(!out.html.contains("class=\"btn\""), "{bad}: {}", out.html);
        assert!(!out.html.contains("href"), "{bad}: {}", out.html);
        assert!(!out.text.contains(bad), "{bad}: {}", out.text);
    }
}

#[test]
fn title_only_message_renders() {
    let v = json!({
        "contextInfo": {
            "eventType": "user.human.something.new",
            "recipientEmailAddress": "ada@example.test"
        },
        "templateData": { "title": "Just a title" }
    });
    let out = render(&v);
    assert_eq!(out.subject, "Just a title");
    assert!(out.html.contains("<h2"), "{}", out.html);
    assert!(out.html.contains("Just a title"));
    assert!(!out.html.contains("class=\"btn\""), "{}", out.html);
    assert!(!out.html.contains("code-box\" dir"), "{}", out.html);
    assert_eq!(out.text.trim(), "Just a title");
}

#[test]
fn pre_header_and_multi_paragraph_footer_render_in_the_layout() {
    let mut v = fixture(CODE);
    v["templateData"]["footerText"] = json!(
        "First line.
Second line."
    );
    let out = render(&v);
    assert!(
        out.html.contains(
            "<div style=\"display:none;max-height:0;overflow:hidden\">Verify email</div>"
        ),
        "{}",
        out.html
    );
    assert_eq!(
        out.html.matches("<p style=\"margin-block:0 8px\">").count(),
        2
    );
    assert!(out.html.contains(">First line.</p>"));
    assert!(out.html.contains(">Second line.</p>"));
}
