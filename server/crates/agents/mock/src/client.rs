//! [`MockClient`]: the mock bound to one grant. It implements the required `AgentClient`
//! calls; each optional capability lives in its own module and is exposed through the
//! accessor of the same name.
//!
//! Every call that would reach a real platform goes through `MockPlatform::call` first, so
//! the call counters see exactly what a wiremock server would.

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use postit_agent_kit::message;
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, AgentClient, AgentContext, AgentError, Credentials,
    DeliveryId, Discovered, MediaAsset, OptionsSchema, PlatformId, PublishMode, PublishOutcome,
    PublishProgress, PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest, Variant,
    Violation,
};

use crate::credentials::{self, ACCOUNT_PREFIX, RATE_LIMIT_DELAY, Token};
use crate::descriptor::{MAX_SCHEDULE_LEAD, MIN_SCHEDULE_LEAD, POSTS_BUCKET};
use crate::options::{Outcome, PostOptions};
use crate::platform::{Call, MockPlatform, NewPost};
use crate::script::{Forced, InfoScript, MockScript};

/// The mock agent bound to one grant.
pub struct MockClient {
    pub(crate) platform_id: PlatformId,
    pub(crate) options: OptionsSchema,
    pub(crate) script: MockScript,
    pub(crate) ctx: AgentContext,
}

/// What the mock platform does with one live publish.
enum Reaction {
    /// Refused before acting: nothing is stored, and a repeated key does not matter.
    Refuse(AgentError),
    /// Accepted and stored; `live` says whether it is live yet.
    Store { live: bool },
    /// Stored, then the response is lost.
    StoreThenLose(&'static str),
    /// Lost before the platform acted: nothing is stored.
    Lose(AgentError),
}

impl MockClient {
    fn platform(&self) -> &MockPlatform {
        self.script.platform()
    }

    fn now(&self) -> DateTime<Utc> {
        self.ctx.clock.now()
    }

    async fn token(&self) -> Result<Token, AgentError> {
        Ok(Token::of_grant(&self.ctx.credentials.credentials().await?))
    }

    fn violation(&self, key: &str, en: &str) -> AgentError {
        AgentError::Validation(vec![Violation::agent_rule(message(
            &self.platform_id,
            key,
            en,
        ))])
    }

    /// A forced fault wins over the token, and the token over the `outcome` option.
    fn reaction(&self, token: Token, options: PostOptions, scheduled: bool) -> Reaction {
        let unknown = |message: &str| AgentError::OutcomeUnknown {
            message: message.to_owned(),
        };
        if let Some(forced) = self.script.forced() {
            return match forced {
                Forced::Unauthorized => Reaction::Refuse(credentials::rejected_token()),
                Forced::RateLimited(retry_after) => {
                    Reaction::Refuse(AgentError::RateLimited { retry_after })
                }
                Forced::ServerError => Reaction::Lose(
                    AgentError::Remote {
                        status: Some(500),
                        code: None,
                        message: "the mock platform failed".to_owned(),
                    }
                    .after_send(),
                ),
                Forced::AcceptedThenLost => {
                    Reaction::StoreThenLose("the mock platform's response was lost")
                }
                Forced::LostBeforeAccept => {
                    Reaction::Lose(unknown("the request to the mock platform was lost"))
                }
                Forced::Malformed => {
                    Reaction::StoreThenLose("the mock platform's response could not be read")
                }
            };
        }
        if token.rejected() {
            return Reaction::Refuse(credentials::rejected_token());
        }
        match options.outcome {
            Outcome::Published => Reaction::Store { live: !scheduled },
            Outcome::Processing => Reaction::Store { live: false },
            Outcome::RetryableFailure => Reaction::Refuse(AgentError::Remote {
                status: Some(503),
                code: None,
                message: "the mock platform is unavailable".to_owned(),
            }),
            Outcome::PermanentFailure => Reaction::Refuse(AgentError::Remote {
                status: Some(400),
                code: Some("rejected".to_owned()),
                message: "the mock platform refused the post".to_owned(),
            }),
            Outcome::OutcomeUnknownLanded => {
                Reaction::StoreThenLose("the mock platform's response was lost")
            }
            Outcome::OutcomeUnknownLost => {
                Reaction::Lose(unknown("the request to the mock platform was lost"))
            }
            Outcome::NeedsReconnect => Reaction::Refuse(credentials::rejected_token()),
            Outcome::RateLimited => Reaction::Refuse(AgentError::RateLimited {
                retry_after: Some(RATE_LIMIT_DELAY),
            }),
            Outcome::QuotaExhausted => Reaction::Refuse(AgentError::Quota {
                bucket: POSTS_BUCKET.to_owned(),
                resets_at: next_midnight(self.now()),
            }),
        }
    }

    fn check_schedule(&self, at: Option<DateTime<Utc>>) -> Result<(), AgentError> {
        let Some(at) = at else { return Ok(()) };
        let lead = at - self.now();
        let min = TimeDelta::from_std(MIN_SCHEDULE_LEAD).ok();
        let max = TimeDelta::from_std(MAX_SCHEDULE_LEAD).ok();
        let fits = min.is_some_and(|min| lead >= min) && max.is_some_and(|max| lead <= max);
        if fits {
            Ok(())
        } else {
            Err(self.violation(
                "schedule_outside_window",
                "The mock platform schedules posts 10 minutes to 30 days ahead",
            ))
        }
    }

    /// One planned step per action a live publish would take.
    fn plan(request: &PublishRequest<'_>) -> Vec<String> {
        let media = request.variant.media.len();
        let mut steps: Vec<String> = (1..=media)
            .map(|i| format!("upload media item {i} of {media}"))
            .collect();
        steps.push(match request.native_publish_at {
            Some(at) => format!("schedule the post for {}", at.to_rfc3339()),
            None => "create the post".to_owned(),
        });
        steps
    }

    /// The shared live path for anything the mock stores under an idempotency key. The check
    /// for an earlier answer and the store happen under one lock, so two clients publishing
    /// the same key at once create one post.
    fn deliver(
        &self,
        key: DeliveryId,
        variant: &Variant,
        reaction: Reaction,
        publish_at: Option<DateTime<Utc>>,
        comments: u32,
    ) -> Result<PublishOutcome, AgentError> {
        let reaction = match reaction {
            Reaction::Refuse(err) => return Err(err),
            other => other,
        };
        let mut state = self.platform().lock();
        if let Some(earlier) = state.existing(key) {
            return Ok(earlier);
        }
        let new = |live: bool| NewPost {
            account: &variant.target.remote_id,
            text: &variant.text,
            live: live && publish_at.is_none(),
            publish_at,
            comments,
            now: self.now(),
        };
        match reaction {
            Reaction::Refuse(err) | Reaction::Lose(err) => Err(err),
            Reaction::Store { live } => Ok(state.create_post(key, &new(live))),
            Reaction::StoreThenLose(message) => {
                state.create_post(key, &new(true));
                Err(AgentError::OutcomeUnknown {
                    message: message.to_owned(),
                })
            }
        }
    }
}

/// The next midnight UTC after `now`, when the daily `posts` bucket resets.
fn next_midnight(now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    now.date_naive()
        .succ_opt()
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|midnight| midnight.and_utc())
}

#[async_trait]
impl AgentClient for MockClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        self.platform().call(Call::Read).await;
        self.script.record_seen(&account.remote_id);
        let active = match self.script.account_info() {
            Some(InfoScript::Ok) => true,
            Some(InfoScript::AuthFailed) => false,
            None => !self.token().await?.rejected(),
        };
        if !active {
            return Err(credentials::rejected_token());
        }
        let name = account
            .remote_id
            .strip_prefix(ACCOUNT_PREFIX)
            .unwrap_or(&account.remote_id);
        Ok(AccountInfo {
            remote_id: account.remote_id.clone(),
            display_name: name.to_owned(),
            handle: Some(format!("@{name}")),
            avatar_url: None,
            status: AccountStatus::Active,
        })
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        self.platform().call(Call::Read).await;
        match self.ctx.credentials.credentials().await? {
            Credentials::Fields(fields) => credentials::discover(&fields),
            Credentials::OAuth(_) => Ok(Discovered {
                accounts: vec![credentials::account("oauth")],
            }),
        }
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        Ok(self.options.clone())
    }

    async fn validate(
        &self,
        variant: &Variant,
        _media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        Ok(PostOptions::parse(&self.platform_id, &variant.options)
            .err()
            .unwrap_or_default())
    }

    async fn publish(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        let options = PostOptions::parse(&self.platform_id, &request.variant.options)
            .map_err(AgentError::Validation)?;
        self.check_schedule(request.native_publish_at)?;
        if request.mode == PublishMode::DryRun {
            return Ok(PublishOutcome::Simulated {
                planned: Self::plan(&request),
            });
        }
        if progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        let token = self.token().await?;
        self.platform().call(Call::Mutating).await;
        let reaction = self.reaction(token, options, request.native_publish_at.is_some());
        self.deliver(
            request.idempotency_key,
            request.variant,
            reaction,
            request.native_publish_at,
            options.comments,
        )
    }

    async fn status(
        &self,
        _account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        self.platform().call(Call::Read).await;
        let now = self.now();
        let mut state = self.platform().lock();
        let Some(post) = receipt
            .remote_id
            .as_deref()
            .and_then(|id| state.post_mut(id))
        else {
            return Err(AgentError::Remote {
                status: Some(404),
                code: Some("not_found".to_owned()),
                message: "the mock platform has no such post".to_owned(),
            });
        };
        // A processing post goes live when asked about; a scheduled one once its time passes.
        post.live = match post.publish_at {
            Some(at) => now >= at,
            None => true,
        };
        Ok(crate::platform::outcome(post))
    }

    async fn reconcile(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        self.platform().call(Call::Read).await;
        Ok(self
            .platform()
            .lock()
            .receipt_for(request.idempotency_key)
            .map_or(ReconcileOutcome::Absent, ReconcileOutcome::Found))
    }

    async fn revoke(&self) -> Result<(), AgentError> {
        self.platform().call(Call::Mutating).await;
        if self.script.record_revoke() {
            return Err(AgentError::Remote {
                status: Some(500),
                code: None,
                message: "the mock platform failed to revoke the grant".to_owned(),
            });
        }
        Ok(())
    }
}
