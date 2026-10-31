//! The sender-domain check over a fake `TxtLookup`: SPF, DKIM, and DMARC findings for the
//! domain of the account's from address.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use postit_agent_email::{EmailAgent, EmailAgentConfig, LookupError, TxtLookup};
use postit_agent_kit::testkit::TestContext;
use postit_core::{
    AccountId, AccountRef, Agent, AgentClient, Capability, CredentialFields, Credentials,
    DomainFinding, DomainRecord, DomainStatus, PlatformId,
};
use serde_json::{Map, json};
use uuid::Uuid;

const FROM: &str = "news@Bakery.Example";
const DOMAIN: &str = "bakery.example";

/// Answers from a table; a name it does not hold has no records. Records every name asked.
#[derive(Default)]
struct FakeLookup {
    answers: BTreeMap<String, Result<Vec<String>, LookupError>>,
    asked: Mutex<Vec<String>>,
}

impl FakeLookup {
    fn with(mut self, name: &str, records: &[&str]) -> Self {
        self.answers.insert(
            name.to_owned(),
            Ok(records.iter().map(|r| (*r).to_owned()).collect()),
        );
        self
    }

    fn failing(mut self, name: &str) -> Self {
        self.answers
            .insert(name.to_owned(), Err(LookupError::Failed));
        self
    }

    /// A domain with a sound SPF, DKIM key under `s1`, and DMARC policy.
    fn sound() -> Self {
        Self::default()
            .with(DOMAIN, &["google-site-verification=abc", "v=spf1 mx -all"])
            .with(
                &format!("s1._domainkey.{DOMAIN}"),
                &["v=DKIM1; k=rsa; p=MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA"],
            )
            .with(
                &format!("_dmarc.{DOMAIN}"),
                &["v=DMARC1; p=reject; rua=mailto:d@x"],
            )
    }
}

#[async_trait]
impl TxtLookup for FakeLookup {
    async fn txt(&self, name: &str) -> Result<Vec<String>, LookupError> {
        self.asked.lock().unwrap().push(name.to_owned());
        match self.answers.get(name) {
            Some(Ok(records)) => Ok(records.clone()),
            Some(Err(_)) => Err(LookupError::Failed),
            None => Ok(Vec::new()),
        }
    }
}

fn agent(lookup: Arc<FakeLookup>) -> EmailAgent {
    EmailAgent::new(EmailAgentConfig {
        allow_private: true,
        image_url_ttl: Duration::from_secs(3600),
        command_timeout: Duration::from_secs(5),
        hello_name: None,
    })
    .with_txt_lookup(lookup)
}

fn fields(from: &str) -> CredentialFields {
    let mut plain = Map::new();
    plain.insert("transport".into(), json!("smtp"));
    plain.insert("from_address".into(), json!(from));
    plain.insert("postal_address".into(), json!("12 Main St"));
    plain.insert("host".into(), json!("127.0.0.1"));
    plain.insert("port".into(), json!(2525));
    plain.insert("tls".into(), json!("none"));
    CredentialFields::from_parts(plain, BTreeMap::new())
}

fn account() -> AccountRef {
    AccountRef {
        platform: PlatformId::new("email").unwrap(),
        account: AccountId::from_uuid(Uuid::from_u128(1)),
        remote_id: FROM.to_owned(),
    }
}

fn client(lookup: Arc<FakeLookup>) -> Box<dyn AgentClient> {
    let ctx = TestContext::new().with_credentials(Credentials::Fields(fields(FROM)));
    agent(lookup)
        .create_client(ctx.agent_context(json!({})))
        .unwrap()
}

async fn check(lookup: FakeLookup, selector: Option<&str>) -> Vec<DomainFinding> {
    let client = client(Arc::new(lookup));
    client
        .domain_check()
        .expect("the email agent offers the check")
        .check(&account(), selector)
        .await
        .unwrap()
}

fn finding(findings: &[DomainFinding], record: DomainRecord) -> (DomainStatus, String) {
    let found: Vec<_> = findings.iter().filter(|f| f.record == record).collect();
    assert_eq!(found.len(), 1, "one finding per record: {findings:?}");
    (found[0].status, found[0].code.clone())
}

fn spf(findings: &[DomainFinding]) -> (DomainStatus, String) {
    finding(findings, DomainRecord::Spf)
}

fn dkim(findings: &[DomainFinding]) -> (DomainStatus, String) {
    finding(findings, DomainRecord::Dkim)
}

fn dmarc(findings: &[DomainFinding]) -> (DomainStatus, String) {
    finding(findings, DomainRecord::Dmarc)
}

fn ok(code: &str) -> (DomainStatus, String) {
    (DomainStatus::Ok, code.to_owned())
}

fn missing(code: &str) -> (DomainStatus, String) {
    (DomainStatus::Missing, code.to_owned())
}

fn weak(code: &str) -> (DomainStatus, String) {
    (DomainStatus::Weak, code.to_owned())
}

fn unknown(code: &str) -> (DomainStatus, String) {
    (DomainStatus::Unknown, code.to_owned())
}

#[test]
fn descriptor_declares_the_check() {
    assert!(
        agent(Arc::default())
            .descriptor()
            .capabilities
            .contains(&Capability::SenderDomainCheck)
    );
}

#[tokio::test]
async fn spf_found() {
    let findings = check(FakeLookup::sound(), Some("s1")).await;
    assert_eq!(findings.len(), 3, "{findings:?}");
    assert_eq!(spf(&findings), ok("spf_ok"));
    assert_eq!(dkim(&findings), ok("dkim_ok"));
    assert_eq!(dmarc(&findings), ok("dmarc_ok"));
}

#[tokio::test]
async fn spf_soft_fail_and_redirect_are_ok() {
    for record in [
        "v=spf1 include:_spf.mail.example ~all",
        "v=spf1 redirect=_spf.mail.example",
    ] {
        let lookup = FakeLookup::sound().with(DOMAIN, &[record]);
        assert_eq!(spf(&check(lookup, None).await), ok("spf_ok"), "{record}");
    }
}

#[tokio::test]
async fn spf_missing() {
    let lookup = FakeLookup::sound().with(DOMAIN, &["google-site-verification=abc"]);
    assert_eq!(spf(&check(lookup, None).await), missing("spf_missing"));

    let none = FakeLookup::sound().with(DOMAIN, &[]);
    assert_eq!(spf(&check(none, None).await), missing("spf_missing"));
}

#[tokio::test]
async fn spf_plus_all_is_weak() {
    for record in [
        "v=spf1 +all",
        "v=spf1 a mx all",
        "v=spf1 include:x.example ?all",
        // No `all` and no `redirect`: the default result is neutral.
        "v=spf1 mx",
    ] {
        let lookup = FakeLookup::sound().with(DOMAIN, &[record]);
        assert_eq!(
            spf(&check(lookup, None).await),
            weak("spf_weak"),
            "{record}"
        );
    }
    // Two SPF records are a permanent error for every receiver.
    let twice = FakeLookup::sound().with(DOMAIN, &["v=spf1 -all", "v=spf1 mx -all"]);
    assert_eq!(spf(&check(twice, None).await), weak("spf_weak"));
}

#[tokio::test]
async fn dmarc_p_none_is_weak() {
    for record in [
        "v=DMARC1; p=none; rua=mailto:d@x",
        "v=DMARC1; rua=mailto:d@x",
    ] {
        let lookup = FakeLookup::sound().with(&format!("_dmarc.{DOMAIN}"), &[record]);
        assert_eq!(
            dmarc(&check(lookup, None).await),
            weak("dmarc_weak"),
            "{record}"
        );
    }
    let quarantine =
        FakeLookup::sound().with(&format!("_dmarc.{DOMAIN}"), &["v=DMARC1;p=quarantine"]);
    assert_eq!(dmarc(&check(quarantine, None).await), ok("dmarc_ok"));
}

#[tokio::test]
async fn dmarc_missing() {
    let lookup = FakeLookup::sound().with(&format!("_dmarc.{DOMAIN}"), &["unrelated"]);
    assert_eq!(dmarc(&check(lookup, None).await), missing("dmarc_missing"));
}

#[tokio::test]
async fn dmarc_found_on_the_parent_domain() {
    // A sending subdomain is covered by its organizational domain's policy, `sp` first.
    let lookup = Arc::new(
        FakeLookup::default()
            .with("mail.bakery.example", &["v=spf1 -all"])
            .with("_dmarc.bakery.example", &["v=DMARC1; p=reject; sp=none"]),
    );
    let ctx = TestContext::new()
        .with_credentials(Credentials::Fields(fields("news@mail.bakery.example")));
    let client = agent(Arc::clone(&lookup))
        .create_client(ctx.agent_context(json!({})))
        .unwrap();
    let findings = client
        .domain_check()
        .unwrap()
        .check(&account(), None)
        .await
        .unwrap();
    assert_eq!(dmarc(&findings), weak("dmarc_weak"));
    let asked = lookup.asked.lock().unwrap().clone();
    assert!(
        asked.contains(&"_dmarc.mail.bakery.example".to_owned()),
        "{asked:?}"
    );
    assert!(
        !asked.iter().any(|name| name == "_dmarc.example"),
        "never past the last two labels: {asked:?}"
    );
}

#[tokio::test]
async fn dkim_unknown_without_selector() {
    let lookup = Arc::new(FakeLookup::sound());
    let client = client(Arc::clone(&lookup));
    let findings = client
        .domain_check()
        .unwrap()
        .check(&account(), None)
        .await
        .unwrap();
    assert_eq!(dkim(&findings), unknown("dkim_unknown"));
    assert!(
        !lookup
            .asked
            .lock()
            .unwrap()
            .iter()
            .any(|name| name.contains("_domainkey")),
        "no DKIM lookup without a selector"
    );
}

#[tokio::test]
async fn dkim_ok_with_selector() {
    let lookup = Arc::new(FakeLookup::sound());
    let client = client(Arc::clone(&lookup));
    let findings = client
        .domain_check()
        .unwrap()
        .check(&account(), Some("S1"))
        .await
        .unwrap();
    assert_eq!(dkim(&findings), ok("dkim_ok"));
    let asked = lookup.asked.lock().unwrap().clone();
    assert!(
        asked.contains(&format!("s1._domainkey.{DOMAIN}")),
        "names are lowercased: {asked:?}"
    );
}

#[tokio::test]
async fn dkim_missing_or_revoked() {
    let missing_key = FakeLookup::sound();
    assert_eq!(
        dkim(&check(missing_key, Some("other")).await),
        missing("dkim_missing")
    );
    // An empty `p=` is a revoked key.
    let revoked =
        FakeLookup::sound().with(&format!("s1._domainkey.{DOMAIN}"), &["v=DKIM1; k=rsa; p="]);
    assert_eq!(
        dkim(&check(revoked, Some("s1")).await),
        missing("dkim_missing")
    );
}

#[tokio::test]
async fn invalid_selector_is_refused() {
    let client = client(Arc::new(FakeLookup::sound()));
    for selector in ["", "a.b", "-s1", "s1-", "s 1", &"s".repeat(64)] {
        let result = client
            .domain_check()
            .unwrap()
            .check(&account(), Some(selector))
            .await;
        assert!(
            matches!(result, Err(postit_core::AgentError::Validation(_))),
            "{selector:?}: {result:?}"
        );
    }
}

#[tokio::test]
async fn lookup_failure_is_unknown_not_error() {
    let lookup = FakeLookup::sound()
        .failing(DOMAIN)
        .failing(&format!("s1._domainkey.{DOMAIN}"))
        .failing(&format!("_dmarc.{DOMAIN}"));
    let findings = check(lookup, Some("s1")).await;
    assert_eq!(spf(&findings), unknown("spf_unknown"));
    assert_eq!(dkim(&findings), unknown("dkim_unknown"));
    assert_eq!(dmarc(&findings), unknown("dmarc_unknown"));
}

#[tokio::test]
async fn findings_hold_no_record_text() {
    let findings = check(FakeLookup::sound(), Some("s1")).await;
    let text = serde_json::to_string(&findings).unwrap();
    for secret in [DOMAIN, "s1", "spf1", "DMARC1", "MIIB"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    assert!(!LookupError::Failed.to_string().contains(DOMAIN));
}

/// The real resolver against public DNS. Opt-in: `POSTIT_LIVE_TESTS=1`.
#[tokio::test]
async fn live_hickory_lookup() {
    if std::env::var("POSTIT_LIVE_TESTS").as_deref() != Ok("1") {
        return;
    }
    let lookup = postit_agent_email::HickoryLookup::new();
    let records = lookup.txt("_dmarc.gmail.com").await.unwrap();
    assert!(
        records.iter().any(|r| r.starts_with("v=DMARC1")),
        "{records:?}"
    );
    let none = lookup
        .txt("no-such-name.postit-check.invalid")
        .await
        .unwrap();
    assert!(none.is_empty(), "{none:?}");
}
