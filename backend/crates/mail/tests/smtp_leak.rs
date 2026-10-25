//! The SMTP password never reaches a `MailError` or a log line, even when the server
//! refuses authentication. Own test binary: it captures logs.

use postit_config::{MailSettings, MailTransport, SmtpSettings, SmtpTls};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_mail::{Mailer, RenderedMessage, SmtpMailer};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const PASSWORD: &str = "smtp-password-7f3a9c";

/// A minimal SMTP server that offers AUTH and refuses it.
async fn refusing_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .port();
    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let (r, mut w) = stream.into_split();
        let mut lines = BufReader::new(r).lines();
        let _ = w.write_all(b"220 test ESMTP\r\n").await;
        while let Ok(Some(line)) = lines.next_line().await {
            let reply: &[u8] = if line.starts_with("EHLO") {
                b"250-test\r\n250 AUTH PLAIN LOGIN\r\n"
            } else if line.starts_with("AUTH") {
                b"535 5.7.8 authentication failed\r\n"
            } else if line.starts_with("QUIT") {
                b"221 bye\r\n"
            } else {
                b"250 ok\r\n"
            };
            let _ = w.write_all(reply).await;
        }
    });
    port
}

#[tokio::test]
async fn smtp_password_never_leaks() {
    let (logs, _guard) = LogCapture::install();
    let port = refusing_server().await;
    let settings = MailSettings {
        transport: MailTransport::Smtp,
        from_address: "postit <noreply@postit.test>".to_string(),
        send_email_max_attempts: 1,
        smtp: SmtpSettings {
            host: "127.0.0.1".to_string(),
            port,
            username: Some("mailer".to_string()),
            password: Some(PASSWORD.to_string().into()),
            tls: SmtpTls::None,
        },
    };
    let mailer = SmtpMailer::new(&settings).unwrap_or_else(|e| unreachable!("{e}"));
    let err = mailer
        .send(RenderedMessage {
            to: "someone@postit.test".to_string(),
            subject: "s".to_string(),
            text: "t".to_string(),
            html: "<p>t</p>".to_string(),
        })
        .await
        .err()
        .unwrap_or_else(|| unreachable!("the server refuses AUTH"));
    assert_no_leak(
        &[PASSWORD],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?} {mailer:?}")),
            ("logs", &logs.text()),
        ],
    );
}
