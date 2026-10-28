#![allow(dead_code, clippy::unwrap_used)]

use chrono::{DateTime, Duration, Utc};
use postit_agent_mock::{MockAgent, MockScript};
use postit_core::{
    AccountId, AccountRef, Agent, DeliveryId, DiscoveredAccount, GrantId, PlatformId, PostId,
    PostKind, PostVersion, PublishJobId, PublishSnapshot, UserId, Variant,
};
use postit_data::OwnerScope;
use postit_data::accounts::{GrantKind, GrantRepo, NewGrant};
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::{CreatedJob, JobRepo, NewDelivery, NewJob, Operation};
use postit_vault::Vault;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

pub const SECRET: &str = "s3cret-token-value";

pub fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap()
}

pub fn platform(id: &str) -> PlatformId {
    PlatformId::new(id).unwrap()
}

/// The mock agent under the platform ID `id`, with its script. Nothing is scripted yet, so
/// the pasted token and `account_name` decide what it does.
pub fn mock_agent(id: &str) -> (Arc<dyn Agent>, MockScript) {
    let agent = MockAgent::broadcast().with_platform(platform(id));
    let script = agent.script();
    (Arc::new(agent), script)
}

/// The OAuth-connected mock agent under the platform ID `id`.
pub fn mock_oauth_agent(id: &str) -> Arc<dyn Agent> {
    let provider = Url::parse("https://example.test/").unwrap();
    Arc::new(MockAgent::oauth(&provider).with_platform(platform(id)))
}

/// The OAuth app's client ID the mock provider requires.
pub const CLIENT_ID: &str = "postit-app";
/// The OAuth app's client secret the mock provider requires.
pub const CLIENT_SECRET: &str = "postit-app-secret-value";

/// The mock OAuth provider, requiring [`CLIENT_ID`] and [`CLIENT_SECRET`].
pub async fn oauth_provider() -> postit_agent_kit::testkit::MockOAuthProvider {
    let provider = postit_agent_kit::testkit::MockOAuthProvider::start().await;
    provider.set_client(CLIENT_ID, CLIENT_SECRET);
    provider
}

/// `mock_oauth` under the platform ID `id`, pointed at `provider`, with its script.
pub fn oauth_agent(
    provider: &postit_agent_kit::testkit::MockOAuthProvider,
    id: &str,
) -> (Arc<dyn Agent>, MockScript) {
    let agent = MockAgent::oauth(&provider.base_url()).with_platform(platform(id));
    let script = agent.script();
    (Arc::new(agent), script)
}

/// The `[platforms.<id>]` table of an OAuth platform: the app's client ID and secret.
pub fn oauth_config() -> Value {
    json!({"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET})
}

/// Registers `ids` in the `platforms` table, switched on.
pub async fn platforms(pool: &PgPool, ids: &[&str]) {
    let registered: Vec<(PlatformId, String)> = ids
        .iter()
        .map(|id| (platform(id), (*id).to_owned()))
        .collect();
    postit_data::platforms::PlatformRepo::upsert_registered(pool, &registered)
        .await
        .unwrap();
}

/// A credentials grant holding `{ token: SECRET }` and `{ account_name }`.
pub async fn credentials_grant(pool: &PgPool, owner: UserId, platform_id: &str) -> GrantId {
    let id = GrantId::from(Uuid::now_v7());
    let credential = vault()
        .seal(&json!({"token": SECRET}), &GrantRepo::seal_context(id))
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    GrantRepo::insert(
        &mut conn,
        &OwnerScope::own(owner),
        NewGrant {
            id,
            platform: platform(platform_id),
            kind: GrantKind::Credentials,
            remote_user_id: None,
            credential,
            connection_config: json!({"account_name": "test-account"}),
            expires_at: None,
            reconnect_by: None,
            scopes: vec![],
        },
    )
    .await
    .unwrap();
    id
}

pub fn found(id: &str) -> DiscoveredAccount {
    DiscoveredAccount {
        remote_id: id.to_owned(),
        display_name: format!("Account {id}"),
        handle: None,
        avatar_url: None,
    }
}

/// The values a user pastes for the mock agent's credentials form.
pub fn values(token: &str) -> Map<String, Value> {
    let Value::Object(map) = json!({"account_name": "test-account", "token": token}) else {
        unreachable!()
    };
    map
}

/// Checks `SECRET` and confirms every account it reaches, as `scope`.
pub async fn connect_all(
    service: &postit_oauth::AccountService,
    scope: &OwnerScope,
    platform_id: &str,
) -> Vec<postit_data::accounts::Account> {
    let check = service
        .start_credentials(scope, &platform(platform_id), values(SECRET))
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    service
        .confirm(
            scope,
            connect_id.as_str(),
            None,
            postit_data::audit::AuditContext::default(),
        )
        .await
        .unwrap()
        .connected
        .into_iter()
        .map(|(account, _)| account)
        .collect()
}

/// A draft post for `owner`.
pub async fn post(pool: &PgPool, owner: UserId) -> PostId {
    let id = PostId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    PostRepo::create(
        &mut conn,
        &OwnerScope::own(owner),
        NewPost {
            id,
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: None,
            text: "hello".to_owned(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    id
}

/// A live job with one delivery for `account`, due in an hour.
pub async fn job_for(
    pool: &PgPool,
    owner: UserId,
    account: &postit_data::accounts::Account,
) -> CreatedJob {
    let scope = OwnerScope::own(owner);
    let post = post(pool, owner).await;
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, post, None).await.unwrap();
    let version = locked.post().version;
    let snapshot = PublishSnapshot {
        post_id: post,
        post_version: PostVersion(version),
        variants: vec![Variant {
            target: AccountRef {
                platform: account.platform.clone(),
                account: account.id,
                remote_id: account.remote_account_id.clone(),
            },
            kind: PostKind::Post,
            text: "hello".to_owned(),
            media: vec![],
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options: serde_json::Map::new(),
        }],
    };
    let job = NewJob {
        id: PublishJobId::from_uuid(Uuid::now_v7()),
        snapshot,
        targets: vec![NewDelivery {
            id: DeliveryId::from_uuid(Uuid::now_v7()),
            account: account.id,
            operation: Operation::Publish,
        }],
        scheduled_at: DateTime::from_timestamp_micros(
            (Utc::now() + Duration::hours(1)).timestamp_micros(),
        )
        .unwrap(),
        native_schedule: false,
        dry_run: false,
        recurrence_id: None,
    };
    let created = JobRepo::create(&mut tx, &scope, &locked, job)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    created
}

/// Forces every delivery of `account` into `state`, with a lease when that state needs one.
pub async fn force_state(pool: &PgPool, account: AccountId, state: &str) {
    sqlx::query(
        "UPDATE deliveries SET state = $2, lease_expires_at = now() + interval '1 minute'          WHERE account_id = $1",
    )
    .bind(account.as_uuid())
    .bind(state)
    .execute(pool)
    .await
    .unwrap();
}

/// `mock_oauth` with its OAuth spec edited, for specs the agent itself never declares (a
/// rotating refresh token, no RFC 7009 endpoint). Everything else is the mock's.
pub struct SpecAgent {
    inner: MockAgent,
    descriptor: postit_core::AgentDescriptor,
}

#[async_trait::async_trait]
impl Agent for SpecAgent {
    fn descriptor(&self) -> &postit_core::AgentDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> Option<&postit_core::OptionsSchema> {
        self.inner.config_schema()
    }

    fn create_client(
        &self,
        ctx: postit_core::AgentContext,
    ) -> Result<Box<dyn postit_core::AgentClient>, postit_core::AgentError> {
        self.inner.create_client(ctx)
    }

    async fn handle_event(
        &self,
        ctx: &postit_core::EventContext,
        event: postit_core::InboundEvent,
    ) -> Result<postit_core::EventOutcome, postit_core::AgentError> {
        self.inner.handle_event(ctx, event).await
    }

    async fn after_exchange(
        &self,
        ctx: &postit_core::ExchangeContext,
        exchange: postit_core::TokenExchange,
    ) -> Result<postit_core::GrantExchange, postit_core::AgentError> {
        self.inner.after_exchange(ctx, exchange).await
    }

    async fn verify_credentials(
        &self,
        ctx: &postit_core::CredentialCheckContext,
        fields: &postit_core::CredentialFields,
    ) -> Result<postit_core::Discovered, postit_core::AgentError> {
        self.inner.verify_credentials(ctx, fields).await
    }
}

/// `mock_oauth` under the platform ID `id`, pointed at `provider`, with `edit` applied to its
/// OAuth spec.
pub fn oauth_agent_with(
    provider: &postit_agent_kit::testkit::MockOAuthProvider,
    id: &str,
    edit: impl FnOnce(&mut postit_core::OAuthProviderSpec),
) -> (Arc<dyn Agent>, MockScript) {
    let inner = MockAgent::oauth(&provider.base_url()).with_platform(platform(id));
    let script = inner.script();
    let mut descriptor = inner.descriptor().clone();
    if let postit_core::ConnectMethod::OAuth(spec) = &mut descriptor.connect {
        edit(spec);
    }
    (Arc::new(SpecAgent { inner, descriptor }), script)
}

/// An active OAuth grant of `owner` on `platform_id`, authorized by `remote_user`, holding the
/// access token `access-<refresh>` and the refresh token `refresh`, with one account per entry
/// of `accounts`, each with the account token `acct-token-<remote>`. Returns the grant and its
/// accounts in the order given.
pub async fn oauth_grant(
    pool: &PgPool,
    owner: UserId,
    platform_id: &str,
    remote_user: Option<&str>,
    refresh: &str,
    accounts: &[&str],
) -> (GrantId, Vec<AccountId>) {
    use postit_data::accounts::{AccountRepo, AccountTokenRepo, NewAccount};
    let id = GrantId::from(Uuid::now_v7());
    let credential = vault()
        .seal(
            &json!({"access_token": format!("access-{refresh}"), "refresh_token": refresh}),
            &GrantRepo::seal_context(id),
        )
        .unwrap();
    let scope = OwnerScope::own(owner);
    let mut conn = pool.acquire().await.unwrap();
    GrantRepo::insert(
        &mut conn,
        &scope,
        NewGrant {
            id,
            platform: platform(platform_id),
            kind: GrantKind::Oauth,
            remote_user_id: remote_user.map(str::to_owned),
            credential,
            connection_config: json!({}),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            reconnect_by: None,
            scopes: vec!["read".to_owned(), "write".to_owned()],
        },
    )
    .await
    .unwrap();
    let mut ids = Vec::new();
    for remote in accounts {
        let account = AccountId::from(Uuid::now_v7());
        AccountRepo::insert_or_reactivate(
            &mut conn,
            &scope,
            NewAccount {
                id: account,
                grant_id: id,
                platform: platform(platform_id),
                remote_account_id: (*remote).to_owned(),
                display_name: (*remote).to_owned(),
                avatar_url: None,
                scopes: vec![],
                test_account: false,
            },
        )
        .await
        .unwrap();
        let sealed = vault()
            .seal(
                &json!(format!("acct-token-{remote}")),
                &AccountTokenRepo::seal_context(account),
            )
            .unwrap();
        AccountTokenRepo::upsert(&mut conn, &scope, account, &sealed, None)
            .await
            .unwrap();
        ids.push(account);
    }
    (id, ids)
}

/// The lowercase hex SHA-256 of `token`, as `MockOAuthProvider::revoked` reports it.
pub fn sha256_hex(token: &str) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    sha2::Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// A query parameter of `url`.
pub fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}
