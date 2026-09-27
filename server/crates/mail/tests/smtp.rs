use postit_config::{MailSettings, MailTransport, RedactedSecret, SmtpSettings, SmtpTls};
use postit_mail::{Mailer, MemoryMailer, RenderedMessage, SmtpMailer};

fn settings(password: Option<&str>) -> MailSettings {
    MailSettings {
        transport: MailTransport::Smtp,
        from_address: "postit <noreply@postit.test>".into(),
        send_email_max_attempts: 8,
        smtp: SmtpSettings {
            host: std::env::var("POSTIT_SMTP_HOST").unwrap_or_else(|_| "localhost".into()),
            port: std::env::var("POSTIT_SMTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(25),
            username: password.map(|_| "mailer".to_string()),
            password: password.map(|p| RedactedSecret::from(p.to_string())),
            tls: SmtpTls::None,
        },
    }
}

#[test]
fn smtp_mailer_debug_never_shows_the_password() {
    let mailer = SmtpMailer::new(&settings(Some("hunter2-smtp")))
        .unwrap_or_else(|e| unreachable!("build: {e}"));
    let debug = format!("{mailer:?}");
    assert!(!debug.contains("hunter2-smtp"));
}

#[test]
fn an_invalid_from_address_is_a_config_error() {
    let mut bad = settings(None);
    bad.from_address = "not an address".into();
    assert!(SmtpMailer::new(&bad).is_err());
}

#[tokio::test]
async fn memory_mailer_records_messages() {
    let mailer = MemoryMailer::default();
    mailer
        .send(RenderedMessage {
            to: "a@b.test".into(),
            subject: "s".into(),
            text: "t".into(),
            html: "h".into(),
        })
        .await
        .unwrap_or_else(|e| unreachable!("send: {e}"));
    assert_eq!(mailer.sent().len(), 1);
    assert_eq!(mailer.sent()[0].to, "a@b.test");
}

/// Opt-in: `POSTIT_SMTP_TESTS=1 cargo test -p postit-mail --test smtp -- --nocapture`
/// with a local SMTP tool (e.g. Papercut) on `POSTIT_SMTP_HOST`:`POSTIT_SMTP_PORT`
/// (default `localhost:25`). Check the tool's inbox for both messages.
#[tokio::test]
async fn sends_both_templates_to_the_local_smtp_tool() {
    if std::env::var("POSTIT_SMTP_TESTS").as_deref() != Ok("1") {
        return;
    }
    let mailer = SmtpMailer::new(&settings(None)).unwrap_or_else(|e| unreachable!("build: {e}"));
    let app =
        url::Url::parse("https://postit.local:44315").unwrap_or_else(|e| unreachable!("url: {e}"));
    for content in [
        postit_mail::MailContent::UserPendingApproval {
            pending: vec![postit_mail::PendingUser {
                display_name: "Member".into(),
                email: Some("member@postit.com".into()),
            }],
            more: 0,
        },
        postit_mail::MailContent::UserApproved {
            display_name: "Member".into(),
        },
    ] {
        let rendered =
            postit_mail::render(&content, &app).unwrap_or_else(|e| unreachable!("render: {e}"));
        mailer
            .send(RenderedMessage {
                to: "admin@postit.com".into(),
                subject: rendered.subject,
                text: rendered.text,
                html: rendered.html,
            })
            .await
            .unwrap_or_else(|e| unreachable!("send: {e}"));
    }
}
