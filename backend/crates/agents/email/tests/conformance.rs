//! The conformance suite in direct mode: the email agent against the in-process SMTP sink.
//!
//! A live `publish` is not how the email agent delivers (campaigns go through `send_batch`), so
//! the suite's live-publish checks report skipped; `direct` sends a real batch to the sink, and
//! `redaction` checks that neither the SMTP password nor any recipient address, which the sink
//! echoes in its refusals, shows in an output or the log.
//!
//! The suite holds a thread-local log capture, so these tests use `#[tokio::test]`'s default
//! current-thread runtime and live in their own test binary.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_agent_email::testkit::{Script, Sink};
use postit_agent_email::{EmailAgent, EmailAgentConfig};
use postit_agent_kit::testkit::TestContext;
use postit_agent_kit::testkit::conformance::{
    ConformanceFixture, CredentialSamples, Fault, Faults, run_conformance,
};
use postit_core::{
    AccountId, AccountRef, CredentialFields, Credentials, DirectRecipient, DirectSendId,
    EmailBlock, EmailDocument, EmailLanguage, PlatformId, PostKind, Variant,
};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use uuid::Uuid;

const FROM: &str = "news@bakery.example";
const USER: &str = "mailer";
const PASSWORD: &str = "smtp-conformance-pass-7e2a";
const SENT: &str = "ana@reader.example";
const BOUNCED: &str = "gone@reader.example";
const SOFT: &str = "busy@reader.example";

/// The sink's normal behavior: a login, one recipient that does not exist, one whose mailbox
/// is busy.
fn base_script() -> Script {
    Script {
        login: Some((USER.to_owned(), PASSWORD.to_owned())),
        rcpt: vec![
            (BOUNCED.to_owned(), "550 5.1.1".to_owned()),
            (SOFT.to_owned(), "450 4.2.1".to_owned()),
        ],
        ..Script::default()
    }
}

/// The sink as the suite's fake platform. A connection is a remote call and a `MAIL` a
/// mutating one. Each fault is scripted as its nearest SMTP analogue; the suite arms none of
/// them for an agent that delivers only through `send_batch`.
struct SinkFaults {
    sink: Sink,
}

#[async_trait]
impl Faults for SinkFaults {
    async fn arm(&self, fault: Fault) {
        self.sink.script(|script| match fault {
            Fault::Unauthorized => script.auth_reply = Some("535 5.7.8 refused".to_owned()),
            Fault::RateLimited(_) => {
                script.mail_refusal = Some((0, "451 4.7.1 slow down".to_owned()));
            }
            Fault::ServerError => {
                script.mail_refusal = Some((0, "451 4.3.0 local error".to_owned()));
            }
            Fault::AcceptedThenLost => {
                script.drop_in_data = vec![SENT.to_owned(), BOUNCED.to_owned(), SOFT.to_owned()];
            }
            Fault::LostBeforeAccept => script.stall_mail = true,
            Fault::Malformed => script.auth_reply = Some("not an SMTP reply".to_owned()),
        });
    }

    async fn clear(&self) {
        self.sink.script(|script| *script = base_script());
    }

    fn remote_calls(&self) -> usize {
        self.sink.connections()
    }

    fn mutating_calls(&self) -> usize {
        self.sink
            .commands()
            .iter()
            .filter(|(_, verb)| verb == "MAIL")
            .count()
    }
}

fn fields(sink: &Sink, password: &str) -> CredentialFields {
    let mut plain = Map::new();
    plain.insert("transport".into(), json!("smtp"));
    plain.insert("from_name".into(), json!("Corner Bakery"));
    plain.insert("from_address".into(), json!(FROM));
    plain.insert("postal_address".into(), json!("12 Main St, Springfield"));
    plain.insert("host".into(), json!("127.0.0.1"));
    plain.insert("port".into(), json!(sink.port()));
    plain.insert("tls".into(), json!("none"));
    plain.insert("username".into(), json!(USER));
    CredentialFields::from_parts(
        plain,
        BTreeMap::from([(
            "password".to_owned(),
            SecretString::from(password.to_owned()),
        )]),
    )
}

fn account() -> AccountRef {
    AccountRef {
        platform: PlatformId::new("email").unwrap(),
        account: AccountId::from_uuid(Uuid::now_v7()),
        remote_id: FROM.to_owned(),
    }
}

fn sample(account: &AccountRef) -> Variant {
    Variant {
        target: account.clone(),
        kind: PostKind::Post,
        text: String::new(),
        media: Vec::new(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: Some(EmailDocument {
            subject: "Fresh bread, {{first_name|friend}}".to_owned(),
            preheader: Some("Baked this morning".to_owned()),
            language: EmailLanguage::EnUs,
            blocks: vec![
                EmailBlock::Text {
                    text: "Hello {{first_name|there}}, come early.".to_owned(),
                },
                EmailBlock::Button {
                    label: "Order".to_owned(),
                    url: "https://bakery.example/order".parse().unwrap(),
                },
            ],
        }),
        options: Map::new(),
    }
}

fn recipient(address: &str) -> DirectRecipient {
    DirectRecipient {
        send_id: DirectSendId::from_uuid(Uuid::now_v7()),
        address: address.to_owned(),
        merge_fields: BTreeMap::from([("first_name".to_owned(), "Ada".to_owned())]),
        locale: "en".to_owned(),
    }
}

async fn fixture() -> (ConformanceFixture, Sink) {
    let sink = Sink::start(base_script()).await;
    let agent = EmailAgent::new(EmailAgentConfig {
        allow_private: true,
        image_url_ttl: Duration::from_secs(3600),
        command_timeout: Duration::from_secs(5),
        hello_name: Some("postit.test".to_owned()),
    });
    let test =
        Arc::new(TestContext::new().with_credentials(Credentials::Fields(fields(&sink, PASSWORD))));
    let account = account();
    let context = {
        let test = Arc::clone(&test);
        Arc::new(move || test.agent_context(Value::Null))
    };
    let fixture = ConformanceFixture {
        agent: Arc::new(agent),
        context,
        check_context: test.check_context(Value::Null),
        samples: vec![sample(&account)],
        account,
        assets: Vec::new(),
        credentials: Some(CredentialSamples {
            valid: fields(&sink, PASSWORD),
            invalid: fields(&sink, "not-the-password"),
        }),
        recipients: [SENT, BOUNCED, SOFT].map(recipient).into(),
        secrets: vec![
            PASSWORD.to_owned(),
            SENT.to_owned(),
            BOUNCED.to_owned(),
            SOFT.to_owned(),
        ],
        faults: Arc::new(SinkFaults { sink: sink.clone() }),
        deduplicates: false,
    };
    (fixture, sink)
}

#[tokio::test]
async fn email_agent_passes_in_direct_mode() {
    let (fixture, sink) = fixture().await;
    let report = run_conformance(fixture).await;
    report.assert_passed();
    assert_eq!(
        report.skipped,
        [
            "error_mapping",
            "idempotency",
            "cancellation",
            "oauth_config_fields"
        ]
    );
    // `direct` reached the sink: one mail accepted, the other two refused at `RCPT`.
    let received = sink.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].to, [SENT]);
}
