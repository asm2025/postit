//! The agent contract (plan 03 phase A1): what a platform implements and what the engine hands
//! it. Shared code depends on these traits, never on a platform's own types.

use std::collections::BTreeSet;

use crate::Capability;

mod capability;
mod client;
mod context;
mod event;

pub use capability::{
    Commenter, DirectRecipient, DirectSendOutcome, DirectSendResult, DirectSender, InboxReader,
    MetricsReader, NativeScheduler, RemoteTarget,
};
pub use client::{
    Agent, AgentClient, PublishMode, PublishProgress, PublishRequest, ReconcileRequest,
};
pub use context::{
    AgentContext, CredentialCheckContext, CredentialStore, Credentials, EventContext, HttpHandle,
    HttpHandles, LinkSigner, OAuthTokens,
};
pub use event::{EventResponse, InboundEvent};

/// The capabilities `client` actually exposes. The conformance suite compares this with the
/// descriptor's declared capabilities.
#[must_use]
pub fn exposed_capabilities(client: &dyn AgentClient) -> BTreeSet<Capability> {
    [
        (client.commenter().is_some(), Capability::Commenter),
        (client.inbox().is_some(), Capability::InboxReader),
        (client.metrics().is_some(), Capability::MetricsReader),
        (client.direct().is_some(), Capability::DirectSender),
        (
            client.native_scheduler().is_some(),
            Capability::NativeScheduler,
        ),
    ]
    .into_iter()
    .filter_map(|(present, capability)| present.then_some(capability))
    .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
pub(crate) mod test_support {
    //! Minimal fakes for this crate's tests. The reusable mock agent is built in B5.

    use std::any::Any;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use url::Url;

    use super::*;
    use crate::{
        AccountInfo, AccountRef, AgentDescriptor, AgentError, AgentKind, ByteStream, ConnectMethod,
        DirectSendId, Discovered, FormatId, FormatSpec, MediaAsset, MediaError, MediaHost,
        MediaRules, MediaStore, OptionsSchema, PlatformId, PublishOutcome, PublishReceipt,
        ReconcileOutcome, TestClock, TextMeasure, TextRules, Variant, Violation,
    };

    static ICON: &[u8] = &[0x89, b'P', b'N', b'G'];

    pub(crate) fn fake_descriptor(platform: &str) -> AgentDescriptor {
        AgentDescriptor {
            platform: PlatformId::new(platform).unwrap(),
            display_name: platform.to_owned(),
            icon_png: ICON,
            brand_color: "#112233".to_owned(),
            kind: AgentKind::Broadcast,
            connect: ConnectMethod::Credentials(OptionsSchema::empty()),
            formats: vec![FormatSpec {
                id: FormatId::Text,
                text: TextRules {
                    measure: TextMeasure::Graphemes,
                    max: 500,
                    required: true,
                },
                media: MediaRules::none(),
                selected_by: None,
            }],
            capabilities: BTreeSet::from([Capability::Commenter]),
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: OptionsSchema::empty(),
        }
    }

    pub(crate) struct FakeAgent {
        descriptor: AgentDescriptor,
    }

    impl FakeAgent {
        pub(crate) fn new(platform: &str) -> Self {
            Self::with_descriptor(fake_descriptor(platform))
        }

        pub(crate) fn with_descriptor(descriptor: AgentDescriptor) -> Self {
            Self { descriptor }
        }
    }

    #[async_trait]
    impl Agent for FakeAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }

        fn create_client(&self, _ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
            Ok(Box::new(FakeClient))
        }
    }

    pub(crate) struct FakeClient;

    #[async_trait]
    impl AgentClient for FakeClient {
        async fn account_info(&self, _account: &AccountRef) -> Result<AccountInfo, AgentError> {
            Err(unsupported())
        }

        async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
            Ok(Discovered {
                accounts: Vec::new(),
            })
        }

        async fn publish_options(
            &self,
            _account: &AccountRef,
        ) -> Result<OptionsSchema, AgentError> {
            Ok(OptionsSchema::empty())
        }

        async fn validate(
            &self,
            _variant: &Variant,
            _media: &[MediaAsset],
        ) -> Result<Vec<Violation>, AgentError> {
            Ok(Vec::new())
        }

        async fn publish(
            &self,
            request: PublishRequest<'_>,
            _progress: &dyn PublishProgress,
        ) -> Result<PublishOutcome, AgentError> {
            match request.mode {
                PublishMode::DryRun => Ok(PublishOutcome::Simulated {
                    planned: vec!["post text".to_owned()],
                }),
                PublishMode::Live => Ok(PublishOutcome::Published(PublishReceipt {
                    remote_id: Some("1".to_owned()),
                    remote_url: None,
                    container_ref: None,
                    remote_publish_at: None,
                })),
            }
        }

        async fn status(
            &self,
            _account: &AccountRef,
            _receipt: &PublishReceipt,
        ) -> Result<PublishOutcome, AgentError> {
            Err(unsupported())
        }

        async fn reconcile(
            &self,
            _request: ReconcileRequest<'_>,
        ) -> Result<ReconcileOutcome, AgentError> {
            Ok(ReconcileOutcome::Unresolvable)
        }

        fn commenter(&self) -> Option<&dyn Commenter> {
            Some(self)
        }
    }

    #[async_trait]
    impl Commenter for FakeClient {
        async fn resolve_target(
            &self,
            _account: &AccountRef,
            _url: &Url,
        ) -> Result<RemoteTarget, AgentError> {
            Err(unsupported())
        }

        async fn comment(
            &self,
            _request: PublishRequest<'_>,
            _progress: &dyn PublishProgress,
        ) -> Result<PublishOutcome, AgentError> {
            Err(unsupported())
        }

        async fn reconcile_comment(
            &self,
            _request: ReconcileRequest<'_>,
        ) -> Result<ReconcileOutcome, AgentError> {
            Err(unsupported())
        }
    }

    fn unsupported() -> AgentError {
        AgentError::Unsupported {
            capability: Capability::Commenter,
        }
    }

    pub(crate) struct NoProgress;

    #[async_trait]
    impl PublishProgress for NoProgress {
        async fn save_container_ref(&self, _container_ref: &str) -> Result<(), AgentError> {
            Ok(())
        }

        fn cancel_requested(&self) -> bool {
            false
        }
    }

    struct FakeHandle;

    impl HttpHandle for FakeHandle {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FakeCredentials;

    #[async_trait]
    impl CredentialStore for FakeCredentials {
        async fn credentials(&self) -> Result<Credentials, AgentError> {
            Err(AgentError::Authentication {
                message: "no credentials".to_owned(),
            })
        }

        async fn store_refreshed(&self, _tokens: OAuthTokens) -> Result<(), AgentError> {
            Ok(())
        }
    }

    struct FakeStore;

    #[async_trait]
    impl MediaStore for FakeStore {
        async fn open(&self, _asset: &MediaAsset) -> Result<ByteStream, MediaError> {
            Err(MediaError::NotFound)
        }
    }

    struct FakeHost;

    impl MediaHost for FakeHost {
        fn public_url(&self, _asset: &MediaAsset, _ttl: Duration) -> Result<Url, MediaError> {
            Err(MediaError::NotReady)
        }
    }

    struct FakeSigner;

    impl LinkSigner for FakeSigner {
        fn unsubscribe_url(&self, _recipient: &DirectRecipient) -> Url {
            Url::parse("https://example.com/unsubscribe").unwrap()
        }

        fn click_url(&self, _send_id: DirectSendId, destination: &Url) -> Url {
            destination.clone()
        }

        fn open_pixel_url(&self, _send_id: DirectSendId) -> Url {
            Url::parse("https://example.com/pixel").unwrap()
        }
    }

    pub(crate) fn fake_http() -> HttpHandles {
        HttpHandles {
            default: Arc::new(FakeHandle),
            user_hosts: Arc::new(FakeHandle),
        }
    }

    pub(crate) fn fake_clock() -> Arc<TestClock> {
        Arc::new(TestClock::new(
            Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap(),
        ))
    }

    pub(crate) fn fake_context() -> AgentContext {
        AgentContext {
            config: serde_json::Value::Null,
            http: fake_http(),
            credentials: Arc::new(FakeCredentials),
            media_store: Arc::new(FakeStore),
            media_host: Arc::new(FakeHost),
            link_signer: Arc::new(FakeSigner),
            clock: fake_clock(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;

    use bytes::Bytes;
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::test_support::{FakeAgent, NoProgress, fake_clock, fake_context, fake_http};
    use super::*;
    use crate::{
        AccountId, AccountRef, AgentError, CredentialFields, DeliveryId, PlatformId, PostKind,
        PublishOutcome, Variant,
    };

    fn variant() -> Variant {
        Variant {
            target: AccountRef {
                platform: PlatformId::new("demo").unwrap(),
                account: AccountId::from_uuid(Uuid::nil()),
            },
            kind: PostKind::Post,
            text: "hello".to_owned(),
            media: Vec::new(),
            link: None,
            tags: Vec::new(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options: serde_json::Map::new(),
        }
    }

    #[tokio::test]
    async fn fake_agent_is_dyn_compatible() {
        let agent: Box<dyn Agent> = Box::new(FakeAgent::new("demo"));
        assert_eq!(agent.descriptor().platform.as_str(), "demo");
        let client = agent.create_client(fake_context()).unwrap();
        let variant = variant();
        let outcome = client
            .publish(
                PublishRequest {
                    variant: &variant,
                    mode: PublishMode::DryRun,
                    native_publish_at: None,
                    container_ref: None,
                    idempotency_key: DeliveryId::from_uuid(Uuid::nil()),
                },
                &NoProgress,
            )
            .await
            .unwrap();
        assert!(matches!(outcome, PublishOutcome::Simulated { .. }));
    }

    #[tokio::test]
    async fn default_optional_methods_are_unsupported() {
        let agent = FakeAgent::new("demo");
        let event = InboundEvent {
            kind: "webhook".to_owned(),
            path_params: BTreeMap::new(),
            headers: Vec::new(),
            raw_body: Bytes::new(),
        };
        let events = EventContext {
            config: serde_json::Value::Null,
            clock: fake_clock(),
        };
        assert!(matches!(
            agent.handle_event(&events, event).await,
            Err(AgentError::Unsupported { .. })
        ));
        let check = CredentialCheckContext {
            config: serde_json::Value::Null,
            http: fake_http(),
            clock: fake_clock(),
        };
        let fields = CredentialFields::default();
        assert!(matches!(
            agent.verify_credentials(&check, &fields).await,
            Err(AgentError::Unsupported { .. })
        ));
        let client = agent.create_client(fake_context()).unwrap();
        assert!(matches!(
            client.revoke().await,
            Err(AgentError::Unsupported { .. })
        ));
    }

    #[test]
    fn exposed_capabilities_reports_accessors() {
        let agent = FakeAgent::new("demo");
        let client = agent.create_client(fake_context()).unwrap();
        assert_eq!(
            exposed_capabilities(client.as_ref()),
            BTreeSet::from([Capability::Commenter])
        );
    }

    #[test]
    fn inbound_event_header_lookup_is_case_insensitive() {
        let event = InboundEvent {
            kind: "webhook".to_owned(),
            path_params: BTreeMap::new(),
            headers: vec![("X-Signature".to_owned(), b"abc".to_vec())],
            raw_body: Bytes::new(),
        };
        assert_eq!(event.header("x-signature"), Some(b"abc".as_slice()));
        assert_eq!(event.header("missing"), None);
        assert_eq!(EventResponse::empty(204).status, 204);
    }

    #[test]
    fn oauth_tokens_debug_redacts() {
        let tokens = OAuthTokens {
            access_token: SecretString::from("access-secret".to_owned()),
            refresh_token: Some(SecretString::from("refresh-secret".to_owned())),
            expires_at: None,
            scopes: vec!["read".to_owned()],
        };
        let shown = format!("{tokens:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("read"));
    }
}
