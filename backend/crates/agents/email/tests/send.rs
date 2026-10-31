//! Sending campaigns over SMTP against the in-process sink.

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod support;

use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use postit_agent_email::{
    EmailAgent, EmailAgentConfig, EmailTransport, OutgoingMail, Resolve, SendFailure, SmtpServer,
    SmtpTransport, SystemResolve, TlsMode, TransportError,
};
use postit_agent_kit::testkit::TestContext;
use postit_core::{
    AccountId, AccountRef, Agent, AgentClient, AgentError, CredentialFields, Credentials,
    DirectRecipient, DirectSendId, DirectSendOutcome, DirectSendResult, EmailBlock, EmailDocument,
    EmailLanguage, FieldKind, PlatformId, PostKind, RetryClass, TEST_SEND_ID, Variant,
    ViolationCode,
};
use postit_http::testkit::StubLookup;
use postit_http::{AddressPolicy, HttpError};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use support::sink::{Script, Sink, closed_port};
use uuid::Uuid;

const FROM: &str = "news@bakery.example";
const USER: &str = "mailer";
const PASSWORD: &str = "smtp-password-5c1d";

/// A resolver over a [`StubLookup`] that counts its lookups.
struct Counting {
    stub: StubLookup,
    calls: AtomicUsize,
    ports: std::sync::Mutex<Vec<u16>>,
}

impl Counting {
    fn new(addrs: Vec<SocketAddr>) -> Arc<Self> {
        Arc::new(Self {
            stub: StubLookup::new(addrs),
            calls: AtomicUsize::new(0),
            ports: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn ports(&self) -> Vec<u16> {
        self.ports.lock().unwrap().clone()
    }
}

#[async_trait]
impl Resolve for Counting {
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        policy: AddressPolicy,
    ) -> Result<Vec<SocketAddr>, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.ports.lock().unwrap().push(port);
        self.stub.resolve_checked(host, port, policy).await
    }
}

fn config(allow_private: bool, command_timeout: Duration) -> EmailAgentConfig {
    EmailAgentConfig {
        allow_private,
        image_url_ttl: Duration::from_secs(3600),
        command_timeout,
        hello_name: None,
    }
}

/// The agent with private hosts allowed and a 5-second command timeout.
fn agent() -> EmailAgent {
    EmailAgent::new(config(true, Duration::from_secs(5)))
}

/// Credential fields for the sink at `host`:`port`, changed by `edit`.
fn fields_with(
    host: &str,
    port: u16,
    edit: impl FnOnce(&mut Map<String, Value>),
) -> CredentialFields {
    let mut plain = Map::new();
    plain.insert("transport".into(), json!("smtp"));
    plain.insert("from_name".into(), json!("Corner Bakery"));
    plain.insert("from_address".into(), json!(FROM));
    plain.insert(
        "postal_address".into(),
        json!("12 Main St\nSpringfield, IL 62701"),
    );
    plain.insert("host".into(), json!(host));
    plain.insert("port".into(), json!(port));
    plain.insert("tls".into(), json!("none"));
    edit(&mut plain);
    CredentialFields::from_parts(plain, BTreeMap::new())
}

fn fields(sink: &Sink) -> CredentialFields {
    fields_with("127.0.0.1", sink.port(), |_| {})
}

/// The sink's fields with a login.
fn login_fields(sink: &Sink, password: &str) -> CredentialFields {
    let (mut plain, _) = fields(sink).split();
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
        account: AccountId::from_uuid(Uuid::from_u128(1)),
        remote_id: FROM.to_owned(),
    }
}

fn doc() -> EmailDocument {
    EmailDocument {
        subject: "Fresh bread, {{first_name|friend}}".to_owned(),
        preheader: None,
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
    }
}

fn variant(doc: EmailDocument) -> Variant {
    Variant {
        target: account(),
        kind: PostKind::Post,
        text: String::new(),
        media: Vec::new(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: Some(doc),
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

fn recipients(addresses: &[&str]) -> Vec<DirectRecipient> {
    addresses.iter().map(|a| recipient(a)).collect()
}

fn client_for(agent: &EmailAgent, fields: CredentialFields) -> Box<dyn AgentClient> {
    let ctx = TestContext::new().with_credentials(Credentials::Fields(fields));
    agent.create_client(ctx.agent_context(json!({}))).unwrap()
}

/// Sends `to` through `agent` with `fields`, bounded so a hang fails the test.
async fn send_with(
    agent: &EmailAgent,
    fields: CredentialFields,
    to: &[DirectRecipient],
) -> Result<Vec<DirectSendResult>, AgentError> {
    let client = client_for(agent, fields);
    let direct = client.direct().unwrap();
    tokio::time::timeout(
        Duration::from_secs(60),
        direct.send_batch(&account(), to, &variant(doc()), &[]),
    )
    .await
    .unwrap()
}

async fn send(sink: &Sink, to: &[DirectRecipient]) -> Result<Vec<DirectSendResult>, AgentError> {
    send_with(&agent(), fields(sink), to).await
}

fn outcome_of(results: &[DirectSendResult], r: &DirectRecipient) -> DirectSendOutcome {
    let found: Vec<_> = results.iter().filter(|x| x.send_id == r.send_id).collect();
    assert_eq!(found.len(), 1, "one result per recipient");
    found[0].outcome.clone()
}

fn is_sent(outcome: &DirectSendOutcome) -> bool {
    matches!(outcome, DirectSendOutcome::Sent { .. })
}

fn is_soft_failure(outcome: &DirectSendOutcome) -> bool {
    matches!(
        outcome,
        DirectSendOutcome::Failed {
            retryable: true,
            ..
        }
    )
}

// ---- What arrives ----

#[tokio::test]
async fn multipart_html_and_text() {
    let sink = Sink::start(Script::default()).await;
    let to = recipients(&["ada@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    let received = sink.received();
    assert_eq!(received.len(), 1);
    let mail = &received[0];
    assert_eq!(mail.from, FROM);
    assert_eq!(mail.to, vec!["ada@example.com".to_owned()]);
    assert!(
        mail.header("Content-Type")
            .unwrap()
            .starts_with("multipart/alternative"),
        "{}",
        mail.data
    );
    assert!(
        mail.data.contains("Content-Type: text/plain"),
        "{}",
        mail.data
    );
    assert!(
        mail.data.contains("Content-Type: text/html"),
        "{}",
        mail.data
    );
    assert_eq!(mail.header("Subject").unwrap(), "Fresh bread, Ada");
    assert_eq!(
        mail.header("From").unwrap(),
        "\"Corner Bakery\" <news@bakery.example>"
    );
    assert_eq!(mail.header("To").unwrap(), "ada@example.com");
}

#[tokio::test]
async fn unsubscribe_headers_present() {
    let sink = Sink::start(Script::default()).await;
    let to = recipients(&["ada@example.com"]);
    send(&sink, &to).await.unwrap();
    let mail = &sink.received()[0];
    assert_eq!(
        mail.header("List-Unsubscribe").unwrap(),
        format!(
            "<https://links.test/unsubscribe/{}?lang=en-US>",
            to[0].send_id
        )
    );
    assert_eq!(
        mail.header("List-Unsubscribe-Post").unwrap(),
        "List-Unsubscribe=One-Click"
    );
}

#[tokio::test]
async fn reply_to_set() {
    let sink = Sink::start(Script::default()).await;
    let fields = fields_with("127.0.0.1", sink.port(), |plain| {
        plain.insert("reply_to".into(), json!("orders@bakery.example"));
    });
    send_with(&agent(), fields, &recipients(&["ada@example.com"]))
        .await
        .unwrap();
    assert_eq!(
        sink.received()[0].header("Reply-To").unwrap(),
        "orders@bakery.example"
    );

    // Without one, there is no Reply-To.
    let other = Sink::start(Script::default()).await;
    send(&other, &recipients(&["ada@example.com"]))
        .await
        .unwrap();
    assert_eq!(other.received()[0].header("Reply-To"), None);
}

#[tokio::test]
async fn message_id_from_send_id() {
    let sink = Sink::start(Script::default()).await;
    let to = recipients(&["ada@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    let id = format!("{}@bakery.example", to[0].send_id);
    assert_eq!(
        sink.received()[0].header("Message-ID").unwrap(),
        format!("<{id}>")
    );
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Sent {
            remote_id: Some(id)
        }
    );
}

#[tokio::test]
async fn test_send_has_no_unsubscribe_headers() {
    let sink = Sink::start(Script::default()).await;
    let mut test = recipient("ada@example.com");
    test.send_id = TEST_SEND_ID;
    send(&sink, &[test]).await.unwrap();
    let mail = &sink.received()[0];
    assert_eq!(mail.header("List-Unsubscribe"), None);
    assert_eq!(mail.header("List-Unsubscribe-Post"), None);
    assert!(
        !mail
            .header("Message-ID")
            .unwrap()
            .contains(&TEST_SEND_ID.to_string())
    );
}

#[tokio::test]
async fn test_sends_get_distinct_message_ids() {
    let sink = Sink::start(Script::default()).await;
    let mut ids = HashSet::new();
    for _ in 0..2 {
        let mut test = recipient("ada@example.com");
        test.send_id = TEST_SEND_ID;
        let results = send(&sink, std::slice::from_ref(&test)).await.unwrap();
        let DirectSendOutcome::Sent { remote_id } = outcome_of(&results, &test) else {
            unreachable!("the test send is sent");
        };
        ids.insert(remote_id.unwrap());
    }
    assert_eq!(ids.len(), 2);
    let headers: HashSet<_> = sink
        .received()
        .iter()
        .map(|m| m.header("Message-ID").unwrap())
        .collect();
    assert_eq!(headers.len(), 2);
}

// ---- Per-recipient results ----

#[tokio::test]
async fn permanent_rcpt_is_bounced() {
    let sink = Sink::start(Script {
        rcpt: vec![("gone@example.com".into(), "550 5.1.1".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["gone@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Bounced {
            permanent: true,
            reason: "smtp_5_1_1".into()
        }
    );
}

#[tokio::test]
async fn temporary_rcpt_is_soft() {
    let sink = Sink::start(Script {
        rcpt: vec![("busy@example.com".into(), "450 4.2.0".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["busy@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Bounced {
            permanent: false,
            reason: "smtp_4_2_0".into()
        }
    );
}

#[tokio::test]
async fn policy_rcpt_is_rejected_without_suppressing() {
    let sink = Sink::start(Script {
        rcpt: vec![("ada@example.com".into(), "550 5.7.1".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["ada@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Failed {
            retryable: false,
            reason: "smtp_5_7_1".into()
        }
    );
}

#[tokio::test]
async fn three_leading_5_7_is_authentication_error() {
    let refused = ["a@example.com", "b@example.com", "c@example.com"];
    let sink = Sink::start(Script {
        rcpt: refused
            .iter()
            .map(|a| ((*a).to_owned(), "550 5.7.1".to_owned()))
            .collect(),
        ..Script::default()
    })
    .await;
    let to = recipients(&[refused[0], refused[1], refused[2], "d@example.com"]);
    let err = send(&sink, &to).await.unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);
    // The batch stopped at the third: the fourth was never offered.
    let rcpts = sink
        .commands()
        .into_iter()
        .filter(|(_, v)| v == "RCPT")
        .count();
    assert_eq!(rcpts, 3);
}

#[tokio::test]
async fn rejection_5_7_after_a_success_stays_per_recipient() {
    let refused = ["b@example.com", "c@example.com", "d@example.com"];
    let sink = Sink::start(Script {
        rcpt: refused
            .iter()
            .map(|a| ((*a).to_owned(), "550 5.7.1".to_owned()))
            .collect(),
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", refused[0], refused[1], refused[2]]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    for r in &to[1..] {
        assert_eq!(
            outcome_of(&results, r),
            DirectSendOutcome::Failed {
                retryable: false,
                reason: "smtp_5_7_1".into()
            }
        );
    }
}

#[tokio::test]
async fn two_leading_5_7_stay_per_recipient() {
    let sink = Sink::start(Script {
        rcpt: vec![
            ("a@example.com".into(), "550 5.7.1".into()),
            ("b@example.com".into(), "550 5.7.1".into()),
        ],
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "b@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    for r in &to {
        assert!(matches!(
            outcome_of(&results, r),
            DirectSendOutcome::Failed {
                retryable: false,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn data_refusal_is_rejected() {
    let sink = Sink::start(Script {
        data: vec![("a@example.com".into(), "554 5.6.0".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "b@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Failed {
            retryable: false,
            reason: "smtp_5_6_0".into()
        }
    );
    assert!(is_sent(&outcome_of(&results, &to[1])));
}

#[tokio::test]
async fn invalid_address_fails_alone() {
    let sink = Sink::start(Script::default()).await;
    let to = recipients(&["not an address", "b@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Failed {
            retryable: false,
            reason: "address_invalid".into()
        }
    );
    assert!(is_sent(&outcome_of(&results, &to[1])));
}

#[tokio::test]
async fn reason_codes_hold_no_server_text() {
    let addresses = [
        "gone@example.com",
        "busy@example.com",
        "policy@example.com",
        "late@example.com",
        "fine@example.com",
    ];
    let sink = Sink::start(Script {
        rcpt: vec![
            ("gone@example.com".into(), "550 5.1.1".into()),
            ("busy@example.com".into(), "451".into()),
            ("policy@example.com".into(), "554".into()),
        ],
        data: vec![("late@example.com".into(), "552 5.3.4".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&addresses);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(results.len(), to.len());
    let mut reasons = Vec::new();
    for result in &results {
        match &result.outcome {
            DirectSendOutcome::Bounced { reason, .. }
            | DirectSendOutcome::Failed { reason, .. } => {
                reasons.push(reason.clone());
            }
            DirectSendOutcome::Sent { remote_id } => {
                let id = remote_id.as_deref().unwrap();
                assert!(id.ends_with("@bakery.example"), "{id}");
            }
        }
    }
    assert_eq!(reasons.len(), 4);
    for reason in &reasons {
        assert!(
            reason.starts_with("smtp_")
                && reason[5..].bytes().all(|b| b.is_ascii_digit() || b == b'_'),
            "{reason}"
        );
        for address in addresses {
            assert!(!reason.contains(address), "{reason}");
        }
    }

    // A refused sender: the error names no address either.
    let refusing = Sink::start(Script {
        mail_refusal: Some((0, "553 5.1.8".into())),
        ..Script::default()
    })
    .await;
    let err = send(&refusing, &recipients(&["fine@example.com"]))
        .await
        .unwrap_err();
    let shown = format!("{err} {err:?}");
    assert!(!shown.contains('@'), "{shown}");
}

// ---- The session ----

#[tokio::test]
async fn refused_recipient_keeps_the_session() {
    let sink = Sink::start(Script {
        rcpt: vec![("gone@example.com".into(), "550 5.1.1".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "gone@example.com", "c@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    assert!(matches!(
        outcome_of(&results, &to[1]),
        DirectSendOutcome::Bounced {
            permanent: true,
            ..
        }
    ));
    assert!(is_sent(&outcome_of(&results, &to[2])));
    assert_eq!(sink.connections(), 1);
    let received = sink.received();
    assert_eq!(received.len(), 2);
    assert!(received.iter().all(|m| m.connection == 1));
    // The refused RCPT was followed by RSET before the next MAIL.
    let verbs: Vec<String> = sink.commands().into_iter().map(|(_, v)| v).collect();
    let refused_at = verbs
        .iter()
        .enumerate()
        .filter(|(_, v)| *v == "RCPT")
        .nth(1)
        .map(|(i, _)| i)
        .unwrap();
    assert_eq!(verbs[refused_at + 1], "RSET");
    assert_eq!(verbs[refused_at + 2], "MAIL");
}

#[tokio::test]
async fn mail_refusal_suppresses_nobody() {
    let sink = Sink::start(Script {
        mail_refusal: Some((0, "553 5.1.8".into())),
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "b@example.com"]);
    let err = send(&sink, &to).await.unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    // One MAIL, and no recipient was ever offered.
    let verbs: Vec<String> = sink.commands().into_iter().map(|(_, v)| v).collect();
    assert_eq!(verbs.iter().filter(|v| *v == "MAIL").count(), 1);
    assert!(
        !verbs.iter().any(|v| v == "RCPT" || v == "DATA"),
        "{verbs:?}"
    );
}

#[tokio::test]
async fn mail_refusal_after_a_success_is_soft() {
    let sink = Sink::start(Script {
        mail_refusal: Some((1, "550 5.7.1".into())),
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "b@example.com", "c@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    for r in &to[1..] {
        let outcome = outcome_of(&results, r);
        assert!(is_soft_failure(&outcome), "{outcome:?}");
    }
    // Nothing after the refusal was offered.
    let mails = sink
        .commands()
        .into_iter()
        .filter(|(_, v)| v == "MAIL")
        .count();
    assert_eq!(mails, 2);
}

#[tokio::test]
async fn connection_drop_mid_batch() {
    let sink = Sink::start(Script {
        drop_after: Some((2, 1)),
        ..Script::default()
    })
    .await;
    let to = recipients(&[
        "a@example.com",
        "b@example.com",
        "c@example.com",
        "d@example.com",
        "e@example.com",
    ]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    assert!(is_sent(&outcome_of(&results, &to[1])));
    // The third's MAIL met the closed connection, so it was never in flight: it goes out on
    // the reopened session with the rest.
    for r in &to[2..] {
        assert!(is_sent(&outcome_of(&results, r)));
    }
    assert_eq!(sink.connections(), 2);
    let received = sink.received();
    assert_eq!(received.len(), 5);
    assert!(received[..2].iter().all(|m| m.connection == 1));
    assert!(received[2..].iter().all(|m| m.connection == 2));
    // Nothing accepted was sent again.
    for r in &to {
        let copies = received
            .iter()
            .filter(|m| m.to == vec![r.address.clone()])
            .count();
        assert_eq!(copies, 1, "{}", r.address);
    }
}

#[tokio::test]
async fn drop_after_the_body_is_final() {
    let sink = Sink::start(Script {
        drop_in_data: vec!["b@example.com".into()],
        ..Script::default()
    })
    .await;
    let to = recipients(&["a@example.com", "b@example.com", "c@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    // The server may have taken the message before the connection broke: a soft failure,
    // never sent again in this batch.
    assert_eq!(
        outcome_of(&results, &to[1]),
        DirectSendOutcome::Failed {
            retryable: true,
            reason: "smtp_connection".into()
        }
    );
    let offered = sink
        .rcpts()
        .into_iter()
        .filter(|(_, a)| a == "b@example.com")
        .count();
    assert_eq!(offered, 1);
    assert!(is_sent(&outcome_of(&results, &to[2])));
    assert_eq!(sink.connections(), 2);
    assert_eq!(sink.received().last().unwrap().connection, 2);
}

#[tokio::test]
async fn closing_reply_before_the_body_is_retried_once() {
    let sink = Sink::start(Script {
        rcpt: vec![("busy@example.com".into(), "421 4.3.2".into())],
        ..Script::default()
    })
    .await;
    let to = recipients(&["busy@example.com", "b@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    // Offered on the first session and once more on a reopened one, then a soft failure.
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Failed {
            retryable: true,
            reason: "smtp_421".into()
        }
    );
    let offered: Vec<usize> = sink
        .rcpts()
        .into_iter()
        .filter(|(_, a)| a == "busy@example.com")
        .map(|(c, _)| c)
        .collect();
    assert_eq!(offered, vec![1, 2]);
    assert!(is_sent(&outcome_of(&results, &to[1])));
    assert_eq!(sink.connections(), 3);
}

#[tokio::test]
async fn connection_keeps_dropping() {
    let sink = Sink::start(Script {
        drop_after: Some((1, usize::MAX)),
        ..Script::default()
    })
    .await;
    let to = recipients(&[
        "a@example.com",
        "b@example.com",
        "c@example.com",
        "d@example.com",
        "e@example.com",
        "f@example.com",
        "g@example.com",
    ]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(results.len(), to.len());
    for (i, r) in to.iter().enumerate() {
        let outcome = outcome_of(&results, r);
        // Each drop is met by the next MAIL, before any body, so that recipient goes out on
        // the reopened session; once both reopens are spent, the rest fail softly.
        if i < 3 {
            assert!(is_sent(&outcome), "{i}: {outcome:?}");
        } else {
            assert!(is_soft_failure(&outcome), "{i}: {outcome:?}");
        }
    }
    // The first session and two reopens.
    assert_eq!(sink.connections(), 3);
    assert_eq!(sink.received().len(), 3);
}

#[tokio::test]
async fn auth_failure_is_authentication_error() {
    let sink = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        ..Script::default()
    })
    .await;
    let err = send_with(
        &agent(),
        login_fields(&sink, "wrong-password"),
        &recipients(&["a@example.com"]),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    assert!(!format!("{err} {err:?}").contains("wrong-password"));

    // The right password logs in and sends.
    let results = send_with(
        &agent(),
        login_fields(&sink, PASSWORD),
        &recipients(&["a@example.com"]),
    )
    .await
    .unwrap();
    assert!(is_sent(&results[0].outcome));
    assert_eq!(sink.auths(), 1);
}

#[tokio::test]
async fn connection_refused_is_retryable() {
    let port = closed_port().await;
    let err = send_with(
        &agent(),
        fields_with("127.0.0.1", port, |_| {}),
        &recipients(&["a@example.com"]),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AgentError::Transport { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::Retryable);
}

#[tokio::test]
async fn stalled_server_times_out() {
    let quick = EmailAgent::new(config(true, Duration::from_millis(500)));

    // Before anything is sent, a server that never greets fails the batch, retryably.
    let silent = Sink::start(Script {
        stall_greeting: true,
        ..Script::default()
    })
    .await;
    let err = send_with(&quick, fields(&silent), &recipients(&["a@example.com"]))
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Transport { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::Retryable);

    // A server that stops answering mid-session: each command times out, and the batch
    // ends with soft failures instead of hanging.
    let stalling = Sink::start(Script {
        stall_mail: true,
        ..Script::default()
    })
    .await;
    let to = recipients(&[
        "a@example.com",
        "b@example.com",
        "c@example.com",
        "d@example.com",
    ]);
    let started = std::time::Instant::now();
    let results = send_with(&quick, fields(&stalling), &to).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    for r in &to {
        assert!(is_soft_failure(&outcome_of(&results, r)));
    }
}

// ---- Addresses and TLS ----

#[tokio::test]
async fn private_host_refused() {
    let sink = Sink::start(Script::default()).await;
    let strict = EmailAgent::new(config(false, Duration::from_secs(5)));
    let fields = fields_with("127.0.0.1", sink.port(), |plain| {
        plain.insert("tls".into(), json!("tls"));
    });
    let err = send_with(&strict, fields, &recipients(&["a@example.com"]))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AgentError::InvalidCredentials {
            field: Some("host".into())
        }
    );
    assert_eq!(sink.connections(), 0);
}

#[tokio::test]
async fn private_host_allowed_with_flag() {
    let sink = Sink::start(Script::default()).await;
    let results = send(&sink, &recipients(&["a@example.com"])).await.unwrap();
    assert!(is_sent(&results[0].outcome));
    assert_eq!(sink.connections(), 1);
}

#[tokio::test]
async fn connects_to_checked_address() {
    let sink = Sink::start(Script::default()).await;
    let lookup = Counting::new(vec![sink.addr()]);
    let agent = agent().with_resolver(lookup.clone());
    // The name does not resolve in real DNS: only the checked address can reach the sink.
    let fields = fields_with("rebind.invalid", 2525, |_| {});
    let to = recipients(&["a@example.com", "b@example.com", "c@example.com"]);
    let results = send_with(&agent, fields, &to).await.unwrap();
    assert!(results.iter().all(|r| is_sent(&r.outcome)));
    assert_eq!(lookup.calls(), 1);
    assert_eq!(sink.connections(), 1);
}

#[tokio::test]
async fn second_address_used_when_the_first_refuses() {
    let sink = Sink::start(Script::default()).await;
    let closed: SocketAddr = format!("127.0.0.1:{}", closed_port().await)
        .parse()
        .unwrap();
    let lookup = Counting::new(vec![closed, sink.addr()]);
    let agent = agent().with_resolver(lookup.clone());
    let fields = fields_with("smtp.invalid", 2525, |_| {});
    let results = send_with(&agent, fields, &recipients(&["a@example.com"]))
        .await
        .unwrap();
    assert!(is_sent(&results[0].outcome));
    assert_eq!(sink.connections(), 1);
}

#[tokio::test]
async fn starttls_missing_is_an_error() {
    let sink = Sink::start(Script::default()).await;
    let fields = fields_with("127.0.0.1", sink.port(), |plain| {
        plain.insert("tls".into(), json!("starttls"));
    });
    let err = send_with(&agent(), fields, &recipients(&["a@example.com"]))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AgentError::InvalidCredentials {
            field: Some("tls".into())
        }
    );
    // It never fell back to sending in the clear.
    assert!(sink.received().is_empty());
    assert!(!sink.commands().iter().any(|(_, v)| v == "MAIL"));
}

#[tokio::test]
async fn tls_none_refused_without_allow_private() {
    let sink = Sink::start(Script::default()).await;
    // A public address, so only the TLS rule can refuse it.
    let public: SocketAddr = "203.0.113.25:25".parse().unwrap();
    let lookup = Counting::new(vec![public]);
    let strict =
        EmailAgent::new(config(false, Duration::from_secs(5))).with_resolver(lookup.clone());
    let fields = fields_with("smtp.invalid", 25, |_| {});
    let err = send_with(&strict, fields, &recipients(&["a@example.com"]))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AgentError::InvalidCredentials {
            field: Some("tls".into())
        }
    );
    assert_eq!(lookup.calls(), 0);
    assert_eq!(sink.connections(), 0);
}

// ---- validate and verify_credentials ----

async fn violations(fields: CredentialFields, doc: EmailDocument) -> Vec<postit_core::Violation> {
    let client = client_for(&agent(), fields);
    client.validate(&variant(doc), &[]).await.unwrap()
}

#[tokio::test]
async fn validate_accepts_a_good_campaign() {
    let sink = Sink::start(Script::default()).await;
    assert_eq!(violations(fields(&sink), doc()).await, Vec::new());
    // validate sends nothing.
    assert_eq!(sink.connections(), 0);
}

#[tokio::test]
async fn validate_requires_postal_address() {
    for postal in [None, Some("  \n ")] {
        let fields = fields_with("127.0.0.1", 25, |plain| match postal {
            None => {
                plain.remove("postal_address");
            }
            Some(text) => {
                plain.insert("postal_address".into(), json!(text));
            }
        });
        let found = violations(fields, doc()).await;
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].code, ViolationCode::EmailPostalAddressRequired);
    }
}

#[tokio::test]
async fn validate_refuses_oversized_html() {
    let mut big = doc();
    big.blocks.push(EmailBlock::Text {
        text: "a".repeat(110 * 1024),
    });
    let found = violations(fields_with("127.0.0.1", 25, |_| {}), big).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, ViolationCode::EmailTooLarge);
    assert_eq!(found[0].params["max_bytes"], json!(100 * 1024));
    assert!(found[0].params["actual_bytes"].as_u64().unwrap() > 110 * 1024);
}

#[tokio::test]
async fn validate_flags_bad_merge_tags() {
    let mut bad = doc();
    bad.blocks.push(EmailBlock::Text {
        text: "Hello {{first_name".to_owned(),
    });
    let found = violations(fields_with("127.0.0.1", 25, |_| {}), bad).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, ViolationCode::EmailMergeTagInvalid);
    assert_eq!(found[0].path.as_deref(), Some("email.blocks[2].text"));
}

#[tokio::test]
async fn verify_credentials_authenticates() {
    let sink = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        ..Script::default()
    })
    .await;
    let agent = agent();
    let ctx = TestContext::new().check_context(json!({}));
    let found = agent
        .verify_credentials(&ctx, &login_fields(&sink, PASSWORD))
        .await
        .unwrap();
    assert_eq!(found.accounts.len(), 1);
    assert_eq!(found.accounts[0].remote_id, FROM);
    assert_eq!(found.accounts[0].display_name, "Corner Bakery");
    assert_eq!(sink.auths(), 1);

    let err = agent
        .verify_credentials(&ctx, &login_fields(&sink, "wrong-password"))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AgentError::InvalidCredentials {
            field: Some("password".into())
        }
    );
    assert_eq!(sink.auths(), 1);
    // Verifying sends no mail.
    assert!(sink.received().is_empty());
}

#[tokio::test]
async fn header_fields_refuse_line_breaks() {
    let sink = Sink::start(Script::default()).await;
    let ctx = TestContext::new().check_context(json!({}));
    for (field, value) in [
        ("from_name", "Bakery\r\nBcc: victim@example.com"),
        ("reply_to", "orders@bakery.example\nBcc: victim@example.com"),
        ("from_address", "not an address"),
    ] {
        let fields = fields_with("127.0.0.1", sink.port(), |plain| {
            plain.insert(field.into(), json!(value));
        });
        let err = agent().verify_credentials(&ctx, &fields).await.unwrap_err();
        assert_eq!(
            err,
            AgentError::InvalidCredentials {
                field: Some(field.into())
            },
            "{field}"
        );
    }
    assert_eq!(sink.connections(), 0);
}

#[test]
fn descriptor_validates_and_declares_open_tracking() {
    let agent = agent();
    let descriptor = agent.descriptor();
    descriptor.validate().unwrap();
    assert_eq!(descriptor.platform.as_str(), "email");
    let field = descriptor
        .options_schema
        .fields()
        .iter()
        .find(|f| f.name == "open_tracking")
        .unwrap();
    assert!(matches!(
        field.kind,
        FieldKind::Boolean {
            default: Some(false)
        }
    ));
}

// ---- Extensions, EHLO, AUTH, TLS, and ports ----

#[tokio::test]
async fn internationalized_address_uses_smtputf8() {
    let sink = Sink::start(Script::default()).await;
    let to = recipients(&["josé@exämple.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert!(is_sent(&outcome_of(&results, &to[0])));
    let mail = &sink.received()[0];
    assert!(
        mail.mail_params.contains("SMTPUTF8"),
        "{}",
        mail.mail_params
    );
    // An ASCII envelope asks for nothing.
    let plain = Sink::start(Script::default()).await;
    send(&plain, &recipients(&["a@example.com"])).await.unwrap();
    assert_eq!(plain.received()[0].mail_params, "");
}

#[tokio::test]
async fn smtputf8_missing_fails_that_recipient_only() {
    let sink = Sink::start(Script {
        hide_smtputf8: true,
        ..Script::default()
    })
    .await;
    let to = recipients(&["josé@exämple.com", "b@example.com"]);
    let results = send(&sink, &to).await.unwrap();
    assert_eq!(
        outcome_of(&results, &to[0]),
        DirectSendOutcome::Failed {
            retryable: false,
            reason: "smtputf8_unsupported".into()
        }
    );
    assert!(is_sent(&outcome_of(&results, &to[1])));
    assert_eq!(sink.connections(), 1);
    assert_eq!(sink.received()[0].connection, 1);
}

fn raw_transport(
    sink: &Sink,
    tls: TlsMode,
    login: Option<(String, SecretString)>,
) -> SmtpTransport {
    SmtpTransport::new(
        SmtpServer {
            host: "127.0.0.1".into(),
            port: sink.port(),
            tls,
            login,
        },
        AddressPolicy {
            allow_private: true,
        },
        Duration::from_secs(5),
        Arc::new(SystemResolve),
    )
}

fn raw_mail(body: &str) -> OutgoingMail {
    OutgoingMail {
        from: FROM.parse().unwrap(),
        to: "a@example.com".parse().unwrap(),
        message: format!("Subject: hi\r\n\r\n{body}\r\n").into_bytes(),
    }
}

#[tokio::test]
async fn eight_bit_body_uses_8bitmime() {
    let sink = Sink::start(Script::default()).await;
    let mut session = raw_transport(&sink, TlsMode::None, None)
        .open()
        .await
        .unwrap();
    session.send(&raw_mail("café")).await.unwrap();
    session.close().await;
    let params = sink.received()[0].mail_params.clone();
    assert!(params.contains("BODY=8BITMIME"), "{params}");

    let without = Sink::start(Script {
        hide_8bitmime: true,
        ..Script::default()
    })
    .await;
    let mut session = raw_transport(&without, TlsMode::None, None)
        .open()
        .await
        .unwrap();
    assert_eq!(
        session.send(&raw_mail("café")).await.unwrap_err(),
        SendFailure::Unsupported("8bitmime_unsupported")
    );
    // The session still carries the next mail.
    session.send(&raw_mail("plain")).await.unwrap();
    assert_eq!(without.received().len(), 1);
}

#[tokio::test]
async fn ehlo_sends_the_configured_name() {
    let sink = Sink::start(Script::default()).await;
    let named = EmailAgent::new(EmailAgentConfig {
        hello_name: Some("postit.example".into()),
        ..config(true, Duration::from_secs(5))
    });
    send_with(&named, fields(&sink), &recipients(&["a@example.com"]))
        .await
        .unwrap();
    assert_eq!(sink.ehlo_names(), vec!["postit.example".to_owned()]);

    // An IP address is sent as an address literal.
    let literal = Sink::start(Script::default()).await;
    let by_ip = EmailAgent::new(EmailAgentConfig {
        hello_name: Some("203.0.113.7".into()),
        ..config(true, Duration::from_secs(5))
    });
    send_with(&by_ip, fields(&literal), &recipients(&["a@example.com"]))
        .await
        .unwrap();
    assert_eq!(literal.ehlo_names(), vec!["[203.0.113.7]".to_owned()]);
}

#[tokio::test]
async fn temporary_auth_refusal_is_retryable() {
    let sink = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        auth_reply: Some("454 4.7.0 temporary authentication failure".into()),
        ..Script::default()
    })
    .await;
    let err = send_with(
        &agent(),
        login_fields(&sink, PASSWORD),
        &recipients(&["a@example.com"]),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AgentError::Transport { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::Retryable);
    let ctx = TestContext::new().check_context(json!({}));
    let err = agent()
        .verify_credentials(&ctx, &login_fields(&sink, PASSWORD))
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Transport { .. }), "{err:?}");

    // A permanent refusal is still the login's fault.
    let refusing = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        auth_reply: Some("535 5.7.8 bad credentials".into()),
        ..Script::default()
    })
    .await;
    let err = send_with(
        &agent(),
        login_fields(&refusing, PASSWORD),
        &recipients(&["a@example.com"]),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
}

#[tokio::test]
async fn starttls_missing_never_sends_the_login() {
    let sink = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        ..Script::default()
    })
    .await;
    let (mut plain, secret) = login_fields(&sink, PASSWORD).split();
    plain.insert("tls".into(), json!("starttls"));
    let fields = CredentialFields::from_parts(
        plain,
        secret
            .into_iter()
            .map(|(k, v)| (k, SecretString::from(v)))
            .collect(),
    );
    let err = send_with(&agent(), fields, &recipients(&["a@example.com"]))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AgentError::InvalidCredentials {
            field: Some("tls".into())
        }
    );
    assert!(!sink.commands().iter().any(|(_, v)| v == "AUTH"));
    assert_eq!(sink.auths(), 0);
}

#[tokio::test]
async fn failed_starttls_handshake_is_a_tls_error() {
    let sink = Sink::start(Script {
        login: Some((USER.into(), PASSWORD.into())),
        starttls_garbage: true,
        ..Script::default()
    })
    .await;
    let login = Some((USER.to_owned(), SecretString::from(PASSWORD.to_owned())));
    let err = raw_transport(&sink, TlsMode::Starttls, login)
        .open()
        .await
        .err()
        .unwrap();
    assert_eq!(err, TransportError::Tls);
    let verbs: Vec<String> = sink.commands().into_iter().map(|(_, v)| v).collect();
    assert!(verbs.iter().any(|v| v == "STARTTLS"), "{verbs:?}");
    assert!(!verbs.iter().any(|v| v == "AUTH"), "{verbs:?}");
    assert_eq!(sink.auths(), 0);
}

#[tokio::test]
async fn port_defaults_follow_the_tls_mode() {
    let sink = Sink::start(Script::default()).await;
    for (tls, port) in [("starttls", 587), ("tls", 465), ("none", 25)] {
        let lookup = Counting::new(vec![sink.addr()]);
        let agent =
            EmailAgent::new(config(true, Duration::from_secs(2))).with_resolver(lookup.clone());
        let fields = fields_with("smtp.invalid", 1, |plain| {
            plain.remove("port");
            plain.insert("tls".into(), json!(tls));
        });
        // Only the port asked for matters here, not whether the sink speaks that mode.
        let _ = send_with(&agent, fields, &recipients(&["a@example.com"])).await;
        assert_eq!(lookup.ports(), vec![port], "{tls}");
    }
}

#[test]
fn port_has_no_schema_default() {
    let agent = agent();
    let postit_core::ConnectMethod::Credentials(schema) = &agent.descriptor().connect else {
        unreachable!("the email agent connects with credentials");
    };
    let port = schema.fields().iter().find(|f| f.name == "port").unwrap();
    assert!(matches!(
        port.kind,
        FieldKind::Integer { default: None, .. }
    ));
    assert!(port.help.is_some());
}
