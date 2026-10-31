//! The campaign fixture shared by the email campaign test binaries: the services the
//! composition root builds over the compiled-in agents, an email account connected to an
//! in-process SMTP sink, and helpers that create, run, and read campaigns.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use postit_agent_email::testkit::{Received, Script, Sink};
use postit_config::{Environment, Settings};
use postit_contacts::{ContactService, NewContactInput};
use postit_core::{
    AccountId, Clock, ContactId, ContactListId, PlatformId, PostId, PostKind, PostVersion,
    TestClock, UserId,
};
use postit_data::OwnerScope;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::Dispatch;
use postit_data::testkit::seed_user;
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use postit_oauth::AccountService;
use postit_publishing::direct::run_batch;
use postit_publishing::{
    DirectSendBatch, PublishDelivery, PublishInput, PublishingService, run_publish,
};
use postit_server::compose;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub const FROM: &str = "news@bakery.example";
pub const POSTAL: &str = "12 Main St, Springfield";

pub fn config_dir(media: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/default.toml"
    ))
    .unwrap();
    std::fs::write(dir.path().join("default.toml"), base).unwrap();
    let dev = format!(
        r#"[server]
public_url = "http://127.0.0.1"
[database]
url = "postgres://localhost:5432/unused"
[auth.oidc]
issuer = "http://127.0.0.1:1/"
audiences = ["postit"]
[auth.bootstrap]
admin_email = "admin@postit.test"
[mail]
from_address = "noreply@postit.test"
[mail.smtp]
host = "127.0.0.1"
port = 2525
tls = "none"
[app]
public_url = "http://127.0.0.1:44315"
[vault.keys]
1 = "insecure-dev-only-vault-key-00001"
[media]
storage_root = '{media}'
signing_key = "campaign-media-signing-key-0123456789"
[email]
signing_key = "campaign-email-signing-key-0123456789"
allow_private_hosts = true
command_timeout = "5s"
"#,
        media = media.display()
    );
    std::fs::write(dir.path().join("development.toml"), dev).unwrap();
    dir
}

pub fn settings(dir: &tempfile::TempDir) -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "campaign-key"),
    ];
    postit_config::load_with(
        Environment::Development,
        dir.path(),
        vars.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .unwrap()
}

pub fn email() -> PlatformId {
    PlatformId::new("email").unwrap()
}

/// What a [`fixture`] changes from the defaults.
#[derive(Default)]
pub struct Options {
    pub script: Script,
    pub daily_cap: i32,
}

/// An owner with an email account connected to the sink and one contact list, and the
/// services the server builds over the compiled-in agents.
pub struct Fx {
    pub pool: PgPool,
    pub settings: Settings,
    pub sink: Sink,
    pub accounts: AccountService,
    pub contacts: ContactService,
    pub publishing: PublishingService,
    pub media: compose::Media,
    pub jobs: JobQueue,
    pub notifier: postit_notify::Notifier,
    pub owner: UserId,
    pub scope: OwnerScope,
    pub account: AccountId,
    pub list: ContactListId,
    _dir: tempfile::TempDir,
    _media: tempfile::TempDir,
}

pub async fn fixture(pool: &PgPool, options: Options) -> Fx {
    let media_dir = tempfile::tempdir().unwrap();
    let dir = config_dir(media_dir.path());
    let settings = settings(&dir);
    let sink = Sink::start(options.script).await;

    let agents = compose::agent_registry(&settings).unwrap();
    let registered: Vec<(PlatformId, String)> = agents
        .iter()
        .map(|a| {
            let d = a.descriptor();
            (d.platform.clone(), d.display_name.clone())
        })
        .collect();
    PlatformRepo::upsert_registered(pool, &registered)
        .await
        .unwrap();
    let compiled: Vec<PlatformId> = registered.into_iter().map(|(id, _)| id).collect();
    assert!(
        compiled.contains(&email()),
        "the email agent is compiled in"
    );
    let configs = postit_server::platforms::resolve_platform_configs(&settings, &agents).unwrap();
    let ids = compose::system_ids();
    let clock = compose::system_clock();
    let media = compose::media_service(&settings, pool, &ids, &clock).unwrap();
    let http = postit_http::build_client(&settings.http).unwrap();
    let vault = postit_vault::Vault::from_settings(&settings.vault).unwrap();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
    let outbox = MailOutbox::new(jobs.clone());
    let notifier = compose::notifier(&settings, &ids, &clock, &jobs, &outbox);
    let accounts = compose::account_service(
        &settings,
        Environment::Development,
        pool,
        &http,
        &vault,
        Arc::new(postit_oauth::PlatformCatalog::new(agents, configs)),
        (
            Arc::clone(&media.store) as Arc<dyn postit_core::MediaStore>,
            Arc::clone(&media.host) as Arc<dyn postit_core::MediaHost>,
        ),
        &compose::email_links(&settings).unwrap(),
        &notifier,
        &ids,
        &clock,
    )
    .unwrap();
    let contacts = compose::contacts(&settings, pool, &ids, &clock, &jobs).unwrap();
    {
        let mut conn = pool.acquire().await.unwrap();
        AppSettingsRepo::seed_defaults(&mut conn, true)
            .await
            .unwrap();
    }
    let owner = seed_user(pool, "active").await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes) \
         VALUES ($1, $2, 0, 0)",
    )
    .bind(owner.as_uuid())
    .bind(options.daily_cap)
    .execute(pool)
    .await
    .unwrap();
    let scope = OwnerScope::own(owner);
    let account = connect(&accounts, &scope, &sink).await;
    let list = contacts.create_list(&scope, "Customers").await.unwrap().id;
    let publishing = compose::publishing(compose::PublishingParts {
        settings: &settings,
        pool,
        accounts: &accounts,
        media: &media,
        notifier: &notifier,
        jobs: &jobs,
        ids: &ids,
        clock: &clock,
        shutdown: tokio_util::sync::CancellationToken::new(),
        compiled_platforms: compiled,
    });
    Fx {
        pool: pool.clone(),
        settings,
        sink,
        accounts,
        contacts,
        publishing,
        media,
        jobs,
        notifier,
        owner,
        scope,
        account,
        list,
        _dir: dir,
        _media: media_dir,
    }
}

/// The sink's SMTP settings, as a user pastes them.
pub fn credentials(sink: &Sink) -> Map<String, Value> {
    let Value::Object(values) = json!({
        "transport": "smtp",
        "from_name": "Corner Bakery",
        "from_address": FROM,
        "postal_address": POSTAL,
        "host": "127.0.0.1",
        "port": sink.port(),
        "tls": "none",
    }) else {
        unreachable!()
    };
    values
}

/// Connects the sending setup through the check-then-confirm flow.
pub async fn connect(accounts: &AccountService, scope: &OwnerScope, sink: &Sink) -> AccountId {
    let check = accounts
        .start_credentials(scope, &email(), credentials(sink))
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    let confirmed = accounts
        .confirm(scope, connect_id.as_str(), None, AuditContext::default())
        .await
        .unwrap();
    confirmed.connected[0].0.id
}

impl Fx {
    /// The engine over the same accounts and media, with its clock stopped at `now`.
    pub fn publishing_at(&self, now: DateTime<Utc>) -> PublishingService {
        let ids = compose::system_ids();
        let clock: Arc<dyn Clock> = Arc::new(TestClock::new(now));
        compose::publishing(compose::PublishingParts {
            settings: &self.settings,
            pool: &self.pool,
            accounts: &self.accounts,
            media: &self.media,
            notifier: &self.notifier,
            jobs: &self.jobs,
            ids: &ids,
            clock: &clock,
            shutdown: tokio_util::sync::CancellationToken::new(),
            compiled_platforms: vec![email()],
        })
    }

    pub fn signing_key(&self) -> Vec<u8> {
        self.settings
            .email
            .signing_key
            .as_ref()
            .unwrap()
            .expose()
            .as_bytes()
            .to_vec()
    }

    /// A subscribed contact on the list.
    pub async fn contact(&self, address: &str, first_name: Option<&str>) -> ContactId {
        let contact = self
            .contacts
            .create(
                &self.scope,
                NewContactInput {
                    email: address.to_owned(),
                    first_name: first_name.map(str::to_owned),
                    last_name: None,
                    fields: None,
                    tags: None,
                    locale: None,
                    consent_source: "signup form".to_owned(),
                    consent_at: None,
                },
            )
            .await
            .unwrap();
        self.contacts
            .add_member(&self.scope, self.list, &[contact.id])
            .await
            .unwrap();
        contact.id
    }

    pub async fn contact_status(&self, id: ContactId) -> String {
        sqlx::query_scalar("SELECT status FROM contacts WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// Creates and confirms a live campaign to the list through `PublishingService`, then runs
    /// its `publish_delivery`, which fixes the audience and queues the first batch.
    pub async fn campaign(&self) -> Dispatch {
        let post = PostId::from_uuid(Uuid::now_v7());
        let mut tx = self.pool.begin().await.unwrap();
        PostRepo::create(
            &mut tx,
            &self.scope,
            NewPost {
                id: post,
                kind: PostKind::Post,
                reply_to: None,
                campaign_id: None,
                text: "Fresh bread".to_owned(),
                link: None,
                tags: vec![],
                disclose_ai_generated: false,
            },
        )
        .await
        .unwrap();
        let locked = PostRepo::lock(&mut tx, &self.scope, post, None)
            .await
            .unwrap();
        let patch = json!({"email": {
            "subject": "Fresh bread, {{first_name|friend}}",
            "language": "en-US",
            "blocks": [
                {"type": "text", "text": "Hello {{first_name|there}}, come early."},
                {"type": "button", "label": "Order", "url": "https://bakery.example/order"},
            ],
        }});
        PostRepo::upsert_override(
            &mut tx,
            &locked,
            self.account,
            patch,
            json!({"lists": [self.list.as_uuid()]}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let created = self
            .publishing
            .create_job(
                &self.scope,
                post,
                PublishInput {
                    targets: vec![self.account],
                    at: None,
                    native_schedule: false,
                    dry_run: false,
                    confirm: true,
                    if_match: PostVersion(2),
                },
                AuditContext::default(),
            )
            .await
            .unwrap();
        let dispatch = created.deliveries[0];
        run_publish(&self.publishing, PublishDelivery::from(dispatch))
            .await
            .unwrap();
        dispatch
    }

    pub async fn state(&self, dispatch: Dispatch) -> String {
        sqlx::query_scalar("SELECT state FROM deliveries WHERE id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    pub async fn counts(&self, dispatch: Dispatch) -> Value {
        sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// Each row's address (via its contact), state, tries, and error code, by address.
    pub async fn rows(&self, dispatch: Dispatch) -> Vec<(String, String, i16, Option<String>)> {
        sqlx::query_as(
            "SELECT c.email::text, s.state, s.tries, s.error_code FROM direct_sends s \
             JOIN contacts c ON c.id = s.contact_id WHERE s.delivery_id = $1 ORDER BY c.email",
        )
        .bind(dispatch.delivery.as_uuid())
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    /// The cap units the account has used, over every day.
    pub async fn cap_units(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(units_used), 0)::bigint FROM platform_quota_usage              WHERE account_id = $1",
        )
        .bind(self.account.as_uuid())
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    pub async fn account_status(&self) -> String {
        sqlx::query_scalar("SELECT status FROM connected_accounts WHERE id = $1")
            .bind(self.account.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// The run times of the `direct_send_batch` jobs queued for `dispatch`, oldest first.
    pub async fn batch_jobs(&self, dispatch: Dispatch) -> Vec<DateTime<Utc>> {
        let all: Vec<(Value, DateTime<Utc>)> = sqlx::query_as(
            "SELECT payload, run_at FROM jobs WHERE job_type = 'direct_send_batch' \
             ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();
        all.into_iter()
            .filter(|(payload, _)| payload["delivery"] == json!(dispatch.delivery.as_uuid()))
            .map(|(_, at)| at)
            .collect()
    }

    /// Runs batches with `service` until the delivery leaves `processing`, a batch queues no
    /// successor, or the campaign waits for its daily cap. A successor delayed by a soft
    /// failure's backoff runs at once. Returns how many batches ran.
    pub async fn drain_with(&self, service: &PublishingService, dispatch: Dispatch) -> usize {
        let mut ran = 0;
        for _ in 0..20 {
            let before = self.batch_jobs(dispatch).await;
            run_batch(service, DirectSendBatch::from(dispatch))
                .await
                .unwrap();
            ran += 1;
            let after = self.batch_jobs(dispatch).await;
            let resumes: Option<DateTime<Utc>> =
                sqlx::query_scalar("SELECT next_attempt_at FROM deliveries WHERE id = $1")
                    .bind(dispatch.delivery.as_uuid())
                    .fetch_one(&self.pool)
                    .await
                    .unwrap();
            let next = after.get(before.len()).copied();
            let waits_for_cap = next.is_some() && next == resumes;
            if self.state(dispatch).await != "processing" || next.is_none() || waits_for_cap {
                return ran;
            }
        }
        panic!("the campaign never finished");
    }

    pub async fn drain(&self, dispatch: Dispatch) -> usize {
        self.drain_with(&self.publishing, dispatch).await
    }
}

/// The envelope recipients of every accepted mail, sorted.
pub fn recipients(received: &[Received]) -> Vec<String> {
    let mut all: Vec<String> = received.iter().flat_map(|r| r.to.clone()).collect();
    all.sort();
    all
}

/// The mail the sink accepted for `address`.
pub fn mail_to<'a>(received: &'a [Received], address: &str) -> &'a Received {
    received
        .iter()
        .find(|r| r.to.iter().any(|a| a == address))
        .unwrap_or_else(|| panic!("no mail to {address}"))
}

/// Distinctive recipients, so a leak cannot hide among ordinary words.
pub const LEAKY: [&str; 5] = [
    "zelda.q7k@inbox-one.example",
    "yorick.w3m@inbox-one.example",
    "xavia.p9t@inbox-one.example",
    "wendel.h2v@inbox-one.example",
    "vesna.j5r@inbox-one.example",
];

/// Every kind of recipient refusal, each echoing its address: unknown mailbox, full mailbox,
/// policy refusal after the message, and a connection lost in the message.
pub fn echoing() -> Script {
    Script {
        rcpt: vec![
            (LEAKY[0].to_owned(), "550 5.1.1".to_owned()),
            (LEAKY[1].to_owned(), "452 4.2.2".to_owned()),
        ],
        data: vec![(LEAKY[2].to_owned(), "554 5.7.1".to_owned())],
        drop_in_data: vec![LEAKY[3].to_owned()],
        ..Script::default()
    }
}

/// Text the relay's sender refusal carries, standing for whatever a server echoes there.
pub const SENDER_MARKER: &str = "leak-marker-q4z";

/// What must never show: each address, its local part, the recipients' domain, and the sender
/// refusal's text.
pub fn leaky_secrets() -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for address in LEAKY {
        out.insert(address.to_owned());
        out.insert(address.split('@').next().unwrap().to_owned());
    }
    out.insert("inbox-one.example".to_owned());
    out.insert(SENDER_MARKER.to_owned());
    out.into_iter().collect()
}

/// A campaign through every recipient refusal of [`echoing`], run to the end, then a second
/// one whose sender the relay refuses (echoing the sender), which pauses it. Returns both.
pub async fn leaky_campaigns(fx: &Fx) -> (Dispatch, Dispatch) {
    for address in LEAKY {
        fx.contact(address, Some("Pat")).await;
    }
    let first = fx.campaign().await;
    fx.drain(first).await;
    assert_eq!(fx.state(first).await, "published");
    fx.sink
        .script(|script| script.mail_refusal = Some((0, format!("550 5.7.1 {SENDER_MARKER}"))));
    let second = fx.campaign().await;
    fx.drain(second).await;
    assert_eq!(fx.state(second).await, "processing");
    assert_eq!(fx.account_status().await, "needs_reconnect");
    (first, second)
}
