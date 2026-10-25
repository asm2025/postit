use postit_i18n::{Args, Locale, t};
use postit_mail::{MailContent, PendingUser, RenderedMessage, render};
use url::Url;

fn app_url() -> Url {
    Url::parse("https://app.postit.test").unwrap_or_else(|e| unreachable!("url: {e}"))
}

#[test]
fn pending_approval_lists_users_links_to_the_pending_filter_and_escapes_html() {
    let content = MailContent::UserPendingApproval {
        pending: vec![
            PendingUser {
                display_name: "Ada <script>".into(),
                email: Some("ada@example.com".into()),
            },
            PendingUser {
                display_name: "Bob".into(),
                email: None,
            },
        ],
        more: 3,
    };
    let rendered =
        render(&content, &app_url(), Locale::EnUs).unwrap_or_else(|e| unreachable!("render: {e}"));

    assert!(rendered.subject.contains('5'), "{}", rendered.subject);
    assert!(rendered.text.contains("Ada <script> <ada@example.com>"));
    assert!(rendered.text.contains("- Bob\n"));
    assert!(rendered.text.contains("and 3 more"));
    assert!(
        rendered
            .text
            .contains("https://app.postit.test/users?status=pending")
    );
    // askama 0.16's html escaper emits numeric character references (`&#60;`/`&#62;`)
    // rather than named entities (`&lt;`/`&gt;`); both are correctly escaped HTML.
    assert!(
        rendered.html.contains("Ada &#60;script&#62;"),
        "{}",
        rendered.html
    );
    assert!(!rendered.html.contains("<script>"));
}

#[test]
fn approved_greets_the_user_and_links_to_the_app() {
    let content = MailContent::UserApproved {
        display_name: "Ada".into(),
    };
    let rendered =
        render(&content, &app_url(), Locale::EnUs).unwrap_or_else(|e| unreachable!("render: {e}"));
    assert!(rendered.text.contains("Ada"));
    assert!(rendered.text.contains("https://app.postit.test/"));
    assert!(rendered.html.contains("https://app.postit.test/"));
}

#[test]
fn rendered_message_debug_hides_address_and_body() {
    let message = RenderedMessage {
        to: "secret.person@example.com".into(),
        subject: "Your postit account was approved".into(),
        text: "body text".into(),
        html: "<p>body html</p>".into(),
    };
    let debug = format!("{message:?}");
    assert!(!debug.contains("secret.person"));
    assert!(!debug.contains("body"));
    assert!(debug.contains("example.com"));
}

fn approved_content() -> MailContent {
    MailContent::UserApproved {
        display_name: "Ada".into(),
    }
}

fn pending_content() -> MailContent {
    MailContent::UserPendingApproval {
        pending: vec![
            PendingUser {
                display_name: "Ada".into(),
                email: Some("ada@example.com".into()),
            },
            PendingUser {
                display_name: "Bob".into(),
                email: None,
            },
        ],
        more: 3,
    }
}

fn rendered(content: &MailContent, locale: Locale) -> postit_mail::Rendered {
    render(content, &app_url(), locale).unwrap_or_else(|e| unreachable!("render: {e}"))
}

#[test]
fn approved_uses_the_shared_layout() {
    for locale in [Locale::EnUs, Locale::Ar] {
        let r = rendered(&approved_content(), locale);
        assert!(r.html.contains("class=\"main-card\""), "{}", r.html);
        assert!(
            r.html
                .contains("<bdi dir=\"ltr\">post<span style=\"color:#df528a\">it</span></bdi>"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains(&format!("<title>{}</title>", r.subject)),
            "{}",
            r.html
        );
        assert!(!r.html.contains("class=\"footer\""), "{}", r.html);
    }
}

#[test]
fn pending_footer_is_the_messages_own() {
    for locale in [Locale::EnUs, Locale::Ar] {
        let r = rendered(&pending_content(), locale);
        let footer = t(locale, "pending-footer", &Args::new());
        assert!(r.html.contains("class=\"footer\""), "{}", r.html);
        assert!(r.html.contains(&footer), "{}", r.html);
        assert!(!r.html.contains("confirm your email address"));
    }
}

#[test]
fn approval_text_parts_are_unchanged() {
    let r = rendered(&approved_content(), Locale::EnUs);
    assert_eq!(
        r.text,
        "Hi Ada,\n\nAn administrator approved your postit account.\nSign in now: https://app.postit.test/"
    );
    let r = rendered(&approved_content(), Locale::Ar);
    assert_eq!(
        r.text,
        "مرحب\u{64b}ا Ada،\n\nوافق أحد المسؤولين على حسابك في postit.\nسج\u{651}ل الدخول الآن: https://app.postit.test/"
    );
    let r = rendered(&pending_content(), Locale::EnUs);
    assert_eq!(
        r.text,
        "These accounts are waiting for approval on postit:\n\n- Ada <ada@example.com>\n- Bob\n...and 3 more.\n\nReview them: https://app.postit.test/users?status=pending\n\nYou are receiving this email because you are a postit administrator."
    );
    let r = rendered(&pending_content(), Locale::Ar);
    assert_eq!(
        r.text,
        "هذه الحسابات تنتظر الموافقة في postit:\n\n- Ada <ada@example.com>\n- Bob\n...و٣ أخرى.\n\nمراجعتها: https://app.postit.test/users?status=pending\n\nوصلتك هذه الرسالة لأنك مسؤول في postit."
    );
}

#[test]
fn layout_mirrors_in_arabic() {
    for content in [approved_content(), pending_content()] {
        let r = rendered(&content, Locale::Ar);
        assert!(r.html.contains("dir=\"rtl\""), "{}", r.html);
        assert!(r.html.contains("padding-inline"), "{}", r.html);
        assert!(r.html.contains("text-align: start"), "{}", r.html);
        for physical in ["text-align: left", "padding-left", "padding-right"] {
            assert!(!r.html.contains(physical), "{physical} in {}", r.html);
        }
    }
}

#[test]
fn approval_html_snapshots() {
    insta::assert_snapshot!(
        "approved_en_us",
        rendered(&approved_content(), Locale::EnUs).html
    );
    insta::assert_snapshot!(
        "approved_ar",
        rendered(&approved_content(), Locale::Ar).html
    );
    insta::assert_snapshot!(
        "pending_en_us",
        rendered(&pending_content(), Locale::EnUs).html
    );
    insta::assert_snapshot!("pending_ar", rendered(&pending_content(), Locale::Ar).html);
}
