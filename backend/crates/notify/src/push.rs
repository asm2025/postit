//! Web Push: the devices a user registers, and the `push_send` job that delivers one
//! notification to one device.
//!
//! The payload holds the notification's ID, kind, a localized title and body (labels only,
//! never post text or addresses), and a `tag`: the job ID for delivery and job kinds, so one
//! job's pushes replace each other on the device, else the notification's own ID. The same
//! tag, as RFC 8030 `Topic`, lets the push service keep only the latest undelivered message.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use emixcrypto::HashAlgorithm as _;
use postit_core::{
    Clock, IdGenerator, Notice, NotificationId, NotificationKind, PushEndpointId, Severity, UserId,
};
use postit_data::DataError;
use postit_data::notifications::{NotificationRepo, NotificationRow};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::push_endpoints::{
    NewPushEndpoint, PushEndpointRepo, PushEndpointRow, PushPlatform,
};
use postit_http::{
    HttpError, Idempotency, Jitter, PublicClient, RateLimitHeaders, RetryPolicy as HttpRetry,
    RetryRequest, Sensitivity, send_with_retry,
};
use postit_i18n::{Locale, t};
use postit_jobs::{Job, JobContext, JobError, Queue, RetryPolicy};
use postit_vault::{Vault, VaultError};
use reqwest::header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

use crate::ece;
use crate::vapid::VapidKeys;

/// How long a push service keeps an undelivered message: one day.
pub const PUSH_TTL_SECS: u32 = 86_400;
/// The longest endpoint URL accepted.
pub const MAX_ENDPOINT_LEN: usize = 2048;
/// The longest device label accepted, in characters.
pub const MAX_DEVICE_LABEL: usize = 100;

/// `push_send`: one notification to one device. IDs only, like every job payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushSend {
    /// The notification.
    pub notification: Uuid,
    /// The device.
    pub endpoint: Uuid,
}

impl Job for PushSend {
    const JOB_TYPE: &'static str = "push_send";
    const QUEUE: Queue = Queue::Default;
}

/// Five tries, from 30 seconds apart up to 30 minutes, for a busy or failing push service.
#[must_use]
pub fn push_retry_policy() -> RetryPolicy {
    RetryPolicy::Backoff {
        max_attempts: Some(5),
        initial: Duration::from_secs(30),
        max: Duration::from_mins(30),
    }
}

/// The tag a notification's pushes share: the job ID for delivery and job kinds, else the
/// notification's own ID.
#[must_use]
pub fn push_tag(id: NotificationId, notice: &Notice) -> Uuid {
    match notice {
        Notice::DeliveryDeadLettered(n)
        | Notice::DeliveryFailed(n)
        | Notice::DeliveryOutcomeUnknown(n)
        | Notice::DeliveryParked(n)
        | Notice::DeliveryStale(n)
        | Notice::DeliveryPublished(n) => n.job_id.as_uuid(),
        Notice::JobPartial(n) => n.job_id.as_uuid(),
        _ => id.as_uuid(),
    }
}

/// The RFC 8030 `Topic` for `tag`: its 16 bytes in base64url, 22 characters.
#[must_use]
pub fn push_topic(tag: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(tag.as_bytes())
}

#[derive(Serialize)]
struct PushPayload<'a> {
    id: Uuid,
    kind: NotificationKind,
    title: &'a str,
    body: &'a str,
    tag: Uuid,
}

/// The payload's JSON for `row` in `locale`.
fn payload(row: &NotificationRow, locale: Locale) -> Result<Vec<u8>, JobError> {
    let kind = row.notice.kind();
    let args = postit_mail::notice_message_args(&row.notice, locale);
    let title = t(locale, &format!("push-title-{}", kind.as_str()), &args);
    let body = t(locale, &format!("push-body-{}", kind.as_str()), &args);
    serde_json::to_vec(&PushPayload {
        id: row.id.as_uuid(),
        kind,
        title: &title,
        body: &body,
        tag: push_tag(row.id, &row.notice),
    })
    .map_err(|e| JobError::Fatal(format!("push payload: {e}")))
}

/// What a [`PushSender`] needs.
#[derive(Clone)]
pub struct PushSenderDeps {
    /// The database.
    pub pool: PgPool,
    /// Opens the stored endpoint and keys.
    pub vault: Vault,
    /// The deployment's VAPID keys; `None` when push is off, and then every job ends at once.
    pub keys: Option<Arc<VapidKeys>>,
    /// The public client, strict unless `push.allow_private_endpoints`.
    pub client: PublicClient,
    /// Stamps the VAPID token and the success times.
    pub clock: Arc<dyn Clock>,
}

/// Runs `push_send`. Cheap to clone.
#[derive(Clone)]
pub struct PushSender {
    deps: PushSenderDeps,
}

/// A device's opened values.
struct Device {
    url: Url,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

/// One attempt per run: the job runtime spaces the retries.
fn one_attempt() -> HttpRetry {
    HttpRetry {
        max_attempts: 1,
        ..HttpRetry::default()
    }
}

impl PushSender {
    /// A sender over `deps`.
    #[must_use]
    pub fn new(deps: PushSenderDeps) -> Self {
        Self { deps }
    }

    /// Sends the notification to the device. A device or notification that no longer exists
    /// ends the job; so does push being off. `2xx` records the success; `404`/`410` deletes
    /// the device; `429`, `5xx`, and transport failures retry; any other `4xx` is dropped with
    /// a warning. An endpoint the address policy refuses fails for good. Nothing about the
    /// endpoint reaches a log or an error.
    ///
    /// # Errors
    ///
    /// [`JobError::Retry`] for the retryable outcomes and database failures,
    /// [`JobError::Fatal`] for a refused or unreadable endpoint.
    pub async fn handle(&self, job: PushSend, _ctx: JobContext) -> Result<(), JobError> {
        let Some(keys) = self.deps.keys.as_deref() else {
            return Ok(());
        };
        let endpoint_id = PushEndpointId::from(job.endpoint);
        let mut conn = self.deps.pool.acquire().await.map_err(retry)?;
        let Some(row) = PushEndpointRepo::get(&mut conn, endpoint_id)
            .await
            .map_err(retry)?
        else {
            return Ok(());
        };
        let notification = match NotificationRepo::get_for_user(
            &mut conn,
            row.user,
            NotificationId::from(job.notification),
        )
        .await
        {
            Ok(n) => n,
            Err(DataError::NotFound) => return Ok(()),
            Err(e) => return Err(retry(e)),
        };
        let locale = UserPreferencesRepo::get(&mut conn, row.user)
            .await
            .map_err(retry)?
            .and_then(|p| Locale::from_tag(&p.locale))
            .unwrap_or_default();
        // No connection is held across the push.
        drop(conn);

        let device = self.open(&row)?;
        let tag = push_tag(notification.id, &notification.notice);
        let plaintext = payload(&notification, locale)?;
        let body = ece::encrypt(&plaintext, &device.p256dh, &device.auth)
            .map_err(|e| JobError::Fatal(e.to_string()))?;
        let authorization = keys
            .authorization(&device.url, self.deps.clock.now())
            .map_err(|e| JobError::Fatal(e.to_string()))?;
        let urgency = match notification.notice.kind().severity() {
            Severity::Critical => "high",
            Severity::Info => "normal",
        };
        let topic = push_topic(tag);

        let make = || -> Result<reqwest::Request, HttpError> {
            let mut request = reqwest::Request::new(reqwest::Method::POST, device.url.clone());
            *request.body_mut() = Some(body.clone().into());
            let headers = request.headers_mut();
            let mut auth = HeaderValue::from_str(&authorization)
                .map_err(|_| HttpError::Permanent("invalid VAPID header".to_owned()))?;
            auth.set_sensitive(true);
            headers.insert(AUTHORIZATION, auth);
            headers.insert(CONTENT_ENCODING, HeaderValue::from_static("aes128gcm"));
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            headers.insert(
                HeaderName::from_static("ttl"),
                HeaderValue::from(PUSH_TTL_SECS),
            );
            headers.insert(
                HeaderName::from_static("urgency"),
                HeaderValue::from_static(urgency),
            );
            headers.insert(
                HeaderName::from_static("topic"),
                HeaderValue::from_str(&topic)
                    .map_err(|_| HttpError::Permanent("invalid topic".to_owned()))?,
            );
            Ok(request)
        };
        let clock = Arc::clone(&self.deps.clock);
        let policy = one_attempt();
        let reset = RateLimitHeaders(Vec::new());
        let result = send_with_retry(
            RetryRequest {
                client: (&self.deps.client).into(),
                policy: &policy,
                idempotency: Idempotency::Idempotent,
                headers: &reset,
                now: Arc::new(move || clock.now()),
                limiter: None,
                sensitivity: Sensitivity::Opaque,
            },
            &mut Jitter::from_entropy(),
            make,
        )
        .await;
        self.settle(job, endpoint_id, notification.id, result).await
    }

    async fn settle(
        &self,
        job: PushSend,
        endpoint: PushEndpointId,
        notification: NotificationId,
        result: Result<reqwest::Response, HttpError>,
    ) -> Result<(), JobError> {
        match result {
            Ok(response) if response.status().is_success() => {
                let now = self.deps.clock.now();
                let mut tx = self.deps.pool.begin().await.map_err(retry)?;
                PushEndpointRepo::mark_success(&mut tx, endpoint, now)
                    .await
                    .map_err(retry)?;
                NotificationRepo::mark_pushed(&mut tx, notification, now)
                    .await
                    .map_err(retry)?;
                tx.commit().await.map_err(retry)
            }
            Ok(response) => {
                tracing::warn!(
                    endpoint = %job.endpoint,
                    status = response.status().as_u16(),
                    "push service redirected the message; dropped"
                );
                Ok(())
            }
            Err(HttpError::Status { status, .. }) => match status {
                404 | 410 => {
                    let mut conn = self.deps.pool.acquire().await.map_err(retry)?;
                    PushEndpointRepo::delete_by_id(&mut conn, endpoint)
                        .await
                        .map_err(retry)?;
                    tracing::info!(endpoint = %job.endpoint, status, "push endpoint gone; removed");
                    Ok(())
                }
                429 | 500..=599 => Err(JobError::Retry(format!("push service answered {status}"))),
                _ => {
                    tracing::warn!(
                        endpoint = %job.endpoint,
                        status,
                        "push service refused the message; dropped"
                    );
                    Ok(())
                }
            },
            Err(HttpError::RateLimited { .. }) => {
                Err(JobError::Retry("push service rate limited".to_owned()))
            }
            Err(HttpError::Transient(_) | HttpError::OutcomeUnknown(_)) => {
                Err(JobError::Retry("push service unreachable".to_owned()))
            }
            Err(HttpError::AddressRejected { .. }) => Err(JobError::Fatal(
                "push endpoint refused by the outbound address policy".to_owned(),
            )),
            Err(_) => Err(JobError::Fatal("push send failed".to_owned())),
        }
    }

    fn open(&self, row: &PushEndpointRow) -> Result<Device, JobError> {
        let unreadable = |_| JobError::Fatal("stored push endpoint cannot be opened".to_owned());
        let vault = &self.deps.vault;
        let url: String = vault
            .open(
                &row.endpoint,
                &PushEndpointRepo::seal_context(row.id, "endpoint"),
            )
            .map_err(unreadable)?;
        let p256dh: String = vault
            .open(
                &row.p256dh,
                &PushEndpointRepo::seal_context(row.id, "p256dh"),
            )
            .map_err(unreadable)?;
        let auth: String = vault
            .open(&row.auth, &PushEndpointRepo::seal_context(row.id, "auth"))
            .map_err(unreadable)?;
        let bad = || JobError::Fatal("stored push endpoint is malformed".to_owned());
        Ok(Device {
            url: Url::parse(&url).map_err(|_| bad())?,
            p256dh: URL_SAFE_NO_PAD.decode(p256dh).map_err(|_| bad())?,
            auth: URL_SAFE_NO_PAD.decode(auth).map_err(|_| bad())?,
        })
    }
}

/// A device to register, as the browser or app reports it.
#[derive(Clone)]
pub struct NewDevice {
    /// The push endpoint URL, a capability to push to the device.
    pub endpoint: String,
    /// The subscription's public key: a 65-byte uncompressed P-256 point, base64url.
    pub p256dh: String,
    /// The subscription's 16-byte authentication secret, base64url.
    pub auth: String,
    /// The push service.
    pub platform: PushPlatform,
    /// A name for the device.
    pub device_label: String,
}

impl std::fmt::Debug for NewDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewDevice")
            .field("platform", &self.platform)
            .field("device_label", &self.device_label)
            .finish_non_exhaustive()
    }
}

/// Why registering or removing a device failed. No variant carries the endpoint or a key.
#[derive(Debug, thiserror::Error)]
pub enum PushEndpointError {
    /// Push is off in this deployment.
    #[error("push is disabled")]
    Disabled,
    /// A field is malformed: `endpoint`, `p256dh`, `auth`, or `device_label`.
    #[error("invalid push endpoint field: {field}")]
    Invalid {
        /// The field at fault.
        field: &'static str,
    },
    /// The caller has no such device.
    #[error("push endpoint not found")]
    NotFound,
    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// Sealing failed.
    #[error(transparent)]
    Vault(#[from] VaultError),
}

/// What [`PushEndpoints`] needs.
#[derive(Clone)]
pub struct PushEndpointsDeps {
    /// The database.
    pub pool: PgPool,
    /// Seals the endpoint and keys.
    pub vault: Vault,
    /// New device IDs.
    pub ids: Arc<dyn IdGenerator>,
    /// The deployment's VAPID keys; `None` when push is off.
    pub keys: Option<Arc<VapidKeys>>,
    /// `push.allow_private_endpoints`: also accept `http` endpoints, for self-hosted
    /// distributors on a LAN.
    pub allow_private: bool,
}

/// The caller's own devices. Cheap to clone.
#[derive(Clone)]
pub struct PushEndpoints {
    deps: PushEndpointsDeps,
}

fn decode_key(value: &str, len: usize, field: &'static str) -> Result<Vec<u8>, PushEndpointError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .map_err(|_| PushEndpointError::Invalid { field })?;
    if bytes.len() == len {
        Ok(bytes)
    } else {
        Err(PushEndpointError::Invalid { field })
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(bytes)
        // Plain SHA-256 has no key or input limit: it cannot fail.
        .unwrap_or_else(|e| unreachable!("SHA-256 hashes any input: {e}"));
    let mut out = [0u8; 32];
    for (slot, byte) in out.iter_mut().zip(digest.iter()) {
        *slot = *byte;
    }
    out
}

impl PushEndpoints {
    /// The service over `deps`.
    #[must_use]
    pub fn new(deps: PushEndpointsDeps) -> Self {
        Self { deps }
    }

    /// The public key browsers subscribe with; `None` when push is off.
    #[must_use]
    pub fn vapid_public_key(&self) -> Option<String> {
        self.deps.keys.as_ref().map(|k| k.public_key_b64url())
    }

    fn validate_endpoint(&self, endpoint: &str) -> Result<Url, PushEndpointError> {
        let invalid = PushEndpointError::Invalid { field: "endpoint" };
        if endpoint.len() > MAX_ENDPOINT_LEN {
            return Err(invalid);
        }
        let url = Url::parse(endpoint.trim())
            .map_err(|_| PushEndpointError::Invalid { field: "endpoint" })?;
        let scheme_ok = match url.scheme() {
            "https" => true,
            "http" => self.deps.allow_private,
            _ => false,
        };
        if !scheme_ok
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid);
        }
        Ok(url)
    }

    /// Registers one of `user`'s devices, or replaces the keys, platform, and label of the
    /// device already registered with the same endpoint, which keeps its ID.
    ///
    /// # Errors
    ///
    /// [`PushEndpointError::Disabled`] when push is off; [`PushEndpointError::Invalid`] for an
    /// endpoint that is not `https` (or `http` with private endpoints allowed), a `p256dh` that
    /// is not a 65-byte P-256 point, an `auth` that is not 16 bytes, or a label over
    /// [`MAX_DEVICE_LABEL`] characters; [`PushEndpointError::Data`] or
    /// [`PushEndpointError::Vault`] otherwise.
    pub async fn register(
        &self,
        user: UserId,
        device: NewDevice,
    ) -> Result<PushEndpointId, PushEndpointError> {
        if self.deps.keys.is_none() {
            return Err(PushEndpointError::Disabled);
        }
        let url = self.validate_endpoint(&device.endpoint)?;
        let p256dh = decode_key(&device.p256dh, ece::PUBLIC_KEY_LEN, "p256dh")?;
        ece::parse_public_key(&p256dh)
            .map_err(|_| PushEndpointError::Invalid { field: "p256dh" })?;
        let auth = decode_key(&device.auth, ece::AUTH_LEN, "auth")?;
        let label = device.device_label.trim().to_owned();
        if label.chars().count() > MAX_DEVICE_LABEL || label.chars().any(char::is_control) {
            return Err(PushEndpointError::Invalid {
                field: "device_label",
            });
        }
        let endpoint = url.as_str().to_owned();
        let hash = sha256(endpoint.as_bytes());
        let (p256dh, auth) = (URL_SAFE_NO_PAD.encode(p256dh), URL_SAFE_NO_PAD.encode(auth));

        // A concurrent registration of the same device can win between `find_id` and
        // `upsert`; the second round then finds its ID.
        let mut last = None;
        for _ in 0..2 {
            let mut tx = self.deps.pool.begin().await.map_err(DataError::from)?;
            let id = match PushEndpointRepo::find_id(&mut tx, user, &hash).await? {
                Some(id) => id,
                None => PushEndpointId::from(self.deps.ids.generate()),
            };
            let seal = |value: &str, column| {
                self.deps.vault.seal(
                    &value.to_owned(),
                    &PushEndpointRepo::seal_context(id, column),
                )
            };
            let new = NewPushEndpoint {
                id,
                user,
                endpoint: seal(&endpoint, "endpoint")?,
                endpoint_hash: hash,
                p256dh: seal(&p256dh, "p256dh")?,
                auth: seal(&auth, "auth")?,
                platform: device.platform,
                device_label: label.clone(),
            };
            match PushEndpointRepo::upsert(&mut tx, &new).await {
                Ok(id) => {
                    tx.commit().await.map_err(DataError::from)?;
                    return Ok(id);
                }
                Err(err @ DataError::Conflict(_)) => last = Some(err),
                Err(err) => return Err(err.into()),
            }
        }
        Err(last.map_or(PushEndpointError::NotFound, PushEndpointError::from))
    }

    /// Removes one of `user`'s devices. Works with push off, so a user can always clean up.
    ///
    /// # Errors
    ///
    /// [`PushEndpointError::NotFound`] when `user` has no such device (another user's ID
    /// included); [`PushEndpointError::Data`] on a database failure.
    pub async fn delete(&self, user: UserId, id: PushEndpointId) -> Result<(), PushEndpointError> {
        let mut conn = self.deps.pool.acquire().await.map_err(DataError::from)?;
        if PushEndpointRepo::delete(&mut conn, user, id).await? {
            Ok(())
        } else {
            Err(PushEndpointError::NotFound)
        }
    }
}
