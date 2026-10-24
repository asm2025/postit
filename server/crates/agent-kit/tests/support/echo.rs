//! `EchoAgent`: a test-only agent that talks JSON to an HTTP API through the kit's `http`,
//! `error`, and `upload` modules. It exists to prove an HTTP agent can pass the conformance
//! suite; [`Defects`] break it in one way each so the suite's negative tests have something to
//! catch.

use std::collections::BTreeSet;

use async_trait::async_trait;
use postit_agent_kit::{
    Chunk, Phase, classify, media_format, message, schema, standard, text_format, text_rules,
    upload_chunks,
};
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, Agent, AgentClient, AgentContext, AgentDescriptor,
    AgentError, AgentKind, Capability, ConnectMethod, CredentialCheckContext, CredentialFields,
    Credentials, DeliveryId, Discovered, DiscoveredAccount, FormatId, MediaAsset, MediaAssetState,
    MediaItem, MediaKind, MediaRules, OptionsSchema, PlatformId, PublishMode, PublishOutcome,
    PublishProgress, PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest,
    TextMeasure, UserId, Variant, Violation,
};
use postit_http::{execute_traced, status_error};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use url::Url;
use uuid::Uuid;

/// The platform ID.
pub const PLATFORM: &str = "echo";
/// Upload chunk size.
pub const CHUNK: usize = 64 * 1024;

static ICON: &[u8] = &[0x89, b'P', b'N', b'G'];

/// Ways to break the agent, one conformance check each.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Defects {
    /// Declares `Commenter` but exposes no commenter.
    pub undeclared_commenter: bool,
    /// A dry run creates a post.
    pub dry_run_calls: bool,
    /// Publish errors carry the token.
    pub secret_in_error: bool,
    /// An options label has no English text.
    pub missing_english: bool,
    /// The create call is classified as if nothing had been sent.
    pub retryable_after_send: bool,
    /// Fixture defect, not the agent's: the fault mock is not counted, and the text-only
    /// sample comes first so no counted upload precedes the faulted create.
    pub uncounted_faults: bool,
    /// Fixture defect, not the agent's: `remote_calls` and `mutating_calls` never move.
    pub frozen_counters: bool,
}

fn platform() -> PlatformId {
    PlatformId::new(PLATFORM).expect("a valid platform ID")
}

fn credentials_schema() -> OptionsSchema {
    schema(
        &platform(),
        r#"{"type":"object","properties":{"token":{"type":"string",
            "x-postit-label":{"key":"echo.token.label","en":"API token"},
            "x-postit-secret":true}},"required":["token"],"additionalProperties":false}"#,
    )
    .expect("credentials schema in the subset")
}

fn config_schema() -> OptionsSchema {
    schema(
        &platform(),
        r#"{"type":"object","properties":{"base_url":{"type":"string",
            "x-postit-label":{"key":"echo.base_url.label","en":"API address"}}},
            "required":["base_url"],"additionalProperties":false}"#,
    )
    .expect("config schema in the subset")
}

fn options_schema(label_en: &str) -> OptionsSchema {
    let json = json!({
        "type": "object",
        "properties": {
            "visibility": {
                "type": "string",
                "enum": ["public", "unlisted"],
                "x-postit-enum-labels": [
                    message(&platform(), "visibility.public", "Public"),
                    message(&platform(), "visibility.unlisted", "Unlisted"),
                ],
                "default": "public",
                "x-postit-label": message(&platform(), "visibility.label", label_en),
            }
        },
        "additionalProperties": false
    });
    schema(&platform(), &json.to_string()).expect("options schema in the subset")
}

/// A test agent for a JSON API at the configured `base_url`.
pub struct EchoAgent {
    descriptor: AgentDescriptor,
    config_schema: OptionsSchema,
    defects: Defects,
}

impl EchoAgent {
    /// The agent with the given defects.
    pub fn new(defects: Defects) -> Self {
        let label = if defects.missing_english {
            ""
        } else {
            "Visibility"
        };
        let capabilities = if defects.undeclared_commenter {
            BTreeSet::from([Capability::Commenter])
        } else {
            BTreeSet::new()
        };
        let image = MediaRules {
            min_count: 1,
            max_count: 4,
            kinds: vec![MediaKind::Image],
            mime_types: vec!["image/png".to_owned()],
            ..MediaRules::none()
        };
        let descriptor = AgentDescriptor {
            platform: platform(),
            display_name: "Echo".to_owned(),
            icon_png: ICON,
            brand_color: "#336699".to_owned(),
            kind: AgentKind::Broadcast,
            connect: ConnectMethod::Credentials(credentials_schema()),
            formats: vec![
                text_format(TextMeasure::Graphemes, 500),
                media_format(
                    FormatId::Image,
                    text_rules(TextMeasure::Graphemes, 500, false),
                    image,
                ),
            ],
            capabilities,
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: options_schema(label),
        };
        Self {
            descriptor,
            config_schema: config_schema(),
            defects,
        }
    }
}

fn rule(key: &str, en: &str) -> AgentError {
    AgentError::Validation(vec![Violation::agent_rule(message(&platform(), key, en))])
}

fn base_url(config: &serde_json::Value) -> Result<Url, AgentError> {
    config
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Url::parse(s).ok())
        .ok_or_else(|| rule("config.base_url", "The API address is missing or invalid."))
}

fn endpoint(base: &Url, path: &str) -> Result<Url, AgentError> {
    base.join(path)
        .map_err(|_| rule("config.base_url", "The API address is missing or invalid."))
}

/// Sends `builder` and classifies any failure as of `phase`.
async fn send(
    client: &reqwest::Client,
    builder: reqwest::RequestBuilder,
    phase: Phase,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<reqwest::Response, AgentError> {
    let request = builder
        .build()
        .map_err(|e| classify(e.into(), Phase::BeforeSend, None))?;
    let response = execute_traced(client, request)
        .await
        .map_err(|e| classify(e, phase, None))?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(classify(status_error(response, now).await, phase, None))
    }
}

#[derive(Deserialize)]
struct Me {
    id: String,
    name: String,
    handle: Option<String>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

async fn read_me(response: reqwest::Response) -> Result<Me, AgentError> {
    response.json::<Me>().await.map_err(|_| AgentError::Remote {
        status: None,
        code: None,
        message: "unreadable profile".to_owned(),
    })
}

#[async_trait]
impl Agent for EchoAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> Option<&OptionsSchema> {
        Some(&self.config_schema)
    }

    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(EchoClient {
            base: base_url(&ctx.config)?,
            options: self.descriptor.options_schema.clone(),
            defects: self.defects,
            ctx,
        }))
    }

    async fn verify_credentials(
        &self,
        ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        let invalid = || AgentError::InvalidCredentials {
            field: Some("token".to_owned()),
        };
        let token = fields.get_secret("token").ok_or_else(invalid)?;
        let base = base_url(&ctx.config)?;
        let client = standard(&ctx.http)?;
        let builder = client
            .get(endpoint(&base, "me")?)
            .bearer_auth(token.expose_secret());
        let response = match send(client, builder, Phase::BeforeSend, ctx.clock.now()).await {
            Err(AgentError::Authentication { .. }) => return Err(invalid()),
            other => other?,
        };
        let me = read_me(response).await?;
        Ok(Discovered {
            accounts: vec![DiscoveredAccount {
                remote_id: me.id,
                display_name: me.name,
                handle: me.handle,
                avatar_url: None,
            }],
        })
    }
}

/// An echo client bound to one grant.
pub struct EchoClient {
    base: Url,
    options: OptionsSchema,
    defects: Defects,
    ctx: AgentContext,
}

/// The engine hands an agent media IDs; the in-memory store reads only the ID.
fn stored(item: &MediaItem) -> MediaAsset {
    MediaAsset {
        id: item.asset,
        owner_id: UserId::from_uuid(Uuid::nil()),
        kind: MediaKind::Image,
        mime: "image/png".to_owned(),
        bytes: 0,
        width: None,
        height: None,
        duration_ms: None,
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

fn content_range(chunk: &Chunk) -> String {
    let end = chunk.offset + chunk.bytes.len() as u64 - 1;
    if chunk.last {
        format!("bytes {}-{end}/{}", chunk.offset, end + 1)
    } else {
        format!("bytes {}-{end}/*", chunk.offset)
    }
}

impl EchoClient {
    fn http(&self) -> Result<&reqwest::Client, AgentError> {
        standard(&self.ctx.http)
    }

    async fn token(&self) -> Result<SecretString, AgentError> {
        match self.ctx.credentials.credentials().await? {
            Credentials::Fields(fields) => {
                fields
                    .get_secret("token")
                    .cloned()
                    .ok_or_else(|| AgentError::Authentication {
                        message: "no token stored".to_owned(),
                    })
            }
            Credentials::OAuth(_) => Err(AgentError::Authentication {
                message: "unexpected OAuth grant".to_owned(),
            }),
        }
    }

    async fn me(&self) -> Result<Me, AgentError> {
        let token = self.token().await?;
        let client = self.http()?;
        let builder = client
            .get(endpoint(&self.base, "me")?)
            .bearer_auth(token.expose_secret());
        read_me(send(client, builder, Phase::BeforeSend, self.ctx.clock.now()).await?).await
    }

    async fn upload(
        &self,
        token: &SecretString,
        item: &MediaItem,
        progress: &dyn PublishProgress,
    ) -> Result<String, AgentError> {
        let client = self.http()?;
        let builder = client
            .post(endpoint(&self.base, "uploads")?)
            .bearer_auth(token.expose_secret())
            .json(&json!({ "mime": "image/png" }));
        let created: Created = send(client, builder, Phase::BeforeSend, self.ctx.clock.now())
            .await?
            .json()
            .await
            .map_err(|_| AgentError::Remote {
                status: None,
                code: None,
                message: "unreadable upload".to_owned(),
            })?;
        progress.save_container_ref(&created.id).await?;
        let stream = self
            .ctx
            .media_store
            .open(&stored(item))
            .await
            .map_err(|_| rule("media.unavailable", "The media file could not be read."))?;
        let url = endpoint(&self.base, &format!("uploads/{}", created.id))?;
        upload_chunks(stream, CHUNK, 0, progress, |chunk| {
            let builder = client
                .put(url.clone())
                .bearer_auth(token.expose_secret())
                .header("content-range", content_range(&chunk))
                .body(chunk.bytes);
            async move {
                send(client, builder, Phase::BeforeSend, self.ctx.clock.now())
                    .await
                    .map(drop)
            }
        })
        .await?;
        Ok(created.id)
    }

    async fn create_post(
        &self,
        token: &SecretString,
        variant: &Variant,
        uploads: &[String],
        key: DeliveryId,
    ) -> Result<PublishReceipt, AgentError> {
        let client = self.http()?;
        let builder = client
            .post(endpoint(&self.base, "posts")?)
            .bearer_auth(token.expose_secret())
            .header("idempotency-key", key.to_string())
            .json(&json!({ "text": variant.text, "media": uploads }));
        let phase = if self.defects.retryable_after_send {
            Phase::BeforeSend
        } else {
            Phase::AfterSend
        };
        let created: Created = send(client, builder, phase, self.ctx.clock.now())
            .await?
            .json()
            .await
            // The platform answered success, so the post may exist: never retry blindly.
            .map_err(|_| AgentError::OutcomeUnknown {
                message: "the platform's reply could not be read".to_owned(),
            })?;
        Ok(self.receipt(created.id))
    }

    fn receipt(&self, id: String) -> PublishReceipt {
        PublishReceipt {
            remote_url: endpoint(&self.base, &format!("posts/{id}")).ok(),
            remote_id: Some(id),
            container_ref: None,
            remote_publish_at: None,
        }
    }

    async fn publish_live(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
        token: &SecretString,
    ) -> Result<PublishOutcome, AgentError> {
        let mut uploads = Vec::with_capacity(request.variant.media.len());
        for item in &request.variant.media {
            uploads.push(self.upload(token, item, progress).await?);
        }
        let receipt = self
            .create_post(token, request.variant, &uploads, request.idempotency_key)
            .await?;
        Ok(PublishOutcome::Published(receipt))
    }

    /// The `secret_in_error` defect: the token lands in the error's text.
    fn leak(&self, err: AgentError, token: &SecretString) -> AgentError {
        if !self.defects.secret_in_error {
            return err;
        }
        let with = |message: String| format!("{message} (token {})", token.expose_secret());
        match err {
            AgentError::Authentication { message } => AgentError::Authentication {
                message: with(message),
            },
            AgentError::OutcomeUnknown { message } => AgentError::OutcomeUnknown {
                message: with(message),
            },
            AgentError::Transport { message } => AgentError::Transport {
                message: with(message),
            },
            AgentError::Remote {
                status,
                code,
                message,
            } => AgentError::Remote {
                status,
                code,
                message: with(message),
            },
            other => other,
        }
    }
}

#[async_trait]
impl AgentClient for EchoClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        let me = self.me().await?;
        if me.id != account.remote_id {
            return Err(rule("account.unknown", "The account is not reachable."));
        }
        Ok(AccountInfo {
            remote_id: me.id,
            display_name: me.name,
            handle: me.handle,
            avatar_url: None,
            status: AccountStatus::Active,
        })
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        let me = self.me().await?;
        Ok(Discovered {
            accounts: vec![DiscoveredAccount {
                remote_id: me.id,
                display_name: me.name,
                handle: me.handle,
                avatar_url: None,
            }],
        })
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        Ok(self.options.clone())
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
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        if request.mode == PublishMode::DryRun {
            if self.defects.dry_run_calls {
                let token = self.token().await?;
                let _ = self
                    .create_post(&token, request.variant, &[], request.idempotency_key)
                    .await;
            }
            let mut planned: Vec<String> = request
                .variant
                .media
                .iter()
                .map(|_| "upload one image".to_owned())
                .collect();
            planned.push("create the post".to_owned());
            return Ok(PublishOutcome::Simulated { planned });
        }
        if progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        let token = self.token().await?;
        self.publish_live(request, progress, &token)
            .await
            .map_err(|err| self.leak(err, &token))
    }

    async fn status(
        &self,
        _account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        Ok(PublishOutcome::Published(receipt.clone()))
    }

    async fn reconcile(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        let token = self.token().await?;
        let client = self.http()?;
        let mut url = endpoint(&self.base, "posts")?;
        url.query_pairs_mut()
            .append_pair("key", &request.idempotency_key.to_string());
        let builder = client.get(url).bearer_auth(token.expose_secret());
        match send(client, builder, Phase::BeforeSend, self.ctx.clock.now()).await {
            Ok(response) => {
                let created: Created = response.json().await.map_err(|_| AgentError::Remote {
                    status: None,
                    code: None,
                    message: "unreadable post".to_owned(),
                })?;
                Ok(ReconcileOutcome::Found(self.receipt(created.id)))
            }
            Err(AgentError::Remote {
                status: Some(404), ..
            }) => Ok(ReconcileOutcome::Absent),
            Err(err) => Err(err),
        }
    }
}
