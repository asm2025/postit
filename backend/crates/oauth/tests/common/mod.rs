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
