//! Shared builders for the mock agent's tests.
#![allow(dead_code)] // each test binary uses a different subset

use std::collections::BTreeMap;

use postit_agent_kit::testkit::TestContext;
use postit_agent_mock::MockAgent;
use postit_core::{
    AccountId, AccountRef, AgentClient, AgentError, CredentialFields, Credentials, DeliveryId,
    PlatformId, PostKind, PublishProgress, Variant,
};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// The token the tests connect with.
pub const TOKEN: &str = "s3cret-token";

/// Pasted credentials with `account_name` and `token`.
pub fn fields(account_name: &str, token: &str) -> CredentialFields {
    let mut plain = Map::new();
    plain.insert("account_name".to_owned(), json!(account_name));
    let secret = BTreeMap::from([("token".to_owned(), SecretString::from(token.to_owned()))]);
    CredentialFields::from_parts(plain, secret)
}

/// A test context whose grant holds `fields("shop", token)`.
pub fn context(token: &str) -> TestContext {
    TestContext::new().with_credentials(Credentials::Fields(fields("shop", token)))
}

/// A client of `agent` connected with `token`.
pub fn client(agent: &MockAgent, ctx: &TestContext) -> Box<dyn AgentClient> {
    use postit_core::Agent as _;
    agent
        .create_client(ctx.agent_context(Value::Null))
        .expect("the mock builds a client")
}

/// The connected account on `platform`.
pub fn account(platform: &str) -> AccountRef {
    AccountRef {
        platform: PlatformId::new(platform).unwrap(),
        account: AccountId::from_uuid(Uuid::nil()),
        remote_id: "mock-account-shop".to_owned(),
    }
}

/// A text post to the `mock` account, with `outcome` as its publish option when given.
pub fn variant(outcome: Option<&str>) -> Variant {
    let mut options = Map::new();
    if let Some(outcome) = outcome {
        options.insert("outcome".to_owned(), json!(outcome));
    }
    Variant {
        target: account("mock"),
        kind: PostKind::Post,
        text: "hello from the mock".to_owned(),
        media: Vec::new(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options,
    }
}

/// A fresh idempotency key.
pub fn key() -> DeliveryId {
    DeliveryId::from_uuid(Uuid::now_v7())
}

/// A progress that may ask to cancel and never fails to save.
pub struct Progress(pub bool);

#[async_trait::async_trait]
impl PublishProgress for Progress {
    async fn save_container_ref(&self, _container_ref: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        self.0
    }
}
