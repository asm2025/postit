//! A small scriptable credentials agent for this crate's tests and the API's route tests.
//! B5's mock agent supersedes it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, Agent, AgentClient, AgentContext, AgentDescriptor,
    AgentError, AgentKind, Capability, ConnectMethod, CredentialCheckContext, CredentialFields,
    Discovered, DiscoveredAccount, FormatId, FormatSpec, MediaAsset, MediaRules, OAuthProviderSpec,
    OptionsSchema, PlatformId, PublishMode, PublishOutcome, PublishProgress, PublishReceipt,
    PublishRequest, ReconcileOutcome, ReconcileRequest, TextMeasure, TextRules, Variant, Violation,
};
use url::Url;

const LABEL: &str = r#"{"key":"k","en":"E"}"#;

/// Chooses a row's behavior and delay from the pasted values.
pub type Decide = Arc<dyn Fn(&CredentialFields) -> (Duration, VerifyScript) + Send + Sync>;

/// What `verify_credentials` does.
#[derive(Clone)]
pub enum VerifyScript {
    /// Succeed, reaching these accounts.
    Accounts(Vec<DiscoveredAccount>),
    /// The platform rejects the credentials, naming a field.
    Invalid(Option<String>),
    /// The platform asks the caller to slow down.
    RateLimited,
    /// A platform failure.
    Fail,
    /// Decide per call from the pasted values, after a delay. For CSV tests.
    ByFields(Decide),
}

/// What `account_info` does.
#[derive(Debug, Clone, Copy)]
pub enum InfoScript {
    /// Succeed.
    Ok,
    /// The platform rejects the stored credential.
    AuthFailed,
}

/// A test agent's behavior.
#[derive(Clone)]
pub struct Script {
    /// `verify_credentials`.
    pub verify: VerifyScript,
    /// `account_info`.
    pub account_info: InfoScript,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            verify: VerifyScript::Accounts(Vec::new()),
            account_info: InfoScript::Ok,
        }
    }
}

#[derive(Default)]
struct Shared {
    script: Mutex<Option<Script>>,
    revokes: Mutex<usize>,
    fail_revoke: Mutex<bool>,
    seen: Mutex<Vec<String>>,
}

/// A credentials agent with a one-plain, one-secret field schema (`server_url`, `token`).
pub struct TestCredentialAgent {
    descriptor: AgentDescriptor,
    shared: Arc<Shared>,
}

impl TestCredentialAgent {
    /// An agent for `platform` that discovers nothing until [`Self::set_script`] says otherwise.
    ///
    /// # Panics
    ///
    /// If `platform` is not a valid platform ID, or the built-in schema is invalid.
    #[must_use]
    pub fn new(platform: &str) -> Arc<Self> {
        Self::build(platform, connect_credentials())
    }

    /// An agent for `platform` whose connect method is OAuth.
    ///
    /// # Panics
    ///
    /// If `platform` is not a valid platform ID.
    #[must_use]
    pub fn oauth(platform: &str) -> Arc<Self> {
        let spec = OAuthProviderSpec::new(
            Url::parse("https://example.test/authorize").unwrap_or_else(|_| unreachable!()),
            Url::parse("https://example.test/token").unwrap_or_else(|_| unreachable!()),
        );
        Self::build(platform, ConnectMethod::OAuth(spec))
    }

    fn build(platform: &str, connect: ConnectMethod) -> Arc<Self> {
        let id = PlatformId::new(platform).unwrap_or_else(|e| unreachable!("platform id: {e}"));
        let descriptor = AgentDescriptor {
            platform: id,
            display_name: platform.to_owned(),
            icon_png: &[0x89, b'P', b'N', b'G'],
            brand_color: "#112233".to_owned(),
            kind: AgentKind::Broadcast,
            connect,
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
            capabilities: BTreeSet::new(),
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: OptionsSchema::empty(),
        };
        Arc::new(Self {
            descriptor,
            shared: Arc::new(Shared::default()),
        })
    }

    /// Replaces the agent's behavior.
    pub fn set_script(&self, script: Script) {
        *self
            .shared
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(script);
    }

    /// How many times a client's `revoke` ran.
    #[must_use]
    pub fn revoke_calls(&self) -> usize {
        *self
            .shared
            .revokes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes `revoke` fail (after counting the call) when `fail` is true.
    pub fn fail_revoke(&self, fail: bool) {
        *self
            .shared
            .fail_revoke
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = fail;
    }

    /// The `remote_id` of every account `account_info` was asked about.
    #[must_use]
    pub fn seen_remote_ids(&self) -> Vec<String> {
        self.shared
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn script(&self) -> Script {
        script_of(&self.shared)
    }
}

fn script_of(shared: &Shared) -> Script {
    shared
        .script
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// The credentials schema the test agent asks for.
///
/// # Panics
///
/// Never; the schema is a constant.
#[must_use]
pub fn credentials_schema() -> OptionsSchema {
    let src = r#"{"type":"object","properties":{
        "server_url":{"type":"string","maxLength":200,"x-postit-label":LBL},
        "token":{"type":"string","maxLength":200,"x-postit-secret":true,"x-postit-label":LBL}
    },"required":["server_url","token"],"additionalProperties":false}"#
        .replace("LBL", LABEL);
    OptionsSchema::from_json_str(&src).unwrap_or_else(|e| unreachable!("schema: {e:?}"))
}

fn connect_credentials() -> ConnectMethod {
    ConnectMethod::Credentials(credentials_schema())
}

#[async_trait]
impl Agent for TestCredentialAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn create_client(&self, _ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(TestClient {
            shared: Arc::clone(&self.shared),
        }))
    }

    async fn verify_credentials(
        &self,
        _ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        let mut verify = self.script().verify;
        loop {
            match verify {
                VerifyScript::Accounts(accounts) => return Ok(Discovered { accounts }),
                VerifyScript::Invalid(field) => {
                    return Err(AgentError::InvalidCredentials { field });
                }
                VerifyScript::RateLimited => {
                    return Err(AgentError::RateLimited { retry_after: None });
                }
                VerifyScript::Fail => {
                    return Err(AgentError::Remote {
                        status: Some(500),
                        code: None,
                        message: "boom".to_owned(),
                    });
                }
                VerifyScript::ByFields(decide) => {
                    let (delay, next) = decide(fields);
                    tokio::time::sleep(delay).await;
                    verify = next;
                }
            }
        }
    }
}

struct TestClient {
    shared: Arc<Shared>,
}

#[async_trait]
impl AgentClient for TestClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        self.shared
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(account.remote_id.clone());
        match script_of(&self.shared).account_info {
            InfoScript::Ok => Ok(AccountInfo {
                remote_id: account.remote_id.clone(),
                display_name: account.remote_id.clone(),
                handle: None,
                avatar_url: None,
                status: AccountStatus::Active,
            }),
            InfoScript::AuthFailed => Err(AgentError::Authentication {
                message: "rejected".to_owned(),
            }),
        }
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        Ok(Discovered {
            accounts: Vec::new(),
        })
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
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
                planned: Vec::new(),
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
        Err(AgentError::Unsupported {
            capability: Capability::Commenter,
        })
    }

    async fn reconcile(
        &self,
        _request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        Ok(ReconcileOutcome::Unresolvable)
    }

    async fn revoke(&self) -> Result<(), AgentError> {
        *self
            .shared
            .revokes
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        if *self
            .shared
            .fail_revoke
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            return Err(AgentError::Remote {
                status: Some(500),
                code: None,
                message: "revoke failed".to_owned(),
            });
        }
        Ok(())
    }
}

/// Media is not built yet (B4): every call says so.
pub struct NoMedia;

#[async_trait]
impl postit_core::MediaStore for NoMedia {
    async fn open(
        &self,
        _asset: &MediaAsset,
    ) -> Result<postit_core::ByteStream, postit_core::MediaError> {
        Err(postit_core::MediaError::NotFound)
    }
}

impl postit_core::MediaHost for NoMedia {
    fn public_url(
        &self,
        _asset: &MediaAsset,
        _ttl: Duration,
    ) -> Result<Url, postit_core::MediaError> {
        Err(postit_core::MediaError::NotReady)
    }
}

/// Link signing is not built yet (C2): every URL is a placeholder.
pub struct NoLinks;

impl postit_core::LinkSigner for NoLinks {
    fn unsubscribe_url(&self, _recipient: &postit_core::DirectRecipient) -> Url {
        placeholder()
    }

    fn click_url(&self, _send_id: postit_core::DirectSendId, _destination: &Url) -> Url {
        placeholder()
    }

    fn open_pixel_url(&self, _send_id: postit_core::DirectSendId) -> Url {
        placeholder()
    }
}

fn placeholder() -> Url {
    Url::parse("https://postit.test/unavailable").unwrap_or_else(|_| unreachable!())
}

/// The vault every test service uses.
///
/// # Panics
///
/// Never; the key is a constant of valid length.
#[must_use]
pub fn test_vault() -> postit_vault::Vault {
    postit_vault::Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

/// Default import limits for tests.
#[must_use]
pub fn test_accounts_settings() -> postit_config::AccountsSettings {
    postit_config::AccountsSettings {
        csv_max_bytes: 5_242_880,
        csv_max_rows: 1_000,
        csv_concurrency: 4,
        csv_timeout: Duration::from_secs(300),
    }
}

/// An [`crate::AccountService`] over `pool` and the given agents, with the test vault, the
/// system clock, and the not-yet-built media and link stubs.
///
/// # Panics
///
/// If an agent's platform is registered twice, or the HTTP clients cannot be built.
#[must_use]
pub fn account_service(
    pool: sqlx::PgPool,
    agents: Vec<Arc<dyn Agent>>,
    settings: postit_config::AccountsSettings,
) -> crate::AccountService {
    let mut registry = postit_core::AgentRegistry::new();
    for agent in agents {
        registry
            .register(agent)
            .unwrap_or_else(|e| unreachable!("register: {e}"));
    }
    let http_settings = postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-test/0".to_owned(),
        extra_ca_files: Vec::new(),
    };
    let standard =
        postit_http::build_client(&http_settings).unwrap_or_else(|e| unreachable!("client: {e}"));
    let public =
        postit_http::build_public_client(&http_settings, postit_http::AddressPolicy::strict())
            .unwrap_or_else(|e| unreachable!("client: {e}"));
    // OAuth platforms need a `[platforms.<id>]` table to be available; give them an empty one.
    let mut configs = crate::PlatformConfigs::default();
    for agent in registry.iter() {
        if matches!(agent.descriptor().connect, ConnectMethod::OAuth(_)) {
            configs.insert(agent.descriptor().platform.clone(), serde_json::json!({}));
        }
    }
    crate::AccountService::new(crate::AccountServiceDeps {
        pool,
        vault: test_vault(),
        catalog: Arc::new(crate::PlatformCatalog::new(registry, configs)),
        http: postit_http::agent_handles(standard, public),
        media_store: Arc::new(NoMedia),
        media_host: Arc::new(NoMedia),
        link_signer: Arc::new(NoLinks),
        clock: Arc::new(postit_core::SystemClock),
        ids: Arc::new(postit_core::SystemIdGenerator),
        settings,
    })
}
