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
    let rendered = render(&content, &app_url()).unwrap_or_else(|e| unreachable!("render: {e}"));

    assert!(rendered.subject.contains('5'), "{}", rendered.subject);
    assert!(rendered.text.contains("Ada <script> <ada@example.com>"));
    assert!(rendered.text.contains("- Bob\n"));
    assert!(rendered.text.contains("and 3 more"));
    assert!(
        rendered
            .text
            .contains("https://app.postit.test/admin/users?status=pending")
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
    let rendered = render(&content, &app_url()).unwrap_or_else(|e| unreachable!("render: {e}"));
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
